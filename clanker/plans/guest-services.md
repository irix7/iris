# Guest service for IRIX 6.5 on IRIS hostcall — plan

**Status:** draft 2026-10-27, revised. Planning only; no code. Branch
`plan/guest-services`, base `irix7/iris` `8da7ae2` (0.1.10+irix7), PR #115.

**Scope:** this guest service runs **only** in an IRIX 6.5 guest under IRIS
built with the `hostcall` feature. It is not a portable QEMU guest agent and
does not pretend to work on real IRIX or another emulator. If `hostcall` is
not built, the service is disabled; there is **no serial, TCP, Seeq, NFS or
scratch control fallback**.

---

## 0. Recommendation in one page

| Decision | v1 |
|---|---|
| Guest component | `iris_guestd`, a small o32 C daemon |
| Transport | **IRIS hostcall `STREAM` service, number 3001**, in memory |
| Framing | Binary frames carrying QGA-shaped JSON control messages and raw data chunks |
| Host surface | `[guest] enabled = true`; `iris-ci guest-*` over the existing CI socket; `guest ...` monitor commands |
| Control | `guest-ping/info`, `guest-get-time`, `guest-get-load`, `guest-get-osinfo`, `guest-shutdown`, `iris-get-status`, `iris-sync-fs` |
| Command pipe | `guest-run` (argv-only) and `guest-shell` (explicit shell), opt-in `allow_run` / `allow_shell` |
| Data plane | `STREAM` data frames; v1 carries command stdout/stderr; v1.1 adds raw-disk `guest-dd` |
| Offline provisioning | `iris-ci image-write` writes a halted image directly on the host (v1.1); this is faster than any guest path |
| Feature gating | `[guest] enabled` requires `--features hostcall`; otherwise the host disables it with a message; the agent exits with `EINVAL` if the syscall is absent |
| New emulated hardware | None |
| New host port/path | None (the CI socket and state directory remain the boundary) |
| Pause/resume | Out of scope; the guest cannot pause the VM, and `machine-stop` is not a transparent pause |
| Second idle detector | **Not proposed**; the host detector already does this |
| Effort | v1 12–14 engineer-days; v1.1 5–7 engineer-days |

The honest summary is unchanged: **the 2 % host CPU of a settled 6.5 getty is
already solved.** The guest service is a lifecycle, telemetry and automation
plane. It lets the fleet stop unused instances cleanly, answer "is this guest
doing work?", run a command without scraping the console, and move disk data
without guest networking. It does not lower the idle 2 %; its polling has a
small cost that must be measured and kept under budget.

---

## 1. What the guest service is for

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

The other half of the original 200 % was `seeq-enet` setting `busy = true`
after one frame and never clearing it on the idle fast path. Both fixes are in
0.1.10. Do not re-litigate them.

The idle detector is in `src/cpu/idle_park.rs`: PC samples every 32
instructions into a 32-entry cluster ring; park only when one long kernel
cluster dominates, with interrupts enabled, a mask bit set, nothing pending
and Compare armed. `idle on|off|status` and `idleprof stats` expose it.

### 1.2 Can the guest service reduce host CPU further?

1. **A guest-side idle detector cannot beat the host-side one.** The host sees
   the PC stream and every interrupt, parks the CPU thread in place and wakes
   on the interrupt that ends the idle loop. A guest daemon can only poll or
   hook the scheduler. Polling adds wakeups; a scheduler hook needs a kernel
   driver. **No second idle detector.**
2. **The remaining ~2 % is not the agent's to remove.** It is spread across
   the host timer tick, parked-CPU wakeups and the device threads idle-pause
   does not park (SCSI, ethernet/NAT, the console, hptimer). This breakdown
   is inferred from the thread list, not separately measured.
3. **The win is lifecycle, not steady state.** An instance that is running but
   unused still costs ~2 % of a core, its RAM (typically 64–256 MB), five to
   ten host threads, scheduler pressure and cache footprint. For 16 idle
   instances that is ~32 % of one core, several GB and hundreds of threads.
   Stopping the ones nobody needs is the win; a clean guest shutdown is the
   primitive that makes it safe.
4. **The new cost is polling.** Hostcall has no host→guest interrupt: the agent
   must call `STREAM_READ`, and it is told `ENOTYET` when the queue is empty.
   At ~1 ms while active and ~50 ms while idle, the agent wakes the guest 20
   times a second when nothing is happening. That is the price of a
   transport with no device and no driver. It must be measured against
   idle-pause and kept within a 1 % regression budget (see §3.6 and §8.2).

### 1.3 Where the value is

| Need | Today | With the guest service | How it is measured |
|---|---|---|---|
| Clean stop/reboot | Scrape the console or telnet; `iris-ci quit` saves NVRAM but does not flush the guest filesystem | `iris-ci guest-shutdown powerdown --wait 120`; the guest runs `/etc/shutdown`, XFS unmounts, then `Machine::stop` saves NVRAM and release builds exit | Shutdown time, exit status, no XFS repair messages on next boot |
| Load/telemetry | Parse `uptime`, `date`, `ps` from the console; each scrape costs guest CPU and console bytes | `guest-get-load`, `guest-get-time`, `guest-get-osinfo`, `iris-get-status`; one hostcall, no shell, no console echo | Request latency, guest CPU per scrape |
| Fleet policy | The scheduler cannot tell "at getty" from "running a build" without scraping | Host park counters plus guest load plus job markers | False stops in a fleet run |
| Command automation | `iris-ci run/put/get` drive `/bin/sh` over the console, parse prompts and base64 files | `guest-run` argv-only over `STREAM`; stdout/stderr are data frames, not console text | Console bytes per operation, failure rate |
| Disk data without networking | Console `dd`/base64 or NFS/scratch setup | `guest-dd` raw-disk data frames over `STREAM` (v1.1); offline `image-write` when the machine is halted | Throughput, no guest network |
| Coverage the detector misses | A headless guest with `xdm` spinning on an absent framebuffer never parks | Named headless profile runs a local allowlisted action (v1.1); the idle detector then parks normally | Host CPU before/after the profile |
| Quiesce before a snapshot | `sync` typed at the console | `iris-sync-fs`; future fsfreeze-like cooperation | Snapshot validator pass rate |

The headless-`xdm` case is the one real "coverage" win, and it is not a
detector: it is a guest configuration action (`/etc/init.d/xdm stop`) selected
by a host profile, with a local action allowlist. It belongs in v1.1.

### 1.4 What it is not

- **Not portable.** It only runs under IRIS with `hostcall`. On real IRIX or
  another emulator the private syscall is absent; the agent exits. There is no
  serial/TCP/network mode.
- **Not QGA-compatible.** We borrow QGA's control vocabulary and some command
  shapes. The transport is hostcall, the framing is binary, and the available
  commands differ. A stock QGA client will not drive it.
- **Not a pause/resume mechanism.** IRIX has no suspend that maps to QGA's
  `guest-suspend-*`; `machine-stop` restarts peripherals and panics a live
  kernel. A host-side pause, if wanted later, is an IRIS feature that parks
  the CPU in place like idle-pause, not a guest command.
- **Not fsfreeze.** No supported IRIX 6.5/XFS freeze interface has been
  confirmed. `iris-sync-fs` calls `sync()` twice and is weaker.
- **Not an unauthenticated root service.** `guest-run`, `guest-shell` and raw
  disk are opt-in and gated; the default agent only reports and shuts down.

---

## 2. Why hostcall only

### 2.1 The constraint

The guest service is an **IRIS feature**, not an IRIX feature. Its value is
that IRIS already has a private, low-latency channel into user-mode IRIX
programs. Using it avoids a new device, a kernel driver, a guest network
configuration, a serial port and a second control plane. The cost is that the
service is not portable, and the plan accepts that explicitly.

`hostcall` is a Cargo feature:

```toml
hostcall = ["dep:iris-hostcall"]
hostgl = ["hostcall", "dep:iris-hostgl"]
```

`src/lib.rs` publishes `HOSTCALL: bool = cfg!(feature = "hostcall")`. The
standard release feature set includes `hostgl`, which implies `hostcall`, but
a plain `cargo build` does not. A fleet that wants the guest service must
build with `hostcall` (or `hostgl`).

**When it is absent:**

- At config load/startup, `[guest] enabled = true` is turned off with:
  `iris: [guest] enabled requires a build with --features hostcall; guest service disabled`.
  This is a disable, not an error that stops the VM and not a fallback.
- `--print-instance` prints `guest: disabled (no hostcall)`.
- `iris-ci guest-status` returns
  `{enabled:false, available:false, reason:"hostcall feature not built"}`.
- The guest agent's first `STREAM_INFO` call receives `EINVAL`; it logs
  `iris_guestd: hostcall STREAM unavailable (not IRIS, hostcall not built, or service disabled)`
  and exits 1. The init script does not respawn it.

### 2.2 How hostcall works

`iris-hostcall` answers an ordinary IRIX `syscall` before the kernel raises an
exception. Numbers 3000–3009 are IRIS's; 3000 is GL, 3009 is the self-test,
and **3001 is `STREAM`**. The calling convention is IRIX's, plus one outcome:

| Outcome | `a3` | `v0` | `v1` |
|---|---|---|---|
| success | 0 | result | second result |
| error | 1 | errno | — |
| need page | 1 | `ENEEDPAGE` (0x3000) | page address, bit 0 = write |
| not yet | 1 | `ENOTYET` (0x3001) | — |

Callers use the **indirect** form: `v0 = 1000` (IRIX `syscall`), host-call
number in `$4`, seven arguments in `$5`..`$11`. On real IRIX, an indirect
unknown syscall fails with `EINVAL` without raising `SIGSYS`; that is how the
agent detects "not IRIS" without a signal handler. The direct form is also
answered by IRIS but raises `SIGSYS` on a real kernel, so the agent does not
use it.

IRIS sees physical memory and the TLB, not IRIX's page tables. If a buffer
page is missing, invalid or not dirty for a write, the call answers
`ENEEDPAGE`; the caller touches the page and retries the same call. The
framework keeps per-call progress (`InFlight`), so a multi-page copy
completes across retries without duplicating work.

