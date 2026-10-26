# Divergence policy: what we send, what we revert, what stays ours

**Status:** decision, 2026-10-25. This supersedes the *classification* in
[upstream-sync-pr-split.md](upstream-sync-pr-split.md); that plan's ordering
detail for the storage and snapshot stacks is still useful.

## The rule

The fork exists for four things:

1. parallel per-task instances and the fleet harness;
2. release engineering, versioning and the benchmark history;
3. the OS-development rig (launcher, rig configs, guest loop);
4. agent notes, skills and fork process docs.

Those are the only differences that may stay. Everything else in the
fork↔upstream diff gets a disposition, and there are only two:

- **send** — an upstream PR that lands the change and then deletes the fork's
  copy of it, or
- **revert** — adopt upstream's implementation and delete ours.

A third implementation is not an option. Every PR description names the
divergence it removes and links #85. No new work is started in an area marked
revert below.

Order of preference: upstream the change; then a thin compatibility layer;
then keep the patch only until its campaign lands; never a parallel
implementation.

## Evidence bar

An upstream PR is opened only when it is **proven**. The PR body and commit
message carry the evidence; a reviewer must be able to check the claim without
trusting us.

What counts:

- **A failing test**: the test exists before the change (or the failure is
  pasted), it fails on upstream `main`, and it passes with the patch. A test
  added by the patch is evidence only when the old code actually fails it —
  say so and show the failure.
- **Measurements**: before/after numbers with the host, build features, the
  command, and the run count. One run is a sample, not a result.
- **A capture**: a trace, replay, serial transcript, register dump or
  guest-side reproduction that fails on upstream and passes with the patch.
- **For a test-only or docs PR**: the behaviour it pins and why nothing pinned
  it before — a conformance test is evidence when it would have caught a real
  regression, not when it restates the implementation.

"Feels faster", "cleaner", a single unmeasured run, or a test that passes on
both sides is not evidence. An item without evidence stays on the shelf; it
does not become a PR. When a PR merges, delete the fork's copy and link the
evidence from the changelog or the issue.

## 1. Already converged — no action

| Fork work | How it converged |
|---|---|
| console `TCP_NODELAY` | upstream #284 |
| CI login-prompt wait | upstream #285 |
| CI connection bound | upstream #286 |
| REX3 RGBMODE / FIFO conformance | upstream #288 |
| monitor state and `machine-start` ack | upstream #312 (fork PR #289 closed as superseded) |
| CI screenshot without a renderer | upstream #315 |
| REX3 hot/cold compile queue | upstream #319 |
| REX3 auxiliary-clear conformance test | upstream #320 |
| NFS host MAC derivation | upstream #321 |
| 8254 PIT read-back and latch release | upstream #323 (the fork keeps its version only until the clock model is settled) |
| Physical decode map | upstream `81a88ac`, adopted in `63f87d1`; the snapshot dirty bitmap now lives on `SparseDeviceMap` |
| machine lifecycle helpers duplicated in `machine.rs` | removed by the O2/`Workstation` merge |
| second-client console refusal | dropped; upstream's multi-client console won |

## 2. Revert to upstream

These are differences with no fork-specific purpose. Adopt upstream and delete
the fork's version.

- **Cache-model default.** Fork `CacheModel::is_shadow(Auto)` is
  `!accurate-cache`, i.e. the shadow cache (observation-only, no cache-error
  exceptions) is the default in every build. Upstream's `Auto` is
  `cfg!(feature = "jitv2")`. This is a correctness divergence for speed.
  Revert the default; keep `cache = "shadow"` as an explicit opt-in. Only send
  a default change upstream if the benchmark history justifies it.
  (`e6050b7`) **Done:** `e7a7f62` adopts upstream's selection and drops the
  `accurate-cache` feature; the CI cell passes `--cache full`.
- **VCE gating — already converged.** The merged `mips_cache_v2.rs` differs
  from upstream only by a jitv2 AOT geometry helper; there is no separate VCE
  branch left to revert. (`b917df6` was subsumed by the O2/`Workstation` merge.)
- **HAL2 output plumbing.** The fork owns `open_cpal_output`,
  `AudioOutput`/`Resampler` traits and the cpal stream setup; upstream now has
  `host_audio`, shared with the O2. Send the pacing/backpressure layer, then
  replace the fork's plumbing with `host_audio` and delete ours.
- **Serial-log sink — done.** Upstream #325 landed the shared sink and the
  `off|wall` timestamp modes. The upstream merge (`f93ca35`) deletes the fork's
  `guest`/`both` sink and its partial-line flusher; the CI and TCP paths use
  one `SerialLog`, and the fork's per-consumer console cursors (#97) remain.
  Do not reintroduce a second writer.
- **CPU clock model (#98) — done.** Upstream's reply on techomancer/iris#322
  pointed at the existing `ci_clock` feature: normal builds keep the host wall
  clock, and `ci_clock` derives Count from retired cycles for the determinism
  validator. `convergence/ci-clock` adopts upstream's clock code, removes the
  ordered guest-time queue (`800f785`, `e8b264e`), returns the PIT to
  upstream's `Instant` channels (keeping #323's read-back) and keeps idle-pace
  re-expressed against the wall clock. The CPU state description is v3 with a
  `clock` field and a v2 migration. See `plans/ci-clock-reversion.md`.
