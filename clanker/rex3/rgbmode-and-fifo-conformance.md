# RGBMODE and GFIFO depth: what IRIS guarantees, and where it is looser than an Indy

Status: current, 2026-10-24. Written for irix7/iris#107.

The OS REX3 driver depends on two hardware behaviours: `DRAWMODE1` bit 15
(`RGBMODE`) is required in addition to RGB plane selection, and the command
FIFO is shallow enough that it must be drained between primitives. Both are now
pinned by tests against the emulator's public register interface. The
deliberate divergences are in `docs/Indy/rex3.md`, section "Where IRIS diverges
from real hardware".

## RGBMODE

The two fields are independent: `PLANES` (bits 2:0) picks the framebuffer,
`RGBMODE` (bit 15) picks RGB versus CI shading.

IRIS matches the hardware requirement. With `PLANES=RGB`, `DRAWDEPTH=24` and
bit 15 clear, the shader is `get_colori()` = `COLORRED >> 11`; `COLORGRN` and
`COLORBLUE` are never read, and the index is written as the whole 24-bit pixel.
BGR packing puts it in the red byte, so `(r,g,b) = (0x80,0x40,0x20)` produces
`0x000080`, not `0x204080`. With bit 15 set, `get_colori()` composes
`blue<<16 | green<<8 | red` and the fill lands all three channels. Confirmed by
`rgb24_fill_needs_rgbmode_as_well_as_rgb_planes`; the JIT agrees
(`jit_rgbmode_fill_matches_interp`).

Also load-bearing: `COLORRED`'s write format depends on RGBMODE and DRAWDEPTH.
`from_color_red` shifts o12.9 into o12.11 only for 12-bit CI mode. Program
`DRAWMODE1` before the colour registers.

Divergence, and it is the *reset* state: spec Table 11 gives DRAWMODE1 the init
value `PLANES=001`, `RGBMODE=1`, `FASTCLEAR=1`, `LOGICOP=SRC`, `COMPARE=0x7`.
`Rex3Context::power_on_default` only sets `COMPARE=0x7`, so the register reads
`0x7000`, and `power_on` also leaves `WRMASK` at 0. Software must program both;
a guest that leans on the hardware reset defaults draws nothing on IRIS.

Gotcha found while writing the reset test: **do not call `power_on` on a
started board.** It replaces the whole `Rex3Context`, including the `host`
back-pointer that `start()` installs, so `draw()` sees `ctx.host.is_null()` and
every primitive silently does nothing. `Machine::reset` is safe because it
stops first and restarts peripherals afterwards; the tests use
`make_rex3_reset` (power-on with the threads stopped, then start).

Secondary reset surprise: `CLIPMODE` resets to `CIDMATCH=0`, which permits no
CIDs, so after a reset nothing draws until CLIPMODE is programmed to
`0xF << CLIPMODE_CIDMATCH_SHIFT` (or the caller writes a permit mask). The
hardware's Table 16 init is also 0x0, so this one is not a divergence — it is
just easy to miss when writing a test.

## GFIFO

Emulated depth is `GFIFO_DEPTH = 65536` entries (65535 live; the ring keeps one
slot empty to tell full from empty). The hardware's is 32 (spec §3.11).

STATUS is computed in `Rex3::bus_read32`:

```
level = if pending == 0 { 0 } else { max(1, pending - (65536 - 32)) }
```

so the 6-bit field shows only the last 32 slots:

| queued | GFIFOLEVEL |
|---|---|
| 0 | 0 |
| 1 … 65505 | 1 |
| 65506 | 2 |
| 65535 (full) | 31 |

`GFXBUSY` is exact: set while the pipeline is busy or the queue is non-empty,
which is the spec's definition and what `REX3WAIT` polls. It is the supported
flow-control signal.

Overflow is back-pressure: `gfifo_try_push` fails and `write32` returns
`BUS_BUSY`, the CPU re-executes the store, nothing is lost. Hardware instead
stalls with `GRXDLY` at `CONFIG.GFIFODEPTH` and drops the write at level 32.

Not modelled at all: `STATUS.GFIFO_INT` / `FIFO_INT_N`. `update_gfifo_irqs`
exists but every call site is commented out, so `CONFIG.GFIFODEPTH` and
`CONFIG.GFIFOABOVEINT` are stored and read back and do nothing else. A driver
waiting on the threshold interrupt would hang here.

Consequence for the OS: omitting the drain between primitives works on IRIS up
to almost 65536 queued entries, so it is invisible in emulation and can still
lose writes on an Indy. That is the "more permissive" half of #107, and it is
pinned rather than fixed: making the queue 32 deep would impose a per-primitive
stall the emulator does not need.

## Tests

All in `src/dev/ng1/rex3_tests.rs`:

| test | pins |
|---|---|
| `reset_leaves_drawmode1_planes_disabled` | reset DRAWMODE1 = 0x7000, WRMASK = 0, and that a draw before programming PLANES writes nothing |
| `rgb24_fill_needs_rgbmode_as_well_as_rgb_planes` | RGBMODE clear → red byte only (0x000080); set → 0x204080 on every pixel; COLORI readback follows |
| `status_reports_only_the_last_32_fifo_slots` | 65535-entry capacity, level mapping 0/1/2/31, BUS_BUSY at the brim with no loss |
| `fifo_threshold_interrupt_stays_clear` | CONFIG threshold fields stored, GFIFO_INT never raised |
| `back_to_back_fills_need_no_status_poll` | 64 fills (384 entries) with no STATUS read or drain, all correct after one drain |
| `jit_rgbmode_fill_matches_interp` (`rex-jit`) | Cranelift and interpreter agree in both RGBMODE states |

Harness pieces: `make_rex3_unstarted` (no consumer, so queue depth is
observable), `FifoResetGuard` (emptiest the ring before `TestBoard::drop`,
whose `stop()` pushes a sentinel with a *blocking* push — a full queue with no
consumer would hang), `read_status_now` (STATUS without the `read_reg`
wait-idle gate), `make_rex3_reset`.

## Not verified

No hardware capture was available. The claims above come from `rex3.pdf` Tables
7, 11, 16, 17 and §3.11, plus IRIS's observable behaviour. In particular the
exact hardware result of a 24bpp CI-index write (index in the red byte versus
some other truncation) is inferred from the spec's field layout and matches the
issue's description; it has not been measured on an Indy. The 32-versus-64
entry ambiguity in §3.11 ("enlarged to 32 deep, doubles") is left as the spec
states 32; IRIS's `GFIFO_HW_DEPTH` is 32.
