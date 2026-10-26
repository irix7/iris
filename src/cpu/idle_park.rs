//! Kernel idle-loop detection and in-place CPU thread parking.
//!
//! Shared by the interpreter run loop (`mips_exec.rs`) and the JIT dispatch
//! loop (`jit/dispatch.rs`). See `clanker/perf/idle-pause-work.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::cpu::mips_core::{CAUSE_IP_MASK, STATUS_IM_MASK};
use crate::cpu::mips_core::MipsCore;

const IDLE_RING: usize = 32;
const SLICE_NS: u64 = 1_000_000;
/// Host-pacing rate for `hot.cycles` while the CPU is parked: one guest cycle
/// per 10 ns of real time, the rate `NS_PER_GUEST_CYCLE` names under
/// `ci_clock`. In every build this keeps cross-thread cycle readers
/// (WD33C93A's deferred-interrupt spin, CP0 Random) moving during a park.
const STALL_NS_PER_CYCLE: u64 = 10;
/// After a park wakes on a pending line the guest cannot take (`(ip & im) ==
/// 0`), do not park again for this many `hot.cycles`. A genuine idle loop
/// wakes for an unmasked interrupt; waking for a masked one means the
/// "repeated state" was a device-poll or DELAY loop that happened to look
/// idle. Sleeping that loop caps it at one batch per slice, and with the
/// wall-clock model the park no longer moves Count to compensate, so boot
/// crawls. About 20 ms of host pacing at the 10 ns/cycle rate.
const PARK_BACKOFF_CYCLES: u64 = 2_000_000;

/// The CPU thread while it is parked in [`IdleParkState::park`], so an
/// interrupt source can wake it at once instead of leaving it to notice on its
/// next slice. There is one CPU thread, so one slot.
static PARKER: parking_lot::Mutex<Option<std::thread::Thread>> = parking_lot::const_mutex(None);
/// Set while the CPU thread is between its last look at the pending word and
/// its sleep. Paired with the writers' order (set the bit, then read this) so
/// a wakeup cannot be lost.
static PARKED: AtomicBool = AtomicBool::new(false);

/// Wake the parked CPU thread, if it is parked. Call after setting a bit in
/// `hot.interrupts` from any thread; one relaxed-cost atomic load when the CPU
/// is running.
#[inline]
pub fn wake() {
    if PARKED.load(Ordering::SeqCst) {
        if let Some(t) = PARKER.lock().as_ref() {
            t.unpark();
        }
    }
}

/// Tracks recent architectural-state hashes to detect polling idle loops.
#[derive(Default)]
pub struct IdleParkState {
    ring: [u64; IDLE_RING],
    ring_len: usize,
    ring_pos: usize,
    /// Do not park again before `hot.cycles` reaches this value; set after a
    /// park that woke on a masked line (see [`PARK_BACKOFF_CYCLES`]).
    park_skip_until_cycle: u64,
}

/// What to do with a repeated (idle-spin) architectural state.
pub enum IdleDecision {
    /// Keep executing: no repeat detected, or an interrupt is ready to
    /// deliver so the guest should run, not sleep.
    None,
    /// The state repeats with interrupts enabled and a mask bit set: park
    /// the CPU thread; an unmasked interrupt ends the park.
    Park,
    /// A masked interrupt is already pending (`ip != 0`), so parking would
    /// wait for a line the guest only takes after it drops the spl — which it
    /// can only do running. Pace instead: sleep in bounded slices, advancing
    /// `hot.cycles` at the 10 ns/cycle host-pacing rate so cross-thread
    /// readers keep moving. Each slice re-checks `running` and the interrupt
    /// word, so a monitor command, a soft reset, or a newly-unmasked
    /// interrupt ends it at once.
    Pace,
}

impl IdleParkState {
    /// Hash PC + GPRs (excluding k0/k1 scratch registers).
    fn hash_state(core: &MipsCore) -> u64 {
        let mut h = core.pc;
        for (i, &g) in core.gpr.iter().enumerate() {
            if i == 26 || i == 27 {
                continue;
            }
            h = h.rotate_left(7) ^ g;
        }
        h
    }