`ENOTYET` is the polling primitive: a service returns it when it has nothing
for the caller yet. The caller yields and calls again. It is not an error the
agent logs.

### 2.3 Rejected transports

These are documented only to close the question. **None is implemented.**

| Transport | Why not |
|---|---|
| SCC channel A / `/dev/ttyd2` serial | Needs a tty daemon, termios, getty/ownership rules, baud pacing and a host serial backend; it would also imply the agent works on real IRIX, which it does not. |
| TCP port forward through the NAT | Needs guest networking, a guest listener, a host forward and authentication; unavailable before networking and a second control plane. |
| NFS `/shared` or the scratch SCSI volume | Payload paths, not request/response: no interrupt, guest polling, mount dependencies, and scratch writes race the guest page cache. Keep them for delivering the tardist and collecting logs. |
| New emulated device (virtio-serial-like) | Needs an IRIX DDI/DKI driver and a new PCI/GIO device; weeks of driver work for a channel hostcall already provides. |
| Hostcall `STREAM` | **Selected.** No device, no driver, no network, no tty; in-memory and already part of IRIS. |

The QGA/virtio-serial model is not applicable to IRIX 6.5 for the same reason:
there is no virtio driver. QGA is retained as **protocol research**, not as a
transport design.

### 2.4 Comparison with other guest agents

| Agent | Transport | Guest component | Why IRIS does not copy it |
|---|---|---|---|
| QEMU GA | virtio-serial (default), isa-serial, unix socket, vsock | `qemu-ga` | No virtio/vsock driver in IRIX 6.5 |
| VMware Tools | VMware backdoor channel | `vmtoolsd` and plugins | No VMware backdoor device |
| Hyper-V integration services | Hyper-V VMBus | integration services (`vmicheartbeat`, `vmicshutdown`, …) | No VMBus driver |
| VirtualBox Guest Additions | VBoxGuest device / HGCM | `VBoxService` | No VBoxGuest driver |
| SPICE | virtio-serial `com.redhat.spice.0` + `spicevmc` | `spice-vdagent` | No virtio-serial driver |
| **IRIS guest service** | **hostcall `STREAM` (3001)** | **`iris_guestd`** | **No device or driver; IRIS-specific by design** |

The common pattern is a paravirtual channel plus a guest driver. IRIS's
hostcall is its paravirtual channel; the guest side is a user-space daemon, not
a driver.

**QGA assumptions that do not hold on IRIX 6.5** (research retained, transport
rejected):

| QGA assumption | IRIX 6.5 reality |
|---|---|
| `/dev/virtio-ports/org.qemu.guest_agent.0` exists | No virtio-serial driver or device node |
| virtio-serial, isa-serial or vsock transport | None available under IRIS/IRIX |
| `systemd`/`pm-utils`/`sysfs` for suspend | No IRIX suspend mechanism that maps to QGA |
| Linux `fsfreeze` ioctls | No confirmed IRIX 6.5/XFS freeze interface |
| `/sbin/poweroff`, `/sbin/halt`, `/sbin/reboot` | IRIX uses `/etc/shutdown` and `uadmin` |
| `getloadavg()` always present | Must verify; `sysmp(MP_SAGET, …)` may be the fallback |
| A systemd/Windows service manager | IRIX uses `/etc/init.d` + `rc2.d`/`rc0.d` |
| File I/O over a byte-stream agent channel | Raw disk is the useful case; general files are out of scope |

**QGA command disposition** (names kept where the shape is useful):

| QGA command | Disposition |
|---|---|
| `guest-ping`, `guest-info` | v1, QGA-shaped |
| `guest-get-time`, `guest-get-load`, `guest-get-osinfo`, `guest-get-host-name` | v1, QGA-shaped |
| `guest-shutdown` | v1, `powerdown`/`reboot`; `halt` rejected |
| `guest-sync`/`guest-sync-delimited` | Not needed: frames carry ids; `STREAM_RESET` resyncs |
| `guest-exec` | Replaced by `guest-run` (argv-only) and `guest-shell` (explicit), gated |
| `guest-file-*` | Excluded; raw disk uses `guest-dd`, bulk provisioning uses `image-write` |
| `guest-fsfreeze-*`, `guest-fstrim` | Excluded; `iris-sync-fs` is the honest subset |
| `guest-suspend-*` | Excluded; no IRIX suspend |
| `guest-set-time` | Excluded; RTC/time setting is risky under emulation |
| vCPU/memory hotplug, `guest-get-cpustats`, `guest-get-devices` | Excluded; no hotplug and not needed |
| `guest-network-get-interfaces`, `guest-get-fsinfo`, `guest-get-disks`, `guest-get-users`, `guest-ssh-*` | v1.1 or later; inventory/security surface |

### 2.5 Feature gating, not fallback

The only supported transport is hostcall. There is no negotiation to a serial
or TCP path. The failure path is always:

1. Host build has no `hostcall`: `[guest]` is disabled at config/startup; the
   agent exits; no control channel exists.
2. Host build has `hostcall` but `[guest] enabled = false`: `STREAM` is not
   registered; the agent gets `EINVAL` and exits.
3. Host build has `hostcall` and `[guest] enabled = true`, but the guest has
   no agent: the service is registered, `guest-status` reports
   "service registered, agent not responding"; there is no fallback.
4. Guest is real IRIX or another emulator: `EINVAL`; the agent exits.

A human can still use the pre-existing console/CI `run` path to recover an
instance, but that is an operational action, not a fallback transport of the
guest service.

---

## 3. The `STREAM` hostcall service

### 3.1 Service number and operations

`iris-hostcall` gets:

```rust
pub const STREAM: u32 = 3001;
```

`STREAM` is one hostcall number with four operations. Arguments are `$5`..`$8`
after the number:

| Op | Value | Arguments | Returns |
|---|---|---|---|
| `INFO` | 0 | — | `(protocol_version, capability_bits)` |
| `READ` | 1 | `guest_addr`, `max_len` | `(n, 0)`, or `ENOTYET` if the queue is empty |
| `WRITE` | 2 | `guest_addr`, `len` | `(accepted, 0)`; `accepted` may be less than `len` |
| `RESET` | 3 | — | `(0, 0)`; clears both queues |

Rules:

- `max_len` and `len` are capped at 65,536 bytes. A larger value is
  `EINVAL`. Zero is allowed and returns zero.
- `READ` when the queue is empty returns `Reply::Err(ENOTYET)`. It does not
  block; the CPU thread must not sleep in a hostcall.
- `WRITE` never blocks. When the guest→host queue is near its cap, it accepts
  fewer bytes; the agent retries the remainder. Back-pressure is a short
  write, not an error.
- `RESET` clears both queues. The host calls it after a machine
  start/stop, snapshot restore/rollback, and when an agent session changes.
- `INFO` returns the protocol version in `v0` and capability bits in `v1`:
  bit 0 control, bit 1 run, bit 2 shell, bit 3 raw disk, bit 4 sync. The
  guest's own `allow_*` configuration can clear bits; the host still gates
  independently.

### 3.2 Queues, framing and back-pressure

`STREAM` is a pair of byte queues in host memory:

```text
host CI / policy ──write──> to_guest   ──STREAM_READ──>  iris_guestd
host CI / policy <──read──  from_guest <──STREAM_WRITE── iris_guestd
```

Caps: 1 MiB `to_guest`, 4 MiB `from_guest`. Both are bounded; a producer that
outruns the consumer gets short writes or a full-queue wait on the host side,
never unbounded memory. `from_guest` has a data condvar (`STREAM_WRITE`
notifies the reader); `to_guest` has a space condvar (`STREAM_READ` notifies a
waiting host writer after it drains).

Frames are length-prefixed and big-endian:

```text
offset  size  field
0       4     magic = 0x49525347 ("IRSG")
4       1     version = 1
5       1     kind: 1 CONTROL, 2 DATA, 3 END, 4 ERROR, 5 RESET
6       2     flags (bit 0 stdout, bit 1 stderr, bit 2 stdin, rest reserved)
8       4     id (request id; 0 for RESET/unsolicited)
12      4     payload length (0..65520)
16      N     payload
```

- **CONTROL** payload is a UTF-8 JSON object, QGA-shaped:
  request `{"execute":"guest-ping","arguments":{}}`, response
  `{"return":{...}}` or `{"error":{"class":"...","desc":"..."}}`.
  For example `{"execute":"guest-run","arguments":{"argv":["/bin/ls","-l"]}}`.
  The id lives in the frame header; the JSON does not repeat it. A response
  is a CONTROL frame carrying `{"return": ...}` or `{"error": {...}}`.
- **DATA** payload is raw bytes for the current command. `flags` selects
  stdout, stderr, stdin or a disk stream.
- **END** payload is JSON: `{"status":0,"signal":0}` or a disk byte count.
- **ERROR** payload is JSON: `{"class":"...","desc":"..."}`.
- **RESET** has no payload; both sides discard partial frames and per-request
  state.

Maximum frame is 64 KiB including the header, so a 32–60 KiB data chunk fits.
The agent uses a 64 KiB receive ring and a 64 KiB send buffer; the host
reader uses the same limits. A bad magic starts a scan for the next magic; a
length over the maximum resets the parser and sends `ERROR`.

### 3.3 Page faults and need-page retries

The service must not duplicate data when the framework answers `ENEEDPAGE`.

**READ (host→guest):**

1. Lock `to_guest`; copy up to `max_len` bytes from the front into a local
   buffer, but do not drain.
2. `mem.write(guest_addr, &buf)` while the queue lock is held. Hostcall
   service code must not call back into `STREAM`, so this cannot deadlock; the
   host writer may wait briefly.
3. If it faults, return `Reply::NeedPage`; the queue is untouched. The guest
   touches the page and retries the same call; the framework skips pages
   already written, and the bytes are identical.
4. On success, drain exactly the bytes copied and unlock. A concurrent
   `RESET` cannot interleave because it needs the same lock, so no generation
   check is needed.

**WRITE (guest→host):**

