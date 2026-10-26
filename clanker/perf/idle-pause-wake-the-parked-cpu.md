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

## Wake latency to a guest-timer deadline (#68) — superseded

> **Superseded (2026-10-26, #98).** #43's guest-time timer queue was removed
> when the fork reverted to upstream's host wall clock. `park` is back to
> waking on any pending interrupt, with the hptimer delivering Compare and PIT
> ticks exactly like a device line; it does not target a `hot.cycles` deadline
> any more. The measurement and test below described the queue build and no
> longer exist. The unpark-latency rule above is unchanged, and the
> `--features ci_clock` threshold exit is still behind a feature flag, so the
> "every exit clears `PARKED`" warning still applies.

Once #43 armed guest-time deadlines, `park` slept to the soonest `hot.cycles`
deadline (Compare or a queued 8254 PIT timer) instead of the fixed `SLICE_NS`.
The measured overshoot on an idle dev host was ~60-75 us for a 300 us deadline.
That machinery is gone with the queue.
