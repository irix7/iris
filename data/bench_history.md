# Benchmark history

One row per recorded CI run. Throughput figures are only comparable
within the same host CPU — GitHub's shared runners vary between runs,
so cross-host MIPS deltas are host noise, not code change.

| date | source | commit | host | r4400-interp | r4400-jitv2 | r5000-interp | r5000-jitv2 |
|---|---|---|---||---:|---:|---:|---:|
| 2026-08-22 | irix7/iris | `b27be2a0` | AMD EPYC 7763 64-Core Processor | 62.3 | — | — | — |
| 2026-08-22 | irix7/iris | `9ea3e755` | AMD EPYC 9V74 80-Core Processor | 80.3 | — | — | — |
| 2026-08-22 | irix7/iris | `d21f9661` | AMD EPYC 7763 64-Core Processor | 62.4 | — | — | — |
| 2026-08-22 | irix7/iris | `3ca2a186` | Intel(R) Xeon(R) 6973P-C | 114.7 | — | — | — |
| 2026-08-22 | irix7/iris | `0a3a5972` | AMD EPYC 9V74 80-Core Processor | 80.2 | — | — | — |
| 2026-08-22 | irix7/iris | `98332eda` | AMD EPYC 9V45 96-Core Processor | 129.6 | — | — | — |
| 2026-08-22 | irix7/iris | `b27be2a0` | AMD EPYC 7763 64-Core Processor | — | 221.5 | — | — |
| 2026-08-22 | irix7/iris | `9ea3e755` | AMD EPYC 9V45 96-Core Processor | — | 638.1 | — | — |
| 2026-08-22 | irix7/iris | `d21f9661` | INTEL(R) XEON(R) PLATINUM 8573C | — | 371.8 | — | — |
| 2026-08-22 | irix7/iris | `3ca2a186` | AMD EPYC 9V74 80-Core Processor | — | 340.4 | — | — |
| 2026-08-22 | irix7/iris | `0a3a5972` | AMD EPYC 7763 64-Core Processor | — | 225.4 | — | — |
| 2026-08-22 | irix7/iris | `98332eda` | AMD EPYC 9V74 80-Core Processor | — | 412.2 | — | — |
| 2026-08-22 | irix7/iris | `0aa29c02` | AMD EPYC 7763 64-Core Processor | 62.6 | — | — | — |
| 2026-08-22 | irix7/iris | `0aa29c02` | AMD EPYC 9V45 96-Core Processor | — | 807.0 | — | — |
| 2026-08-22 | irix7/iris | `2537f0a5` | INTEL(R) XEON(R) PLATINUM 8573C | 105.0 | — | — | — |
| 2026-08-22 | irix7/iris | `2537f0a5` | AMD EPYC 7763 64-Core Processor | — | 302.0 | — | — |
| 2026-08-22 | irix7/iris | `02c4e155` | AMD EPYC 7763 64-Core Processor | 62.7 | — | — | — |
| 2026-08-22 | irix7/iris | `02c4e155` | AMD EPYC 7763 64-Core Processor | — | 300.2 | — | — |
| 2026-08-22 | irix7/iris | `00cd5df7` | AMD EPYC 7763 64-Core Processor | 62.8 | — | — | — |
| 2026-08-22 | irix7/iris | `00cd5df7` | AMD EPYC 7763 64-Core Processor | — | 291.2 | — | — |
| 2026-08-22 | irix7/iris | `41865506` | AMD EPYC 7763 64-Core Processor | 61.9 | — | — | — |
| 2026-08-22 | irix7/iris | `41865506` | AMD EPYC 7763 64-Core Processor | — | 293.8 | — | — |
| 2026-08-22 | irix7/iris | `3f14f1a1` | AMD EPYC 9V74 80-Core Processor | 80.1 | — | — | — |
| 2026-08-22 | irix7/iris | `3f14f1a1` | AMD EPYC 9V45 96-Core Processor | — | 787.1 | — | — |
| 2026-08-23 | irix7/iris | `8a9de6c8` | AMD EPYC 7763 64-Core Processor | 62.3 | — | — | — |
| 2026-08-23 | irix7/iris | `8a9de6c8` | INTEL(R) XEON(R) PLATINUM 8573C | — | 469.6 | — | — |
| 2026-08-23 | irix7/iris | `6e86811d` | Intel(R) Xeon(R) 6973P-C | 111.9 | — | — | — |
| 2026-08-23 | irix7/iris | `6e86811d` | AMD EPYC 9V74 80-Core Processor | — | 373.4 | — | — |
| 2026-08-23 | irix7/iris | `c2caadf9` | AMD EPYC 9V74 80-Core Processor | 80.3 | — | — | — |
| 2026-08-23 | irix7/iris | `c2caadf9` | AMD EPYC 7763 64-Core Processor | — | 277.8 | — | — |
| 2026-08-25 | irix7/iris | `73a10e26` | AMD EPYC 9V45 96-Core Processor | 136.2 | — | — | — |
| 2026-08-25 | irix7/iris | `73a10e26` | AMD EPYC 9V74 80-Core Processor | — | 469.3 | — | — |
| 2026-08-25 | irix7/iris | `1a1274a4` | AMD EPYC 7763 64-Core Processor | 63.4 | — | — | — |
| 2026-08-25 | irix7/iris | `1a1274a4` | AMD EPYC 9V45 96-Core Processor | — | 786.9 | — | — |
| 2026-08-25 | irix7/iris | `caf9ca2b` | INTEL(R) XEON(R) PLATINUM 8573C | 103.0 | — | — | — |
| 2026-08-25 | irix7/iris | `caf9ca2b` | AMD EPYC 7763 64-Core Processor | — | 295.0 | — | — |
| 2026-08-25 | irix7/iris | `dfc80233` | AMD EPYC 7763 64-Core Processor | 63.9 | — | — | — |
| 2026-08-25 | irix7/iris | `dfc80233` | AMD EPYC 7763 64-Core Processor | — | 287.0 | — | — |
| 2026-08-25 | irix7/iris | `2ad9f00b` | AMD EPYC 7763 64-Core Processor | 64.6 | — | — | — |
| 2026-08-25 | irix7/iris | `2ad9f00b` | AMD EPYC 7763 64-Core Processor | — | 296.0 | — | — |
| 2026-08-25 | irix7/iris | `2598df25` | AMD EPYC 7763 64-Core Processor | 63.9 | — | — | — |
| 2026-08-25 | irix7/iris | `2598df25` | AMD EPYC 7763 64-Core Processor | — | 299.4 | — | — |
| 2026-08-25 | irix7/iris | `a99d94ae` | AMD EPYC 9V45 96-Core Processor | — | 753.9 | — | — |
| 2026-08-25 | irix7/iris | `a408811d` | AMD EPYC 7763 64-Core Processor | 63.1 | — | — | — |
| 2026-08-25 | irix7/iris | `a408811d` | INTEL(R) XEON(R) PLATINUM 8573C | — | 473.6 | — | — |
| 2026-08-25 | irix7/iris | `d549a6eb` | Intel(R) Xeon(R) 6973P-C | 115.5 | — | — | — |
| 2026-08-25 | irix7/iris | `d549a6eb` | AMD EPYC 9V74 80-Core Processor | — | 366.2 | — | — |
| 2026-08-26 | irix7/iris | `2ad1d072` | AMD EPYC 9V74 80-Core Processor | 76.0 | — | — | — |
| 2026-08-26 | irix7/iris | `2ad1d072` | AMD EPYC 7763 64-Core Processor | — | 289.9 | — | — |
| 2026-08-26 | irix7/iris | `81f1b205` | AMD EPYC 9V45 96-Core Processor | 135.2 | — | — | — |
| 2026-08-26 | irix7/iris | `81f1b205` | AMD EPYC 9V74 80-Core Processor | — | 469.1 | — | — |
| 2026-08-26 | irix7/iris | `060b2d9e` | AMD EPYC 7763 64-Core Processor | 64.5 | — | — | — |
| 2026-08-26 | irix7/iris | `060b2d9e` | AMD EPYC 9V45 96-Core Processor | — | 737.0 | — | — |
| 2026-08-27 | irix7/iris | `a55cbc81` | INTEL(R) XEON(R) PLATINUM 8573C | 102.1 | — | — | — |
| 2026-08-27 | irix7/iris | `a55cbc81` | AMD EPYC 7763 64-Core Processor | — | 297.5 | — | — |
| 2026-08-27 | irix7/iris | `e804d22a` | AMD EPYC 7763 64-Core Processor | 63.8 | — | — | — |
| 2026-08-27 | irix7/iris | `e804d22a` | AMD EPYC 7763 64-Core Processor | — | 302.5 | — | — |
| 2026-08-27 | irix7/iris | `c8e42865` | AMD EPYC 7763 64-Core Processor | 65.1 | — | — | — |
| 2026-08-27 | irix7/iris | `c8e42865` | AMD EPYC 7763 64-Core Processor | — | 410.5 | — | — |
| 2026-08-27 | irix7/iris | `4bc2e43b` | AMD EPYC 7763 64-Core Processor | 65.4 | — | — | — |
| 2026-08-27 | irix7/iris | `4bc2e43b` | AMD EPYC 7763 64-Core Processor | — | 411.5 | — | — |
| 2026-08-28 | irix7/iris | `37cf1226` | AMD EPYC 9V74 80-Core Processor | 81.9 | — | — | — |
| 2026-08-28 | irix7/iris | `37cf1226` | AMD EPYC 9V45 96-Core Processor | — | 1007.5 | — | — |
| 2026-08-29 | irix7/iris | `980875fe` | AMD EPYC 7763 64-Core Processor | 63.9 | — | — | — |
| 2026-08-29 | irix7/iris | `980875fe` | INTEL(R) XEON(R) PLATINUM 8573C | — | 587.9 | — | — |
| 2026-08-29 | irix7/iris | `64dd9157` | Intel(R) Xeon(R) 6973P-C | 117.9 | — | — | — |
| 2026-08-29 | irix7/iris | `64dd9157` | AMD EPYC 9V74 80-Core Processor | — | 480.5 | — | — |
| 2026-08-29 | irix7/iris | `4ada5a4e` | AMD EPYC 9V74 80-Core Processor | 74.5 | — | — | — |
| 2026-08-29 | irix7/iris | `4ada5a4e` | AMD EPYC 7763 64-Core Processor | — | 418.1 | — | — |
| 2026-08-29 | irix7/iris | `35aeb6de` | AMD EPYC 9V45 96-Core Processor | 137.8 | — | — | — |
| 2026-08-29 | irix7/iris | `35aeb6de` | AMD EPYC 9V74 80-Core Processor | — | 594.9 | — | — |
| 2026-08-29 | irix7/iris | `6b6de886` | AMD EPYC 7763 64-Core Processor | 65.0 | — | — | — |
| 2026-08-29 | irix7/iris | `6b6de886` | AMD EPYC 9V45 96-Core Processor | — | 1012.0 | — | — |
| 2026-08-30 | irix7/iris | `7d4935d0` | INTEL(R) XEON(R) PLATINUM 8573C | 101.4 | — | — | — |
| 2026-08-30 | irix7/iris | `7d4935d0` | AMD EPYC 7763 64-Core Processor | — | 430.7 | — | — |
| 2026-08-30 | irix7/iris | `de244934` | AMD EPYC 7763 64-Core Processor | 65.0 | — | — | — |
| 2026-08-30 | irix7/iris | `de244934` | AMD EPYC 7763 64-Core Processor | — | 402.0 | — | — |
| 2026-08-31 | irix7/iris | `5079d2f0` | AMD EPYC 7763 64-Core Processor | 63.6 | — | — | — |
| 2026-08-31 | irix7/iris | `49d77d4d` | AMD EPYC 9V74 80-Core Processor | 80.7 | — | — | — |
| 2026-08-31 | irix7/iris | `f40a1c14` | AMD EPYC 7763 64-Core Processor | 65.1 | — | — | — |
| 2026-08-31 | irix7/iris | `5079d2f0` | AMD EPYC 7763 64-Core Processor | — | 409.1 | — | — |
| 2026-08-31 | irix7/iris | `49d77d4d` | AMD EPYC 9V45 96-Core Processor | — | 998.2 | — | — |
| 2026-08-31 | irix7/iris | `f40a1c14` | AMD EPYC 7763 64-Core Processor | — | 409.7 | — | — |
| 2026-09-03 | irix7/iris | `fede2427` | INTEL(R) XEON(R) PLATINUM 8573C | — | 576.6 | — | — |
| 2026-09-03 | irix7/iris | `a77fc060` | AMD EPYC 9V74 80-Core Processor | — | 496.2 | — | — |
| 2026-09-03 | irix7/iris | `d6568abf` | AMD EPYC 7763 64-Core Processor | — | 412.3 | — | — |
| 2026-09-03 | irix7/iris | `224fc8eb` | AMD EPYC 9V45 96-Core Processor | 132.6 | — | — | — |
| 2026-09-03 | irix7/iris | `224fc8eb` | AMD EPYC 9V74 80-Core Processor | — | 576.4 | — | — |
| 2026-09-08 | irix7/iris | `c2e085a3` | AMD EPYC 7763 64-Core Processor | 64.4 | — | — | — |
| 2026-09-08 | irix7/iris | `c2e085a3` | AMD EPYC 9V45 96-Core Processor | — | 1079.5 | — | — |
| 2026-09-08 | irix7/iris | `81c34ba4` | INTEL(R) XEON(R) PLATINUM 8573C | 100.0 | — | — | — |
| 2026-09-08 | irix7/iris | `81c34ba4` | AMD EPYC 7763 64-Core Processor | — | 429.9 | — | — |
| 2026-09-08 | irix7/iris | `8911c1be` | AMD EPYC 7763 64-Core Processor | 64.7 | — | — | — |
| 2026-09-08 | irix7/iris | `8911c1be` | AMD EPYC 7763 64-Core Processor | — | 435.9 | — | — |
| 2026-09-08 | irix7/iris | `05409919` | AMD EPYC 7763 64-Core Processor | 64.4 | — | — | — |
| 2026-09-08 | irix7/iris | `05409919` | AMD EPYC 7763 64-Core Processor | — | 430.9 | — | — |
| 2026-09-09 | irix7/iris | `aac7674f` | AMD EPYC 7763 64-Core Processor | 63.2 | — | — | — |
| 2026-09-09 | irix7/iris | `aac7674f` | AMD EPYC 7763 64-Core Processor | — | 434.8 | — | — |
| 2026-09-11 | irix7/iris | `47508124` | AMD EPYC 9V74 80-Core Processor | 82.4 | — | — | — |
| 2026-09-11 | irix7/iris | `0f7f9a60` | AMD EPYC 7763 64-Core Processor | 63.9 | — | — | — |
| 2026-09-11 | irix7/iris | `2d98434f` | Intel(R) Xeon(R) 6973P-C | 101.1 | — | — | — |
| 2026-09-11 | irix7/iris | `47508124` | AMD EPYC 9V45 96-Core Processor | — | 1038.4 | — | — |
| 2026-09-11 | irix7/iris | `0f7f9a60` | INTEL(R) XEON(R) PLATINUM 8573C | — | 614.8 | — | — |
| 2026-09-11 | irix7/iris | `2d98434f` | AMD EPYC 9V74 80-Core Processor | — | 501.0 | — | — |
| 2026-09-12 | irix7/iris | `5150db08` | AMD EPYC 9V74 80-Core Processor | 74.3 | — | — | — |
| 2026-09-12 | irix7/iris | `cd8b55de` | AMD EPYC 9V45 96-Core Processor | 131.1 | — | — | — |
| 2026-09-12 | irix7/iris | `5afab5e4` | AMD EPYC 7763 64-Core Processor | 64.7 | — | — | — |
| 2026-09-12 | irix7/iris | `e293b470` | INTEL(R) XEON(R) PLATINUM 8573C | 87.2 | — | — | — |
| 2026-09-12 | irix7/iris | `78742e0d` | AMD EPYC 7763 64-Core Processor | 64.4 | — | — | — |
| 2026-09-12 | irix7/iris | `5150db08` | AMD EPYC 7763 64-Core Processor | — | 428.7 | — | — |
| 2026-09-12 | irix7/iris | `cd8b55de` | AMD EPYC 9V74 80-Core Processor | — | 624.9 | — | — |
| 2026-09-12 | irix7/iris | `5afab5e4` | AMD EPYC 9V45 96-Core Processor | — | 1073.5 | — | — |
| 2026-09-12 | irix7/iris | `e293b470` | AMD EPYC 7763 64-Core Processor | — | 433.8 | — | — |
| 2026-09-12 | irix7/iris | `78742e0d` | AMD EPYC 7763 64-Core Processor | — | 418.8 | — | — |
| 2026-09-12 | irix7/iris | `25f86c10` | AMD EPYC 7763 64-Core Processor | 64.4 | — | — | — |
| 2026-09-12 | irix7/iris | `25f86c10` | AMD EPYC 7763 64-Core Processor | — | 428.0 | — | — |
| 2026-09-12 | irix7/iris | `7ecccf4b` | AMD EPYC 7763 64-Core Processor | 64.5 | — | — | — |
| 2026-09-12 | irix7/iris | `8ab65ffa` | AMD EPYC 9V74 80-Core Processor | 82.3 | — | — | — |
| 2026-09-12 | irix7/iris | `7ecccf4b` | AMD EPYC 7763 64-Core Processor | — | 429.7 | — | — |
| 2026-09-12 | irix7/iris | `8ab65ffa` | AMD EPYC 9V45 96-Core Processor | — | 1002.2 | — | — |
| 2026-09-13 | irix7/iris | `c1692ae3` | AMD EPYC 7763 64-Core Processor | 63.3 | — | — | — |
| 2026-09-13 | irix7/iris | `066935bb` | Intel(R) Xeon(R) 6973P-C | 98.9 | — | — | — |
| 2026-09-13 | irix7/iris | `c1692ae3` | INTEL(R) XEON(R) PLATINUM 8573C | — | 47.8 | — | — |
| 2026-09-13 | irix7/iris | `066935bb` | AMD EPYC 9V74 80-Core Processor | — | 37.5 | — | — |
| 2026-09-13 | irix7/iris | `f8fb34eb` | AMD EPYC 9V74 80-Core Processor | 79.7 | — | — | — |
| 2026-09-13 | irix7/iris | `f8fb34eb` | AMD EPYC 7763 64-Core Processor | — | 34.5 | — | — |
| 2026-09-13 | irix7/iris | `4c6e37e1` | AMD EPYC 9V45 96-Core Processor | 126.4 | — | — | — |
| 2026-09-13 | irix7/iris | `4c6e37e1` | AMD EPYC 9V74 80-Core Processor | — | 48.0 | — | — |
| 2026-09-15 | irix7/iris | `d75d8ff9` | AMD EPYC 7763 64-Core Processor | 63.0 | — | — | — |
| 2026-09-15 | irix7/iris | `bb9ef187` | INTEL(R) XEON(R) PLATINUM 8573C | 98.3 | — | — | — |
| 2026-09-15 | irix7/iris | `b3f36b8f` | AMD EPYC 7763 64-Core Processor | 63.2 | — | — | — |
| 2026-09-15 | irix7/iris | `d75d8ff9` | AMD EPYC 9V45 96-Core Processor | — | 78.5 | — | — |
| 2026-09-15 | irix7/iris | `bb9ef187` | AMD EPYC 7763 64-Core Processor | — | 37.9 | — | — |
| 2026-09-15 | irix7/iris | `b3f36b8f` | AMD EPYC 7763 64-Core Processor | — | 38.2 | — | — |
| 2026-09-16 | irix7/iris | `9ea63c1f` | AMD EPYC 7763 64-Core Processor | 64.2 | — | — | — |
| 2026-09-16 | irix7/iris | `9ea63c1f` | AMD EPYC 7763 64-Core Processor | — | 38.2 | — | — |
| 2026-09-16 | irix7/iris | `1257607e` | AMD EPYC 7763 64-Core Processor | 64.1 | — | — | — |
| 2026-09-16 | irix7/iris | `c84fe8ed` | AMD EPYC 9V74 80-Core Processor | 79.8 | — | — | — |
| 2026-09-16 | irix7/iris | `86248141` | AMD EPYC 7763 64-Core Processor | 64.7 | — | — | — |
| 2026-09-16 | irix7/iris | `f76506fd` | Intel(R) Xeon(R) 6973P-C | 109.4 | — | — | — |
| 2026-09-16 | irix7/iris | `6cc9e448` | AMD EPYC 9V74 80-Core Processor | 80.0 | — | — | — |
| 2026-09-16 | irix7/iris | `520441bd` | AMD EPYC 9V45 96-Core Processor | 128.8 | — | — | — |
| 2026-09-16 | irix7/iris | `51339267` | AMD EPYC 7763 64-Core Processor | 64.5 | — | — | — |
| 2026-09-16 | irix7/iris | `1257607e` | AMD EPYC 7763 64-Core Processor | — | 37.9 | — | — |
| 2026-09-16 | irix7/iris | `c84fe8ed` | AMD EPYC 9V45 96-Core Processor | — | 79.8 | — | — |
| 2026-09-16 | irix7/iris | `86248141` | INTEL(R) XEON(R) PLATINUM 8573C | — | 47.7 | — | — |
| 2026-09-16 | irix7/iris | `f76506fd` | AMD EPYC 9V74 80-Core Processor | — | 38.0 | — | — |
| 2026-09-16 | irix7/iris | `6cc9e448` | AMD EPYC 7763 64-Core Processor | — | 37.7 | — | — |
| 2026-09-16 | irix7/iris | `520441bd` | AMD EPYC 9V74 80-Core Processor | — | 42.6 | — | — |
| 2026-09-16 | irix7/iris | `51339267` | AMD EPYC 9V45 96-Core Processor | — | 78.0 | — | — |
| 2026-09-16 | irix7/iris | `f1d0fcb6` | INTEL(R) XEON(R) PLATINUM 8573C | 98.2 | — | — | — |
| 2026-09-16 | irix7/iris | `35a3b18a` | AMD EPYC 7763 64-Core Processor | 63.5 | — | — | — |
| 2026-09-16 | irix7/iris | `f1d0fcb6` | AMD EPYC 7763 64-Core Processor | — | 38.3 | — | — |
| 2026-09-16 | irix7/iris | `35a3b18a` | AMD EPYC 7763 64-Core Processor | — | 38.7 | — | — |
| 2026-09-17 | irix7/iris | `e93c5bb5` | AMD EPYC 7763 64-Core Processor | 63.6 | — | — | — |
| 2026-09-17 | irix7/iris | `e93c5bb5` | AMD EPYC 7763 64-Core Processor | — | 38.2 | — | — |
| 2026-09-17 | irix7/iris | `c55de0b9` | AMD EPYC 7763 64-Core Processor | 62.5 | — | — | — |
| 2026-09-17 | irix7/iris | `c55de0b9` | AMD EPYC 7763 64-Core Processor | — | 38.3 | — | — |
| 2026-09-18 | irix7/iris | `44f4b243` | AMD EPYC 9V74 80-Core Processor | 79.7 | — | — | — |
| 2026-09-18 | irix7/iris | `44f4b243` | AMD EPYC 9V45 96-Core Processor | — | 80.4 | — | — |
| 2026-09-18 | irix7/iris | `edccb811` | AMD EPYC 7763 64-Core Processor | 62.5 | — | — | — |
| 2026-09-18 | irix7/iris | `156936a0` | Intel(R) Xeon(R) 6973P-C | 105.1 | — | — | — |
| 2026-09-18 | irix7/iris | `edccb811` | INTEL(R) XEON(R) PLATINUM 8573C | — | 49.0 | — | — |
| 2026-09-18 | irix7/iris | `156936a0` | AMD EPYC 9V74 80-Core Processor | — | 39.0 | — | — |
| 2026-09-18 | irix7/iris | `1390bf6e` | AMD EPYC 9V74 80-Core Processor | 74.6 | — | — | — |
| 2026-09-18 | irix7/iris | `1390bf6e` | AMD EPYC 7763 64-Core Processor | — | 37.8 | — | — |
| 2026-09-18 | irix7/iris | `66d5151e` | AMD EPYC 9V45 96-Core Processor | 128.2 | — | — | — |
| 2026-09-18 | irix7/iris | `66d5151e` | AMD EPYC 9V74 80-Core Processor | — | 45.5 | — | — |
| 2026-09-18 | irix7/iris | `45f26b8b` | AMD EPYC 7763 64-Core Processor | 62.2 | — | — | — |
| 2026-09-18 | irix7/iris | `e603daff` | INTEL(R) XEON(R) PLATINUM 8573C | 99.2 | — | — | — |
| 2026-09-18 | irix7/iris | `45f26b8b` | AMD EPYC 9V45 96-Core Processor | — | 78.8 | — | — |
| 2026-09-18 | irix7/iris | `e603daff` | AMD EPYC 7763 64-Core Processor | — | 38.1 | — | — |
| 2026-09-18 | irix7/iris | `f1a561d0` | AMD EPYC 7763 64-Core Processor | 63.0 | — | — | — |
| 2026-09-18 | irix7/iris | `f1a561d0` | AMD EPYC 7763 64-Core Processor | — | 37.4 | — | — |
| 2026-09-19 | irix7/iris | `d1bf817f` | AMD EPYC 7763 64-Core Processor | 63.1 | — | — | — |
| 2026-09-19 | irix7/iris | `d1bf817f` | AMD EPYC 7763 64-Core Processor | — | 37.5 | — | — |
| 2026-09-19 | irix7/iris | `eab2df28` | AMD EPYC 7763 64-Core Processor | 63.9 | — | — | — |
| 2026-09-19 | irix7/iris | `eab2df28` | AMD EPYC 7763 64-Core Processor | — | 37.4 | — | — |
| 2026-09-19 | irix7/iris | `84344461` | AMD EPYC 9V74 80-Core Processor | 78.5 | — | — | — |
| 2026-09-19 | irix7/iris | `84344461` | AMD EPYC 9V45 96-Core Processor | — | 78.8 | — | — |
| 2026-09-19 | irix7/iris | `39d304f6` | AMD EPYC 7763 64-Core Processor | 65.0 | — | — | — |
| 2026-09-19 | irix7/iris | `39d304f6` | INTEL(R) XEON(R) PLATINUM 8573C | — | 48.4 | — | — |
| 2026-09-19 | irix7/iris | `18109d80` | Intel(R) Xeon(R) 6973P-C | 113.5 | — | — | — |
| 2026-09-19 | irix7/iris | `18109d80` | AMD EPYC 9V74 80-Core Processor | — | 38.1 | — | — |
| 2026-09-19 | irix7/iris | `b6440aad` | AMD EPYC 9V74 80-Core Processor | 70.0 | — | — | — |
| 2026-09-19 | irix7/iris | `b6440aad` | AMD EPYC 7763 64-Core Processor | — | 37.8 | — | — |
| 2026-09-19 | irix7/iris | `6ab3f112` | AMD EPYC 9V45 96-Core Processor | 127.5 | — | — | — |
| 2026-09-19 | irix7/iris | `6ab3f112` | AMD EPYC 9V74 80-Core Processor | — | 47.6 | — | — |
| 2026-09-19 | irix7/iris | `2b55ed61` | AMD EPYC 7763 64-Core Processor | 63.8 | — | — | — |
| 2026-09-19 | irix7/iris | `5219128d` | INTEL(R) XEON(R) PLATINUM 8573C | 90.1 | — | — | — |
| 2026-09-19 | irix7/iris | `2b55ed61` | AMD EPYC 9V45 96-Core Processor | — | 79.6 | — | — |
| 2026-09-19 | irix7/iris | `5219128d` | AMD EPYC 7763 64-Core Processor | — | 38.0 | — | — |
| 2026-09-19 | irix7/iris | `a68e7a36` | AMD EPYC 7763 64-Core Processor | 64.2 | — | — | — |
| 2026-09-19 | irix7/iris | `d5d695e1` | AMD EPYC 7763 64-Core Processor | 64.7 | — | — | — |
| 2026-09-19 | irix7/iris | `a68e7a36` | AMD EPYC 7763 64-Core Processor | — | 37.0 | — | — |
| 2026-09-19 | irix7/iris | `d5d695e1` | AMD EPYC 7763 64-Core Processor | — | 37.9 | — | — |
| 2026-09-19 | irix7/iris | `b7bca7c4` | AMD EPYC 7763 64-Core Processor | 62.8 | — | — | — |
| 2026-09-19 | irix7/iris | `b7bca7c4` | AMD EPYC 7763 64-Core Processor | — | 37.9 | — | — |
| 2026-09-20 | irix7/iris | `a6f773e2` | AMD EPYC 9V74 80-Core Processor | 80.2 | — | — | — |
| 2026-09-20 | irix7/iris | `0f1a3a2d` | AMD EPYC 7763 64-Core Processor | 63.6 | — | — | — |
| 2026-09-20 | irix7/iris | `e76520e0` | Intel(R) Xeon(R) 6973P-C | 109.2 | — | — | — |
| 2026-09-20 | irix7/iris | `3f51ff0f` | AMD EPYC 9V74 80-Core Processor | 72.0 | — | — | — |
| 2026-09-20 | irix7/iris | `e03b9b80` | AMD EPYC 9V45 96-Core Processor | 131.3 | — | — | — |
| 2026-09-20 | irix7/iris | `44e9b9e2` | AMD EPYC 7763 64-Core Processor | 62.9 | — | — | — |
| 2026-09-20 | irix7/iris | `a6f773e2` | AMD EPYC 9V45 96-Core Processor | — | 78.4 | — | — |
| 2026-09-20 | irix7/iris | `0f1a3a2d` | INTEL(R) XEON(R) PLATINUM 8573C | — | 47.4 | — | — |
| 2026-09-20 | irix7/iris | `e76520e0` | AMD EPYC 9V74 80-Core Processor | — | 38.4 | — | — |
| 2026-09-20 | irix7/iris | `3f51ff0f` | AMD EPYC 7763 64-Core Processor | — | 37.2 | — | — |
| 2026-09-20 | irix7/iris | `e03b9b80` | AMD EPYC 9V74 80-Core Processor | — | 49.0 | — | — |
| 2026-09-20 | irix7/iris | `44e9b9e2` | AMD EPYC 9V45 96-Core Processor | — | 81.5 | — | — |
| 2026-09-21 | irix7/iris | `8dfc3650` | INTEL(R) XEON(R) PLATINUM 8573C | 98.2 | — | — | — |
| 2026-09-21 | irix7/iris | `da2bb7d5` | AMD EPYC 7763 64-Core Processor | 62.9 | — | — | — |
| 2026-09-21 | irix7/iris | `8dfc3650` | AMD EPYC 7763 64-Core Processor | — | 36.4 | — | — |
| 2026-09-21 | irix7/iris | `da2bb7d5` | AMD EPYC 7763 64-Core Processor | — | 38.2 | — | — |
| 2026-09-21 | irix7/iris | `bea0af66` | AMD EPYC 7763 64-Core Processor | 63.7 | — | — | — |
| 2026-09-21 | irix7/iris | `bea0af66` | AMD EPYC 7763 64-Core Processor | — | 37.5 | — | — |
| 2026-09-25 | irix7/iris | `b0e6f7bc` | AMD EPYC 7763 64-Core Processor | 62.6 | — | — | — |
| 2026-09-25 | irix7/iris | `b0e6f7bc` | AMD EPYC 7763 64-Core Processor | — | 37.1 | — | — |
| 2026-09-27 | irix7/iris | `c5e3ad4b` | AMD EPYC 9V74 80-Core Processor | 79.3 | — | — | — |
| 2026-09-27 | irix7/iris | `c5e3ad4b` | AMD EPYC 9V45 96-Core Processor | — | 76.8 | — | — |
| 2026-09-28 | irix7/iris | `5d07b4d6` | AMD EPYC 7763 64-Core Processor | 63.6 | — | — | — |
| 2026-09-28 | irix7/iris | `5d07b4d6` | INTEL(R) XEON(R) PLATINUM 8573C | — | 44.9 | — | — |
| 2026-09-29 | irix7/iris | `2383f267` | Intel(R) Xeon(R) 6973P-C | 109.1 | — | — | — |
| 2026-09-29 | irix7/iris | `30fb4ffe` | AMD EPYC 9V74 80-Core Processor | 78.2 | — | — | — |
| 2026-09-29 | irix7/iris | `a528f183` | AMD EPYC 9V45 96-Core Processor | 130.1 | — | — | — |
| 2026-09-29 | irix7/iris | `e8060668` | AMD EPYC 7763 64-Core Processor | 64.3 | — | — | — |
| 2026-09-29 | irix7/iris | `2383f267` | AMD EPYC 9V74 80-Core Processor | — | 35.3 | — | — |
| 2026-09-29 | irix7/iris | `30fb4ffe` | AMD EPYC 7763 64-Core Processor | — | 35.8 | — | — |
| 2026-09-29 | irix7/iris | `a528f183` | AMD EPYC 9V74 80-Core Processor | — | 45.2 | — | — |
| 2026-09-29 | irix7/iris | `e8060668` | AMD EPYC 9V45 96-Core Processor | — | 78.9 | — | — |
| 2026-09-29 | irix7/iris | `bb286ea7` | INTEL(R) XEON(R) PLATINUM 8573C | 98.8 | — | — | — |
| 2026-09-29 | irix7/iris | `bb286ea7` | AMD EPYC 7763 64-Core Processor | — | 38.4 | — | — |
| 2026-09-29 | irix7/iris | `36ec12b6` | AMD EPYC 7763 64-Core Processor | 64.2 | — | — | — |
| 2026-09-29 | irix7/iris | `36ec12b6` | AMD EPYC 7763 64-Core Processor | — | 35.6 | — | — |
| 2026-09-29 | irix7/iris | `70a5e394` | AMD EPYC 7763 64-Core Processor | 64.6 | — | — | — |
| 2026-09-29 | irix7/iris | `70a5e394` | AMD EPYC 7763 64-Core Processor | — | 35.8 | — | — |
| 2026-09-29 | irix7/iris | `e6bde884` | AMD EPYC 7763 64-Core Processor | 62.8 | — | — | — |
| 2026-09-29 | irix7/iris | `e6bde884` | AMD EPYC 7763 64-Core Processor | — | 37.0 | — | — |
| 2026-09-29 | irix7/iris | `b7d2a18b` | AMD EPYC 9V74 80-Core Processor | 79.2 | — | — | — |
| 2026-09-29 | irix7/iris | `b7d2a18b` | AMD EPYC 9V45 96-Core Processor | — | 76.6 | — | — |
| 2026-09-29 | irix7/iris | `da4223ab` | AMD EPYC 7763 64-Core Processor | 62.8 | — | — | — |
| 2026-09-29 | irix7/iris | `da4223ab` | INTEL(R) XEON(R) PLATINUM 8573C | — | 697.4 | — | — |
| 2026-09-29 | irix7/iris | `a09186a6` | Intel(R) Xeon(R) 6973P-C | 112.0 | — | — | — |
| 2026-09-29 | irix7/iris | `a09186a6` | AMD EPYC 9V74 80-Core Processor | — | 521.8 | — | — |
| 2026-09-29 | irix7/iris | `772e01c2` | AMD EPYC 9V74 80-Core Processor | 80.1 | — | — | — |
| 2026-09-29 | irix7/iris | `772e01c2` | AMD EPYC 7763 64-Core Processor | — | 439.7 | — | — |
| 2026-09-29 | irix7/iris | `3e384084` | AMD EPYC 9V45 96-Core Processor | 131.8 | — | — | — |
| 2026-09-29 | irix7/iris | `3e384084` | AMD EPYC 9V74 80-Core Processor | — | 594.6 | — | — |
| 2026-09-29 | irix7/iris | `38e510ca` | AMD EPYC 7763 64-Core Processor | 63.8 | — | — | — |
| 2026-09-29 | irix7/iris | `38e510ca` | AMD EPYC 9V45 96-Core Processor | — | 1199.9 | — | — |
| 2026-09-29 | irix7/iris | `95b7ad0a` | INTEL(R) XEON(R) PLATINUM 8573C | 101.3 | — | — | — |
| 2026-09-29 | irix7/iris | `95b7ad0a` | AMD EPYC 7763 64-Core Processor | — | 459.9 | — | — |
| 2026-09-30 | irix7/iris | `839155a2` | AMD EPYC 7763 64-Core Processor | 61.5 | — | — | — |
| 2026-09-30 | irix7/iris | `afb8d1ba` | AMD EPYC 9V74 80-Core Processor | 82.9 | — | — | — |
| 2026-09-30 | irix7/iris | `837b048e` | AMD EPYC 7763 64-Core Processor | 61.7 | — | — | — |
| 2026-09-30 | irix7/iris | `0b1ea38c` | AMD EPYC 7763 64-Core Processor | 66.8 | — | — | — |
| 2026-09-30 | irix7/iris | `85796146` | AMD EPYC 7763 64-Core Processor | 62.9 | — | — | — |
| 2026-09-30 | irix7/iris | `839155a2` | AMD EPYC 7763 64-Core Processor | — | 448.3 | — | — |
| 2026-09-30 | irix7/iris | `afb8d1ba` | AMD EPYC 9V45 96-Core Processor | — | 1148.1 | — | — |
| 2026-09-30 | irix7/iris | `837b048e` | INTEL(R) XEON(R) PLATINUM 8573C | — | 691.4 | — | — |
| 2026-09-30 | irix7/iris | `0b1ea38c` | AMD EPYC 7763 64-Core Processor | — | 447.1 | — | — |
| 2026-09-30 | irix7/iris | `85796146` | AMD EPYC 7763 64-Core Processor | — | 457.5 | — | — |
| 2026-09-30 | techomancer/iris | `70a5e394` | AMD EPYC 7763 64-Core Processor | 63.5 | 48.7 | 51.4 | 31.7 |
| 2026-09-30 | irix7/iris | `08be2936` | Intel(R) Xeon(R) 6973P-C | 103.4 | — | — | — |
| 2026-09-30 | irix7/iris | `08be2936` | AMD EPYC 9V74 80-Core Processor | — | 541.7 | — | — |
| 2026-09-30 | techomancer/iris | `b7d2a18b` | AMD EPYC 9V74 80-Core Processor | 81.5 | 50.7 | 51.7 | 30.1 |
| 2026-09-30 | techomancer/iris | `da4223ab` | Intel(R) Xeon(R) 6973P-C | 119.6 | 405.0 | 51.5 | 1062.6 |
| 2026-09-30 | techomancer/iris | `a09186a6` | AMD EPYC 9V74 80-Core Processor | 83.5 | 1190.8 | 110.3 | 387.7 |
| 2026-09-30 | irix7/iris | `2a796e2a` | AMD EPYC 9V74 80-Core Processor | 74.9 | — | — | — |
| 2026-09-30 | irix7/iris | `5038af99` | AMD EPYC 9V45 96-Core Processor | 121.2 | — | — | — |
| 2026-09-30 | irix7/iris | `98a523ea` | AMD EPYC 7763 64-Core Processor | 60.1 | — | — | — |
| 2026-09-30 | irix7/iris | `2a796e2a` | AMD EPYC 7763 64-Core Processor | — | 420.3 | — | — |
| 2026-09-30 | irix7/iris | `5038af99` | AMD EPYC 9V74 80-Core Processor | — | 702.8 | — | — |
| 2026-09-30 | irix7/iris | `98a523ea` | AMD EPYC 9V45 96-Core Processor | — | 1166.8 | — | — |
| 2026-09-30 | irix7/iris | `5787d027` | INTEL(R) XEON(R) PLATINUM 8573C | 96.1 | — | — | — |
| 2026-09-30 | irix7/iris | `5787d027` | AMD EPYC 7763 64-Core Processor | — | 443.0 | — | — |
| 2026-09-30 | irix7/iris | `b5cc3635` | AMD EPYC 7763 64-Core Processor | 60.7 | — | — | — |
| 2026-09-30 | irix7/iris | `b5cc3635` | AMD EPYC 7763 64-Core Processor | — | 422.1 | — | — |
| 2026-09-30 | irix7/iris | `1a93808d` | AMD EPYC 7763 64-Core Processor | 60.2 | — | — | — |
| 2026-09-30 | irix7/iris | `c8dc330d` | AMD EPYC 7763 64-Core Processor | 60.9 | — | — | — |
| 2026-09-30 | irix7/iris | `1a93808d` | AMD EPYC 7763 64-Core Processor | — | 424.6 | — | — |
| 2026-09-30 | irix7/iris | `c8dc330d` | AMD EPYC 7763 64-Core Processor | — | 430.0 | — | — |
| 2026-09-30 | irix7/iris | `0be4d1c5` | AMD EPYC 9V74 80-Core Processor | 83.2 | — | — | — |
| 2026-09-30 | irix7/iris | `0be4d1c5` | AMD EPYC 9V45 96-Core Processor | — | 1168.9 | — | — |
| 2026-09-30 | techomancer/iris | `837b048e` | AMD EPYC 7763 64-Core Processor | 63.0 | 430.0 | 69.0 | 455.1 |
| 2026-09-30 | irix7/iris | `0f5c65c9` | AMD EPYC 7763 64-Core Processor | 61.0 | — | — | — |
| 2026-09-30 | irix7/iris | `0f5c65c9` | INTEL(R) XEON(R) PLATINUM 8573C | — | 707.6 | — | — |
| 2026-09-30 | irix7/iris | `05242a6a` | Intel(R) Xeon(R) 6973P-C | 106.9 | — | — | — |
| 2026-09-30 | irix7/iris | `05242a6a` | AMD EPYC 9V74 80-Core Processor | — | 524.4 | — | — |
| 2026-09-30 | irix7/iris | `6aa3a218` | AMD EPYC 9V74 80-Core Processor | 81.3 | — | — | — |
| 2026-09-30 | irix7/iris | `6aa3a218` | AMD EPYC 7763 64-Core Processor | — | 414.5 | — | — |
| 2026-09-30 | irix7/iris | `e38f38ab` | AMD EPYC 9V45 96-Core Processor | 122.5 | — | — | — |
| 2026-09-30 | irix7/iris | `e38f38ab` | AMD EPYC 9V74 80-Core Processor | — | 679.6 | — | — |
| 2026-10-01 | techomancer/iris | `0f5c65c9` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 68.7 | 637.1 | 52.7 | 505.6 |
| 2026-10-01 | irix7/iris | `9f17c3dc` | AMD EPYC 7763 64-Core Processor | 62.1 | — | — | — |
| 2026-10-01 | irix7/iris | `36bfb4c7` | INTEL(R) XEON(R) PLATINUM 8573C | 86.1 | — | — | — |
| 2026-10-01 | irix7/iris | `9f17c3dc` | AMD EPYC 9V45 96-Core Processor | — | 1142.7 | — | — |
| 2026-10-01 | irix7/iris | `36bfb4c7` | AMD EPYC 7763 64-Core Processor | — | 435.2 | — | — |
| 2026-10-01 | irix7/iris | `71790bd9` | AMD EPYC 7763 64-Core Processor | 61.2 | — | — | — |
| 2026-10-01 | irix7/iris | `71790bd9` | AMD EPYC 7763 64-Core Processor | — | 434.7 | — | — |
| 2026-10-01 | irix7/iris | `4d6e97c1` | AMD EPYC 7763 64-Core Processor | 63.6 | — | — | — |
| 2026-10-01 | irix7/iris | `4d6e97c1` | AMD EPYC 7763 64-Core Processor | — | 431.8 | — | — |
| 2026-10-01 | irix7/iris | `e9e52c65` | Intel(R) Xeon(R) 6973P-C | 111.1 | — | — | — |
| 2026-10-01 | irix7/iris | `e9e52c65` | AMD EPYC 9V74 80-Core Processor | — | 518.0 | — | — |
| 2026-10-01 | irix7/iris | `061b3479` | AMD EPYC 9V74 80-Core Processor | 79.0 | — | — | — |
| 2026-10-01 | irix7/iris | `24144598` | AMD EPYC 9V45 96-Core Processor | 125.2 | — | — | — |
| 2026-10-01 | irix7/iris | `061b3479` | AMD EPYC 7763 64-Core Processor | — | 423.9 | — | — |
| 2026-10-01 | irix7/iris | `24144598` | AMD EPYC 9V74 80-Core Processor | — | 619.7 | — | — |
| 2026-10-01 | irix7/iris | `320c38aa` | AMD EPYC 7763 64-Core Processor | 62.9 | — | — | — |
| 2026-10-01 | irix7/iris | `320c38aa` | AMD EPYC 9V45 96-Core Processor | — | 1144.9 | — | — |
| 2026-10-02 | irix7/iris | `2231d58a` | INTEL(R) XEON(R) PLATINUM 8573C | 96.7 | — | — | — |
| 2026-10-02 | irix7/iris | `2231d58a` | AMD EPYC 7763 64-Core Processor | — | 457.4 | — | — |
| 2026-10-02 | irix7/iris | `e0f0e664` | AMD EPYC 7763 64-Core Processor | 62.9 | — | — | — |
| 2026-10-02 | irix7/iris | `e0f0e664` | AMD EPYC 7763 64-Core Processor | — | 446.4 | — | — |
| 2026-10-02 | irix7/iris | `5582e969` | AMD EPYC 7763 64-Core Processor | 63.5 | — | — | — |
| 2026-10-02 | irix7/iris | `5582e969` | AMD EPYC 7763 64-Core Processor | — | 457.0 | — | — |
| 2026-10-02 | techomancer/iris | `71790bd9` | AMD EPYC 7763 64-Core Processor | 61.4 | 779.1 | 113.2 | 423.8 |
| 2026-10-02 | irix7/iris | `05295175` | AMD EPYC 7763 64-Core Processor | 61.7 | — | — | — |
| 2026-10-02 | irix7/iris | `05295175` | AMD EPYC 7763 64-Core Processor | — | 401.2 | — | — |
| 2026-10-02 | irix7/iris | `0cd1bae0` | AMD EPYC 9V74 80-Core Processor | 80.3 | — | — | — |
| 2026-10-02 | irix7/iris | `0cd1bae0` | AMD EPYC 9V45 96-Core Processor | — | 1121.9 | — | — |
| 2026-10-02 | techomancer/iris | `320c38aa` | AMD EPYC 9V74 80-Core Processor | 79.0 | 456.2 | 69.3 | 363.6 |
| 2026-10-02 | techomancer/iris | `e0f0e664` | AMD EPYC 7763 64-Core Processor | 62.3 | 436.5 | 52.4 | 354.7 |
| 2026-10-02 | techomancer/iris | `05295175` | AMD EPYC 7763 64-Core Processor | 61.6 | 459.8 | 52.8 | 405.1 |
| 2026-10-02 | irix7/iris | `7728ca26` | AMD EPYC 7763 64-Core Processor | 62.9 | — | — | — |
| 2026-10-02 | irix7/iris | `f9eceab3` | Intel(R) Xeon(R) 6973P-C | 104.7 | — | — | — |
| 2026-10-02 | irix7/iris | `7728ca26` | INTEL(R) XEON(R) PLATINUM 8573C | — | 576.6 | — | — |
| 2026-10-02 | irix7/iris | `f9eceab3` | AMD EPYC 9V74 80-Core Processor | — | 533.4 | — | — |
| 2026-10-03 | irix7/iris | `181c8b10` | AMD EPYC 9V74 80-Core Processor | 79.3 | — | — | — |
| 2026-10-03 | irix7/iris | `6bc20822` | AMD EPYC 9V45 96-Core Processor | 127.3 | — | — | — |
| 2026-10-03 | irix7/iris | `181c8b10` | AMD EPYC 7763 64-Core Processor | — | 411.6 | — | — |
| 2026-10-03 | irix7/iris | `6bc20822` | AMD EPYC 9V74 80-Core Processor | — | 624.3 | — | — |
| 2026-10-04 | irix7/iris | `c16e18d9` | AMD EPYC 7763 64-Core Processor | 60.6 | — | — | — |
| 2026-10-04 | irix7/iris | `e966d544` | INTEL(R) XEON(R) PLATINUM 8573C | 96.3 | — | — | — |
| 2026-10-04 | irix7/iris | `c16e18d9` | AMD EPYC 9V45 96-Core Processor | — | 1127.2 | — | — |
| 2026-10-04 | irix7/iris | `e966d544` | AMD EPYC 7763 64-Core Processor | — | 412.0 | — | — |
| 2026-10-04 | irix7/iris | `ce561234` | AMD EPYC 7763 64-Core Processor | 61.9 | — | — | — |
| 2026-10-04 | irix7/iris | `ce561234` | AMD EPYC 7763 64-Core Processor | — | 459.1 | — | — |
| 2026-10-04 | irix7/iris | `5abfa7f7` | AMD EPYC 7763 64-Core Processor | 61.9 | — | — | — |
| 2026-10-04 | irix7/iris | `5abfa7f7` | AMD EPYC 7763 64-Core Processor | — | 437.9 | — | — |
| 2026-10-04 | irix7/iris | `d8b8131e` | AMD EPYC 7763 64-Core Processor | 61.2 | — | — | — |
| 2026-10-04 | irix7/iris | `d6184b81` | AMD EPYC 9V74 80-Core Processor | 81.4 | — | — | — |
| 2026-10-04 | irix7/iris | `d8b8131e` | AMD EPYC 7763 64-Core Processor | — | 485.4 | — | — |
| 2026-10-04 | irix7/iris | `d6184b81` | AMD EPYC 9V45 96-Core Processor | — | 1120.1 | — | — |
| 2026-10-06 | irix7/iris | `862d0fe1` | AMD EPYC 7763 64-Core Processor | 62.7 | — | — | — |
| 2026-10-06 | irix7/iris | `862d0fe1` | INTEL(R) XEON(R) PLATINUM 8573C | — | 739.7 | — | — |
| 2026-10-07 | irix7/iris | `614ec2ea` | Intel(R) Xeon(R) 6973P-C | 89.1 | — | — | — |
| 2026-10-07 | irix7/iris | `614ec2ea` | AMD EPYC 9V74 80-Core Processor | — | 562.3 | — | — |
| 2026-10-07 | irix7/iris | `e6b77c64` | AMD EPYC 9V74 80-Core Processor | 81.8 | — | — | — |
| 2026-10-07 | irix7/iris | `e6b77c64` | AMD EPYC 7763 64-Core Processor | — | 461.1 | — | — |
| 2026-10-07 | irix7/iris | `9f005809` | AMD EPYC 9V45 96-Core Processor | 129.1 | — | — | — |
| 2026-10-07 | irix7/iris | `9f005809` | AMD EPYC 9V74 80-Core Processor | — | 721.5 | — | — |
| 2026-10-08 | irix7/iris | `ee8cddb6` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 65.8 | 1057.6 | 49.6 | 496.0 |
| 2026-10-08 | irix7/iris | `a4d6b7fb` | AMD EPYC 7763 64-Core Processor | 59.5 | 589.0 | 103.2 | 556.5 |
| 2026-10-08 | irix7/iris | `819dd39e` | AMD EPYC 7763 64-Core Processor | 57.9 | — | — | — |
| 2026-10-08 | irix7/iris | `819dd39e` | AMD EPYC 7763 64-Core Processor | — | 589.0 | — | — |
| 2026-10-08 | irix7/iris | `b9b0dd2b` | INTEL(R) XEON(R) PLATINUM 8573C | 88.0 | 570.4 | 60.1 | 514.1 |
| 2026-10-08 | irix7/iris | `f05a350a` | AMD EPYC 7763 64-Core Processor | 56.5 | — | — | — |
| 2026-10-08 | irix7/iris | `f05a350a` | INTEL(R) XEON(R) PLATINUM 8573C | — | 672.8 | — | — |
| 2026-10-08 | irix7/iris | `1528d703` | AMD EPYC 7763 64-Core Processor | 64.0 | — | — | — |
| 2026-10-08 | irix7/iris | `1528d703` | AMD EPYC 9V45 96-Core Processor | — | 1101.6 | — | — |
| 2026-10-08 | irix7/iris | `0aacb755` | INTEL(R) XEON(R) PLATINUM 8573C | 98.8 | — | — | — |
| 2026-10-08 | irix7/iris | `0aacb755` | AMD EPYC 7763 64-Core Processor | — | 458.5 | — | — |
| 2026-10-08 | irix7/iris | `9d1988a3` | AMD EPYC 7763 64-Core Processor | 56.5 | — | — | — |
| 2026-10-08 | irix7/iris | `9d1988a3` | AMD EPYC 7763 64-Core Processor | — | 596.0 | — | — |
| 2026-10-08 | irix7/iris | `df5df542` | AMD EPYC 7763 64-Core Processor | 57.2 | — | — | — |
| 2026-10-08 | irix7/iris | `df5df542` | AMD EPYC 7763 64-Core Processor | — | 613.2 | — | — |
| 2026-10-08 | irix7/iris | `947a60c1` | AMD EPYC 7763 64-Core Processor | 57.4 | — | — | — |
| 2026-10-08 | irix7/iris | `947a60c1` | AMD EPYC 7763 64-Core Processor | — | 593.2 | — | — |
| 2026-10-08 | irix7/iris | `1bd24a1c` | AMD EPYC 9V74 80-Core Processor | 73.2 | — | — | — |
| 2026-10-08 | irix7/iris | `1bd24a1c` | AMD EPYC 9V45 96-Core Processor | — | 1127.0 | — | — |
| 2026-10-08 | irix7/iris | `d90dcc2d` | AMD EPYC 7763 64-Core Processor | 56.7 | — | — | — |
| 2026-10-08 | irix7/iris | `d90dcc2d` | INTEL(R) XEON(R) PLATINUM 8573C | — | 697.9 | — | — |
| 2026-10-08 | irix7/iris | `ee8cddb6` | Intel(R) Xeon(R) 6973P-C | 110.5 | — | — | — |
| 2026-10-08 | irix7/iris | `ee8cddb6` | AMD EPYC 9V74 80-Core Processor | — | 593.3 | — | — |
| 2026-10-08 | irix7/iris | `784f6411` | AMD EPYC 9V74 80-Core Processor | 74.0 | — | — | — |
| 2026-10-08 | irix7/iris | `784f6411` | AMD EPYC 7763 64-Core Processor | — | 589.8 | — | — |
| 2026-10-08 | irix7/iris | `06b45eec` | AMD EPYC 9V45 96-Core Processor | 124.5 | — | — | — |
| 2026-10-08 | irix7/iris | `06b45eec` | AMD EPYC 9V74 80-Core Processor | — | 781.2 | — | — |
| 2026-10-08 | irix7/iris | `ac957abc` | AMD EPYC 7763 64-Core Processor | 57.8 | — | — | — |
| 2026-10-08 | irix7/iris | `ac957abc` | AMD EPYC 9V45 96-Core Processor | — | 1102.1 | — | — |
| 2026-10-08 | irix7/iris | `8ea03119` | INTEL(R) XEON(R) PLATINUM 8573C | 96.9 | — | — | — |
| 2026-10-08 | irix7/iris | `8ea03119` | AMD EPYC 7763 64-Core Processor | — | 588.0 | — | — |
| 2026-10-08 | irix7/iris | `a4d6b7fb` | AMD EPYC 7763 64-Core Processor | 57.4 | — | — | — |
| 2026-10-08 | irix7/iris | `a4d6b7fb` | AMD EPYC 7763 64-Core Processor | — | 603.7 | — | — |
| 2026-10-08 | irix7/iris | `5e72b960` | AMD EPYC 7763 64-Core Processor | 56.6 | — | — | — |
| 2026-10-08 | irix7/iris | `5e72b960` | AMD EPYC 7763 64-Core Processor | — | 592.2 | — | — |
| 2026-10-08 | irix7/iris | `b9b0dd2b` | AMD EPYC 9V74 80-Core Processor | 74.1 | — | — | — |
| 2026-10-08 | irix7/iris | `b9b0dd2b` | AMD EPYC 9V45 96-Core Processor | — | 1118.1 | — | — |
| 2026-10-08 | irix7/iris | `258dee07` | Intel(R) Xeon(R) 6973P-C | 111.4 | — | — | — |
| 2026-10-08 | irix7/iris | `258dee07` | AMD EPYC 9V74 80-Core Processor | — | 613.4 | — | — |
| 2026-10-08 | irix7/iris | `71758983` | AMD EPYC 9V74 80-Core Processor | 73.9 | — | — | — |
| 2026-10-08 | irix7/iris | `71758983` | AMD EPYC 7763 64-Core Processor | — | 582.7 | — | — |
| 2026-10-08 | irix7/iris | `55cd2fe7` | AMD EPYC 9V45 96-Core Processor | 121.8 | — | — | — |
| 2026-10-08 | irix7/iris | `55cd2fe7` | AMD EPYC 9V74 80-Core Processor | — | 755.6 | — | — |
| 2026-10-08 | irix7/iris | `b8c75dd1` | AMD EPYC 7763 64-Core Processor | 57.6 | — | — | — |
| 2026-10-08 | irix7/iris | `b8c75dd1` | AMD EPYC 9V45 96-Core Processor | — | 1134.8 | — | — |
| 2026-10-08 | irix7/iris | `fe5ba69a` | INTEL(R) XEON(R) PLATINUM 8573C | 96.2 | — | — | — |
| 2026-10-08 | irix7/iris | `fe5ba69a` | AMD EPYC 7763 64-Core Processor | — | 592.9 | — | — |
| 2026-10-08 | irix7/iris | `c29ff7f5` | AMD EPYC 7763 64-Core Processor | 57.1 | — | — | — |
| 2026-10-08 | irix7/iris | `c29ff7f5` | AMD EPYC 7763 64-Core Processor | — | 581.5 | — | — |
| 2026-10-08 | irix7/iris | `817513e2` | AMD EPYC 7763 64-Core Processor | 56.6 | — | — | — |
| 2026-10-08 | irix7/iris | `817513e2` | AMD EPYC 7763 64-Core Processor | — | 587.6 | — | — |
| 2026-10-08 | irix7/iris | `8877a691` | AMD EPYC 7763 64-Core Processor | 57.6 | — | — | — |
| 2026-10-08 | irix7/iris | `8877a691` | AMD EPYC 7763 64-Core Processor | — | 594.4 | — | — |
