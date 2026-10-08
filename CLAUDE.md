# Claude Instructions — IRIS

IRIS is an SGI Indy (IP24) and Indigo2 (IP22/IP28) emulator written in Rust, with an
R4400, R5000, or R10000 CPU selected at runtime. It boots IRIX 6.5 and 5.3 to a usable
system (shell, networking, X11). It is **not** cycle-accurate
— IRIX doesn't need it and accuracy would only make it slower.

This repository (`irix7/iris`) is a fork of
[`techomancer/iris`](https://github.com/techomancer/iris); upstream is the
canonical project. The fork carries performance work and parallel per-task
instance operation for development. Sync upstream with
`git fetch upstream && git merge upstream/main`.

## Read these first

- `HACKING.md` — architecture: data path/endianness, concurrency model, the
  MC bus/device/port abstraction. **Read before touching device or CPU code.**
- `HELP.md` — running it: serial ports, monitor console, NVRAM/MAC setup, disk
  image prep.
- `README.md` — overview, feature flags, current status.
- `iris-gui-README.md` — the optional egui front-end (`-p iris-gui`).
- `CHANGELOG.md` — what changed, by area.
- `docs/` — per-device notes and design docs (hal2, rex3, wd33c93a, ppmem,
  tcache, nutlb, …) plus the hardware datasheets (PDFs).
- `docs/parallel-instances.md` — the per-instance isolation model, the
  `StatePaths` / `derive_instance` seam, and the ticket map (GitHub epic #1,
  tickets #2–#21). Read before touching config, persistent paths or ports.
- `rules/` — accumulated, hard-won findings about emulator behaviour
  (`jitv2/`, `rex3/`, `snapshot/`, `irix/`, `testing/`, `gui/`, `perf/`,
  `scsi/`, `macos/`, `build/`). The IRIX install guide is
  `rules/irix/irix-install.md`. Check here before re-deriving a
  gotcha; when you confirm a non-obvious fix, write it up here as a short
  markdown note so the next session doesn't relearn it.
- `ignore/rules` - local unpublished info

## Build & run

```
cargo run --release                                       # interpreter
cargo run --release --features lightning,rex-jit          # recommended for speed
cargo run --release --features jitv2,rex-jit              # enable MIPS JIT v2 (experimental)
cargo run --release -- --cpu r5000                        # CPU is a runtime choice, not a feature
cargo run -p iris-gui --release                           # GUI front-end
```

The build environment is the Nix flake (`flake.nix`). Run cargo from inside the
dev shell rather than bare, or the native headers/libs (ALSA, X11/Wayland, GL,
v4l, libclang for bindgen) and the pinned toolchain are missing:

```
nix develop --command cargo check -p iris
nix develop --command cargo test -p iris
```

The dev shell pins rustup at the host's shared rig toolchain
(`RUSTUP_HOME`/`CARGO_HOME` under `/mnt/europa/sgi-toolchain-scratch`), which
`rust-toolchain.toml` names (`nightly-2026-10-02`). On another host, drop the
flake's `shellHook` and let rustup honour `rust-toolchain.toml` itself. If the
rig toolchain stops launching with "required file not found", a nix GC has
collected the glibc its ELF interpreter points at: repoint `cargo`/`rustc` with
`patchelf --set-interpreter <a surviving glibc>/lib/ld-linux-x86-64.so.2`, and
set `TMPDIR` off any full root filesystem.

`lightning` and `developer` are mutually exclusive; `r5ksc`/`r5ksc_triton`
deliberately fail to build.

Binaries: `iris` (the emulator), `iris-ci` (CI/automation socket client),
`iris-bench` (benchmark driver), `coffdump`, `mkvh` (SGI volume headers),
`chd_extract` (`chd`), `jitv2_analyze`/`jitv2_verify`/`jitv2_pcp_dump`
(`jitv2`), and `iris-gui` in the workspace. Feature flags are documented in
`README.md`.

## Testing and benchmarking

- `cargo test --workspace` — unit tests (per-device snapshot round trips, CPU,
  TLB, REX3; add `--features jitv2` for the JIT equivalence tests).
- `cpu-tests/` — bare-metal MIPS III/IV correctness suite. "Is this instruction
  right", one instruction at a time. `make -C cpu-tests run`.
- `bench/` — bare-metal benchmark suite. "How fast is this build, and is it
  still right after ten million of them." Reports throughput, guest MIPS and an
  accuracy score per kernel; `iris-bench matrix` sweeps R4400/R5000 x
  interpreter/jitv2. Read `rules/testing/benchmark-suite-gotchas.md` before
  adding a kernel — the accuracy check catches endianness, uninitialised
  memory and out-of-bounds reads, and every one of those has already happened.
- Both share `cpu-tests/harness` (toolchain probe, SCC console, startup and
  exception dispatch). Changing those files affects both suites.
- `test/fleet/run.sh` — the parallel-agent end-to-end test: boots two isolated
  instances (`--instance`) and drives each over its own CI serial socket (the
  Indy PROM command monitor), asserting disjoint derivation and concurrent
  operation. No IRIX media needed; runs in `.github/workflows/rust.yml`.
  `$FLEET_N` sets the instance count, `$KEEP=1` keeps the temp dir.

## Hard invariants (from HACKING.md)

- **Endianness lives only at "The Edge."** Host `u32`/`u64` are bit-containers;
  byte-swapping happens at PROM/disk I/O via `swap_on_load`, never in CPU/bus/MC
  logic. **Do not suggest `.to_be()` / `.to_le()` for memory or register code.**
- **Concurrency is per-device.** CPU, REX3, SCSI, and ethernet run on their own
  threads and lock their own state. Deadlocks live in callbacks *up* to a parent
  device (e.g. SCSI → HPC3) — be careful there.

## Automation & CI

- `iris-ci` is the canonical socket interface for driving a running emulator
  (snapshots, scripted input, headless runs). Prefer it over ad-hoc serial
  poking. See `rules/snapshot/` and the CI section of `README.md`.
- Install IRIX only from original media (see `rules/irix/irix-install.md`). Never use a
  pre-built MAME CHD as a shortcut.
- Normal `Machine::stop` (GUI Stop/Quit and guest power-off) saves NVRAM and
  the Indigo2 motherboard EEPROM. Before forced termination, run `rtc save`
  (Indy) or `nveeprom save` (Indigo2) from the monitor to retain PROM changes.
- Whole-page compilation is the only jitv2 implementation; `j2wp` is an alias.
  Persistent compiled-page reuse is opt-in via `[jitv2] cache` / `cache_dir`.
- CHD, camera, DaynaPort, Ultra64, IP28, and ppmem are unconditional in core;
  do not use their retired Cargo features in build commands.

## Parallel instances (fleet)

The fork is growing an opt-in instance mode so N VMs run side by side from one
host. Model and ticket map: `docs/parallel-instances.md` (epic #1, tickets
#2–#21). The rules that matter when editing:

- **One `Machine` per process.** Never run two `Machine`s in one process; a
  fleet is N processes, each `iris --instance N`. Process-global state (idle
  parking, CPU affinity, the dev log, the CI socket path) is why.
- **Two seams own isolation, and nothing else may invent paths or ports.**
  `src/state.rs` (`StatePaths`) is the registry of persistent artefacts;
  `config::derive_instance()` is the one pure step that turns a base config plus
  `(id, state-dir, port-base)` into a derived one. New artefacts get a
  `StatePaths` accessor; new per-instance values get derived in
  `derive_instance`.
- **Opt-in and backwards compatible.** With no `--instance`/`--state-dir`/
  `--port-base`, behaviour is unchanged. `--print-instance` prints the derived
  endpoints as `key: value`.
- **Status.** #2 (`StatePaths`) and #3 (`InstanceIdentity` + CLI) have landed;
  #4 onward are open. The serial/CI session track (#15–#21) is independent of
  the instance track and can run in parallel.

## Agent skills

### Issue tracker

Issues and specs live as GitHub issues on `irix7/iris`, managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles, label strings equal to their names: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` at the repo root plus `docs/adr/`. See `docs/agents/domain.md`.
