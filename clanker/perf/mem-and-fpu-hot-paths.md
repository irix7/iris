# Guest memory and FPU hot paths: what is actually slow

Status: measured 2026-10-25 on `perf/mem-fpu-study` (`bde03fe`). No product
change. The micro-probe kernels used for the isolated measurements were built in
a scratch copy of `bench/` and are not committed; the standard-suite numbers are
from the checked-in suite.

## The short answer

**Memory is not "really really slow" in the recommended JIT build, but it is in
the interpreter with the full cache model. FPU is genuinely disproportionate,
and the cost is the model around the arithmetic, not the arithmetic.**

| workload | interpreter, full cache | jitv2, shadow | host | what it means |
|---|---:|---:|---:|---|
| scalar load, 8 KB (L1) | 42.9 ns/access | 15.8 ns/access | 0.18 ns/access | JIT is 8x an integer op (5x per guest instruction); interpreter is dispatch-bound |
| scalar store, 8 KB (L1) | 42.8 ns/access | 18.6 ns/access | 0.36 ns/access | JIT pays ~2.8 ns/store for the generation bump/dirty path |
| pointer chase, 8 KB (L1) | 51.7 ns/load | 14.3 ns/load | 1.41 ns/load | same shape: fixed per-access work, not DRAM |
| pointer chase, 8 MB (DRAM) | 271.1 ns/load | 102.1 ns/load | 89.7 ns/load | JIT is at host DRAM latency; the interpreter's L2 model doubles it |
| `mem/copy` | 38.2 ns/byte | 2.1 ns/byte | 0.02 ns/byte | the full L2 data model is the interpreter's memory problem |
| FP add, single | 29.8 ns/op | 8.0 ns/op | 0.17 ns/op | JIT is 4x an integer op, 47x host, and latency == throughput |
| FP add, dependent chain | 30.1 ns/op | 8.4 ns/op | 1.41 ns/op | host FPU latency is completely hidden |

The two sentences that matter:

* **Interpreter**: every guest instruction costs ~18 ns of generic
  fetch/decode/dispatch, so a scalar load at 2.5 guest instructions costs
  ~43 ns whether it hits L1 or DRAM. The memory *model* only becomes visible
  when it misses (L2 fills, DRAM chase) or when the kernel streams (copy,
  stream, fill).
* **jitv2**: integer instructions cost ~1.2 ns, FP and memory instructions cost
  ~6.3 ns each. FP is 5x an integer instruction because a guest `fadd.s` emits
  ~109 x86 instructions, one of which is `vaddss`. Memory is 5x because the
  inline load/store guard re-translates and re-checks every access.

Absolute verdict: JIT memory latency is at host parity for DRAM and about 2x
host for L2; JIT FP add is ~1.6x a real 200 MHz R4400's throughput. Neither is
pathological against the hardware. Against the emulator's own integer core, FP
is the outlier; against the interpreter, memory streaming is the outlier.

## Method

Host: Intel Core i5-8500T, 6 cores, 2.1 GHz base / 3.5 GHz max, Linux x86_64,
16 GB. Load average during the runs was 3-12 (other work on the rig); all
non-JIT runs were pinned to one core, JIT runs were deliberately not pinned
(the compile pool needs the other cores). Three whole runs per cell; the tables
give the median. Spread is shown where it is material.

Builds (all `cargo build --release -p iris`, nightly 2026-10-02, commit
`bde03fe`):

| cell | features | cache |
|---|---|---|
| `interp-full` | `lightning` | `--cache full` |
| `interp-shadow` | `lightning` | `--cache shadow` |
| `interp-tcache` | `lightning,tcache` | `--cache full` (tags kept, data transparent) |
| `jitv2` | `lightning,jitv2` (implies `tcache`) | `--cache shadow` and `--cache full` |

`lightning` implies `opcodefusion`; this is the recommended fast interpreter.

