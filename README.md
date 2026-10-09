But that doesn't mean we don't do proper software engineering. So lets keep PRs small and reasonable to review. One issue/fix per PR, preferably in one commit, since LLM code churn doesn't help with clarity. Lets keep this bisectable too.

<!-- BENCHMARKS -->

## Benchmarks

On this host the JIT (jitv2) runs the R4400 guest at 632 MIPS versus 46 MIPS interpreted (13.8× speedup).

See **[BENCHMARKS.md](BENCHMARKS.md)** for the full benchmark suite documentation: bare-metal kernels, IRIX workloads, CI workflows, data pipeline, and generated charts.

![latest benchmark cells](data/bench_cells.svg)

Latest run (4 cells):

| cell | CPU | accuracy | MIPS | DMIPS | LINPACK MFLOPS | Whetstone k/s |
|---|---|---:|---:|---:|---:|---:|
| `r4400-interp` | R4400 | 100.0% | 45.9 | 66.4 | 8.2 | 2266.0 |
| `r4400-jitv2` | R4400 | 100.0% | 632.3 | 546.7 | 47.8 | 1143.0 |
| `r5000-interp` | R5000 | 100.0% | 45.6 | 66.6 | 8.1 | 2228.0 |
| `r5000-jitv2` | R5000 | 100.0% | 420.8 | 1056.3 | 111.2 | 9575.0 |

Charts (auto-regenerated from CI history):

- [Raw MIPS history](data/bench_history.svg) — log scale, host-break lines
- [Normalised efficiency](data/bench_history_eff.svg) — % of native host speed
- [JIT speedup](data/bench_speedup.svg) — JIT/interp ratio over time
- [Efficiency by group](data/bench_groups.svg) — int/fpu/mem/img/vid/codec/sys
- [Group heatmap](data/bench_heatmap.svg) — 80 commits × 7 groups
- [Full history table](data/bench_history.md) — with inline sparklines

Profile artifacts (Rust flamegraphs): see `bench-profile.yml` workflow, uploaded per commit.

<!-- BENCHMARKS -->