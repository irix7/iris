# jitv2 cold-page compile latency can outlive a short kernel

**Status:** measured (2026-10-26). Corrects the first diagnosis in #112.

## Rule

A code page is requested and published correctly. What can go wrong is
**latency**: on a cold page the first publish can land after a short kernel has
already finished, so the whole timed run is interpreted. Do not read a `j2 pcp`
snapshot taken before the page executes as "never requested"; poll it while the
kernel is actually running.

Measured on upstream `fba85e7` (R4400, `lightning,jitv2`, `--cache shadow`,
i5-8500T): the same conversion kernels run at 133–169 ns/op with each entry on
its own page, and 15–18 ns/op when packed into pages shared with boot-time
harness code. 40/40 accuracy in both shapes.

## Evidence

- `j2 pcp` before the kernel's page executes: `0 / 1024 requested`, `func=0`.
  A moment later, same page: `requested=true`, then `published=true`,
  `entry_valid=true`. It was an idle-page artefact.
- Per-page counters over a run: one dispatch (`jit=1`) against millions of
  interpreter fall-throughs (`interp=11.7M`, `12.5M`), so the compiled code was
  healthy when entered but almost never entered.
- Compile worker: request handling took 2.7, 17, 67, 74, 110 ms; the queue was
  10–13 deep whenever a page was popped; 248 seals over ~50 pages, i.e. each
  page recompiled ~5× because every newly discovered entry offset triggers a
  full-page recompile.
- Cold compile + seal was 13.6 ms in the quiet case, but queueing and seal
  delay stretched the first publish to 0.2–1.5 s under load. The timed kernel
  lifetime is ~130 ms.

## Lever not yet proven

Upstream's default is one worker (`default_jitv2_threads() -> 1`); the fork's
`9d1988a` already uses `min(available_parallelism, 4)`, so fork builds run four.
The sweep below was on upstream with an explicit override.

Explicit `[jitv2] threads = 4` made most aligned kernels fast in one sweep, and
2 was marginal. But it is not a clean win: four workers add 1–3 s to the 38 s
suite, regress packed `cvt_s_d` 129 → 143 ns/op in one cell, and leave one
aligned run at 103.6 ns/op. Inline compile reaches 10–15 ns/op but destroys
`int/alu` (2.8 → ~960 ns/op) and doubles the suite's wall time.

The timeline shows the queue is the cost: 501 ms of the 529 ms first-publish
latency is waiting behind other pages' compiles. The compile itself is 13.6 ms
and seal -> publish 14 ms, so a shorter seal timer cannot fix it. The next
design levers are compile throughput (workers, smaller compiles) and avoiding
a full-page recompile for every newly discovered entry.

## Reproduction

The bench-kernel patch that puts each conversion entry on its own page is
[cold-page-compile-latency.patch](cold-page-compile-latency.patch). Apply it,
rebuild the guest image with clang + `ld.lld` (the recipe is in #112), run
`iris-bench run`, and poll `j2 pcp <va>` while the kernel is running. The
packed shape is the same image with `-DCVT_ALIGN=1`.

This also explains the layout noise seen in the FPU and conversion benchmark
campaigns: kernels sharing a page with already-compiled harness code are fast,
kernels alone on a cold page may never be compiled in time.
