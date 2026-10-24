# HPC3 DMA is bulk, not byte-at-a-time — and so is the real hardware

**Confirmed by the hardware it emulates, not just best practice.** The Linux
driver for this exact chipset makes the case airtight — see Sources.

`DmaClient` (`src/traits.rs:259-273`) exposes only per-unit `read()` / `write()`.
Consumers loop byte-by-byte:

- SCSI: `send_data_chunked` / `receive_data_chunked_from`
  (`src/dev/wd33c93a.rs:2184-2326`) — `dma_dev.write(data[i] as u32, is_last)` and
  `data.push(dma_dev.read() as u8)` per byte.
- Ethernet: `pump_tx` / `pump_rx` (`src/dev/seeq8003.rs:333-404`).
- HAL2: `read_frame_from` (`src/dev/hal2.rs`), 1–4 `client.read()` per sample.

Each unit is one `parking_lot::Mutex` acquire plus one full bus traversal:
`PdmaClientImpl::read/write` lock the channel per call (`src/dev/hpc3.rs:738-752`),
then `dma_read`/`dma_write` do a single `mem.read8/16/32` through the MC bus
(`hpc3.rs:531-654`). A 64 KiB SCSI block is therefore thousands of lock+bus
operations.

## The correction to the obvious fix

The SCSI **bus handshake** (REQ/ACK per byte) is irreducibly byte-level — the
MAME `scsihle`/`wd33c9x` models keep it byte-level too. The layer that must
become bulk is the **HPC3 DMA-memcpy path**, which in hardware moves whole
descriptor spans, not bytes.

The real driver confirms it. `drivers/scsi/sgiwd93.c` `fill_hpc_entries()` builds
a descriptor **chain** in bulk (one descriptor per ≤8 KiB chunk — the comment
"without magic only 8192 works correctly" records a hardware quirk), tags the
last with `HPCDMA_EOX`, then `dma_setup()` kicks the whole chain once
(`ctrl = HPC3_SCTRL_ACTIVE`). `drivers/scsi/wd33c93.c` walks scatter-gather per
*buffer*, not per byte, and latches EOP/EOX/XIE only at completion/interrupt.

## Suggested change

Add bulk methods to `DmaClient`, defaulting to a loop over the existing per-unit
calls so other implementors are unaffected:

```rust
fn read_block(&self, dst: &mut [u8]) -> DmaStatus;
fn write_block(&self, src: &[u8]) -> DmaStatus;
```

`PdmaClientImpl` overrides them to lock the channel **once**, memcpy across the
descriptor span (via `BusDevice::read_block`/`write_block`, already on the
trait), and process descriptor boundaries (EOX/EOP/XIE/IRQ) only at transitions.
Carry boundary state back in the returned `DmaStatus` rather than a per-unit
`bool is_last`.

Rules to hold onto:

- **Chunk at descriptor granularity (≤8 KiB), matching hardware.** This also
  bounds the channel-lock hold time — the "lock held across a whole transfer"
  anti-pattern (see QEMU's BQL history) is the other half of this bug.
- **Deliver IRQ/status after unlocking**, never while holding the DMA mutex —
  the callback-up-to-a-parent deadlock class HACKING.md warns about.
- **Endianness stays at The Edge**: apply the swap once per buffer (or per
  descriptor), not per byte in the core. The hardware has `HPC3_SCTRL_ENDIAN` /
  `HPC3_SDCFG_SWAP` for exactly this.
- Keep the byte-level path only as a PIO fallback (WD33C93A `DATA` register /
  `CTRL_DM_POLLED`), which the real driver also keeps.

## Sources

- Linux `drivers/scsi/sgiwd93.c` (HPC3 descriptor chain, EOX, 8 KiB chunks)
- Linux `drivers/scsi/wd33c93.c` (SG per-buffer, PIO fallback, residual read-back)
- Linux `arch/mips/include/asm/sgi/hpc3.h` (`hpc_dma_desc`: `pbuf`/`cntinfo`/`pnext`,
  `HPCDMA_EOX`/`EOXP`/`XIE`/`BCNT`)
- QEMU `system/dma-helpers.c` + `include/system/dma.h` (SG list → single AIO)
- MAME `src/devices/machine/wd33c9x.cpp` (byte bus, per-phase IRQ)
