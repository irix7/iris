# The interpreter dispatches via call threading — the slowest portable technique

**Hypothesis to measure, not an established win.** This is grounded in external
research (Ertl's dispatch microbenchmark), not yet measured on IRIS. Do not
implement without an `iris-bench` before/after.

The interpreter's per-instruction dispatch is an indirect *call*:

```rust
// src/cpu/mips_exec.rs:9129-9131 (exec_decoded_int)
type Fn<T, C> = fn(&mut MipsExecutor<T, C>, &DecodedInstr) -> ExecStatus;
let f: Fn<T, C> = unsafe { std::mem::transmute(d.handler) };
f(self, d)
```

`d.handler` is a type-erased fn pointer stored in the decoded slot
(`DecodedInstr.handler: RawInstrFn`, `mips_exec.rs:810/831`). That makes this
**call threading**: one indirect `call` + `ret` per guest instruction.

## Why it is slow

The canonical dispatch comparison (Anton Ertl's threading microbenchmark,
`complang.tuwien.ac.at/forth/threading/`), cycles/dispatch:

| technique | Golden Cove | Zen 4 |
|---|---:|---:|
| direct / indirect threading (computed goto) | 1.14–1.19 | 2.0–2.1 |
| switch threading (`match` over opcode) | 1.68 | 4.7 |
| **call threading (this code)** | **5.58** | **24.0** |

Two costs compound:
1. The `call`/`ret` pair itself.
2. The indirect call is a full compiler barrier, so the executor's live state is
   spilled to memory before every call and reloaded after — cross-instruction
   optimisation (store-to-load forwarding of the GPR array, CSE) is destroyed at
   every instruction boundary.

The "decode once, dispatch many" cache (decoded slots in L1I/L2) is sound and
orthogonal — it amortises *decode*, it does not amortise the dispatch call.

## The folklore correction

"`match` loops are much worse than computed goto" is 2001-era folklore. Rohou &
Seznec ("Branch prediction and the performance of interpreters — Don't trust
folklore", CGO 2015) show modern ITTAGE predictors predict a *shared* dispatch
branch accurately by correlating with history, so switch threading is within
~45% of direct threading on Golden Cove, not 3–5× behind as on a Pentium 4.

## Suggested change

Switch threading: keep the decode cache and `DecodedInstr`, but dispatch with a
`match` over a small `#[repr(u8)]` opcode enum instead of a fn pointer. The
handler bodies move into the `match` arms. Rust 1.87 stabilised `asm!`
`label<block>` (issue #119364), so true computed-goto/direct threading is also
available if the `match` version still shows dispatch-bound behaviour.

## Notes

- `opcodefusion`'s value is partly masking this: fused pairs reduce the number of
  call-threading dispatches. Re-evaluate it *after* switching dispatch, against
  the new baseline, rather than assuming it still pays.
- The JIT path (`exec_decoded`'s gate) is unaffected — this is the pure
  interpreter (`step_int` → `exec_decoded_int`) and the JIT's interpreter-fallback
  heads.
- Report any measured delta against `bench/` (`iris-bench run`), since a live
  IRIX boot is I/O-bound and may under-state the interpreter's dispatch cost.
