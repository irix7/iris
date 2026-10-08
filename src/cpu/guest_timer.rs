//! One ordered, serialisable queue of guest-time (cycle) deadlines.
//!
//! Before this, timer delivery was split and host-anchored: the CP0 Compare
//! deadline was a `hot.cycles` threshold checked by the interpreter's
//! preamble, the 8254 PIT armed recurring `hptimer` timers on a background
//! thread and interpolated its latched count from a `std::time::Instant`, and
//! the idle-park loop re-checked `hot.cycles` on fixed 1 ms slices. This module
//! gives all of them one ordered set of absolute guest-cycle deadlines so the
//! run loop can park to the *next* deadline instead of a slice, and so the PIT
//! reads a count-down derived from guest time rather than wall time.
//!
//! The design follows MAME's `device_scheduler` timer list and ares'
//! `priority-queue` (see `docs/research/b06-timers-interrupts-determinism.md`):
//!
//! - **Ordered.** Entries sort by `(deadline, seq)`. `seq` is a monotonically
//!   increasing insertion counter, so two timers armed for the same cycle fire
//!   in a stable, reproducible order rather than by whichever happened to be
//!   compared first.
//! - **Serialisable.** [`GuestTimerQueue::to_toml`]/[`from_toml`] round-trip
//!   the deadlines; `from_toml` re-sorts (MAME's `postload`, the "re-sort on
//!   load" rule) rather than trusting the saved order.
//! - **Named, not anonymous.** Every entry has a [`TimerKey`]. Callbacks are
//!   held in a separate map and are deliberately *not* serialised: a transient
//!   closure with no stable owner cannot be restored, so it is banned from
//!   snapshot state. Owners (the 8254 PIT, the Compare deadline) re-register
//!   their named deadlines from their own saved state on load.
//!
//! Deadlines are absolute `hot.cycles` values, the same time base CP0 Count is
//! derived from (`NS_PER_GUEST_CYCLE` ns per retired cycle). Nothing here reads
//! the host clock; host time is kept only to pace parking to wall clock.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use crate::cpu::mips_core::CyclesPtr;
use crate::snapshot::{get_field, hex_u64, toml_u64};

/// Stable identity of a queue entry. Also used as the callback key, so two
/// timers cannot share an owner without one cancelling the other.
pub type TimerKey = u64;

/// 8254 PIT channel 0 (system clock).
pub const PIT_CH0: TimerKey = 1;
/// 8254 PIT channel 1 (profiling clock).
pub const PIT_CH1: TimerKey = 2;
/// 8254 PIT channel 2 (chain / rate generator).
pub const PIT_CH2: TimerKey = 3;
/// CP0 Count==Compare deadline.
pub const COMPARE: TimerKey = 4;

/// A callback invoked when a queued deadline is reached. `Send + Sync` so the
/// queue can live behind an `Arc` shared with device threads.
pub trait GuestTimerCallback: Send + Sync {
    fn fire(&self);
}

/// One absolute guest-cycle deadline.
///
/// `period == 0` is a one-shot; a non-zero period re-arms the entry by
/// `deadline + period` each time it fires (single-step; [`GuestTimers::drain`]
/// loops so a whole period's worth of missed ticks are delivered in order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerEntry {
    pub key: TimerKey,
    pub deadline: u64,
    pub period: u64,
    /// Stable tie-break for equal deadlines; assigned in insertion order.
    pub seq: u64,
}

/// The ordered, serialisable set of deadlines. No callbacks, no clock, no
/// locks — this is the part that can be snapshotted.
#[derive(Default, Clone)]
pub struct GuestTimerQueue {
    entries: Vec<TimerEntry>,
    next_seq: u64,
}

