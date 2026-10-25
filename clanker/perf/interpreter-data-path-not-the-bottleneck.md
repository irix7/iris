# The interpreter's data-path split is not worth it

**Status:** measured, negative, 2026-10-25. Do not retry without a different
seam.

The performance study ranked "inline the interpreter's load/store hit path,
out-line the miss" second. Three behaviour-preserving shapes were built on
`upstream/main` (R4400, `lightning`, interpreter only, whole suite):

| shape | full cache | shadow |
|---|---:|---:|
| A: executor split only, cold fills | −6.5 % | +3.4 % (not interleaved) |
| B: A + split `CpuCache::read`/`write` at the L1-D seam | −1.3 % | — |
| C: B with `read_data`/`write_data` `#[inline(never)]` | −6.1 % | +1.3 % |

`perf` explains it: the entire load/store handler and data path is **13.2 %** of
the suite (L2 fill/writeback another 6.8 %), so perfect elimination has a
**1.15×** ceiling. A hit is ~90 host instructions and the split addresses ~20 of
them. Shape B removes 2.6 % of host instructions per guest instruction but adds
1.4 % cycles (IPC 3.10 → 2.97): the removed frame/`Result` work was overlapped
with the nutlb → L1-D tag → data → GPR chain.

**Placement noise is larger than the effect.** Marking `fill_l1d_line` cold cost
~5 % on the full-cache suite, and B vs C (one `#[inline]` apart) measured 5 %
apart. Any interpreter change here needs interleaved A/B runs and a control
kernel, not a single before/after.

**Rule.** Spend interpreter effort on dispatch (the 63–73 % self time) or the
cache model, not on splitting the data path. The evidence (diffs, 47 raw runs,
perf CSVs) was produced in the `perf/interp-data` worktree and is not kept.
