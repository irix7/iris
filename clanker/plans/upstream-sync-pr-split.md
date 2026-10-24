# Upstream sync: what to send to `techomancer/iris`, and in what order

**Status:** plan, 2026-10-24. Written after merging `upstream/main` (41 commits)
into `irix7/iris` `main`. Nothing here has been sent upstream yet.

This is a proposal for slicing the fork's 133 commits into pull requests
against upstream. It is derived from the merge's conflict surface and from each
commit's file list — not from a line-by-line audit of all 133. Treat the
ordering and the dependency claims as the useful part; re-derive the contents
against upstream `main` when you actually open each PR.

## Precondition

Nothing is sent until `main` is green on:

```
nix develop --command cargo test --workspace
nix develop --command cargo test --workspace --features jitv2
```

The merge that produced this plan is the compatibility proof for everything
below. A PR that cannot be rebased onto upstream `main` and pass those two runs
is not ready.

## What upstream asks for

`README.md` ends with a **Contribution policy**:

> We have no problems with LLM generated code. In fact most of IRIS is made with
> LLMs. But that doesn't mean we don't do proper software engineering. So lets
> keep PRs small and reasonable to review. One issue/fix per PR, preferably in
> one commit, since LLM code churn doesn't help with clarity. Lets keep this
> bisectable too.

Observed practice matches it. Of the last 25 merged PRs, nearly all are a
single commit touching one to four files; median time from open to merge is
hours; only a handful of closed PRs were closed unmerged. External
contributors (`atomchild411`, `pbartfai`, `AlexanderNyr`, `slaapliedje`) are
merged routinely. There is no `CONTRIBUTING.md` and no PR template, so the
policy above is the whole contract.

Consequences for this plan:

- **Squash each unit to one commit.** Multi-commit series are the exception
  upstream.
- Send the units below, not the stacks: the storage and snapshot work is
  already split into independently reviewable pieces.
- "One issue/fix per PR" rules out bundling the perf sweeps; split first.

## Coordination with the contributors we build on

| Upstream PR | Author | Our relation |
|---|---|---|
| #266 `z85c30: the TCP serial console takes more than one client` | atomchild411 | PR 1 adds `TCP_NODELAY` on top of their `accept_waiting`. |
| #265 `scsi: targets any controller can open, with the machine's identity` | atomchild411 | PR 3 replaces the `DiskBackend` foundation their identity work sits on. |
| #279, #283 allocation-length truncation | atomchild411, techomancer | PR 3 must keep both working; the merge already ported them onto `BlockNode`. |
| #261 `irq: an InterruptController trait` | atomchild411 | The merge adopts it wholesale; the fork's `PdmaCallback` and `fifo_full_cb` closures are abandoned in its favour. |
| #253 `idle-pause: wake for an interrupt masked by a raised spl` | slaapliedje | The fork's `IdleDecision::Pace` is a follow-up to their fix; credit it. |

Unit 4 (storage) is the only one that removes another contributor's design, so
it should start as an issue addressed to `atomchild411` rather than a surprise
patch.

## Closed — upstream already solved these differently

Do not re-open these. The merge deliberately took upstream's side, and the
fork's changelog records it:

| Fork work | Resolution |
|---|---|
| `8514e6c` single-client console refusal | Upstream's `3e73186` supports multiple clients; only the `TCP_NODELAY` half survives, folded into `accept_waiting`. |
| `hpc3` `PdmaCallback` / `irq_pending` deferral | Upstream's `IrqLine`/`InterruptController` rework (`5884984`) supersedes it. Deleted with its call sites. |
| `rex3` `fifo_full_cb` closure | Same rework; upstream's `IrqLine` type wins. The fork's additive `dma_space_cb` survives and belongs with GR2 `#67`. |
| idle-pause masked-interrupt spin (`35b4ec5`, `#83`) | Reconciled: upstream's "a pending masked line is about to be taken" check now returns the fork's `IdleDecision::Pace` instead of parking. One behaviour, not two. |
| `b917df6` VCE gating | Subsumed by upstream's R10000 model (`4b254ee`, `1121bfd`): the compile gates are gone and the R10000 re-aliases instead of raising. Nothing to send. |
| `bench/prebuilt/irisbench.elf` + `PROVENANCE` | Never hand-merge a binary. Upstream's image taken, stamp regenerated with `prebuilt-stamp.py --write`. |

