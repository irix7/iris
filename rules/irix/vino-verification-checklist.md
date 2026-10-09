# VINO / IndyCam verification checklist

Consolidates guidance from `rules/irix/vino-*.md`.

## Prerequisites

- `[vino]` configured in TOML; host camera support is built in (no `camera` feature)
- MC SYSID bit 4 set (`rules/irix/vino-attach-via-sysid-bit4.md`)
- GIO alias `0x1F080000` → VINO (`rules/irix/vino-gio-alias-offset.md`)

## Per-channel routing (implemented)

- **SELECT_D1 clear:** composite / SAA7191 path → `source_d0` (default black NTSC field)
- **SELECT_D1 set:** IndyCam / CDMC path → `source_d1` (`set_source` from machine)

## I2C

- Repeated-start reads: START → addr → subaddr → RE-START → read-addr → READ (`src/dev/vino.rs` tests)
- Mid-transaction streaming: `I2C_DATA` writes while `NOT_IDLE` set

## Frame rate mask

- `CH_FRAME_RATE` write resets `field_counter` so the 12/10-field mask applies from field 0

## Manual IRIX tests

1. `vlinfo` — `vino 0` with 5 nodes
2. IRIX 5.3: `indycam_eoe` / capture — run the live procedure in
   "IRIX 5.3 live capture validation (issue #70)" below; background in
   `rules/irix/indycam-end-to-end-capture.md`
3. IRIX 6.5: `videod` + `vl_eoe` / `vino_eoe` per `rules/irix/vino-capture-on-6.5-progress.md`
4. `vino status` in monitor — D0/D1 source lines

## IRIX 5.3 live capture validation (issue #70)

The VINO descriptor-engine rewrite (#56) and the pixel-producer/descriptor split
(#71) changed the interlaced (6.5) path. IRIX 5.3 capture is EOF-driven and
non-interleaved, so it is structurally unchanged — but no live 5.3 `vidtomem`
run was made after the rewrite. The unit suite pins the 5.3 shape
(`nix develop --command cargo test -p iris --lib vino_5_3`); the steps below are
the human-side live check against real media. **No IRIX 5.3 media is available
in the agent/CI environment, so this run has not been performed for #70.**

### Prerequisites

- A bootable IRIX 5.3 Indy disk and the 5.3 base CD (see
  `rules/irix/irix-install.md`).
- `iris-irix53.toml` with `[vino] source = "camera"` (or `"test_pattern"` to
  take the camera out of the variable) and `[scsi.2] scratch = true`.
- A host camera for D1/IndyCam when using `source = "camera"`.

### Run it

```bash
# host — build and boot with VINO tracing on
cargo build --release
IRIS_DEBUG_LOG=vino ./target/release/iris --config iris-irix53.toml --ci &
./target/release/iris-ci boot --timeout 600
./target/release/iris-ci login                                   # root, no password

# guest — confirm the board/camera, then capture one frame
./target/release/iris-ci run 'vlinfo'                            # vino 0, EXT_camera
./target/release/iris-ci run 'vidtomem -f /tmp/cap -v 0'         # -> "saved image to file"

# host — pull the frame back over the scratch volume (fast path)
./target/release/iris-ci get /tmp/cap-00000.rgb --to /tmp/grab53.rgb
file /tmp/grab53.rgb    # expect: SGI image data, RLE, 3-D, 640 x 480, 3 channels

# quiesce before terminating (keeps the root filesystem clean)
./target/release/iris-ci run 'sync; halt -y'
./target/release/iris-ci quit
```

`videod` is usually already running; if `vidtomem` cannot connect, start it
first with `./target/release/iris-ci run '/usr/etc/videod &'`.

### What "pass" looks like (5.3-specific signals)

- `vidtomem` prints `saved image to file` and exits 0, and the file is a clean
  640×480 3-channel SGI image (open it on a 6.5 guest with `ipaste`, or convert
  it host-side).
- **INTR is EOF-only.** In the driver's `INTR_STATUS` reads (or the `vino
  status` monitor command) the only channel-A interrupt state during capture is
  `0x01` (EOF). `0x05` (DESC|EOF) must **not** appear — it would mean the
  6.5-oriented STOP handling leaked into the 5.3 path (the STOP branch is
  `interleave`-gated, so it should not fire here).
- Channel-A DMA stays enabled for the whole run; the 5.3 chain is not
  STOP-terminated, so nothing should disable it.
- No `vino.rs` descriptor-fetch errors and no kernel panic.

### If it diverges

Keep the `IRIS_DEBUG_LOG=vino` trace and report it on #70 with the observed
`INTR_STATUS` values and whether `vidtomem` hung or saved a bad frame. The
last-known-good 5.3 capture predates the #56/#71 rewrite, so a bisect between
the pre-rewrite commit and HEAD isolates a regression to one of those two
commits.

## Known gaps

- 6.5 interlace capture may show a thin diagonal artifact (`src/dev/vino.rs` comments)
- HPC1 region black-holed to avoid capture panic (`src/physical.rs`)

## CDMC → VideoSource (implemented)

- `CdmcAdjustedSource` applies gain, colour balance, saturation, and shutter exposure
  to UYVY fields from `source_d1` (`cdmc.rs` → `video_source.rs`)
- Manual IRIX capture still required to close the checklist end-to-end