impl GuestTimerQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep `entries` ordered by `(deadline, seq)`.
    fn sort(&mut self) {
        self.entries
            .sort_by(|a, b| a.deadline.cmp(&b.deadline).then(a.seq.cmp(&b.seq)));
    }

    /// Arm (or re-arm) `key` at `deadline`, replacing any previous entry for
    /// the same key. Returns the insertion sequence used for tie-breaking.
    pub fn schedule(&mut self, key: TimerKey, deadline: u64, period: u64) -> u64 {
        self.entries.retain(|e| e.key != key);
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.entries.push(TimerEntry { key, deadline, period, seq });
        self.sort();
        seq
    }

    /// Remove `key`, returning true if it was present.
    pub fn cancel(&mut self, key: TimerKey) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        self.entries.len() != before
    }

    /// The soonest deadline, or `None` when nothing is armed.
    pub fn next_deadline(&self) -> Option<u64> {
        self.entries.first().map(|e| e.deadline)
    }

    /// Remove and return the earliest entry due at or before `now`.
    ///
    /// Only the first entry needs checking: the vector is sorted, so if the
    /// soonest is not yet due nothing is.
    pub fn pop_due(&mut self, now: u64) -> Option<TimerEntry> {
        if self.entries.first().map_or(false, |e| e.deadline <= now) {
            Some(self.entries.remove(0))
        } else {
            None
        }
    }

    /// Re-insert a fired periodic entry at `deadline + period`, keeping its
    /// original `seq` so it stays ordered against same-deadline peers. Any
    /// entry the owner scheduled for the same key while the callback ran wins.
    fn repeat(&mut self, entry: TimerEntry) {
        if self.entries.iter().any(|e| e.key == entry.key) {
            return;
        }
        let next = TimerEntry {
            deadline: entry.deadline.saturating_add(entry.period),
            ..entry
        };
        self.entries.push(next);
        self.sort();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Serialise every entry. Callbacks are not included (see module docs).
    pub fn to_toml(&self) -> toml::Value {
        let arr: Vec<toml::Value> = self
            .entries
            .iter()
            .map(|e| {
                let mut t = toml::map::Map::new();
                t.insert("key".into(), hex_u64(e.key));
                t.insert("deadline".into(), hex_u64(e.deadline));
                t.insert("period".into(), hex_u64(e.period));
                t.insert("seq".into(), hex_u64(e.seq));
                toml::Value::Table(t)
            })
            .collect();
        toml::Value::Array(arr)
    }

    /// Deserialise and **re-sort** (never trust saved order). `next_seq` is
    /// advanced past every restored entry so later inserts still break ties
    /// after the restored ones.
    pub fn from_toml(v: &toml::Value) -> Self {
        let mut q = GuestTimerQueue::default();
        if let Some(arr) = v.as_array() {
            for item in arr {
                let get = |k: &str| get_field(item, k).and_then(toml_u64).unwrap_or(0);
                let e = TimerEntry {
                    key: get("key"),
                    deadline: get("deadline"),
                    period: get("period"),
                    seq: get("seq"),
                };
                if e.seq >= q.next_seq {
                    q.next_seq = e.seq.wrapping_add(1);
                }
                q.entries.push(e);
            }
        }
        q.sort();
        q
    }
}

/// Runtime owner of the queue: callbacks, the guest clock, and a cached
/// earliest-deadline word cheap enough for the run loop and idle park to poll.
struct Inner {
    queue: GuestTimerQueue,
    callbacks: HashMap<TimerKey, Arc<dyn GuestTimerCallback>>,
}

/// Shared handle to the one guest-time timer queue. Devices (the 8254 PIT) and
/// the CPU (the Compare deadline) hold the same `Arc`.
pub struct GuestTimers {
    inner: Mutex<Inner>,
    /// Cached `queue.next_deadline()`, `u64::MAX` when empty. Read without the
    /// lock by the run loop and idle park.
    next: AtomicU64,
    /// Current guest cycle, wired to the CPU's `Hot::cycles` once the executor
    /// has reached its final address. Dangling (`0`) until then.
    clock: OnceLock<CyclesPtr>,
}

impl GuestTimers {
    pub fn new(clock: CyclesPtr) -> Self {
        let clock_slot = OnceLock::new();
        let _ = clock_slot.set(clock);
        Self {
            inner: Mutex::new(Inner {
                queue: GuestTimerQueue::new(),
                callbacks: HashMap::new(),
            }),
            next: AtomicU64::new(u64::MAX),
            clock: clock_slot,
        }
    }

    /// Wire the live guest clock. Called once, after the executor is inside
    /// its `MipsCpu` and the cycle field has a stable address.
    pub fn set_clock(&self, clock: CyclesPtr) {
        let _ = self.clock.set(clock);
    }

    /// Current guest cycle (`hot.cycles`), or 0 before the clock is wired.
    pub fn now(&self) -> u64 {
        self.clock.get().map(|c| c.get()).unwrap_or(0)
    }

    /// Arm `key` at an absolute guest-cycle `deadline`, optionally re-arming
    /// every `period` cycles, with `cb` invoked on every fire.
    pub fn schedule(
        &self,
        key: TimerKey,
        deadline: u64,
        period: u64,
        cb: Option<Arc<dyn GuestTimerCallback>>,
    ) {
        let next = {
            let mut inner = self.inner.lock();
            inner.queue.schedule(key, deadline, period);
            if let Some(cb) = cb {
                inner.callbacks.insert(key, cb);
            }
            inner.queue.next_deadline().unwrap_or(u64::MAX)
        };
        self.next.store(next, Ordering::Relaxed);
    }