    /// Update the idle ring and decide what to do with the current state.
    ///
    /// Returns `None` unless the architectural state (PC + GPRs, excluding
    /// k0/k1 scratch) repeated across batches with no interrupt ready to
    /// deliver — that is a genuine idle spin. With interrupts enabled and a
    /// mask bit set the spin is safe to `park`; with interrupts masked it is
    /// paced instead (see [`IdleParkState::pace`]).
    pub fn update(&mut self, core: &MipsCore) -> IdleDecision {
        // A recent park woke on a line the guest could not take: keep
        // executing for the back-off window instead of re-parking a loop
        // that is waiting on device progress.
        if core.hot.cycles < self.park_skip_until_cycle {
            return IdleDecision::None;
        }
        let pending = core.hot.interrupts.load(Ordering::Relaxed) as u32;
        let ip = (core.cp0_cause | pending) & CAUSE_IP_MASK;
        let im = core.cp0_status & STATUS_IM_MASK;
        let interrupt_ready = (ip & im) != 0;

        // An interrupt is ready to deliver: run the guest, don't sleep.
        if interrupt_ready {
            self.ring_len = 0;
            self.ring_pos = 0;
            return IdleDecision::None;
        }

        // With every mask bit clear nothing can satisfy `park`'s
        // `(ip & im) != 0`, so parking here never wakes. Do not pace either:
        // an IE=0/IM=0 repeated loop is not an idle guest but a busy wait
        // (the PROM's device-poll and DELAY loops look exactly like this),
        // and sleeping it throttles boot to about one batch per slice.
        if im == 0 {
            self.ring_len = 0;
            self.ring_pos = 0;
            return IdleDecision::None;
        }

        // A pending interrupt that is masked now is still about to be
        // taken: a loop that raises and drops the spl (IRIX's splhi leaves
        // only IP6 unmasked on IP22) is mostly seen in its masked stretch,
        // and takes the line at splx a few instructions on. Parking there
        // would wait for something that only running can bring, so pace
        // instead: bounded sleeps keep the host core free without waiting on
        // a wake condition the guest is about to create.
        if ip != 0 {
            self.ring_len = 0;
            self.ring_pos = 0;
            return IdleDecision::Pace;
        }

        // IE=0: no interrupt can be delivered at all, so a park could never
        // end. Same busy-wait trap as `im == 0` above; keep executing.
        if !core.interrupts_enabled() {
            self.ring_len = 0;
            self.ring_pos = 0;
            return IdleDecision::None;
        }

        let h = Self::hash_state(core);
        if self.ring[..self.ring_len].contains(&h) {
            return IdleDecision::Park;
        }
        self.ring[self.ring_pos] = h;
        self.ring_pos = (self.ring_pos + 1) % IDLE_RING;
        if self.ring_len < IDLE_RING {
            self.ring_len += 1;
        }
        IdleDecision::None
    }

    /// Park in ≤1 ms slices until an interrupt is pending (masked or not)
    /// or the CPU stops.
    ///
    /// The Count==Compare interrupt needs no special handling here: the
    /// compare timer fires on the hptimer thread and ORs IP7 into
    /// `hot.interrupts` exactly like a device line, and CP0 Count itself is
    /// virtual (materialized from the wall clock on read), so nothing has to
    /// advance it during the sleep. Only `hot.cycles` is advanced — at the
    /// 10 ns/guest-cycle host-pacing rate — so cross-thread cycle readers
    /// (Wd33c93a's deferred-interrupt spin-wait, CP0 Random) keep seeing
    /// progress. Under `ci_clock` the compare deadline is a cycles threshold
    /// instead, so the park also stops once it is crossed.
    pub fn park(&mut self, core: &mut MipsCore, running: &AtomicBool) {
        // Only park once the guest has actually armed a Compare deadline.
        // Before that (PROM), Compare use is ad-hoc and there may be nothing
        // armed to wake us. cp0_compare is zero out of reset and the guest
        // must write it to schedule anything, so a non-zero value is the
        // signal that parking is safe.
        if core.cp0_compare == 0 {
            return;
        }

        *PARKER.lock() = Some(std::thread::current());
        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }
            // Announce the park before the last look at the pending word: a
            // writer sets its bit and then reads PARKED, so either we see the
            // bit here or the writer sees PARKED and unparks us.
            PARKED.store(true, Ordering::SeqCst);
            let pending = core.hot.interrupts.load(Ordering::SeqCst) as u32;
            let ip = (core.cp0_cause | pending) & CAUSE_IP_MASK;
            // Any line, masked or not: the mask is the guest's of the moment
            // `update` sampled - the middle of a raised spl, often - and a
            // line it masks there it takes once it drops the spl, which it
            // can only do running. (Waiting for `ip & im` slept through the
            // timer for good in such a loop: an IRIX 6.5 hang.)
            if ip != 0 {
                break;
            }
            // ci_clock has no hptimer — the fire point is a cycles threshold
            // checked in step()'s preamble, so stop parking once we cross it.
            #[cfg(feature = "ci_clock")]
            if core.hot.cycles >= core.count_fire_cycle {
                break;
            }

