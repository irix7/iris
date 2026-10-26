# The determinism validator must release the stopped-CPU Count latch

`iris-ci validate` loads the snapshot with every thread stopped
(`load_snapshot_paused`) and re-executes the same instructions twice, in line,
under the executor lock. `Machine::stop` calls `MipsCore::on_cpu_stop`, which
sets `count_paused`; in a default (wall-clock) build `materialize_count` then
returns the latched `cp0_count` and never reads the host clock.

Consequence: both passes see an identical frozen Count no matter how much host
time passes between them, so the validator reports `deterministic: true` even
on the wall clock whenever the guest does not otherwise diverge. That hid the
clock model entirely — the check could not tell #98's two designs apart.

## Rule

`validate_snapshot_determinism` steps with
`Machine::cpu_step_n_inline_with_live_count`, which calls
`MipsCore::resume_count_for_inline_pass` before each pass:

- clears `count_paused`;
- re-anchors Count at the restored value (`reanchor_count_and_reschedule`);
- disarms the Compare deadline, so a host timer cannot inject IP7 between the
  two passes and make the result depend on wall time in a different way.

With the host wall clock, a snapshot whose guest reads Count now diverges
(`cp0_count` and the destination register differ); with `--features ci_clock`
the same snapshot is deterministic because Count is a function of retired
cycles.

## Evidence

A hand-built bare-metal loop (`mfc0 t0, $9; addiu; bne`) snapshotted from a
bare machine:

| build | `validate -n 1000000` |
|---|---|
| default wall clock | `deterministic: false`, `cp0_count` A=0xe071974 B=0xe071a0e |
| `--features ci_clock` | `deterministic: true` |

`inline_pass_releases_the_stopped_count_latch` and
`inline_pass_keeps_cycle_count_under_ci_clock` pin the core behaviour in unit
tests (`src/cpu/mips_core.rs`).

Normal monitor `step` is deliberately unchanged: it keeps the frozen latch so
debugging does not see Count move under the debugger.
