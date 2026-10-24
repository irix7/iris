# Serial log guest stamps use synthetic cycle time, not wall time

Status: current (2026-10-24, #105).

## Rule

`--serial-log-timestamps guest` derives its stamp from `Hot::cycles` at
`NS_PER_GUEST_CYCLE` (10 ns per retired cycle), the same synthetic time base
CP0 Count uses. It is not wall time: it advances only while the guest
executes, so a paused or slow host shows as a gap-free timeline that runs
slower than the host clock. Use `wall` when a transcript must line up with
host logs; use `guest` for deterministic, host-independent ordering.

## Wiring

The sink is constructed with the SCC, before the CPU exists, so it holds an
`AtomicPtr<u64>` into the CPU's `Hot::cycles` and `Machine` wires it at the
same point it wires the guest timer queue (`set_guest_cycles`, after the CPU
is inside its final allocation). It is cleared in `Machine::drop` before the
CPU is freed. Unit tests must call `set_guest_cycles` themselves or guest
stamps read zero.

## Flush and format

- `off` (default) is a byte-for-byte transcript: no added bytes.
- `guest`/`wall`/`both` prefix each line at its first byte. A `\r` alone does
  not start a new line (progress bars stay one line).
- One completed line (`\n`) causes one `BufWriter::flush`; a partial line is
  flushed by the `serial-log-flush` thread after at most 100 ms, even if the
  guest writes nothing more; `Machine::stop` and `SerialLog::drop` flush again.
- Both the CI console and the TCP tee share one `SerialLog`, so they cannot
  disagree about buffering or timestamps.