            let t0 = Instant::now();
            // Still a bounded slice — `running` and the ci_clock threshold are
            // only polled — but an interrupt now ends it at once.
            std::thread::park_timeout(Duration::from_nanos(SLICE_NS));
            let elapsed_ns = t0.elapsed().as_nanos() as u64;
            core.hot.cycles = core.hot.cycles.wrapping_add(elapsed_ns / STALL_NS_PER_CYCLE);
        }
        // If the wake was for a line the guest cannot take (masked), this
        // was not an idle loop; back off so a device-poll or DELAY loop runs
        // at full speed instead of one batch per slice.
        let pending = core.hot.interrupts.load(Ordering::SeqCst) as u32;
        let ip = (core.cp0_cause | pending) & CAUSE_IP_MASK;
        let im = core.cp0_status & STATUS_IM_MASK;
        if ip != 0 && (ip & im) == 0 {
            self.park_skip_until_cycle = core.hot.cycles.saturating_add(PARK_BACKOFF_CYCLES);
        }
        // Every exit, including the ci_clock one, leaves the flag clear: a
        // stale `true` would put `wake` on the mutex for a running CPU.
        PARKED.store(false, Ordering::SeqCst);
    }

    /// Pace a repeated state whose interrupts are masked.
    ///
    /// Unlike [`IdleParkState::park`], this never waits indefinitely: every
    /// slice is bounded, so there is no deadlock risk when nothing can wake
    /// the CPU. `hot.cycles` advances during the sleep at the host-pacing
    /// rate (`STALL_NS_PER_CYCLE`), so cross-thread cycle readers keep seeing
    /// progress and, under `ci_clock`, the Compare threshold still arrives.
    /// The loop ends at once when `running` is cleared (monitor command /
    /// soft reset), when an interrupt becomes deliverable, when the guest
    /// re-enables interrupts (the normal park path takes over next), or —
    /// under `ci_clock` — when the compare deadline is reached.
    pub fn pace(&self, core: &mut MipsCore, running: &AtomicBool) {
        *PARKER.lock() = Some(std::thread::current());
        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }
            // Announce the park before the last look at the pending word
            // (Dekker): a writer sets its bit then reads `PARKED`, so either
            // we see the bit here or the writer sees `PARKED` and unparks us
            // via [`wake`].
            PARKED.store(true, Ordering::SeqCst);
            let ie = core.interrupts_enabled();
            let pending = core.hot.interrupts.load(Ordering::SeqCst) as u32;
            let ip = (core.cp0_cause | pending) & CAUSE_IP_MASK;
            let im = core.cp0_status & STATUS_IM_MASK;
            if (ip & im) != 0 {
                break;
            }
            // The guest re-enabled interrupts: hand back to the run loop so
            // the next `update` can park normally.
            if ie && im != 0 {
                break;
            }
            // ci_clock: stop once the compare deadline is reached, so the run
            // loop can deliver IP7.
            #[cfg(feature = "ci_clock")]
            if core.hot.cycles >= core.count_fire_cycle {
                break;
            }
            let t0 = Instant::now();
            std::thread::park_timeout(Duration::from_nanos(SLICE_NS));
            let elapsed_ns = t0.elapsed().as_nanos() as u64;
            core.hot.cycles = core.hot.cycles.wrapping_add(elapsed_ns / STALL_NS_PER_CYCLE);
        }
        // Every exit leaves the flag clear: a stale `true` would put `wake`
        // on the mutex for a running CPU.
        PARKED.store(false, Ordering::SeqCst);
    }
}

