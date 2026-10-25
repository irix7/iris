# jitv2 inline memory: one shifted compare for KSEG0 wins

**Upstream status: rejected for now.** PR techomancer/iris#330 was closed on
2026-10-25. The objection: on a working system most accesses are user-space
KUSEG, and this adds a compare to every one of them; the bare-metal benchmark
suite is all KSEG0, so 1.29x is a ceiling, not a prediction. Re-propose only
with a user-space benchmark and a shape that does not tax KUSEG. The fork does
not carry this change.

**Rule.** The inline load/store guard first tests
`(va >> 29) == core.jit_kseg0_rule`; on a match `phys = va & 0x1FFF_FFFF`, and
everything downstream (cache model, VCE, aliases, generations) is the unchanged
code. On a miss the full nutlb probe runs. The rule is derived state, refreshed
from `Status` through `resync_privilege_state` and from `Config` through
`handle_cp0_side_effects`; it requires kernel privilege, the sign-extended KSEG0
shape and a cached `Config.K0` in the model's table. Anything else stores the
sentinel `1 << 63`, which `va >> 29` cannot produce, so the access falls back to
the probe.

`0xFFFF_FFFF_8800_0000 >> 29 == 0x7_FFFF_FFFC` — one compare covers both halves
of the address test. **Write the constant with the pinned test's helper**: it is
easy to write one `F` short, which silently disables the whole path (it was
initially wrong in this work).

**Numbers** (upstream/main, r4400, `lightning,jitv2`, shadow, 3 interleaved
runs): whole suite 1.29×, DMIPS 1.63×, L1 scalar loads 1.67×, random 1.96×,
copy 1.45×, fill 1.61×, stream 1.32×, DRAM chase 1.09×. Full-cache suite 1.02×.
The all-callout `sys/tlb_hit` and `sys/uncached` kernels pay 3–10 % — the trade
the study predicted.

**Dead ends.** A per-access predicate lost (375 vs 498 MIPS) and a region-entry
hoist lost (428–503 MIPS); only the mirrored field won. Do not re-try those
shapes.

**Verification.** jitv2 equivalence 1752 passed; workspace 1351 passed; four
`jitv2_lockstep` failures are pre-existing (identical on upstream without the
change). Live IRIX was not booted (no safe disk image on the measurement host).
