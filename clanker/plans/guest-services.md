# Guest service for IRIX 6.5 — plan

**Status:** draft 2026-10-27. Planning only; no code. Branch `plan/guest-services`,
base `irix7/iris` `8da7ae2` (0.1.10+irix7).

This plan proposes an installable IRIX guest service in the shape of QEMU's
guest agent, and the host-side IRIS support needed to use it. The initial goal
is narrower than "QGA on IRIX": **get host resource usage for parallel
instances under control**. The fork has just shipped the IRIX 6.5 idle
detector, so the plan starts from that result rather than proposing another
one.

**Primary use case: headless software development.** The fleet is used for
serial/telnet consoles, NFS or scratch sources, builds and tests, not desktop
sessions. v1 therefore prioritises headless lifecycle, load telemetry, clean
shutdown and a named headless profile that stops a desktop session if the
image starts one. Desktop-instance management is out of scope.

---

## 0. Recommendation in one page

| Decision | v1 |
|---|---|
| Guest component | `iris_guestd`, a small o32 C daemon running as root |
| Transport | SCC channel A (`/dev/ttyd2`), in-process `GuestLink` backend on the host |
| Protocol | Newline-delimited JSON, a deliberately small QGA subset (not wire-compatible with QGA) |
| Host surface | `[guest] enabled = true`; `iris-ci guest-*` over the existing CI socket; `guest ...` monitor commands |
| Lifecycle policy | Observe `idle status` plus guest load; apply the headless profile; cleanly stop unused instances with `guest-shutdown powerdown` |
| Packaging | One IRIX `inst` product, `iris_guest`, one `sw` subsystem, `.tardist` built in the guest with MIPSpro + `gendist` |
| New emulated hardware | None |
| New host port/path | None (the CI socket and the existing state directory are the boundary) |
| Pause/resume | **Out of scope**; the guest cannot pause the VM, and `machine-stop` is not a transparent pause |
| Second idle detector | **Not proposed**; the host detector already does this |
| Headless profile | **v1**: stop `xdm`/`amstart` via a fixed allowlist if the image started them; report the result. No arbitrary command execution. |
| Effort | ~13–16 engineer-days for one engineer, including rig time |

The honest summary: **a guest agent will not lower the 2 % host CPU a settled
6.5 getty already uses.** It earns its place by making it safe to *stop*
instances that are running but unused, by replacing console scraping for
lifecycle and telemetry, by giving the fleet a reliable answer to "is this
guest actually doing work?", and by stopping a desktop session that a headless
image did not need — the one case where the host detector cannot park because
the CPU is busy in a userland spin.

---

## 1. What the guest service is for on IRIX

### 1.1 The baseline is already good

Measured on `callisto` (x86_64, 16 cores), IRIX 6.5 IP22, R4400, fresh disk,
recorded in `clanker/perf/idle-pause-work.md` and
`clanker/testing/release-idle-pause-hangs-prom-boot.md`:

| State | Host CPU |
|---|---|
| No idle-pause, 6.5 getty | 200–202 % |
| Idle-pause, 6.5 getty | 2 %, 2 %, 2 % |
| Two parallel instances, each getty | 1–2 % each |
| Idle-pause cold boot to login | 42 s |
| `ci_clock,idle-pause` cold boot to login | 46 s |

The other half of the original 200 % was a separate bug: `seeq-enet` set
`busy = true` after one frame and its idle fast path never cleared it
(`clanker/devices/seeq-enet-busy-pinned-by-idle-fast-path.md`). Both fixes are
in 0.1.10. **Do not re-litigate them.**

The idle detector is in `src/cpu/idle_park.rs`: it samples the PC every 32
instructions into a 32-entry ring, groups samples into PC-space clusters, and
parks only when one long kernel cluster dominates with at least 12 distinct
PCs, interrupts enabled, a mask bit set, nothing pending and Compare armed. It
rejects short DELAY and device-poll loops, refuses user-dominated samples, and
starts 60 s after the CPU thread starts. `idle on|off|status` and
`idleprof stats` expose it (`debug.md`).

### 1.2 Can a guest agent reduce host CPU further?

Three separate questions hide in that one:

1. **Can a guest-side idle detector beat the host-side one?** No. The host
   detector sees the PC stream and every interrupt; it parks the CPU thread in
   place and wakes on the interrupt that ends the idle loop. A guest daemon
   can only poll or hook the scheduler. Polling adds wakeups; hooking the
   scheduler needs a kernel driver. Either is worse. **No second idle
   detector.**
2. **Can a guest daemon shave the remaining ~2 %?** No meaningful amount. The
   remaining 2 % is spread across the host timer tick, the parked CPU's
   periodic wakeups, and the device threads that idle-pause does not park
   (SCSI, ethernet/NAT, serial, hptimer); this breakdown is inferred from the
   thread list, not separately measured. A daemon blocked in `read()` on a
   serial line adds no wakeups and no measurable CPU. At best it can report
   *why* an instance is not parking.
3. **Can a guest agent reduce total host resource usage?** Yes, but by
   **changing the lifecycle**, not the steady state. An instance that is
   running but unused still costs:

   - ~2 % of a core for the CPU thread and tick;
   - its RAM (typically 64–256 MB for an Indy/Indigo2 profile);
   - five to ten host threads (SCSI, enet/NAT, serial, hptimer, monitor, CI);
   - host scheduler pressure and cache footprint.

   For 16 idle instances that is ~32 % of one core, several GB of RAM and a
   few hundred threads. Stopping the ones nobody needs is the win. A clean
   guest shutdown is the primitive that makes that safe.

### 1.3 Where the value actually is

| Need | Today | With the guest service | How it is measured |
|---|---|---|---|
| Clean stop/reboot | Scrape the console or telnet, type `shutdown`, hope the prompt parser is right; `iris-ci quit` saves NVRAM but does not flush the guest filesystem | `iris-ci guest-shutdown powerdown --wait 120`; the guest runs `/etc/shutdown`, XFS unmounts, then `Machine::stop` saves NVRAM and release builds exit | Shutdown time, exit status, no XFS repair on next boot |
| Load/telemetry | Parse `uptime`, `date`, `ps` from the console; each scrape costs guest CPU and bytes on the console | `guest-get-load`, `guest-get-time`, `guest-get-osinfo`, `iris-get-status`; one small request, no shell, no console echo | Request latency, bytes on the wire, guest CPU per scrape |
| Fleet policy | The scheduler cannot tell "at getty" from "running a build" without scraping | Park counters (host) plus guest load (guest) plus job markers (launcher) | False stops in a fleet run |
| Automation | `iris-ci run/put/get` drive `/bin/sh` and parse prompts; csh quirks, base64 over the console | v1 only telemetry/lifecycle; v2 `guest-exec`/`guest-file-*` gated | Console bytes per operation, failure rate |
| Coverage the detector misses | A headless guest with `xdm` spinning on an absent framebuffer never parks (87,700 distinct PCs over 3 s in the 5.3 investigation) | The agent applies the named headless profile in v1 (stop `xdm`/`amstart` through a fixed allowlist); the idle detector then parks normally | Host CPU before/after the profile; no userland process left spinning |
| Quiesce before a snapshot | `sync` typed at the console | `iris-sync-fs`; future fsfreeze-like cooperation | Snapshot validator pass rate |

The headless-`xdm` case is the one real "coverage" win, and it is **not** a
detector: it is a guest configuration action (`/etc/init.d/xdm stop` or the
equivalent script on the image) selected by the host profile. Because the fork
is used for headless development, it belongs in **v1**, behind a fixed
allowlist: the daemon may stop `xdm` and `amstart`, and nothing else. A desktop
instance simply does not enable the profile.

### 1.4 What it is not

- **Not QGA-compatible.** We borrow QGA's framing and a few command names and
  shapes. The transport, the service manager, the supported commands and the
  error semantics differ. A stock QGA client will not drive it.
- **Not a pause/resume mechanism.** IRIX has no suspend-to-RAM/disk that maps
  to QGA's `guest-suspend-*`. `machine-stop` restarts peripherals and panics a
  live kernel (`clanker/perf/idle-pause-work.md` §4.1), so it is not a pause.
  A host-side pause, if wanted later, must park the CPU in place like
  idle-pause; that is an IRIS feature, not a guest command.
- **Not fsfreeze.** We have not confirmed a supported IRIX 6.5/XFS freeze
  interface. `iris-sync-fs` calls `sync()` twice; it is weaker than QGA's
  fsfreeze and must be described as such.
- **Not an arbitrary exec/file service.** `guest-exec` and `guest-file-*` are
  excluded from v1 for security and scope. They are the main reason a guest
  agent becomes a root RCE, and the current console path already covers the
  rare cases.

---

## 2. Transport

### 2.1 Requirements

1. Available **before guest networking** (a broken `inetd`/route must not cost
   control of the instance).
2. No conflict with the serial console (`ttyd1`) or the PROM.
3. Per-instance and disjoint under `--instance`.
4. Reconnect after host or guest restart; no state that cannot be rebuilt.
5. No new emulated device or IRIX kernel driver.
6. Low host CPU when idle; no guest polling.
7. Local-only by default; no network exposure.
8. Cheap to implement and test.

### 2.2 Candidates

