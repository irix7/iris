# Scalar RAM access bypasses the ppmem window; wire the bitmap into read/write

The ppmem fast path ("is this RAM? direct host access : dispatch") is wired only
into the bulk operations — `mem_ptr` (`physical.rs:1144`), `read_block` (`1153`),
`write_block` (`1169`), `gen_ptr` (`1125`) — and into the JIT's transparent-cache
path. The scalar accessors do **not** use it:

```rust
// src/physical.rs:1042-1044 (read32; read8/16/64 and all write* are identical)
let device_ptr = self.device_map[(addr >> 16) as usize];
let r = unsafe { (*device_ptr).read32(addr) };
```

So an uncached (KSEG1) guest access — or any interpreter cache-miss fill — pays
two `dyn BusDevice` dispatches (`Physical` → `PpMemory`) plus `PpMemory`'s
`addr_mask` atomic load (`ppmem.rs:266`) per access, even though the address is
plain mapped RAM.

## Prior art: this test is universal

Every system emulator resolves RAM-ness once and takes a direct pointer for RAM,
dispatching only MMIO: QEMU's TLB carries a RAM flag + host offset
(`memory.rst`, `tcg.rst` — "access is faster for RAM and ROM because the
translation cache also hosts the offset"), MAME's address map yields either a raw
`ram()` pointer or a `.r()/.w()` handler. Crucially, **uncached RAM is still RAM**
— no emulator routes uncached/KSEG1 traffic off the direct path.

The rejected alternative — mapping MMIO pages unmapped and trapping device
accesses via SIGSEGV/Mach/VEH — is *not* what anyone does in software: a host page
fault is ~µs vs ~1 ns for a predicted branch (~10³–10⁴×), the faulting thread
blocks, and signal handlers are hostile to per-device locks. QEMU uses faults only
for rare events (postcopy, self-modifying code). See `docs/ppmem-design.md` §11.

## Suggested change

Add the bitmap test to the top of `Physical::read8/16/32/64` and
`write8/16/32/64`, mirroring `ppmem_ptr` (`physical.rs:817`) but at byte
granularity with the correct swizzle (the tcache path's `tc_read`/`tc_write` in
`mips_cache_v2.rs` already encodes the byte-indexed swizzles). Writes must still
bump the jitv2 generation counter for the page (`ppmem_bump_gen_range`,
`physical.rs:839`).

Two caveats before relying on it:

1. **Granularity.** The bitmap is one bit per 64 MB (`BITMAP_SHIFT`). A 64 MB
   region that mixes RAM and MMIO cannot be expressed and would need to fall back
   to `device_map` for the whole region. Audit the physical map for mixed regions
   (SIMM mirroring near bank boundaries, KSEG1 holes); if any exist, either shrink
   the bit (e.g. 4 MB ⇒ a 128-byte bitmap) or add a per-region fallback flag.
   `docs/ppmem-design.md` §3 already lists LOMEM/HIMEM as the clean cases.
2. **The JIT's cached path already bypasses via tcache.** The bitmap's marginal
   value is the scalar interpreter path — uncached/KSEG1 accesses and cache-miss
   fills. Do not add the test to the already-fast cached path.

Measure against `sys/uncached` in `bench/`, which is the kernel that isolates this
exact cost.
