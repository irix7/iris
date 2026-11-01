---
name: iris-dev-loop
description: >-
  Use IRIS as a development workbench — for changing the emulator and for
  developing software inside the IRIX guest. Maps every way in and out:
  build/run variants, the monitor console, iris-ci, the MCP server, the GDB
  stub, serial ports, snapshots and overlays, parallel `--instance` fleets,
  the guest loop (telnet, NFS, MIPSpro builds, file push/pull, autologin,
  shutdown), and the test/benchmark suites. Use whenever an agent must start
  IRIS, run commands or move files inside IRIX, inspect or debug a running
  machine, or validate a change against the system.
---

# IRIS development loop

IRIS is an SGI Indy / Indigo2 / O2 emulator that boots real IRIX. Two loops use
it, usually together:

- **Emulator loop** — change Rust code, run the media-free suites, boot IRIX to
  the changed path, inspect from the monitor, capture once and replay offline,
  snapshot/rollback between attempts.
- **Guest loop** — develop software *inside* IRIX: boot to a shell or desktop,
  move sources in over NFS, build with MIPSpro, run/measure the result, debug a
  panic or a hang, package with `inst`.

Everything below runs from the IRIS repository root. When this skill is used
through a global link from another project, resolve the link (`readlink -f` the
skill directory) to find the checkout and work from there. Deeper material is
in the sibling skills (`irix-guest-loop`, `irix-gfx-loop`,
`irix-kernel-debug`, `irix-inst-packaging`) and in the documents linked at the
end.

## 1. Build

On this rig the toolchain and native libraries come from the Nix dev shell —
run cargo through it, or the build misses ALSA/X11/GL/libclang:

```sh
nix develop --command cargo check -p iris
nix develop --command cargo test -p iris
```

Entering the shell installs `target/launcher/bin/iris` and `iris-ci`: wrappers
that carry the shell's library search path. Use them for `--ci` runs and
scripts (no `LD_LIBRARY_PATH` needed); use the raw `target/release/iris` under
`gdb`/`perf`. Check a launcher from an empty environment with
`tools/iris-launcher-smoke.sh` or `nix run .#smoke`.

Outside the shell the build stops early: `cargo check -p iris` fails in
`alsa-sys`/`pkg-config`. The shell is not optional for building.

