# VINO descriptor-engine rewrite — plan spike (#55)

Status: **plan only — no emulator behaviour is changed by this document or the
target test it accompanies.** This is the spike requested by issue #55: bound
the previous, reverted attempts, specify the target architecture, and land a
single `#[ignore]`d end-to-end target test that encodes the acceptance criterion
("IRIX 6.5 `vidtomem` capture yields a clean frame").

Companion primary sources, read before touching this:

- `rules/irix/vino-capture-on-6.5-progress.md` — the cont. 1–16 campaign log
  (the disassembly of `vino.o` `0x77c0` / `0x60b4` / `0x7640`).
- `docs/research/b10-vino-descriptor-sync.md` — the MAME/Dolphin/ares
  comparison that motivates the split.
- `docs/research/attempts/b10-attempted.md` — what actually landed, and where
  each recommendation stalled.

## 1. Where the current model lives

| Concern | Current code (`src/dev/vino.rs`) | Note |
|---|---|---|
| Pixel production | `render_and_pump` (`:818`) | walks the clipped rectangle, converts, packs bytes; stops on rectangle exhaustion **or** a STOP reported by the DMA helper |
| DMA / STOP | `dma_emit_dword` (`:605`) | the only place STOP is consumed: raises `CHA_DESC` and clears DMA enable (deferred to the second field when interleaved and `field_counter == 0`) |
| 4-descriptor cache | `descriptor_fetch` (`:521`), `shift_descriptors` (`:570`) | loads four words; rotates on 4 K page wrap; reloads on invalid or JUMP; JUMP target 16-byte-aligned via `desc::PTR_MASK = 0x3FFF_FFF0` |
| Page index wrap | `page_index_w` (`:549`) | reached **only** from a guest `CH_PAGE_INDEX` write, never from the DMA loop (which does its own `& 0x0FFF` arithmetic) |
| Field boundary | `pump_field` (`:716`) | unconditionally rewinds the cursor to `start_desc_ptr` every field |
| Readback shim | `read_channel_reg(CH_DESC_TABLE_PTR)` (`:1044`) | returns `start_desc_ptr + FIELD_DESC_SPAN` (`0x780`) once `field_counter >= 2` |

The shipped 6.5 path (cont. 12/15/16) delivers a 640×480 frame, but it does so
with two 6.5-only heuristics layered on a **pixel-driven** DMA loop:

1. `dma_emit_dword` defers the STOP completion to the second interlaced field
   so the kernel's field-parity counter (`conn+0xd2`) reaches ODD.
2. `CH_DESC_TABLE_PTR` reports the kernel's recorded field boundary as a
   hard-coded `base + 0x780` so `0x77c0` returns 1 and `0x7640` does not abort.

Neither is derived from the descriptor chain, and neither generalises past
640×480. The rewrite replaces both by modelling the hardware as it actually
behaves: a descriptor engine that walks the chain to its STOP, once per
interlaced frame, with the field boundary emerging from the walk.

## 2. Prior attempts and why they were reverted

All were working-tree experiments on the since-deleted `vino-6.5-capture-engage`
branch; only prose survives in the campaign log. None ever landed.

| cont. | Attempt | What it did | Why reverted |
|---|---|---|---|
| **2** | "Contiguous model" | Dropped the per-row interleave skip + stride pad and rewound the cursor only on the even field, so one cursor flowed across both fields to the frame STOP. | Reached the frame STOP but `videod` still did not deliver; it also touched the working IRIX 5.3 interlace geometry. Later re-characterised (cont. 4) as the **wrong direction**: it made capture EOF-only (no DESC), so `vinoEOD` never fired. |
| **3** | Rewind-to-rearm-base | Honoured the live `A_NEXT_4_DESC` ping-pong instead of always rewinding to `start_desc_ptr`, so iris filled the buffer the driver actually re-armed. | The readback froze at descriptor ~120; stopped after six builds. The deeper finding was the **modelling gap**: iris's DMA is pixel-driven, so it stops at the pixel count and never walks the chain to STOP — exactly the gap this rewrite closes. |
| **4** | `drain_to_stop` | After the pixel pump, walked the remaining descriptors (following JUMPs) to consume the trailing JUMP→STOP and raise DESC. | Reached correct DESC+STOP (and required the 16-byte JUMP alignment), but cont. 5 found the live `A_DESC_TABLE_PTR` then read *past* STOP (`0x08621500`), making `0x77c0` return 2 and **skip** the delivery function `0x7640`. Recommended revert. |
| **9** | EOF/DESC split | Emitted EOF and DESC as two separate interrupts (INTR `0x01` / `0x04`) so the parity write and read land in different ISR passes. | Mechanically worked and parity reached ODD, but still no delivery, and it **harmed `vinoEOD`**: `vinoEOD` aborts unless EOF fired in the same ISR pass (`a2 == 0`), so DESC alone aborts. Reverted. |
| **12** | Field-counter defer (the shipped interim) | In `dma_emit_dword`, on the first interlaced field of a DMA-enable cycle (`field_counter == 0`) reach STOP but defer DESC/disable; the second field completes. | **Not reverted — it ships** (with the cont. 15/16 `0x780` readback + ABGR order). Listed here because it is the hack the rewrite retires: it is still pixel-driven, and it needs both the `field_counter==0` heuristic and the constant readback to make the 6.5 kernel's `0x60b4` tail run. The descriptor-engine model should reproduce the same INTR sequence (EOF on field 1, EOF+DESC on field 2) as a *consequence* of the chain walk, after which both heuristics and `FIELD_DESC_SPAN` can be deleted. |