| Transport | Available | Throughput / latency | Conflicts | Host work | Guest work | Security | Cost | Verdict |
|---|---|---|---|---|---|---|---|---|
| **SCC channel A** (`/dev/ttyd2`, host `serial_port_a`) | After `init` reaches run level 2; before network | Baud-paced unless host-speed; 100–300 B JSON is fine; not for bulk | `ttyd2` may have a `getty` (`respawn`) on some releases; console is `ttyd1` | Add a `GuestLink` backend; no new port | Open tty, raw termios, read/write | Localhost/in-process; device mode must be tightened | **Low** | **v1** |
| **TCP port forward through NAT** | Only after guest network and the agent binds a port | Fast (emulated SEEQ, but orders of magnitude over 9600) | Needs a free guest port and a host forward; `bind=any` exposes it | Derive a per-instance host port in `derive_instance`; add forward | Agent listens on TCP; inetd or self-bind | Network-reachable; needs auth | Medium | v1.1 optional |
| **NFS `/shared`** | After mount; host serves it in-process | Bulk only | Mount can hang; `/shared` is `noexec` in the test loop | None (already exists) | Mount, poll files | Host directory permissions | Low | Payloads only |
| **Scratch SCSI volume** | After the guest mounts it | Bulk only; block device | Guest page cache vs host writes; mount/unmount | `iris-ci scratch-*` already exists | Mount, parse | Disk image access | Low | Payloads only |
| **New emulated device** (virtio-serial-like) | After an IRIX driver exists | Excellent | Needs a PCI/GIO device and a DDI/DKI driver | New device model, GFIFO/interrupt work | Kernel driver + user daemon | Device-local | **High** | Out of scope |
| **`hostcall` private syscalls** (existing IRIS feature) | Any user process; no network | Excellent | IRIS-only; `hostcall` Cargo feature; no host→guest push | New service in `iris-hostcall` | Compiled-in stub, poll for commands | Same process boundary | Medium | v2 data plane |

#### SCC channel A — the evidence

- IRIS wires channel A to `/dev/ttyd2` (DB-9) and channel B to `/dev/ttyd1`
  (console): `HELP.md` "Serial ports", and `src/dev/z85c30.rs` `DEFAULT_PORTS`
  (8880/8881).
- IRIX 6.5 ships `ttyd2` **off** by default:
  `t2:23:off:/etc/getty -N ttyd2 co_9600 # port 2` in the IRIX Admin guide's
  inittab example, and the same line in *IRIX Admin: Peripheral Devices*
  (007-2861-005), which explains how to change it to `respawn` for a terminal
  or modem. Some sites do change it; the package must detect that (see §5.5).
- In `--ci` mode, IRIS installs `CiSerialBackend` on **channel B only**
  (`src/machine.rs:616-623`); channel A keeps its default TCP backend (or a
  null backend under `new_null`). A fleet instance therefore cannot reach a
  guest agent on ttyd2 today. v1 adds a `GuestLink` backend on A for both
  modes.
- The existing TCP serial A backend is a **telnet** endpoint: it sends a
  12-byte IAC handshake on connect, strips inbound IAC sequences, and doubles
  `0xFF` in guest output (`src/dev/z85c30.rs:536-547`,
  `src/net/telnet.rs`). A JSON client that connects to 8880 must be a telnet
  client, not a raw byte pipe. An in-process `GuestLink` avoids that entirely.
- TX is paced from the guest's WR12/WR13 baud divisor (`src/dev/z85c30.rs`
  `update_tx_delay`); RX holds a byte when the 8-byte SCC FIFO is full and
  sleeps 10 ms when the backend is empty. At 9600 baud a 100-byte JSON line is
  ~100 ms; at 38400 it is ~25 ms. That is fine for control, not for file
  transfer. `SerialBackend::host_speed()` already lets the CI console bypass
  pacing; `GuestLink` returns `true`.
- The guest tty is one byte stream. The TCP backend lets several clients
  observe and type, and `CiSerialBackend` gives each consumer its own read
  cursor, but the agent serialises requests; no multiplexing protocol is
  needed for v1.

#### TCP port forward — why not v1

- It works only after IRIX networking is up and the agent is listening. The
  whole point of a lifecycle channel is to work when networking does not.
- It needs a guest listener (self-bind, or inetd/xinetd), a host forward, and
  a per-instance port. `derive_instance` already adds a telnet forward to
  guest 23 (`src/config.rs:382-387`); a second forward is easy but adds a
  second moving part and a second failure mode.
- It is reachable from the guest's network. Without authentication, any peer
  that can reach the guest port can power the VM off. Token auth and a
  localhost-only bind would be required; that is more work than the serial
  path.
- Keep it as the v1.1 bulk path (file transfer, larger exec output) and as a
  debugging fallback (`guest_tcp_port`, off by default, bound to 127.0.0.1 on
  the host forward).

#### NFS and scratch — why not a control channel

- NFS is excellent for moving a tardist, source tree or log. It is not a
  request/response channel: the host cannot interrupt the guest, the guest must
  poll, and a hung mount can hang the agent's startup.
- The scratch SCSI volume is a block device the host can write while the guest
  runs. Without guest cooperation, host writes race the guest page cache; the
  guest must mount/unmount it. It is a payload channel, not a control channel.
- Both remain the right way to deliver the installer and collect logs.

#### A new emulated device — why not

- A virtio-serial-like device needs an IRIX PCI/GIO driver built with
  MIPSpro/DDI-DKI, plus device model, interrupt and DMA work in IRIS. That is
  weeks of driver work before the first `guest-ping`, for a channel the SCC
  already provides.
- It is the right long-term answer if the guest service becomes a platform
  feature (file/exec, bulk transfer, many clients). It is not the MVP.

#### `hostcall` — the interesting fifth option

`iris-hostcall` already lets an IRIX user program make an indirect `syscall`
1000 with a service number 3000–3009; IRIS answers it before the kernel raises
an exception. It needs no device and no driver, and it is fast. Two problems
for v1:

1. It is IRIS-specific. The guest agent would not run on real IRIX, under
   another emulator, or in a test that disables `hostcall`.
2. Host→guest delivery is the hard direction. The daemon would have to poll
   (`ENOTYET` or a timed retry), which wakes the guest and works against the
   idle goal. A blocking "wait for host command" syscall is possible but is a
   new ABI and a new liveness problem.

Keep it as the v2 data plane if the service grows beyond JSON control.

### 2.3 Recommendation

**v1 uses SCC channel A with an in-process `GuestLink` backend.**

- No new port, no new path, no guest network, no new device.
- Per-instance by construction: every instance already has its own SCC and
  state directory; `--ci` instances get a `GuestLink` on A instead of a null
  backend.
- The host owns both ends; the agent is a plain tty daemon that also works on
  a real IRIX machine with a serial cable.
- `host_speed()` removes baud pacing on the IRIS side; the guest's 8-byte FIFO
  and per-byte IRQ remain the limit. Measure it; if a command costs more than
  ~50 ms, optimise the backend before adding a second transport.
- The TCP serial endpoint is not used by default. If `[guest] enabled` is set,
  channel A is owned by the guest service; `serial_port_a` is not bound and
  the startup preflight must skip it. Prefer a constructor/flag that does not
  bind A at all: `Z85c30::new_with_ports` binds first and `set_backend_a`
  replaces the listener afterwards, which briefly holds the port and logs a
  misleading "serial port unavailable" warning if it is busy. Document that
  the ttyd2 console is no longer available while the service is enabled.

Wire behaviour:

- One request at a time, one outstanding id. The host serialises with a mutex;
  the agent processes lines in order.
- On connect (host process start, machine start, or after a reset) the host
  sends `guest-sync` and discards everything until the matching reply. QGA's
  `guest-sync-delimited` (a `0xFF` sentinel before the reply) is supported by
  the agent; over `GuestLink` the byte is raw, over the TCP debug path it is
  doubled by telnet escaping, so the client must decode it.
- A partial line older than 2 s is discarded when the next line or sync
  arrives. No timer in the agent (see §3.2); the age check is lazy.
- On snapshot restore/rollback, `Machine::ci_restore`/`ci_rollback` call
  `GuestLink::reset()`, which clears both queues and bumps a generation. The
  host client returns `GuestReset`; the next request re-syncs.

---

## 3. The guest agent

### 3.1 Language and ABI

**C89, o32, MIPSpro.** Reasons:

- IRIX 6.5's native compiler is MIPSpro 7.4.x; the packaging tools (`gendist`,
  `strip`, `tag`) exist only in the guest, so the build already runs there.
- o32 is the ABI that also exists on 5.3, and `-mips2` code runs on every CPU
  IRIS emulates (R4400/R5000/R10000). n32 is 6.x-only and buys nothing for a
  single-threaded byte pump. Build `-o32 -mips2`. (5.3 support is not tested
  in v1; the o32 build is a starting point, not a promise.)
- No threads, no C++, no `<stdint.h>` (MIPSpro's C is C89-ish; use
  `<sys/types.h>` and `unsigned long long`).
- Dynamic link against libc is fine; the daemon starts from `rc2.d` after
  `/usr` is mounted. (A static build is a possible rescue variant; not v1.)

Build command (in the guest):

```sh
cc -o32 -mips2 -O2 -Wall -fullwarn -o iris_guestd iris_guestd.c
strip iris_guestd
```

**Why not a shell script?**

| | C daemon | Shell |
|---|---|---|
| Raw tty | `tcgetattr`/`tcsetattr`, exact flags | `stty raw -echo` races with getty and other users of the port |
| JSON | one small parser | `case`/`read` cannot parse JSON reliably; quoting is a trap |
| Load/time | `getloadavg`/`sysmp`, `gettimeofday` | spawns `uptime`/`date` per request: guest CPU, load, fork noise |
| Idle | blocks in `read()`, zero wakeups | a `while read` loop is fine, but every helper is a process |
| Errors | errno, timeouts, partial lines | no structured errors |
| Install | one binary | still needs an init script and a package |