    /// Cancel `key` and forget its callback.
    pub fn cancel(&self, key: TimerKey) {
        let next = {
            let mut inner = self.inner.lock();
            inner.queue.cancel(key);
            inner.callbacks.remove(&key);
            inner.queue.next_deadline().unwrap_or(u64::MAX)
        };
        self.next.store(next, Ordering::Relaxed);
    }

    /// Cached soonest deadline, `u64::MAX` if nothing is armed.
    pub fn next_deadline(&self) -> u64 {
        self.next.load(Ordering::Relaxed)
    }

    /// Fire every deadline at or before `now`, in queue order.
    ///
    /// Periodic entries are re-armed by `deadline + period` and the loop keeps
    /// draining, so a stretch longer than one period delivers every missed
    /// tick rather than collapsing them into one. Callbacks run with the queue
    /// lock released, so a callback may schedule/cancel freely.
    pub fn drain(&self, now: u64) {
        loop {
            let popped = {
                let mut inner = self.inner.lock();
                match inner.queue.pop_due(now) {
                    Some(entry) => {
                        let cb = inner.callbacks.get(&entry.key).cloned();
                        if entry.period != 0 {
                            inner.queue.repeat(entry);
                        }
                        Some((entry, cb))
                    }
                    None => None,
                }
            };
            let Some((_entry, cb)) = popped else { break };
            if let Some(cb) = cb {
                cb.fire();
            }
        }
        let next = self.inner.lock().queue.next_deadline().unwrap_or(u64::MAX);
        self.next.store(next, Ordering::Relaxed);
    }

    /// Replace the queue from serialised state (callbacks are left as they
    /// were — owners re-register on load). Re-sorts.
    pub fn load_state(&self, v: &toml::Value) {
        let queue = GuestTimerQueue::from_toml(v);
        let next = {
            let mut inner = self.inner.lock();
            inner.queue = queue;
            inner.queue.next_deadline().unwrap_or(u64::MAX)
        };
        self.next.store(next, Ordering::Relaxed);
    }

    /// Serialise the queue's deadlines.
    pub fn save_state(&self) -> toml::Value {
        self.inner.lock().queue.to_toml()
    }
}

#[cfg(test)]
pub struct TestClock {
    cell: Box<u64>,
}

