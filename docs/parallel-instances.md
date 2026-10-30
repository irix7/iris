# Parallel instances

IRIS is normally a one-VM tool. This fork is growing an opt-in **instance
mode** so several emulated machines can run side by side from one host without
sharing a single persistent byte: separate NVRAM/EEPROM, snapshot and chunk
store, COW overlay and CHD diff, JIT cache, serial log, ports, CI socket, guest
MAC and NAT subnet.

The work is tracked as GitHub epic **#1** on `irix7/iris`; the tickets are
#2–#21. This file is the orientation note: the model, the seam, and where the
implementation stands.

## The architectural decision

**One `Machine` per process.** Running several `Machine`s in one process is
explicitly not supported. Idle parking, CPU affinity, the dev log and the CI
socket path are all process-global, so a second `Machine` in the same process
would fight the first. A "fleet" is N processes, each with `--instance N`.
Where a later change is tempted to add process-global state, prefer per-instance
derivation instead.

**Opt-in.** With no instance flag, behaviour is byte-identical to before. The
single-instance path is unchanged.

## The seam

Instance mode is two pure steps plus a CLI surface. Nothing else may invent a
per-instance path or port.

- **`src/state.rs` — `StatePaths`.** The registry of persistent artefacts.
  `StatePaths::neutral()` returns exactly today's paths (relative names in the
  cwd, the platform jitv2 cache, the platform CI socket). `StatePaths::under(root)`
  relocates the whole set beneath one directory. Accessors: `nvram`,
  `nveeprom`, `snapshots_dir`, `chunk_store_dir`, `jit_cache_dir`, `serial_log`
  (an `Option` — there is no default today), `test_device_dump`, `crash_log`,
  `ci_socket`, and `cow_overlay_dir`/`chd_diff_dir` (both `Option`, `None` =
  "resolve the existing way").

- **`src/config.rs` — `InstanceIdentity` and `derive_instance()`.** The one
  step allowed to turn a base config plus `(id, state_dir, port_base)` into a
  fully-derived config. It is pure and unit-tested. It routes the file names
  through `StatePaths` so names live in one place.

- **CLI (`config::Cli`, `load_config()`).** `--instance N` (0-based),
  `--state-dir PATH`, `--port-base PORT`, and `--print-instance`. Presence of
  any one engages derivation; with none, the config is untouched.

## Default derivation

For instance id `N` (0-based), unless the base config already set a value:

| Artefact | Derived value |
|---|---|
| state dir | `iris-instance-{N}` (overridable with `--state-dir`) |
| ownership lock | `<state_dir>/iris.lock` |
| NVRAM / EEPROM | `<state_dir>/nvram.bin`, `<state_dir>/nveeprom.bin` |
| snapshot store | `<state_dir>/saves` (with chunk store `<state_dir>/saves/.cas`) |
| COW overlay dir | `<state_dir>/overlays` |
| CHD diff dir | `<state_dir>/diffs` |
| jitv2 cache | `<state_dir>/jitv2` |
| serial log | `<state_dir>/iris-serial.log` (derived only; logging stays off unless set) |
| test dump | `<state_dir>/iris-testdev-dump.json` |
| crash log | `<state_dir>/iris-crash.log` |
| monitor | `9000 + N*10` |
| serial A / B | `port_base + 1` / `+ 2` |
| CI socket | Unix `<state_dir>/iris.sock`; Windows `127.0.0.1:{port_base+3}` |
| MAC | stable SGI-OUI `08:00:69` address per id (FNV-1a over the id bytes) |
| NAT subnet | `192.168.{N % 256}.0/24` |

The default port block starts at 9000, clear of the legacy single-instance
defaults (monitor 8888, serial 8880/8881), so an instance can run alongside a
plain `iris`. `--print-instance` prints the derived endpoints as `key: value`
lines.

## Status

Landed on this tree:

- **#2** `StatePaths` seam (`src/state.rs`).
- **#3** `InstanceIdentity` / `derive_instance` and the CLI flags
  (`src/config.rs`, `src/main.rs`).
- **#4–#7** the derived state directory wired into `Machine`: NVRAM/EEPROM, the
  serial log, the test dump, the jitv2 cache, the crash log, the snapshot and
  chunk store, and the COW/CHD overlay and diff directories.
- **#8–#9** per-instance ports from `--port-base`, a CI socket under the state
  directory, a liveness probe that refuses a live socket, and a hard error on a
  port/socket bind failure in instance mode.
- **#11** snapshot commands scoped to the instance store
  (`snapshot::store_root`).