The standard suite (`bench/`, 46 kernels, 250 ms target, best-of-two per run)
was run in-process, which uses the checked-in guest image and bypasses the
PROM. The isolated probe suite had 25 extra kernels in a scratch copy:

* four-way unrolled `volatile` 32-bit load, store and read-modify-write loops
  over 8 KB, 256 KB and 8 MB buffers;
* a 64-step Sattolo pointer chase over 8 KB, 128 B stride (dependent loads);
* one word per 4 KB page over 96 pages (KSEG0), the same shape as `sys/tlb_hit`;
* FP loops of eight operations per iteration: eight independent accumulators
  (throughput) or one dependent accumulator (latency), in S and D, for
  add/mul/div/sqrt;
* `cvt.s.d`, `cvt.w.s`/`cvt.s.w` and `cvt.l.d`/`cvt.d.l` round trips, and a
  data-dependent compare + branch (`c.lt.s` + `bc1t`).

The probes were loaded through the monitor's `loadelf`, because `--load-elf` is
unwired on this branch (see "Incidental finding"). The monitor path boots the
PROM first, so wall-clock boot time and `perf` totals include it; the harness's
own `TESTDEV_HOST_NS` timing starts and stops around the timed loop only, so
the ns/op figures are unaffected. For `perf stat` per-op instruction counts the
boot was measured with an empty image and subtracted; those figures are
labelled "boot-subtracted".

## Measurements

### Isolated scalar accesses (micro probes, ns per access; median of 3)

| kernel | interp full | interp shadow | interp tcache | jitv2 shadow | host |
|---|---:|---:|---:|---:|---:|
| `mem/load_l1` | 42.9 | 45.2 | 48.3 | **15.8** | 0.18 |
| `mem/store_l1` | 42.8 | 44.4 | 46.4 | **18.6** | 0.36 |
| `mem/rmw_l1` (per access) | 38.3 | 42.2 | 41.3 | **15.9** | 0.18 |
| `mem/load_l2` | 43.2 | 46.4 | 46.4 | **15.2** | 0.23 |
| `mem/store_l2` | 43.0 | 45.0 | 46.5 | **18.9** | 0.41 |
| `mem/load_dram` | 43.1 | 45.0 | 47.8 | **15.9** | 0.35 |
| `mem/store_dram` | 42.9 | 48.5 | 46.5 | **19.4** | 0.46 |
| `mem/chase_l1` | 51.7 | 52.4 | 54.7 | **14.3** | 1.41 |
| `mem/page_kseg0` | 67.1 | 67.2 | 71.1 | **19.1** | 1.83 |

The sequential L1/L2/DRAM columns are nearly flat. That is not because the
emulator ignores the hierarchy; it is because a 4-way-unrolled stream amortises
the line fill and the interpreter's fixed per-instruction cost is larger. The
pointer chase and the page walk, where each access is its own line, show the
hierarchy and the extra instructions.

The guest executes 2.50 instructions per access in the sequential loops
(`icount/work`). At the measured integer rate of 17.8 ns per guest instruction,
dispatch alone predicts 44.5 ns/access; the measured 42.9 ns is within noise of
that. In the JIT the integer rate is 1.19 ns/instruction, so dispatch predicts
3.0 ns and the remaining ~12.8 ns (81%) is the inline memory guard. The same
decomposition for the other shapes:

| kernel | guest instr/access | interpreter: dispatch predicted | measured | memory-specific share | JIT: dispatch predicted | measured | memory-specific share |
|---|---:|---:|---:|---:|---:|---:|---:|
| load L1 | 2.50 | 44.5 ns | 42.9 | ~0% | 3.0 ns | 15.8 | 81% |
| store L1 | 2.50 | 44.5 ns | 42.8 | ~0% | 3.0 ns | 18.6 | 84% |
| read-modify-write | 1.75 | 31.1 ns | 38.3 | 19% | 2.1 ns | 15.9 | 87% |
| pointer chase L1 | 3.05 | 54.3 ns | 51.7 | ~0% | 3.6 ns | 14.3 | 75% |
| page walk KSEG0 | 4.03 | 71.7 ns | 67.1 | ~0% | 4.8 ns | 19.1 | 75% |

