# Reverting the fork clock to upstream's `ci_clock`

**Status:** done 2026-10-26 (irix7/iris#98, techomancer/iris#322), branch
`convergence/ci-clock`. Measured on `callisto` (x86_64, 16 cores): determinism
on/off, boot time and bench. Idle-pace stays, with a known cold-boot
interaction recorded in `clanker/testing/release-idle-pause-hangs-prom-boot.md`.
This was the largest convergence left; it was done as one campaign.

## What upstream has

- Normal builds derive CP0 Count from a host `Instant` anchor at a fixed rate
  (33 MHz IP22/IP24, 97.5 MHz IP28) and arm Compare on the hptimer thread.
- The `ci_clock` feature swaps the time source to retired guest instructions:
  Count comes from `hot.cycles` at 10 ns each; Compare becomes a cycle
  threshold (`count_fire_cycle`) checked by the per-instruction preamble; the
  idle-park loop exits when the threshold is crossed; snapshots record the
  enabled feature set and refuse a mismatched restore.
- `ci_clock` exists for the snapshot determinism validator, not for normal use.

## What the fork has instead

- The cycle-derived clock is the default (`e8b264e`), Count is always synthetic.
- `src/cpu/guest_timer.rs`: one ordered queue of guest-time deadlines keyed on
  `hot.cycles`; the PIT channels and Compare register into it; batches drain it.
- The PIT was rewritten around that queue (`800f785`, `a1bf297`), including the
  read-back work now merged upstream separately.
- Idle-pace (`35b4ec5`, #83) advances `cycles` while parked so Count and armed
  timers keep moving.
- The CPU snapshot carries the queue and an interrupt word (`StateDesc` v2), and
  many tests assert cycle-based behaviour.

## Steps

1. **Start from upstream's clock code.** Take upstream's
   `src/cpu/mips_core.rs`, `mips_exec.rs`, `cop0.rs`, `jitv2/cop0.rs`,
   `idle_park.rs`, `src/dev/pit8254.rs` and the `hptimer` interaction as the
   base for the clock paths. Do not cherry-pick the fork's files backwards;
   build the revert as "upstream plus the fork-only features below".
2. **Delete the guest-time queue.** Remove `src/cpu/guest_timer.rs`, the
   `guest_timers` field, `drain_guest_timers` calls, deadline registration from
   the PIT, and the batch-drain comments. The PIT returns to upstream's
   `Instant`-based channels (it keeps the read-back command from #323).
3. **Restore `ci_clock`.** The fork's `ci_clock` feature is currently a no-op;
   make it upstream's real implementation (threshold Compare, idle-park exit,
   snapshot feature gating). Keep the wall clock as the default.
4. **Keep the fork-only features on top.** Idle-pace (#83) must be re-expressed
   against the wall clock: the parked CPU still needs to wake for the next host
   deadline and must not spin; upstream's idle-park exit is the reference.
   Journal hooks (`#47`) and `StateDesc` stay but lose the queue fields.
5. **Snapshots.** Keep the fork's registered state descriptions, but the CPU
   payload's clock fields become upstream's (Count, Compare, count-fire cycle
   under `ci_clock`, feature-set record). Bump the CPU state version and write
   the migration: a fork v2 snapshot restored into the wall-clock model gets a
   fresh Compare deadline, as upstream's restore does.
6. **Tests.**
   - Mark/adapt the fork tests that assume cycle Count (`clanker/cpu/cpu-count-is-wallclock-derived.md`).
   - Run the determinism validator with `--features ci_clock` and require
     `deterministic: true`.
   - Idle CPU: a parked guest must stay near 0 % host and still wake on a timer,
     an interrupt and a monitor command.
   - The PIT read-back tests keep passing on the Instant PIT.
7. **Measure and record.** Boot time, guest MIPS for the bench kernels, idle
   host CPU, and Count drift against host time, with and without `ci_clock`.
   Compare with the current fork release; the default clock changing is a
   user-visible release note.

## Risks

- **The OS depends on the fork clock.** `irix7/os` timing, the PIT trust
  decision and the benchmark history all assume the synthetic clock. If the
  default changes, the OS rig must use `ci_clock` for deterministic runs and
  wall time otherwise; say so in the release notes and the OS docs.
- **Idle-pace is a fork feature.** Do not lose it in the revert; it is the fix
  for the masked-interrupt spin (#83) and is independent of the clock source.
- **Snapshot compatibility.** Old fork snapshots carry the queue; refuse them
  with a clear version message rather than misinterpreting the fields.

## Evidence to bring

The PR is not opened until the numbers exist: determinism on/off, idle CPU,
boot time, and the bench kernels, on a named host with run counts. The policy
in `divergence-policy.md` applies: no PR without evidence.