- **#13–#14** a fleet launcher and a derived per-VM telnet forward.
- **#15–#21** the serial/CI session track: per-consumer read cursors,
  reset-wakes-waiters, host-speed CI serial, a buffered `--serial-log`, TCP
  `TCP_NODELAY` and second-client refusal, bounded CI connections, and macro
  prompt-waits.
- **#103** instance ownership: a locked `iris.lock` per state directory,
  liveness by lock rather than PID, `--replace`, listener preflight, and
  signal/exit cleanup (`src/instance.rs`, `tests/instance_lifecycle.rs`).

Still open:

- **#10** identity hardening: MAC/NAT subnet overrides, bridged NFS-host MAC.
- **#11** the opt-in shared, read-mostly snapshot base a fleet can fork from.
- **#12** disk advisory locking (a read-write base image is not locked yet).
- **GUI ownership.** `iris-gui` embeds `Machine` directly and does not take the
  lock, so a GUI session and a CLI session on one state directory can still
  collide; the GUI start preflight is the only guard there today.

Known gaps:

- NAT subnets repeat past 255 (`N % 256`); full uniqueness is ticket #10.
- The default subnet range can collide with a DaynaPort's default
  (`192.168.10.0/24`); #10 owns the DaynaPort side. It fails validation loudly,
  not silently.
- `port_base` saturates at `u16::MAX`, so a very large fleet (id beyond ~5653)
  would share a block.

## Ownership and cleanup

One state directory is owned by one live process. The marker is
`<state_dir>/iris.lock` (plain `iris.lock` in the working directory for a
single-instance run), held with a non-blocking exclusive advisory lock for the
process lifetime. The kernel releases the lock on process death, so a crash or
`SIGKILL` leaves a file the next start reclaims instead of a state directory
that is permanently "in use". The JSON record inside it names the PID, instance
id, state directory, CI socket, monitor port and process start time; a refused
start prints the record, and `--replace` uses the start time to avoid signalling
a PID that has been recycled by an unrelated process.

`--replace` prefers a clean `quit` over the recorded CI socket, which runs
`Machine::stop` and saves NVRAM/EEPROM. Only when no CI socket answers as the
recorded owner does it send `SIGTERM`, after validating the process start
marker. A live listener that does not speak the CI protocol is never touched.

Startup checks the listeners it is about to bind (monitor, serial A/B outside
`--ci`, the GDB stub, a TCP CI socket) before constructing the Machine. A Unix
CI socket is probed over the CI protocol instead: a stale file is removed, a
live iris is refused unless `--replace` says otherwise, and a foreign listener
is left alone.

`SIGINT`, `SIGTERM` and `SIGHUP` unlink the CI socket and lock file and
re-raise the signal, but deliberately do not stop the guest — `Machine::stop`
takes locks and writes NVRAM, which is not async-signal-safe. Quit through the
monitor or `iris-ci quit` for a clean shutdown.

## Testing

`tests/instance_lifecycle.rs` spawns the real binary in a temporary working
directory and covers start → quit → restart, a refused second start,
`--replace` over the CI socket, a stale socket and lock, `SIGTERM` cleanup, and a
busy monitor port. It needs no IRIX media and runs in the Rust workflow:

```sh
cargo test -p iris --test instance_lifecycle
```

`test/fleet/run.sh` is the end-to-end check: it boots two instances and drives
each over its own CI serial socket (the Indy PROM command monitor), asserting
disjoint derivation, concurrent operation, a distinct guest `eaddr` per
instance, and that quitting one does not disturb the others. It needs no IRIX
media, runs in the Rust workflow (`.github/workflows/rust.yml`), and can be run
locally against the debug binaries:

```sh
test/fleet/run.sh target/debug/iris target/debug/iris-ci
```

`$FLEET_N` sets the instance count (default 2); `$KEEP=1` keeps the temp dir on
exit for inspection.

For a development fleet, `scripts/fleet.sh -n N` derives and prints every
instance's endpoints and starts them in the background (PIDs in
`<state-root>/pids`). `-c` selects the base TOML, `-s` the state root and `-b`
the first port block; `--dry-run` validates and prints without starting:

```sh
scripts/fleet.sh -n 4 -c iris.toml -s ~/iris-fleet --dry-run
scripts/fleet.sh -n 4 -c iris.toml -s ~/iris-fleet
```

## For agents

- Implement tickets against the seam, not around it: new persistent artefacts
  get a `StatePaths` accessor, new per-instance values get derived in
  `derive_instance`, and neither is invented at a call site.
- `src/state.rs`'s jitv2 path deliberately mirrors `cpu::jitv2::pcache::default_base`,
  which lives behind the `jitv2` feature and so cannot be called from the
  always-compiled `state` module. Keep the two in lockstep.
- Build and test from the Nix dev shell; see `CLAUDE.md`.