In the interpreter, scalar memory is *dispatch*, not memory. In the JIT, scalar
memory is *the guard*, not dispatch.

### Standard-suite memory kernels (ns per work unit; median of 3)

| kernel | interp full | interp shadow | interp R5000 full | interp tcache | jitv2 shadow | host |
|---|---:|---:|---:|---:|---:|---:|
| `mem/latency_l1` | 40.6 | 47.4 | 45.2 | 42.8 | **9.7** | 1.4 |
| `mem/latency_l2` | 48.1 | 47.4 | 57.2 | 49.8 | **18.6** | 9.3 |
| `mem/latency_dram` | 271.1 | **123.7** | **135.5** | **154.3** | **102.1** | 89.7 |
| `mem/copy` (per byte) | 38.2 | **8.8** | **8.2** | **16.7** | **2.1** | 0.02* |
| `mem/stream_copy` (per byte) | 19.7 | **5.6** | **5.5** | **8.9** | **1.3** | 0.05 |
| `mem/stream_scale` (per byte) | 21.5 | **6.8** | **7.4** | **10.9** | **2.0** | 0.05 |
| `mem/stream_triad` (per byte) | 23.1 | **8.2** | **10.2** | **12.0** | **3.2** | 0.05 |
| `mem/fill` (per byte) | 11.3 | 9.9 | 10.3 | 11.3 | **2.6** | 0.03 |
| `mem/random` | 528.2 | **343.5** | **362.9** | **368.6** | **122.8** | 7.0 |

\* The host `mem/copy` is the host libc/compiler's vectorised memcpy. Do not
read the per-byte ratio as an emulator ratio; the guest copy loop is plain
32-bit word copy. The latency and random rows are the fair host comparisons.

This is the cleanest cache-model experiment in the data: **R5000 full vs R4400
full** is the same code and the same engine with the L2 model switched off
(`Config.SC=1`). The L1 geometry also changes (16 KB direct-mapped to 32 KB
2-way), but the copy/stream buffers are 2-4 MB, far above both, so the L2
switch is the meaningful difference. Copy goes 38.2 -> 8.2 ns/byte (4.7x),
stream_copy 19.7 -> 5.5 (3.6x), DRAM chase 271 -> 135.5 (2.0x). The R4400's L2
model is the dominant interpreter memory cost, not the L1 tag check.

`tcache` (L2 tags but no line data) recovers about half: copy 16.7, stream 8.9,
DRAM 154.3. `shadow` (no L2 at all, direct ppmem window) recovers nearly all:
copy 8.8, stream 5.6, DRAM 123.7. It costs 5-10% on scalar hits (load_l1 45.2 vs
42.9) because the decode cache re-decodes rather than keeping decoded slots in
L2 lines; the trade is strongly in shadow's favour on memory-heavy code.

JIT shadow vs JIT full shows the same thing without interpreter fetch noise:
copy 2.1 vs 12.7 ns/byte (6x; 6.2x using the fastest full run), stream_copy
1.3 vs 17.6 (7-13x; 7x using the fastest run), latency_l2 18.6 vs 33.7 (1.8x),
latency_dram 102 vs 133.5 (1.3x). Integer and FP kernels are unchanged.

### FPU: per operation (micro probes, ns per op; median of 3)