A shell agent is a useful 30-line prototype for `ping`/`shutdown`, not the
shipped service.

### 3.2 Process model

```
/etc/init.d/iris_guestd start
  -> /usr/sbin/iris_guestd -d
       open /dev/ttyd2 O_RDWR|O_NOCTTY|O_NONBLOCK
       TIOCEXCL (best effort)
       tcgetattr; raw 8N1; CLOCAL|CREAD; no flow control; B38400
       clear O_NONBLOCK; install SIGTERM/SIGHUP/SIGPIPE handlers
       write /var/run/iris_guestd.pid under an fcntl write lock
       loop: blocking read(fd, buf, sizeof buf)
             accumulate to '\n'
             parse one request, execute, write one response
```

Rules that matter for the host-CPU goal:

- **No periodic timer.** Do not use `select()` with a timeout, `alarm()`, or a
  1 s heartbeat. The daemon sleeps in `read()` until a byte arrives. A signal
  interrupts `read` (`EINTR`) so `SIGTERM` still works.
- **No subprocess per request.** `getloadavg`/`sysmp`, `gettimeofday`,
  `uname` and `gethostname` are direct calls. The only `fork`/`exec` is
  shutdown/reboot.
- **No logging per request.** Log startup, shutdown, errors and (with `-v`)
  requests to syslog. A log line per ping would be guest disk I/O.
- **One process.** If a second starts, the `fcntl(F_SETLK)` on the pidfile
  fails and it exits 1. This avoids two readers stealing tty bytes.

Signals:

- `SIGTERM`: set `stop`, close the tty (unblocks `read`), remove the pidfile,
  exit 0.
- `SIGHUP`: close/reopen the tty and reset the parser. Useful after a host
  restart or a tty hangup.
- `SIGPIPE`: ignore; `write` to a closed tty returns `EIO`/`EPIPE`.
- `SIGCHLD`: ignore or reap; the shutdown child is waited for only to log a
  failure.

Termios sequence (exact flags, no `cfmakeraw` requirement):

```c
struct termios t;
tcgetattr(fd, &t);
t.c_iflag &= ~(IGNBRK|BRKINT|PARMRK|ISTRIP|INLCR|IGNCR|ICRNL|IXON|IXOFF);
t.c_oflag &= ~OPOST;
t.c_lflag &= ~(ECHO|ECHONL|ICANON|ISIG|IEXTEN);
t.c_cflag &= ~(CSIZE|PARENB|CSTOPB);
t.c_cflag |= CS8|CLOCAL|CREAD;
t.c_cc[VMIN] = 1;
t.c_cc[VTIME] = 0;
cfsetispeed(&t, B38400);   /* verify B38400 on the guest */
cfsetospeed(&t, B38400);
tcsetattr(fd, TCSANOW, &t);
```

`O_NONBLOCK` on open, then clear it: IRIX's tty open can wait for carrier; the
`CLOCAL` flag is set only after open. If open returns `EBUSY`, retry a few
times and log. `TIOCEXCL` prevents a second opener (including a getty); if the
kernel rejects it, log and continue.

Device permissions: the init script should `chmod 600 /dev/ttyd2` and
`chown root:sys /dev/ttyd2` before starting, if the site has no getty on that
port. This is the main defence against a local user injecting commands. Verify
the default mode on the guest (`ls -l /dev/ttyd2`) before deciding whether the
chmod is necessary.

Init script (`/etc/init.d/iris_guestd`, `#!/sbin/sh`):

```sh
case "$1" in
start)
    [ -c /dev/ttyd2 ] || exit 0
    [ -x /usr/sbin/iris_guestd ] || exit 1
    if grep 'ttyd2' /etc/inittab 2>/dev/null | grep respawn >/dev/null 2>&1; then
        echo "iris_guestd: ttyd2 has a getty (inittab respawn); set it off and run telinit q" >&2
        exit 1
    fi
    chmod 600 /dev/ttyd2 2>/dev/null
    /usr/sbin/iris_guestd -d
    ;;
stop)
    if [ -f /var/run/iris_guestd.pid ]; then
        pid=`cat /var/run/iris_guestd.pid`
        # Verify the pid still belongs to the daemon before signalling it.
        # Check the exact ps -o syntax on IRIX; adjust if needed.
        if ps -p "$pid" -o comm 2>/dev/null | grep iris_guestd >/dev/null 2>&1; then
            kill "$pid" 2>/dev/null
            sleep 1
            kill -9 "$pid" 2>/dev/null
        fi
        rm -f /var/run/iris_guestd.pid
    fi
    ;;
restart)
    $0 stop
    sleep 1
    $0 start
    ;;
status)
    if [ -f /var/run/iris_guestd.pid ] && kill -0 `cat /var/run/iris_guestd.pid` 2>/dev/null; then
        echo "iris_guestd running"
    else
        echo "iris_guestd stopped"
    fi
    ;;
esac
```

Run-level links: `/etc/rc2.d/S33iris_guestd` and
`/etc/rc0.d/K03iris_guestd`, both pointing at `../init.d/iris_guestd`.
`S33` is early enough to start before the network scripts and late enough that
`/usr` is mounted; verify the local `rc2.d` ordering and adjust the number if a
site's scripts differ. IRIX uses the `rc2.d`/`rc0.d` S/K convention (IRIX Admin:
System Configuration and Operation, ch. 3); `chkconfig(1M)` is the local admin
tool for enabling/disabling services, but the package should install the links
explicitly so `inst` owns them. Do not edit `/etc/inittab`.

### 3.3 Protocol

**Framing.** One UTF-8 JSON object per line, terminated by `\n`; accept
`\r\n`. Requests are at most 4096 bytes; responses at most 16 KiB. A longer
line is discarded to the next `\n` and answered with `InvalidParameter`.
Empty lines are ignored.

**Request:**

```json
{"execute":"guest-ping","id":1}
{"execute":"guest-get-load","id":2}
{"execute":"guest-shutdown","arguments":{"mode":"powerdown"},"id":3}
```

`execute` is required; `arguments` is optional; `id` is optional but echoed if
present. The host always sends an id. Unknown top-level keys are ignored;
unknown `arguments` keys are rejected.

**Response:**

```json
{"return":{"version":"1.0.0","protocol":1},"id":1}
{"return":{"load1m":0.02,"load5m":0.01,"load15m":0.00},"id":2}
{"return":null,"id":3}
{"error":{"class":"CommandNotFound","desc":"guest-exec is not supported by iris_guestd 1.0.0"},"id":9}
```

Error classes: `GenericError`, `CommandNotFound`, `InvalidParameter`,
`Unsupported`, `PermissionDenied`, `InternalError`. The host treats an unknown
class as `GenericError`.

**Sync.** On connect and after any timeout:

```json
-> {"execute":"guest-sync","arguments":{"id":123456789},"id":99}
<- {"return":123456789,"id":99}
```

`guest-sync-delimited` is the same but the response is prefixed with `0xFF`.
The agent resets its parser and output buffer before replying. A leading
`0xFF` in the request stream is ignored and also resets the parser, matching
QGA's client-side flush convention.

**JSON subset.** The agent's parser accepts:

- objects with string keys, nesting depth 2 (`request` -> `arguments`);
- strings of printable ASCII with `\"` and `\\` escapes (reject other escapes);
- signed 64-bit integers, booleans and `null`;
- no arrays, floats in requests, or Unicode escapes.

That is enough for the v1 command set and keeps the parser ~250 lines. The
host uses `serde_json`; the agent only has to parse requests and print
responses. If `guest-exec`/file ever lands, use a real JSON library or extend
the parser deliberately.

**QGA subset implemented in v1:**

| Command | Arguments | Returns | Notes |
|---|---|---|---|
| `guest-sync` | `id` int | the same int | stream resync |
| `guest-sync-delimited` | `id` int | `0xFF` + the same int | QGA-compatible sentinel |
| `guest-ping` | none | `{version, protocol}` | liveness; QGA returns nothing, we add the two fields so one round trip identifies the peer |
| `guest-info` | none | `{version, protocol, supported_commands}` | capability discovery |
| `guest-get-time` | none | int nanoseconds since epoch | QGA shape |
| `guest-get-load` | none | `{load1m, load5m, load15m}` | if the API exists |
| `guest-get-osinfo` | none | QGA `GuestOSInfo` subset | `id`, `name`, `pretty-name`, `version`, `version-id`, `kernel-release`, `kernel-version`, `machine` |
| `guest-get-host-name` | none | `{host-name}` | `gethostname` |
| `guest-shutdown` | `mode` = `powerdown` (default), `reboot` | `null` before acting | `halt` rejected in v1 |
| `iris-get-status` | none | `{agent, load, os, config}` | IRIX extension, compact |
| `iris-sync-fs` | none | `{synced:true}` | `sync(); sync();` |

**Deliberately excluded from v1:**