1. Record the queue's `reset_gen`.
2. `mem.read(guest_addr, &mut buf)` into a local buffer, with no queue lock
   held. If it faults, return `NeedPage`; nothing has been appended.
3. Lock `from_guest`; if `reset_gen` changed, drop the bytes and return
   `ENOTYET`/`EAGAIN`. Otherwise append as much as fits and return the
   accepted count.
4. The agent retries the remainder (or the whole call after a reset).

This is why `STREAM` needs no service-side "pending read" buffer and no
address-space key: the queue is only mutated after guest memory succeeds, and
the READ lock is held across the memory write.

### 3.4 The guest-side stub

The agent is o32 C. A small assembly stub performs the indirect syscall and
returns `v0`, `v1` and `a3` to C:

```c
struct hc_reply { long v0, v1, a3; };
long iris_hc(long op, long a1, long a2, long a3, struct hc_reply *out);
```

The stub sets `$4 = 3001`, `$5 = op`, `$6 = a1`, `$7 = a2`, `$8 = a3`,
`$2 = 1000`, executes `syscall`, and stores `$2/$3/$7` into `out`. The C
wrapper:

```c
for (tries = 0; tries < 64; tries++) {
    iris_hc(op, a1, a2, a3, &r);
    if (r.a3 == 0) return r.v0;                 /* success */
    if (r.v0 == ENEEDPAGE) {                    /* 0x3000 */
        volatile unsigned char *p = (unsigned char *)(r.v1 & ~1UL);
        if (r.v1 & 1) *p = *p; else (void)*p;   /* touch, then retry */
        continue;
    }
    if (r.v0 == ENOTYET) { errno = ENOTYET; return -1; } /* 0x3001 */
    errno = (int)r.v0;
    return -1;
}
errno = EFAULT;
return -1;
```

The exact assembly is verified on the guest; the important properties are the
indirect form, the register shift, `ENEEDPAGE` retry, and `ENOTYET` mapping.
The direct form is not used because it raises `SIGSYS` on a real kernel.
`INFO` reads the capability bits from `out.v1`; the other operations use
`v0`.

Detection: the agent calls `INFO` once at startup.

- Success: print `iris_guestd: STREAM protocol N, capabilities 0x...`.
- `EINVAL` (22): `iris_guestd: hostcall STREAM unavailable (not IRIS, hostcall not built, or service disabled)`; exit 1.
- Any other error: log and exit 1.
- `ENOTYET` is impossible for `INFO`; treat it as a retry and then fail.

No serial/TCP probe, no `/dev/ttyd2`, no network attempt.

### 3.5 Host-side `GuestStream`

`src/guest_stream.rs` owns the host client and a **reader thread** per
instance:

```rust
pub struct GuestStream {
    shared: Arc<StreamShared>,       // from iris-hostcall::stream
    reader: JoinHandle<()>,
    pending: Mutex<HashMap<u32, Sender<Frame>>>,
    next_id: AtomicU32,
}

impl GuestStream {
    pub fn request(&self, cmd: &str, args: Value, timeout: Duration)
        -> Result<Value, GuestError>;
    pub fn run(&self, argv: &[String], stdout: &Path, stderr: &Path,
               timeout: Duration) -> Result<RunStatus, GuestError>;
    pub fn disk_read(&self, path: &str, offset: u64, length: u64,
                     to: &Path, timeout: Duration) -> Result<u64, GuestError>;
    pub fn disk_write(&self, path: &str, offset: u64, from: &Path,
                      timeout: Duration) -> Result<u64, GuestError>;
    pub fn status(&self) -> GuestStatus;
    pub fn reset(&self);
}
```

- The reader thread waits on `from_guest` (condvar), parses frames, and routes
  CONTROL/END/ERROR replies to the waiting request by id; DATA frames go to
  the session's file writer or a bounded channel. It never calls up into
  `Machine`.
- DATA is written to the session's host file as it arrives (or into a bounded
  queue), so a fast child cannot grow host memory without bound. A full file
  or channel applies back-pressure to the guest's `STREAM_WRITE` short write.
- Control requests are independent of data sessions, so a long `guest-run`
  does not block `guest-ping` or `guest-load`. The agent must multiplex them
  (see §4.4).
- A timeout removes the pending sender; late frames with an unknown id are
  discarded. `reset()` clears queues, pushes one RESET frame so the agent
  discards any partial frame, and drops all pending requests with
  `GuestReset`.
- The host writes frames with `shared.push_to_guest(frame_bytes)`; the
  `STREAM_READ` service drains them into guest memory. The reader thread only
  reads what `STREAM_WRITE` has already copied out of guest memory.
- The reader thread is parked on a condvar when there is no data; it costs no
  host CPU. It is stopped in `Machine`'s drop path.

### 3.6 Polling and idle-pause

Hostcall has no host→guest doorbell. The agent polls:

| State | Poll interval | Why |
|---|---|---|
| Active (a command was handled within the last 2 s) | 1 ms | Low command latency and data throughput |
| Idle | 50 ms | 20 wakeups/s; bounded idle cost |
| Long idle (optional backoff) | 250 ms after 60 s with no command | Only if measurement shows the 50 ms poll costs >1 % host CPU |

The agent sleeps in `nanosleep` between polls; it does not spin. The guest
wakes from idle-pause for the syscall, executes a few dozen instructions,
gets `ENOTYET`, and sleeps again. The idle detector sees a short user-mode
burst and should re-park; this must be measured, because it is exactly the
kind of user-mode activity that can defeat the park window.

Measurement plan:

- Run a settled getty with the agent disabled: baseline 2 % host CPU.
- Enable the agent, no commands, 1 ms / 50 ms / 250 ms poll: 60 s each,
  three runs, `pidstat -p <iris-pid> 1 60` or `/proc/<pid>/stat` deltas.
- Record `iris-ci idle-status` park count/sleep and guest `ps -l` agent CPU
  time.
- Acceptance: agent idle host CPU ≤ baseline + 1 %. If 50 ms fails, the
  default moves to 250 ms and command latency budget becomes 500 ms. Do not
  hide the cost by disabling idle-pause.

### 3.7 Security of the channel

- `STREAM` is a **guest-wide** syscall. Any user process in the guest can call
  it, read the host→guest queue, or inject frames. There is no per-process
  credential in the hostcall ABI (the page-access ASID is not a PID and
  changes with address space). The host cannot authenticate "the agent".
- The service is in-process and local; there is no network listener. The CI
  socket is the host-side authority.
- The command gates (`allow_run`, `allow_shell`, `allow_disk`) are the real
  security boundary. A malicious guest process can spoof telemetry or steal a
  command; it cannot make the root agent execute a command unless the agent
  reads it. A malicious root in the guest owns the guest anyway.
- Enable `allow_run`/`allow_shell`/`allow_disk` only on trusted fleet images.
  The default is all off except lifecycle and telemetry.

---

## 4. The guest agent

### 4.1 Language, ABI and build

**C89, o32, MIPSpro.** No tty, no termios, no networking, no threads.

- o32 is the ABI that also exists on 5.3, and `-mips2` runs on every CPU IRIS
  emulates. n32 buys nothing for a single-threaded byte pump. Build
  `-o32 -mips2`; 5.3 is not tested.
- Dynamic libc is fine; the daemon starts from `rc2.d` after `/usr` is
  mounted. No `libpthread`, no `libsocket`, no `libnsl`.
- C89 style: declarations at the top of a block, no `<stdint.h>`, use
  `<sys/types.h>` and `unsigned long long`. MIPSpro's `printf` may not accept
  `%lld`; verify on the guest and format 64-bit values with a helper if
  needed.
- Use one page-aligned 64 KiB receive buffer and one 64 KiB send buffer
  (`memalign(4096, 65536)` or a static aligned array). Fewer pages touched
  means fewer `ENEEDPAGE` retries.

Build command in the guest:

```sh
cc -o32 -mips2 -O2 -Wall -fullwarn -o iris_guestd \
   iris_guestd.c json.c hostcall.s
strip iris_guestd
```

**Why C, not shell:** a shell agent would fork `date`/`uptime` per request,
cannot parse binary frames reliably, and has no way to call hostcall without
an extra compiled helper. The C daemon blocks in `nanosleep`, parses frames
and calls hostcall directly.

### 4.2 Process model

```
/etc/init.d/iris_guestd start
  -> /usr/sbin/iris_guestd -d
       STREAM_INFO; if EINVAL: log and exit 1
       STREAM_RESET; clear stale host queues and start a fresh parser
       write /var/run/iris_guestd.pid under an fcntl write lock
       loop:
         read frames until STREAM_READ returns ENOTYET
         dispatch control / continue child sessions / service disk sessions
         if no active work: nanosleep(50 ms)
         else: nanosleep(1 ms) or select() on child pipes with 1 ms timeout
```

CLI: `-d` daemonise, `-f` foreground, `-v` verbose, `-t` test `STREAM_INFO`
and exit, `-c FILE` config (default `/etc/iris_guestd.conf`). The init script
uses `-d`; the rig uses `-f -v` and `-t`.

Guest config (`/etc/iris_guestd.conf`, `0600 root:sys`):

```ini
allow_shutdown = 1     # lifecycle; the default
allow_run = 0          # guest-run, argv-only, root
allow_shell = 0        # guest-shell, /bin/sh -c, root
allow_disk = 0         # guest-dd raw disk
allow_profile = 0      # named headless profile
poll_active_ms = 1     # after a command
poll_idle_ms = 50      # steady state
poll_long_ms = 250     # optional backoff after 60 s idle
run_timeout_ms = 0     # 0 = no timeout; the host supplies one
```

Rules that matter for host CPU:

- **No periodic timer beyond the poll.** The poll interval is the only timer.
- **No subprocess per control request.** `getloadavg`/`sysmp`,
  `gettimeofday`, `uname` and `gethostname` are direct calls.
- **No logging per request.** Log startup, shutdown, errors and, with `-v`,
  command names. A log line per ping is guest disk I/O.
- **One process.** A second instance fails the `fcntl(F_SETLK)` on the
  pidfile and exits 1.