- **O2 SCSI raw-only shim.** `aic7880::attach_device` currently accepts raw
  images only because the fork's disks are `BlockNode`s and upstream's are
  `DiskBackend`s. This is temporary: delete it when BlockNode lands upstream;
  do not grow it.
- **Unmeasured perf branches.** Any part of the perf buckets (`9d1988a`,
  `d90dcc2`) that has no before/after measurement reverts rather than becoming
  a permanent fork behaviour.
- **jitv2 compile-pool default — measured, decision open.** Upstream defaults
  to one worker; the fork's `9d1988a` uses `min(available_parallelism, 4)`.
  The #112 cold-page A/B shows 4 workers recover 127–166 ns/op aligned kernels
  to 13–18, but add 1–3 s to a 38 s suite and mix the packed results. Keep the
  fork default for now and document it; send upstream only after a real IRIX
  boot soak, otherwise revert to upstream's 1.

## 3. Send upstream

Each item is a convergence PR: after it lands, the fork deletes its copy.

| Order | Divergence | Evidence | State |
|---|---|---|---|
| 1 | `--serial-log-timestamps` | three tests (raw identity, wall stamping/hold, stamp shape) plus a live ROM boot showing the stamps | **merged: #325; sink deleted in `f93ca35`** |
| 2 | GR2 FIN2/FIN3 sequence tokens (#95) | a minimal upstream test where a deep FIFO raises FIN3 early; the fork's `fin3-must-track-pending-finish` rule describes the failure | **needs upstream repro** |
| 3 | 8254 PIT readback/latch (#106 code) | three tests fail on upstream's PIT (read-back ignored, single-byte latch stuck); they pass with the patch | **merged: #323** |
| 4 | Per-consumer CI serial cursors (#97) | a two-consumer test where one consumer steals the other's bytes | **needs test** |
| 5 | HAL2 pacing/backpressure (#91) | the underrun/drift numbers in `clanker/perf/hal2-pump-thread.md` and `clanker/devices/cpal-alsa-busy-poll.md`, with method and run count | **cite the existing numbers** |
| 6 | REX3 VDMA condvar re-dispatch (`c8e7a49`) | before/after CPU time or latency for the busy-wait path | **needs measurement** |
| 7 | VINO descriptor engine (#96) | replay/validation captures from #56/#71/#70 on real IRIX | **needs capture** |
| 8 | BlockNode storage (#89) | the fork's storage test suite run against the new interface, plus a no-behaviour-change statement | **sent: techomancer/iris#337** (one commit, all CI green; advisory lock stays fork-only) |
| 9 | StateDesc campaign (#92) | parity and migration tests per PR | **per-PR evidence** |
| 10 | Perf bundles (#100) | benchmark history: named host, features, run count, before/after | **measured wins only** |
| 11 | jitv2 (#99) | the same benchmark standard | **measured wins only** |
| 12 | CPU clock model (#98) | **resolved by upstream**: use the existing `ci_clock` feature for determinism and keep the wall clock as default — see the revert list and `plans/ci-clock-reversion.md` | **decided, planned** |

## 4. Keep fork-only

- **Instances/fleet:** `src/instance.rs`, `src/state.rs`, `derive_instance`,
  `--instance`/`--state-dir`/`--port-base`, the ownership lock, the fleet
  harness, per-instance paths/ports/MACs/overlays, `IRIS_COW_OVERLAY_DIR`,
  state-dir COW overlays and CHD diffs.
- **Releases/versioning:** `vX.Y.Z+irix7` tags, release workflow and notes,
  version-bump commits, the fork launcher.
- **Benchmark history:** `data/`, the backfill/A-B workflows, README graphs,
  history fragments.
- **Agent/process docs:** `clanker/`, `docs/agents/`, skills, the fork's
  CHANGELOG entries and attribution rule.
- **Local build tooling:** `flake.nix`, the launcher, rig-specific paths.

`IRIS_COW_OVERLAY_DIR` and the state-dir overlays stay with the instance work,
not with storage, because they exist to let parallel instances share a base
image.

## 5. Execution order

A PR whose evidence row is not satisfied is not opened; its fork code waits,
or is reverted if upstream does not need it.

Phase 0 (cheap convergence):

1. Verify and revert the cache default and the VCE branch.
2. Send the REX3 aux-clear test and the NFS MAC fix.
3. Open the CPU-clock discussion issue upstream (#98).

Phase 1 (serial and GR2):

4. ~~Send serial timestamps; revert the serial sink.~~ Done: #325 and the
   upstream merge.
5. Port GR2 sequence tokens.
6. Port the PIT readback onto upstream's PIT.

Phase 2 (audio and CI):

7. Send HAL2 pacing; revert the HAL2 plumbing.
8. Send per-consumer CI serial cursors.

Phase 3 (storage foundation):

9. BlockNode (#89) → advisory lock (#90) → O2 SCSI → bulk DMA (#101).

Phase 4 (the rest):

10. StateDesc (#92) → journal (#93); VINO (#96); perf (#100) and jitv2 (#99)
    as measured, split PRs.

**After each upstream merge, delete the fork-side code it replaced in the same
week.** A convergence PR that leaves the old path behind has not resolved the
divergence.
