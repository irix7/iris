# Measuring the callout clobber-mask gain, in reloads

#37 gave every JIT→Rust callout a declared GPR/FPR read/write mask and added a
per-region GPR forwarding cache that consumes it, but the win was asserted only
as "a smaller `SpillPlan`". #66 adds the missing measurement: a per-region GPR
load/reload count, an A/B switch that reproduces pre-#37 conservative masks, and
a deterministic unit test that pins the delta. This note records what that
measurement found and what is still unmeasured.

## The instrument

`ForwardStats` (`src/cpu/jitv2/codegen.rs`) counts the two outcomes of every
GPR read `emit_read_gpr` makes:

- `gpr_loads_avoided` — the forwarding cache had the value, so no `load` was
  emitted;
- `gpr_loads_emitted` — a real `load` off `core.gpr` was emitted.

`Codegen::last_forward_stats()` reports the last compile's counts, so a corpus
run can sum them across every region:
`zz_corpus_sizes` now prints `CORPUS forward_masks=on reads=… avoided=… emitted=…`.

The A/B switch is `Codegen::set_callout_masks_enabled(bool)` (seeded process-wide
by `IRIS_CALLOUT_MASKS=0`). `false` substitutes `CalloutClobbers::CONSERVATIVE`
for every call, i.e. exactly the pre-#37 emission, without reverting #36/#37.

## The measured delta, unit level

`masked_callouts_avoid_a_gpr_reload_the_conservative_masks_force` compiles one
region both ways and diffs the counts. The region is:

```
addu r4, r2, r3   ; write r4
sw   r4, 0(r1)    ; memory callout (MemoryWrite mask declares no GPR)
addu r6, r4, r5   ; read r4 — must not be reloaded
```

`sw` is the shape where the win is real:

```
CALLOUT_MASK_GAIN masked avoided=2 emitted=5 | conservative avoided=1 emitted=6
```

One reload avoided per store-crossing read. The test asserts the delta is
exactly one, that both compiles see the same read stream (7 reads), and that the
conservative build reports a fully conservative `last_region_clobbers`.

## A load does NOT get the same win — and a `SpillPlan` could never show it

A `lw` callout declares the *same* empty `MemoryRead` mask, but it does not
benefit. `emit_mem_read` follows the callout with `emit_check_mem_status`, which
splits to a fresh continuation block; the load then writes its destination GPR in
that new block, and `GprForward::put`'s `sync` clears the whole cache because the
block changed and was never `bless`ed. The mask is consulted *before* the split,
so by the time the next instruction reads a previously-live GPR the cache is
already empty in **both** builds.

This is the concrete reason the original claim ("a smaller `SpillPlan`") was not
evidence of anything: the spill plan shrinks in both the store and the load case,
but only the store case turns it into a recovered reload. Only counting reloads
distinguishes them. The interrupt and SMC preambles `bless` their continuation
blocks precisely so their (callout-free) split does not cost forwarding;
`emit_check_mem_status` and `emit_trap_if_nonzero` do not, so any GPR write that
follows one wipes the cache. That is a candidate follow-up optimisation, not
part of this measurement.

## Running the corpus A/B

```
IRIS_CORPUS_DIR=jitv2_corpus IRIS_OPT_SPEED=1 IRIS_FORWARD_AB=1 \
  cargo test --release --features jitv2 zz_corpus_sizes -- --nocapture
```

`IRIS_FORWARD_AB=1` compiles each entry a second time with the masks off and
prints `forward_masks=off` plus `forward_masks_delta`. It doubles compile time,
so it is off by default; a plain run still reports the masked totals. For a live
boot, `IRIS_CALLOUT_MASKS=0` seeds the conservative baseline.

## What is still not measured here

- **No checked-in corpus, and no IRIX boot available in this worktree.** The
  numbers above are one hand-built region, not a real guest workload. The corpus
  harness is wired but has no data to run on; `IRIS_FORWARD_AB` is the path to
  the real number once a `.pcp` corpus is captured with `j2 corpus`.
- **No wall-clock/throughput figure.** Consistent with
  `interrupt-check-frequency-gates-gpr-forwarding.md`, a static load count is
  not a speed claim; the recovered load was an L1-resident `core.gpr` access
  already off the critical path. Treat this as a code-quality measurement until
  a live benchmark pairs it with cycles.
- **The load-path cache clear above is unaddressed**, so the corpus delta will
  be dominated by stores and the FPU-callout / other non-splitting callouts, not
  by the load-heavy traffic #37 was first argued for.