## Pull requests, in order

Each entry gives the canonical commits (the `main` side where a work branch and
`main` both carry one), the files, and what must land first.

### 1. serial: `TCP_NODELAY` on the TCP console

- `8514e6c` — **the nodelay half only**, against upstream's `accept_waiting`.
- 1 file: `src/dev/z85c30.rs`, 5 lines.
- **Prepared:** branch `pr/serial-nodelay`, commit `38a47fd`. Applies and
  compiles on `upstream/main`. **Open: `techomancer/iris#284`.**

### 2. ci: wait for the login prompt instead of sleeping

- `4b0b4a0`
- 1 file: `src/iris_ci_main.rs`, +7/−2.
- Replaces a fixed 2 s sleep in `cmd_login` with a bounded wait for IRIX's
  `TERM =` prompt. Upstream already has `wait_with_deadline`, and its
  `cmd_login` is identical to the fork's pre-image, so the patch is clean.
- **Prepared:** branch `pr/ci-login-prompt`, commit `855316c`.
  **Open: `techomancer/iris#285`.**

### 3. ci: bound connections and reap idle handlers

- `687b76d`
- 1 file: `src/ci.rs`, +32/−2.
- Caps in-flight CI connections at 32 and gives each handler a 300 s read
  timeout, so abandoned clients cannot accumulate threads and descriptors.
- **Prepared:** branch `pr/ci-bound-connections`, commit `86d209f`.
  **Open: `techomancer/iris#286`.**

### Not standalone: scsi bulk DMA

`1492b44` ("make bulk DMA advance the channel address") reads like a
one-file bug fix, but upstream's `DmaClient` trait has no `read_block`/
`write_block` at all — bulk DMA is the fork's `d90dcc2`. So it is a fix to
fork-only code and must travel **with the bulk-DMA introduction** as one PR,
not on its own. It also conflicts with upstream's `hpc3` structure, which now
uses `IrqLine` rather than the fork's `PdmaCallback`.

### 4. storage: the layered block-node interface (`#49` → `#51`)

- `df7f6c7` (`#49`), `1801c06` (`#50`), `d9c1953` (`#51`)
- 8 files: `src/block_node.rs` (new), `src/chd_disk.rs`, `src/cow_disk.rs`,
  `src/lib.rs`, `src/scsi.rs`, `src/dev/wd33c93a.rs`, `src/bin/chd_extract.rs`,
  `iris-gui/src/handle.rs`

This is the expensive one: upstream is actively building *on* `DiskBackend`
(controller identity, allocation-length truncation), so the PR must carry
upstream's `ScsiIdentity` and `alloc_len_10`/`request_sense_alloc_len` work on
top of the new trait. The merge already has that reconciliation — use it as the
reference. Do not bundle feature changes into this PR.

Squash the three commits into one: upstream sees one change ("replace the
backend enum with a layered block node"), and it must stay bisectable.

### 5. disks: advisory-lock read-write base images

- `ab0be92`
- 1 file: `src/block_node.rs`. **Depends on 3.**

### 6. HAL2: capture, backpressure and pacing

- `47ce24f` (`#60`), `ffe3e04` (`#62`), `50093f0` (`#61`), `63ce732` (`#59`),
  `3f274b6` (`#72`), `4f04cff` (`#73`), `5ea051c`
- 5 files: `src/dev/hal2.rs`, `src/config.rs`, `src/traits.rs`, `src/ci.rs`,
  `.github/workflows/rust.yml`

Land the trait/backpressure commits first; the wav/null capture and the CI job
follow. `c3dac87` (`#44`, snapshot support) does **not** belong here — it goes
with 6/7.

### 7. snapshot: registered state descriptions (`#45` → `#46` → `#48` → `#69`)

- `e86b001` (`#45`), `ed9bcce` (`#46`), `12e53f0` (`#48`), `559f3d9` (`#69`)
- `src/state_desc.rs` (new), `src/snapshot.rs`, `src/traits.rs`,
  `src/machine.rs`, `src/chunk_store.rs`, `src/physical.rs`,
  `src/ppmem/ppmem.rs`, the migrated devices, plus the `clanker/snapshot/` note
- `c3dac87` (`#44` HAL2 snapshot) lands here, after `#45`.

Open these as four PRs, not one: `#45` is the seam, `#46` adds versioning, `#48`
is an independent chunk-store optimisation, `#69` is mechanical adoption.

### 8. cpu: record/replay journal (`#47`)

- `2f5b89e`
- `src/cpu/journal.rs` (new), `src/cpu/mips_exec.rs`, `src/ci.rs`,
  `src/iris_ci_main.rs`, `test/journal/*`, workflow

Depends on 6 only if the journal uses `StateDesc`.

### 9. rex3-jit: hot/cold compile queue (`#58`)

- `951da79`
- 4 files: `src/dev/ng1/rex3_jit_queue.rs` (new),
  `src/dev/ng1/rex3_jit/mod.rs`, `src/dev/ng1/mod.rs`, the queue-retry note

Strictly better than upstream's single channel plus retry-on-full: the draw
path is never dropped for a full queue, warm-up blocks instead.

### 10. gr2: FIN2/FIN3 sequence tokens (`#57`, `#67`)

- `b8976a0` (`#57`), `a47c451` (rollback), `310c511` (`#67`), `817513e`
- `src/dev/gr2/*`, `src/machine.rs`

`310c511` also wants the fork's `dma_space_cb` seam on REX3/GR2 (see the closed
table), so either include that or land `#67` with a different wake path.

