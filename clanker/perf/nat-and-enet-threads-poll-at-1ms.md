# NAT and seeq-enet threads poll at 1 ms; make them event-driven

Two device threads wake ~1000×/s and do real work on every wake, even when the
guest network is idle.

- **NAT** (`src/net/mod.rs:1469-1582`): loops on `tx_wake` with a **1 ms**
  timeout (`wait_for(..., Duration::from_millis(1))`, line 1476) and then
  unconditionally calls `update_snapshot()` (line 1551) every iteration —
  rebuilding three fresh `Vec`s (TCP/UDP/ICMP) and walking every NAT entry. That
  is ~1000 allocator calls and ~1000 full table walks per second of pure idle
  overhead, and it scales with table size.
- **seeq-enet** (`src/dev/seeq8003.rs:631-722`): waits on `rx_wake` with a 1 ms
  timeout (line 639), then takes `SeeqState` twice per wake — once to snapshot
  `rx_cmd`/`station_addr` (643-646), once to apply results (674) — even when the
  pumps do nothing.

The producers already signal the condvars (`rx_wake.1.notify_one()` at
`seeq8003.rs:427/433`; NAT's `enqueue_rx`). The 1 ms timeout exists only as a
fallback, and it defeats the point of the signalling.

## Prior art: blocking-wait-with-computed-timeout is the standard

QEMU's `aio_poll` computes the timeout from the *earliest pending timer* and, with
nothing pending, blocks indefinitely (`timeout = -1`), woken by `aio_notify()` →
`event_notifier_set()` (an eventfd). MAME keeps a sorted timer list and advances to
the next deadline; there is no fixed tick. A 1 ms periodic wake keeps the CPU from
ever entering idle: the kernel's tickless (`NO_HZ`) machinery only switches the
tick off "when the CPU is idle", and a thread waking every 1 ms is never idle, so
it both keeps the tick firing and prevents deep C-states (`docs/kernel.org/timers/no_hz.html`).

The lazy-snapshot pattern is likewise canonical: Linux `/proc/net/ip_conntrack`
builds its view on read via `seq_file`, not eagerly.

## Suggested change

1. **NAT:** block on `tx_wake`/`rx_wake` with a *seconds-scale* watchdog timeout
   (e.g. 10 s) as the only backstop against missed wakeups; drop the per-tick
   `update_snapshot()` and build the status view lazily in the `net status`
   command's read path (it already locks `nat_ctl.snapshot`). Tolerate a non-atomic
   view — the consumer is a status display.
2. **seeq-enet:** wait on `rx_wake` indefinitely (same watchdog backstop), and
   skip the state snapshot/apply entirely when nothing changed.
3. Use `parking_lot::Condvar` if not already on it (no spurious wakeups, requeue
   on broadcast). An `eventfd` only buys something if the wait is ever folded into
   an `epoll` set, which it is not here.

The backstop must be a watchdog (seconds), not a poll — a 1 ms→10 ms widening
changes nothing material; the win is going event-driven.
