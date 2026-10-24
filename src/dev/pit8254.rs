//! Intel 8254 programmable interval timer, as embedded in the SGI IOC.
//!
//! # Hardware wiring
//!
//! Counter 2 is clocked at 1 MHz from the IOC's divide-by-20 divider, and
//! counter 2's OUT clocks counters 0 and 1. Counters 0 and 1 therefore do not
//! count until the guest programs counter 2 as a rate generator (mode 2) or
//! square-wave generator (mode 3); with counter 2 idle their count is frozen,
//! exactly as on the chip. Terminal counts of counters 0 and 1 drive the IOC
//! timer interrupt sources.
//!
//! # Time base
//!
//! Every channel's deadline is an absolute guest cycle (`hot.cycles`), and a
//! read interpolates `deadline - now` in guest cycles at
//! [`NS_PER_GUEST_CYCLE`] ns per cycle. Nothing here reads the host clock, so
//! reads are deterministic and reproduce exactly with a snapshot.
//!
//! The consequence an OS timing probe must know: this is **not an independent
//! reference**. CP0 Count is derived from the same `hot.cycles` clock (scaled
//! by `count_hz`), so calibrating Count against the PIT measures the ratio the
//! emulator was configured with, not the hardware. Against host wall time the
//! rate is undefined while executing (one guest cycle is one retired
//! instruction) and only tracks wall time while the CPU is parked or stalled
//! in `WAIT`. The only host-referenced clock in the machine is the DS1386 RTC
//! at 10 ms (hundredths of a second) resolution, so the guest has no
//! microsecond wall-clock source.
//!
//! # Resolution and modelled behaviour
//!
//! - One count tick per input clock. At the 1 MHz input that is 1 µs, or 100
//!   guest cycles per tick; a read truncates to whole ticks.
//! - Modes 2 and 3 are modelled as rate generators; modes 0/1/4/5 are armed
//!   and re-armed like mode 2 (documented approximation).
//! - Counting is binary; BCD is recorded and reported in the status byte but
//!   not applied.
//! - A reload of 0 is treated as disarmed, not as 65536.
//! - The counter latch command and the read-back command (count and status)
//!   are implemented. GATE inputs and the OUT pin's electrical waveform are
//!   not; the status OUT bit reports high while a mode 2/3 channel is armed.
//!
//! A frozen count is not stale data: when counter 2 supplies no clock the
//! counter genuinely does not advance. Status bit D6 (null count) stays set
//! until a count reaches the counting element, so a probe can distinguish
//! "not clocked" from "counting" without relying on the value alone.

use std::sync::Arc;
use spin::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use crate::cpu::guest_timer::{GuestTimerCallback, GuestTimers, PIT_CH0, PIT_CH1, PIT_CH2, TimerKey};
use crate::cpu::mips_core::NS_PER_GUEST_CYCLE;
use crate::traits::{BusRead8, BusRead16, BusRead32, BusRead64, BUS_OK, BUS_ERR, Device, Resettable, Saveable};
use crate::snapshot::{get_field, toml_u16, toml_u32, toml_u8, toml_bool, hex_u16, hex_u32, hex_u8};
use std::io::Write;

pub trait TimerCallback: Send + Sync {
    fn callback(&self);
}

/// Which queue key an 8254 channel registers its deadline under.
const PIT_KEYS: [TimerKey; 3] = [PIT_CH0, PIT_CH1, PIT_CH2];

struct Channel {
    // Registers
    count: u16, // Last latched/loaded count (fallback when disarmed)
    reload: u16, // Reload value
    latched_count: Option<u16>,
    /// Status byte captured by a read-back command, returned by the next read
    /// of this channel before any count bytes.
    latched_status: Option<u8>,

    // Configuration
    mode: u8,
    rw_mode: u8, // 0: Latch, 1: LSB, 2: MSB, 3: LSB+MSB
    bcd: bool,

    // Internal State
    rw_state: u8, // 0: First byte, 1: Second byte (for rw_mode 3)
    /// 8254 status bit D6: the last count written has not reached the
    /// counting element yet. Cleared when a channel with a clock is armed.
    null_count: bool,

    // Guest-time tracking: the absolute guest cycle the count next reaches
    // zero, and the period in guest cycles. No host `Instant` — the count is
    // `deadline - hot.cycles` converted to input-clock ticks.
    deadline_cycle: Option<u64>,
    period_cycles: Option<u64>,
    input_freq: u32,
}

impl Channel {
    fn new() -> Self {
        Self {
            count: 0,
            reload: 0,
            latched_count: None,
            latched_status: None,
            mode: 0,
            rw_mode: 1, // Default LSB
            bcd: false,
            rw_state: 0,
            null_count: true,
            deadline_cycle: None,
            period_cycles: None,
            input_freq: 0,
        }
    }
}