- **Children only for `guest-run`/`guest-shell`.** `guest-dd` and control
  never fork.

Signals:

- `SIGTERM`: set `stop`, kill/reap any child, remove the pidfile, exit 0.
- `SIGHUP`: reset the frame parser and per-request state; keep running.
- `SIGPIPE`: ignore; a closed child pipe returns `EPIPE`.
- `SIGCHLD`: reap; a child exit marks its session END.

### 4.3 Control commands

The control payload is QGA-shaped JSON. The frame id matches request and
response.

| Command | Arguments | Result | Notes |
|---|---|---|---|
| `guest-info` | — | `{version, protocol, transport:"hostcall", caps, pid, started, gates, poll_ms}` | capability and gate discovery |
| `guest-ping` | — | `{version, protocol, latency?}` | liveness; one round trip |
| `guest-get-time` | — | int nanoseconds since epoch | QGA shape |
| `guest-get-load` | — | `{load1m, load5m, load15m}` | if the IRIX API exists |
| `guest-get-osinfo` | — | QGA `GuestOSInfo` subset | `id`, `name`, `pretty-name`, `version`, `version-id`, `kernel-release`, `kernel-version`, `machine` |
| `guest-get-host-name` | — | `{host-name}` | `gethostname` |
| `guest-shutdown` | `mode` = `powerdown` (default), `reboot` | `null` before acting | `halt` rejected in v1 |
| `iris-get-status` | — | `{agent, load, os, config}` | compact status |
| `iris-sync-fs` | — | `{synced:true}` | `sync(); sync();` |
| `iris-set-profile` | `profile` | `{applied:true}` | v1.1; named local action, see §4.6 |

`guest-info` reports the local gates (`allow_run`, `allow_shell`,
`allow_disk`, `allow_shutdown`) and the capability bits from `STREAM_INFO`.
The host requires protocol 1; an unknown protocol disables the commands and
logs a version-skew error.

`guest-shutdown` writes and flushes the response frame, waits 200 ms for the
host reader to consume it, then
`fork`/`execl("/etc/shutdown", "shutdown", "-y", "-i0", "-g0", NULL)` for
powerdown or `-i6` for reboot. The host treats an ack or a clean link close
as accepted; `--wait` verifies the machine stopped. `reboot(2)` is not used;
`/etc/shutdown` is the documented path.

### 4.4 Command pipe: `guest-run` and `guest-shell`

These are the data-plane commands. They are **opt-in**:

- Host: `[guest] allow_run = false` default.
- Guest: `/etc/iris_guestd.conf` `allow_run = 0` default.
- Both must allow; either one false returns `PermissionDenied`.
- `guest-shell` additionally requires `allow_shell = true` in both. It is a
  separate switch because `/bin/sh -c` is arbitrary code by definition.

`guest-run` is **argv-only**:

```json
{"execute":"guest-run","arguments":{
    "argv":["/bin/ls","-l","/tmp"],
    "stdout":"capture",
    "stderr":"capture"
}}
```

- No shell interpolation, no globbing, no redirection. The agent calls
  `execv(argv[0], argv)` directly.
- `argv` is a JSON array of strings; the C parser supports arrays of strings
  and integers (the only place it needs arrays).
- The agent returns `{pid}` immediately, then streams child stdout/stderr as
  DATA frames with the same id (flags select stdout/stderr). On exit it sends
  END with `{status, signal}`.
- Child stdout/stderr are non-blocking. If the host queue is full, the agent
  stops reading the child until it drains; it never buffers a command's
  output without bound. A child blocked on a full pipe is normal
  back-pressure, not a hang.
- `argv[0]` is a path passed to `execv`, not a `PATH` lookup. Use an absolute
  path or a path the daemon can resolve; no shell, no globbing, no
  redirection.
- v1 captures stdout/stderr; stdin is v1.1. A child that reads stdin sees
  `/dev/null`.
- Environment is inherited from the daemon; no per-request env in v1.
- `guest-shell` takes `{"script":"..."}` and runs
  `/bin/sh -c script`; it is explicitly a shell, never the default path.

The agent multiplexes: while a child runs, its main loop uses `select()` on
the child's stdout/stderr with a 1 ms timeout, calls `STREAM_READ` for host
frames (including a future cancel/stdin), and writes DATA frames. Control
commands such as `guest-ping` still work during a run because the loop
services them between polls.

Security: `guest-run` is root code execution in the guest. It is for trusted
fleet automation, not for untrusted tenants. `allow_run` is off by default
and documented as equivalent to handing the CI socket root in the guest.

### 4.5 Raw-disk data: `guest-dd` (v1.1)

`guest-dd` moves raw disk bytes between the guest and a host file over
`STREAM` data frames. It is the live-guest path; the offline host image write
is faster when the machine can be stopped.

Control:

```json
{"execute":"guest-dd","arguments":{
    "direction":"read",              // read = guest disk -> host file
    "path":"/dev/rdsk/dks0d2s0",
    "offset":0,
    "length":1048576,
    "bs":65536
}}
```

- The agent opens the path `O_RDONLY`/`O_WRONLY` (no shell, no `dd` process),
  seeks to `offset`, and loops reading/writing `bs` bytes.
- `bs` and `offset` must be multiples of 512; IRIX raw disk returns I/O errors
  on non-512 reads (the scratch-volume gotcha in `STORAGE.md`). `bs` is
  capped at 64 KiB by the frame size.
- `read`: the agent sends DATA frames to the host; the CI server writes them
  to the requested host file. `write`: the host sends DATA frames; the agent
  writes them to the device.
- The path allowlist is `/dev/rdsk/` only. `allow_disk` must be true on both
  sides. Writing sector 0/volume header is the caller's risk; the plan
  recommends a separate allowlist for whole-disk paths and documents the VH
  layout (`src/sgi_vh.rs`, `STORAGE.md`).
- Raw device I/O bypasses the guest buffer cache. The caller must ensure the
  filesystem is unmounted or the device is not otherwise in use; the agent
  cannot know. This is a data-integrity gate, not a convenience.
- Host paths are validated against the state/cwd sandbox, as the scratch
  commands do; no `..` components and no paths outside the allowed root.
- v1.1 includes a `guest-dd` CLI wrapper with `--from`, `--to`, `--bs`,
  `--count`, `--seek`; it is still frame data, not a guest `dd` process.

### 4.6 Headless profile (v1.1)

`iris-set-profile {"profile":"headless"}` runs a **named local action** from
`/etc/iris_guestd.conf`:

```ini
[profile headless]
action = /etc/init.d/xdm stop
```

- The wire carries only the profile name; the action is a root-owned local
  configuration line. There is no arbitrary argv over the wire.
- `allow_profile` gates it; default off.
- The intended use is the headless-`xdm` case: stop the X login manager that
  spins on an absent framebuffer, then let idle-pause park the kernel loop.
- A profile action is a shell command executed by the agent; treat it as root
  code and keep the config `0600`.

### 4.7 IRIX API checks

Do not trust a web man page. Run these on the target 6.5 guest and record the
result in `clanker/irix/`.

| Need | Intended call | Check on the guest | If absent |
|---|---|---|---|
| Hostcall | indirect `syscall` 1000, number 3001 | Run the package's `iris_guestd -t` (test mode) under IRIS; expect `STREAM_INFO` success. On real IRIX expect `EINVAL`. | Agent exits; host disables |
| Load average | `getloadavg(3)`, else `sysmp(MP_SAGET, <load selector>, ...)` | `man getloadavg \| cat`; `grep -n 'MPSA_\|MP_SAGET' /usr/include/sys/sysmp.h`; link `-lbsd` if needed | Return `Unsupported`; host uses park counters |
| Time | `gettimeofday(2)` | `man gettimeofday \| cat` | Always present |
| OS identity | `uname(2)`, `gethostname(2)` | `uname -a`; `man uname \| cat` | Always present |
| Sleep | `nanosleep(2)` or `select(2)` | `man nanosleep \| cat` | Use `select` with a zero-fd timeout |
| Daemonise | `daemon(3)`, else double-fork | `man daemon \| cat` | Double-fork |
| PID lock | `fcntl(F_SETLK)` | `man fcntl \| cat`; `ls -d /var/run` | `O_EXCL` pidfile only; use `/var/adm` if `/var/run` is absent |
| Shutdown | `/etc/shutdown -y -i0 -g0` | `man shutdown \| cat`; `ls -l /etc/shutdown /sbin/shutdown` | Try `/sbin/shutdown`; else `Unsupported` |
| Reboot | `/etc/shutdown -y -i6 -g0` | `man shutdown \| cat`; `grep '^rb:' /etc/inittab` | Reject `reboot` |
| Init links | `/etc/rc2.d`, `/etc/rc0.d` | `ls /etc/rc2.d \| head`; `man chkconfig \| cat` | Install links explicitly |
| Child exec | `fork`/`execv`/`waitpid`/`select` | `man execv \| cat`; `man select \| cat` | Reject run/shell |
| Raw disk | `open`/`lseek`/`read`/`write` on `/dev/rdsk/...` | `ls -l /dev/rdsk`; read 512 bytes with `dd bs=512 count=1` | Disable `guest-dd` |

No termios, tty or serial checks are needed.

### 4.8 Agent failure handling

| Failure | Agent behaviour |
|---|---|
| `STREAM_INFO` returns `EINVAL` | Log "hostcall STREAM unavailable"; exit 1 |
| `STREAM_READ` returns `ENOTYET` | Sleep; not an error |
| `STREAM_WRITE` short write | Retry the remainder after a 1 ms sleep |
| `ENEEDPAGE` | Touch the page and retry, up to 64 times |
| Malformed frame | Send ERROR, reset parser, continue |
| Oversize frame | Reset parser, send ERROR |
| Unknown command | `CommandNotFound`; keep running |
| Gate false | `PermissionDenied`; no action |
| `fork`/`execv` fails | ERROR/END with errno; keep running |
| Child hangs | `run_timeout_ms` (default 0 = no timeout; CI supplies one); kill child on timeout |
| Host abandons a run | v1 has no cancel; the child continues and the host discards its frames. v1.1 adds cancel. |
| `STREAM` disappears (host restart) | `STREAM_INFO` fails; log and exit 1; init can restart manually |
| Agent killed | No respawn in v1; host reports not responding; manual restart/reboot |
| Parser desync | `RESET` frame clears; `STREAM_RESET` clears host queues |