| QGA command | Why excluded |
|---|---|
| `guest-exec`, `guest-exec-status` | Root RCE; arrays/output capture; needs an allowlist and a separate threat model |
| `guest-file-open/read/write/seek/flush/close` | Root file access; bulk data belongs over NFS/scratch, not a 9600-baud line |
| `guest-fsfreeze-*` | No confirmed IRIX/XFS freeze interface; `iris-sync-fs` is the honest subset |
| `guest-fstrim` | No discard/TRIM path in this stack |
| `guest-suspend-ram/disk/hybrid` | IRIX has no suspend; IRIS snapshot is host-side and not a guest suspend |
| `guest-set-time` | Setting the RTC/time from the host is risky under emulation; not needed for the goal |
| `guest-set-vcpus`, `guest-get-vcpus`, `guest-set-memory-blocks`, `guest-get-memory-blocks` | No hotplug in IRIS; the machine profile is fixed at start |
| `guest-network-get-interfaces`, `guest-network-get-route` | `getifaddrs`/route parsing not verified; add in v1.1 |
| `guest-get-fsinfo`, `guest-get-disks`, `guest-get-diskstats` | Inventory, not lifecycle; add when needed |
| `guest-get-users`, `guest-set-user-password`, `guest-ssh-*` | Security surface with no v1 use |
| `guest-get-cpustats`, `guest-get-devices` | Kernel counter/device enumeration work; `getloadavg` covers the goal |
| Pause/resume/idle hint | The guest cannot pause the VM; the host already detects idle (see §1.2) |

**Deviations from QGA, documented in `guest-info`:**

- Transport is a serial line, not virtio-serial; there is no
  `/dev/virtio-ports/org.qemu.guest_agent.0`.
- `guest-shutdown` **does** return a response (then acts 200 ms later), unlike
  QGA's `success-response: false`. This lets `iris-ci` distinguish "agent
  absent" from "shutdown accepted".
- `id` is always echoed; QGA clients that omit it are fine.
- `guest-ping` returns `{version, protocol}` instead of an empty object. A
  strict QGA client would ignore the extra fields; we do not claim one will
  work.
- No events, no `guest-exec`, no file handles, no fsfreeze.
- `guest-get-load` may be `Unsupported` on a build without `getloadavg`.

### 3.4 IRIX API mapping and checks

Do not trust a web man page for these; run the check on the target 6.5 guest
and record the result in `clanker/irix/`.

| Need | Intended call | Check on the guest | If absent |
|---|---|---|---|
| Load average | `getloadavg(3)`, else `sysmp(MP_SAGET, <load selector>, ...)` | `man getloadavg \| cat`; `grep -n 'MPSA_\|MP_SAGET' /usr/include/sys/sysmp.h`; if the symbol is only in the BSD library, link `-lbsd` | Return `Unsupported`; host uses park counters only |
| Time | `gettimeofday(2)` | `man gettimeofday \| cat` | Always present |
| OS identity | `uname(2)`, `gethostname(2)` | `uname -a`; `man uname \| cat` | Always present |
| Raw tty | `tcgetattr`/`tcsetattr`, `cfsetispeed` | `man termios \| cat`; `man tcsetattr \| cat` | POSIX termios is present on 6.5 |
| Exclusive open | `ioctl(TIOCEXCL)` | `grep TIOCEXCL /usr/include/sys/ioctl.h` | Skip; rely on `chmod 600` |
| Daemonise | `daemon(3)`, else double-fork | `man daemon \| cat` | Double-fork |
| PID lock | `fcntl(F_SETLK)` | `man fcntl \| cat` | `O_EXCL` pidfile only |
| Shutdown | `/etc/shutdown -y -i0 -g0` | `man shutdown \| cat`; `ls -l /etc/shutdown /sbin/shutdown` | Try `/sbin/shutdown`; else `Unsupported` |
| Reboot | `/etc/shutdown -y -i6 -g0` | `man shutdown \| cat`; `grep '^rb:' /etc/inittab` | Reject `reboot` |
| Process count (optional) | `opendir("/proc")` | `ls /proc \| head`; `man proc \| cat` | Omit from status |
| Init links | `/etc/rc2.d`, `/etc/rc0.d` | `ls /etc/rc2.d \| head`; `man chkconfig \| cat` | Install links explicitly |
| Serial device | `/dev/ttyd2` | `ls -l /dev/ttyd2`; `grep ttyd /etc/inittab` | Package installs but service does not start; log |

`reboot(2)` is deliberately not used: `/etc/shutdown` is the documented,
tested path and it runs the rc scripts and `uadmin`. If a future version calls
`reboot(2)`, verify the `RB_*` constants in `<sys/reboot.h>` first.

MIPSpro 7.4 supports `long long`, but verify `printf("%lld", ...)` on the
target; if the libc's `printf` rejects it, format the 64-bit value with a
small helper rather than dropping to seconds.

### 3.5 Agent failure handling

| Failure | Agent behaviour |
|---|---|
| Malformed JSON | Reply `InvalidParameter`; keep the connection; discard to newline |
| Oversize line | Discard to newline; reply `InvalidParameter` |
| Unknown command | `CommandNotFound`; list supported commands in `guest-info` |
| Unknown argument | `InvalidParameter` |
| `guest-get-load` unavailable | `Unsupported`; `guest-info` omits it from `supported_commands` |
| Shutdown disabled by config | `PermissionDenied`; no action |
| `fork`/`exec` fails | Log to syslog; keep serving; a `guest-shutdown` caller times out and the host falls back |
| Tty hangup / `EIO` | Log, close, try to reopen after 1 s, up to a few times; then exit 1 (init can restart) |
| `SIGTERM` during a request | Finish or abort the write, close, exit; the host times out and re-syncs |
| Parser desync | `guest-sync` resets the parser and buffers |

### 3.6 Security model

- **Trust boundary:** the host-side CI socket / monitor, not the serial line.
  `GuestLink` is in-process; there is no TCP listener by default.
- **Root daemon, fixed command set.** The agent runs as root because
  `/etc/shutdown` needs it and the tty is root-owned. It exposes no arbitrary
  exec, file access, password change or SSH key operation. The worst a caller
  can do is power off/reboot the guest and read load/time/os identity.
- **Local users.** A user who can write `/dev/ttyd2` can issue those commands.
  The init script sets the device to `0600 root:sys` and uses `TIOCEXCL`. This
  is a deliberate, documented trade-off; if the site needs ttyd2 for a terminal,
  do not install the agent on that machine.
- **Network.** No network listener. The v1.1 TCP transport must bind the guest
  listener to the NAT address only, forward to host `127.0.0.1`, and require a
  token stored `0600` in the guest; do not ship it on by default.
- **Host side.** The CI socket is the authority. Its permissions are the state
  directory's; the fleet launcher should create state directories `0700`.
  Anyone who can talk to the CI socket can already `quit` the machine, so the
  guest service adds no new host-side privilege.
- **Logging.** Log command names and errors, never full request bodies if a
  future command carries data. No secrets in v1.

---

## 4. Host-side IRIS integration

### 4.1 Modules and types

| File | Change |
|---|---|
| `src/guest_link.rs` (new) | `GuestLink: SerialBackend`: `to_guest`/`from_guest` queues, `push_host`, `wait_line`, `reset`, `generation`, 64 KiB cap, `host_speed() = true` |
| `src/guest.rs` (new) | `GuestService`: request/response matching, `guest-sync`, timeouts, `ping/info/load/time/osinfo/shutdown/status`, error types |
| `src/dev/z85c30.rs` | No protocol logic; `set_backend_a` already exists. Optionally expose `guest_link()` for diagnostics |
| `src/machine.rs` | Create `GuestLink` + `GuestService` when `cfg.guest.enabled`; install the link on SCC A before `start()`; store the service; `get_guest()` |
| `src/dev/hpc3.rs` | Hold `Option<Arc<GuestService>>`; register `guest` monitor command and dispatch it |
| `src/ci.rs` | Hold `Option<Arc<GuestService>>`; add `guest-*` and `idle-status` commands; reset on restore/rollback |
| `src/config.rs` | `GuestConfig` + `[guest]` table + CLI flags + validation; `--print-instance` line |
| `src/instance.rs` | Skip the serial-A listener preflight when guest mode owns A |
| `src/iris_ci_main.rs` | `guest-ping`, `guest-info`, `guest-load`, `guest-time`, `guest-status`, `guest-shutdown`, `guest-sync`, `idle-status` |
| `src/monitor.rs` | No change if `guest` is registered on HPC3 (already registered) |
| `iris-gui` | Out of scope; later a Debug-tab status line |
| `clanker/iris_mcp.py` | No new tool; `run_command "guest status"` covers it |

`GuestService` API sketch (host side):

```rust
pub struct GuestService {
    link: Arc<GuestLink>,
    next_id: AtomicU64,
    state: Mutex<ClientState>, // needs_sync, last_error, last_seen
}

impl GuestService {
    pub fn request(&self, cmd: &str, args: Option<Value>, timeout: Duration)
        -> Result<Value, GuestError>;
    pub fn ping(&self, timeout: Duration) -> Result<GuestInfo, GuestError>;
    pub fn load(&self, timeout: Duration) -> Result<Option<GuestLoad>, GuestError>;
    pub fn shutdown(&self, mode: ShutdownMode, timeout: Duration) -> Result<(), GuestError>;
    pub fn status(&self) -> GuestStatus; // configured, responding, version, last_error
    pub fn reset(&self);
}
```

The `request` method:

1. Lock `ClientState` (one request at a time).
2. If `needs_sync`, send `guest-sync` with a random id and discard until its
   reply; on timeout return `GuestTimeout`.
3. Assign an id, serialise the request with `serde_json`, append `\n`, and
   `GuestLink::push_host`.
4. `GuestLink::wait_line(deadline)`; discard lines that are not a response
   with the matching id (stale replies, events).
5. On timeout: set `needs_sync`, return `GuestTimeout`. On generation change:
   return `GuestReset`.

`GuestLink` is deliberately not `CiSerialBackend`: the CI console needs
consumer cursors and a 1 MiB transcript; the guest link needs line framing,
one client and a hard cap. Reusing `CiSerialBackend` would couple two
different policies.