/// Park the CPU thread while the guest is inside a WAIT instruction, until an
/// enabled unmasked interrupt is pending or a soft reset is requested.
///
/// The same Dekker wake as [`IdleParkState::park`]: a writer sets its bit and
/// then reads `PARKED`, so either this loop sees the bit or the writer sees
/// `PARKED` and unparks us via [`wake`]. Unlike [`IdleParkState::park`], this
/// runs *inside* a single instruction's stall (`exec_wait`), not the run loop,
/// so there is no `cp0_compare`-armed guard and no `running` flag to poll: it
/// stays parked until the architectural wake condition (or the soft-reset bit,
/// which is set by `MipsCpu::signal` and also goes through [`wake`]). Only
/// `hot.cycles` advances — at the host-pacing rate — so cross-thread cycle
/// readers (Wd33c93a's deferred-interrupt spin) and, under `ci_clock`, CP0
/// Count keep seeing progress exactly as the old `spin_loop` stall did.
pub fn park_wait(core: &mut MipsCore) {
    const SOFT_RESET_BIT: u64 = 1u64 << 63;
    *PARKER.lock() = Some(std::thread::current());
    loop {
        // Announce the park before the last look at the pending word (Dekker).
        PARKED.store(true, Ordering::SeqCst);
        let pending = core.hot.interrupts.load(Ordering::SeqCst);
        if pending & SOFT_RESET_BIT != 0 {
            break;
        }
        let ip = (core.cp0_cause | (pending as u32)) & CAUSE_IP_MASK;
        let im = core.cp0_status & STATUS_IM_MASK;
        if (ip & im) != 0 {
            break;
        }
        // ci_clock: stop once the compare deadline is reached so the next
        // step delivers it (same rule as `IdleParkState::park`).
        #[cfg(feature = "ci_clock")]
        if core.hot.cycles >= core.count_fire_cycle {
            break;
        }
        let t0 = Instant::now();
        std::thread::park_timeout(Duration::from_nanos(SLICE_NS));
        let elapsed_ns = t0.elapsed().as_nanos() as u64;
        core.hot.cycles = core.hot.cycles.wrapping_add(elapsed_ns / STALL_NS_PER_CYCLE);
    }
    // Every exit leaves the flag clear: a stale `true` would put `wake` on the
    // mutex for a running CPU.
    PARKED.store(false, Ordering::SeqCst);
}

/// Is the CPU thread parked now? (Tests wait for it, not a guessed sleep.)
#[cfg(test)]
pub(crate) fn parked() -> bool {
    PARKED.load(Ordering::SeqCst)
}

/// Tests that park take this first: there is one parked-thread slot
/// (`PARKER`, as there is one CPU thread), so two parking at once would wake
/// each other's threads and miss their own.
#[cfg(test)]
pub(crate) static PARK_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::const_mutex(());

