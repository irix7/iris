# AOT/persistent jitv2 page cache — viability spike (#54)

**Decision: GO on feasibility; GO for a bounded, corpus-driven, opt-in offline
filler. NO-GO on a from-binaries AOT compiler, on making AOT a default, or on
shipping prewarmed caches, until the conditions in [Decision](#decision) are
met.**

This is the write-up for #54, blocked by #38 (arena lifetime split). It answers
one question: can the existing jitv2 persistent code cache (`src/cpu/jitv2/pcache.rs`)
be filled **offline**, emitting relocation-free code keyed on a fingerprint plus
build id, with a defined invalidation story — and is the benefit worth it over
the existing side-effect fill?

A small prototype delivers an affirmative feasibility answer:
`src/bin/jitv2_aot.rs` re-runs the real compile front half over a `j2 corpus`
capture and writes production-format blobs into the real cache layout. It is
opt-in and changes no default behaviour. Design background:
[docs/jitv2-persistent-cache.md](jitv2-persistent-cache.md); sibling research:
`docs/research/b02-dbt-granularity-caches.md` (§problem 5, §R6).

---

## What was already true before this spike

The persistent cache is filled only as a side effect of live compilation. What
it stores is already exactly what an offline filler needs to produce:

- **Position-independent code is the default.** Compiled code takes the core
  pointer as a function argument and never bakes a host address unless
  `IRIS_BAKE_HOOKS=1` (with a published core) or `IRIS_MEM_HELPERS` is on.
  `Codegen::cache_fingerprint` returns `None` — disabling caching — when either
  would put an address in the stream (`codegen.rs`).
- **Relocation-free is a hard gate.** `compile_region` records a storable blob
  only when `cc.buffer.relocs().is_empty()` (`codegen.rs:2602-2611`); a page
  with a libcall relocation is simply never cached. `pcache::summary` reports
  these as `refused`. The persistent-cache measurements record **zero
  relocation refusals** over a real IRIX workload, so the gate is rarely the
  limit.
- **The page is not an emitter input.** Branch targets are page-relative,
  `j`/`jal` resolve at run time, and the `PhysicalCodePage*` codegen receives is
  bookkeeping (`pcp_dump.rs`'s "no virtual address here" note). A compiled page
  is fully determined by the page's bytes, its entry set, FR mode, ISA and CPU
  model, and the codegen configuration.
- **Offline compilation already exists.** `jitv2_pcp_dump --compile` walks a
  `.pcp` dump and runs `Codegen::compile_region_uncommitted` with a null page
  pointer, no bus and no `Machine` (`src/bin/jitv2_pcp_dump.rs:238`). The
  harness (`zz_corpus_sizes`, `jitv2/mod.rs`) compiles whole corpora offline for
  code-size measurement.

So the spike is not "can code be emitted without a machine" — that is already
done. It is "can it be emitted under **exactly the production key** and stored
so the emulator loads it". The gap was in the key, not the compiler.

---

## Feasibility: determined — YES

Relocation-free offline page compilation is **feasible and demonstrated**.

The prototype `jitv2_aot` takes a `.pcp` page, walks the union of its
`requested` and `compiled` entry offsets through the real `Analyzer`, runs the
real `Codegen::compile_region_uncommitted`, and stores the relocation-free
machine code with `pcache::fill_blob` under the emulator's build id. It adds
three things the existing offline tools lacked, all on the key side:

1. **Explicit build id.** The cache namespace is
   `<base>/<build-id>/<fingerprint>/…`, and `build-id` is BLAKE3 of the
   *running executable*. An offline tool is a different binary, so it cannot
   use its own hash. `pcache::build_id_of_exe(path)` hashes the emulator binary
   named by `--iris-exe` instead. (`root()` was refactored onto the same
   helper, so both sides agree byte for byte.)
2. **Production cache geometry.** The live worker compiles with the configured
   CPU's `JitDcGeometry`; `jitv2_pcp_dump` compiles with `unsupported()`, a
   different configuration and therefore a different fingerprint. The new
   `jit_dc_geometry_for_model::<M>(tagless)` computes the model's steady-state
   geometry from its cache consts, with no cache instance and no tmem windows —
   functional shape for R4400/R5000, tagless for the R10000 shadow cache.
3. **The fingerprint path.** `jitv2_aot` sets `isa::set_mips4(M::MIPS4)`, the
   opt level and `intrun` from flags, stamps the geometry, then calls the same
   `Codegen::cache_fingerprint(true)` the live worker calls. With
   `IRIS_BAKE_HOOKS`/`IRIS_MEM_HELPERS` off — the production default — the
   output is position-independent and relocation-free, so it carries the same
   key a live compile of the same page would and is loadable by the emulator.

Measured on this machine: a synthetic `.pcp` page (one `addiu`, a `jr $ra` and
its slot) compiled offline for all three CPU models with **0 relocation
refusals**, and was stored as a real `.jc` blob under the exact runtime layout
(`…/<build-id>/<fingerprint>/<page-hash>-0/<entries-hash>.jc`). The `decode`
path a live lookup runs accepts it (`fill_blob_round_trips_through_the_disk_path`).
No real IRIX corpus was available in this session, so no real-corpus hit-rate
number is claimed here; the numbers below are the existing cache's, measured.

The remaining sharp edge is **entry-set selection**, which is a policy problem,
not a feasibility one — see [Entry sets](#entry-sets-the-one-real-choice).

---

## The cache key

Already implemented in `pcache.rs`; unchanged by this spike except that the
build id is now nameable from outside.

| Component | Value | Source |
|---|---|---|
| build id | BLAKE3(exe bytes)[..16], hex | binary source, features, Cranelift version |
| fingerprint | BLAKE3 of `Codegen::cache_fingerprint(true)`'s config string | runtime codegen switches |
| page hash | BLAKE3 of all 1024 page words (LE) | the guest page |
| FR mode | `0`/`1`, a directory component | guest `STATUS.FR` |
| entry variant | first 8 bytes BLAKE3 of the entry bitmap | which offsets the blob serves |

Paths: `<base>/<build-id>/<fingerprint>/<page-hash>-<fr>/<entries-hash>.jc`.
The fingerprint string covers, in `cache_fingerprint`:

- instruction budgets (`max`/`min` instructions per compile),
- opt level, interrupt-run coalescing budget,
- inline-memory and shared-memory-helper switches,
- analyzer fallback and forced entry preamble,
- `skip_entry_preamble` (`true` on the production compile path),
- ISA level (`mips4`),
- **data-cache geometry** (so CPU model is pinned here),
- Cranelift target triple, target flags, and ISA flags (**host features**),
- the per-category opcode enable bitmap (`j2 alu/fpu/branch/loadstore/cop0`).

The **build id** covers everything the fingerprint does not: the codegen source
itself, the Cranelift version, and Cargo features. (`JitConsts` is runtime data,
not compiled-in: when it *would* change emitted code — baked hook addresses —
`cache_fingerprint` returns `None` and nothing is cached, so it never needs a
key.) That split is what makes AOT safe to bolt on: the offline tool does not
need to reimplement the fingerprint, only to reproduce the inputs to it.

### What must be pinned for relocation-free reuse

1. **No baked host addresses.** `IRIS_BAKE_HOOKS` off (default) and
   `IRIS_MEM_HELPERS` off (default); otherwise `cache_fingerprint` returns
   `None` and nothing is stored. `JitConsts.core` may be `Some` with baking
   off — the emitted load-from-core path is identical.
2. **The exact page bytes**, verified by a full 4 KB compare at lookup, not the
   hash alone.
3. **An entry superset**: a stored variant serves any request whose entries are
   a subset of its own.
4. **FR mode**, ISA level, CPU model (via geometry), codegen configuration —
   all in the fingerprint.
5. **The host ISA feature set**, in the fingerprint via Cranelift's target
   flags. This is a *correct* pin (code compiled with AVX is only served where
   the fingerprint matches), but it also means blobs are host-feature-specific,
   not portable across heterogeneous hosts.
6. **AOT only:** the emulator binary's build id, named explicitly by
   `--iris-exe` / `build_id_of_exe`.

---

## Invalidation

The key is content-addressed, so most invalidation is structural — a stale
blob is simply never found under the new key, and never served because lookup
re-checks the page:

- **Page content changes** (SMC, DMA, relocation): the page hash changes path,
  and even a deliberate hash collision fails the full 4 KB compare. Runtime
  generation counters cover the live SMC window; an AOT blob is re-validated
  against the *current* bytes on every lookup, so it can never serve wrong
  bytes.
- **Emulator rebuild**: new build id, new namespace. The old namespace is never
  read and is pruned to the newest `KEEP_BUILDS = 3`.
- **Configuration change** (opt level, `intrun`, category enables, CPU model,
  ISA, host features): new fingerprint.
- **Format change**: `FORMAT` is bumped; a header whose magic/version does not
  decode is discarded (`discard_bad`).
- **AOT-specific**: because the build id scopes a blob to one exact binary, the
  filler must be re-run when the binary changes. There is **no cross-build
  reuse**, by design — the alternative (a looser codegen-content key) would
  have to prove that no input to emitted bytes changed, which the build id
  already guarantees cheaply.

The one thing the key does not defend against is a **deliberately tampered
blob**: the page compare establishes that the stored words match the live page,
not that the stored code is a correct compilation of them. We trust locally
written `.jc` files as executable native code. The runtime cache already has
this property; an AOT/import path would widen the trust boundary, so treat a
distributed blob pack like any other downloaded code (sign or generate
locally). This is a deployment caveat, not a blocker.

---

## Entry sets: the one real choice

Live compilation discovers exactly the entries the guest reaches. Offline, the
blob must contain a **superset** of what a later run asks for, or it is a miss.

- **Corpus-driven (implemented).** Read the observed entry set from a `.pcp`
  capture (`requested` ∪ `compiled`; `requested` alone is not cumulative — see
  `rules/jitv2/corpus-capture-from-the-pcp-cache.md`). No guessing; the only
  assumption is that the next run reaches a subset of the captured set. This is
  the same union-on-miss convergence the runtime cache already relies on, and
  it is what `jitv2_aot` does.
- **From binaries (not built).** Walk kernel text and prelinked shared-library
  pages and guess entries from symbols, branch targets and return sites (the
  persistent-cache doc's "Towards AOT"). Safe in principle — the subset rule
  means an over-broad set never serves wrong code — but an over-broad set makes
  each offline compile larger and its coverage unproven. NO-GO until measured.

---

## Benefit, versus the existing side-effect fill

The existing fill's payoff is measured (`docs/jitv2-persistent-cache.md`,
2026-09-24, IP28/R10000, 4 compile threads):

| | cold | warm | warm 2 |
|---|---|---|---|
| lookups served from cache | 15% | 86% | 95% |
| Cranelift time, all threads | 143 s | 63 s | 27 s |
| boot to `login:` | 42 s | 39 s | 39 s |

and, at the design stage: 97.1% of run 2's compiles were of page content run 1
had already compiled; 82.7% were covered by a run-1 variant; 85.7% with
union-on-miss. A cached page loads in ~8 µs against ~22 ms to compile.

AOT does not make any of that faster; it changes **who pays for the first
compile of a build**:

- **Compute cost is unchanged.** The offline filler compiles at the same
  ~22 ms/page. A ~3,000-page session is ~66 s single-threaded, ~17 s on four
  threads — the same work the cold boot does, moved.
- **The marginal win is first-boot latency.** A fixed binary's *first* run can
  start warm instead of paying the ~120 s of Cranelift time the side-effect
  fill only recovers from the *second* run on.
- **The win is scoped by the key.** Because the build id is the exact binary
  and the fingerprint carries host ISA flags, an AOT-filled cache benefits
  (a) repeated runs of one unchanged binary on one host, and (b) release/CI
  where the same build is run many times — provided the host feature set
  matches, or the build forces a conservative target baseline.
- **#38 gates the payoff.** The compile backlog's cause is not only cold
  compiling; arena churn from repeated flush/compile degrades throughput, and
  #38 is the fix for that. An AOT fill addresses the first-boot queue, not the
  churn, so it should land after #38 to be measurable against a clean
  baseline.

Net: AOT is a **scheduling** win (move compile work off the guest's critical
path), not a compute win. It is worth it only where the same build is run many
times and the offline compile can happen off the critical path — a
post-build/release step or an idle daemon.

---

## Decision

**GO — feasibility.** Relocation-free offline page compilation is determined to
be feasible and is demonstrated by `jitv2_aot`: real codegen, real fingerprint,
real on-disk format, zero relocation refusals, loaded by the same `decode` a
live lookup uses.

**GO — bounded, opt-in filler.** Keep `jitv2_aot` (and the `pcache::fill_blob`
/ `build_id_of_exe` seam) as a corpus-driven offline filler behind the existing
`[jitv2] cache` configuration. It changes no default behaviour and cannot serve
code the live cache would not have compiled itself.

**NO-GO — from-binaries AOT, default-on AOT, shipped prewarmed caches.**
Reasons, in order: entry-set guessing is unproven; the build-id namespace means
no cross-build reuse, so the value is narrow; host-ISA-flag scoping limits
pack distribution; and #38 is the higher-value throughput fix and is still
open. Revisit when all of these hold:

1. #38 has landed and the flush/compile churn baseline is clean.
2. A release/CI experiment shows a real first-boot wall-clock win with an AOT
   fill on the critical path's off-hours, on the same binary and host.
3. Either a forced conservative target baseline (so one blob pack serves
   heterogeneous hosts) or an accepted per-host fill step.

Until then the existing side-effect fill is the right default. The blob format
(`FORMAT = 1`) should be treated as frozen; a from-binaries compiler, if ever
built, is a separate decision with its own measurement.

---

## Prototype surface (this spike)

- `src/cpu/jitv2/pcache.rs`: `build_id_of_exe`, `fill_blob` (both public); the
  writer refactored so runtime and offline writes share one `write_blob_to`.
  New test `fill_blob_round_trips_through_the_disk_path`.
- `src/cpu/mips_cache_v2.rs`: `jit_dc_geometry_for_model::<M>(tagless)`.
- `src/bin/jitv2_aot.rs` (`--features jitv2`): the filler. Reads one `.pcp` or
  a directory, reports `pages/entries/compiled/walk_declined/codegen_declined/
  relocation_refused/stored/bytes`, resets the arena on a byte budget.

Usage:

```
jitv2_aot <corpus> --iris-exe <iris-binary> --cache-dir <base> \
          [--cpu r4400|r5000|r10000] [--fr1 0|1|auto] \
          [--opt speed|none] [--intrun N] [--dry-run] [--build-id <hex>]
```

Deferred to a follow-up, not part of this spike: wiring the filler into the
build/release pipeline, a real-corpus hit-rate measurement, and forcing a
portable Cranelift target baseline.