/// Fires one 8254 channel's IOC callback at its guest-time deadline and
/// advances the channel's mirrored deadline. The shared queue re-arms the
/// entry itself (`deadline + period`), so this and the queue stay in step.
struct PitChannelCallback {
    chan: Arc<Mutex<Channel>>,
    index: usize,
    callback: Option<Arc<dyn TimerCallback>>,
    debug: Arc<AtomicBool>,
}

impl GuestTimerCallback for PitChannelCallback {
    fn fire(&self) {
        {
            let mut chan = self.chan.lock();
            chan.count = 0;
            if let (Some(d), Some(p)) = (chan.deadline_cycle, chan.period_cycles) {
                chan.deadline_cycle = Some(d.wrapping_add(p));
            }
        }
        if let Some(cb) = &self.callback {
            if self.debug.load(Ordering::Relaxed) && self.index != 2 {
                println!("PIT: Channel {} expired, triggering callback", self.index);
            }
            cb.callback();
        } else if self.debug.load(Ordering::Relaxed) && self.index != 2 {
            println!("PIT: Channel {} expired (no callback)", self.index);
        }
    }
}

#[derive(Clone)]
pub struct Pit8254 {
    channels: [Arc<Mutex<Channel>>; 3],
    callbacks: [Option<Arc<dyn TimerCallback>>; 3],
    guest_timers: Arc<OnceLock<Arc<GuestTimers>>>,
    debug: Arc<AtomicBool>,
    base_frequency: u32,
}

impl Pit8254 {
    pub fn new(base_frequency: u32, cb0: Option<Arc<dyn TimerCallback>>, cb1: Option<Arc<dyn TimerCallback>>, cb2: Option<Arc<dyn TimerCallback>>) -> Self {
        let pit = Self {
            channels: [
                Arc::new(Mutex::new(Channel::new())),
                Arc::new(Mutex::new(Channel::new())),
                Arc::new(Mutex::new(Channel::new())),
            ],
            callbacks: [cb0, cb1, cb2],
            guest_timers: Arc::new(OnceLock::new()),
            debug: Arc::new(AtomicBool::new(false)),
            base_frequency,
        };
        // Channel 2 is driven by the base frequency (1 MHz)
        pit.channels[2].lock().input_freq = base_frequency;
        pit
    }

    /// Wire the shared guest-time timer queue, on which every channel's
    /// deadline is registered as an absolute `hot.cycles` value.
    pub fn set_guest_timers(&self, gt: Arc<GuestTimers>) {
        let _ = self.guest_timers.set(gt);
    }

    fn guest_now(&self) -> u64 {
        self.guest_timers.get().map(|t| t.now()).unwrap_or(0)
    }

    /// Remaining ticks of `chan` at guest time, or its latched `count` when it
    /// is not armed.
    fn remaining_ticks(&self, chan: &Channel) -> u16 {
        if let Some(deadline) = chan.deadline_cycle {
            if chan.input_freq > 0 {
                let remaining = deadline.saturating_sub(self.guest_now());
                let ns_per_tick = 1_000_000_000u64 / chan.input_freq as u64;
                return (remaining.saturating_mul(NS_PER_GUEST_CYCLE) / ns_per_tick) as u16;
            }
        }
        chan.count
    }

    fn arm_channel(&self, idx: usize) {
        let Some(tm) = self.guest_timers.get() else { return; };

        // Disarm any existing timer first
        self.disarm_channel(idx);

        let (deadline, period_cycles);
        {
            let mut chan = self.channels[idx].lock();
            if chan.reload == 0 || chan.input_freq == 0 {
                return;
            }
            let ns_per_tick = 1_000_000_000u64 / chan.input_freq as u64;
            let p = (chan.reload as u64 * ns_per_tick) / NS_PER_GUEST_CYCLE;
            let now = tm.now();
            deadline = now.saturating_add(p);
            period_cycles = p;
            chan.deadline_cycle = Some(deadline);
            chan.period_cycles = Some(period_cycles);
            // The count has now reached the counting element, so status bit
            // D6 clears. A channel with no input clock (counter 2 not a rate
            // generator) never gets here and keeps D6 set, which is what a
            // real 8254 does: no clock edge, no load.
            chan.null_count = false;
        }

        let cb: Arc<dyn GuestTimerCallback> = Arc::new(PitChannelCallback {
            chan: self.channels[idx].clone(),
            index: idx,
            callback: self.callbacks[idx].clone(),
            debug: self.debug.clone(),
        });
        tm.schedule(PIT_KEYS[idx], deadline, period_cycles, Some(cb));
    }

    fn disarm_channel(&self, idx: usize) {
        if let Some(tm) = self.guest_timers.get() {
            tm.cancel(PIT_KEYS[idx]);
        }
        let mut chan = self.channels[idx].lock();
        // Freeze the count where it stopped, so a read after the clock is
        // removed reports the last running value rather than the reload.
        if chan.deadline_cycle.is_some() {
            chan.count = self.remaining_ticks(&chan);
        }
        chan.deadline_cycle = None;
        chan.period_cycles = None;
    }

