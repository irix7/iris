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
