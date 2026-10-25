# jitv2 FP arithmetic: prove flags away, and measure one kernel per image

**Rule (code).** Two ordinary finite operands provably raise no FP flag. The
jitv2 FP add/sub/mul/div/sqrt path classifies the operands once; if both are
finite and normal it skips the per-operand denormal/qNaN checks, the sNaN flag
computation and the per-op Cause store. The slow path (zero, denormal, inf,
NaN, negative sqrt, divide by zero) stays the previous body verbatim, out of
line. Skipping the Cause store when Cause is already clear is unobservable and
removes a store→load chain through `core.fpu_fcsr`.

Measured on `upstream/main` (PR techomancer/iris#328): `fadd.s`/`fmul.s`
1.50–2.04×, median ~1.9×; `div_s` 2.03× threaded / 1.17× dependent (host
divider latency); `sqrt_d` 1.76×. Static: median x86 instructions per
`vaddss` 106 → 74. Semantics: the jitv2 equivalence suite (whole core
snapshot incl. `fpu_fcsr`), a new clear-Cause/keep-Flags test, the cpu-tests FPU
suite identical, full workspace 1429 passed.

**Gotcha (measurement).** The study's multi-kernel probe image is
layout-sensitive: several hot loops share one compiled guest page, and
*untouched* kernels moved up to 3× between builds. The root cause is now
known: a page-aligned entry can fail to request a compile and run interpreted
for the whole run (issue #112). Measure FP kernels with one kernel per image
when attributing a change to the model; keep a control kernel in the same image
to detect drift.