    fn read_channel(&self, idx: usize) -> u8 {
        let mut chan = self.channels[idx].lock();

        // A latched status byte is returned first, exactly once, then the
        // latched (or live) count follows.
        if let Some(status) = chan.latched_status.take() {
            return status;
        }

        // If latched, read from latched value
        let val = if let Some(latched) = chan.latched_count {
            latched
        } else {
            self.remaining_ticks(&chan)
        };

        match chan.rw_mode {
            1 => { // LSB
                let v = (val & 0xFF) as u8;
                chan.latched_count = None; // a single-byte read releases the latch
                v
            }
            2 => { // MSB
                let v = (val >> 8) as u8;
                chan.latched_count = None;
                v
            }
            3 => { // LSB then MSB
                if chan.rw_state == 0 {
                    chan.rw_state = 1;
                    (val & 0xFF) as u8
                } else {
                    chan.rw_state = 0;
                    chan.latched_count = None; // Clear latch after full read
                    (val >> 8) as u8
                }
            }
            _ => 0,
        }
    }

    /// The 8254 status byte: OUT pin, null count, then the mode control word
    /// exactly as programmed (RW, mode, BCD).
    ///
    /// OUT is modelled coarsely: high while a mode 2/3 channel is armed and
    /// counting, low when it is stopped. The other modes are not modelled
    /// (see the module docs), so their OUT pin is always reported low.
    fn status_byte(&self, chan: &Channel) -> u8 {
        let out = if chan.deadline_cycle.is_some() && matches!(chan.mode, 2 | 3) { 0x80 } else { 0 };
        let null = if chan.null_count { 0x40 } else { 0 };
        out | null | ((chan.rw_mode & 0x3) << 4) | ((chan.mode & 0x7) << 1) | chan.bcd as u8
    }

    /// The 8254 read-back command (control word `11`).
    ///
    /// D5=0 latches the selected counters' count, D4=0 latches their status,
    /// D3/D2/D1 select counters 2/1/0. A latch already pending is left alone:
    /// the first one wins until it has been read, as on the chip.
    fn readback_command(&self, val: u8) {
        let latch_count = (val & 0x20) == 0;
        let latch_status = (val & 0x10) == 0;
        for (idx, channel_arc) in self.channels.iter().enumerate() {
            if (val & (1 << (idx + 1))) == 0 {
                continue;
            }
            let mut chan = channel_arc.lock();
            if latch_count && chan.latched_count.is_none() {
                chan.latched_count = Some(self.remaining_ticks(&chan));
            }
            if latch_status && chan.latched_status.is_none() {
                chan.latched_status = Some(self.status_byte(&chan));
            }
        }
        if self.debug.load(Ordering::Relaxed) {
            println!("PIT: read-back {:02x} (count={} status={})", val, latch_count, latch_status);
        }
    }

    fn write_channel(&self, idx: usize, val: u8) {
        let mut update_chain = false;
        let mut do_arm = false;
        {
            let mut chan = self.channels[idx].lock();

            match chan.rw_mode {
                1 => { // LSB
                    chan.reload = (chan.reload & 0xFF00) | (val as u16);
                    chan.count = chan.reload;
                    chan.null_count = true;
                    do_arm = true;
                }
                2 => { // MSB
                    chan.reload = (chan.reload & 0x00FF) | ((val as u16) << 8);
                    chan.count = chan.reload;
                    chan.null_count = true;
                    do_arm = true;
                }
                3 => { // LSB then MSB
                    if chan.rw_state == 0 {
                        chan.reload = (chan.reload & 0xFF00) | (val as u16);
                        chan.rw_state = 1;
                    } else {
                        chan.reload = (chan.reload & 0x00FF) | ((val as u16) << 8);
                        chan.rw_state = 0;
                        chan.count = chan.reload;
                        // Null count rises when the second byte is written.
                        chan.null_count = true;
                        do_arm = true;
                    }
                }
                _ => {}
            }
            if idx == 2 {
                update_chain = true;
            }
        } // release lock before arm_channel / update_chaining

        if do_arm && !update_chain {
            self.arm_channel(idx);
        }
        if update_chain {
            self.update_chaining();
        }
    }

    fn write_control(&self, val: u8) {
        let sc = (val >> 6) & 0x3;
        let rw = (val >> 4) & 0x3;
        let m = (val >> 1) & 0x7;
        let bcd = (val & 1) != 0;

        if sc == 3 {
            self.readback_command(val);
            return;
        }

        let idx = sc as usize;
        let mut update_chain = false;
        {
            let mut chan = self.channels[idx].lock();

            if rw == 0 {
                // Counter Latch Command: capture the current guest-time count.
                if chan.latched_count.is_none() {
                    chan.latched_count = Some(self.remaining_ticks(&chan));
                }
            } else {
                // Mode/RW setup. Reprogramming releases a pending latch.
                chan.rw_mode = rw;
                chan.mode = m;
                chan.bcd = bcd;
                chan.rw_state = 0;
                chan.latched_count = None;
                chan.latched_status = None;
                chan.null_count = true;
                if idx == 2 {
                    update_chain = true;
                }
            }
        }

        if update_chain {
            self.update_chaining();
        }
    }

