# Serial log timestamps are wall-clock only (off or wall)

Status: current (2026-10-26). Replaces the fork's guest-time sink, removed when
upstream's serial timestamps (#325) were merged.

## Rule

`--serial-log-timestamps` has two values: `off` (the default, a byte-for-byte
raw transcript) and `wall` (UTC ISO-8601 before each completed line). The
fork's `guest` and `both` modes are gone; guest time was the synthetic cycle
clock, which is not wall time. Deterministic scheduling belongs to `ci_clock`,
not the serial log.

## Behaviour

- One sink is shared by the CI console and the TCP tee, so both write the same
  bytes (upstream #325).
- `off` writes every byte as it arrives, partial line included.
- `wall` holds a partial line until its newline (a stamp belongs to the line it
  starts); `Machine::stop` / `SerialLog::flush` writes it. A prompt without a
  newline does not appear in `wall` mode until the guest continues or stops.
- The fork's 100 ms `serial-log-flush` thread and the `set_guest_cycles`
  wiring were removed with the old sink. Do not reintroduce either without a
  new upstream design.

## What remains fork-only

The CI serial backend's append-only `ConsoleRing` and its per-consumer cursors
are independent of the log sink and stayed. `CiSerialBackend::send_byte` still
pushes into the ring and mirrors the same byte to the shared log.
