# Benchmark history

One row per recorded CI run. Throughput figures are only comparable
within the same host CPU — GitHub's shared runners vary between runs,
so cross-host MIPS deltas are host noise, not code change.

| date | source | commit | host | r4400-interp | r4400-jitv2 | r5000-interp | r5000-jitv2 |
|---|---|---|---||---:|---:|---:|---:|
| 2026-08-22 | irix7/iris | `b27be2a0` | AMD EPYC 7763 64-Core Processor | 62.2 | — | — | — |
| 2026-08-22 | irix7/iris | `9ea3e755` | AMD EPYC 9V45 96-Core Processor | 131.9 | — | — | — |
| 2026-08-22 | irix7/iris | `d21f9661` | INTEL(R) XEON(R) PLATINUM 8573C | 84.0 | — | — | — |
| 2026-08-22 | irix7/iris | `3ca2a186` | AMD EPYC 7763 64-Core Processor | 62.6 | — | — | — |
| 2026-08-22 | irix7/iris | `0a3a5972` | AMD EPYC 7763 64-Core Processor | 62.5 | — | — | — |
| 2026-08-22 | irix7/iris | `98332eda` | Intel(R) Xeon(R) 6973P-C | 105.6 | — | — | — |
| 2026-08-22 | irix7/iris | `b27be2a0` | AMD EPYC 9V74 80-Core Processor | — | 444.8 | — | — |
| 2026-08-22 | irix7/iris | `d21f9661` | AMD EPYC 7763 64-Core Processor | — | 224.1 | — | — |
| 2026-08-22 | irix7/iris | `3ca2a186` | AMD EPYC 9V45 96-Core Processor | — | 645.1 | — | — |
| 2026-08-22 | irix7/iris | `0a3a5972` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 226.9 | — | — |
| 2026-08-22 | irix7/iris | `98332eda` | AMD EPYC 7763 64-Core Processor | — | 222.2 | — | — |
| 2026-08-22 | irix7/iris | `0aa29c02` | AMD EPYC 9V74 80-Core Processor | 80.4 | — | — | — |
| 2026-08-22 | irix7/iris | `0aa29c02` | AMD EPYC 7763 64-Core Processor | — | 296.1 | — | — |
| 2026-08-22 | irix7/iris | `2537f0a5` | AMD EPYC 9V45 96-Core Processor | 138.7 | — | — | — |
| 2026-08-22 | irix7/iris | `2537f0a5` | AMD EPYC 9V45 96-Core Processor | — | 758.1 | — | — |
| 2026-08-22 | irix7/iris | `02c4e155` | INTEL(R) XEON(R) PLATINUM 8573C | 83.6 | — | — | — |
| 2026-08-22 | irix7/iris | `02c4e155` | AMD EPYC 9V45 96-Core Processor | — | 803.3 | — | — |
| 2026-08-22 | irix7/iris | `00cd5df7` | AMD EPYC 9V45 96-Core Processor | 136.1 | — | — | — |
| 2026-08-22 | irix7/iris | `00cd5df7` | AMD EPYC 9V45 96-Core Processor | — | 815.1 | — | — |
| 2026-08-22 | irix7/iris | `41865506` | AMD EPYC 7763 64-Core Processor | 63.1 | — | — | — |
| 2026-08-22 | irix7/iris | `41865506` | AMD EPYC 9V74 80-Core Processor | — | 487.0 | — | — |
| 2026-08-22 | irix7/iris | `3f14f1a1` | AMD EPYC 9V45 96-Core Processor | 131.9 | — | — | — |
| 2026-08-22 | irix7/iris | `3f14f1a1` | INTEL(R) XEON(R) PLATINUM 8573C | — | 448.5 | — | — |
| 2026-08-23 | irix7/iris | `8a9de6c8` | INTEL(R) XEON(R) PLATINUM 8573C | 81.0 | — | — | — |
| 2026-08-23 | irix7/iris | `8a9de6c8` | AMD EPYC 7763 64-Core Processor | — | 294.0 | — | — |
| 2026-08-23 | irix7/iris | `6e86811d` | AMD EPYC 7763 64-Core Processor | 62.4 | — | — | — |
| 2026-08-23 | irix7/iris | `6e86811d` | AMD EPYC 9V45 96-Core Processor | — | 775.3 | — | — |
| 2026-08-23 | irix7/iris | `c2caadf9` | AMD EPYC 7763 64-Core Processor | 61.2 | — | — | — |
| 2026-08-23 | irix7/iris | `c2caadf9` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 313.4 | — | — |
| 2026-08-25 | irix7/iris | `73a10e26` | Intel(R) Xeon(R) 6973P-C | 106.8 | — | — | — |
| 2026-08-25 | irix7/iris | `73a10e26` | AMD EPYC 7763 64-Core Processor | — | 297.1 | — | — |
| 2026-08-25 | irix7/iris | `1a1274a4` | AMD EPYC 9V74 80-Core Processor | 81.8 | — | — | — |
| 2026-08-25 | irix7/iris | `1a1274a4` | AMD EPYC 7763 64-Core Processor | — | 295.2 | — | — |
| 2026-08-25 | irix7/iris | `caf9ca2b` | AMD EPYC 9V45 96-Core Processor | 142.0 | — | — | — |
| 2026-08-25 | irix7/iris | `caf9ca2b` | AMD EPYC 9V45 96-Core Processor | — | 761.4 | — | — |
| 2026-08-25 | irix7/iris | `dfc80233` | INTEL(R) XEON(R) PLATINUM 8573C | 82.8 | — | — | — |
| 2026-08-25 | irix7/iris | `dfc80233` | AMD EPYC 9V45 96-Core Processor | — | 782.6 | — | — |
| 2026-08-25 | irix7/iris | `2ad9f00b` | AMD EPYC 9V45 96-Core Processor | 128.9 | — | — | — |
| 2026-08-25 | irix7/iris | `2ad9f00b` | AMD EPYC 9V45 96-Core Processor | — | 800.9 | — | — |
| 2026-08-25 | irix7/iris | `2598df25` | AMD EPYC 7763 64-Core Processor | 64.1 | — | — | — |
| 2026-08-25 | irix7/iris | `2598df25` | AMD EPYC 9V74 80-Core Processor | — | 482.1 | — | — |
| 2026-08-25 | irix7/iris | `a99d94ae` | INTEL(R) XEON(R) PLATINUM 8573C | — | 469.9 | — | — |
| 2026-08-25 | irix7/iris | `a408811d` | INTEL(R) XEON(R) PLATINUM 8573C | 80.3 | — | — | — |
| 2026-08-25 | irix7/iris | `a408811d` | AMD EPYC 7763 64-Core Processor | — | 295.8 | — | — |
| 2026-08-25 | irix7/iris | `d549a6eb` | AMD EPYC 7763 64-Core Processor | 63.9 | — | — | — |
| 2026-08-25 | irix7/iris | `d549a6eb` | AMD EPYC 9V45 96-Core Processor | — | 762.2 | — | — |
| 2026-08-26 | irix7/iris | `2ad1d072` | AMD EPYC 7763 64-Core Processor | 64.4 | — | — | — |
| 2026-08-26 | irix7/iris | `2ad1d072` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 334.2 | — | — |
| 2026-08-26 | irix7/iris | `81f1b205` | Intel(R) Xeon(R) 6973P-C | 106.0 | — | — | — |
| 2026-08-26 | irix7/iris | `81f1b205` | AMD EPYC 7763 64-Core Processor | — | 293.5 | — | — |
| 2026-08-26 | irix7/iris | `060b2d9e` | AMD EPYC 9V74 80-Core Processor | 82.8 | — | — | — |
| 2026-08-26 | irix7/iris | `060b2d9e` | AMD EPYC 7763 64-Core Processor | — | 288.9 | — | — |
| 2026-08-27 | irix7/iris | `a55cbc81` | AMD EPYC 9V45 96-Core Processor | 142.8 | — | — | — |
| 2026-08-27 | irix7/iris | `a55cbc81` | AMD EPYC 9V45 96-Core Processor | — | 773.1 | — | — |
| 2026-08-27 | irix7/iris | `e804d22a` | INTEL(R) XEON(R) PLATINUM 8573C | 85.5 | — | — | — |
| 2026-08-27 | irix7/iris | `e804d22a` | AMD EPYC 9V45 96-Core Processor | — | 787.1 | — | — |
| 2026-08-27 | irix7/iris | `c8e42865` | AMD EPYC 9V45 96-Core Processor | 136.1 | — | — | — |
| 2026-08-27 | irix7/iris | `c8e42865` | AMD EPYC 9V45 96-Core Processor | — | 998.8 | — | — |
| 2026-08-27 | irix7/iris | `4bc2e43b` | AMD EPYC 7763 64-Core Processor | 65.0 | — | — | — |
| 2026-08-27 | irix7/iris | `4bc2e43b` | AMD EPYC 9V74 80-Core Processor | — | 625.2 | — | — |
| 2026-08-28 | irix7/iris | `37cf1226` | AMD EPYC 9V45 96-Core Processor | 137.4 | — | — | — |
| 2026-08-28 | irix7/iris | `37cf1226` | INTEL(R) XEON(R) PLATINUM 8573C | — | 566.7 | — | — |
| 2026-08-29 | irix7/iris | `980875fe` | INTEL(R) XEON(R) PLATINUM 8573C | 76.0 | — | — | — |
| 2026-08-29 | irix7/iris | `980875fe` | AMD EPYC 7763 64-Core Processor | — | 407.8 | — | — |
| 2026-08-29 | irix7/iris | `64dd9157` | AMD EPYC 7763 64-Core Processor | 64.8 | — | — | — |
| 2026-08-29 | irix7/iris | `64dd9157` | AMD EPYC 9V45 96-Core Processor | — | 986.5 | — | — |
| 2026-08-29 | irix7/iris | `4ada5a4e` | AMD EPYC 7763 64-Core Processor | 65.4 | — | — | — |
| 2026-08-29 | irix7/iris | `4ada5a4e` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 451.2 | — | — |
| 2026-08-29 | irix7/iris | `35aeb6de` | Intel(R) Xeon(R) 6973P-C | 105.0 | — | — | — |
| 2026-08-29 | irix7/iris | `35aeb6de` | AMD EPYC 7763 64-Core Processor | — | 417.7 | — | — |
| 2026-08-29 | irix7/iris | `6b6de886` | AMD EPYC 9V74 80-Core Processor | 82.6 | — | — | — |
| 2026-08-29 | irix7/iris | `6b6de886` | AMD EPYC 7763 64-Core Processor | — | 415.6 | — | — |
| 2026-08-30 | irix7/iris | `7d4935d0` | AMD EPYC 9V45 96-Core Processor | 144.8 | — | — | — |
| 2026-08-30 | irix7/iris | `7d4935d0` | AMD EPYC 9V45 96-Core Processor | — | 1047.3 | — | — |
| 2026-08-30 | irix7/iris | `de244934` | INTEL(R) XEON(R) PLATINUM 8573C | 80.1 | — | — | — |
| 2026-08-30 | irix7/iris | `de244934` | AMD EPYC 9V45 96-Core Processor | — | 1034.3 | — | — |
| 2026-08-31 | irix7/iris | `5079d2f0` | AMD EPYC 7763 64-Core Processor | 64.2 | — | — | — |
| 2026-08-31 | irix7/iris | `49d77d4d` | AMD EPYC 9V45 96-Core Processor | 137.9 | — | — | — |
| 2026-08-31 | irix7/iris | `f40a1c14` | AMD EPYC 9V45 96-Core Processor | 133.4 | — | — | — |
| 2026-08-31 | irix7/iris | `5079d2f0` | AMD EPYC 9V74 80-Core Processor | — | 614.8 | — | — |
| 2026-08-31 | irix7/iris | `49d77d4d` | INTEL(R) XEON(R) PLATINUM 8573C | — | 581.8 | — | — |
| 2026-08-31 | irix7/iris | `f40a1c14` | AMD EPYC 9V45 96-Core Processor | — | 995.1 | — | — |
| 2026-09-03 | irix7/iris | `fede2427` | AMD EPYC 7763 64-Core Processor | — | 404.5 | — | — |
| 2026-09-03 | irix7/iris | `a77fc060` | AMD EPYC 9V45 96-Core Processor | — | 1001.4 | — | — |
| 2026-09-03 | irix7/iris | `d6568abf` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 441.6 | — | — |
| 2026-09-03 | irix7/iris | `224fc8eb` | Intel(R) Xeon(R) 6973P-C | 110.5 | — | — | — |
| 2026-09-03 | irix7/iris | `224fc8eb` | AMD EPYC 7763 64-Core Processor | — | 428.0 | — | — |
| 2026-09-08 | irix7/iris | `c2e085a3` | AMD EPYC 9V74 80-Core Processor | 82.2 | — | — | — |
| 2026-09-08 | irix7/iris | `c2e085a3` | AMD EPYC 7763 64-Core Processor | — | 432.6 | — | — |
| 2026-09-08 | irix7/iris | `81c34ba4` | AMD EPYC 9V45 96-Core Processor | 142.0 | — | — | — |
| 2026-09-08 | irix7/iris | `81c34ba4` | AMD EPYC 9V45 96-Core Processor | — | 1076.2 | — | — |
| 2026-09-08 | irix7/iris | `8911c1be` | INTEL(R) XEON(R) PLATINUM 8573C | 80.9 | — | — | — |
| 2026-09-08 | irix7/iris | `8911c1be` | AMD EPYC 9V45 96-Core Processor | — | 1065.6 | — | — |
| 2026-09-08 | irix7/iris | `05409919` | AMD EPYC 9V45 96-Core Processor | 143.1 | — | — | — |
| 2026-09-08 | irix7/iris | `05409919` | AMD EPYC 9V45 96-Core Processor | — | 1009.2 | — | — |
| 2026-09-09 | irix7/iris | `aac7674f` | AMD EPYC 7763 64-Core Processor | 64.2 | — | — | — |
| 2026-09-09 | irix7/iris | `aac7674f` | AMD EPYC 9V74 80-Core Processor | — | 646.4 | — | — |
| 2026-09-11 | irix7/iris | `47508124` | AMD EPYC 9V45 96-Core Processor | 138.3 | — | — | — |
| 2026-09-11 | irix7/iris | `0f7f9a60` | INTEL(R) XEON(R) PLATINUM 8573C | 70.6 | — | — | — |
| 2026-09-11 | irix7/iris | `2d98434f` | AMD EPYC 7763 64-Core Processor | 64.3 | — | — | — |
| 2026-09-11 | irix7/iris | `47508124` | INTEL(R) XEON(R) PLATINUM 8573C | — | 617.9 | — | — |
| 2026-09-11 | irix7/iris | `0f7f9a60` | AMD EPYC 7763 64-Core Processor | — | 429.2 | — | — |
| 2026-09-11 | irix7/iris | `2d98434f` | AMD EPYC 9V45 96-Core Processor | — | 1005.3 | — | — |
| 2026-09-12 | irix7/iris | `5150db08` | AMD EPYC 7763 64-Core Processor | 63.1 | — | — | — |
| 2026-09-12 | irix7/iris | `cd8b55de` | Intel(R) Xeon(R) 6973P-C | 90.4 | — | — | — |
| 2026-09-12 | irix7/iris | `5afab5e4` | AMD EPYC 9V74 80-Core Processor | 81.9 | — | — | — |
| 2026-09-12 | irix7/iris | `e293b470` | AMD EPYC 9V45 96-Core Processor | 141.2 | — | — | — |
| 2026-09-12 | irix7/iris | `78742e0d` | INTEL(R) XEON(R) PLATINUM 8573C | 70.9 | — | — | — |
| 2026-09-12 | irix7/iris | `5150db08` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 439.6 | — | — |
| 2026-09-12 | irix7/iris | `cd8b55de` | AMD EPYC 7763 64-Core Processor | — | 439.7 | — | — |
| 2026-09-12 | irix7/iris | `5afab5e4` | AMD EPYC 7763 64-Core Processor | — | 427.1 | — | — |
| 2026-09-12 | irix7/iris | `e293b470` | AMD EPYC 9V45 96-Core Processor | — | 1056.2 | — | — |
| 2026-09-12 | irix7/iris | `78742e0d` | AMD EPYC 9V45 96-Core Processor | — | 1026.1 | — | — |
| 2026-09-12 | irix7/iris | `25f86c10` | AMD EPYC 9V45 96-Core Processor | 143.1 | — | — | — |
| 2026-09-12 | irix7/iris | `25f86c10` | AMD EPYC 9V45 96-Core Processor | — | 1026.1 | — | — |
| 2026-09-12 | irix7/iris | `7ecccf4b` | AMD EPYC 7763 64-Core Processor | 64.8 | — | — | — |
| 2026-09-12 | irix7/iris | `8ab65ffa` | AMD EPYC 9V45 96-Core Processor | 137.2 | — | — | — |
| 2026-09-12 | irix7/iris | `7ecccf4b` | AMD EPYC 9V74 80-Core Processor | — | 631.7 | — | — |
| 2026-09-12 | irix7/iris | `8ab65ffa` | INTEL(R) XEON(R) PLATINUM 8573C | — | 607.8 | — | — |
| 2026-09-13 | irix7/iris | `c1692ae3` | INTEL(R) XEON(R) PLATINUM 8573C | 70.4 | — | — | — |
| 2026-09-13 | irix7/iris | `066935bb` | AMD EPYC 7763 64-Core Processor | 63.1 | — | — | — |
| 2026-09-13 | irix7/iris | `c1692ae3` | AMD EPYC 7763 64-Core Processor | — | 38.4 | — | — |
| 2026-09-13 | irix7/iris | `066935bb` | AMD EPYC 9V45 96-Core Processor | — | 75.5 | — | — |
| 2026-09-13 | irix7/iris | `f8fb34eb` | AMD EPYC 7763 64-Core Processor | 62.7 | — | — | — |
| 2026-09-13 | irix7/iris | `f8fb34eb` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 41.4 | — | — |
| 2026-09-13 | irix7/iris | `4c6e37e1` | Intel(R) Xeon(R) 6973P-C | 89.4 | — | — | — |
| 2026-09-13 | irix7/iris | `4c6e37e1` | AMD EPYC 7763 64-Core Processor | — | 34.3 | — | — |
| 2026-09-15 | irix7/iris | `d75d8ff9` | AMD EPYC 9V74 80-Core Processor | 79.7 | — | — | — |
| 2026-09-15 | irix7/iris | `bb9ef187` | AMD EPYC 9V45 96-Core Processor | 136.3 | — | — | — |
| 2026-09-15 | irix7/iris | `b3f36b8f` | INTEL(R) XEON(R) PLATINUM 8573C | 79.8 | — | — | — |
| 2026-09-15 | irix7/iris | `d75d8ff9` | AMD EPYC 7763 64-Core Processor | — | 37.2 | — | — |
| 2026-09-15 | irix7/iris | `bb9ef187` | AMD EPYC 9V45 96-Core Processor | — | 79.6 | — | — |
| 2026-09-15 | irix7/iris | `b3f36b8f` | AMD EPYC 9V45 96-Core Processor | — | 75.0 | — | — |
| 2026-09-16 | irix7/iris | `9ea63c1f` | AMD EPYC 9V45 96-Core Processor | 135.7 | — | — | — |
| 2026-09-16 | irix7/iris | `9ea63c1f` | AMD EPYC 9V45 96-Core Processor | — | 79.0 | — | — |
| 2026-09-16 | irix7/iris | `1257607e` | AMD EPYC 7763 64-Core Processor | 64.5 | — | — | — |
| 2026-09-16 | irix7/iris | `c84fe8ed` | AMD EPYC 9V45 96-Core Processor | 133.3 | — | — | — |
| 2026-09-16 | irix7/iris | `86248141` | INTEL(R) XEON(R) PLATINUM 8573C | 72.9 | — | — | — |
| 2026-09-16 | irix7/iris | `f76506fd` | AMD EPYC 7763 64-Core Processor | 65.0 | — | — | — |
| 2026-09-16 | irix7/iris | `6cc9e448` | AMD EPYC 7763 64-Core Processor | 64.2 | — | — | — |
| 2026-09-16 | irix7/iris | `520441bd` | Intel(R) Xeon(R) 6973P-C | 100.7 | — | — | — |
| 2026-09-16 | irix7/iris | `51339267` | AMD EPYC 9V74 80-Core Processor | 79.8 | — | — | — |
| 2026-09-16 | irix7/iris | `1257607e` | AMD EPYC 9V74 80-Core Processor | — | 49.1 | — | — |
| 2026-09-16 | irix7/iris | `c84fe8ed` | INTEL(R) XEON(R) PLATINUM 8573C | — | 47.9 | — | — |
| 2026-09-16 | irix7/iris | `86248141` | AMD EPYC 7763 64-Core Processor | — | 37.7 | — | — |
| 2026-09-16 | irix7/iris | `f76506fd` | AMD EPYC 9V45 96-Core Processor | — | 76.4 | — | — |
| 2026-09-16 | irix7/iris | `6cc9e448` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 41.3 | — | — |
| 2026-09-16 | irix7/iris | `520441bd` | AMD EPYC 7763 64-Core Processor | — | 38.0 | — | — |
| 2026-09-16 | irix7/iris | `51339267` | AMD EPYC 7763 64-Core Processor | — | 38.1 | — | — |
| 2026-09-16 | irix7/iris | `f1d0fcb6` | AMD EPYC 9V45 96-Core Processor | 137.3 | — | — | — |
| 2026-09-16 | irix7/iris | `35a3b18a` | INTEL(R) XEON(R) PLATINUM 8573C | 77.0 | — | — | — |
| 2026-09-16 | irix7/iris | `f1d0fcb6` | AMD EPYC 9V45 96-Core Processor | — | 77.3 | — | — |
| 2026-09-16 | irix7/iris | `35a3b18a` | AMD EPYC 9V45 96-Core Processor | — | 80.2 | — | — |
| 2026-09-17 | irix7/iris | `e93c5bb5` | AMD EPYC 9V45 96-Core Processor | 136.8 | — | — | — |
| 2026-09-17 | irix7/iris | `e93c5bb5` | AMD EPYC 9V45 96-Core Processor | — | 79.0 | — | — |
| 2026-09-17 | irix7/iris | `c55de0b9` | AMD EPYC 7763 64-Core Processor | 63.7 | — | — | — |
| 2026-09-17 | irix7/iris | `c55de0b9` | AMD EPYC 9V74 80-Core Processor | — | 50.3 | — | — |
| 2026-09-18 | irix7/iris | `44f4b243` | AMD EPYC 9V45 96-Core Processor | 132.3 | — | — | — |
| 2026-09-18 | irix7/iris | `44f4b243` | INTEL(R) XEON(R) PLATINUM 8573C | — | 50.4 | — | — |
| 2026-09-18 | irix7/iris | `edccb811` | INTEL(R) XEON(R) PLATINUM 8573C | 70.1 | — | — | — |
| 2026-09-18 | irix7/iris | `156936a0` | AMD EPYC 7763 64-Core Processor | 63.0 | — | — | — |
| 2026-09-18 | irix7/iris | `edccb811` | AMD EPYC 7763 64-Core Processor | — | 38.3 | — | — |
| 2026-09-18 | irix7/iris | `156936a0` | AMD EPYC 9V45 96-Core Processor | — | 76.9 | — | — |
| 2026-09-18 | irix7/iris | `1390bf6e` | AMD EPYC 7763 64-Core Processor | 62.6 | — | — | — |
| 2026-09-18 | irix7/iris | `1390bf6e` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 42.1 | — | — |
| 2026-09-18 | irix7/iris | `66d5151e` | Intel(R) Xeon(R) 6973P-C | 97.4 | — | — | — |
| 2026-09-18 | irix7/iris | `66d5151e` | AMD EPYC 7763 64-Core Processor | — | 38.5 | — | — |
| 2026-09-18 | irix7/iris | `45f26b8b` | AMD EPYC 9V74 80-Core Processor | 80.0 | — | — | — |
| 2026-09-18 | irix7/iris | `e603daff` | AMD EPYC 9V45 96-Core Processor | 138.2 | — | — | — |
| 2026-09-18 | irix7/iris | `45f26b8b` | AMD EPYC 7763 64-Core Processor | — | 38.0 | — | — |
| 2026-09-18 | irix7/iris | `e603daff` | AMD EPYC 9V45 96-Core Processor | — | 78.9 | — | — |
| 2026-09-18 | irix7/iris | `f1a561d0` | INTEL(R) XEON(R) PLATINUM 8573C | 83.3 | — | — | — |
| 2026-09-18 | irix7/iris | `f1a561d0` | AMD EPYC 9V45 96-Core Processor | — | 75.3 | — | — |
| 2026-09-19 | irix7/iris | `d1bf817f` | AMD EPYC 9V45 96-Core Processor | 134.0 | — | — | — |
| 2026-09-19 | irix7/iris | `d1bf817f` | AMD EPYC 9V45 96-Core Processor | — | 76.5 | — | — |
| 2026-09-19 | irix7/iris | `eab2df28` | AMD EPYC 7763 64-Core Processor | 64.6 | — | — | — |
| 2026-09-19 | irix7/iris | `eab2df28` | AMD EPYC 9V74 80-Core Processor | — | 48.8 | — | — |
| 2026-09-19 | irix7/iris | `84344461` | AMD EPYC 9V45 96-Core Processor | 132.5 | — | — | — |
| 2026-09-19 | irix7/iris | `84344461` | INTEL(R) XEON(R) PLATINUM 8573C | — | 48.0 | — | — |
| 2026-09-19 | irix7/iris | `39d304f6` | INTEL(R) XEON(R) PLATINUM 8573C | 70.4 | — | — | — |
| 2026-09-19 | irix7/iris | `39d304f6` | AMD EPYC 7763 64-Core Processor | — | 37.9 | — | — |
| 2026-09-19 | irix7/iris | `18109d80` | AMD EPYC 7763 64-Core Processor | 64.5 | — | — | — |
| 2026-09-19 | irix7/iris | `18109d80` | AMD EPYC 9V45 96-Core Processor | — | 74.0 | — | — |
| 2026-09-19 | irix7/iris | `b6440aad` | AMD EPYC 7763 64-Core Processor | 64.9 | — | — | — |
| 2026-09-19 | irix7/iris | `b6440aad` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 41.9 | — | — |
| 2026-09-19 | irix7/iris | `6ab3f112` | Intel(R) Xeon(R) 6973P-C | 99.3 | — | — | — |
| 2026-09-19 | irix7/iris | `6ab3f112` | AMD EPYC 7763 64-Core Processor | — | 38.5 | — | — |
| 2026-09-19 | irix7/iris | `2b55ed61` | AMD EPYC 9V74 80-Core Processor | 78.5 | — | — | — |
| 2026-09-19 | irix7/iris | `5219128d` | AMD EPYC 9V45 96-Core Processor | 135.4 | — | — | — |
| 2026-09-19 | irix7/iris | `2b55ed61` | AMD EPYC 7763 64-Core Processor | — | 38.4 | — | — |
| 2026-09-19 | irix7/iris | `5219128d` | AMD EPYC 9V45 96-Core Processor | — | 80.4 | — | — |
| 2026-09-19 | irix7/iris | `a68e7a36` | INTEL(R) XEON(R) PLATINUM 8573C | 78.6 | — | — | — |
| 2026-09-19 | irix7/iris | `d5d695e1` | AMD EPYC 9V45 96-Core Processor | 137.6 | — | — | — |
| 2026-09-19 | irix7/iris | `a68e7a36` | AMD EPYC 9V45 96-Core Processor | — | 79.8 | — | — |
| 2026-09-19 | irix7/iris | `d5d695e1` | AMD EPYC 9V45 96-Core Processor | — | 76.7 | — | — |
| 2026-09-19 | irix7/iris | `b7bca7c4` | AMD EPYC 7763 64-Core Processor | 63.2 | — | — | — |
| 2026-09-19 | irix7/iris | `b7bca7c4` | AMD EPYC 9V74 80-Core Processor | — | 48.6 | — | — |
| 2026-09-20 | irix7/iris | `a6f773e2` | AMD EPYC 9V45 96-Core Processor | 132.4 | — | — | — |
| 2026-09-20 | irix7/iris | `0f1a3a2d` | INTEL(R) XEON(R) PLATINUM 8573C | 65.7 | — | — | — |
| 2026-09-20 | irix7/iris | `e76520e0` | AMD EPYC 7763 64-Core Processor | 64.1 | — | — | — |
| 2026-09-20 | irix7/iris | `3f51ff0f` | AMD EPYC 7763 64-Core Processor | 63.1 | — | — | — |
| 2026-09-20 | irix7/iris | `e03b9b80` | Intel(R) Xeon(R) 6973P-C | 96.0 | — | — | — |
| 2026-09-20 | irix7/iris | `44e9b9e2` | AMD EPYC 9V74 80-Core Processor | 80.8 | — | — | — |
| 2026-09-20 | irix7/iris | `a6f773e2` | INTEL(R) XEON(R) PLATINUM 8573C | — | 47.2 | — | — |
| 2026-09-20 | irix7/iris | `0f1a3a2d` | AMD EPYC 7763 64-Core Processor | — | 38.0 | — | — |
| 2026-09-20 | irix7/iris | `e76520e0` | AMD EPYC 9V45 96-Core Processor | — | 75.6 | — | — |
| 2026-09-20 | irix7/iris | `3f51ff0f` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 41.8 | — | — |
| 2026-09-20 | irix7/iris | `e03b9b80` | AMD EPYC 7763 64-Core Processor | — | 38.3 | — | — |
| 2026-09-20 | irix7/iris | `44e9b9e2` | AMD EPYC 7763 64-Core Processor | — | 38.3 | — | — |
| 2026-09-21 | irix7/iris | `8dfc3650` | AMD EPYC 9V45 96-Core Processor | 136.3 | — | — | — |
| 2026-09-21 | irix7/iris | `da2bb7d5` | INTEL(R) XEON(R) PLATINUM 8573C | 75.8 | — | — | — |
| 2026-09-21 | irix7/iris | `8dfc3650` | AMD EPYC 9V45 96-Core Processor | — | 75.6 | — | — |
| 2026-09-21 | irix7/iris | `da2bb7d5` | AMD EPYC 9V45 96-Core Processor | — | 75.8 | — | — |
| 2026-09-21 | irix7/iris | `bea0af66` | AMD EPYC 9V45 96-Core Processor | 137.0 | — | — | — |
| 2026-09-21 | irix7/iris | `bea0af66` | AMD EPYC 9V45 96-Core Processor | — | 76.2 | — | — |
| 2026-09-25 | irix7/iris | `b0e6f7bc` | AMD EPYC 7763 64-Core Processor | 62.7 | — | — | — |
| 2026-09-25 | irix7/iris | `b0e6f7bc` | AMD EPYC 9V74 80-Core Processor | — | 50.0 | — | — |
| 2026-09-27 | irix7/iris | `c5e3ad4b` | AMD EPYC 9V45 96-Core Processor | 128.8 | — | — | — |
| 2026-09-27 | irix7/iris | `c5e3ad4b` | INTEL(R) XEON(R) PLATINUM 8573C | — | 45.0 | — | — |
| 2026-09-28 | irix7/iris | `5d07b4d6` | INTEL(R) XEON(R) PLATINUM 8573C | 70.6 | — | — | — |
| 2026-09-28 | irix7/iris | `5d07b4d6` | AMD EPYC 7763 64-Core Processor | — | 35.9 | — | — |
| 2026-09-29 | irix7/iris | `2383f267` | AMD EPYC 7763 64-Core Processor | 63.3 | — | — | — |
| 2026-09-29 | irix7/iris | `30fb4ffe` | AMD EPYC 7763 64-Core Processor | 63.3 | — | — | — |
| 2026-09-29 | irix7/iris | `a528f183` | Intel(R) Xeon(R) 6973P-C | 99.0 | — | — | — |
| 2026-09-29 | irix7/iris | `e8060668` | AMD EPYC 9V74 80-Core Processor | 79.7 | — | — | — |
| 2026-09-29 | irix7/iris | `2383f267` | AMD EPYC 9V45 96-Core Processor | — | 70.1 | — | — |
| 2026-09-29 | irix7/iris | `30fb4ffe` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 40.1 | — | — |
| 2026-09-29 | irix7/iris | `a528f183` | AMD EPYC 7763 64-Core Processor | — | 37.2 | — | — |
| 2026-09-29 | irix7/iris | `e8060668` | AMD EPYC 7763 64-Core Processor | — | 38.7 | — | — |
| 2026-09-29 | irix7/iris | `bb286ea7` | AMD EPYC 9V45 96-Core Processor | 138.1 | — | — | — |
| 2026-09-29 | irix7/iris | `bb286ea7` | AMD EPYC 9V45 96-Core Processor | — | 79.5 | — | — |
| 2026-09-29 | irix7/iris | `36ec12b6` | INTEL(R) XEON(R) PLATINUM 8573C | 79.0 | — | — | — |
| 2026-09-29 | irix7/iris | `36ec12b6` | AMD EPYC 9V45 96-Core Processor | — | 73.2 | — | — |
| 2026-09-29 | irix7/iris | `70a5e394` | AMD EPYC 9V45 96-Core Processor | 136.9 | — | — | — |
| 2026-09-29 | irix7/iris | `70a5e394` | AMD EPYC 9V45 96-Core Processor | — | 70.3 | — | — |
| 2026-09-29 | irix7/iris | `e6bde884` | AMD EPYC 7763 64-Core Processor | 62.4 | — | — | — |
| 2026-09-29 | irix7/iris | `e6bde884` | AMD EPYC 9V74 80-Core Processor | — | 49.1 | — | — |
| 2026-09-29 | irix7/iris | `b7d2a18b` | AMD EPYC 9V45 96-Core Processor | 134.2 | — | — | — |
| 2026-09-29 | irix7/iris | `b7d2a18b` | INTEL(R) XEON(R) PLATINUM 8573C | — | 44.4 | — | — |
| 2026-09-29 | irix7/iris | `da4223ab` | INTEL(R) XEON(R) PLATINUM 8573C | 73.8 | — | — | — |
| 2026-09-29 | irix7/iris | `da4223ab` | AMD EPYC 7763 64-Core Processor | — | 449.0 | — | — |
| 2026-09-29 | irix7/iris | `a09186a6` | AMD EPYC 7763 64-Core Processor | 63.4 | — | — | — |
| 2026-09-29 | irix7/iris | `a09186a6` | AMD EPYC 9V45 96-Core Processor | — | 1166.0 | — | — |
| 2026-09-29 | irix7/iris | `772e01c2` | AMD EPYC 7763 64-Core Processor | 66.3 | — | — | — |
| 2026-09-29 | irix7/iris | `772e01c2` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 466.0 | — | — |
| 2026-09-29 | irix7/iris | `3e384084` | Intel(R) Xeon(R) 6973P-C | 100.3 | — | — | — |
| 2026-09-29 | irix7/iris | `3e384084` | AMD EPYC 7763 64-Core Processor | — | 459.9 | — | — |
| 2026-09-29 | irix7/iris | `38e510ca` | AMD EPYC 9V74 80-Core Processor | 83.1 | — | — | — |
| 2026-09-29 | irix7/iris | `38e510ca` | AMD EPYC 7763 64-Core Processor | — | 420.8 | — | — |
| 2026-09-29 | irix7/iris | `95b7ad0a` | AMD EPYC 9V45 96-Core Processor | 143.9 | — | — | — |
| 2026-09-29 | irix7/iris | `95b7ad0a` | AMD EPYC 9V45 96-Core Processor | — | 1182.3 | — | — |
| 2026-09-30 | irix7/iris | `839155a2` | AMD EPYC 7763 64-Core Processor | 62.5 | — | — | — |
| 2026-09-30 | irix7/iris | `afb8d1ba` | AMD EPYC 9V45 96-Core Processor | 131.8 | — | — | — |
| 2026-09-30 | irix7/iris | `837b048e` | INTEL(R) XEON(R) PLATINUM 8573C | 74.3 | — | — | — |
| 2026-09-30 | irix7/iris | `0b1ea38c` | INTEL(R) XEON(R) PLATINUM 8573C | 83.4 | — | — | — |
| 2026-09-30 | irix7/iris | `85796146` | AMD EPYC 9V45 96-Core Processor | 134.4 | — | — | — |
| 2026-09-30 | irix7/iris | `839155a2` | AMD EPYC 9V74 80-Core Processor | — | 711.4 | — | — |
| 2026-09-30 | irix7/iris | `afb8d1ba` | INTEL(R) XEON(R) PLATINUM 8573C | — | 676.5 | — | — |
| 2026-09-30 | irix7/iris | `837b048e` | AMD EPYC 7763 64-Core Processor | — | 430.3 | — | — |
| 2026-09-30 | irix7/iris | `0b1ea38c` | AMD EPYC 9V45 96-Core Processor | — | 1195.9 | — | — |
| 2026-09-30 | irix7/iris | `85796146` | AMD EPYC 9V45 96-Core Processor | — | 1175.5 | — | — |
| 2026-09-30 | techomancer/iris | `70a5e394` | AMD EPYC 7763 64-Core Processor | 63.5 | 48.7 | 51.4 | 31.7 |
| 2026-09-30 | irix7/iris | `08be2936` | AMD EPYC 7763 64-Core Processor | 61.1 | — | — | — |
| 2026-09-30 | irix7/iris | `08be2936` | AMD EPYC 9V45 96-Core Processor | — | 1206.9 | — | — |
| 2026-09-30 | techomancer/iris | `b7d2a18b` | AMD EPYC 9V74 80-Core Processor | 81.5 | 50.7 | 51.7 | 30.1 |
| 2026-09-30 | techomancer/iris | `da4223ab` | Intel(R) Xeon(R) 6973P-C | 119.6 | 405.0 | 51.5 | 1062.6 |
| 2026-09-30 | techomancer/iris | `a09186a6` | AMD EPYC 9V74 80-Core Processor | 83.5 | 1190.8 | 110.3 | 387.7 |
| 2026-09-30 | irix7/iris | `2a796e2a` | AMD EPYC 7763 64-Core Processor | 61.0 | — | — | — |
| 2026-09-30 | irix7/iris | `5038af99` | Intel(R) Xeon(R) 6973P-C | 94.2 | — | — | — |
| 2026-09-30 | irix7/iris | `98a523ea` | AMD EPYC 9V74 80-Core Processor | 81.9 | — | — | — |
| 2026-09-30 | irix7/iris | `2a796e2a` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 468.5 | — | — |
| 2026-09-30 | irix7/iris | `5038af99` | AMD EPYC 7763 64-Core Processor | — | 439.9 | — | — |
| 2026-09-30 | irix7/iris | `98a523ea` | AMD EPYC 7763 64-Core Processor | — | 413.9 | — | — |
| 2026-09-30 | irix7/iris | `5787d027` | AMD EPYC 9V45 96-Core Processor | 131.0 | — | — | — |
| 2026-09-30 | irix7/iris | `5787d027` | AMD EPYC 9V45 96-Core Processor | — | 1195.8 | — | — |
| 2026-09-30 | irix7/iris | `b5cc3635` | INTEL(R) XEON(R) PLATINUM 8573C | 77.0 | — | — | — |
| 2026-09-30 | irix7/iris | `b5cc3635` | AMD EPYC 9V45 96-Core Processor | — | 1132.0 | — | — |
| 2026-09-30 | irix7/iris | `1a93808d` | AMD EPYC 7763 64-Core Processor | 61.1 | — | — | — |
| 2026-09-30 | irix7/iris | `c8dc330d` | AMD EPYC 9V45 96-Core Processor | 132.8 | — | — | — |
| 2026-09-30 | irix7/iris | `1a93808d` | AMD EPYC 9V74 80-Core Processor | — | 704.2 | — | — |
| 2026-09-30 | irix7/iris | `c8dc330d` | AMD EPYC 9V45 96-Core Processor | — | 1166.5 | — | — |
| 2026-09-30 | irix7/iris | `0be4d1c5` | AMD EPYC 9V45 96-Core Processor | 129.3 | — | — | — |
| 2026-09-30 | irix7/iris | `0be4d1c5` | INTEL(R) XEON(R) PLATINUM 8573C | — | 676.9 | — | — |
| 2026-09-30 | techomancer/iris | `837b048e` | AMD EPYC 7763 64-Core Processor | 63.0 | 430.0 | 69.0 | 455.1 |
| 2026-09-30 | irix7/iris | `0f5c65c9` | INTEL(R) XEON(R) PLATINUM 8573C | 72.5 | — | — | — |
| 2026-09-30 | irix7/iris | `0f5c65c9` | AMD EPYC 7763 64-Core Processor | — | 448.6 | — | — |
| 2026-09-30 | irix7/iris | `05242a6a` | AMD EPYC 7763 64-Core Processor | 61.5 | — | — | — |
| 2026-09-30 | irix7/iris | `05242a6a` | AMD EPYC 9V45 96-Core Processor | — | 1137.9 | — | — |
| 2026-09-30 | irix7/iris | `6aa3a218` | AMD EPYC 7763 64-Core Processor | 62.2 | — | — | — |
| 2026-09-30 | irix7/iris | `6aa3a218` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 469.6 | — | — |
| 2026-09-30 | irix7/iris | `e38f38ab` | Intel(R) Xeon(R) 6973P-C | 85.6 | — | — | — |
| 2026-09-30 | irix7/iris | `e38f38ab` | AMD EPYC 7763 64-Core Processor | — | 447.3 | — | — |
| 2026-10-01 | techomancer/iris | `0f5c65c9` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 68.7 | 637.1 | 52.7 | 505.6 |
| 2026-10-01 | irix7/iris | `9f17c3dc` | AMD EPYC 9V74 80-Core Processor | 83.0 | — | — | — |
| 2026-10-01 | irix7/iris | `36bfb4c7` | AMD EPYC 9V45 96-Core Processor | 134.2 | — | — | — |
| 2026-10-01 | irix7/iris | `9f17c3dc` | AMD EPYC 7763 64-Core Processor | — | 460.1 | — | — |
| 2026-10-01 | irix7/iris | `36bfb4c7` | AMD EPYC 9V45 96-Core Processor | — | 1210.7 | — | — |
| 2026-10-01 | irix7/iris | `71790bd9` | INTEL(R) XEON(R) PLATINUM 8573C | 77.9 | — | — | — |
| 2026-10-01 | irix7/iris | `71790bd9` | AMD EPYC 9V45 96-Core Processor | — | 1139.3 | — | — |
| 2026-10-01 | irix7/iris | `4d6e97c1` | AMD EPYC 9V45 96-Core Processor | 135.1 | — | — | — |
| 2026-10-01 | irix7/iris | `4d6e97c1` | AMD EPYC 9V45 96-Core Processor | — | 1076.1 | — | — |
| 2026-10-01 | irix7/iris | `e9e52c65` | AMD EPYC 7763 64-Core Processor | 62.6 | — | — | — |
| 2026-10-01 | irix7/iris | `e9e52c65` | AMD EPYC 9V45 96-Core Processor | — | 1040.3 | — | — |
| 2026-10-01 | irix7/iris | `061b3479` | AMD EPYC 7763 64-Core Processor | 62.5 | — | — | — |
| 2026-10-01 | irix7/iris | `24144598` | Intel(R) Xeon(R) 6973P-C | 102.7 | — | — | — |
| 2026-10-01 | irix7/iris | `061b3479` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 416.1 | — | — |
| 2026-10-01 | irix7/iris | `24144598` | AMD EPYC 7763 64-Core Processor | — | 429.8 | — | — |
| 2026-10-01 | irix7/iris | `320c38aa` | AMD EPYC 9V74 80-Core Processor | 81.5 | — | — | — |
| 2026-10-01 | irix7/iris | `320c38aa` | AMD EPYC 7763 64-Core Processor | — | 434.9 | — | — |
| 2026-10-02 | irix7/iris | `2231d58a` | AMD EPYC 9V45 96-Core Processor | 134.8 | — | — | — |
| 2026-10-02 | irix7/iris | `2231d58a` | AMD EPYC 9V45 96-Core Processor | — | 1161.2 | — | — |
| 2026-10-02 | irix7/iris | `e0f0e664` | INTEL(R) XEON(R) PLATINUM 8573C | 78.0 | — | — | — |
| 2026-10-02 | irix7/iris | `e0f0e664` | AMD EPYC 9V45 96-Core Processor | — | 1194.8 | — | — |
| 2026-10-02 | irix7/iris | `5582e969` | AMD EPYC 9V45 96-Core Processor | 133.0 | — | — | — |
| 2026-10-02 | irix7/iris | `5582e969` | AMD EPYC 9V45 96-Core Processor | — | 1096.3 | — | — |
| 2026-10-02 | techomancer/iris | `71790bd9` | AMD EPYC 7763 64-Core Processor | 61.4 | 779.1 | 113.2 | 423.8 |
| 2026-10-02 | irix7/iris | `05295175` | AMD EPYC 7763 64-Core Processor | 62.8 | — | — | — |
| 2026-10-02 | irix7/iris | `05295175` | AMD EPYC 9V74 80-Core Processor | — | 705.4 | — | — |
| 2026-10-02 | irix7/iris | `0cd1bae0` | AMD EPYC 9V45 96-Core Processor | 130.5 | — | — | — |
| 2026-10-02 | irix7/iris | `0cd1bae0` | INTEL(R) XEON(R) PLATINUM 8573C | — | 658.6 | — | — |
| 2026-10-02 | techomancer/iris | `320c38aa` | AMD EPYC 9V74 80-Core Processor | 79.0 | 456.2 | 69.3 | 363.6 |
| 2026-10-02 | techomancer/iris | `e0f0e664` | AMD EPYC 7763 64-Core Processor | 62.3 | 436.5 | 52.4 | 354.7 |
| 2026-10-02 | techomancer/iris | `05295175` | AMD EPYC 7763 64-Core Processor | 61.6 | 459.8 | 52.8 | 405.1 |
| 2026-10-02 | irix7/iris | `7728ca26` | INTEL(R) XEON(R) PLATINUM 8573C | 73.6 | — | — | — |
| 2026-10-02 | irix7/iris | `f9eceab3` | AMD EPYC 7763 64-Core Processor | 62.3 | — | — | — |
| 2026-10-02 | irix7/iris | `7728ca26` | AMD EPYC 7763 64-Core Processor | — | 448.3 | — | — |
| 2026-10-02 | irix7/iris | `f9eceab3` | AMD EPYC 9V45 96-Core Processor | — | 1146.9 | — | — |
| 2026-10-03 | irix7/iris | `181c8b10` | AMD EPYC 7763 64-Core Processor | 62.1 | — | — | — |
| 2026-10-03 | irix7/iris | `6bc20822` | Intel(R) Xeon(R) 6973P-C | 103.4 | — | — | — |
| 2026-10-03 | irix7/iris | `181c8b10` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 405.1 | — | — |
| 2026-10-03 | irix7/iris | `6bc20822` | AMD EPYC 7763 64-Core Processor | — | 398.3 | — | — |
| 2026-10-04 | irix7/iris | `c16e18d9` | AMD EPYC 9V74 80-Core Processor | 80.1 | — | — | — |
| 2026-10-04 | irix7/iris | `e966d544` | AMD EPYC 9V45 96-Core Processor | 132.1 | — | — | — |
| 2026-10-04 | irix7/iris | `c16e18d9` | AMD EPYC 7763 64-Core Processor | — | 428.7 | — | — |
| 2026-10-04 | irix7/iris | `e966d544` | AMD EPYC 9V45 96-Core Processor | — | 1146.9 | — | — |
| 2026-10-04 | irix7/iris | `ce561234` | INTEL(R) XEON(R) PLATINUM 8573C | 73.1 | — | — | — |
| 2026-10-04 | irix7/iris | `ce561234` | AMD EPYC 9V45 96-Core Processor | — | 1136.8 | — | — |
| 2026-10-04 | irix7/iris | `5abfa7f7` | AMD EPYC 9V45 96-Core Processor | 134.2 | — | — | — |
| 2026-10-04 | irix7/iris | `5abfa7f7` | AMD EPYC 9V45 96-Core Processor | — | 1151.3 | — | — |
| 2026-10-04 | irix7/iris | `d8b8131e` | AMD EPYC 7763 64-Core Processor | 60.8 | — | — | — |
| 2026-10-04 | irix7/iris | `d6184b81` | AMD EPYC 9V45 96-Core Processor | 131.9 | — | — | — |
| 2026-10-04 | irix7/iris | `d8b8131e` | AMD EPYC 9V74 80-Core Processor | — | 706.3 | — | — |
| 2026-10-04 | irix7/iris | `d6184b81` | INTEL(R) XEON(R) PLATINUM 8573C | — | 714.2 | — | — |
| 2026-10-06 | irix7/iris | `862d0fe1` | INTEL(R) XEON(R) PLATINUM 8573C | 68.5 | — | — | — |
| 2026-10-06 | irix7/iris | `862d0fe1` | AMD EPYC 7763 64-Core Processor | — | 472.0 | — | — |
| 2026-10-07 | irix7/iris | `614ec2ea` | AMD EPYC 7763 64-Core Processor | 63.3 | — | — | — |
| 2026-10-07 | irix7/iris | `614ec2ea` | AMD EPYC 9V45 96-Core Processor | — | 1063.1 | — | — |
| 2026-10-07 | irix7/iris | `e6b77c64` | AMD EPYC 7763 64-Core Processor | 63.8 | — | — | — |
| 2026-10-07 | irix7/iris | `e6b77c64` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 490.1 | — | — |
| 2026-10-07 | irix7/iris | `9f005809` | Intel(R) Xeon(R) 6973P-C | 97.5 | — | — | — |
| 2026-10-07 | irix7/iris | `9f005809` | AMD EPYC 7763 64-Core Processor | — | 475.7 | — | — |
| 2026-10-08 | irix7/iris | `ee8cddb6` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 65.8 | 1057.6 | 49.6 | 496.0 |
| 2026-10-08 | irix7/iris | `a4d6b7fb` | AMD EPYC 7763 64-Core Processor | 59.5 | 589.0 | 103.2 | 556.5 |
| 2026-10-08 | irix7/iris | `819dd39e` | AMD EPYC 9V45 96-Core Processor | 127.3 | — | — | — |
| 2026-10-08 | irix7/iris | `819dd39e` | AMD EPYC 9V45 96-Core Processor | — | 1102.7 | — | — |
| 2026-10-08 | irix7/iris | `b9b0dd2b` | INTEL(R) XEON(R) PLATINUM 8573C | 88.0 | 570.4 | 60.1 | 514.1 |
| 2026-10-08 | irix7/iris | `f05a350a` | INTEL(R) XEON(R) PLATINUM 8573C | 73.6 | — | — | — |
| 2026-10-08 | irix7/iris | `f05a350a` | AMD EPYC 7763 64-Core Processor | — | 604.1 | — | — |
| 2026-10-08 | irix7/iris | `1528d703` | AMD EPYC 9V74 80-Core Processor | 82.0 | — | — | — |
| 2026-10-08 | irix7/iris | `1528d703` | AMD EPYC 7763 64-Core Processor | — | 466.2 | — | — |
| 2026-10-08 | irix7/iris | `0aacb755` | AMD EPYC 9V45 96-Core Processor | 136.5 | — | — | — |
| 2026-10-08 | irix7/iris | `0aacb755` | AMD EPYC 9V45 96-Core Processor | — | 1115.7 | — | — |
| 2026-10-08 | irix7/iris | `9d1988a3` | INTEL(R) XEON(R) PLATINUM 8573C | 71.9 | — | — | — |
| 2026-10-08 | irix7/iris | `9d1988a3` | AMD EPYC 9V45 96-Core Processor | — | 1161.8 | — | — |
| 2026-10-08 | irix7/iris | `df5df542` | AMD EPYC 9V45 96-Core Processor | 129.0 | — | — | — |
| 2026-10-08 | irix7/iris | `df5df542` | AMD EPYC 9V45 96-Core Processor | — | 1127.4 | — | — |
| 2026-10-08 | irix7/iris | `947a60c1` | AMD EPYC 7763 64-Core Processor | 58.0 | — | — | — |
| 2026-10-08 | irix7/iris | `947a60c1` | AMD EPYC 9V74 80-Core Processor | — | 792.5 | — | — |
| 2026-10-08 | irix7/iris | `1bd24a1c` | AMD EPYC 9V45 96-Core Processor | 129.3 | — | — | — |
| 2026-10-08 | irix7/iris | `1bd24a1c` | INTEL(R) XEON(R) PLATINUM 8573C | — | 698.3 | — | — |
| 2026-10-08 | irix7/iris | `d90dcc2d` | INTEL(R) XEON(R) PLATINUM 8573C | 71.5 | — | — | — |
| 2026-10-08 | irix7/iris | `d90dcc2d` | AMD EPYC 7763 64-Core Processor | — | 544.6 | — | — |
| 2026-10-08 | irix7/iris | `2aafdc8b` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | 68.3 | 777.1 | 49.1 | 533.4 |
| 2026-10-08 | irix7/iris | `ee8cddb6` | AMD EPYC 7763 64-Core Processor | 57.2 | — | — | — |
| 2026-10-08 | irix7/iris | `ee8cddb6` | AMD EPYC 9V45 96-Core Processor | — | 1036.2 | — | — |
| 2026-10-08 | irix7/iris | `aea36137` | AMD EPYC 9V74 80-Core Processor | 48.2 | 341.8 | 61.6 | 316.3 |
| 2026-10-08 | irix7/iris | `784f6411` | AMD EPYC 7763 64-Core Processor | 57.5 | — | — | — |
| 2026-10-08 | irix7/iris | `784f6411` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 544.3 | — | — |
| 2026-10-08 | irix7/iris | `06b45eec` | Intel(R) Xeon(R) 6973P-C | 98.5 | — | — | — |
| 2026-10-08 | irix7/iris | `06b45eec` | AMD EPYC 7763 64-Core Processor | — | 598.5 | — | — |
| 2026-10-08 | irix7/iris | `ac957abc` | AMD EPYC 9V74 80-Core Processor | 74.2 | — | — | — |
| 2026-10-08 | irix7/iris | `ac957abc` | AMD EPYC 7763 64-Core Processor | — | 565.3 | — | — |
| 2026-10-08 | irix7/iris | `8ea03119` | AMD EPYC 9V45 96-Core Processor | 130.0 | — | — | — |
| 2026-10-08 | irix7/iris | `8ea03119` | AMD EPYC 9V45 96-Core Processor | — | 1108.6 | — | — |
| 2026-10-08 | irix7/iris | `a4d6b7fb` | INTEL(R) XEON(R) PLATINUM 8573C | 80.0 | — | — | — |
| 2026-10-08 | irix7/iris | `a4d6b7fb` | AMD EPYC 9V45 96-Core Processor | — | 1150.8 | — | — |
| 2026-10-08 | irix7/iris | `5e72b960` | AMD EPYC 7763 64-Core Processor | 56.9 | — | — | — |
| 2026-10-08 | irix7/iris | `5e72b960` | AMD EPYC 9V74 80-Core Processor | — | 757.4 | — | — |
| 2026-10-08 | irix7/iris | `b9b0dd2b` | AMD EPYC 9V45 96-Core Processor | 128.1 | — | — | — |
| 2026-10-08 | irix7/iris | `b9b0dd2b` | INTEL(R) XEON(R) PLATINUM 8573C | — | 657.9 | — | — |
| 2026-10-08 | irix7/iris | `258dee07` | AMD EPYC 7763 64-Core Processor | 57.4 | — | — | — |
| 2026-10-08 | irix7/iris | `258dee07` | AMD EPYC 9V45 96-Core Processor | — | 1077.1 | — | — |
| 2026-10-08 | irix7/iris | `71758983` | AMD EPYC 7763 64-Core Processor | 57.1 | — | — | — |
| 2026-10-08 | irix7/iris | `71758983` | Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz | — | 554.8 | — | — |
| 2026-10-08 | irix7/iris | `55cd2fe7` | Intel(R) Xeon(R) 6973P-C | 101.0 | — | — | — |
| 2026-10-08 | irix7/iris | `55cd2fe7` | AMD EPYC 7763 64-Core Processor | — | 573.0 | — | — |
| 2026-10-08 | irix7/iris | `b8c75dd1` | AMD EPYC 9V74 80-Core Processor | 74.8 | — | — | — |
| 2026-10-08 | irix7/iris | `b8c75dd1` | AMD EPYC 7763 64-Core Processor | — | 572.6 | — | — |
| 2026-10-08 | irix7/iris | `fe5ba69a` | AMD EPYC 9V45 96-Core Processor | 131.4 | — | — | — |
| 2026-10-08 | irix7/iris | `fe5ba69a` | AMD EPYC 9V45 96-Core Processor | — | 1141.4 | — | — |
| 2026-10-08 | irix7/iris | `c29ff7f5` | INTEL(R) XEON(R) PLATINUM 8573C | 81.6 | — | — | — |
| 2026-10-08 | irix7/iris | `c29ff7f5` | AMD EPYC 9V45 96-Core Processor | — | 1130.5 | — | — |
| 2026-10-08 | irix7/iris | `817513e2` | AMD EPYC 9V45 96-Core Processor | 129.4 | — | — | — |
| 2026-10-08 | irix7/iris | `817513e2` | AMD EPYC 9V45 96-Core Processor | — | 1146.8 | — | — |
| 2026-10-08 | irix7/iris | `8877a691` | AMD EPYC 7763 64-Core Processor | 57.8 | — | — | — |
| 2026-10-08 | irix7/iris | `8877a691` | AMD EPYC 9V74 80-Core Processor | — | 763.6 | — | — |
