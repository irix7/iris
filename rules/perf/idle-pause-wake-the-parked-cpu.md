# idle-pause: an interrupt must unpark the CPU thread, not wait for the slice

`IdleParkState::park` sleeps in 1 ms slices and re-reads `hot.interrupts` at
the top of each one. Setting an interrupt bit from another thread is therefore
only noticed at the next slice boundary, so every interrupt that ends an idle
stretch — a device line through IOC, the compare timer, a `Signal::Interrupt`
— is delivered up to a millisecond late.

Measured on an otherwise idle machine, from `fetch_or` on the raising thread to
`park` returning on the CPU thread:

| | latency |
|---|---|
| `thread::sleep` slices | 649 us - 1.26 ms, spread across the slice |
| `park_timeout` + `unpark` | 6.2 - 6.8 us at every phase |

The fix is a registered thread handle plus a `PARKED` flag. Ordering is the
only subtle part, and it is a Dekker pattern: the parker stores `PARKED = true`
**before** its last look at the pending word, and each writer sets its bit
**before** reading `PARKED`. Both sides use `SeqCst`, so either the parker sees
the bit or the writer sees the flag. Weakening either to `Relaxed` reintroduces
a lost wakeup that only shows up as an occasional millisecond stall.

Two things that look like bugs and are not:

- A spurious `unpark` (the writer reads `PARKED` just before the parker clears
  it) leaves a token that makes the next `park_timeout` return at once. The
  loop re-checks and parks again; one wasted iteration, no misbehaviour.
- `unpark` on a thread that has already exited is a no-op, so a stale handle
  left in `PARKER` after the machine stops is harmless.

One that *is* a bug: every exit from the loop must leave `PARKED` false. The
`ci_clock` exit is easy to miss because it is behind a feature — and a stale
`true` puts every interrupt writer on the mutex while the CPU is running,
which is exactly the cost the flag exists to avoid.

`an_interrupt_ends_the_park_without_waiting_out_the_slice` sweeps the interrupt
across the slice, so it cannot pass on an interrupt that happened to land just
before a boundary.

## Wake latency to a guest-timer deadline (#68)

Once #43 armed guest-time deadlines, `park` no longer sleeps a fixed
`SLICE_NS` when a deadline is due: it sleeps to the soonest `hot.cycles`
deadline (Compare or a queued 8254 PIT timer). The figure that matters is the
host wall-clock time *after* that deadline at which the CPU thread wakes —
the observed sleep minus the sleep the deadline asked for. `park` cannot
return before `hot.cycles` reaches the deadline, so a spurious `wake` costs an
extra loop iteration, not an early return.

`host_wake_latency_after_the_guest_deadline_is_bounded` measures it for a
300 us guest deadline (30 000 cycles) and bounds the **best of five** samples:

| deadline | intrinsic overshoot (idle dev host) | bound |
|---|---|---|
| 300 us | ~60-75 us | 500 us |

Best-of-five because a `park_timeout` is a futex wait: a loaded host schedules
the wake late for reasons the emulator does not control (a busy dev box showed
outliers up to ~6 ms). The bound is deliberately under the retired 1 ms slice,
so a park that reverted to slice parking (~700 us overshoot on a 300 us
deadline) fails the test.
