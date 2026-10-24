# Weaken the interrupt load, don't coarsen the check

Companion to `interrupt-check-frequency-gates-gpr-forwarding.md`, which settled
that `j2 intrun` coarsening shrinks emitted code but does **not** speed up a live
boot. This note is the follow-on the external research points at instead.

The per-instruction pending-interrupt preamble (`emit_pending_interrupt_preamble`,
`src/cpu/jitv2/codegen.rs:2580-2632`) emits a **SeqCst** `atomic_load` of
`core.hot.interrupts` (line 2603). Its own comment (2594-2598) notes this is
*stronger* than the interpreter, which reads the same word with
`Ordering::Relaxed` — and that the whole point was to stop `opt_level=speed` from
hoisting a stale snapshot.

## The finding

A SeqCst load is a full compiler alias-analysis barrier (LLVM Atomics: "alias
analysis will return ModRef for anything Acquire or Release"; optimisers treat
SeqCst like an opaque call), but on x86 it is a plain `MOV` — no fence. That
barrier is what blocks Cranelift's store→load forwarding of GPR values across
the instruction boundary, which is the *actual* forwarding loss the coarsening
experiments were chasing.

The ordering is over-strong for what the flag is: a single-writer, polled bit
that only needs to be *observed eventually*. **V8 reads its interrupt flag
(`StackGuard`) with `Relaxed_Load` for exactly this reason** (its docs state the
relaxed choice is deliberate, "to avoid an expensive memory fence"). LLVM's
Atomics doc and the Linux kernel memory model (LKMM) both confirm: compiler
barrier = ordering, not load; on x86 a SeqCst *load* emits no hardware fence.

## Suggested change

Emit the preamble's interrupt load with **Relaxed** (matching the interpreter)
instead of SeqCst. This should restore Cranelift store→load forwarding of the GPR
array with **zero coarsening and no correctness loss** — the same effect `j2
intrun` was trying to buy, but applied at the right place (the ordering, not the
frequency).

Caveats before believing it:

- Cranelift's alias analysis is coarse (wasmtime issue #4166: four memory
  classes). A relaxed flag load and the GPR spill slots may still land in
  different categories, or a `call_indirect` may intervene — so the win is not
  guaranteed. Measure emitted load counts (as
  `what-actually-blocks-gpr-forwarding.md` does), not just total bytes.
- The flag must genuinely be single-writer/eventual-visibility. Confirm nothing
  relies on the SeqCst ordering across the flag load and neighbouring memory ops
  (the original comment's hoisting concern still applies to a *plain* load; a
  relaxed *atomic* load is the correct middle ground, since it cannot be
  reordered with itself but does not fence other accesses).
- Verify under `jitv2_lockstep` (which keeps per-instruction emission) that
  correctness is unchanged, and re-run `iris-bench run` for the throughput delta.

## Result (measured 2026-10-08, offline)

Ran the in-tree probe at `opt_level=speed`
(`IRIS_RUN_CL_PROBE=1 cargo test --features jitv2 zz_cl_forwarding -- --nocapture`)
against the committed plain-load code (9d1988a). The probe's `barrier`/`sidexit`
shapes still emit the *old* `atomic_load`; `exitbr`/`exitbr2` (a plain
`MemFlagsData::trusted()` load + `brif` to a cold block) are the faithful proxy
for the new plain load — same flags, differing only in field width/offset, which
alias analysis does not distinguish.

Decisive signal, store then load of the same GPR with the check in between:

| shape | check between store→load | reload of gpr[a]? |
|---|---|---|
| plain | none | no — forwarded (`leaq 8(%rsi), %rsi`) |
| barrier | seqcst `atomic_load` | **yes** (`addq 0x88(%rdi), %rsi`) |
| sidexit | `atomic_load` + brif | **yes** (`addq 0x88(%rdi), %r8`) |
| split_barrier | `atomic_load` across a jump | **yes** |
| exitbr | plain load + brif | **no** (`leaq 8(%rsi), %r8`) |
| exitbr2 | plain load + brif, cold arm reads gpr | **no** — rematerialized in the cold arm |

The plain load unblocks forwarding: the seqcst `atomic_load` shapes reload gpr[a]
after its store; the plain-load shape does not. The committed change therefore
achieves the goal with zero coarsening.

Not yet measured: a real before/after on emitted bytes/loads over a corpus — that
needs a live-boot `j2 corpus` capture (no `.pcp` corpus is checked in), and a true
A/B needs the pre-change tree (1528d70), which the current working tree's
uncommitted work from other agents makes unsafe to check out. `iris-bench` and
`jitv2_lockstep` verification likewise still pending.