    fn update_chaining(&self) {
        // Channel 2 drives Channel 0 and 1.
        // If Ch2 is in Mode 2 (Rate Generator) or Mode 3 (Square Wave), it generates a clock.
        // Frequency = Input Freq / Reload Value.
        let freq = {
            let chan = self.channels[2].lock();
            let mode_periodic = chan.mode == 2 || chan.mode == 3;
            if mode_periodic && chan.reload > 0 && chan.input_freq > 0 {
                chan.input_freq / chan.reload as u32
            } else {
                0
            }
        };

        // Re-arm channel 2 first (its own config may have changed)
        self.arm_channel(2);

        for i in 0..2 {
            let changed = {
                let chan = self.channels[i].lock();
                chan.input_freq != freq
            };
            if changed {
                // Capture the count under the old clock first: changing
                // `input_freq` makes `remaining_ticks` fall back to the stale
                // register value.
                self.disarm_channel(i);
                {
                    let mut chan = self.channels[i].lock();
                    if self.debug.load(Ordering::Relaxed) {
                        println!("PIT: Ch{} input freq updated to {} Hz (driven by Ch2)", i, freq);
                    }
                    chan.input_freq = freq;
                }
                self.arm_channel(i);
            }
        }
    }

    /// Human-readable status for the `pit status` monitor command. Kept
    /// separate from the command so a test can assert on the capability
    /// lines without a writer.
    fn status_lines(&self) -> Vec<String> {
        let mut lines = vec![
            "PIT status (Intel 8254, IOC PIO6; INT2 +0x30 on Indigo2):".to_string(),
            "  reference: guest cycle clock, 10 ns per retired cycle".to_string(),
            "  independent: no - derived from the same guest cycle clock as CP0 Count".to_string(),
            "  host reference: no - guest cycles are retired instructions, not wall time".to_string(),
            format!("  clock: counter 2 input {} Hz; counter 2 OUT clocks counters 0 and 1", self.base_frequency),
            "  caps: counter latch=yes read-back=yes bcd=no modes=2,3".to_string(),
        ];
        for (i, channel_arc) in self.channels.iter().enumerate() {
            let chan = channel_arc.lock();
            lines.push(format!(
                "  channel {}: mode={} rw={} bcd={} reload={:04x} count={:04x} input={}Hz armed={} null={}",
                i, chan.mode, chan.rw_mode, chan.bcd, chan.reload, self.remaining_ticks(&chan),
                chan.input_freq, chan.deadline_cycle.is_some(), chan.null_count
            ));
        }
        lines
    }

    pub fn read(&self, addr: u32) -> BusRead8 {
        let val = match addr {
            0 => self.read_channel(0),
            1 => self.read_channel(1),
            2 => self.read_channel(2),
            _ => return BusRead8::err(),
        };
        if self.debug.load(Ordering::Relaxed) {
            println!("PIT: Read addr {} -> {:02x}", addr, val);
        }
        BusRead8::ok(val)
    }

    pub fn write(&self, addr: u32, val: u8) -> u32 {
        if self.debug.load(Ordering::Relaxed) {
            println!("PIT: Write addr {} val {:02x}", addr, val);
        }
        match addr {
            0 => self.write_channel(0, val),
            1 => self.write_channel(1, val),
            2 => self.write_channel(2, val),
            3 => self.write_control(val),
            _ => return BUS_ERR,
        }
        BUS_OK
    }
}

impl Device for Pit8254 {
    fn step(&self, _cycles: u64) {}

    fn stop(&self) {
        for i in 0..3 {
            self.disarm_channel(i);
        }
    }

    fn start(&self) {
        for i in 0..3 {
            self.arm_channel(i);
        }
    }

    fn is_running(&self) -> bool { self.guest_timers.get().is_some() }
    fn get_clock(&self) -> u64 { 0 }

    fn register_commands(&self) -> Vec<(String, String)> {
        vec![("pit".to_string(), "PIT commands: pit status | pit debug <on|off> [DEV]".to_string())]
    }