pub fn idle_park_enabled() -> bool {
    std::env::var_os("IRIS_NO_IDLE").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::mips_core::{MipsCore, STATUS_IE, STATUS_IM_SHIFT};

    /// A state the detector would otherwise park on: interrupts enabled, none
    /// ready, and the same architectural state seen twice.
    fn repeated_idle_state(status: u32) -> (IdleParkState, MipsCore) {
        let mut core = MipsCore::default();
        core.cp0_status = status;
        core.cp0_cause = 0;
        core.pc = 0x8000_0100;
        let mut st = IdleParkState::default();
        st.update(&core); // first sighting fills the ring
        (st, core)
    }

    #[test]
    fn update_parks_on_a_repeated_state_when_an_interrupt_could_arrive() {
        // Control for the test below: with a mask bit set, `park`'s
        // `(ip & im) != 0` is satisfiable, so parking is safe and expected.
        let (mut st, core) = repeated_idle_state(STATUS_IE | (1 << (STATUS_IM_SHIFT + 7)));
        assert!(
            matches!(st.update(&core), IdleDecision::Park),
            "a repeated idle state with IM set should park"
        );
    }

    #[test]
    fn update_declines_to_park_while_a_masked_interrupt_is_pending() {
        // IRIX's splhi on IP22 leaves only IP6 (the bus error) unmasked, and
        // a loop that raises and drops the spl is sampled mostly in its
        // masked stretch. With the timer's IP4 already pending there, the
        // guest takes it a few instructions later at splx: nothing to wait
        // for; pace rather than park.
        use crate::cpu::mips_core::CAUSE_IP4;
        let (mut st, core) = repeated_idle_state(STATUS_IE | (1 << (STATUS_IM_SHIFT + 6)));
        core.hot.interrupts.store(CAUSE_IP4 as u64, Ordering::SeqCst);
        assert!(
            matches!(st.update(&core), IdleDecision::Pace),
            "a pending interrupt is about to be taken"
        );
        assert!(
            matches!(st.update(&core), IdleDecision::Pace),
            "... however often the state repeats"
        );
    }

    #[test]
    fn update_declines_to_park_or_pace_when_every_interrupt_is_masked() {
        // IE set but IM zero: the guest would take an interrupt, but none can
        // be delivered, so the wait `park` performs can never end. This is
        // also what a PROM device-poll or DELAY loop looks like; pacing it
        // throttles boot to one batch per slice, so keep executing.
        let (mut st, core) = repeated_idle_state(STATUS_IE);
        assert!(
            matches!(st.update(&core), IdleDecision::None),
            "IM == 0 makes park's wake condition unsatisfiable and pacing unsafe"
        );
    }

    #[test]
    fn update_declines_to_pace_a_repeated_state_with_interrupts_disabled() {
        // IE clear: no interrupt can be taken at all. A repeated IE=0 state
        // is a busy wait, not an idle guest.
        let mut core = MipsCore::default();
        core.cp0_status = 0;
        core.cp0_cause = 0;
        core.pc = 0x8000_0100;
        let mut st = IdleParkState::default();
        st.update(&core);
        assert!(
            matches!(st.update(&core), IdleDecision::None),
            "IE == 0 must not sleep"
        );
    }

    #[test]
    fn update_returns_none_when_an_interrupt_is_ready_to_deliver() {
        // Even a repeated state must not sleep while an unmasked interrupt is
        // pending — the guest should take it.
        let (mut st, mut core) = repeated_idle_state(STATUS_IE | (1 << (STATUS_IM_SHIFT + 7)));
        core.cp0_cause = crate::cpu::mips_core::CAUSE_IP7;
        assert!(
            matches!(st.update(&core), IdleDecision::None),
            "a ready unmasked interrupt means do not sleep"
        );
    }

    /// Pacing returns at once when `running` is cleared (monitor command /
    /// soft reset) — no sleep, no cycle advance.
    #[test]
    fn pace_returns_when_not_running() {
        let mut core = MipsCore::default();
        let running = AtomicBool::new(false);
        let before = core.hot.cycles;
        let st = IdleParkState::default();
        st.pace(&mut core, &running);
        assert_eq!(core.hot.cycles, before, "pace must not sleep when not running");
    }

    /// Pacing returns at once when an interrupt becomes deliverable.
    #[test]
    fn pace_returns_when_an_interrupt_is_deliverable() {
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 7));
        core.cp0_cause = crate::cpu::mips_core::CAUSE_IP7; // unmasked IP7 pending
        let running = AtomicBool::new(true);
        let before = core.hot.cycles;
        let st = IdleParkState::default();
        st.pace(&mut core, &running);
        assert_eq!(
            core.hot.cycles,
            before,
            "pace must not sleep while an interrupt is deliverable"
        );
    }

    /// Pacing advances `hot.cycles` while it sleeps, so cross-thread cycle
    /// readers keep seeing progress.
    #[test]
    fn pace_advances_guest_cycles_while_masked() {
        let _one = PARK_TEST_LOCK.lock();
        let mut core = MipsCore::default();
        core.cp0_status = 0; // interrupts masked: the pace path
        let running = std::sync::Arc::new(AtomicBool::new(true));
        let stop = running.clone();
        let before = core.hot.cycles;
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            stop.store(false, Ordering::SeqCst);
            wake();
        });
        IdleParkState::default().pace(&mut core, &running);
        stopper.join().unwrap();
        assert!(
            core.hot.cycles > before,
            "pace must advance guest cycles while it sleeps"
        );
    }
}

