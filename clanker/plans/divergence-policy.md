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

## 1. Already converged — no action

| Fork work | How it converged |
|---|---|
| console `TCP_NODELAY` | upstream #284 |
| CI login-prompt wait | upstream #285 |
| CI connection bound | upstream #286 |
| REX3 RGBMODE / FIFO conformance | upstream #288 |
| monitor state and `machine-start` ack | upstream #312 (fork PR #289 closed as superseded) |
| CI screenshot without a renderer | upstream #315 |
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
  (`e6050b7`)
- **VCE gating for the full cache model.** Upstream's R10000 model re-aliases
  instead of raising; the fork raises VCE at run time for the same case.
  Compare against upstream's current `mips_cache_v2.rs`, keep the R4400 VCE
  behaviour both have, delete the fork branch. (`b917df6`, part of `8df2ad0`)
- **HAL2 output plumbing.** The fork owns `open_cpal_output`,
  `AudioOutput`/`Resampler` traits and the cpal stream setup; upstream now has
  `host_audio`, shared with the O2. Send the pacing/backpressure layer, then
  replace the fork's plumbing with `host_audio` and delete ours.
- **Serial-log sink.** Upstream flushes the serial log per byte in both
  backends. The fork's `SerialLog` exists for timestamps and partial-line
  flushing. Send the timestamp feature, then delete the sink and use upstream's
  `TeeBackend`/`CiSerialBackend` with the timestamp flag. Do not keep two
  writers.
- **O2 SCSI raw-only shim.** `aic7880::attach_device` currently accepts raw
  images only because the fork's disks are `BlockNode`s and upstream's are
  `DiskBackend`s. This is temporary: delete it when BlockNode lands upstream;
  do not grow it.
- **Unmeasured perf branches.** Any part of the perf buckets (`9d1988a`,
  `d90dcc2`) that has no before/after measurement reverts rather than becoming
  a permanent fork behaviour.

## 3. Send upstream

Each item is a convergence PR: after it lands, the fork deletes its copy.

| Order | Divergence | Effort / blocker |
|---|---|---|
| 1 | REX3 auxiliary-clear conformance test (`846f8da`) | 1 file; add to rex3 PR #319 or send alone |
| 2 | NFS host MAC from the bridge IP (`4283144`) | 5 lines |
| 3 | `--serial-log-timestamps` | medium; unblocks the serial-sink revert |
| 4 | GR2 FIN2/FIN3 sequence tokens (#95) | dedicated port; upstream's `gr2/mod.rs` diverged 575 lines |
| 5 | 8254 PIT readback/latch (#106 code) | port onto upstream's host-`Instant` PIT; the fork's guest-clock conclusion stays fork-side |
| 6 | Per-consumer CI serial cursors (#97) | after the serial backend converges |
| 7 | HAL2 pacing and backpressure (#91) | port onto `host_audio`; then revert the plumbing |
| 8 | REX3 VDMA condvar re-dispatch (`c8e7a49`, #40) | small |
| 9 | VINO descriptor engine (#96) | high risk, after storage |
| 10 | BlockNode storage (#89) | foundation; then advisory lock (#90), O2 SCSI, bulk DMA (#101) |
| 11 | StateDesc campaign (#92) | four PRs; then the journal (#93) |
| 12 | Perf bundles (#100) | split by area; send only measured wins |
| 13 | jitv2 (#99) | rebase on upstream jitv2; send measured wins; drop the rest |
| 14 | CPU clock model (#98) | discussion first; if upstream declines, a *documented* fork divergence under the determinism goal |

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

Phase 0 (cheap convergence):

1. Verify and revert the cache default and the VCE branch.
2. Send the REX3 aux-clear test and the NFS MAC fix.
3. Open the CPU-clock discussion issue upstream (#98).

Phase 1 (serial and GR2):

4. Send serial timestamps; revert the serial sink.
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