### 4.2 Configuration and CLI

Host TOML:

```toml
[guest]
enabled = false          # opt-in; no change to existing configs
transport = "scc-a"      # v1: only value
request_timeout_ms = 2000
shutdown_timeout_ms = 120000
```

CLI: `--guest` (enable), `--guest-transport scc-a`, `--guest-timeout-ms N`.
Validation:

- `enabled = false` (default): behaviour byte-identical to today.
- `transport != "scc-a"`: error at load time, naming the future values.
- `enabled = true` and an explicit `serial_port_a`: warn that guest mode owns
  channel A and the configured port is not bound.
- `enabled = true` on the O2 (`Ip32Machine`): warn "guest service not
  implemented on this profile" and leave it off; v1 targets Indy/Indigo2.

`--print-instance`:

```
instance: 3
state_dir: /home/me/iris-fleet/iris-instance-3
monitor: 9030
serial_a: guest (in-process)
serial_b: 9032
ci_socket: /home/me/iris-fleet/iris-instance-3/iris.sock
guest: scc-a
```

No new `StatePaths` accessor and no new `derive_instance` value in v1. If a
per-instance guest log is added later, it must be
`StatePaths::guest_log()` -> `<state_dir>/iris-guest.log`; do not invent a
path at the call site. If a TCP transport is added later, the port must be
derived in `derive_instance` (e.g. `port_base + 4`), not chosen by a script.

One `Machine` per process (the fleet invariant) means the `GuestLink` and
`GuestService` are fields of `Machine`, not process globals. A fleet of N
instances is N processes, each with its own link, its own agent session and its
own SCC; nothing is shared. `derive_instance` has no guest endpoint to derive
because v1's transport is in-process. That is deliberate, not an omission:
adding a port here would be the first thing to go through the seam.

### 4.3 CI and monitor surface

New CI commands (newline-delimited JSON over the existing socket):

| Command | Arguments | Returns |
|---|---|---|
| `guest-ping` | `timeout_ms?` | `{version, protocol, latency_ms}` |
| `guest-info` | `timeout_ms?` | the agent's `guest-info` reply |
| `guest-load` | `timeout_ms?` | `{supported, load1m, load5m, load15m}` |
| `guest-time` | `timeout_ms?` | `{nanoseconds, seconds}` |
| `guest-status` | none | `{configured, responding, version, protocol, last_error, last_seen_ms}` |
| `guest-shutdown` | `mode`, `wait_ms?` | `{accepted, stopped, elapsed_ms}` |
| `guest-sync` | `id?` | `{synced, id}` |
| `idle-status` | none | park counters from `idle_park.rs` |

`idle-status` reads the existing public statics (`PARK_COUNT`,
`PARK_SLEEP_NS`, `PARK_WAKE_IRQ`, `PARK_WAKE_OTHER`, `UPDATES`, `RESET_*`,
`WINDOW_*`) and returns them as numbers. It does not touch the CPU lock and
does not stop the machine.

`guest-shutdown` semantics:

- Send the request; wait for the ack (default 2 s).
- If `wait_ms` is 0 (default), return `{accepted:true}` immediately after the
  ack. The caller can poll `iris-ci status`.
- If `wait_ms > 0`, poll `Machine::status`/the process until halted or the
  timeout; return `{accepted, stopped, elapsed_ms}`.
- If the link closes before the ack, treat it as accepted only if the write
  completed; the agent replies before acting, so a lost ack usually means the
  machine went down. Log the ambiguity.
- Never call `machine-stop`, `idle off`, or `SIGKILL` from this handler.

Monitor commands (registered on HPC3, which the monitor already holds):

```
guest status
guest ping
guest info
guest load
guest time
guest osinfo
guest shutdown powerdown|reboot
guest sync
guest reset
```

`guest status` prints configured/responding, agent version/protocol, the last
error, and the last request/response size. It must never block for more than
the request timeout.

Example session:

```console
$ iris-ci --socket /home/me/iris-fleet/iris-instance-3/iris.sock guest-ping
guest-ping: ok 1.0.0 protocol 1 (2.1 ms)
$ iris-ci --socket ... guest-load
load 0.02 0.01 0.00
$ iris-ci --socket ... idle-status
parked 1421 times, slept 812.4 s, woke 1420 on irq, 1 other
$ iris-ci --socket ... guest-shutdown --mode powerdown --wait 120
accepted; stopped in 8.4 s
```

### 4.4 Lifecycle policy — how an idle/load hint becomes a resource action

The policy is a **reference script** in v1 (`scripts/fleet-policy.sh`, or a
new `tools/fleet-policy.py` if shell parsing gets ugly), not a Rust subsystem.
It consumes three inputs:

1. **Host park state** from `iris-ci idle-status`: `park_count` and
   `sleep_ns` increasing over two samples means the CPU thread is parking.
2. **Guest load** from `iris-ci guest-load`: `load1m < 0.1` means no runnable
   work (one emulated CPU). `Unsupported` means "unknown", not "idle".
3. **Job state** from the fleet launcher: a `<state_dir>/busy` marker (v1).
   The CI connection count is available, but a "last client activity"
   timestamp is a v1.1 addition; do not infer it from the socket. The policy
   must never stop an instance that a job owns.

Reference rule:

```
every 30 s, for each instance:
    if <state_dir>/busy exists:                  continue
    if a CI client has been active in 5 min:     continue
    parked = idle-status.park_count increased over the last 5 min
    load   = guest-load (if supported)
    if parked and (load is unknown or load.load1m < 0.1)
       and no job for 15 min:
           iris-ci guest-shutdown --mode powerdown --wait 120
           if not stopped after 120 s: log; leave it running (do not SIGKILL)
```

Actions and non-actions:

| Action | v1? | Mechanism |
|---|---|---|
| Park | Already automatic | `src/cpu/idle_park.rs`; the policy only observes, never toggles `idle` |
| Pause | No | Would need an in-place host park API; `machine-stop` panics the kernel |
| Snapshot | Optional, off by default | `iris-sync-fs`, then `iris-ci save`, then `quit`; validate with `iris-ci validate`; only on COW/CHD overlays |
| Stop | Yes | `guest-shutdown powerdown`; the guest flushes and unmounts, then `Machine::stop` saves NVRAM |
| Force stop | No (manual only) | `iris-ci quit` is a host stop that does not flush the guest filesystem; document the XFS risk |

Why stop rather than park or snapshot:

| Policy | Host CPU | Host RAM | Reuse latency | Risk |
|---|---|---|---|---|
| Keep running, idle-pause | ~2 % | Full guest RAM | Immediate | None |
| Park (automatic) | ~2 % | Full guest RAM | Immediate | None; this is the steady state |
| Pause in place | ~0 (device threads remain) | Full guest RAM | Immediate | Not implemented; needs a host park API, not `machine-stop` |
| Snapshot + quit | 0 | Snapshot storage | ~145 ms restore (`iris-ci restore`) | Snapshot consistency and restore fidelity must be validated |
| Powerdown | 0 | 0 | ~42 s cold boot | None if the guest shuts down cleanly |

For a fleet that reuses an instance within minutes, pause or snapshot wins;
for one that leaves instances idle for hours, powerdown wins on RAM and thread
count. v1 implements the powerdown path because it is the one with no new
fidelity risk; snapshot+quit is the optional warm-pool path once
`iris-ci validate` passes on the image.

**No double-parking.** The policy never calls `idle off`/`idle on`; idle-pause
owns the CPU. A guest command wakes the CPU via the SCC IRQ, the agent answers,
and the CPU re-parks on the next idle window. The command's cost is one wake
plus a few hundred instructions, not a second parking mechanism.

**No hanging.** Every guest request has a hard timeout. `guest-shutdown`
returns after the ack by default; `--wait` is bounded. The CI handler must not
hold the machine-slot lock while waiting (copy the `Arc<GuestService>` out,
release the slot, then request). A timed-out request marks the link
`needs_sync` and the next request re-syncs. A dead agent leaves the instance
running and logs; the policy falls back to the console path only if it can
prove a shell prompt.

**Fallback when the agent is absent:**

1. `iris-ci status` says running; `guest-status` says not responding.
2. If the guest is at a shell prompt, use the existing console path:
   `iris-ci run 'shutdown -p -g0 -y'` (the `irix-guest-loop` skill's tested
   command) and wait for exit.
3. If the console is not at a prompt, leave the instance running and report
   "no clean stop path". Do not `SIGKILL` a running guest.

### 4.5 Concurrency and locks

- `GuestService` owns one `Mutex<ClientState>` and one `GuestLink`. The SCC RX
  thread only touches `GuestLink` queues; it never calls up into
  `GuestService`, `CiServer` or `Machine`.
- `CiServer` holds `Option<Arc<GuestService>>` at construction (like
  `ci_serial`), so a `guest-*` handler does not need `with_machine` and does
  not hold the machine slot while waiting.
- The monitor's `guest` command runs on the monitor thread and takes the same
  `GuestService` mutex; it waits at most the request timeout, so a stuck guest
  cannot wedge the monitor.
- On `ci_restore`/`ci_rollback`, call `guest.reset()` next to
  `ci_serial.reset()` (`src/machine.rs:1658-1685`).
- On `Machine::stop`/`start`, reset the link so stale bytes from a previous
  run are not read as a response.
- No device lock is held while doing tty I/O except the `GuestLink` queues;
  the actual `read`/`write` happen on the SCC threads. The `GuestService`
  mutex is a host-side serialiser, not a device lock, and is held only for the
  duration of one request.

### 4.6 Documentation and tests to update in the implementation change