#[cfg(test)]
mod wake_tests {
    use super::*;
    use crate::cpu::mips_core::{MipsCore, CAUSE_IP7, STATUS_IE, STATUS_IM_SHIFT};
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    /// `park` holds `&mut MipsCore`; the interrupt writers reach the same word
    /// through `MipsCpu::interrupts_ptr`, which is what this models.
    struct InterruptsPtr(*const AtomicU64);
    unsafe impl Send for InterruptsPtr {}

    /// Time from an interrupt bit being set on another thread to `park`
    /// returning on the CPU thread. `raise_after` picks where in a slice the
    /// interrupt lands.
    fn delivery_latency(raise_after: Duration) -> Duration {
        let _one = PARK_TEST_LOCK.lock();
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 7));
        core.cp0_compare = 1; // park returns immediately while this is zero
        let ptr = InterruptsPtr(&core.hot.interrupts as *const AtomicU64);
        let running = Arc::new(AtomicBool::new(true));
        let raised = Arc::new(parking_lot::Mutex::new(None::<Instant>));
        let raised_tx = raised.clone();

        let raiser = std::thread::spawn(move || {
            let p = ptr;
            std::thread::sleep(raise_after);
            *raised_tx.lock() = Some(Instant::now());
            unsafe { &*p.0 }.fetch_or(CAUSE_IP7 as u64, Ordering::SeqCst);
            wake();
        });

        let mut st = IdleParkState::default();
        st.park(&mut core, &running);
        let returned = Instant::now();
        raiser.join().unwrap();
        let at = raised.lock().expect("raiser set the bit");
        returned - at
    }

    /// One pass over the slice. Sweeping matters: a single sample can be fast
    /// by luck, on an interrupt that landed just before a slice boundary.
    fn worst_over_a_slice() -> Duration {
        (0..5)
            .map(|i| delivery_latency(Duration::from_micros(2_100 + i * 200)))
            .max()
            .unwrap()
    }

    #[test]
    fn a_masked_interrupt_ends_the_park() {
        // Parked with only IP6 unmasked (the spl raised), the timer's IP4
        // arrives: masked now, but taken as soon as the guest drops the spl,
        // which it can only do running. Before the fix `park` waited for
        // `ip & im` and slept through it for good; the deadline turns that
        // hang into a failure.
        use crate::cpu::mips_core::CAUSE_IP4;
        let _one = PARK_TEST_LOCK.lock();
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 6));
        core.cp0_compare = 1;
        let ptr = InterruptsPtr(&core.hot.interrupts as *const AtomicU64);
        let running = Arc::new(AtomicBool::new(true));
        let stop = running.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let raiser = std::thread::spawn(move || {
            let p = ptr;
            let t0 = Instant::now();
            while !parked() && t0.elapsed() < Duration::from_secs(2) {
                std::thread::yield_now();
            }
            unsafe { &*p.0 }.fetch_or(CAUSE_IP4 as u64, Ordering::SeqCst);
            wake();
            // the deadline: release a park that never returns
            if rx.recv_timeout(Duration::from_secs(2)).is_err() {
                stop.store(false, Ordering::SeqCst);
                wake();
                return false;
            }
            true
        });
        IdleParkState::default().park(&mut core, &running);
        let _ = tx.send(());
        assert!(raiser.join().unwrap(),
            "park slept through a pending interrupt masked only by the spl");
    }

    /// A park that wakes on a masked line was a false positive: back off so
    /// a device-poll or DELAY loop keeps running instead of sleeping a slice
    /// per batch. A park woken by an unmasked line leaves parking enabled.
    #[test]
    fn a_park_woken_by_a_masked_line_backs_off() {
        use crate::cpu::mips_core::CAUSE_IP4;
        let _one = PARK_TEST_LOCK.lock();

        // Masked: only IP6 unmasked, IP4 pending.
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 6));
        core.cp0_compare = 1;
        core.hot.interrupts.store(CAUSE_IP4 as u64, Ordering::SeqCst);
        let running = AtomicBool::new(false); // return at the first check
        let mut st = IdleParkState::default();
        st.park(&mut core, &running);
        assert!(
            matches!(st.update(&core), IdleDecision::None),
            "a masked wake must disable parking for the back-off window"
        );

        // Unmasked: IP7 unmasked and pending.
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 7));
        core.cp0_compare = 1;
        core.hot.interrupts.store(CAUSE_IP7 as u64, Ordering::SeqCst);
        let mut st = IdleParkState::default();
        st.park(&mut core, &running);
        assert!(
            matches!(st.update(&core), IdleDecision::None),
            "a deliverable pending interrupt means run, not park (ring cleared)"
        );
    }

    /// The CP0 Compare deadline is an `hptimer` one-shot in a wall-clock
    /// build. A parked CPU must wake when it fires, without any other thread
    /// raising a line: the callback sets IP7 in `hot.interrupts`, and `park`
    /// exits on any pending line. This is the link #43's queue used to
    /// provide and #98 moved back to the host timer.
    #[cfg(not(feature = "ci_clock"))]
    #[test]
    fn an_armed_compare_timer_ends_the_park() {
        let _one = PARK_TEST_LOCK.lock();
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 7));
        core.set_timer_manager(Arc::new(crate::hptimer::TimerManager::new()));
        // Arm Compare a few Count ticks ahead: about 1 us at 33 MHz. park's
        // own guard requires a non-zero Compare.
        let now = core.count_now();
        core.write_cp0(11, now.wrapping_add(33) as u64);
        let running = AtomicBool::new(true);

        let t0 = Instant::now();
        IdleParkState::default().park(&mut core, &running);
        let elapsed = t0.elapsed();
        assert_ne!(
            core.hot.interrupts.load(Ordering::SeqCst) as u32 & CAUSE_IP7,
            0,
            "the Compare one-shot must have raised IP7"
        );
        assert!(
            elapsed < Duration::from_millis(100),
            "park waited {elapsed:?} for the armed Compare timer"
        );
    }

    /// Under `ci_clock` there is no host timer: the Compare deadline is a
    /// `hot.cycles` threshold, and the park must stop once the advancing cycle
    /// counter crosses it so the run loop can deliver IP7.
    #[cfg(feature = "ci_clock")]
    #[test]
    fn the_cycle_compare_threshold_ends_the_park() {
        let _one = PARK_TEST_LOCK.lock();
        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 7));
        core.cp0_compare = 1; // park's guard requires a non-zero Compare
        core.count_fire_cycle = core.hot.cycles + 300_000; // 3 ms at 10 ns/cycle
        let running = AtomicBool::new(true);

        let t0 = Instant::now();
        IdleParkState::default().park(&mut core, &running);
        let elapsed = t0.elapsed();
        assert!(
            core.hot.cycles >= 300_000,
            "park returned before the cycle threshold: cycles={}",
            core.hot.cycles
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "park waited {elapsed:?} for the cycle threshold"
        );
    }

    #[test]
    fn an_interrupt_ends_the_park_without_waiting_out_the_slice() {
        // Without `wake` every sweep contains a phase that waits out most of a
        // slice, so no number of retries makes this pass; with it, each phase
        // costs a thread wakeup (~6 us here). The retries are only so a
        // scheduling stall on a loaded CI runner does not fail the build.
        const LIMIT: Duration = Duration::from_micros(300);
        let mut seen = Vec::new();
        for _ in 0..3 {
            let worst = worst_over_a_slice();
            if worst < LIMIT {
                return;
            }
            seen.push(worst);
        }
        panic!(
            "worst-phase latency {seen:?} over 3 sweeps, all above {LIMIT:?}; \
             a slice is {:?}",
            Duration::from_nanos(SLICE_NS)
        );
    }
}