### 4.9 Security

- **Feature-local.** The service only exists under IRIS with `hostcall`; no
  network listener, no tty, no guest port.
- **Root daemon, gated commands.** Root is needed for `/etc/shutdown`, raw
  disk and `guest-run`. All three are opt-in. The default command set is
  telemetry, identity, sync and shutdown.
- **Guest-wide hostcall.** Any guest process can call `STREAM`; the channel is
  not authenticated per process. The gates are configured on the host and in
  the guest; do not enable `allow_run`/`allow_shell`/`allow_disk` on a guest
  with untrusted users.
- **Host side.** The CI socket is the authority; its permissions follow the
  state directory. The fleet launcher should create state directories `0700`.
  Anyone who can talk to the CI socket can already quit the machine; enabling
  `allow_run` widens that to root in the guest.
- **Logging.** Log command names and errors, not payloads. `guest-shell`
  scripts may contain secrets; do not log them.

---

## 5. Host-side IRIS integration

### 5.1 Modules and types

| File | Change |
|---|---|
| `iris-hostcall/src/lib.rs` | `pub const STREAM: u32 = 3001;` and `pub mod stream;` |
| `iris-hostcall/src/stream.rs` (new) | `StreamShared` (queues, caps, condvar, reset generation), `StreamService: Service` (INFO/READ/WRITE/RESET), frame header constants |
| `src/guest_stream.rs` (new) | `GuestStream` host client: reader thread, request routing, `run`, `disk_read`, `disk_write`, `status`, `reset`; frame codec; JSON control |
| `src/machine.rs` | When `cfg.guest.enabled` and `cfg!(feature="hostcall")`: create `StreamShared`, register `StreamService`, create `GuestStream`; store it; `get_guest_stream()`; reset on start/stop/restore; stop the reader on drop |
| `src/workstation.rs` | `get_guest_stream()` on the `Workstation` trait, default `None`; `Machine` returns the instance; O2 keeps `None` |
| `src/ci.rs` | Hold `Option<Arc<GuestStream>>`; add `guest-*`, `guest-run`, `guest-shell`, `guest-dd`, `image-write`, `idle-status` |
| `src/config.rs` | `GuestConfig` (`enabled`, `allow_run`, `allow_shell`, `allow_disk`, `run_timeout_ms`), `[guest]` table, CLI flags, hostcall gating |
| `src/instance.rs` | No change: this plan adds no listener or port to preflight |
| `src/state.rs` | No change in v1; a later guest log is `StatePaths::guest_log()` |
| `src/iris_ci_main.rs` | CLI subcommands for the new CI commands |
| `iris-hostgl` | No change; its threaded service is the model for `ENOTYET` and page borrowing, not a dependency |
| `iris-gui` | Out of scope; later a Debug-tab status line |
| `clanker/iris_mcp.py` | No new tool; `run_command "guest status"` covers the monitor |

All `src/guest_stream.rs` references are behind `#[cfg(feature = "hostcall")]`.
`CiServer`'s field and the `guest-*` handlers are cfg-gated; a build without
hostcall still answers `guest-status` with `available:false` and
`reason:"hostcall feature not built"`, and every other `guest-*` command
returns a clear error. There is no stub transport.

The `GuestStream` reader thread is the bridge between the CPU-thread service
and the CI handlers: `STREAM_WRITE` copies guest bytes into `from_guest`; the
reader thread parses frames and routes them. The CPU thread never waits for
CI, and CI never touches guest memory.

### 5.2 Configuration and CLI

```toml
[guest]
enabled = false          # opt-in; requires a hostcall build
allow_run = false        # guest-run (argv-only, root)
allow_shell = false      # guest-shell (/bin/sh -c, root)
allow_disk = false       # guest-dd raw disks
run_timeout_ms = 0       # host-side default; 0 = no timeout
```

CLI: `--guest`, `--guest-allow-run`, `--guest-allow-shell`,
`--guest-allow-disk`.

Validation:

- `enabled = false` (default): behaviour byte-identical to today.
- `enabled = true` and `!cfg!(feature = "hostcall")`: log the clear message
  and set `enabled = false`. Do not fail the VM and do not select another
  transport.
- `enabled = true` on the O2 (`Ip32Machine`): warn "guest service not
  implemented on this profile" and leave it off; v1 targets Indy/Indigo2.
- `allow_shell = true` without `allow_run = true`: reject; shell implies run.

The agent's poll intervals live in `/etc/iris_guestd.conf`, not the host
config, because the agent owns its loop. `guest-info` reports the actual
values. The host only records them in `guest-status`.

`--print-instance`:

```
instance: 3
state_dir: /home/me/iris-fleet/iris-instance-3
monitor: 9030
ci_socket: /home/me/iris-fleet/iris-instance-3/iris.sock
guest: hostcall (STREAM 3001)
```

If the feature is absent: `guest: disabled (no hostcall)`. There is no port.

No new `StatePaths` accessor and no new `derive_instance` value in v1. The
transport is in-process. If a guest log is added later, it is
`StatePaths::guest_log()` -> `<state_dir>/iris-guest.log`; do not invent a
path at the call site. If a TCP or serial transport is ever added (it is not
in this plan), its port/path must go through `derive_instance`.

One `Machine` per process means the `StreamShared`, `GuestStream` and reader
thread are fields of `Machine`, not process globals. The `iris-hostcall`
registry itself is process-global by design; with one `Machine` per process
the registered `STREAM` service is effectively per-instance. A fleet is N
processes, each with its own STREAM queues and agent session.

### 5.3 CI surface

New CI commands, all over the existing newline-delimited JSON socket:

| Command | Arguments | Returns |
|---|---|---|
| `guest-status` | — | `{enabled, hostcall_built, service_registered, agent_responding, version, protocol, caps, gates, poll_ms, last_error}` |
| `guest-ping` | `timeout_ms?` | `{version, protocol, latency_ms}` |
| `guest-info` | `timeout_ms?` | agent `guest-info` |
| `guest-load` | `timeout_ms?` | `{supported, load1m, load5m, load15m}` |
| `guest-time` | `timeout_ms?` | `{nanoseconds, seconds}` |
| `guest-shutdown` | `mode`, `wait_ms?` | `{accepted, stopped, elapsed_ms}` |
| `guest-run` | `argv[]`, `stdout_path?`, `stderr_path?`, `timeout_ms?` | `{status, signal, stdout_bytes, stderr_bytes}`; output written to host files |
| `guest-shell` | `script`, `stdout_path?`, `stderr_path?`, `timeout_ms?` | same, gated by `allow_shell` |
| `guest-dd` | `direction`, `device`, `offset`, `length`, `bs`, `host_path` | `{bytes, elapsed_ms}` (v1.1) |
| `image-write` | `image`, `host_path`, `offset`, `length` | `{bytes}` (v1.1; halted only) |
| `idle-status` | — | park counters from `idle_park.rs` |

`guest-run`/`guest-shell` write stdout/stderr to host files supplied by the
caller; if a path is omitted, the CI server uses a unique file under the
platform temporary directory and returns the path. Nothing is base64 over the
CI socket, and no new per-instance state directory is introduced. `guest-dd`
and `image-write` also take explicit host paths. This keeps the CI protocol
line-oriented and avoids a second data socket.

`image-write` is the **offline provisioning path**: it writes a raw image file
directly on the host when the machine is halted (or before the instance is
started), with no guest agent, no hostcall and no guest CPU. It is faster than
`guest-dd`; it must refuse a live machine, must only run when the SCSI queue is
idle, and must respect the disk advisory lock / COW ownership. The image path
must match a configured SCSI image; it is not an arbitrary host path. CHD is
out of scope for v1.1.

`idle-status` reads the existing public statics (`PARK_COUNT`,
`PARK_SLEEP_NS`, `PARK_WAKE_IRQ`, `PARK_WAKE_OTHER`, `UPDATES`, `RESET_*`,
`WINDOW_*`) and returns numbers. It does not touch the CPU lock or stop the
machine.

`guest-shutdown` semantics:

- Send the control frame; wait for the ack (default 2 s).
- `wait_ms = 0`: return `{accepted:true}` after the ack; the caller polls
  `iris-ci status`.
- `wait_ms > 0`: poll `Machine::status` until halted or timeout; return
  `{accepted, stopped, elapsed_ms}`.
- If the link closes before the ack, treat it as accepted only if the frame
  was written; log the ambiguity.
- Never call `machine-stop`, `idle off`, or `SIGKILL` from this handler.

### 5.4 Monitor surface

The monitor is line-oriented, so it exposes control and telemetry, not binary
data:

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

`guest status` prints configured/available/responding, agent version and
protocol, gates, poll intervals and the last error. It must never block for
more than the request timeout. `guest run`/`guest dd` are CI-only because
their output is binary.

### 5.5 Lifecycle policy

The policy is a reference script (`scripts/fleet-policy.sh` or a small Python
tool), not a Rust subsystem. Inputs:

1. Host park state from `iris-ci idle-status`: `park_count`/`sleep_ns`
   increasing over two samples.
2. Guest load from `iris-ci guest-load`: `load1m < 0.1` means no runnable work
   on one emulated CPU. `Unsupported` means unknown, not idle.
