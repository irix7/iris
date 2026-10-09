# aotbench — measuring the AOT page-cache first-boot win

Repeatable measurement harness for issue
[#76](https://github.com/irix7/iris/issues/76), the follow-up to the #54
viability spike ([`docs/jitv2-aot-cache-spike.md`](../../docs/jitv2-aot-cache-spike.md)).

The spike was a **conditional GO**: relocation-free offline page compilation is
feasible and prototyped (`src/bin/jitv2_aot.rs`), but further investment — and
especially shipping prewarmed packs — needs a **measured release/CI first-boot
win**. This directory is how that measurement is taken, so it can be repeated
and compared instead of felt.

> **The end-to-end number needs a guest.** Time-to-first-frame and boot-to-login
> can only be measured by booting a real IRIX image. Nothing in this directory
> can produce that number on a machine with no media. What *is* reproducible
> without a guest is the per-lookup cost the cache trades a compile for — see
> [The guest-free micro-benchmark](#the-guest-free-micro-benchmark).

## What the harness does

`aotbench.py` runs the same IRIX boot in a set of arms and reports wall-clock
medians plus the cache's own hit totals:

| arm | persistent cache | what it answers |
|---|---|---|
| `nocache` | off | the "without the cache" baseline: every page compiled live |
| `coldcache` | on, empty (opt-in) | isolates the AOT fill from the cost of doing lookups at all |
| `warmaot` | on, prewarmed by `jitv2_aot` | the AOT win: a fixed binary's *first* run starts warm |

Each arm is repeated `--reps` times. Per boot it records:

- **`first-frame ms`** — wall time from CPU start to the first framebuffer
  change from the reset screen (screenshot-polled over the CI socket). This is
  the "time to first frame" proxy: the first thing the PROM draws.
- **`login ms`** — wall time from CPU start to the serial login marker. This is
  the endpoint the persistent-cache design measured
  ([`docs/jitv2-persistent-cache.md`](../../docs/jitv2-persistent-cache.md),
  "boot to `login:`"), so it is the directly comparable number.
- **cache totals** — the last `jitcache: lookups=… hits=…` line the emulator
  printed, and the offline fill's `pages/compiled/stored/relocation_refused`.

## Prerequisites

1. A build with `jitv2`, yielding `iris`, `iris-ci` and `jitv2_aot`:

   ```
   nix develop --command cargo build --release --features jitv2
   # binaries: target/release/{iris,iris-ci,jitv2_aot}
   ```

   (`iris-ci` and `jitv2_aot` are workspace bins; `jitv2_aot` is only built
   with the `jitv2` feature.)

2. An IRIX disk image and an `iris.toml` that boots it. **The config must not
   set `[jitv2] cache = true` or a nonempty `cache_dir`** — a nonempty config
   value overrides the environment the arms rely on, so the harness refuses to
   run and says so. Leave both at their defaults and let `aotbench.py` own the
   cache through `IRIS_JIT_CACHE`/`IRIS_JIT_CACHE_DIR`.

3. The disk must be **static across runs**: pass a read-only or COW-backed
   image. The harness runs each boot from a fresh working directory so NVRAM
   and relative state are per-run, but it does not copy or reset the disk.
   A guest that writes its root filesystem between boots makes the arms
   incomparable.

4. A `.pcp` corpus for the *same* CPU model and workload. Capture one with the
   running emulator's monitor command:

   ```
   # inside the monitor console (TCP; monitor_port in the config, default 8888)
   > j2 corpus /path/to/corpus
   ```

   or let the harness do it (`--capture-corpus`) — it boots, waits for the
   login marker, runs `j2 corpus <--corpus>` over the monitor TCP port, then
   quits. Capture the corpus from a boot that does the workload you intend to
   measure; the entry sets it records are a **superset** the next run draws on
   (`rules/jitv2/corpus-capture-from-the-pcp-cache.md`).

## Running it

```
# Build first (see Prerequisites), then:
nix develop --command python3 test/aotbench/aotbench.py \
    --iris      target/release/iris \
    --iris-ci   target/release/iris-ci \
    --jitv2-aot target/release/jitv2_aot \
    --config    iris.toml \
    --corpus    /path/to/corpus \
    --cpu       r10000 \
    --reps      3
```

Useful flags:

- `--with-cold-cache` — add the cache-on-but-empty arm (separates the AOT win
  from the lookup overhead a warmed namespace also has).
- `--capture-corpus` — fill `--corpus` from a first boot instead of requiring
  one (see above).
- `--timeout N` — per-boot timeout, seconds (default 900).
- `--poll S` — screenshot poll interval, seconds (default 0.15).
- `--json OUT` — write every run's raw numbers as JSON.
- `--keep` — keep the scratch directory (per-run stderr, serial logs,
  screenshots).
- `--tests-only` — check the binaries, config and corpus exist, then exit
  without booting. **This is the only mode that does not need a guest.**

The script prints a median table and, for `nocache` vs `warmaot`, the
first-boot win in milliseconds and percent.

## Reading the result (the #76 go/no-go)

Issue #76's acceptance criteria are:

> - [ ] A first-boot measurement with a prewarmed cache is recorded.
> - [ ] A go/no-go on shipping prewarmed packs follows from the number.

The number to record is the **`nocache` → `warmaot` login (and first-frame)
delta with the prewarm off the guest's critical path** — i.e. `jitv2_aot` was
run before the boot, and the boot itself only loads blobs. The spike's
conditions for shipping packs (beyond this measurement) are in its
[Decision](../../docs/jitv2-aot-cache-spike.md#decision): a clean post-#38
baseline, a real wall-clock win on the same binary and host, and either a
portable target baseline or an accepted per-host fill step. This harness gives
the wall-clock win; it does not make the other conditions true.

Two things to sanity-check in the output before trusting the delta:

- **`warmaot` hit rate should be high** (the spike's own sessions reached
  86–95%). A low hit rate means the fingerprint or the corpus/CPU model does
  not match, and the arm is really a second `coldcache`.
- **`relocation_refused` in the fill should be ~0** (the spike saw zero on a
  real workload). A large value means many pages cannot be reused by *any*
  cache, so the ceiling is lower than the logs suggest.

## The guest-free micro-benchmark

The per-lookup cost — what a cache hit replaces a ~22 ms compile with — is
measurable with no guest and no media:

```
nix develop --command cargo test --release --features jitv2 \
    --lib lookup_cost_microbench -- --nocapture
```

**Use `--release`**: a debug build is dominated by BLAKE3 overhead and is not
the number to compare against production. (A plain `--lib pcache` run also
exercises `lookup_at`'s correctness alongside the existing round-trip tests.)

`lookup_cost_microbench` (`src/cpu/jitv2/pcache.rs`) writes 512 synthetic page
blobs through the real `fill_blob` path, then times the real disk-read +
container-check + decode + full-4-KB-compare path via `lookup_at` (the
read-side twin of `fill_blob`). It asserts every blob is found and that a
different fingerprint misses, so it is a correctness test as well as a
measurement. The absolute figure is a property of the host's filesystem and
CPU; compare it to the compile time *on the same host*.

## What still needs a guest to complete

- The end-to-end `nocache` vs `warmaot` first-boot delta (issue #76's actual
  acceptance number).
- The corpus itself, and therefore the warm `warmaot` hit rate on real IRIX
  code.
- The go/no-go on shipping prewarmed packs, which follows from that delta.

Everything else — the offline fill, the arm orchestration, the timing/statistics
and the decode/lookup cost — is here and runs without media.
