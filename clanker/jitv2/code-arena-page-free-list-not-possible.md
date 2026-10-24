# A page-granular free list for the jitv2 code arena is impossible under the sealed-watermark / W^X model

Status: finding, 2026-10-09. Supersedes the "not delivered" half of issue
#38 (`impl/38-freelist`). Read with `jit-v2-design.md` §6.3/§12.3.4 and
`docs/research/b02-dbt-granularity-caches.md` (recommendation 1).

## What was asked

Issue #38 wants "a page-granular free list" for the transient compiled-page
code arena, so a flush can recycle pages incrementally instead of the whole
arena, without discarding helpers and without breaking the sealed-watermark /
W^X invariants.

## What already exists (the achievable half)

- Permanent/transient split: `Jitv2::perm_helpers` (`src/cpu/jitv2/jitv2.rs`)
  owns a helper-only `Codegen` whose arena is never reset, so helper
  addresses survive every flush. The transient compilers get those addresses
  via `Jitv2::ensure_perm_helpers` / `Codegen::set_mem_helpers`.
- Whole-transient-arena reclamation on flush: `run_leader_flush`
  (`jitv2.rs`) builds one fresh shared arena and rebuilds every worker's
  `Codegen` on it; `Codegen::release` frees the retired arena
  (`jitv2.rs`/`codegen.rs`). Helpers are deliberately *not* rebuilt into the
  fresh arena (`Codegen::reset_inner` preserves `mem_helpers`).
- A per-guest-page metadata free list: `Jitv2::free_head`/`free_page`/
  `page_for` over `PhysicalCodePage` slots. This recycles *page identity*,
  not code bytes.

## Why a page-granular *code-arena* free list cannot be added

`SharedArena` (`src/cpu/jitv2/paged_memory.rs`) is a single-map, monotonic
bump arena:

1. `allocate` (l.627) only ever grows `position` forward and **skips past**
   `sealed_up_to` (l.631-632). It never hands out a byte below the watermark.
2. `try_seal_ready` (l.735) and `seal_prefix_no_publish` (l.718) only ever
   *advance* `sealed_up_to`; nothing moves it back.
3. Sealing is one-way, `set_readable_and_executable` (l.221): RW -> RX via
   `region::protect(..., READ_EXECUTE)`. There is **no RX -> RW path
   anywhere**. `region::Protection::READ_WRITE` appears exactly once, in
   `allocate`, for newly committed bytes above the watermark.
4. `free_memory` (l.840) / `Drop` release only the *whole* reservation; a
   live (already-sealed) arena is deliberately leaked.

Therefore every byte below `sealed_up_to` is permanently unreachable to the
allocator. A true page-granular free list would have to make a dead, sealed
page allocatable again, which requires one of:

- `mprotect` RX -> RW and re-seal (an explicit W^X violation window), or
- a dual RW/RX mapping (`jit-v2-design.md` §12.3.2's adopted-long-term idea)
  that lets a "sealed" page be rewritten.

Both contradict "sealed memory is permanently off-limits". There is also no
liveness map from arena bytes to guest pages: `func_ranges` is per-`FuncId`
inside one `Codegen` and is dropped at finalize; packing puts many functions
on one host page, so "this page is fully dead" is not even cheaply
decidable. This is why the design already names **generational arenas**
(`jit-v2-design.md` §12.3.4 item 4: "young/old Codegen modules, retire the
old wholesale") as the real epoch reclamation, not per-artifact frees.

## What is actually available, in order of granularity

1. **Page metadata recycling** (already present, currently unused at
   runtime): wire `free_page` into the pool-exhaustion path so
   `PhysicalCodePage` slots are evicted and reused one at a time instead of
   `mega_flush` resetting every slot. This is page-granular over *compiled
   page identities*; the sealed code bytes still leak until the arena
   flushes. It reduces how often a full flush happens, not arena pressure.
2. **Whole-transient-arena reclamation** (already shipped): the flush path.
3. **Generational arenas** (not built): N arenas, retire a whole arena once
   all its code is dead. Real code-memory reclamation, but arena-granular,
   not page-granular.

A page-granular free list for the *executable* arena is not on this list
because the monotonic-seal invariant rules it out.

## Consequence for issue #38

The "page-granular free list" acceptance criterion should be either narrowed
to (1) the metadata pool, or refiled as (3) generational arenas. Do not add a
free list that reuses sealed pages; it cannot be made correct under the
existing W^X model.