3. Job state from the fleet launcher: its own job record (for example a
   `busy` marker in the launcher's work directory), not an iris state file.
   The policy must never stop an instance that a job owns.

Reference rule:

```
every 30 s, for each instance:
    if the launcher's job record says busy:     continue
    if a job has owned the instance in 15 min:   continue
    parked = idle-status.park_count increased over the last 5 min
    load   = guest-load (if supported)
    if parked and (load unknown or load.load1m < 0.1):
        iris-ci guest-shutdown --mode powerdown --wait 120
        if not stopped after 120 s: log; leave it running (no SIGKILL)
```

Actions and non-actions:

| Action | v1? | Mechanism |
|---|---|---|
| Park | Already automatic | `src/cpu/idle_park.rs`; the policy only observes, never toggles `idle` |
| Pause | No | Needs an in-place host park API; `machine-stop` panics the kernel |
| Snapshot | Optional, off by default | `iris-sync-fs`, then `iris-ci save`, then `quit`; validate with `iris-ci validate`; only on COW/CHD overlays |
| Stop | Yes | `guest-shutdown powerdown`; the guest flushes and unmounts, then `Machine::stop` saves NVRAM |
| Force stop | Manual only | `iris-ci quit` does not flush the guest filesystem; document the XFS risk |

Why stop rather than park or snapshot:

| Policy | Host CPU | Host RAM | Reuse latency | Risk |
|---|---|---|---|---|
| Keep running, idle-pause | ~2 % | Full guest RAM | Immediate | None |
| Park (automatic) | ~2 % | Full guest RAM | Immediate | None |
| Pause in place | ~0 (device threads remain) | Full guest RAM | Immediate | Not implemented; needs a host park API |
| Snapshot + quit | 0 | Snapshot storage | ~145 ms restore | Snapshot consistency/fidelity must be validated |
| Powerdown | 0 | 0 | ~42 s cold boot | None if the guest shuts down cleanly |

For a fleet that reuses an instance within minutes, pause or snapshot wins;
for one idle for hours, powerdown wins on RAM and thread count. v1 implements
powerdown because it has no new fidelity risk; snapshot+quit is the optional
warm-pool path once `iris-ci validate` passes on the image.

**No double-parking.** The policy never calls `idle off`/`idle on`. A guest
command wakes the CPU, the agent polls and answers, and the CPU re-parks. The
command costs one wake plus the poll, not a second parking mechanism.

**No hanging.** Every request has a hard timeout. `guest-shutdown` returns
after the ack by default; `--wait` is bounded. CI handlers use the reader
thread and do not hold the machine-slot lock while waiting. A timed-out
request is dropped from the pending map; late frames are discarded. A dead
agent leaves the instance running and logs.

**No fallback transport.** If `guest-status` says not responding, the policy
does not try a serial/TCP path. It leaves the instance running, or a human
uses the existing console. `[guest]` disabled means no control channel.

### 5.6 Concurrency and locks

- `StreamShared` has two queue mutexes and one condvar. The CPU thread's
  `STREAM_READ`/`WRITE` copies bytes under the queue lock; the reader thread
  waits on the condvar. No callback up into `Machine`.
- `GuestStream` has a reader thread, a pending-request map, and a next-id
  atomic. It serialises frame parsing but not requests: control and data have
  independent ids.
- `CiServer` holds `Option<Arc<GuestStream>>` at construction, like the
  existing CI console backend; a `guest-*` handler does not need
  `with_machine` and does not hold the machine slot while waiting.
- The monitor uses the same `GuestStream`; `guest status` has a short timeout.
- On `Machine::stop`/`start`, `stream.reset()` clears queues and pending
  requests. On `ci_restore`/`ci_rollback`, reset and drop sessions alongside
  the existing console reset (`src/machine.rs:1658-1685`).
- The reader thread is stopped in `Machine`'s drop path by `GuestStream::shutdown()`
  (stop flag + condvar notify), even if a CI handler still holds an `Arc`;
  it must not outlive the `StreamShared`.

### 5.7 Snapshot, restore and restart

- `ci_restore`/`ci_rollback`: `GuestStream::reset()`; clear queues; pending
  requests return `GuestReset`. The agent keeps running from the restored
  state; the next `guest-ping` re-syncs.
- Host restart: new `StreamShared`, new `GuestStream`, new service
  registration; the guest agent's next poll gets `INFO` and continues. If the
  agent was mid-command, it sees a new stream and resets its parser.
- Guest agent restart: new pid/start time in `guest-info`; the host clears
  pending sessions; old frames are discarded by id.
- Machine stop: queues cleared; if the guest is halted, no one polls; on
  start, the agent polls again.

### 5.8 Documentation and tests

- `debug.md`: `guest ...` monitor commands, `iris-ci guest-*`,
  `idle-status`, `image-write`.
- `HELP.md`: `[guest]` config, `--guest*`, hostcall feature requirement,
  install and gates.
- `FEATURES.md`: new config keys, CLI flags and the `hostcall` requirement.
- `HACKING.md`: `STREAM` is a hostcall service on 3001; the CPU thread must
  never block; the CI reader thread is the bridge; no serial/TCP fallback.
- `docs/parallel-instances.md`: guest lifecycle section and status.
- `CHANGELOG.md`: under the release that lands it.
- `clanker/INDEX.md`: plan row now; howto/measure notes when numbers exist.
- `clanker/iris_mcp.py`: no new tool; `run_command "guest status"`.
- Tests: `tests/guest_stream.rs` (frame codec, queue caps, short writes,
  `ENOTYET`, `ENEEDPAGE` retry, reset, reader routing); `tests/guest_protocol.rs`
  (JSON control, timeouts, id mismatch); a mock guest thread that calls the
  service through `iris_hostcall::dispatch` with a fake `PageAccess`.
- Rig-only `clanker/testing/guest-service-e2e.md`: 6.5 under IRIS with
  hostcall, no hostcall, real IRIX exit, agent kill/restart, 8-instance fleet.

---

## 6. Packaging

### 6.1 Product layout

| Item | Value |
|---|---|
| Product | `iris_guest` |
| Image | `sw` |
| Subsystem | `iris_guest.sw.base` |
| Version | `100` (1.0.0); protocol version `1` |
| Description | "IRIS guest service for IRIX 6.5 (hostcall only)" |
| Files | `/usr/sbin/iris_guestd`, `/etc/init.d/iris_guestd`, `/etc/iris_guestd.conf`, `/usr/share/iris_guest/README`, `/usr/share/iris_guest/protocol.md` |
| Links | `/etc/rc2.d/S33iris_guestd`, `/etc/rc0.d/K03iris_guestd` -> `../init.d/iris_guestd` |
| Dependencies | libc only; no networking, no threads, no tty |
| Build | native MIPSpro in the guest; `gendist` in the guest; host script drives it |

Repository tree:

```
guest/
  README.md                 # developer notes, not installed
  src/iris_guestd.c
  src/json.c, json.h
  src/hostcall.s            # indirect syscall stub
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

### 6.2 `iris_guest.idb` (sorted on fields 5 and 6)

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

- `config(suggest)` keeps site edits on upgrade and installs the new file as
  `.N`.
- `exitop` starts/restarts only on a live root (`$rbase = /`); in a miniroot
  or `-r` install it does nothing.
- `removeop` calls the init script if present (the script verifies the PID),
  falls back to the pidfile, and removes it.
- No `tag()`; a daemon does not need a filetype rule.

### 6.3 `iris_guest.spec`

```text
product iris_guest
    id "IRIS guest service for IRIX 6.5 (hostcall only)"
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

`VERSION` is substituted from `guest/version`; versions are integers. No
`prereq` in v1: libc only. The package installs on any 6.5, but the agent only
runs under IRIS with hostcall; the README says so in the first line.

### 6.4 Build

In the guest, from a copy in `/tmp` (`/shared` is `noexec`):

```sh
mkdir -p /tmp/iris-guest/root/usr/sbin
cc -o32 -mips2 -O2 -Wall -fullwarn -o /tmp/iris-guest/root/usr/sbin/iris_guestd \
   iris_guestd.c json.c hostcall.s
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
3. Runs the build commands; fails on the first non-zero status.
4. Pulls the tardist back with `tools/irix-sh.py pull` or copies it to
   `/shared`.
5. Records `cc -version`, `versions -n eoe | cat` and the `gendist` output.

Cross-compilation with clang is out of scope: `gendist`/`strip` and the tested
ABI are in the guest; an IRIX sysroot/linker is not in this tree.

### 6.5 Init script

No tty, no getty, no device permissions. The script only starts/stops the
daemon and reports status:

```sh
case "$1" in
start)
    [ -x /usr/sbin/iris_guestd ] || exit 1
    /usr/sbin/iris_guestd -d
    ;;
stop)
    if [ -f /var/run/iris_guestd.pid ]; then
        pid=`cat /var/run/iris_guestd.pid`
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

`S33` starts before the network scripts and after `/usr` is mounted; verify the
local `rc2.d` order. IRIX uses the `rc2.d`/`rc0.d` S/K convention (IRIX Admin:
System Configuration and Operation, ch. 3); `chkconfig(1M)` is the local admin
tool, but `inst` owns the links. Do not edit `/etc/inittab`.

If the agent exits because hostcall is absent, the init script does not
respawn it; `ps`/`guest-status` shows it is not running. A human can restart
after fixing the build/feature. Do not add an inittab `respawn` line: on real
IRIX it would loop forever.

### 6.6 Install, upgrade, uninstall

Install from an extracted dist directory:

```sh
mkdir -p /tmp/iris_guest-dist && cd /tmp/iris_guest-dist && tar xf /tmp/iris_guest-100.tardist
inst -a -f /tmp/iris_guest-dist -I iris_guest
versions -n iris_guest | cat
showfiles -- /usr/sbin/iris_guestd | cat
ps -ef | grep iris_guestd
```

Check under IRIS with hostcall:

```sh
/usr/sbin/iris_guestd -f -v          # foreground; expect STREAM protocol 1
iris-ci guest-ping                   # from the host
iris-ci guest-info
```

Check under IRIS without hostcall or on real IRIX:

```sh
/usr/sbin/iris_guestd -f -v          # expect EINVAL and exit 1
```

Upgrade: bump `guest/version` to `101`, rebuild, install; `exitop` restarts.
Check `versions -n iris_guest` and `iris-ci guest-info` report 1.0.1; check an
edited config is `.N`.

Uninstall:

```sh
inst -a -R iris_guest
versions -n iris_guest | cat
ps -ef | grep iris_guestd
ls -l /etc/rc2.d/S33iris_guestd /etc/rc0.d/K03iris_guestd
```

