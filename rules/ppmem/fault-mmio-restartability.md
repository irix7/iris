# Fault/restartability invariant for partially-completed stores

Any store that can partially complete before faulting must **compute-then-commit
or record undo**.

## Why

A guest store can be wider than the gap to a page boundary. If the host commits
the words on the present page and then faults on a trapped (device) page,
re-executing the guest instruction after servicing the fault commits the
present page's bytes a second time. The result is a torn write that no retry
can repair — the guest never observes the single atomic store it asked for.

This is the invariant Dolphin encodes when it rewinds `nonAtomicSwapStoreSrc`
and the LEA displacement before retrying a faulting JIT load/store
(`Source/Core/Core/PowerPC/Jit64/Jit.cpp:166-254`), and it is the memory-side
twin of `rules/testing/device-write-must-honour-bus-busy.md`'s BUS_BUSY
re-execution rule.

## What to do

Either:

1. **Compute-then-commit (decompose).** Split the store so every committing
   step lies wholly within one page and is all-or-nothing, and commit the
   possibly-faulting piece **last**. A single aligned store within one page
   cannot partially commit, so no retry can tear it.
2. **Record undo.** If the store cannot be decomposed, save enough state at
   codegen (the original destination bytes, the destination register and
   offset/size) for the fault handler to roll back the partial write before
   re-executing.

## Where this already matters

- `PpMemory::write64_masked` (`src/ppmem/ppmem.rs:602-611`) is a
  read-modify-write of one qword.
- `swap_word_halves_store` (`src/ppmem/ppmem.rs:443-453`) writes a multi-word
  block.
- `jitv2` inline/callout stores and any future fault-based MMIO path.

None of these currently crosses a device mapping, so the hazard is latent, not
live. Honour the rule before any of them is allowed to touch a trapped or
otherwise partially-present page.

## Regression net

`src/faultmmio.rs` (feature `faultmmio`, off by default) carries the executable
form: `naive_boundary_store_can_tear` shows the hazard, and
`validated_boundary_store_is_atomic` shows the fix. The viability reasoning and
the go/no-go live in `docs/ppmem-fault-mmio-spike.md`.
