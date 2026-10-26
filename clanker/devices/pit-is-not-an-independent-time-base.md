# The 8254 PIT is host-timed, not a good CP0 Count reference

> **Status: current again (2026-10-26, #98).** The fork's #43 rewrite put the
> PIT on the same `hot.cycles` queue as CP0 Count; the clock revert put it back
> on `hptimer` host timers. The rule below is why: two clocks derived from one
> source cannot check each other. The counter-2 chaining rule is unchanged.

## Symptom

A guest timing probe programs counter 0, reads it twice, and expects a rate it
can compare with CP0 Count. In the default build both are host-anchored: Count
from an `Instant` anchor at `count_hz`, the PIT from recurring `hptimer`
callbacks and a `Instant`-interpolated count. The ratio is the emulator's
chosen constant plus two host scheduling paths, so it is neither exact nor an
independent accuracy measurement. Under `--features ci_clock` Count is instead
derived from retired instructions while the PIT stays host-timed: the two
diverge by design.

## Cause

1. **Counters 0 and 1 have no clock of their own.** The IOC datasheet's clock
   chain is a divide-by-20 state machine giving counter 2 1 MHz, and *counter
   2's OUT clocks counters 0 and 1*. The model is faithful: `write_control` /
   `write_channel` call `update_chaining`, which sets counter 0/1's
   `input_freq` from counter 2's mode and reload. If counter 2 is not a mode 2
   or 3 rate generator, counters 0/1 never arm and their count reads stay at
   the reload. On real hardware they would also not count.
2. **The PIT is armed on the `hptimer` thread**, one recurring timer per
   channel period, and a read interpolates from a host `Instant`. CP0 Count in
   a default build is host-anchored too. Neither is derived from the other, but
   both inherit host scheduling jitter.

## Rule

- A guest-side timing probe must program counter 2 as a rate generator before
  counter 0/1 will count, and must latch (counter latch or read-back command)
  before reading two bytes.
- Do not treat the PIT as an accuracy reference for CP0 Count. It is a coarse
  host-timer device with microsecond ticks.
- If an OS genuinely needs a host time reference, it has to come from outside
  the emulated timing path (NTP, or the RTC for a coarse check).
- A deterministic run must not depend on PIT arrival: `ci_clock` fixes Count
  and Compare only; the PIT still fires from the host timer thread.

## Evidence

- `src/dev/pit8254.rs`: the read-back/latch tests from upstream #323, and the
  save/load round trip, all pass on the `Instant`-based channels.
- `src/cpu/idle_park.rs` and `src/cpu/mips_exec.rs` tests cover the parked CPU
  waking for an interrupt raised by another thread.

## Related

- [cpu-count-is-wallclock-derived.md](../cpu/cpu-count-is-wallclock-derived.md)
  — current again; default-build Count tests can flake under host load.
- `docs/Indy/ioc-8254-timer.md` — the human-facing description.