### 11. vino: descriptor engine rewrite (`#56`, `#71`, `#70`)

- `0f15003` (`#56`), `f84d43a` (`#71`), `dfc1f0b` (`#70`), `f446354` (plan doc)
- `src/dev/vino.rs`, `docs/vino-descriptor-plan.md`

Highest risk and highest payoff. Upstream still patches the old
`dma_emit_dword`; the fork splits pixel production from descriptor fetch. Needs
its own review cycle and live 5.3/6.5 capture validation.

### 12. CI serial harness hardening

- `59b1458`, `df49429`
- `src/ci.rs`, `src/dev/z85c30.rs`

Generic, but written against the fork's serial plumbing. Split one behaviour
per PR. (`687b76d` and `4b0b4a0` are already units 2 and 3 above.)

### 13. Discussion first: the CPU clock model

- `800f785` (`#43` guest-time timer queue), `e8b264e` (`#42` cycle-derived
  clock), `8df2ad0` (`#35` accurate-cache), `e6050b7`

This changes a contract `HACKING.md` states differently upstream (host
wall-clock vs retired cycles). Open an issue with the reasoning and the idle-park
interaction before writing a PR; it touches every timing-sensitive test.

### 14. jitv2 track — separate campaign

- `0ebca99`, `e0cf094`, `2a7889a`, `89671d8`, `f8394e6`, `b8a2393`, `f1513f1`,
  `f830864`, `8c6265a`

Conflicts with upstream's own jitv2 work (`13ffcc2` and neighbours). Rebase and
re-measure each against upstream `main` after 6–11 land.

### 15. perf bundles — split before sending

- `9d1988a` (26 files), `d90dcc2` (13 files)

Both are mixed: interpreter dispatch, ppmem fast paths, device idles, NAT waits.
Send as separate focused PRs after the storage/snapshot items, since several
touch the same files.

## Stays fork-only — never upstream

- Instances/fleet: `df5df54`, `947a60c`, `1bd24a1`, `1f8a51a`, `8c496b0`,
  `45b3744`, `8f38295`, `0f4faa4`, `35013be`, `7d4cc80`, `ac957ab`, `8ea0311`,
  `1528d70`. This is the fork's stated purpose.
- Releases and versioning: `ba54679`, `b91e961`, `0ee1029`, `39d3d62`,
  `c252663`, `10f6de9`.
- Benchmark history, graphs and backfill: ~30 commits under `data/`,
  `tools/bench_*`, `.github/workflows`.
- `CONTEXT.md`, `docs/adr/`, `docs/research/` (carried on `wip/pre-upstream-sync`).
- Attribution (`61fae44`) — must stay in the fork.
- The fork's own `clanker/` notes and local `flake.nix`/`irix-build` tooling.

## Provenance

Credit for the design ideas reused here (the hot/cold compile queue, the
sequence-token FIN2/FIN3 model, the block-node layering) belongs to
**Jupiter Belic** and the **`irix7/iris`** fork. If any of this plan's content is
carried into an upstream PR, the commit and PR description must name the fork
commit it came from.
