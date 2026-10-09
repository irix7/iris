# IRIS Benchmark Suite

Comprehensive performance measurement for the IRIS SGI Indy/Indigo2 emulator.

## Quick Start

```bash
# Run once (in-process, no cross-toolchain needed)
iris-bench run

# Quick mode (~30% wall time, same kernels)
iris-bench run --quick

# Full matrix: all CPUs × engines (separate builds per feature set)
iris-bench matrix

# Compare against a baseline cell
iris-bench report --baseline r4400-interp

# Measure native host speed (for efficiency normalisation)
iris-bench host
```

## What It Measures

### Bare-metal suite (no IRIX required)

46 deterministic kernels in 6 groups, autoscaled to ~250ms each:

| Group | Kernels | What it tests |
|-------|---------|---------------|
| `int/` | alu, alu_ilp, alu64, muldiv, branch, bitops, **dhrystone** | Dispatch cost, 64-bit paths, unpredictable branches |
| `fpu/` | scalar_s, scalar_d, divsqrt, transcend, **whetstone**, **linpack**, matmul | FPU under sustained load |
| `mem/` | L1/L2/DRAM latency, copy, fill, stream, unaligned, random | Cache hierarchy as a curve |
| `img/` | rgb2ycbcr, convolve3x3, sharpen5x5, dct8x8, resize, rotate90, composite, dither, histogram | Imaging workloads (Indy's camera/Photoshop use case) |
| `vid/` | motion_est, yuv2rgb | MPEG inner loop |
| `codec/` | crc32, adler32, rle, lz, huffman | Table lookups, hash chains, bit packing |
| `sys/` | tlb_hit, tlb_miss, exception, cache_flush, uncached, llsc | Emulator-only paths (TLB, exceptions, cache maintenance) |

**Industry-standard figures:**
- `int/dhrystone` → DMIPS (Dhrystone 2.1, rate / 1757)
- `fpu/linpack` → MFLOPS (LINPACK 100×100, rate / 1e6)

### IRIX suite (requires IRIX disk image)

Real-world workloads through IRIX kernel:
- Filesystem: `dd` cold/warm, 500 files create/stat/remove, `tar`/`gzip`/`compress`
- Syscalls: 256MB read/write through IRIX buffer cache
- Network: loopback ping, FTP, NFS
- Graphics: `xwd` framebuffer readback via REX3

```bash
# With emulator running --ci and IRIX at shell prompt:
iris-bench irix --socket /tmp/iris.sock
```

## Output Formats

| Command | Output |
|---------|--------|
| `iris-bench run` | Human table + `IRIS-BENCH-BEGIN`/`END` machine block |
| `iris-bench report --format json` | Full JSON with per-kernel rates, speedups, accuracy |
| `iris-bench report --format md` | Markdown tables (CI artifact) |
| `iris-bench matrix` | All 4 cells (r4400/r5000 × interp/jitv2) |

## CI Integration

### Automatic on push (`.github/workflows/bench-profile.yml`)

On every push to `main`:
1. **Builds** 4 configurations (r4400/r5000 × interp/jitv2)
2. **Runs** bare-metal suite under `perf record -g -F 99`
3. **Generates** interactive flamegraphs (SVG) + folded stacks
4. **Uploads** artifacts (30-day retention)

### A/B comparison (`.github/workflows/bench-ab.yml`)

```bash
gh workflow run bench-ab.yml -f refs="OLD_SHA NEW_SHA" -f cpu=r4400 -f engine=jitv2 -f repeat=8
```
Builds both commits on same runner, runs back-to-back, reports efficiency ratio.

### Historical backfill (`.github/workflows/bench-backfill.yml`)

One-time backfill of historical commits with current apparatus:
```bash
gh workflow run bench-backfill.yml -f cpu=r4400 -f engine=jitv2 -f repeat=3
```
Sharded across 10 runners, merges into `data/bench_history.json`.

## Data Pipeline

```
CI run → report.md → bench_history.py collect → bench_history.json → bench_graphs.py → SVG charts
```

### Generated Assets

| File | Description |
|------|-------------|
| `data/bench_cells.svg` | Latest run: grouped bars (MIPS, DMIPS, LINPACK, Whetstone) |
| `data/bench_history.svg` | Raw guest MIPS over all commits (log scale, host breaks) |
| `data/bench_history_eff.svg` | Normalised efficiency (% of native host speed) |
| `data/bench_speedup.svg` | JIT/interpreter speedup ratio over time |
| `data/bench_groups.svg` | Per-kernel-group efficiency (int/fpu/mem/img/vid/codec/sys) |
| `data/bench_heatmap.svg` | 80-commit × 7-group efficiency heatmap |
| `data/bench_history.md` | Full history table with inline sparklines |

### Profile Artifacts (from `bench-profile.yml`)

| Artifact | Contents |
|----------|----------|
| `profile-{cpu}-{engine}-{sha}/flamegraph.svg` | Interactive Rust flamegraph |
| `profile-{cpu}-{engine}-{sha}/folded.txt` | Collapsed stacks (speedscope-compatible) |
| `profile-{cpu}-{engine}-{sha}/summary.txt` | MIPS summary for correlation |
| `profile-irix-boot-{sha}/flamegraph.svg` | IRIX boot profile (if `IRIS_IRIX_IMAGE_URL` secret set) |

## Viewing Flamegraphs

- **SVG**: Open in browser — click to zoom, search function names
- **Speedscope**: Drag `folded.txt` to <https://www.speedscope.app/>
- **Local diff**: `inferno-diff-folded old.txt new.txt | inferno-flamegraph > diff.svg`

## Key Metrics

| Metric | Meaning | Good |
|--------|---------|------|
| **Guest MIPS** | Emulated instructions / host second | Higher = faster |
| **Efficiency** | Guest MIPS / native host MIPS | 1.0 = native speed |
| **Speedup** | JIT MIPS / Interp MIPS | >1.0 = JIT helps |
| **Accuracy** | Checksum pass rate | 100% required |
| **Group efficiency** | Per-subsystem native fraction | Identifies bottlenecks |

## Adding a Kernel

1. Add `.c` file in `bench/kernels/<group>/`
2. Follow rules in `bench/README.md` (work_alloc once, cksum_u64, init buffers)
3. Add `BENCH("group/name", ...)` to `harness/groups.c`
4. `make -C bench golden` → updates `bench/golden/golden.h`
5. `make -C bench prebuilt` → refreshes `bench/prebuilt/irisbench.elf`
6. Commit both source and prebuilt image

## Configuration

`bench/run/bare.toml`:
```toml
[run]
time_pct = 100    # 100 = full, 30 = quick
repeats = 3       # samples per kernel

[groups]
int = true
fpu = true
mem = true
img = true
vid = true
codec = true
sys = true
```

## Regenerating Charts

```bash
# After bench_history.json updates
python3 tools/bench_graphs.py

# Without README rewrite
python3 tools/bench_graphs.py --no-readme
```

## Files

- `bench/` — Suite source, harness, kernels, prebuilt guest
- `src/bin/iris_bench.rs` — Runner, reporter, CLI
- `src/dev/testdev.rs` — Test device (host timebase, instruction counter)
- `tools/bench_history.py` — Ingest report.md → history.json
- `tools/bench_graphs.py` — Generate SVG charts + rewrite README section
- `tools/bench-backfill.sh` — Build historical commits with current apparatus
- `.github/workflows/bench-*.yml` — CI automation
- `data/bench_history.json` — Persistent history (410+ entries)
- `data/bench_*.svg` — Generated charts