| kernel | interp full | interp shadow | interp tcache | jitv2 shadow | host |
|---|---:|---:|---:|---:|---:|
| `fpu/add_s_thr` | 29.8 | 28.1 | 28.2 | **8.0** | 0.17 |
| `fpu/add_s_lat` | 30.1 | 27.7 | 28.2 | **8.4** | 1.41 |
| `fpu/add_d_thr` | 30.5 | 30.7 | 29.0 | **8.3** | 0.18 |
| `fpu/mul_s_thr` | 33.1 | 29.7 | 28.9 | **8.2** | 0.18 |
| `fpu/div_s_thr` | 29.4 | 28.3 | 29.4 | **9.7** | 0.50 |
| `fpu/div_d_thr` | 28.1 | 29.2 | 30.0 | **11.0** | 0.70 |
| `fpu/sqrt_s_thr` | 26.1 | 26.9 | 26.2 | **7.1** | 0.91 |
| `fpu/sqrt_d_thr` | 28.3 | 28.0 | 27.9 | **6.9** | 1.75 |
| `fpu/cvt_s_d` | 24.9 | 25.2 | 25.3 | **4.6** | 0.00* |
| `fpu/cvt_w_s` | 30.5 | 31.7 | 30.6 | **11.8** | 0.04 |
| `fpu/cvt_l_d` | 30.0 | 31.9 | 29.8 | **12.5** | 0.26 |
| `fpu/cmp_s` (per compare) | 118.6 | 129.8 | 117.1 | **20.9** | 3.98 |

\* The host `cvt.s.d` loop is optimised away by GCC; `div` may be a reciprocal
multiply. Host conversion/div figures are not a fair arithmetic baseline.

Three independent facts:

1. **Latency equals throughput.** `add_s_lat` 30.1 vs `add_s_thr` 29.8 in the
   interpreter, 8.4 vs 8.0 in the JIT. The host FPU pipeline is irrelevant;
   the emulator's per-op work, not the arithmetic, is the limit.
2. **The cache model does not touch FP.** Full, shadow and tcache are within
   noise on every FP kernel.
3. **The JIT helps FP far less than integer.** Per guest instruction (from
   `icount/work`): integer 17.8 ns interpreter -> 1.19 ns JIT (15x); FP add
   23.9 ns -> 6.4 ns (3.7x). The JIT removes dispatch but leaves the FP model.

Standard-suite FP kernels confirm the same ratio:

| kernel | interp full | jitv2 shadow | speedup | host |
|---|---:|---:|---:|---:|
| `fpu/scalar_d` | 67.9 | 20.0 | 3.4x | 1.50 |
| `fpu/scalar_s` | 66.6 | 18.8 | 3.5x | 1.49 |
| `fpu/divsqrt` | 48.5 | 16.9 | 2.9x | 1.12 |
| `fpu/linpack` | 98.8 | 23.1 | 4.3x | 0.20 |
| `fpu/matmul` | 83.5 | 27.1 | 3.1x | 0.16 |
| `int/alu` | 30.1 | 2.0 | **15.0x** | 0.52 |

## Where the time goes

### 1. The interpreter's fetch/decode/dispatch is the biggest single cost

`perf record` on a full in-process interpreter run attributes **63.0% self** to
`step_int`, the generic fetch/decode/dispatch loop. The L2 fill/writeback and
data paths are the next group: `fill_l2_line_for` 4.8%, `read_data_impl<1>`
4.1%, `<4>` 3.4%, `writeback_l1d_line` 1.4%, `write_data_impl<4>` 1.1%,
`exec_lbu` 1.2%, `exec_lw` 0.9%.

The same run with `--cache shadow` puts **73.1% self** in `step_int`, with the
decode-cache fetch 3.5% and no L2 fill in the top symbols. Removing the L2 data
model shifts ~10 percentage points of whole-suite cycles into dispatch; the
absolute memory-kernel wins are much larger (the table above), but the
whole-suite average dilutes them.

A boot-subtracted `perf stat` of one-kernel images makes the shape concrete:

| kernel | host instr/guest instr | guest instr/op | host instr/op | cycles/op | IPC | branch miss |
|---|---:|---:|---:|---:|---:|---:|
| `int/alu` | 157 | 1.69 | 265 | 115 | 2.31 | 1.67% |
| `fpu/add_s_thr` | 211 | 1.25 | 263 | 82 | 3.19 | 0.01% |
| `mem/load_l1` | 177 | 2.50 | 442 | 136 | 3.26 | 0.20% |
| `mem/store_l1` | 173 | 2.50 | 433 | 137 | 3.16 | 0.24% |

IPC is 2.3-3.3 and branch misses are near zero except for integer ALU. The
emulator is **host-instruction-throughput bound**, not branch- or cache-bound.
That is why a scalar load and an FP add cost roughly the same as an integer op
in the interpreter: the interpreter executes 157-211 host instructions for
every guest instruction, and the guest handler is a small part of that.

### 2. The full L2 model is the memory-specific cost

The full L2 line path is `fill_l2_line_for` (`src/cpu/mips_cache_v2.rs:2480`):

* evicts the victim through `writeback_l2_line` + `invalidate_l2_line`
  (`:2045`), which on R4400 cascades the eviction to 8 L1-I and 8 L1-D tag
  checks and invalidations;
* reads 128 bytes and rotates every u64 (`rotate_left(32)`);
* **on a data fill it still initialises all 32 `l2.instrs` slots and sets
  `has_code`**, even though the line is data (`:2565-2629`);
* `fill_l1d_line` (`:2915`) then copies the 16-byte L1 line out of L2.

Every L2 data fill pays that whole path. `invalidate_l2_line` (`:2045`) and
`writeback_l1d_line` (`:2198`) are the functions that show up in the profile;
`fill_l2_line_for` is 4.8% of the whole suite and much more of `mem/copy` and
the stream kernels. The A/B numbers are in the previous section: R5000 with no
L2 is 4.7x faster on copy and 3.6x on stream; tcache halves the gap, shadow
removes it.

This is a **deliberate accuracy choice**, not a bug: the full model is what
makes `CACHE` instructions, VCE and the R4400's decoded-instruction slots
observable. The report's recommendation is about choosing the model per use,
not deleting it.

### 3. The FPU model is ~109 x86 instructions per guest `fadd.s`

The interpreter handler is `exec_fadd_s` (`src/cpu/mips_exec.rs:8456`). Its
compiled body is **159 x86 instructions**, of which one is `addss`. It:

* makes two indirect calls through `fpr_read_w` (the fields at
  `mips_exec.rs:1379-1382`, installed at `:3730-3740`) and one through
  `fpr_write_w`;
* calls `fpu_check_denorm_operand_s` twice (`:8280`) — qNaN/subnormal operand
  checks, ~17 instructions each;
* calls `fpu_arith_flags_snan_only_s` (`:8295`) for the sNaN Invalid flag;
* checks the result with `result.is_subnormal()` and computes its sign;
* calls `fpu_update_fcsr_full` (`:8209`), which clears `FCSR.Cause` and stores
  FCSR on every op, tests the enable bits, and either writes the result or
  takes `EXC_FPE`.

The JIT emits the same semantics in `emit_fbinop_s`
(`src/cpu/jitv2/codegen.rs:8054`): two `emit_check_denorm_operand` calls
(`:5862`), `emit_fpu_arith_flags_snan_only_*`, `emit_fpu_update_fcsr_arith`
(`:5951`) and `emit_fpu_update_fcsr` (`:5638`). Counting the longest compiled
`fpu/add_s_thr` region with `IRIS_JIT_DISASM=1`, the median distance between
consecutive `vaddss` instructions is **109 x86 instructions**, including one
arithmetic instruction. The interpreter handler is 159; the JIT has removed the
dispatch/fetch around it, not the model.

That is why the JIT FP speedup is 3.5x while integer is 15x. A host `addss` is
0.17 ns; the guest `fadd.s` is 8.0 ns. About 6.5-7 ns of the 8 ns is model.

The same accounting for the integer and load handlers:

| handler | x86 instructions (static) |
|---|---:|
| `exec_addu` | 28 |
| `exec_addiu` | 27 |
| `exec_bne` | 28 |
| `exec_lw` | 60 |
| `exec_sw` | 42 |
| `read_data_impl<4>` (called by `lw`) | 304 (worst-case static path) |
| `exec_fadd_s` | **159** |
| `exec_fmul_s` | 159 |
| `exec_fdiv_s` | 166 |
| `exec_fsqrt_s` | 153 |

The FP handlers are 5-6x the integer handlers. In the interpreter the generic
dispatch is so large that this only moves FP from ~18 to ~24 ns/instruction;
in the JIT there is no generic dispatch to hide it.

### 4. Conversion to/from integer is a Rust callout per operation

`cvt.s.d` is inline and costs 4.6 ns; `cvt.w.s` costs 11.8 ns and `cvt.l.d`
12.5 ns. The difference is the callout: `emit_fcvt_to_int` /
`emit_fcvt_from_int` emit a `call_indirect` through
`core.fpu_cvt_to_int_fn` / `fpu_cvt_int_to_float_fn`
(`jitv2/codegen.rs:8616`, `:8654`; installed at `mips_exec.rs:3209-3210`) to
`jit_cvt_to_int` / `jit_cvt_int_to_float`. The callout exists because the exact
MIPS inexact/rounding flags are computed by value, not read from host MXCSR.
A conversion-heavy guest pays ~7 ns per conversion for that call.

### 5. Other items on the checklist

* **TLB translation**: every data access still calls `nanotlb_translate`
  (`mips_exec.rs:3223`) and `nutlb_translate` (`:3288`), including KSEG0
  addresses. It is inlined and not a top symbol; the KSEG0 page-walk probe
  (67.1 ns) is not slower per instruction than an L1 load. The JTLB is only
  touched on a `nutlb` miss. Translation is present but not dominant.
* **ppmem window vs bus**: the scalar `Physical::read32`/`write32` already take
  the ppmem direct path (`physical.rs:1351`/`:1368`, `ppmem_ptr` at `:1050`).
  The old note `scalar-ram-access-bypasses-ppmem-window.md` is stale. The bus
  path is used for fills, MMIO and PROM, not for cached RAM accesses.
* **Decode table (`device_map`)**: only on the bus path. Not a scalar cached
  access cost.
* **Decode cache**: `CpuCache::fetch` is inlined; `decode_into` is only run when
  the slot is undecoded. In one code-layout-sensitive FPU-only run
  `decode_into` was 22% of samples; the standard full-suite profile does not
  show that, and the shadow profile shows 3.5% in the decode-cache fetch. Treat
  the 22% as a layout artefact until reproduced.
* **Endianness**: 32-bit loads/stores have no swap; u64 uses one
  `rotate_left(32)`. The only bulk rotate is the 128-byte L2 fill. It is not a
  per-access cost.
* **Bounds/alias checks**: none beyond the ppmem bitmap and cache tag compare.
  The JIT uses `MemFlagsData::trusted()`.
* **Atomics/locks**: no locks in the FPU path. The generation counter is the
  only per-store atomic: interpreter `tcache` writes call `tc_bump_gen`
  (`mips_cache_v2.rs:1758`) and `ShadowCache::bump_gen` (`mips_cache_shadow.rs:233`)
  which use `fetch_add`; the JIT inline store uses a plain load/add/store
  (`codegen.rs:5179-5188`). In the JIT store is 18.6 ns vs load 15.8 ns, so
  the gen bump + dirty/L2-code path is ~2.8 ns (18%) per store.
* **TCACHE vs full**: tcache removes the L2 data copy but keeps the tag,
  eviction, VCE and `has_code` machinery. It is 2.2-2.3x faster than full on
  copy/stream and 1.8x on DRAM chase, but 8-13% slower on scalar L1 hits
  because it adds the bitmap/window check to every access.