- `debug.md`: `guest ...` monitor commands, `iris-ci guest-*`, `idle-status`.
- `HELP.md`: `[guest]` config, `--guest`, the package install and the fact
  that guest mode owns `ttyd2`.
- `FEATURES.md`: new config keys and CLI flags.
- `HACKING.md`: `GuestLink` is a host-speed in-process serial backend; no
  polling in the agent; do not use `machine-stop` as a pause.
- `docs/parallel-instances.md`: guest lifecycle section and status.
- `CHANGELOG.md`: under the release that lands it.
- `clanker/INDEX.md`: add the plan row now; add a howto/measure note when the
  measurements exist.
- `clanker/iris_mcp.py`: no new tool needed; `run_command` reaches `guest`.
- Tests: `tests/guest_protocol.rs` (codec, timeouts, sync, id mismatch,
  reset), `tests/guest_link.rs` (queues, line framing, host speed), a mock
  agent thread; `test/fleet/run.sh` unchanged (PROM only), plus a rig-only
  `clanker/testing/guest-service-e2e.md`.

---

## 5. Installable package

### 5.1 Product layout

| Item | Value |
|---|---|
| Product | `iris_guest` |
| Image | `sw` |
| Subsystem | `iris_guest.sw.base` |
| Version | `100` (1.0.0); protocol version `1` is separate |
| Description | "IRIS guest service for IRIX 6.5" |
| Files | `/usr/sbin/iris_guestd`, `/etc/init.d/iris_guestd`, `/etc/iris_guestd.conf`, `/usr/share/iris_guest/README`, `/usr/share/iris_guest/protocol.md` |
| Links | `/etc/rc2.d/S33iris_guestd`, `/etc/rc0.d/K03iris_guestd` -> `../init.d/iris_guestd` |
| Dependencies | libc only; no networking libraries, no threads |
| Build | native MIPSpro in the guest; `gendist` in the guest; host script drives it |

Repository tree:

```
guest/
  README.md                 # developer notes, not installed
  src/iris_guestd.c
  src/json.c, json.h        # optional split
  init/iris_guestd
  etc/iris_guestd.conf
  share/README
  share/protocol.md
  iris_guest.idb.in
  iris_guest.spec.in
  version                   # 100
tools/irix-guest-dist.sh    # host driver: push, build, gendist, pull
clanker/testing/guest-service-e2e.md
```

### 5.2 `iris_guest.idb` (sorted on fields 5 and 6)

```text
f 0755 root sys etc/init.d/iris_guestd etc/init.d/iris_guestd iris_guest.sw.base
f 0644 root sys etc/iris_guestd.conf etc/iris_guestd.conf iris_guest.sw.base config(suggest)
l 0777 root sys etc/rc0.d/K03iris_guestd etc/rc0.d/K03iris_guestd iris_guest.sw.base symval(../init.d/iris_guestd)
l 0777 root sys etc/rc2.d/S33iris_guestd etc/rc2.d/S33iris_guestd iris_guest.sw.base symval(../init.d/iris_guestd)
f 0755 root sys usr/sbin/iris_guestd usr/sbin/iris_guestd iris_guest.sw.base exitop("[ \"$rbase\" = / ] && $rbase/etc/init.d/iris_guestd restart || true") removeop("if [ -x $rbase/etc/init.d/iris_guestd ]; then $rbase/etc/init.d/iris_guestd stop; elif [ -f /var/run/iris_guestd.pid ]; then kill `cat /var/run/iris_guestd.pid` 2>/dev/null; fi; rm -f /var/run/iris_guestd.pid")
d 0755 root sys usr/share/iris_guest usr/share/iris_guest iris_guest.sw.base
f 0644 root sys usr/share/iris_guest/README usr/share/iris_guest/README iris_guest.sw.base
f 0644 root sys usr/share/iris_guest/protocol.md usr/share/iris_guest/protocol.md iris_guest.sw.base
```

Notes:

- `config(suggest)` keeps a site's edits on upgrade and installs the new file
  as `.N`; the agent keeps using the old file until the admin merges.
- The `exitop` starts/restarts only on a live root (`$rbase = /`). In a
  miniroot or a `-r` install it does nothing.
- The `removeop` calls the init script if it is still present (the script
  verifies the PID), falls back to the pidfile, and removes the pidfile. This
  keeps uninstall safe even if `inst` removes files in a different order.
- `inst` creates parent directories; the `d` line is explicit for clarity.
- No `tag()`: a daemon does not need a filetype rule. Add one only if a
  desktop entry is wanted.

### 5.3 `iris_guest.spec`

```text
product iris_guest
    id "IRIS guest service for IRIX 6.5"
    image sw
        id "IRIS guest service"
        version VERSION
        order 500
        subsys base default
            id "iris_guestd Control Daemon"
            replaces iris_guest.sw.base 0 maxint
            exp iris_guest.sw.base
        endsubsys
    endimage
endproduct
```

`VERSION` is substituted from `guest/version` by the build script. Versions
are integers; a dotted version breaks upgrades (packaging skill §1). No
`prereq` in v1: the daemon needs only libc. If a site wants a 6.5-only guard,
add `prereq ( eoe.sw.base 0 maxint )` after checking the product name on the
target.

### 5.4 Build

In the guest, from a copy in `/tmp` (`/shared` is `noexec`):

```sh
mkdir -p /tmp/iris-guest/root/usr/sbin
cc -o32 -mips2 -O2 -Wall -fullwarn -o /tmp/iris-guest/root/usr/sbin/iris_guestd iris_guestd.c
strip /tmp/iris-guest/root/usr/sbin/iris_guestd
# stage the rest under root/
sort -k 5,6 raw.idb -o iris_guest.idb
/usr/sbin/gendist -sbase root -idb iris_guest.idb -spec iris_guest.spec -dist out
cd out && tar cf ../iris_guest-100.tardist *
```

The host driver `tools/irix-guest-dist.sh`:

1. Reads `IRIS_GUEST` (the `tools/irix-sh.py` target) and a work directory.
2. Pushes text sources with `tools/irix-sh.py push`; binary inputs via
   `/shared` then `cp` to `/tmp`.
3. Runs the build commands above; fails on the first non-zero status.
4. Pulls `iris_guest-100.tardist` back with `tools/irix-sh.py pull` or copies
   it to `/shared`.
5. Records `cc -version`, `versions -n eoe | cat` and the `gendist` output in
   the build log.

Cross-compilation with clang is **out of scope**: the packaging tools and the
tested ABI are in the guest, and an IRIX sysroot/linker is not part of this
tree. Revisit only if native builds become a bottleneck.

### 5.5 Install, upgrade, uninstall

Install (from an extracted dist directory; `inst -a` is non-interactive):

```sh
mkdir -p /tmp/iris_guest-dist && cd /tmp/iris_guest-dist && tar xf /tmp/iris_guest-100.tardist
inst -a -f /tmp/iris_guest-dist -I iris_guest
versions -n iris_guest | cat
showfiles -- /usr/sbin/iris_guestd | cat
ps -ef | grep iris_guestd
```

The `exitop` starts the daemon. Check:

```sh
ls -l /dev/ttyd2
grep ttyd /etc/inittab
/usr/sbin/iris_guestd -f -v      # foreground debug if the service did not start
```

If `inittab` has `t2:23:respawn:... ttyd2 ...`, a getty owns the port. The
package must **not** silently edit `inittab`. The install `exitop` should
print a warning and leave the service stopped; the admin changes `respawn` to
`off`, runs `telinit q`, and starts the service. This is the one site
configuration the package cannot guess.

Upgrade: bump `guest/version` to `101`, rebuild, `inst -a -f ... -I iris_guest`.
The `exitop` restarts the daemon. Check `versions -n iris_guest` and
`iris-ci guest-info` report 1.0.1; check the old config is `.N` if it was
edited.

Uninstall:

```sh
inst -a -R iris_guest
versions -n iris_guest | cat
ps -ef | grep iris_guestd
ls -l /etc/rc2.d/S33iris_guestd /etc/rc0.d/K03iris_guestd
```

The `removeop` stops the daemon; `inst` removes the `l` links. If a modified
`/etc/iris_guestd.conf` is kept by `inst`'s config rules, document it and
remove it manually if the site wants a clean uninstall.

### 5.6 Test matrix

| Case | What it proves |
|---|---|
| 6.5.22 and 6.5.30 fresh installs | ABI/API assumptions; `ttyd2` default line |
| 6.5.28 upgraded from an older patch | `inst` upgrade path, `config(suggest)` |
| `ttyd2` off (default) | Normal start |
| `ttyd2` respawn (getty) | The package warns and does not fight the getty |
| `[guest] enabled` with `--ci` | `GuestLink` on A while CI drives B |
| `[guest] enabled` without `--ci` | Monitor `guest` commands, no serial-A TCP listener |
| Agent killed with `kill -9` | Host timeout, `guest-status`, restart recovery |
| Host restarted while agent runs | `guest-sync` on the new link |
| Snapshot restore/rollback | `GuestLink::reset()`, no stale response |
| 8 parallel instances | No cross-talk, per-instance stop |
| O2 (`Ip32Machine`) | Warns and stays off (not supported in v1) |

The end-to-end procedure goes in `clanker/testing/guest-service-e2e.md`; it
needs IRIX media, so it is a rig test, not a CI test. Host-only tests use a
mock agent thread and run in `cargo test`.

---

## 6. Failure modes and security

