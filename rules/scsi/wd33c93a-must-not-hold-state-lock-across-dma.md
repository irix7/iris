# WD33C93A must not hold its state lock across the DMA transfer (or spin-yield under it)

Two concurrency anti-patterns in the SCSI worker, both in `src/dev/wd33c93a.rs`:

1. **`state` is held across the entire transfer.** The worker acquires the state
   guard once at loop entry (`~1077`) and keeps it through
   `process_wd_command` → `send_data_chunked`/`receive_data_chunked_from`
   (`2184-2326`), i.e. the whole byte-by-byte DMA loop. The CPU thread takes the
   *same* lock on every register read/write (`read` `687`, `write` `891`), so ASR/
   status polling blocks behind the worker for the entire transfer.
2. **A 100 ms spin-yield wait runs *under* that lock.** `send_data_chunked`
   (`2196-2208`) and `receive_data_chunked_from` (`2274-2301`) spin
   `while not_active { thread::yield_now() }` for up to 100 ms waiting for the DMA
   channel to arm — so the CPU thread can stall on the mutex for up to 100 ms.

## Why `yield_now()` is wrong here, and it is not subtle

`sched_yield(2)`: "use of sched_yield() with nondeterministic scheduling policies
… is unspecified and very likely means your application design is broken", and the
caveat describes this case verbatim — yielding while holding a resource other
threads need. Rust's `yield_now` doc says the same: it does not block, so
yielding in a loop busy-waits, and a blocking primitive (Condvar/channel/join) is
the first choice. Holding `state` while yield-spinning is a live-lock risk: the
CPU thread that must arm the channel can be starved of scheduler slots by the very
thread waiting on it.

## Prior art

QEMU's Big QEMU Lock (BQL) is the canonical cautionary tale: one lock serialising
all register access became *the* scalability bottleneck, and the fix was (a)
finer-grained locks and (b) lockless reads of progress state (RCU/QHT, wait-free
read side) so a poll never blocks behind a transfer. `multi-thread-tcg.rst`:
"we push the use of the lock as far down … to minimise contention".

## Suggested fix

1. **Do not hold `state` across the data move.** Snapshot the command + transfer
   parameters under the lock, release it, then run the DMA loop. This is a state
   machine, not a critical section.
2. **Replace the arm-spin with a blocking wait.** `parking_lot::Condvar` (or
   `park_timeout`) on the channel's "armed" flag; the DMA channel notifies on arm.
   A futex-backed sleep costs ~1-2 µs to wake instead of a 100 ms spin.
3. **Make the CPU's status poll lock-free.** Expose busy/phase/byte-count/error as
   atomics written by the worker (release) and read by the CPU (acquire), composed
   into the ASR/status word in the read handler — the same lockless-read-side move
   QEMU made with RCU.
4. Keep the hptimer "spin under ~200 µs" policy for genuinely sub-µs waits; a
   100 ms wait belongs to a condvar, not a spin.

This pairs with `rules/scsi/hpc3-dma-is-bulk-not-byte-at-a-time.md`: the bulk-DMA
change (chunked ≤8 KiB, IRQ after unlock) is what actually shortens the critical
section, so the two should land together.
