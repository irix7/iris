# Benchmark history

One row per recorded CI run. Throughput figures are only comparable
within the same host CPU — GitHub's shared runners vary between runs,
so cross-host MIPS deltas are host noise, not code change.

| date | source | commit | host | r4400-interp | r4400-jitv2 | r5000-interp | r5000-jitv2 |
|---|---|---|---||---:|---:|---:|---:|
| 2026-09-30 | techomancer/iris | `70a5e394` | AMD EPYC 7763 64-Core Processor | 63.5 | 48.7 | 51.4 | 31.7 |
| 2026-09-30 | techomancer/iris | `b7d2a18b` | AMD EPYC 9V74 80-Core Processor | 81.5 | 50.7 | 51.7 | 30.1 |
| 2026-09-30 | techomancer/iris | `da4223ab` | Intel(R) Xeon(R) 6973P-C | 119.6 | 405.0 | 51.5 | 1062.6 |
| 2026-09-30 | techomancer/iris | `a09186a6` | AMD EPYC 9V74 80-Core Processor | 83.5 | 1190.8 | 110.3 | 387.7 |
| 2026-09-30 | techomancer/iris | `837b048e` | AMD EPYC 7763 64-Core Processor | 63.0 | 430.0 | 69.0 | 455.1 |
| 2026-10-01 | techomancer/iris | `0f5c65c9` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 68.7 | 637.1 | 52.7 | 505.6 |
| 2026-10-02 | techomancer/iris | `71790bd9` | AMD EPYC 7763 64-Core Processor | 61.4 | 779.1 | 113.2 | 423.8 |
| 2026-10-02 | techomancer/iris | `320c38aa` | AMD EPYC 9V74 80-Core Processor | 79.0 | 456.2 | 69.3 | 363.6 |
| 2026-10-02 | techomancer/iris | `e0f0e664` | AMD EPYC 7763 64-Core Processor | 62.3 | 436.5 | 52.4 | 354.7 |
| 2026-10-02 | techomancer/iris | `05295175` | AMD EPYC 7763 64-Core Processor | 61.6 | 459.8 | 52.8 | 405.1 |
| 2026-10-08 | irix7/iris | `ee8cddb6` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 65.8 | 1057.6 | 49.6 | 496.0 |