**A downloaded release needs the libraries but has no wrapper.** The CI-pinned
build is the fastest way to reproduce a CI result: guest failures repeat
exactly (the same trap `EPC` values as the job's log), so debug locally instead
of iterating through CI runs. Get it with:

```sh
gh release download "v0.1.10+irix7" --repo irix7/iris \
  --pattern 'IRIS-cli-linux-x64-*.tar.gz' && tar xzf IRIS-cli-linux-x64-*.tar.gz
```

Run outside the shell it dies with `error while loading shared libraries:
libGL.so.1`, then `libasound.so.2` once that is fixed. Point it at the store
copies (this is what the launcher wrappers do for a local build):

```sh
export LD_LIBRARY_PATH=$(dirname "$(find /nix/store -maxdepth 4 -name libGL.so.1 | head -1)"):$(dirname "$(find /nix/store -maxdepth 4 -name libasound.so.2 | head -1)"):$LD_LIBRARY_PATH
```

**The pinned release is a `lightning` build, so it cannot debug.** It boots and
runs CI scenarios faithfully — but `lightning` strips breakpoints, `dt` and the
traceback buffer. The monitor still *accepts* `bp add` and `bl` lists the
breakpoint; it simply never fires. For stepping, watchpoints or history, build
your own without `lightning` first:

```sh
nix develop --command cargo build --release --features developer \
  --target-dir target/agent
```

| Build | Gets you |
|---|---|
| `cargo run --release --features lightning,gpujit` | everyday speed (opcode fusion, all three graphics JITs) |
| `... --features lightning,gpujit,jitv2` | MIPS JIT v2 (experimental; `j2` monitor commands) |
| `cargo run --profile developer --features developer` | undo, `si`, instruction tracing, `jitcheck`, graphics traces; **CPU starts paused** (`start`) |
| `... --features gputrace` | `rex/gr2/mgras trace` and `rec` on a fast build |
| `... --features idle-pause` | park the CPU thread in guest idle loops |
| `... --features ci_clock` | deterministic Count clock (snapshot validation, replays only) |
| `... --features perfetto` / `pcap` / `hostgl` | timeline traces / bridged LAN / host GL for guest programs |

- `lightning` and `developer` are mutually exclusive; `lightning` strips
  breakpoints, `dt` and the traceback buffer for speed. `--all-features` does
  not build.
- CHD, camera, DaynaPort, Ultra64, IP28, ppmem, MIPS IV are always built in —
  do not pass their old feature names.
- If the user has a build in use, do not disturb `target/`: build into
  `--target-dir target/agent` (or your own directory) and run that binary.
- Binaries: `iris`, `iris-ci`, `iris-bench`, `jitv2-analyze`/`-verify`/`-pcp-dump`,
  `coffdump`, `mkvh`, `chd-extract`; GUI is `cargo run -p iris-gui --release`.

## 2. Start a machine

`iris.toml` in the working directory is read unless `--config` says otherwise
(`iris-irix53.toml` / `iris-irix65.toml` are install templates). Top-level
scalars go before the first `[section]`; unknown keys are hard errors; missing
disk images are fatal at startup — a media-free machine needs an explicit empty
`[scsi]` table.

```sh
target/launcher/bin/iris --config iris.toml                    # window + monitor
target/launcher/bin/iris --config iris.toml --ci               # automation socket, offscreen graphics
target/launcher/bin/iris --config iris.toml --ci --headless    # no graphics; serial only
target/launcher/bin/iris --config iris.toml --ci --serial-log run.log
```

Useful flags: `--monitor-port` (default 8888), `--serial-port-a/b` (8880/8881),
`--gdb-port`, `--cpu r4400|r5000|r10000`, `--graphics newport|xz|extreme|solidimpact|highimpact|maximpact`,
`--nfs-dir DIR`, `--tftp-dir DIR`, `--rsh-dir DIR`, `--nat-subnet CIDR`.

**Check that your NVRAM actually reached the guest.** Symptoms that look like
"the guest ignored my environment" are usually worth one check: the kernel
prints the station address it adopted (`network: mac …`) early in the boot, so
compare that against the MAC in the image you passed. Passing signs of a stale
or wrong NVRAM (a derived MAC, a default environment) look identical to a guest
ignoring a `config_get`, and the wrong trail can cost a debug cycle.

**Lifecycle.** A fresh `--ci` machine is `starting`: the monitor answers but the
CPU is parked until `iris-ci start` (or `machine-start`). States are `idle`,
`starting`, `running`, `halted`; `machine-status json` reports `state`,
`cpu_running`, `monitor_ready`, `monitor_addr`. Poll, never sleep: the CI socket
exists only after the monitor binds.

**Clean shutdown matters.** Normal `Machine::stop` (monitor `machine-stop`,
`iris-ci quit`, guest power-off) saves NVRAM and the Indigo2 motherboard
EEPROM. `SIGINT`/`SIGTERM` do **not** stop the guest safely. Before a forced
kill, run `rtc save` (Indy) or `nveeprom save` (Indigo2) from the monitor.
Never kill iris while IRIX is running unless the disk is on a COW overlay —
a killed guest corrupts XFS, and the resulting TLBMISS panics look exactly like
emulator bugs (`clanker/testing/disk-image-hygiene.md`).

**Fleet instances** (parallel agents on one host):

```sh
iris --instance 0 --ci &            # own state dir, ports 9000+, MAC, NAT subnet, socket
iris --instance 1 --ci &
iris --instance 2 --print-instance  # key: value endpoints, don't boot
```

`--state-dir PATH`, `--port-base PORT`, `--replace` (stops the lock's owner,
cleanly over its CI socket when possible). One `Machine` per process — never
run two in one process. `scripts/fleet.sh -n N` starts a fleet; the lock is
`<state_dir>/iris.lock` and liveness is the file lock, not the PID. Details:
`docs/parallel-instances.md`.

## 3. Ways in

| Interface | Use for | Entry point |
|---|---|---|
| Monitor console | live debugger: regs, memory, breakpoints, devices, JIT | `tools/mon.py "cmd" ...`, `nc 127.0.0.1 8888`, or the terminal iris runs in |
| `iris-ci` | the canonical automation client for scripts/agents | `target/launcher/bin/iris-ci` or `$IRIS_SOCKET` |
| MCP server | monitor commands as agent tools | `clanker/iris_mcp.py` (`pip install mcp`), register in `.mcp.json` |
| GDB stub | gdb-native breakpoints/stepping | `--gdb-port 1234` |
| Serial console | guest console bytes, PROM, boot logs | TCP 8881 (ttyd1/console), 8880 (ttyd2); in-process via `iris-ci serial-*` in `--ci` |
| Guest shell | development inside IRIX | telnet forward 2323 + `tools/irix-sh.py` (§4) |
| NFS | host directory at `/shared` in the guest | `[nfs] shared_dir` / `--nfs-dir` |
| Filename services | read-only host dirs to the guest | `--tftp-dir` (PROM netboot), `--rsh-dir` (`rsh 192.168.0.1 ls|cat|dd|echo|pwd`) |
| Raw socket | non-`iris-ci` harnesses | newline-delimited JSON `{cmd, args}` → `{ok, data, error}` (`src/ci.rs`) |

**Monitor essentials.** `help` lists what this build registered; `[DEV]` markers
need the `developer` feature. Every command except `start/stop/status/wait/help`
pauses a running CPU for its duration. Arguments are hexadecimal (`0x`), octal
(`0`), or decimal; symbols work after `loadsym unix.map`; a value of at most
eight hex digits with bit 31 set is sign-extended (so `0x88001000` means the
KSEG0 address `0xffffffff88001000` — write more digits for the zero-extended
form). The prompt is `\r\n> ` **at line start** — never
search for a bare `"> "` (`clanker/testing/monitor-prompt-is-line-start.md`).

- **Hex needs the `0x` prefix.** `mem 885FFFD0 40 b` answers
  `Error: Invalid number: 885FFFD0`; `mem 0x885FFFD0 40 b` works. A scripted
  monitor session that "cannot see" memory is usually this.
- A `[DEV]` command is *accepted* by every build and does nothing without the
  feature — silence is not proof it ran.

| Want | Commands |
|---|---|
| run control | `start` `stop` `status` `wait [ms]` `run/c [addr]` `step/s [n]` `next` `finish` `jump <addr>` |
| registers | `regs` `cop0` `cop1` `setreg <reg> <val>` |
| memory | `mem/m <addr> [n] [b\|h\|w\|d]` `mw` `ms` `stack` `dis/d` `translate/t` `loadelf` `loadbin` |
| symbols / kernel | `loadsym unix.map` `sym` `bt` `eframe/ef <ep>` `putbuf/dmesg` `proc info` |
| breakpoints | `bp add/b <addr> [pc\|r\|w\|f\|pr\|pw\|pf] [if expr]` `bp list/del/enable/disable/ignore/clear` `bp save/load` |
| exceptions | `ex <all\|int\|tlb\|addr\|bus\|sys\|ri\|arith\|watch\|vce> <on\|off>` |
| history | `dt [n]` (last 1,048,576 instructions), `undo [n]`/`u` **[DEV]**, `debug on` **[DEV]** |
| TLB/caches | `tlb dump` `tlb trans <va> [asid]` `l1i/l1d/l2 <check\|dump>` `ll` |
| snapshots | `save <name>` `load <name>` `reset` `machine-status [json]` |
| SCSI | `scsi status/regs/list/eject/add/del/next` (ids per controller; `scsi1 ...` on Indigo2/O2), `cow status/commit/reset` |
| graphics | `rex …` `vc2/xmap/cmap/bt445/disp …` (Newport); `gr2 …` `re3 …`; `mgras …` `rss …`; `iris-ci screenshot` |
| devices | `mc regs/dma` `hpc3 status` `ioc status` `pdma` `rtc` `eeprom` `nveeprom` `ps2 …` `serial status` `vino status` |
| input | `ps2 type <text>` `ps2 enter` `ps2 esc` `ps2 key <mod> <down\|up>` `ps2 mouse <dx> <dy> [btn]` |
| JIT v2 | `j2 status` `j2 dispatch/inline/opt/flush` `j2 pcp [addr]` `j2 corpus` `jitcheck <n>` **[DEV]** |
| diagnosis | `locks`, `perf snapshot`, `log <module> <on\|off>` **[DEV]**, `perfetto start FILE [cats=]` |
| O2 (IP32) | `crime/gbe/pci/mec/…` and the `o2 shot/status/type/key/mouse/peek/…` family |

Breakpoints need a non-`lightning` build: a `lightning` build *accepts*
`bp add` and `bl` lists it, but it never fires (§1). A hit while running is
reported by `status`/`wait`, not printed. Monitor breakpoints (ids from 1) and
GDB's (from 10000) coexist.

**`iris-ci` cheat sheet** (`iris-ci <cmd> --help` for each; `--json`, `-q`,
`--consumer NAME`, `--socket PATH`/`$IRIS_SOCKET` are global):

```sh
iris-ci status --wait-ready 60          # poll until the monitor binds
iris-ci start                           # unpark the CPU (idempotent)
iris-ci boot                            # PROM menu → IRIS console login (~40 s)
iris-ci login                           # root + vt100 prompt + wait for '#'
iris-ci run 'ls /'                      # exit code = the guest command's
iris-ci wait-prompt
iris-ci serial-send '5'
iris-ci serial-read
iris-ci serial-wait 'Option?' --timeout 60
iris-ci screenshot out.png
iris-ci save base/desktop               # saves/<name>/ under iris's cwd
iris-ci restore base/desktop            # full reload, ~150 ms
iris-ci rollback                        # in-memory rewind, ~40 ms
iris-ci diff base mutated               # device/RAM-chunk/COW deltas
iris-ci tree                            # snapshot parent chains; also list/info/delete/gc
iris-ci validate base/desktop -n 1000000
iris-ci put localfile /tmp/file         # bs=512/csh handled; binary-safe
iris-ci get /tmp/out --to ./out
iris-ci cdrom-load 4 disc2.iso          # or cdrom-eject 4
iris-ci rtc-save                        # chd-sync and quit [--sync-chd] too
iris-ci script tests/scenario.iris      # one command per line, one connection
iris-ci pull http://reg/snap NAME       # push NAME http://reg/ to publish
```

- Serial cursors are **per-consumer and forward-only**: a wait returns bytes
  from where that consumer stopped, so sequential waits must name patterns in
  the order the guest prints them, or each assertion needs its own
  `--consumer` name. The `--serial-log` file remains the full transcript.
- `run`/`boot`/`login` handle csh (no `2>&1`, typed-input echo) — don't
  reimplement their waits on raw serial unless you must
  (`clanker/snapshot/iris-ci-is-the-canonical-ci-socket-interface.md`).
- `validate` is bit-deterministic only with `--features ci_clock`; with the
  default wall clock, guest Count reads are expected to diverge.

**MCP server** tools: `run_command` (anything), `get_status`/`wait_for_stop`,
`start_cpu`/`stop_cpu`, `step`/`next_instruction`/`continue_execution`/`finish_function`,
`get_registers`/`read_cop0`/`read_cop1`, `read_memory`/`write_memory`/`read_string`,
`disassemble`, `add_breakpoint`/`remove_breakpoint`/`list_breakpoints`,
`backtrace`/`traceback`/`undo`, `translate_address`/`dump_tlb`,
`lookup_symbol`, `read_eframe`/`kernel_log`. Everything else goes through
`run_command`; it retries until iris is up.

**GDB stub** (`--gdb-port 1234`, one client):

```
set architecture mips:isa64
set mips abi n64
set mips mask-address off
set heuristic-fence-post 0
target remote localhost:1234
```

`set mips abi n64` is required (else 64-bit registers show as 32-bit);
`mask-address off` stops GDB truncating `0xffffffff80010000`-style addresses.
Developer builds start paused, so you can inspect before the first instruction.
A hand-rolled RSP client must send the same settings: the `g` packet before the
ABI is set comes back as something that is not a register blob, while `m`
(memory), `Z2` (watchpoint), `c` and `?` work without it. Replies are
run-length encoded — a run of N identical characters is sent as the character,
`*`, and `chr(N + 28)`, and the decoded length is what you count, not the raw
packet length.

**The stub perturbs timing; the monitor does not.** Merely having `--gdb-port`
enabled (no client attached) changed host scheduling enough to hide a
timing-sensitive kernel bug that otherwise reproduced on every boot, and the
same run failed only when the stub was *absent*. Prefer the monitor for
anything that is not a deterministic breakpoint: it is always listening
(`--monitor-port`), and it can inspect a machine that is already wedged.

## 4. Developing inside the guest

Add once to the machine config (`tools/irix-sh.py` and `iris-ci put/get`
depend on them):

```toml
[nfs]
shared_dir = "./shared"                # host dir appears as 192.168.0.1:/ → mount at /shared

[[port_forward]]
proto = "tcp"
host_port = 2323
guest_port = 23
bind = "localhost"
```

Boot and drive:

```sh
iris-ci boot && iris-ci login
iris-ci run 'uname -a'

# or over telnet, session-per-invocation:
tools/irix-sh.py wait --timeout 900
tools/irix-sh.py run 'cc -version' 'command2'     # exit = last command's; -k keeps going
tools/irix-sh.py run 'mkdir -p /shared/out' 'cp /tmp/x /shared/out/'
tools/irix-sh.py push local.c /tmp/local.c        # text only
tools/irix-sh.py pull /tmp/out.txt ./out.txt
tools/irix-sh.py mount-shared                     # mount the NFS share at /shared
```

`tools/irix-sh.py` defaults to `root@localhost:2323`, no password
(`--password`/`$IRIX_ROOT_PASSWORD` if the guest has one; never commit a
password). A fresh IRIX install has none.

**Files in and out.** Use `iris-ci put/get` for anything (they handle the
`bs=512` scratch/raw-device rules); use NFS (`/shared`) for work in progress.
`/shared` is mounted `noexec` — copy sources and binaries to `/tmp` before
running them. `/tmp` is emptied at boot; re-stage after every boot. A `scratch = true` SCSI volume at `/dev/rdsk/dks0d2s0` is the
networking-free fallback (`iris-ci scratch`, or `dd bs=512` by hand; reads must
be 512-multiples).

**Desktop session (X/GL programs need one).** Without a logged-in session,
xdm's login window owns the display:

```sh
tools/irix-sh.py run 'echo root > /etc/autologin' 'touch /etc/autologin.on'
```

`/etc/autologin.on` is **one-shot** — re-create it before every shutdown. The
session appears ~1 min after telnet: wait for `4Dwm` and `toolchest` in
`ps -ef`, add ~15 s, then `xset s off` (the screen saver blanks probes after
10 min).

**MIPSpro builds** (copy sources to `/tmp/<work>` first):

| ABI | Sysroot | Compile | Shared link |
|---|---|---|---|
| o32 | `/usr/lib` | `cc -o32 -mips2 -O2` | `cc -o32 -mips2 -shared -soname libfoo.so -set_version sgi1.0 -check_registry /usr/lib/so_locations -all … -lc -no_unresolved` |
| n32 | `/usr/lib32` | `cc -n32 -mips3 -O2` | same with `-n32 -mips3`, registry `/usr/lib32/so_locations` |
| n64 | `/usr/lib64` | `cc -64 -mips3 -O2` (or `-mips4`) | same with `-64 -mips3`, registry `/usr/lib64/so_locations` |

- Copy `so_locations` to `/tmp` before `-update_registry`; never update the
  system registry from a scratch build.
- Test a rebuilt library without installing: stage under `/tmp/stage/{lib,lib32,lib64}`
  and point rld at it (`LD_LIBRARY_PATH`, `LD_LIBRARYN32_PATH`,
  `LD_LIBRARY64_PATH`); `_RLD_ARGS="-v"` reports loads. Never overwrite
  `/usr/lib*/lib*.so` to test.
- Inspect: `elfdump -h -L -Dc`, `odump -D`, `showfiles --`, `versions -n | cat`
  (everything pages through `more`).
- Stock IRIX: no bash, no `pkill`, `chmod` wants `a+x`; root's *login* shell is
  csh (`2>&1` is sh-only — use `>&`). Wrap sh-only syntax in `sh -c "..."`.

**Debugging inside the guest:** application misbehaviour is usually a driver /
hardware-contract question — use the `irix-kernel-debug` skill (`loadsym unix.map`,
PROM `setenv showconfig 1`, `putbuf`, `eframe <ep>`, conditional breakpoints,
SCSI probe-to-mount walk). Graphics: `irix-gfx-loop` skill. Packaging:
`irix-inst-packaging` skill (`inst -a -f dist -I prod`, patches via
`gendist -maint`).

**Shutdown** (always — see §2):

```sh
tools/irix-sh.py run 'touch /etc/autologin.on; sync; shutdown -p -g0 -y'
```

Release builds exit when the guest powers off; `developer` builds stay up —
wait ~1 min and check the log/`status` before killing by exact PID.

## 5. Snapshots, overlays, disk safety

- `overlay = true` on a raw disk keeps the base read-only (`scsi1.raw.overlay`;
  `cow status/commit/reset`). CHD images always write to a `.diff.chd` sidecar;
  `iris-ci chd-sync` or `quit --sync-chd` folds it back.
- Snapshot inner loop: `save` once, then `restore` (~150 ms) or `rollback`
  (~40 ms) between test attempts. Schema v3, content-addressed RAM, CPU-model
  checked on restore; GR2/IMPACT graphics state is incomplete. `--features
  ci_clock` + `iris-ci validate` proves bit-determinism.
- In `--ci` the COW overlay lives at `/tmp/iris-ci-<pid>-scsi<id>.overlay`,
  not beside the image (find it with `lsof -p <pid> | grep overlay`).
- Mounting a written-into image on Linux: the XFS root is not at offset 0
  (SGI DVH, type 10; typically 136314880). Dirty logs need
  `xfs_repair -L` first — IRIX's big-endian log can't be replayed
  (`clanker/testing/disk-image-hygiene.md`).

## 6. Observe, trace, measure

- Console transcript: `--serial-log FILE`, `--serial-log-timestamps off|guest|wall|both`
  (timestamp modes are line records, not byte-exact captures).
- Device logs: `developer` build + `log <module> <on|off>` / `IRIS_DEBUG_LOG=scsi,net`
  (modules `net hpc3 seeq hal2 mc rex3 mips ioc scsi pdma vino scc ps2 rtc …`).
- Any build: `IRIS_TFTP_TRACE=1` (BOOTP/TFTP), `IRIS_RSH_TRACE=1`.
- Graphics: `rex trace/rec`, `gr2 trace`, `mgras trace/rec` (`gputrace`), replayed
  offline as unit tests (`REX3_REPLAY=…`, `GR2_REPLAY=…`, `MGRAS_REPLAY=…`) —
  see `irix-gfx-loop`.
- Timelines: `--features perfetto`, `--perfetto FILE [--perfetto-cats …] [--perfetto-clock host|guest]`
  or monitor `perfetto start/cats/stop`; categories `cpu irq timer dma scsi net
  audio gfx prim jit`. Host clock is `CLOCK_BOOTTIME` (loads beside `perf`).
- JIT diagnosis: `IRIS_JIT_DISPATCH=off` (interpreter fallback),
  `IRIS_NO_INLINE_MEM`, `IRIS_JIT_CACHE(_DIR)`, `j2 pcp`/`j2 corpus`, `jitcheck`.
- Runtime interrupts/parking: `locks`, `perf snapshot`, `idle on|off|status`,
  `idleprof report`.
- Release ROM smoke: set `IRIS_NO_IDLE=1` (a no-disk PROM boot can park in a
  loop that isn't woken; `docs/ci.md`).

**Inspecting a machine that has already wedged.** A trap handler that loops
(the guest prints e.g. `trap: TLB refill epc=… (software refill not
implemented)`) leaves every register and byte in place, keeps `status:
running`, and still answers the monitor. Reproduce the hang, then read the
scene rather than re-running it:

```
status          # still "running" — the CPU is spinning, not stopped
regs            # PC is inside the handler; EPC/BadVAddr are in cop0, not regs
cop0            # EPC, BadVAddr, Cause — what faulted and where
mem 0x8810b000 256 b   # a static, a buffer, a descriptor ring…
dis 0x88094ff0 8       # the code at EPC/the caller
```

`regs` alone is misleading here: `PC` is the handler's loop, while the
interesting address is `CP0 EPC`/`BadVAddr` (shown by `cop0`, and printed by
the guest's own handler when it has one).

## 7. Tests and benchmarks

```sh
nix develop --command cargo test --workspace          # add --features jitv2 for equivalence tests
make -C cpu-tests && make -C cpu-tests run            # bare-metal instruction correctness
FORCE_BUILD=1 cpu-tests/run/matrix.sh                 # R4400/R5000 × interp/jitv2
./target/release/iris-bench run [--quick]             # throughput + accuracy, no toolchain needed
./target/release/iris-bench {matrix | report --baseline r4400-interp | host}
test/fleet/run.sh target/debug/iris target/debug/iris-ci   # $FLEET_N, $KEEP=1
test/journal/run.sh                                        # snapshot journal record/replay
```

- `cargo test --workspace` covers per-device snapshot round trips, CPU/TLB,
  REX3, and with `jitv2` the JIT equivalence suite. GUI: `-p iris-gui`.
- `make -C cpu-tests run` does **not** exit 0 on a healthy tree. Build with
  default features so the binary matches, then diff the failing **identities**
  against your own baseline — counts drift with load and config
  (`clanker/testing/cpu-tests-known-failure-baseline.md`).
  `Error 124` means the 600 s timeout killed the run (no result, not a
  failure); `run/matrix.sh` reuses stale binaries unless `FORCE_BUILD=1`.
- `iris-bench` runs a checked-in prebuilt guest image in-process; accuracy is
  per-kernel checksum agreement. IRIX workloads: `iris-bench irix --socket`.
- Media-free end-to-end: `test/fleet/run.sh` (two `--instance` VMs, PROM
  command monitor over CI serial) and `test/journal/run.sh`.
- Don't judge graphics/timing bugs from `iris-gui` under Xvfb/llvmpipe: the
  software compositor starves the CPU thread. Reproduce with `iris --ci` first
  (`clanker/testing/gui-under-xvfb-starves-cpu.md`).

## 8. Rules that keep you out of trouble

- Never kill iris while IRIX runs (unless on an overlay); shut the guest down
  or `quit` via the monitor/CI. `rtc save`/`nveeprom save` before any forced
  termination.
- One `Machine` per process; use `--instance` for parallelism. Never invent a
  per-instance path or port — `--print-instance` is the discoverability
  surface.
- Poll for readiness (`iris-ci status --wait-ready N`), don't sleep; a `--ci`
  machine answers while still `starting`.
- Monitor prompt must match at line start (`\r\n> `).
- **Never grade a build from one boot.** A guest bug that corrupts memory can
  flip on code layout alone: in one kernel bisect, adding ≈1 KiB of
  never-executed diagnostics turned a 3-out-of-3 trap into a 3-out-of-3 pass,
  and only the *unmodified* build was reliably clean. Run each candidate build
  at least three times, and bisect by changing one file at a time rather than
  by reasoning from symptoms — a symptom that moves with the code is a timing
  bug, not a code bug.
- When a failure is that layout-sensitive, suspect a race between the guest and
  the peripherals' host threads (DMA, NAT, audio) before the guest's own logic.
  Deterministic-looking evidence (`EPC` values that repeat every run of the
  same ROM) is consistent with a race that the same instruction stream always
  loses.
- Blueprint discipline when changing iris: monitor commands →
  `debug.md`; traits/threading/GFIFO/jitv2 publication → `HACKING.md`;
  config/CLI/env → `FEATURES.md`+`HELP.md`; new gotchas → `clanker/<area>/` +
  `clanker/INDEX.md`.
- Never copy SGI source text; provenance and attribution rules are in
  `clanker/CLANKER.md`. No AI `Co-Authored-By` trailers on commits.

## 9. Where to read next

| Need | Read |
|---|---|
| Complete monitor / GDB / MCP / tracing reference | `debug.md` |
| Running, config, serial, NVRAM, NFS, port forwards | `HELP.md` |
| Every feature, config key, CLI flag, env var | `FEATURES.md` |
| iris-ci, snapshots, overlays, scratch volume | `STORAGE.md` |
| Suites and methodology | `TESTING.md`, `BENCHMARKS.md`, `bench/README.md` |
| Parallel instances model and tickets | `docs/parallel-instances.md` |
| Perfetto categories and instrumentation | `docs/perfetto.md` |
| GitHub Actions setup | `docs/ci.md` |
| Remote X login (XDMCP) | `docs/xdmcp.md` |
| O2 (IP32) specifics | `docs/o2-ip32.md` |
| Subsystem gotchas, indexed | `clanker/INDEX.md` |
| Guest shell / graphics / kernel / packaging workflows | sibling skills in `clanker/skills/` |
