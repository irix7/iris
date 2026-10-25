# jitv2 integer conversions: inline the common case, call out the proofs

**Status:** merged upstream as techomancer/iris#332 (`fba85e7`), 2026-10-25. The four conversion
families (`CVT.W/L.S/D`, `ROUND/TRUNC/CEIL/FLOOR.W/L.S/D`, `CVT.S/D.W/L`) were
the last FPU instructions compiled to a per-op `call_indirect`; they cost
17–19 ns against 3.1 ns for the already-inline `cvt.d.s`.

The common case is now inline: a rounding step, a range test against the
interpreter's own bounds, a saturating cast, one Cause store and the FPR write.
The unchanged callout runs, before any guest state is written, for NaN, ±inf,
out-of-range, non-RN-even rounding, and every Invalid/Inexact trap.

Rules that make it exact:

- **RN-even via magic constants**, only while `|x| < 2^mantissa`: add/subtract
  `2^23` (F32) or `2^52` (F64) in the source's sign direction. Above that every
  value is already integral. This holds only when host MXCSR is RN-even, which
  `emit_fpu_set_mode` guarantees on every FCSR write; guard `FCSR.RM == 0`.
- **Integer → float writes the destination before the Inexact check** — the
  interpreter leaves the converted value in the FPR when the trap fires.
- **Inexact in range is unreachable** for float → int: the rounded value is
  integral, so the round-trip compare cannot fire. That includes the `+2^31`
  edge an F32 source is admitted at.

Measured (upstream, R4400, `lightning,jitv2`, per-instruction probe and guest
kernels): `cvt.w.s` 2.71×, `cvt.l.d` 2.74×, `cvt.l.s` 3.06×, `trunc.w.s`
1.90×; int → float 1.5–1.7×; the untouched `cvt.s.d`/`cvt.d.s` controls flat.
Guest `cvt.w.s`/`cvt.l.d` kernels 1.76×. Whole workspace 1755 passed.

**Gotchas.** The magic-number round is invalid at the boundary: `2^23 + 1`
came back as `2^23` until the `|x| < 2^23` select was added — the edge test
caught it. And do not attribute a guest-kernel regression to a conversion
emitter without a per-instruction probe: a page-aligned entry can run entirely
interpreted (issue #112), and one shared code page makes layout noise larger
than the change.
