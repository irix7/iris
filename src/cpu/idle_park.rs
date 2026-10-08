//! Kernel idle-loop detection and in-place CPU thread parking.
//!
//! Shared by the interpreter run loop (`mips_exec.rs`) and the JIT dispatch
//! loop (`jit/dispatch.rs`). See `rules/perf/idle-pause-work.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::cpu::mips_core::{CAUSE_IP_MASK, STATUS_IM_MASK};
use crate::cpu::mips_core::MipsCore;

const IDLE_RING: usize = 32;
const SLICE_NS: u64 = 1_000_000;
/// Upper bound on a single park when a guest-time deadline is armed, so a
/// stop request or a lost wake is still noticed promptly. Not a fixed slice:
/// with a timer due sooner, the park ends at the timer.
const MAX_PARK_NS: u64 = 100_000_000;

/// The soonest thing that can end a park: the Compare deadline or the next
/// guest-time timer (8254 PIT) on the shared queue. `u64::MAX` when neither
/// is armed.
fn next_deadline_cycle(core: &MipsCore) -> u64 {
    let queue = core
        .guest_timers
        .as_ref()
        .map(|t| t.next_deadline())
        .unwrap_or(u64::MAX);
    core.count_fire_cycle.min(queue)
}

/// How long to sleep before re-checking: the time to the next guest-time
/// deadline, or a short fallback slice when only external interrupts can wake
/// us. This is what replaces the fixed 1 ms slice.
fn park_timeout_nanos(core: &MipsCore) -> u64 {
    let deadline = next_deadline_cycle(core);
    if deadline == u64::MAX {
        return SLICE_NS;
    }
    let remaining = deadline.saturating_sub(core.hot.cycles);
    remaining
        .saturating_mul(crate::cpu::mips_core::NS_PER_GUEST_CYCLE)
        .min(MAX_PARK_NS)
}

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

    /// Update idle ring. Returns true when the current state repeated (safe to park).
    pub fn update(&mut self, core: &MipsCore) -> bool {
        let ie = core.interrupts_enabled();
        let pending = core.hot.interrupts.load(Ordering::Relaxed) as u32;
        let ip = (core.cp0_cause | pending) & CAUSE_IP_MASK;
        let im = core.cp0_status & STATUS_IM_MASK;
        let interrupt_ready = (ip & im) != 0;

        // With every mask bit clear nothing can satisfy `park`'s
        // `(ip & im) != 0`, so parking here never wakes.
        if im == 0 {
            self.ring_len = 0;
            self.ring_pos = 0;
            return false;
        }

        if !(ie && !interrupt_ready) {
            self.ring_len = 0;
            self.ring_pos = 0;
            return false;
        }

        let h = Self::hash_state(core);
        if self.ring[..self.ring_len].contains(&h) {
            return true;
        }
        self.ring[self.ring_pos] = h;
        self.ring_pos = (self.ring_pos + 1) % IDLE_RING;
        if self.ring_len < IDLE_RING {
            self.ring_len += 1;
        }
        false
    }

    /// Park in ≤1 ms slices until an interrupt is pending or the CPU stops.
    ///
    /// The Count==Compare deadline is a `hot.cycles` threshold, so parking
    /// must stop once the threshold is crossed and let step()'s preamble
    /// deliver IP7. `hot.cycles` is advanced during the sleep — at the
    /// `NS_PER_GUEST_CYCLE` (10 ns) host-pacing rate — so the deadline still
    /// arrives, and cross-thread cycle readers (Wd33c93a's deferred-interrupt
    /// spin-wait, CP0 Random, CP0 Count) keep seeing progress.
    pub fn park(&self, core: &mut MipsCore, running: &AtomicBool) {
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
            let im = core.cp0_status & STATUS_IM_MASK;
            if (ip & im) != 0 {
                break;
            }
            // Stop once the soonest guest deadline — Compare or a queued PIT
            // timer — is reached, so the run loop can deliver it.
            if core.hot.cycles >= next_deadline_cycle(core) {
                break;
            }

            let t0 = Instant::now();
            // Park exactly to the next deadline (capped), rather than a fixed
            // slice; an interrupt still ends it at once via `wake`.
            std::thread::park_timeout(Duration::from_nanos(park_timeout_nanos(core)));
            let elapsed_ns = t0.elapsed().as_nanos() as u64;
            core.hot.cycles = core.hot.cycles.wrapping_add(elapsed_ns / 10);
        }
        // Every exit, including the compare-deadline one, leaves the flag
        // clear: a stale `true` would put `wake` on the mutex for a running
        // CPU.
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
/// `hot.cycles` advances — at the 10 ns/guest-cycle host-pacing rate — so
/// cross-thread cycle readers (Wd33c93a's deferred-interrupt spin) and the
/// virtual CP0 Count keep seeing progress exactly as the old `spin_loop`
/// stall did.
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
        // Stop once the soonest guest deadline is reached so the next step
        // delivers it (same rule as `IdleParkState::park`).
        if core.hot.cycles >= next_deadline_cycle(core) {
            break;
        }
        let t0 = Instant::now();
        std::thread::park_timeout(Duration::from_nanos(park_timeout_nanos(core)));
        let elapsed_ns = t0.elapsed().as_nanos() as u64;
        core.hot.cycles = core.hot.cycles.wrapping_add(elapsed_ns / 10);
    }
    // Every exit leaves the flag clear: a stale `true` would put `wake` on the
    // mutex for a running CPU.
    PARKED.store(false, Ordering::SeqCst);
}

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
        assert!(st.update(&core), "a repeated idle state with IM set should park");
    }

    #[test]
    fn update_declines_to_park_when_every_interrupt_is_masked() {
        // IE set but IM zero: the guest would take an interrupt, but none can
        // be delivered, so the wait `park` performs can never end.
        let (mut st, core) = repeated_idle_state(STATUS_IE);
        assert!(!st.update(&core), "IM == 0 makes park's wake condition unsatisfiable");
    }

    /// With no armed guest deadline the park falls back to the short slice
    /// (only an external interrupt can end it).
    #[test]
    fn park_timeout_is_a_slice_when_nothing_is_armed() {
        let core = MipsCore::default();
        assert_eq!(park_timeout_nanos(&core), SLICE_NS);
    }

    /// The park timeout is derived from the next guest-time deadline, not a
    /// fixed slice: a timer 300 us of guest time away parks for 300 us.
    #[test]
    fn park_waits_to_the_next_guest_deadline() {
        use crate::cpu::guest_timer::{GuestTimers, PIT_CH0};
        use crate::cpu::mips_core::CyclesPtr;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let mut core = MipsCore::default();
        core.cp0_status = STATUS_IE | (1 << (STATUS_IM_SHIFT + 7));
        core.cp0_compare = 1; // park returns immediately while this is zero
        // Wire the queue's clock at the core's own cycle counter, exactly as
        // production does, so `park` advancing `hot.cycles` also advances it.
        let timers = Arc::new(GuestTimers::new(CyclesPtr::new(
            &core.hot.cycles as *const u64,
        )));
        // A PIT channel due in 30 000 guest cycles == 300 us at 10 ns/cycle.
        timers.schedule(PIT_CH0, 30_000, 0, None);
        core.guest_timers = Some(timers);
        core.count_fire_cycle = 1_000_000; // Compare far in the future

        assert_eq!(
            park_timeout_nanos(&core),
            300_000,
            "park must target the 300 us queue deadline, not the fixed slice"
        );

        let running = AtomicBool::new(true);
        let st = IdleParkState::default();
        st.park(&mut core, &running);
        assert!(
            core.hot.cycles >= 30_000,
            "park returned before the guest deadline: cycles={}",
            core.hot.cycles
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

        let st = IdleParkState::default();
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
