# Allocator, target-cpu, PGO, and toolchain pinning defaults

Build-config findings, grounded in primary sources. All are low-risk and
independent of the code-level changes; measure each one at a time against a pinned
toolchain.

## 1. Allocator — mimalloc, but watch RSS

IRIS uses the system glibc allocator. The codebase already observed ~4.5% of
runtime in allocator frames (`Global`/`RawTable` rehash) in a hot path. The
cross-thread allocation pattern (per-device threads, JIT pool) is exactly what
mimalloc's free-list sharding targets (the `larsonN`/`sh6bench`/`xmalloc-testN`
wins in the `mimalloc-bench` suite). Drop-in:

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

**Caveat:** mimalloc is faster but not always smaller — a rustc experiment showed
a ~5% compile-time win with up to ~35% RSS increase (rust PR #92249). A/B time
*and* RSS; if memory balloons, `tikv-jemallocator` (jemalloc) is the fallback (and
what rustc ships with on Linux, with THP tuning available).

## 2. `-C target-cpu=native` — opt-in, not a magic number

The Rust Performance Book: "can improve runtime speed, especially if the compiler
finds vectorization opportunities." For a branchy interpreter/dispatch loop the win
is typically low single-digit (newer encodings, scheduling), and AVX-512 can even
*regress* on some CPUs via downclocking. It does not affect JIT-emitted guest code
(generated independently). Add as an opt-in for local/CI benchmark builds; keep
generic x86-64 for distributed binaries.

Cargo profiles cannot set `-C target-cpu=native` (rustflags are global — via
`.cargo/config.toml` or `RUSTFLAGS`), so the opt-in is an env var, optionally
paired with the `release-native` profile in `Cargo.toml` for an unambiguous
target dir:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build --profile release-native
```

`release-native` merely `inherits = "release"`; the native codegen comes from
`RUSTFLAGS` alone. Do **not** add a `.cargo/config.toml` that sets
`target-cpu=native` unconditionally — that would silently apply it to every
build, including the default generic binary. For a one-off check the plain
profile also works: `RUSTFLAGS="-C target-cpu=native" cargo build --release`.

## 3. PGO — the highest-confidence codegen win

rustc's own PGO blog (Nov 2020): PGO on the dispatch-heavy front-end ≈ 5%
instruction-count reduction; PGO+LLVM ≈ 10-16% wall-time. The front-end is the
right analogue for IRIS's interpreter. Standard tool is `cargo-pgo`; BOLT adds a
further few % (i-cache/function layout) at higher operational cost — skip it
initially. The training workload must match real usage: an IRIX boot-to-shell plus
a representative `bench/` run, covering both interpreter and JIT-compile paths.

## 4. Pin the nightly date

`rust-toolchain.toml` pins bare `channel = "nightly"` — a moving target that makes
`iris-bench matrix` numbers non-comparable across a week. Pin
`channel = "nightly-YYYY-MM-DD"` and track `Cargo.lock` (the rustup book's
documented practice).

## Order

mimalloc (A/B time+RSS) → pin nightly → PGO (`cargo-pgo`) → native (opt-in). The
existing `[profile.release]` (fat LTO, codegen-units=1, panic=abort) is already
optimal; do not change it.
