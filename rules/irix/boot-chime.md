# The boot chime is the PROM's power-on tune

**Keywords:** audio, chime, boot, tune, hal2, ip22, ip24, ip28, prom, nvram, volume, ci

The SGI boot chime is real and IRIS drives it to the wav sink. It is the
**PROM's power-on tune**, not an IRIX-side sound: it plays before any OS load and
even with no disk attached. It is capturable on the Indy (IP22/IP24) and the
IP28 profiles.

## What gates it

- **HAL2 present** — `no_audio = false`. With `no_audio = true` the HPC3 has no
  HAL2 at all, so nothing is captured. This was the root cause of the earlier
  all-zero result.
- **PROM env `volume` set (non-zero)** — the tune reads the boot-tune volume
  out of NVRAM. A fresh/erased NVRAM has no `volume` and the first boot is
  silent. The rig's `nvram-iris.bin` carries `volume=80`, so it plays.
- IP28 additionally gates `play_hello_tune` on the CPU revision (IRIS reports
  the R10000 PRId `0x0925`), and its erased motherboard EEPROM is initialised
  with `volume 80` / `boottune 1` before the CPU starts — see
  `rules/irix/nvram-persistence.md`.

Correction to the first #78 write-up: it claimed the IP22/IP24 PROMs do not play
a chime. They do. That session used a *fresh* NVRAM (no `volume`), so the Indy
PROM was silent, and the rig that produced the original all-zero capture had
`no_audio = true`.

## Confirmed capture (real media, 2026-10-09)

Media: `/mnt/europa/sgi-mame/irix-6.5.22m-MAME/irix65.chd` (Indy 6.5.22f install),
`scratch.raw`, and `nvram-iris.bin` (`volume=80`).

```
# copy the rig config, set no_audio = false, keep the same disk/scratch/nvram
IRIS_HAL2_CAPTURE=$PWD/.tmp/chime.wav \
  iris --config .tmp/iris-ci-audio.toml --ci --serial-log .tmp/iris-console.log
iris-ci --socket 127.0.0.1:19878 start
# the chime fires at power-on; ~2 s later the capture has it
iris-ci --socket 127.0.0.1:19878 quit --sync-chd all
```

Measured with the worktree debug build: 401244-byte WAV (44-byte header +
200600 samples = 100300 stereo frames at 48 kHz = 2.09 s), 200550 non-zero
samples, first non-zero at sample 4, peak 32768. Identical with and without the
disk attached, confirming the tune is the PROM's.

## Current-main IRIX boot regression (separate from the chime)

The rig disks do **not** read on current main. `hinv` at the PROM command
monitor lists the SCSI targets, but

```
ls dksc(0,1,8)              -> dksc(0,1,8): no such device
boot -f dksc(0,1,8)sash     -> Unable to load dksc(0,1,8)sash: no such device
```

so IRIX never boots (`Autoboot failed` / hangs at "waiting for kernel boot").
Both the CHD (ID 1) and the raw disk (ID 3) fail, so it is the SCSI read path,
not a CHD-specific problem, and it reproduces in both the debug and release
builds of current main (so it is not an overflow/debug-assert artefact). The
pre-wave release binary built from `origin/add-riscv-packages` (`e5458148`,
which is `b0e6f7b` + 1 and has none of the storage waves) boots the same CHD to
`IRIS console login:` in ~6 min.

Bisect window: `b0e6f7b` (boots) .. `4f04cff` (main, fails). The layered
block-node storage waves — #49 (`df7f6c7`), #50 (`1801c06`), #51 (`d9c1953`) —
are the prime suspect, but the range also carries the cpu/dev directory moves
and the bulk-DMA/perf commits. The chime capture above does not need IRIX to
boot, so it is unaffected.

## CI

`src/machine.rs` `boot_chime_tests::ip28_prom_boot_chime_is_captured_through_the_wav_backend`
is the media-free CI check: it boots IP28 headless with `IRIS_HAL2_CAPTURE` set
and asserts a 48 kHz RIFF/WAVE with non-zero samples within the first 100 ms and
a plausible duration. IP28 is used because its EEPROM is auto-initialised with
`volume 80`, so the test needs no seeded NVRAM and no media. The Indy PROM
chime is validated by the real-media procedure above.
