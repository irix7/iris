# The 8254 PIT is not an independent time base

The IOC's 8254 counter 0 is readable and does advance with guest time, but it
cannot be used to calibrate CP0 Count: both are derived from the same
`hot.cycles` clock. Do not reintroduce CP0/PIT calibration on the assumption
that the PIT is a separate reference.

## Symptom

A guest timing probe programs counter 0, reads it twice, and sees either a
count that never moves, or a Count:PIT ratio that is always exactly
`count_hz / 1_000_000` (33 at the default 33 MHz). The first looks like a dead
PIT; the second looks like a perfect calibration. Neither says anything about
whether the emulated machine keeps accurate time.

## Cause

Two separate things:

1. **Counters 0 and 1 have no clock of their own.** The IOC datasheet's clock
   chain is a divide-by-20 state machine giving counter 2 1 MHz, and *counter
   2's OUT clocks counters 0 and 1*. The model is faithful: `write_control` /
   `write_channel` call `update_chaining`, which sets counter 0/1's
   `input_freq` from counter 2's mode and reload. If counter 2 is not a mode 2
   or 3 rate generator, counters 0/1 never arm, their count reads stay at the
   reload, and status D6 (null count) stays set. On real hardware they would
   also not count.
2. **The PIT is derived from `hot.cycles` at `NS_PER_GUEST_CYCLE` (10 ns per
   retired cycle).** CP0 Count is derived from that same word, scaled by
   `count_hz`. The ratio is a constant the emulator chooses. There is zero
   drift between them by construction, so a calibration test always passes and
   can never detect an error.

Before issue #43 the PIT was interpolated from a host `Instant`, so it *was*
an independent (if jittery) reference. #43 replaced host time with the guest
cycle queue for determinism, which removed that independence deliberately.
The move is correct for replays and snapshots; it just means the PIT is no
longer a host-reference check.

Against wall time the rate is undefined while the CPU executes: one guest
cycle is one retired instruction, so guest time tracks host time only while
the CPU is idle-parked or stalled in `WAIT`. The only host-referenced clock in
the machine is the DS1386 RTC at 10 ms (hundredths) resolution.

## Rule

- A guest-side timing probe must program counter 2 as a rate generator before
  counter 0/1 will count, and must latch (counter latch or read-back command)
  before reading two bytes.
- Do not calibrate CP0 Count against the PIT and treat the result as evidence
  of accuracy. Use `pit status` (which says `independent: no`) and the
  documented resolution in `docs/Indy/ioc-8254-timer.md`.
- If an OS genuinely needs a host time reference, it has to come from outside
  the emulated timing path (NTP, or the RTC for a coarse check). Adding a
  host-backed timer would break the deterministic-time invariant unless it is
  kept out of guest-visible time.

## Evidence

`src/dev/pit8254.rs` tests, all deterministic (no host sleeps):

- `ch0_count_advances_with_guest_cycles` — 5 ms of guest time is exactly 5000
  ticks, a full period reloads.
- `ch0_rate_is_exact_over_ten_seconds_of_guest_time` — 100 Hz for 10 s of
  guest time is exactly 1000 callbacks; absolute deadlines, no accumulation
  error.
- `ch0_is_frozen_until_counter_2_supplies_a_clock` — the hardware trap, plus
  status D6 set/clear.
- `counter_latch_holds_its_value_across_guest_time`,
  `latch_single_byte_read_releases_the_latch`,
  `readback_latches_count_and_status_status_first`,
  `readback_second_latch_is_ignored_until_read`,
  `stopping_the_clock_freezes_the_count_where_it_was` — the read paths.
- `status_states_reference_and_capabilities` — the diagnostic reports it.

## Related

- [cpu-count-is-wallclock-derived.md](../cpu/cpu-count-is-wallclock-derived.md)
  — superseded; Count is cycle-derived now.
- `docs/Indy/ioc-8254-timer.md` — the human-facing description.
- `src/cpu/guest_timer.rs` — the shared deadline queue.