    fn execute_command(&self, cmd: &str, args: &[&str], mut writer: Box<dyn Write + Send>) -> Result<(), String> {
        if cmd == "pit" {
            if args.is_empty() {
                return Err("Usage: pit <debug|status> ...".to_string());
            }
            match args[0] {
                "debug" => {
                    let val = match args.get(1).map(|s| *s) {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("Usage: pit debug <on|off>".to_string()),
                    };
                    self.debug.store(val, Ordering::Relaxed);
                    writeln!(writer, "PIT debug {}", if val { "enabled" } else { "disabled" }).unwrap();
                    Ok(())
                }
                "status" => {
                    for line in self.status_lines() {
                        writeln!(writer, "{}", line).unwrap();
                    }
                    Ok(())
                }
                _ => Err("Usage: pit <debug|status> ...".to_string()),
            }
        } else {
            Err("Command not found".to_string())
        }
    }
}

// ============================================================================
// Resettable + Saveable for Pit8254
// ============================================================================

impl Resettable for Pit8254 {
    /// Reset all channel registers to power-on defaults.
    fn power_on(&self) {
        // Disarm all timers before resetting state
        for i in 0..3 {
            self.disarm_channel(i);
        }
        for (i, channel_arc) in self.channels.iter().enumerate() {
            let mut chan = channel_arc.lock();
            chan.count = 0;
            chan.reload = 0;
            chan.latched_count = None;
            chan.latched_status = None;
            chan.mode = 0;
            chan.rw_mode = 1; // default LSB
            chan.bcd = false;
            chan.rw_state = 0;
            chan.null_count = true;
            chan.deadline_cycle = None;
            chan.period_cycles = None;
            // Channel 2 is driven by base frequency; channels 0,1 start with 0 until chaining fires.
            chan.input_freq = if i == 2 { self.base_frequency } else { 0 };
        }
    }
}

fn chan_to_toml(chan: &Channel) -> toml::Value {
    let mut t = toml::map::Map::new();
    t.insert("count".into(),      hex_u16(chan.count));
    t.insert("reload".into(),     hex_u16(chan.reload));
    t.insert("mode".into(),       hex_u8(chan.mode));
    t.insert("rw_mode".into(),    hex_u8(chan.rw_mode));
    t.insert("bcd".into(),        toml::Value::Boolean(chan.bcd));
    t.insert("input_freq".into(), hex_u32(chan.input_freq));
    toml::Value::Table(t)
}