#[cfg(test)]
impl TestClock {
    pub fn new() -> Self {
        Self { cell: Box::new(0) }
    }
    pub fn ptr(&self) -> CyclesPtr {
        CyclesPtr::new(&*self.cell as *const u64)
    }
    pub fn now(&self) -> u64 {
        unsafe { std::ptr::read_volatile(&*self.cell as *const u64) }
    }
    pub fn set(&mut self, v: u64) {
        unsafe { std::ptr::write_volatile(&mut *self.cell as *mut u64, v) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64 as StdAtomicU64;

    fn counting(counter: &Arc<StdAtomicU64>) -> Arc<dyn GuestTimerCallback> {
        struct C(Arc<StdAtomicU64>);
        impl GuestTimerCallback for C {
            fn fire(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        Arc::new(C(counter.clone()))
    }

    #[test]
    fn queue_orders_by_deadline() {
        let mut q = GuestTimerQueue::new();
        q.schedule(PIT_CH0, 300, 0);
        q.schedule(PIT_CH1, 100, 0);
        q.schedule(PIT_CH2, 200, 0);
        assert_eq!(q.next_deadline(), Some(100));
        assert_eq!(q.pop_due(99), None);
        assert_eq!(q.pop_due(100).unwrap().key, PIT_CH1);
        assert_eq!(q.pop_due(200).unwrap().key, PIT_CH2);
        assert_eq!(q.pop_due(300).unwrap().key, PIT_CH0);
        assert_eq!(q.next_deadline(), None);
    }

    #[test]
    fn same_deadline_tie_break_is_stable_and_insertion_ordered() {
        // Two deadlines at the same cycle must fire in the order they were
        // armed, not by map iteration or comparison order.
        let mut q = GuestTimerQueue::new();
        q.schedule(PIT_CH2, 500, 0);
        q.schedule(PIT_CH0, 500, 0);
        q.schedule(PIT_CH1, 500, 0);
        let fired: Vec<u64> = (0..3).map(|_| q.pop_due(500).unwrap().key).collect();
        assert_eq!(fired, vec![PIT_CH2, PIT_CH0, PIT_CH1]);

        // Arming them the other way round swaps the order deterministically.
        let mut q = GuestTimerQueue::new();
        q.schedule(PIT_CH0, 500, 0);
        q.schedule(PIT_CH2, 500, 0);
        let fired: Vec<u64> = (0..2).map(|_| q.pop_due(500).unwrap().key).collect();
        assert_eq!(fired, vec![PIT_CH0, PIT_CH2]);
    }

    #[test]
    fn same_deadline_survives_rearm_of_an_unrelated_key() {
        // Re-arming a later key must not disturb the tie-break of the two
        // equal-deadline entries.
        let mut q = GuestTimerQueue::new();
        q.schedule(PIT_CH0, 100, 0);
        q.schedule(PIT_CH1, 100, 0);
        q.schedule(PIT_CH2, 900, 0);
        q.schedule(PIT_CH2, 950, 0); // re-arm the later key
        let fired: Vec<u64> = (0..2).map(|_| q.pop_due(100).unwrap().key).collect();
        assert_eq!(fired, vec![PIT_CH0, PIT_CH1]);
    }

    #[test]
    fn reschedule_replaces_previous_deadline() {
        let mut q = GuestTimerQueue::new();
        q.schedule(PIT_CH0, 100, 0);
        q.schedule(PIT_CH0, 400, 0);
        assert_eq!(q.len(), 1);
        assert_eq!(q.next_deadline(), Some(400));
        assert!(q.cancel(PIT_CH0));
        assert!(!q.cancel(PIT_CH0));
    }

    #[test]
    fn queue_round_trips_and_resorts_on_load() {
        let mut q = GuestTimerQueue::new();
        q.schedule(PIT_CH0, 700, 0);
        q.schedule(PIT_CH1, 100, 1);
        q.schedule(COMPARE, 400, 0);
        let v = q.to_toml();

        // Corrupt the saved order: loading must re-sort rather than trust it.
        let mut arr = v.as_array().unwrap().clone();
        arr.reverse();
        let loaded = GuestTimerQueue::from_toml(&toml::Value::Array(arr));
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.next_deadline(), Some(100));
        // Round-trip of the unmodified value keeps identical content.
        let again = GuestTimerQueue::from_toml(&q.to_toml());
        assert_eq!(again.to_toml(), q.to_toml());
    }

    #[test]
    fn timer_fires_at_its_guest_deadline() {
        let mut clock = TestClock::new();
        let timers = GuestTimers::new(clock.ptr());
        let count = Arc::new(StdAtomicU64::new(0));
        timers.schedule(PIT_CH2, 1_000, 0, Some(counting(&count)));

        clock.set(999);
        timers.drain(clock.now());
        assert_eq!(count.load(Ordering::SeqCst), 0, "must not fire early");

        clock.set(1_000);
        timers.drain(clock.now());
        assert_eq!(count.load(Ordering::SeqCst), 1, "fires exactly at the deadline");
        assert_eq!(timers.next_deadline(), u64::MAX);
    }

    #[test]
    fn periodic_timer_fires_every_period_and_catches_up() {
        let mut clock = TestClock::new();
        let timers = GuestTimers::new(clock.ptr());
        let count = Arc::new(StdAtomicU64::new(0));
        timers.schedule(PIT_CH0, 100, 100, Some(counting(&count)));

        clock.set(350); // three periods' worth (100, 200, 300) have elapsed
        timers.drain(clock.now());
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(timers.next_deadline(), 400);
    }

    #[test]
    fn callbacks_see_queue_ordering() {
        // The same-deadline tie-break must be observable through `drain`,
        // not just the raw queue: callbacks run in insertion order.
        let mut clock = TestClock::new();
        let timers = GuestTimers::new(clock.ptr());
        let order = Arc::new(Mutex::new(Vec::new()));
        struct Record(Arc<Mutex<Vec<u64>>>, u64);
        impl GuestTimerCallback for Record {
            fn fire(&self) {
                self.0.lock().push(self.1);
            }
        }
        timers.schedule(PIT_CH2, 10, 0, Some(Arc::new(Record(order.clone(), PIT_CH2))));
        timers.schedule(PIT_CH0, 10, 0, Some(Arc::new(Record(order.clone(), PIT_CH0))));
        clock.set(10);
        timers.drain(clock.now());
        assert_eq!(*order.lock(), vec![PIT_CH2, PIT_CH0]);
    }
}