| Symptom | Detection | Host behaviour | Guest behaviour | Recovery |
|---|---|---|---|---|
| No agent installed | `guest-status` not responding after sync | Fall back to console or leave running | None | Install package |
| Agent crashed | `guest-ping` timeout | Mark not responding; log | init script does not auto-restart in v1; admin `restart` or reboot | `restart`; optionally add a supervisor in v1.1 |
| Agent hung in `write` | Timeout | `needs_sync`; next request syncs | Blocked until the host drains; no timer | Host reset/reopen; agent recovers |
| Partial line after host restart | Parser state | Send `guest-sync`; discard until match | Lazy age check discards stale partial line | Automatic |
| Guest/host version skew | `guest-info` protocol/version | Refuse commands newer than the host knows; warn | Unknown command -> `CommandNotFound` | Upgrade one side |
| Serial A busy / no `/dev/ttyd2` | open/EBUSY or missing node | `guest-status` not responding | Init script exits 0; no service | Fix port or config |
| getty on `ttyd2` | `grep ttyd /etc/inittab` | Install warns | Agent cannot open (busy) | Admin sets `off`, `telinit q` |
| Local user writes the tty | Device mode | None | Accepts fixed commands; worst case power-off | `chmod 600`, `TIOCEXCL`; do not install on shared terminals |
| Shutdown rejected | `PermissionDenied` | Report; no action | Config `allow_shutdown = 0` | Admin enables |
| Shutdown accepted but guest hangs | `--wait` timeout | Log; leave running; do not SIGKILL | rc script or driver hung | Inspect console; manual recovery |
| Poweroff before ack | Link EOF | Treat as accepted only if the write completed; verify with status | Agent writes ack, waits 200 ms, then execs | Check process exit |
| Snapshot restore | `ci_restore` | `GuestLink::reset()` | Agent keeps running; next sync | Automatic |
| Host restart | New process | New `GuestLink`; sync | Tty stays open; agent keeps reading | Automatic |
| Two host clients | `GuestService` mutex | Second waits; monitor has a timeout | Serialises requests | Automatic |
| Buffer overflow | 64 KiB cap | Drop oldest; mark overflow; log | N/A | Reset |
| Guest panic | No response; console shows panic | Policy leaves it; snapshot/crash log for diagnosis | N/A | Reboot/restore |

Security summary: local-only in-process transport, root daemon with a fixed
non-RCE command set, `0600` device, no network listener, no exec/file/password
commands. The CI socket remains the host-side authority; the guest service does
not widen it.

---

## 7. Milestones, effort, acceptance

### 7.1 Milestones

| Milestone | Work | Days | Exit criterion |
|---|---|---|---|
| M0 — Protocol + link, host-only | `src/guest.rs`, `src/guest_link.rs`, mock agent thread, unit tests | 2 | `cargo test guest_` covers ping/timeout/sync/reset/id mismatch |
| M1 — Agent on ttyd2 | C daemon, termios, JSON subset, init script; manual run on 6.5 | 3 | `guest-ping`, `guest-info`, `guest-get-load`, `guest-get-time` from the monitor |
| M2 — Lifecycle + headless profile | `guest-shutdown` ack/act, CI `guest-*`, `idle-status`, `iris-set-profile headless` (fixed `xdm`/`amstart` allowlist), policy script | 3 | Clean powerdown in ≤30 s p95; no fsck/XFS repair on next boot; profile idempotent and no userland spinner |
| M3 — Packaging | idb/spec, gendist, tardist, install/upgrade/uninstall | 2 | `inst -a` ≤30 s; upgrade keeps config; uninstall leaves no process/links |
| M4 — Fleet + headless | `[guest]`, `--guest`, `--print-instance`, preflight skip, 8 headless-instance run, profile + stop policy, docs | 3 | 8 instances: all ping; all apply headless; all stop; no cross-talk; idle CPU unchanged |
| M5 — Rig hardening | 6.5.22/6.5.30, getty/respawn case, crash/recovery, measurements | 2–3 | Test matrix green; numbers recorded in `clanker/` |
| v1.1 (optional) | TCP transport, `guest-get-fsinfo`/network, supervisor | 3–5 | Separate plan |

Total v1: **13–16 engineer-days**, assuming access to a 6.5 guest and the
packaging skill's workflow. The critical path is M1 (IRIX tty behaviour), not
the host code.

### 7.2 Acceptance metrics

| Metric | Baseline | Target | Method |
|---|---|---|---|
| Settled 6.5 getty host CPU, agent installed, no traffic | 2 % | ≤3 % (regression budget +1 %) over 60 s, 3 runs | `pidstat -p <pid> 1 60` or `/proc/<pid>/stat` deltas |
| Park behaviour with agent | parks | `park_count` and `sleep_ns` increase; agent has no periodic wakeups | `iris-ci idle-status`; guest `ps -l` |
| Clean powerdown from getty | console scrape | ≤30 s p95, process exits, next boot clean | 10 runs, `time iris-ci guest-shutdown --mode powerdown --wait 120` |
| Install | — | ≤30 s; no `inst` errors | `time inst -a ...` |
| Upgrade | — | ≤30 s; config `.N`; version 101 | install 100 then 101 |
| Uninstall | — | ≤10 s; no process, no links, `versions` empty | `inst -a -R` |
| Parallel 8 instances | 8 × 1–2 % | all at getty; all ping ≤500 ms; all stop ≤60 s; no cross-talk | `scripts/fleet.sh -n 8` + policy |
| Headless profile, image that starts xdm | 100 % CPU in the userland spin, never parks | profile stops `xdm`/`amstart`; settled CPU **1–5 %**; `idle status` parks | 3 runs, `pidstat`, `ps -l` shows no xdm/amstart |
| Headless build cycle | — | no stop while a job marker/load is high; stop after the idle threshold; next boot clean | scripted build + 3 cycles |
| Boot to login with agent | 42 s | ≤47 s | 3 runs |
| Protocol reliability | — | 10,000 pings, 0 errors, p99 ≤50 ms in-process | `tests/guest_protocol.rs` stress |
| Failure recovery | — | agent kill -> `guest-status` not responding ≤2 s; restart -> ping works | manual/rig |

### 7.3 Explicitly out of scope

- `guest-exec`, `guest-file-*`, `guest-fsfreeze-*`, `guest-ssh-*`,
  `guest-set-user-password`.
- General desktop management beyond the fixed `xdm`/`amstart` stop; starting
  or reconfiguring a desktop session is not a guest-service job.
- Pause/resume and any second idle detector.
- virtio/vsock/new emulated device; `hostcall` as the v1 transport.
- O2/IP32 support; IRIX 5.3 guarantee (o32 builds, not tested).
- TCP transport, remote network exposure, multi-client multiplexing.
- QGA wire compatibility or third-party QGA clients.
- A full fleet scheduler UI; only the reference policy script.
- Package signing, licensing, SGI patch products, GUI integration.
- Snapshot consistency guarantees beyond `sync()`.

### 7.4 Risks

| Risk | Mitigation |
|---|---|
| `ttyd2` has a getty on some release/site | Detect `inittab`; warn and stay stopped; document `telinit q` |
| `getloadavg`/`sysmp` selector differs | Verify on the guest; return `Unsupported`; policy uses park counters |
| SCC FIFO/latency makes commands slow | `host_speed`; measure; optimise the backend before adding TCP |
| CI mode leaves A null today | `GuestLink` install is part of M1; test both modes |
| Temptation to use `machine-stop` as pause | Documented as forbidden; policy only uses guest shutdown |
| Root daemon/device permissions | `0600`, `TIOCEXCL`, fixed commands, no exec/file |
| Stale bytes after restore | `GuestLink::reset()` in `ci_restore`/`ci_rollback` |
| Host/guest skew | Protocol version in `guest-info`; host feature-detects |
| Package install on a patch level we did not test | Test 6.5.22/6.5.30; document the tested versions |

---

## 8. Decisions for headless development

The fork is primarily used for headless software development, so these are
the v1 decisions. Revisit only if that use case changes.

1. **`ttyd2` is owned exclusively by the service.** The package detects an
   existing getty/respawn on channel A and stays stopped with a clear message;
   sites that need a terminal on ttyd2 do not install the service.
2. **A root daemon is acceptable for v1**, with a fixed command set, no
   `guest-exec`/`guest-file-*`, and no network transport. Privilege
   separation is a later hardening item, not a v1 requirement.
3. **Install in headless/fleet images first**, opt-in through the package.
   Standard desktop images are not changed by default.
4. **Load API:** verify `getloadavg(3)` on the target 6.5; if absent, use
   `sysmp(MP_SAGET, …)` only after checking the 6.5 header. If neither is
   available, `guest-get-load` returns `Unsupported` and the policy relies on
   host park counters plus job markers.
5. **Default idle action is `guest-shutdown powerdown`.** Snapshot+quit is a
   later warm-pool feature, not the v1 stop path.
6. **`iris-ci guest-*` requires `--ci`.** Headless fleet runs already use it;
   without `--ci`, the `guest ...` monitor commands remain available and the
   agent is still reachable, but there is no CI-socket surface.
7. **No per-instance guest log in v1.** Use the existing serial log and the
   agent's guest-side log; if a host-side log is needed later, add
   `StatePaths::guest_log()` through the seam rather than inventing a path.
8. **No QGA wire compatibility.** The protocol is a documented QGA-inspired
   subset; third-party QGA clients are not a goal.

The only remaining human check is the IRIX fact list: `getloadavg`/`sysmp`,
termios details, `/dev/ttyd2` permissions and the exact `xdm`/`amstart` init
scripts on the target image. Those are verification tasks for M1/M2, not
design decisions.

## 9. Sources