fn chan_from_toml(v: &toml::Value, chan: &mut Channel) {
    if let Some(x) = get_field(v, "count")      { if let Some(n) = toml_u16(x) { chan.count = n; } }
    if let Some(x) = get_field(v, "reload")     { if let Some(n) = toml_u16(x) { chan.reload = n; } }
    if let Some(x) = get_field(v, "mode")       { if let Some(n) = toml_u8(x)  { chan.mode = n; } }
    if let Some(x) = get_field(v, "rw_mode")    { if let Some(n) = toml_u8(x)  { chan.rw_mode = n; } }
    if let Some(x) = get_field(v, "bcd")        { if let Some(b) = toml_bool(x) { chan.bcd = b; } }
    if let Some(x) = get_field(v, "input_freq") { if let Some(n) = toml_u32(x) { chan.input_freq = n; } }
    // Transient guest-time deadlines are re-derived by `start()`/`arm_channel`
    // from the restored register state; they are not restored from the
    // snapshot (the guest cycle base is not snapshot state).
    chan.latched_count = None;
    chan.latched_status = None;
    chan.rw_state = 0;
    chan.null_count = true;
    chan.deadline_cycle = None;
    chan.period_cycles = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::guest_timer::TestClock;
    use std::sync::atomic::{AtomicU32, Ordering as AOrdering};
    use std::sync::Mutex;

    // Serialise all timing-sensitive tests so they don't interfere with each other
    // when the test suite runs with multiple threads.
    static SERIAL: Mutex<()> = Mutex::new(());

    struct CountCallback(Arc<AtomicU32>);
    impl TimerCallback for CountCallback {
        fn callback(&self) { self.0.fetch_add(1, AOrdering::SeqCst); }
    }

    fn make_pit(freq: u32) -> Pit8254 {
        let clock = TestClock::new();
        let gt = Arc::new(GuestTimers::new(clock.ptr()));
        let pit = Pit8254::new(freq, None, None, None);
        pit.set_guest_timers(gt);
        // Keep the clock alive for the life of the test by leaking it: the
        // pointer in `gt` must stay valid, and these tests are short-lived.
        std::mem::forget(clock);
        pit
    }

    /// A PIT with a guest clock the test can advance manually. No host sleep,
    /// no `Instant`: guest time is exactly what the test sets it to.
    fn make_pit_clocked(freq: u32) -> (Pit8254, TestClock, Arc<GuestTimers>) {
        let clock = TestClock::new();
        let gt = Arc::new(GuestTimers::new(clock.ptr()));
        let pit = Pit8254::new(freq, None, None, None);
        pit.set_guest_timers(gt.clone());
        (pit, clock, gt)
    }

    // Program channel `ch` as mode 2 (rate generator), LSB+MSB, with given reload.
    fn program_mode2(pit: &Pit8254, ch: u8, reload: u16) {
        // Control word: SC=ch, RW=3 (LSB+MSB), M=2, BCD=0
        let ctrl = (ch << 6) | (3 << 4) | (2 << 1);
        pit.write(3, ctrl);
        pit.write(ch as u32, (reload & 0xFF) as u8);
        pit.write(ch as u32, (reload >> 8) as u8);
    }

    // Latch and read a 16-bit count from channel `ch`.
    fn latch_read16(pit: &Pit8254, ch: u8) -> u16 {
        // Latch command: SC=ch, RW=0
        let ctrl = (ch << 6) | 0;
        pit.write(3, ctrl);
        read_bytes16(pit, ch)
    }

    // Issue a read-back command for one channel, optionally latching its
    // count and/or status (8254 control word D5/D4 = 0 selects each).
    fn readback(pit: &Pit8254, ch: u8, latch_count: bool, latch_status: bool) {
        let mut cmd = 0xC0 | (1 << (ch + 1));
        if !latch_count {
            cmd |= 0x20;
        }
        if !latch_status {
            cmd |= 0x10;
        }
        pit.write(3, cmd);
    }

    fn read8(pit: &Pit8254, ch: u8) -> u8 {
        let r = pit.read(ch as u32);
        assert!(r.is_ok(), "PIT read of channel {} failed", ch);
        r.data
    }

    fn read_bytes16(pit: &Pit8254, ch: u8) -> u16 {
        let lo = read8(pit, ch);
        let hi = read8(pit, ch);
        (hi as u16) << 8 | lo as u16
    }

    /// Channel 2 at 1 MHz, reload=100: one period = 100 µs = 10 000 guest
    /// cycles. Advance 105 000 cycles (10.5 periods) and the count must be
    /// mid-period, i.e. < reload.
    #[test]
    fn test_ch2_100us_period() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 100);
        clock.set(105_000);
        gt.drain(clock.now());
        let count = latch_read16(&pit, 2);
        pit.stop();
        assert!(count < 100, "count={} should be < reload=100", count);
    }

    /// Mid-period read: 7 ms into a 10 ms period, count is exactly 3000. Guest
    /// time is set explicitly, so this is exact, not "≈".
    #[test]
    fn test_ch2_midperiod_read() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 10_000); // 10 ms = 1 000 000 cycles
        clock.set(700_000);
        gt.drain(clock.now());
        let count = latch_read16(&pit, 2);
        pit.stop();
        assert_eq!(count, 3000, "7 ms into a 10 ms period leaves 3000 ticks");
    }

    /// Callback fires exactly 100 times when 100 periods (100 ms) of guest
    /// time elapse — deterministic, no host jitter window.
    #[test]
    fn test_ch2_callback_rate() {
        let _lock = SERIAL.lock().unwrap();
        let counter = Arc::new(AtomicU32::new(0));
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.stop();
        // Rebuild with the counting callback on channel 2.
        let cb = Arc::new(CountCallback(counter.clone())) as Arc<dyn TimerCallback>;
        let pit2 = Pit8254::new(1_000_000, None, None, Some(cb));
        pit2.set_guest_timers(gt.clone());
        pit2.start();
        program_mode2(&pit2, 2, 1000); // 1 ms = 100 000 cycles
        clock.set(10_000_000); // 100 periods
        gt.drain(clock.now());
        pit2.stop();
        let fires = counter.load(AOrdering::SeqCst);
        assert_eq!(fires, 100, "100 ms of guest time at a 1 ms period");
        drop(pit);
    }

    /// The count decrements monotonically within a period.
    #[test]
    fn test_ch2_count_decrements() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 0xFFFF); // ~65.5 ms = 6 553 500 cycles
        let c0 = latch_read16(&pit, 2);
        clock.set(500_000); // 5 ms
        gt.drain(clock.now());
        let c1 = latch_read16(&pit, 2);
        pit.stop();
        assert_eq!(c0, 0xFFFF);
        assert!(c0 > c1, "count should decrement: c0={} c1={}", c0, c1);
    }

    /// Reload=0xFFFF at 1 MHz: period = 6 553 500 cycles. After 1 200 000
    /// cycles (12 ms) the remaining count is exactly 53 535.
    #[test]
    fn test_ch2_large_reload() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 0xFFFF);
        clock.set(1_200_000);
        gt.drain(clock.now());
        let count = latch_read16(&pit, 2);
        pit.stop();
        assert_eq!(count, 53_535);
    }

    // ------------------------------------------------------------------
    // Channel 0 as a readable reference: the evidence for issue #106.
    // ------------------------------------------------------------------

    /// Counter 0 is readable and its count advances with guest time once
    /// counter 2 clocks it. Half a 10 ms period is exactly 5000 ticks; a full
    /// period reloads. No host time is involved anywhere.
    #[test]
    fn ch0_count_advances_with_guest_cycles() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 1); // counter 2: 1 MHz output (divide by 1)
        program_mode2(&pit, 0, 10_000); // counter 0: 100 Hz = 10 ms

        clock.set(0);
        gt.drain(clock.now());
        assert_eq!(latch_read16(&pit, 0), 10_000, "freshly loaded count");

        clock.set(500_000); // 5 ms = 500 000 guest cycles
        gt.drain(clock.now());
        assert_eq!(latch_read16(&pit, 0), 5_000, "5 ms of guest time is 5000 ticks");

        clock.set(1_000_000); // exactly one period
        gt.drain(clock.now());
        assert_eq!(latch_read16(&pit, 0), 10_000, "a full period reloads");
        pit.stop();
    }

    /// Resolution against the guest cycle clock: one 1 MHz tick is exactly
    /// 100 guest cycles (1 µs / 10 ns). A long run has no cumulative drift
    /// because every deadline is `deadline + period`, never a re-read count.
    #[test]
    fn ch0_rate_is_exact_over_ten_seconds_of_guest_time() {
        let _lock = SERIAL.lock().unwrap();
        let counter = Arc::new(AtomicU32::new(0));
        let cb = Arc::new(CountCallback(counter.clone())) as Arc<dyn TimerCallback>;
        let (_idle, mut clock, gt) = make_pit_clocked(1_000_000);
        let pit = Pit8254::new(1_000_000, Some(cb), None, None);
        pit.set_guest_timers(gt.clone());
        pit.start();
        program_mode2(&pit, 2, 1000); // counter 2: 1 kHz
        program_mode2(&pit, 0, 10); // counter 0: 100 Hz

        clock.set(1_000_000_000); // 10 s at 10 ns per guest cycle
        gt.drain(clock.now());
        pit.stop();
        assert_eq!(counter.load(AOrdering::SeqCst), 1_000, "100 Hz for 10 s of guest time");
    }

    /// A counter with no input clock is genuinely frozen: on its own, counter
    /// 0 holds its loaded count and status bit D6 (null count) stays set.
    /// Programming counter 2 as a rate generator starts it and clears D6.
    ///
    /// This is the hardware trap a timing probe must handle: reading counter
    /// 0 without programming counter 2 first looks exactly like a dead PIT.
    #[test]
    fn ch0_is_frozen_until_counter_2_supplies_a_clock() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 0, 10_000);

        clock.set(500_000);
        gt.drain(clock.now());
        assert_eq!(latch_read16(&pit, 0), 10_000, "no clock, no count");

        readback(&pit, 0, false, true);
        assert_eq!(read8(&pit, 0) & 0x40, 0x40, "null count set while unclocked");

        program_mode2(&pit, 2, 1); // counter 2: 1 MHz output
        clock.set(1_000_000);
        gt.drain(clock.now());
        assert_eq!(latch_read16(&pit, 0), 5_000, "counts once clocked");

        readback(&pit, 0, false, true);
        assert_eq!(read8(&pit, 0) & 0x40, 0, "null count clears once the count loads");
        pit.stop();
    }

    /// The counter latch command captures the count at the instant it is
    /// issued: guest time passing before the bytes are read does not change
    /// what is returned.
    #[test]
    fn counter_latch_holds_its_value_across_guest_time() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 1);
        program_mode2(&pit, 0, 10_000);

        clock.set(0);
        gt.drain(clock.now());
        pit.write(3, 0); // latch counter 0

        clock.set(500_000);
        gt.drain(clock.now());
        assert_eq!(read_bytes16(&pit, 0), 10_000, "latched at t=0");
        assert_eq!(latch_read16(&pit, 0), 5_000, "the live count has moved on");
        pit.stop();
    }

    /// A single-byte (LSB-only) latch is released by its one read; the next
    /// read returns the live count again.
    #[test]
    fn latch_single_byte_read_releases_the_latch() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 1);
        // Counter 0: mode 2, RW=1 (LSB only), reload 0x10 = 16 ticks.
        pit.write(3, (1 << 4) | (2 << 1));
        pit.write(0, 0x10);

        clock.set(0);
        gt.drain(clock.now());
        pit.write(3, 0); // latch counter 0
        assert_eq!(read8(&pit, 0), 0x10, "latched LSB");

        clock.set(80_800); // 50.5 periods of 1600 cycles: 8 ticks left
        gt.drain(clock.now());
        assert_eq!(read8(&pit, 0), 0x08, "the first read released the latch");
        pit.stop();
    }

    /// The read-back command latches count and status together. The status
    /// byte is returned first, then the count as it was when latched, then
    /// live counts again.
    #[test]
    fn readback_latches_count_and_status_status_first() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 1);
        program_mode2(&pit, 0, 10_000);

        readback(&pit, 0, true, true); // latch both at t=0
        clock.set(500_000);
        gt.drain(clock.now());

        // OUT=1, null=0, RW=3, mode=2, BCD=0.
        assert_eq!(read8(&pit, 0), 0b1011_0100, "latched status byte");
        assert_eq!(read_bytes16(&pit, 0), 10_000, "count as latched at t=0");
        assert_eq!(latch_read16(&pit, 0), 5_000, "live count after the latch is drained");
        pit.stop();
    }

    /// A second latch command before the first is read is ignored: the first
    /// captured value wins, as on the 8254.
    #[test]
    fn readback_second_latch_is_ignored_until_read() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 1);
        program_mode2(&pit, 0, 10_000);

        readback(&pit, 0, true, false); // count only, at t=0
        clock.set(500_000);
        gt.drain(clock.now());
        readback(&pit, 0, true, false); // ignored: already latched

        assert_eq!(read_bytes16(&pit, 0), 10_000, "first latch wins");
        pit.stop();
    }

    /// When counter 2 stops being a rate generator, counters 0 and 1 lose
    /// their clock and freeze at the count they had reached, not at their
    /// reload value.
    #[test]
    fn stopping_the_clock_freezes_the_count_where_it_was() {
        let _lock = SERIAL.lock().unwrap();
        let (pit, mut clock, gt) = make_pit_clocked(1_000_000);
        pit.start();
        program_mode2(&pit, 2, 1);
        program_mode2(&pit, 0, 10_000);

        clock.set(500_000);
        gt.drain(clock.now());

        // Counter 2 to mode 0: its OUT no longer clocks counters 0 and 1.
        pit.write(3, (2 << 6) | (3 << 4) | (0 << 1));
        assert_eq!(latch_read16(&pit, 0), 5_000, "frozen at the last running count");

        clock.set(1_000_000);
        gt.drain(clock.now());
        assert_eq!(latch_read16(&pit, 0), 5_000, "still frozen with no clock");
        pit.stop();
    }

    /// `pit status` states the time base and the modelled capabilities, so a
    /// host-side timing probe can see that the PIT is not an independent or
    /// host-referenced clock before trying to calibrate against it.
    #[test]
    fn status_states_reference_and_capabilities() {
        let _lock = SERIAL.lock().unwrap();
        let pit = make_pit(1_000_000);
        let text = pit.status_lines().join("\n");
        assert!(text.contains("reference: guest cycle clock"), "{text}");
        assert!(text.contains("independent: no"), "{text}");
        assert!(text.contains("host reference: no"), "{text}");
        assert!(text.contains("counter latch=yes"), "{text}");
        assert!(text.contains("read-back=yes"), "{text}");
        assert!(text.contains("bcd=no"), "{text}");
    }

    /// The monitor command renders the same lines (and none of the count
    /// fields panic while a channel is being read from another thread).
    #[test]
    fn status_command_renders_capabilities() {
        let _lock = SERIAL.lock().unwrap();
        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }

        let pit = make_pit(1_000_000);
        let out = Buf(Arc::new(Mutex::new(Vec::new())));
        pit.execute_command("pit", &["status"], Box::new(out.clone())).unwrap();
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("PIT status"), "{text}");
        assert!(text.contains("independent: no"), "{text}");
        assert!(text.contains("channel 0:"), "{text}");
    }

    /// Phase 1.7 round-trip: program a few channels with non-default values,
    /// save, load into a fresh PIT, save again, assert the two save_states are
    /// byte-identical. Catches load_state forgetting a channel field.
    #[test]
    fn save_load_round_trip() {
        let _lock = SERIAL.lock().unwrap();
        let src = make_pit(1_000_000);
        program_mode2(&src, 0, 0x1234);
        program_mode2(&src, 1, 0x5678);
        program_mode2(&src, 2, 0xabcd);
        let v1 = src.save_state();

        let dst = make_pit(1_000_000);
        dst.load_state(&v1).expect("load_state");
        let v2 = dst.save_state();

        assert_eq!(v1, v2, "Pit8254 save_state mismatch after load_state round-trip");
    }
}

impl Saveable for Pit8254 {
    fn state_desc(&self) -> Option<crate::state_desc::StateDesc<'_>> {
        use crate::state_desc::{FieldKind, StateDesc};
        let mut d = StateDesc::new("pit", 1);
        for (i, channel_arc) in self.channels.iter().enumerate() {
            let save_arc = channel_arc.clone();
            let load_arc = channel_arc.clone();
            let name = format!("ch{}", i);
            d = d.field(
                name,
                FieldKind::Table,
                1,
                move |t| { t.insert(format!("ch{}", i), chan_to_toml(&save_arc.lock())); },
                move |v| { chan_from_toml(v, &mut load_arc.lock()); Ok(()) },
            );
        }
        Some(d)
    }

    fn save_state(&self) -> toml::Value {
        self.state_desc().expect("pit has a state description").save()
    }

    fn load_state(&self, v: &toml::Value) -> Result<(), String> {
        self.state_desc().expect("pit has a state description").load(v)
    }
}