## Ranked recommendations

Ceilings are extrapolations from the measured components, not implemented A/B
results. Nothing here was landed.

| rank | change | evidence | expected ceiling | risk |
|---:|---|---|---|---|
| 1 | **FPU JIT: shrink the per-op model.** Hoist `fcsr` into a host register across a region and write it back on region exit/FCSR access; skip the Cause store when `fcsr & CM == 0` and `flags == 0`; make the sNaN/denorm flags produce a single cold branch. | ~109 x86 instructions per `fadd.s`, one arithmetic; JIT FP 8.0 ns vs int 2.0 ns; host add 0.17 ns | 2-4x on add/mul; ~1.5x on div; ~1.2x on sqrt (host unit latency becomes visible) | exact exception semantics must be preserved; the current code deliberately computes flags by value |
| 2 | **Interpreter: reduce host instructions per guest instruction.** Split `read_data_impl`/`write_data_impl` into an inline hit path and an out-of-line miss path, as `CpuCache::fetch` already does; consider monomorphising FR mode instead of the three FPR fn-pointer calls. | 157-211 host guest-instructions per executed guest instruction; IPC 2.3-3.3; scalar loads are dispatch-bound | 1.3-1.5x overall; 10-20% of interpreter FP from removing the three indirect calls | code-size growth; must not repeat the `fetch` inlining regression |
| 3 | **Make the full-cache L2 model opt-in for speed-sensitive use**, or implement the `tcache` data-elision in the default interpreter. Keep full for `CACHE`/VCE accuracy. | R5000/no-L2 4.7x faster on copy, 3.6x stream, 2.0x DRAM chase; tcache 2.2x; shadow 4.4x | 2-4x on memory-heavy interpreter workloads | `CACHE`-observable state; already exposed as `--cache shadow` and `tcache` |
| 4 | **JIT memory guard: a KSEG0/KSEG1 direct-address fast path** (translation is arithmetic for unmapped segments) and a per-page translation cache, so the guard is one compare rather than a full nutlb probe; keep the callout for misses. | 12.8 ns of a 15.8 ns L1 load is guard; inline is 1.4x better than callout on copy, but callout is 14-29% better on DRAM/random latency | 1.5-2.5x on JIT scalar memory; more on kernel code | must preserve permissions, ASID, VCE and alias semantics |
| 5 | **Inline integer conversions in the JIT** (`cvt.w.s`, `cvt.l.d`, and the reverse) with an inexact-flag computation, instead of the per-op Rust callout. | `cvt.s.d` 4.6 ns vs `cvt.w.s` 11.8 / `cvt.l.d` 12.5; callout at `codegen.rs:8616`/`:8654` | 2-3x on conversion-heavy code | exact Inexact/rounding flags; the callout exists for a reason |
| 6 | **Skip the JIT store generation bump when the target page has no compiled code** (a code-page bitmap), and clear L2 `has_code` only when it is set. | JIT store 18.6 vs load 15.8 ns; ~2.8 ns/store | ~10-15% on store-heavy code | a missed bump can leave stale compiled code; must be exact |
| 7 | **Interpreter FP: remove the three FPR indirect calls** by branching on `STATUS_FR` or templating the executor on FR mode. | 159-instruction `exec_fadd_s`, `call *0x84f68(%rbx)` twice and one indirect write; FPR accessors are tiny | 10-20% of interpreter FP | FR changes are rare; branch predictor handles it; must resync on Status writes |

What **not** to do: do not chase endianness swaps, ppmem bus dispatch, TLB
lookups or per-access bounds checks as memory causes. The measurements put the
cost in host instruction count (interpreter) and the L2 model (streaming), not
in those paths.

## What was not measured

* **Live IRIX / real workloads.** Everything here is the bare-metal suite. The
  IRIX boot and real applications may have different cache/TLB locality.