`removeop` stops the daemon; `inst` removes the `l` links. A modified
`/etc/iris_guestd.conf` may be kept by `inst`'s config rules; document it.

### 6.7 Test matrix

| Case | What it proves |
|---|---|
| 6.5.22 and 6.5.30 fresh installs | ABI/API assumptions; init links |
| 6.5.28 upgraded from an older patch | `inst` upgrade, `config(suggest)` |
| IRIS release build with `hostcall`/`hostgl` | Normal operation |
| IRIS build without `hostcall` | `[guest]` disabled with message; agent exits; no fallback |
| `[guest] enabled = false` | Service not registered; agent exits |
| Real IRIX (if available) | `EINVAL`; agent exits; no serial/TCP attempt |
| Agent `kill -9` | Host reports not responding; restart works |
| Host restart while agent runs | New STREAM; agent re-syncs |
| Snapshot restore/rollback | `GuestStream::reset()`; no stale frames |
| `guest-run` with `allow_run=0` | `PermissionDenied` |
| `guest-run` with `allow_run=1` | argv-only, stdout/stderr stream, exit status |
| `guest-shell` with `allow_shell=0` | `PermissionDenied` |
| `guest-dd` with `allow_disk=0` | `PermissionDenied` |
| `image-write` on a live machine | Refused |
| 8 parallel instances | No cross-talk, per-instance stop |
| O2 (`Ip32Machine`) | Warns and stays off |

End-to-end procedure goes in `clanker/testing/guest-service-e2e.md`; it needs
IRIS and a 6.5 image, so it is a rig test, not a CI test. Host-only tests use a
mock guest through `iris_hostcall::dispatch` and run in `cargo test`.

---

## 7. Failure modes and security

| Symptom | Detection | Host behaviour | Guest behaviour | Recovery |
|---|---|---|---|---|
| Hostcall feature absent | `cfg!(feature)` | `[guest]` disabled with message; no service registered | Agent gets `EINVAL`; exits 1 | Rebuild with `hostcall`/`hostgl` |
| `[guest] enabled=false` | config | No service registered | Agent gets `EINVAL`; exits | Enable and restart |
| Agent absent | `guest-status` after ping timeout | Service registered, agent not responding; no fallback | None | Install/start agent |
| Agent crashed | ping timeout | Not responding; pending requests dropped | Init does not respawn in v1 | Manual restart/reboot |
| Agent hung | timeout | Pending requests time out; no machine lock held | Poll loop stuck | Kill/restart |
| Frame desync | bad magic/length | Send `RESET`; clear queues | Reset parser | Automatic |
| Queue full | short write/full wait | Short write or timeout; no unbounded memory | Agent retries | Automatic |
| `ENEEDPAGE` | hostcall reply | Nothing duplicated; retry | Touch page, retry | Automatic |
| Host/guest protocol skew | `guest-info` | Disable commands newer than host; log | Unknown command -> `CommandNotFound` | Upgrade one side |
| Guest user spoofing | trusted-guest assumption | Telemetry may be false; gates still apply | Any guest process can call STREAM | Trusted images only |
| `allow_run` enabled | config | `guest-run` becomes root RCE in guest | Executes argv | Disable; restrict CI socket |
| `guest-dd` bad offset | 512 alignment | Refuse or error | I/O error | Use 512 multiples |
| `image-write` live | status check | Refuse | N/A | Stop the machine |
| Shutdown accepted but hangs | `--wait` timeout | Log; leave running; no SIGKILL | rc script/driver hung | Inspect console |
| Snapshot restore | `ci_restore` | `GuestStream::reset()` | Agent resumes; next poll re-syncs | Automatic |
| Host restart | new process | New STREAM/shared/service | Agent's next poll gets INFO | Automatic |
| Guest panic | no response | Policy leaves it; crash log | N/A | Reboot/restore |

Security summary: in-process hostcall only, no network or tty; host feature
gating; root daemon with opt-in run/shell/disk; `allow_*` defaults false;
guest-wide hostcall channel documented as trusted-guest; CI socket is the
host-side authority. Enabling `allow_run` is equivalent to granting root in
the guest to anyone who can use the CI socket.

---

## 8. Milestones, effort, acceptance

### 8.1 Scope split

**v1 (12–14 engineer-days)** — the control and automation MVP:

- `STREAM` service 3001, framing, queues, page-fault handling, reader thread.
- Guest agent: hostcall stub, control/telemetry, shutdown/reboot, sync,
  `guest-run`/`guest-shell` opt-in, init script.
- Host: `[guest]` feature gating, CI commands, monitor commands, `idle-status`.
- Packaging (`iris_guest`), install/upgrade/uninstall.
- Lifecycle policy and measurements.
- No raw disk, no direct image write, no headless profile.

**v1.1 (5–7 engineer-days)** — data and coverage:

- `guest-dd` raw disk over STREAM, 512-byte alignment, `allow_disk`.
- `image-write` offline host image provisioning.
- Headless profile with local action allowlist.
- Optional stdin for `guest-run`, richer file transfer, `guest-get-fsinfo` /
  network info if verified.
- Poll backoff if v1 measurement demands it.

The tty/serial work from the first draft (termios, getty ownership, SCC
backend, baud, tty packaging) is gone. The hostcall service, frame protocol,
page-fault handling and the `guest-run` data path replace it, so v1 effort is
roughly the same, not lower; v1.1 is genuinely new work.

**Explicitly out of scope:** serial/TCP/Seeq control, real IRIX portability,
QGA transport/virtio, pause/resume, second idle detector, O2/IP32, IRIX 5.3,
GUI, CHD direct image write, package signing/licensing.

### 8.2 Milestones (v1)

| Milestone | Work | Days | Exit criterion |
|---|---|---|---|
| M0 — STREAM core | `iris-hostcall/src/stream.rs`, frame codec, fake `PageAccess` tests | 3 | `cargo test guest_stream` covers INFO/READ/WRITE/RESET, `ENOTYET`, `ENEEDPAGE`, caps |
| M1 — Agent | hostcall stub, control/telemetry, init, `guest-info` | 3 | `guest-ping/info/load/time` from CI under IRIS+hostcall |
| M2 — Command pipe | `guest-run`/`guest-shell`, reader routing, file capture | 2 | argv-only run with stdout/stderr and exit status; gates |
| M3 — Lifecycle/CI | `guest-shutdown`, `idle-status`, policy script, monitor | 2 | clean powerdown ≤30 s p95; no XFS repair |
| M4 — Packaging | idb/spec, gendist, install/upgrade/uninstall | 1.5 | `inst -a` ≤30 s; upgrade keeps config; uninstall clean |
| M5 — Fleet/measure | 8 instances, polling measurements, docs | 2.5 | 8 ping/stop; idle CPU budget; test matrix |

Total ~14 days; the critical path is M0/M1 (hostcall ABI and agent stub), not
the host UI. v1.1 adds 5–7 days.

### 8.3 Acceptance metrics

| Metric | Baseline | Target | Method |
|---|---|---|---|
| Settled 6.5 getty host CPU, agent idle | 2 % | ≤3 % (budget +1 %) over 60 s, 3 runs | `pidstat -p <pid> 1 60` or `/proc/<pid>/stat` |
| Poll cost at 1/50/250 ms | — | 50 ms ≤3 %; if not, 250 ms and ≤3 % | 3 runs each; `idle-status` park count/sleep |
| Park behaviour with agent | parks | park count/sleep increase; no permanent unpark | `iris-ci idle-status` |
| Clean powerdown | console scrape | ≤30 s p95, process exits, next boot clean | 10 runs |
| `guest-run` | console `run` | argv-only, stdout/stderr correct, gate off by default | manual/rig |
| Install | — | ≤30 s, no `inst` errors | `time inst -a` |
| Upgrade | — | ≤30 s, `.N` config, version 101 | install 100 then 101 |
| Uninstall | — | ≤10 s, no process/links | `inst -a -R` |
| Parallel 8 | 8 × 1–2 % | all ping ≤500 ms; all stop ≤60 s; no cross-talk | `scripts/fleet.sh -n 8` |
| Boot with agent | 42 s | ≤47 s | 3 runs |
| Protocol reliability | — | 10,000 pings, 0 errors, p99 ≤50 ms | `tests/guest_stream` stress |
| No-hostcall build | — | `[guest]` disabled; agent exits; no fallback | rig |

### 8.4 Risks

| Risk | Mitigation |
|---|---|
| Hostcall is not in the fleet build | Add `hostcall`/`hostgl` to the fleet build; config disables with a clear message; no fallback |
| Polling defeats idle-pause | Measure 1/50/250 ms; default 50 ms, back off to 250 ms if needed; do not disable idle-pause |
| Need-page retries duplicate data | Peek/drain READ, read-before-append WRITE; tests with a fake TLB |
| Reader thread complexity | One reader thread per instance, condvar-parked; route by id; no machine lock |
| `guest-run` root risk | `allow_run`/`allow_shell` default false; both host and guest gates; trusted images only |
| Raw disk corruption | `allow_disk`; `/dev/rdsk/` allowlist; 512 alignment; document VH; offline image path preferred |
| Live image write race | Only halted/offline; respect lock/COW; refuse otherwise |
| Hostcall ABI drift | Protocol version in `INFO`/`guest-info`; host/guest version check |
| Agent crash | Host reports not responding; no auto-respawn; manual restart |
| O2/5.3 | Out of scope; config warns/disabled |

---

## 9. Open questions for a human decision

1. **Is `hostcall` in every fleet/release build?** Standard release includes
   `hostgl`, which implies it; confirm the fleet build. If not, add it; the
   service is disabled otherwise.
2. **Is enabling `guest-run`/`guest-shell` acceptable on fleet images?** They
   are root code execution in the guest. The plan defaults both off, with
   separate switches. Is `allow_shell` needed at all in v1?
3. **Is v1 the right line for `guest-run`/`guest-shell`, or should they be
   v1.1?** The plan puts them in v1 because they exercise the data phase; the
   host-resource goal alone does not need them.
4. **Raw disk scope:** which paths are allowed (`/dev/rdsk/` only, or a
   per-instance allowlist)? Is sector 0/volume-header write ever allowed?