Two corrections the rewrite must respect, learned the hard way:

- **`vinoEOD` needs EOF in the same ISR pass.** DESC must stay *with* EOF on
  the completing field (cont. 9/12) — do not split them again.
- **The `0x7640` abort is load-bearing.** Suppressing it by faking the readback
  (cont. 14's `base + 0x10`) desyncs the field pairing. The readback must be the
  *true live cursor*, advanced by real descriptor consumption.

## 3. Target architecture

MAME `src/mame/sgi/vino.cpp` separates the two concerns; IRIS should adopt that
shape (with IRIS's measured corrections), then delete the shims. See the B10
research doc for the line-by-line MAME references (`end_of_field` `:473-518`,
`do_dma_transfer` `:520-539`, `push_fifo` `:541-561`, `count_pixel` `:646-671`,
`page_index_w` `:863-879`, `shift_dma_descriptors` `:898-918`,
`load_dma_descriptors` `:920-939`).

### 3.1 Stage 1 — pixel production (`count_pixel` → per-channel FIFO)

- Keep the field timing and format conversion, but make it purely a *producer*.
  `render_and_pump` becomes a thin wrapper over a `count_pixel`-equivalent that
  tracks the field's remaining pixel budget only. It sets an `end_of_field`
  flag; it does **not** know about descriptors, STOP, or the cursor.
- Converted output goes into a per-channel FIFO (assembled qwords), exactly the
  role of MAME `push_fifo`. The existing `emit_byte`/`accum`/`bytes_in`
  machinery can become the FIFO writer.
- Decimate/clip/stride padding stay in this stage (they are source geometry,
  not descriptor geometry).

### 3.2 Stage 2 — descriptor engine (consume descriptors to STOP)

- A single per-channel `do_dma_transfer` loop drains the FIFO into memory
  bounded by the FIFO *and* the STOP bit — never by the pixel count:

  ```
  while fifo has a whole qword && !(descriptors[0] & DESC_STOP_BIT) {
      address = (descriptors[0] & 0x3ffff000) | (page_index & 0x0ff8);
      mem.write64(address, qword);
      page_index_w(page_index + 8);          // advances + shifts on wrap
      if interleaved { line_count_w(line_count + 8); }
  }
  ```

- **`page_index_w` becomes the DMA engine's page walk**, not a guest-only
  helper. It wraps at `0x1000`; when the value decreases it calls
  `shift_dma_descriptors`. This is the piece cont. 4/5 identified as missing:
  the cursor must advance by descriptor consumption, not by pixel writes.

### 3.3 4-descriptor cache

- Cache `descriptors[0..3]`; rotate on page-index wrap (the existing
  `shift_descriptors`).
- On rotation, if the new `descriptors[0]` has no valid bit, load the next
  four words from `next_desc_ptr` and `next_desc_ptr += 16`.
- If it carries the JUMP bit, load four words from the target.
- **Corrections IRIS measured, keep them** (MAME is a structural reference, not
  an oracle):
  - 16-byte-align the JUMP target (`& 0x3FFF_FFF0`); the jump-bug chain carries
    a `+4` low-bit offset that scrambles the walk if followed unaligned.
  - Track the live base separately from `start_desc_ptr` (MAME never updates
    `m_next_desc_ptr` to the jump target, which is a bug).

### 3.4 `end_of_field` even-rewind / odd-advance

Mirror MAME `end_of_field` at each field boundary:

```
if interleaved {
    if field is even {
        line_count_w(0);
        page_index_w(line_size + 8);   // second field's first row
        next_desc_w(start_desc_ptr);   // rewind the chain within this buffer
    } else /* odd */ {
        line_count_w(0);
        page_index_w(0);
        start_desc_ptr = next_desc_ptr; // advance: ring progresses frame-by-frame
    }
}
raise EOF
```

- The odd-field `start_desc_ptr = next_desc_ptr` is the advance that makes the
  ring progress and lets a multi-buffer chain eventually reach STOP. This is
  what cont. 3/4 lacked.
- Gate on `interleave`: IRIX 5.3 is EOF-driven and page-steps `NEXT_4_DESC`
  per field, never reaching a STOP descriptor here, so its path must be a
  structural no-op (but still regression-test it live).
- Stop consuming STOP in the pixel pump (`dma_emit_dword`). Consume it in the
  descriptor fetch (`load_dma_descriptors` analogue): when `descriptors[0]` has
  `DESC_STOP_BIT`, raise `CHx_DESC` and clear `CTRL_CHx_DMA_EN` **there**. This
  yields exactly one DESC per frame, which is what the 6.5 kernel's
  `vinoEOD`/`0x60b4` delivery path keys on. On the first interlaced field the
  STOP is not yet reached (both fields traverse the same dense chain), so DESC
  naturally lands on the completing field — no `field_counter` heuristic needed.

### 3.5 Retiring `FIELD_DESC_SPAN = 0x780`

The `0x780` constant exists only because the current model cannot present a
cursor that reaches the kernel's recorded field boundary
(`*(bufentry+0x10)` = `base + 0x780` = 240 rows/field × 8). With the descriptor
engine in place:

1. `read_channel_reg(CH_DESC_TABLE_PTR)` returns the **live cursor** (derived
   from `next_desc_ptr` / `descriptors[0]`), advanced as a real consequence of
   DMA progress — not `start_desc_ptr` and not `start_desc_ptr + const`.
2. Delete `const FIELD_DESC_SPAN` and the `field_counter >= 2` branch at
   `src/dev/vino.rs:1044-1051`.
3. Any remaining need for a field-boundary value is derived from `line_size`
   and the clip height (rows-per-field × bytes-per-row), never a literal
   tuned for 640×480.

The cont. 14 lesson applies: do **not** fake this readback to force `0x77c0` to
return 1. It must equal the true cursor, or the `0x7640` abort (which drives the
restart and the even/odd pairing) desyncs.

### 3.6 Migration and the 5.3 gate

Sequencing to keep the working 5.3 and 6.5 paths green:

1. Introduce the descriptor-engine loop behind the existing tests; keep
   `dma_emit_dword`'s STOP path until the engine's STOP consumption is proven.
2. Land the 4-descriptor cache/`page_index_w` walk and the `end_of_field`
   even-rewind/odd-advance, `interleave`-gated.
3. Reproduce the 6.5 INTR sequence from the chain walk alone (EOF on field 1,
   EOF+DESC on field 2), then remove the `field_counter==0` defer.
4. Switch `CH_DESC_TABLE_PTR` to the live cursor and delete `FIELD_DESC_SPAN`.
5. Regression-test live: stock `vidtomem` on 6.5 (clean frame, delivery) **and**
   on 5.3 (unchanged EOF-driven path), plus `cargo test --workspace --lib`.

## 4. Acceptance criterion and target test

The target is a single, `#[ignore]`d test that encodes the acceptance
criterion: **an IRIX 6.5 interlaced capture walked through the jump-bug
descriptor chain produces a clean, gap-free frame.**

- Test: `vino_6_5_vidtomem_capture_yields_clean_frame`
  (`src/dev/vino.rs`, `#[cfg(test)] mod tests`).
- It builds a `vinoBuildJumpBugDAPS`-shaped chain (300 data pages + JUMPs, a
  terminating STOP), programs a 640×480 interlaced RGBA capture, pumps both
  fields, and asserts:
  - exactly one `CHA_DESC` for the frame (STOP consumed once, in the chain walk);
  - all 300 chain data pages are written exactly once (the descriptor engine
    consumes the chain, independent of the pixel rectangle);
  - the reconstructed buffer is the full, contiguous 640×480×4 frame with the
    expected ABGR content (no gaps, no duplicated pages).

It is `#[ignore]`d so it does not turn the suite red while the rewrite is
unbuilt; run it with
`cargo test -p iris --lib vino_6_5_vidtomem -- --ignored --nocapture`.
Its failure *is* the work item.

## 5. Risks

- **5.3 regression.** The rewrite touches the interlace path 5.3 delivery
  depends on. Structural arguments are not enough (the campaign proved this);
  run stock `vidtomem` on both guests before committing.
- **MAME is not an oracle.** Its unaligned JUMP walk and stale `next_desc_ptr`
  are wrong for this chain; port the shape, keep IRIS's measured corrections.
- **Snapshot state.** The descriptor cache/cursor state is not currently
  `Saveable` (the B10 audit, `T8`). If the engine adds observable cursor state,
  decide explicitly whether it must round-trip; the VINO device has no
  `Saveable` impl today.
- **Interrupt ordering.** Keep DESC with EOF in the same ISR pass (`vinoEOD`
  requirement) and let the abort run — do not "fix" it with readback fakes.

## 6. References

- MAME `src/mame/sgi/vino.cpp` — `end_of_field`, `do_dma_transfer`,
  `push_fifo`, `count_pixel`, `merge_pixel`, `page_index_w`,
  `shift_dma_descriptors`, `load_dma_descriptors`, `next_desc_w`,
  `line_count_w`, `control_w`.
- MAME `src/mame/sgi/vino.h:121-127` — `DESC_PTR_MASK`, `DESC_VALID_BIT`,
  `DESC_STOP_BIT`, `DESC_JUMP_BIT`.
- IRIS `src/dev/vino.rs` — `render_and_pump`, `dma_emit_dword`,
  `descriptor_fetch`, `shift_descriptors`, `page_index_w`, `pump_field`,
  `read_channel_reg`.
- IRIS disassembly `vino.o` `0x77c0` / `0x60b4` / `0x7640` — recorded in
  `rules/irix/vino-capture-on-6.5-progress.md`.
