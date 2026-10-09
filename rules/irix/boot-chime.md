# The boot chime is a PROM tune on IP28 only

**Keywords:** audio, chime, boot, tune, hal2, ip22, ip24, ip28, prom, ci

The SGI boot chime is real and IRIS can capture it end to end — but only on the
profile whose **PROM** plays it.

## Which profile emits a chime

- **IP28** (`indigo2_ip28`, R10000): the embedded `070-1477-002` PROM runs
  `play_hello_tune` early in POST. It is gated on the R10000 PRId (IRIS reports
  `0x0925`, so it plays; see `src/cpu/mips_exec_test.rs`) and on the
  motherboard EEPROM's boot-tune volume. `Machine::new` initialises an erased
  IP28 EEPROM with `volume 80` / `boottune 1` before the CPU starts
  (`Eeprom93c56::initialize_ip28_if_erased`), so the first boot is not silent.
  The tune enables Codec A DMA on the HAL2 and plays ~2 s of audio.
- **IP22 / IP24**: the embedded PROMs only *initialise* the HAL2 — the BRES
  clock (44100 Hz) and codec registers — and never enable Codec A DMA. A
  headless boot to the PROM command monitor (or `Start System` with no boot
  device) is silent. The Indigo2/IP22 startup sound is an IRIX-side playback
  and needs a guest; the `hal2.rs` "IP22 boot tune" comment on the BRES default
  refers to that clock setup, not a PROM-emitted tune.

## Why the #73 capture was all-zero

#73 looked for the chime on the wrong profile/config: with no IRIX media the
Indy/Indigo2 PROM command-monitor path never drives HAL2 playback. The audio
path itself was fine — the same wav backend captures the IP28 PROM tune sample
for sample.

## What CI checks

`src/machine.rs` `boot_chime_tests::ip28_prom_boot_chime_is_captured_through_the_wav_backend`
boots an IP28 machine headless with `IRIS_HAL2_CAPTURE` set to the wav backend
and asserts the capture is a 48 kHz RIFF/WAVE with non-zero samples in the
first 100 ms and a plausible (~2 s) duration. It is `#[ignore]` because
`IRIS_HAL2_CAPTURE` is process-global; rust.yml runs it with `--ignored
--test-threads=1`.