5. **Direct image write:** only when halted/before start, or also through a
   COW overlay while an instance is stopped? CHD is out of scope.
6. **Is `getloadavg(3)` available on the target 6.5?** If not, is
   `sysmp(MP_SAGET, …)` acceptable, or do we ship without load?
7. **Idle poll budget:** is ≤1 % host CPU the right regression budget? If
   50 ms fails, is 250 ms command latency acceptable for the fleet?
8. **Should the package be installed in standard IRIX images, or only in
   fleet images?** The agent exits on non-hostcall builds; installing it
   everywhere is harmless but noisy.
9. **QGA schema compatibility:** do third-party tools need exact QGA command
   names/shapes, or is the QGA-inspired `guest-*` control schema acceptable?
10. **Per-instance guest log:** `StatePaths::guest_log()` or `log::debug` only?

---

## 10. Sources

Verified means the page/source was read during this planning session; inferred
means a reasonable conclusion from the code or documentation, not directly
tested on the target.

### Hostcall and IRIS

| Claim | Source | Status |
|---|---|---|
| Hostcall numbers 3000–3009; 3000 GL, 3009 SELFTEST; indirect `syscall` 1000, number in `$4`, args `$5`..`$11`; `ENEEDPAGE`/`ENOTYET`; need-page retry; real IRIX indirect call fails `EINVAL` | `iris-hostcall/src/lib.rs` | verified in source; real-IRIX claim is documented as measured |
| Hostcall only from user mode and outside a delay slot; answered in `exec_syscall` before the IRIX exception; replies in `v0/v1/a3` | `src/cpu/mips_exec.rs:5868-5898`, `:16164-16230` | verified |
| The self-test service is registered in `Machine::new`; the registry is process-global | `src/machine.rs:479-482` | verified |
| `hostcall` is a Cargo feature; `hostgl` implies it; `HOSTCALL` is exposed in `src/lib.rs` | `Cargo.toml:250-264`; `src/lib.rs:101-106` | verified |
| Host GL's threaded service uses `ENOTYET` and a separate thread; a hostcall runs on the CPU thread and must not block | `iris-hostgl/src/threaded.rs`, `iris-hostgl/src/lib.rs` | verified |
| No serial/TCP control path is being added | this plan | design decision |

### QEMU guest agent (protocol research only)

| Claim | Source | Status |
|---|---|---|
| QGA transport options: `unix-listen`, `virtio-serial` (default), `isa-serial`, `vsock-listen`; default path `/dev/virtio-ports/org.qemu.guest_agent.0` | <https://qemu-project.gitlab.io/qemu/interop/qemu-ga.html> | verified |
| QGA uses QMP's JSON framing minus negotiation; `guest-sync`/`guest-sync-delimited` with `0xFF`; `{"return":...}`/`{"error":...}` | <https://wiki.qemu.org/Features/GuestAgent>, <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/main.c> | verified |
| Command set and shapes: `guest-ping`, `guest-get-time`, `guest-get-load`, `guest-shutdown`, `guest-exec`, `guest-file-*`, `guest-suspend-*`, `guest-fsfreeze-*` | <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/qapi-schema.json> | verified |
| QGA's POSIX load calls `getloadavg(loadavg, 3)` under `CONFIG_GETLOADAVG`; shutdown runs `/sbin/poweroff`, `/sbin/halt`, `/sbin/reboot` | <https://gitlab.com/qemu-project/qemu/-/raw/master/qga/commands-posix.c> | verified |
| virtio-console/serial is a virtio device with ports/virtqueues and needs a guest driver | <https://docs.oasis-open.org/virtio/virtio/v1.3/virtio-v1.3.html> (§5.3) | verified |

### Other guest agents

| Claim | Source | Status |
|---|---|---|
| Hyper-V integration services communicate over Hyper-V's channel (VMBus); services include heartbeat, KVP, shutdown, VSS | <https://learn.microsoft.com/en-us/virtualization/hyper-v-on-windows/reference/integration-services> | verified; VMBus name inferred |
| VirtualBox Guest Additions use a guest device/HGCM and `VBoxService` for guest properties/control | <https://www.virtualbox.org/manual/ch04.html> | verified (manual); HGCM detail inferred |
| SPICE's `spice-vdagent` uses a virtio-serial port `com.redhat.spice.0` with `spicevmc` | <https://www.spice-space.org/spice-user-manual.html> | verified |
| VMware Tools uses `vmtoolsd` and a VMware-specific backdoor channel, not virtio | <https://techdocs.broadcom.com/us/en/vmware-cis/vsphere/tools/12-4-0/vmware-tools-administration-12-4-0/introduction-to-vmware-tools/vmware-tools-service.html> | service verified; channel partly inferred |

### IRIX

| Claim | Source | Status |
|---|---|---|
| Shutdown: `/etc/shutdown -y -i0 -g0` goes to state 0; `of:0:wait:/etc/uadmin 2 0` powers off; `rb:6:wait:/etc/uadmin 2 1` reboots; `init 0`/`init 6` are the run-level forms | *IRIX Admin: System Configuration and Operation* 007-2859-021, ch. 3, <https://techpubs.jurassic.nl/library/manuals/2000/007-2859-021/sgi_html/ch03.html> | verified |
| Run-level scripts live in `/etc/init.d` with `S`/`K` links in `/etc/rc2.d`/`/etc/rc0.d`; `chkconfig(1M)` is the admin tool | same, ch. 3 | verified for layout; `chkconfig` with a new script: verify on the guest |
| inittab entry format `id:rstate:action:process`; actions include `respawn`, `off`, `wait`, `once`, `boot`, `bootwait`, `sysinit`, `initdefault` | IRIX `inittab(4)` (6.2 mirror), <http://ibgwww.colorado.edu/~lessem/psyc5112/usail/man/irix/inittab.4.html> | verified for 6.2; 6.5 assumed (verify `man inittab`) |
| `init` run levels 0–6, `init Q` re-examines inittab | IRIX `init(1M)` (6.2 mirror), <http://ibgwww.colorado.edu/~lessem/psyc5112/usail/man/irix/init.1.html> | verified for 6.2; verify on 6.5 |
| `inst` options `-a`, `-f`, `-r`, `-I`, `-R`, `-u`, `-F`, `-N`, `-M`; automatic mode; distribution directories | IRIX `inst(1M)` (6.2 mirror), <http://ibgwww.colorado.edu/~lessem/psyc5112/usail/man/irix/inst.1.html> | verified for 6.2; packaging skill is the tested 6.5 recipe |
| Product/image/subsystem layout, idb/spec, `gendist`, install/upgrade/remove | `clanker/skills/irix-inst-packaging/SKILL.md`; *IRIX Admin: Software Installation and Licensing* 007-1364-140, <https://techpubs.jurassic.nl/library/manuals/1000/007-1364-140/sgi_html/index.html> | verified (skill is the tested recipe) |
| `getloadavg(3)` / `sysmp(MP_SAGET, ...)` selector | IRIX `sysmp(2)` (5.3 man page), <https://techpubs.jurassic.nl/manpages_0530/cat2/standard/sysmp.html>, documents `MP_SAGET` and says the structure is selected by `arg1` in `<sys/sysmp.h>`; the load selector must be read from the 6.5 header | partly verified; **verify on the guest** |
| Raw disk needs 512-byte multiples; `bs=64` returns I/O error; scratch VH layout | `STORAGE.md` "Scratch volume"; `src/ci.rs` scratch comments; `src/sgi_vh.rs` | verified in repo docs/code |

### IRIS repository

| Claim | Source | Status |
|---|---|---|
| Idle detector design and measurements (2 %, 1–2 % per instance, 42 s boot) | `src/cpu/idle_park.rs`; `clanker/perf/idle-pause-work.md`; `clanker/testing/release-idle-pause-hangs-prom-boot.md`; `CHANGELOG.md` 0.1.10 | verified |
| `machine-stop` restarts peripherals and panics a live kernel; idle-pause parks in place | `clanker/perf/idle-pause-work.md` §4.1 | verified |
| CI command table and request/response framing | `src/ci.rs:455-475`; `src/ci.rs:406-440` | verified |
| Instance derivation: monitor `9000 + N*10`, CI socket, NAT subnet; this plan adds no endpoint | `docs/parallel-instances.md:48-72`; `src/config.rs:355-390` | verified |
| `StatePaths` accessors and the "no invented paths" rule | `src/state.rs`; `docs/parallel-instances.md` | verified |
| Scratch/direct disk paths and 512-byte alignment | `STORAGE.md`; `src/ci.rs` scratch commands | verified |
| Guest shutdown command `shutdown -p -g0 -y` is tested in this repo's loop | `clanker/skills/irix-guest-loop/SKILL.md` §7 | verified |

---

## 11. Decision log

| Decision | Alternative | Why |
|---|---|---|
| Hostcall `STREAM` 3001 only | Serial SCC, TCP, NFS/scratch, new device | IRIS-only by requirement; no driver, no network, no tty, in-memory |
| Binary frames + QGA-shaped JSON | QGA JSON-lines | Data phase needs raw bytes; framing gives ids and lengths |
| Poll `STREAM_READ` at 1/50 ms | Host→guest interrupt | No doorbell without a kernel driver; polling is the honest cost |
| Peek/drain READ, read-before-append WRITE | Service-side pending buffer | Avoids duplicate data across `ENEEDPAGE` retries |
| Host reader thread | CI handlers parse frames directly | Long `guest-run` must not block ping/lifecycle |
| `guest-run`/`guest-shell` in v1, opt-in | Defer to v1.1 | Exercises the data phase; root gates keep default safe |
| `guest-dd`/`image-write` in v1.1 | v1 | Heavier disk safety; v1 lifecycle goal does not need it |
| Disable when `hostcall` absent | Fallback transport | The service is IRIS-only; no false portability |
| No new `StatePaths`/`derive_instance` | Guest log/port | In-process transport; no endpoint |
| Native MIPSpro build | Cross-clang | `gendist`/`strip` are in the guest; no IRIX sysroot |
