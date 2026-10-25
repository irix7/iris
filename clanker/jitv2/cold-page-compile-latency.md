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

Explicit `[jitv2] threads = 4` made most aligned kernels fast in one sweep, and
2 was marginal. But the default is already `min(available_parallelism, 4)` —
4 on this host — so that sweep does not prove "more workers by default"; host
load (2.4–4.1 from other sessions) may explain the difference. The next
experiment is one ordered per-page timeline (`REQ → POP → compile → SEAL →
PUB`, plus queue depth), with a controlled A/B of default threads and
`j2 inline on`.

## Reproduction

The bench-kernel patch that puts each conversion entry on its own page is
[cold-page-compile-latency.patch](cold-page-compile-latency.patch). Apply it,
rebuild the guest image with clang + `ld.lld` (the recipe is in #112), run
`iris-bench run`, and poll `j2 pcp <va>` while the kernel is running. The
packed shape is the same image with `-DCVT_ALIGN=1`.

This also explains the layout noise seen in the FPU and conversion benchmark
campaigns: kernels sharing a page with already-compiled harness code are fast,
kernels alone on a cold page may never be compiled in time.
