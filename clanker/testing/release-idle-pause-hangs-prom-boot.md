# Release builds with idle-pause park the no-disk PROM boot and never wake it

Status: open, 2026-10-25. Observed once with `irix7/iris` release `v0.1.6+irix7`;
**not reproduced** on a quiet host with the same binary and the same config (see
the update at the end). Treat the workaround as belt-and-braces, not a fix for a
confirmed emulator bug.

## Symptom

A release CLI boot of the embedded PROM with no media hangs before the System
Maintenance menu:

```toml
banks = [128, 128, 0, 0]
headless = true
no_audio = true
[machine]
profile = "indy_ip24"
[scsi]
```

`iris --ci` starts, the monitor binds, `iris-ci status --wait-ready 30` returns
`running`, and the PC advances, but the serial log stays empty. A graphics
boot of the same config never reaches a video mode, so `iris-ci screenshot`
reports `the display has no video mode yet`. Stderr fills with
`MC: GIO Timeout at 1f0f1338` lines. `iris --help` is fine; the problem is the
guest boot, not loading the binary.

## Cause

The release CLI is built with `idle-pause` (`release.yml` / `nightly.yml` use
`lightning,jitv2,gpujit,idle-pause,hostgl`). The no-disk PROM path enters a loop
that idle parking recognises and parks on, but the interrupt that should wake it
does not get it out. The evidence is the toggle and the two builds:

- `v0.1.6+irix7` with `IRIS_NO_IDLE=1` reaches `Option?` in about 6 s, and with
  graphics reaches a screenshot in about 5 s.
- The same release without the variable never gets there in 60 s.
- A source build with `lightning,opcodefusion,rex-jit,gr4-jit,gr2-jit,...`
  (no `idle-pause`) reaches the menu normally.
- `IRIS_JIT_DISPATCH=off` does not change the release failure, so it is not the
  jitv2 dispatch path.

## Rule

For a ROM-only smoke test of a *release* binary, set `IRIS_NO_IDLE=1`. Source
builds with the CI feature set `lightning,gpujit` do not need it. Do not add
`idle-pause` to a source smoke until this is fixed, or the job will time out
waiting for the PROM.

`docs/ci.md` carries the user-facing version of this workaround; this note is
the evidence trail.

## Update, 2026-10-25 (reproduction attempt)

The exact config above, the same `v0.1.6+irix7` release binary, on a quiet host:
the menu appears in under 10 s (`System Maintenance Menu` / `Option?` in a 333-byte
serial log) with `idle-pause` active. A second run with `--ci --headless` and the
embedded PROM also halts normally (`No boot payload present - halted`). So the
hang is not deterministic; it may be load- or timing-dependent, or was an
artifact of the busy host that first ran the smoke (fake apt, several downloads).
Keep `IRIS_NO_IDLE=1` in the release self-test until someone reproduces it with
the failing command and artifacts.

## Update, 2026-10-26 (clock revert #98)

The wall-clock revert changes what a parked CPU can rely on. `park` used to
advance `hot.cycles`, and under the fork's cycle-derived clock that *was*
guest time, so an instruction-counted delay loop inside a parked stretch could
still "catch up" in Count. After #98 Count is host time; `hot.cycles` is a
progress counter, so a delay loop that needs executed instructions is simply
stalled.

Measured on `callisto` with an IRIX 6.5 IP22 disk:

| build | cold boot to login |
|---|---|
| default wall clock | 45 s (three runs) |
| `ci_clock` | 45 s |
| `ci_clock,idle-pause` before the back-off | 176 s |
| `ci_clock,idle-pause` with the back-off | 45 s |
| `idle-pause` (wall clock) | not reached in 400 s; serial console showed "The system is coming up" and the CPU was at 2 % |

The back-off in `idle_park.rs` (do not re-park for ~20 ms after waking on a
line the guest cannot take) fixed the `ci_clock` case. It does not fix the
wall-clock case, where the wake line (`IP7`) *is* deliverable and the parked
loop is a delay loop rather than an idle loop.

At the IRIX 6.5 getty the detector did not engage at all: host CPU stayed at
~194 % over three 10 s samples, while the same image with no `idle-pause` read
~202 %. IRIX 5.3's idle loop (the one this detector was built against) did
park.

## Resolved, 2026-10-27 (issue #65)

The 6.5 detector was replaced with a PC-cluster sample (`src/cpu/idle_park.rs`;
see `clanker/perf/idle-pause-work.md` for the capture and rules) plus a 60 s
boot warm-up. Measured on `callisto`, IRIX 6.5 IP22, fresh disk:

| state | host CPU / time |
|---|---|
| idle-pause cold boot to login | 42 s (three samples; was >400 s) |
| `ci_clock,idle-pause` cold boot to login | 46 s |
| settled getty, idle-pause | 2 %, 2 %, 2 % |
| two parallel instances, each getty | 1-2 % each, both responsive |

The remaining ~100 % of the old ~200 % was not parking at all: `seeq-enet`
never cleared its `busy` flag on the idle fast path and spun forever. Fixed in
`src/dev/seeq8003.rs`; see
[seeq-enet-busy-pinned-by-idle-fast-path.md](../devices/seeq-enet-busy-pinned-by-idle-fast-path.md).

Not verified: IRIX 5.3, because the rig has no 5.3 image. Its 14-instruction
loop fits the new window rules and the park/wake mechanics are unchanged.
