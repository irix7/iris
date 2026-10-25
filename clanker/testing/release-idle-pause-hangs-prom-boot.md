# Release builds with idle-pause park the no-disk PROM boot and never wake it

Status: open, 2026-10-25. Observed with `irix7/iris` release `v0.1.6+irix7`.

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
