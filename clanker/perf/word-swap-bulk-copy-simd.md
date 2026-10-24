# The word-swap bulk copy should be a SIMD dword shuffle, not a rotate loop

Guest RAM stores each u64 `rotate_left(32)` (word-swapped; endianness at The Edge).
Bulk copies do a scalar rotate loop per word:

- `src/ppmem/ppmem.rs:479-497` (`read_block`/`write_block`)
- `src/physical.rs:1153-1184` (the ppmem-pointer fast paths)

`dst[i] = src[i].rotate_left(32)` — a 256 KB DMA chunk is 32768 load-rotate-store
iterations. These run on cache-line fills/writebacks and VDMA staging.

## The better instruction

The 32-bit half-swap is a *dword shuffle*, not a byte-shuffle: swap dword0↔dword1
and dword2↔dword3 within each 128-bit lane. That is exactly `PSHUFD` with order
`0xB1`, available from the **SSE2 baseline** (no feature gate):

```rust
// x86-64: _mm_shuffle_epi32(v, 0xB1) (SSE2) or _mm256_shuffle_epi32(v, 0xB1) (AVX2)
// aarch64: REV64.32  — the same operation natively
```

`0xB1` = `_MM_SHUFFLE(2,3,0,1)`: `dest[0..4] = src[1,0,3,2]` (PSHUFD reads the
imm's 2-bit fields LSB-first). No mask vector, no SSSE3, no `vpermd` (that would
reorder whole u64s — wrong). `pshufb` works but needs SSSE3 and a mask.

**Gotcha (bitten here):** `0x4E` is the *wrong* immediate for this — it encodes
`dest[0..4] = src[2,3,0,1]`, which swaps the two u64s in a lane rather than the
two 32-bit halves of each. The "swap halves" encoding is `0xB1`. The original
note transcribed the bit order MSB-first and got `0x4E`; the ppmem unit tests
(`matches_memory_byte_for_byte`, `window_block_access_matches_busdevice`) caught
it.

Portably (IRIS is already nightly), `std::simd` with
`simd_swizzle!(v, [1,0,3,2,5,4,7,6])` lowers to `vpshufd`/`REV64.32`. Do **not**
use `Simd::swap_bytes` — that is a full byte-reverse, the wrong operation.

## Why the rotate loop is not auto-vectorised well

LLVM lowers `rotate_left(32)` to `llvm.fshl`; the vector form on x86 lowers to
`vpsllq`+`vpsrlq`+`vpor` (three ALU ops per vector — no vector rotate before
AVX-512), and LLVM's loop-idiom pass does not pattern-match a half-swap into a
shuffle. So today's loop is ~3× the ALU work and ~4× the iterations of the optimal
form. The operation is an involution, so one routine serves both fill and writeback.

## Where the win is

- Cache-line fills/writebacks (32-128 B): L1-resident, latency/instruction-count win.
- DMA staging (256 KB): L2/L3-resident → uop-bound → the ~4× uop reduction is real.
- DRAM-scale copies: memory-bound; only the wider load/store width helps.

## Notes

- The no-rotation case (copying between two already-swapped buffers) can use
  `ptr::copy_nonoverlapping` directly.
- Prior art is thin because mature emulators swap per-accessor (QEMU `bswap.h`,
  glib `GUINT32_SWAP_*`), not bulk buffers — IRIS's bulk-swapped layout is the
  unusual case, which is why this is an under-exploited win here.
