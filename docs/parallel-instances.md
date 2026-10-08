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

Landed:

- **#2** `StatePaths` seam (`src/state.rs`).
- **#3** `InstanceIdentity` / `derive_instance` and the CLI flags
  (`src/config.rs`, `src/main.rs`).

Open, in dependency order (see the GitHub issues for acceptance criteria):

- **#4–#7** wire the derived state dir into `Machine` — NVRAM/EEPROM/logs/test
  dump, COW/CHD, snapshot/chunk store, jitv2 cache.
- **#8–#9** per-instance ports and the CI socket, with a liveness probe so a
  new instance never steals a live one's socket.
- **#10** identity hardening: MAC/NAT subnet overrides, bridged NFS-host MAC.
- **#11–#12** snapshot scoping + shared read-only base; disk advisory locking.
- **#13–#14** fleet launcher; bridged networking and per-VM telnet.
- **#15–#21** the serial/CI session track: per-connection read cursors,
  reset-wakes-waiters, host-speed CI serial, buffered `--serial-log`, TCP
  backend hardening, connection bounds, and macro prompt-waits. These are
  independent of the instance track.

Known gaps while the work is in flight:

- NAT subnets repeat past 255 (`N % 256`); full uniqueness is ticket #10.
- The default subnet range can collide with a DaynaPort's default
  (`192.168.10.0/24`); #10 owns the DaynaPort side. It fails validation loudly,
  not silently.
- `port_base` saturates at `u16::MAX`, so a very large fleet (id beyond ~5653)
  would share a block.

## Testing

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
exit for inspection. The serial/session semantics that the later tickets add
(per-connection read cursors, reset-wakes-waiters, host-speed CI pacing) will
extend this script's checks as those tickets land.

## For agents

- Implement tickets against the seam, not around it: new persistent artefacts
  get a `StatePaths` accessor, new per-instance values get derived in
  `derive_instance`, and neither is invented at a call site.
- `src/state.rs`'s jitv2 path deliberately mirrors `cpu::jitv2::pcache::default_base`,
  which lives behind the `jitv2` feature and so cannot be called from the
  always-compiled `state` module. Keep the two in lockstep.
- Build and test from the Nix dev shell; see `CLAUDE.md`.