* **A clean per-kernel profile of the JIT.** `IRIS_JIT_DISASM` works, but the
  micro images are page-layout-sensitive: the same `fpu/add_s_thr` kernel
  measured 8 ns/op in one image and 50-100 ns/op in another, depending on how
  the JIT split and published the page. I did not root-cause that page-level
  variation, so the JIT per-op numbers here are taken from the image where the
  kernel was stable; the standard suite is the cross-check.
* **Denormal/NaN/trap paths.** The probes use normal operands and no enabled
  exceptions. The cost of an actual `EXC_FPE` round trip, and of the rare
  denormal/qNaN branches, is not in these numbers.
* **Dynamic rounding modes.** All probes run in round-to-nearest. The
  interpreter's software rounding for `cvt.*` with non-RN modes was not timed.
* **Page-size/TLB isolation.** KSEG0 versus a mapped KUSEG loop was not a
  controlled pair; `sys/tlb_hit` and the KSEG0 page walk differ in more than
  translation. Huge pages were not tested.
* **True host arithmetic baselines for division/conversion.** GCC optimises
  the host `div` loop (probably reciprocal multiply) and eliminates the
  `cvt.s.d` loop, so the host columns for those rows are lower bounds, not
  like-for-like.
* **Cache-model instruction counts.** The full L2 fill path's exact host
  instruction count was not isolated; the A/B ratios and the profile shares are
  the evidence.

## Incidental finding: `--load-elf` is broken on this branch

`src/workstation.rs:174` gives the `Workstation` trait a default `load_elf`
returning `NOT_HERE`, but `impl Workstation for Machine` (`src/machine.rs:2433`)
never overrides it. `Machine::load_elf` exists at `machine.rs:1438` and is what
`main.rs:115` calls through the trait, so `iris --load-elf FILE` prints
`--load-elf ...: not supported on this machine` and exits. This is a regression
from the 2026-10-23 workstation-trait refactor (`59b7881`), not a CPU issue.
It breaks `iris-bench run --iris` (the subprocess developer path) and any
scripted bare-metal run. The in-process `iris-bench run` is unaffected because
`bench_runner` calls `Machine::load_elf_bytes` directly. The monitor's
`loadelf` also still works, but only after POST has mapped RAM; that is why the
probe runs in this study used the monitor and why their `perf` totals include
PROM boot. Wiring the three-line override would restore the documented path;
it was not changed here.

## Reproduce

Standard suite, in-process (the clean path):

```sh
nix develop /mnt/europa/iris-worktrees/perf-mem-fpu --command \
  cargo build --release -p iris --features lightning        # interpreter
nix develop /mnt/europa/iris-worktrees/perf-mem-fpu --command \
  cargo build --release -p iris --features lightning,tcache
nix develop /mnt/europa/iris-worktrees/perf-mem-fpu --command \
  cargo build --release -p iris --features lightning,jitv2

target/release/iris-bench run --cpu r4400 --cache full
target/release/iris-bench run --cpu r4400 --cache shadow
target/release/iris-bench run --cpu r5000 --cache full
target/release/iris-bench run --cpu r4400 --cache full   # tcache build
target/release/iris-bench run --cpu r4400 --cache shadow # jitv2 build
```

The isolated probes are the loop shapes described under "Method". The two
that matter most are:

```c
/* four-way unrolled, one guest load per reported access */
for (i = 0; i + 3 < n; i += 4) {
    a0 += p[i]; a1 += p[i+1]; a2 += p[i+2]; a3 += p[i+3];
}

/* throughput: eight independent chains; latency: one chain, eight ops */
a0 = a0 + b; a1 = a1 + b; ... a7 = a7 + b;   /* or a = a + b; x8 */
```

Use `IRIS_JIT_DISASM=1` to see the emitted FP sequence, and
`IRIS_NO_INLINE_MEM=1` to A/B the JIT inline memory path. On this branch the
probe ELF has to be loaded with the monitor (`loadelf`) because of the
`--load-elf` regression above.