Verified means the page/source was read during this planning session; inferred
means it is a reasonable conclusion from the code or documentation but not
directly tested on IRIX 6.5.

### QEMU guest agent

| Claim | Source | Status |
|---|---|---|
| QGA transport options: `unix-listen`, `virtio-serial` (default), `isa-serial`, `vsock-listen`; default path `/dev/virtio-ports/org.qemu.guest_agent.0`; config keys | <https://qemu-project.gitlab.io/qemu/interop/qemu-ga.html> | verified |
| QGA uses QMP's JSON framing minus negotiation; one object per line; `{"return":...}` / `{"error":...}`; `guest-sync`/`guest-sync-delimited` with a `0xFF` sentinel | <https://wiki.qemu.org/Features/GuestAgent>, <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/main.c> | verified |
| Command set and shapes: `guest-ping`, `guest-get-time`, `guest-get-load` (`load1m/5m/15m`), `guest-shutdown` modes and `success-response: false`, `guest-exec`, `guest-file-*` (48 MB/base64), `guest-suspend-*`, `guest-fsfreeze-*` | <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/qapi-schema.json> | verified |
| QGA's POSIX load implementation calls `getloadavg(loadavg, 3)` under `CONFIG_GETLOADAVG` | <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/commands-posix.c> | verified |
| QGA's POSIX shutdown runs `/sbin/poweroff`, `/sbin/halt`, `/sbin/reboot` with `-P`, `-H`, `-r` on Linux | <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/commands-posix.c> | verified |
| virtio-console/serial is a virtio device with ports and virtqueues; a guest driver is required | <https://docs.oasis-open.org/virtio/virtio/v1.3/virtio-v1.3.html> (§5.3) | verified |

### Other guest agents (design comparison)

| Claim | Source | Status |
|---|---|---|
| Hyper-V integration services let the VM communicate with the host; they are host/guest services (heartbeat, KVP, shutdown, VSS) over Hyper-V's channel | <https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/reference/integration-services> | verified |
| VirtualBox Guest Additions provide guest properties and host-driven application control; the guest service is `VBoxService` and the channel is the VirtualBox guest device/HGCM | <https://www.virtualbox.org/manual/ch04.html> | verified (manual); HGCM detail inferred |
| SPICE's guest agent (`spice-vdagent`) uses a virtio-serial port named `com.redhat.spice.0` with `spicevmc` | <https://www.spice-space.org/spice-user-manual.html> | verified |
| VMware Tools uses a `vmtoolsd` guest service and a VMware-specific backdoor channel, not virtio | <https://techdocs.broadcom.com/us/en/vmware-cis/vsphere/tools/12-4-0/vmware-tools-administration-12-4-0/introduction-to-vmware-tools/vmware-tools-service.html> (services); the backdoor I/O port protocol is not in the official admin guide | service verified; channel partly inferred; not used as a design dependency |

The common pattern is a paravirtual device plus a guest driver. IRIX 6.5 has
neither for virtio, VMBus, HGCM or the VMware backdoor. That is the reason the
QGA design does not port directly, and the reason this plan uses a serial line.

### IRIX

| Claim | Source | Status |
|---|---|---|
| Default `ttyd2` line is `t2:23:off:/etc/getty -N ttyd2 co_9600 # port 2`; change to `respawn` to enable a terminal; `telinit q` re-reads inittab | *IRIX Admin: Peripheral Devices* 007-2861-005, pp. 7–12, <https://irix7.com/techpubs/007-2861-005.pdf> | verified (PDF text) |
| The inittab example also shows `t2:23:off:/etc/getty ttyd2 co_9600`; run-level 2 starts `rc2`, getty; `t2` is port 2 | *IRIX Admin: System Configuration and Operation* 007-2859-021, ch. 3, <https://techpubs.jurassic.nl/library/manuals/2000/007-2859-021/sgi_html/ch03.html> | verified |
| Shutdown: `/etc/shutdown -y -i0 -g0` goes to state 0; `of:0:wait:/etc/uadmin 2 0` powers off; `rb:6:wait:/etc/uadmin 2 1` reboots; `init 0`/`init 6` are the run-level forms | same, ch. 3 and inittab example | verified |
| Run-level scripts live in `/etc/init.d` with `S`/`K` links in `/etc/rc2.d`/`/etc/rc0.d`; `chkconfig(1M)` is the admin tool | same, ch. 3; `chkconfig` references in IRIX admin docs | verified for the layout; `chkconfig` behaviour with a new script: verify on the guest |
| inittab entry format `id:rstate:action:process`, actions `respawn`, `off`, `wait`, `once`, `boot`, `bootwait`, `sysinit`, `initdefault`; 512-char limit | IRIX `inittab(4)` (6.2 mirror), <http://ibgwww.colorado.edu/~lessem/psyc5112/usail/man/irix/inittab.4.html> | verified for 6.2; 6.5 assumed compatible (verify `man inittab`) |
| `init` run levels 0–6, `init Q` re-examines inittab, `/etc/ioctl.syscon` | IRIX `init(1M)` (6.2 mirror), <http://ibgwww.colorado.edu/~lessem/psyc5112/usail/man/irix/init.1.html> | verified for 6.2; verify on 6.5 |
| `inst` options `-a`, `-f`, `-r`, `-I`, `-R`, `-u`, `-F`, `-N`, `-M`; automatic mode; distribution directories | IRIX `inst(1M)` (6.2 mirror), <http://ibgwww.colorado.edu/~lessem/psyc5112/usail/man/irix/inst.1.html> | verified for 6.2; the packaging skill's tested 6.5 recipe is the working reference |
| Product/image/subsystem layout, idb/spec syntax, `gendist`, install/upgrade/remove | `clanker/skills/irix-inst-packaging/SKILL.md`; *IRIX Admin: Software Installation and Licensing* 007-1364-140, <https://techpubs.jurassic.nl/library/manuals/1000/007-1364-140/sgi_html/index.html> | verified (skill is the tested recipe) |
| `getloadavg(3)` / `sysmp(MP_SAGET, ...)` selector | IRIX `sysmp(2)` (5.3 man page), <https://techpubs.jurassic.nl/manpages_0530/cat2/standard/sysmp.html>, documents `MP_SAGET` and says the structure is selected by `arg1` in `<sys/sysmp.h>`; the load selector name must be read from the 6.5 header | partly verified; **verify on the guest** |
| termios raw mode, `tcgetattr`/`tcsetattr`, `TCSANOW`, `VMIN`/`VTIME` | POSIX `tcsetattr`, <https://pubs.opengroup.org/onlinepubs/9699919799/functions/tcsetattr.html>; IRIX 6.5 provides POSIX termios. Exact `struct termios` and `cfmakeraw` availability: verify on the guest | partly verified |
| `/dev/ttyd2` permissions and owner | Not verified in a 6.5 guest during this session | **verify on the guest** |

### IRIS repository

| Claim | Source | Status |
|---|---|---|
| Idle detector design and measurements (2 %, 1–2 % per instance, 42 s boot) | `src/cpu/idle_park.rs`; `clanker/perf/idle-pause-work.md`; `clanker/testing/release-idle-pause-hangs-prom-boot.md`; `CHANGELOG.md` 0.1.10 | verified |
| Serial A/B mapping, TCP 8880/8881, telnet filter, baud pacing, `host_speed` | `HELP.md` "Serial ports"; `src/dev/z85c30.rs` | verified |
| CI mode installs `CiSerialBackend` on B only; A keeps its default backend | `src/machine.rs:616-623` | verified |
| CI command table and request/response framing | `src/ci.rs:455-475`; `src/ci.rs:406-440` | verified |
| Instance derivation: monitor `9000 + N*10`, serial A/B `port_base+1/+2`, CI socket, NAT subnet, telnet forward | `docs/parallel-instances.md:48-72`; `src/config.rs:355-390` | verified |
| `StatePaths` accessors and the "no invented paths" rule | `src/state.rs`; `docs/parallel-instances.md` | verified |
| `machine-stop` restarts peripherals and panics a live kernel; idle-pause parks in place | `clanker/perf/idle-pause-work.md` §4.1 | verified |
| Guest shutdown command `shutdown -p -g0 -y` is tested in this repo's loop | `clanker/skills/irix-guest-loop/SKILL.md` §7 | verified |
| Package idb/spec/gendist recipe | `clanker/skills/irix-inst-packaging/SKILL.md` | verified |

---

## 10. Decision log

| Decision | Alternative | Why |
|---|---|---|
| SCC A serial, in-process link | TCP forward | Available before network; no auth surface; no new port; works in `--ci` |
| o32 C daemon | Shell, n32, Perl | No subprocess/polling, exact termios, o32 ABI, MIPSpro-native |
| QGA subset | Full QGA | Transport and service model differ; full QGA brings exec/file/RCE and Linux-specific suspend/fsfreeze |
| Ack before shutdown | QGA's no-response | Lets `iris-ci` report "accepted" vs "no agent"; the 200 ms delay covers the reply |
| Stop, not pause | Snapshot/pause | `machine-stop` panics the kernel; a transparent pause is a host feature, not a guest command |
| No second idle detector | Guest idle hint | The host detector sees the PC stream and every interrupt; a guest hint adds wakeups and trust |
| No new `StatePaths`/`derive_instance` values | Guest log/port | v1 needs neither; adding them later must go through the seam |
| Install links, not inittab edits | `inittab` respawn | The package must not fight a site's getty; `inst` owns `rc` links |
| Native MIPSpro build | Cross-clang | `gendist`/`strip` are in the guest; no IRIX sysroot in the tree |
