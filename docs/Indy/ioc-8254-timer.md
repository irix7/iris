# IOC 8254 Programmable Interval Timer

The Indy's IOC (IP24 "Guinness"; Indigo2 IP22 "Full House") embeds an
Intel 8254-compatible programmable interval timer. It is the machine's tick
source: IRIX uses counter 0 for its 100 Hz system clock and counter 1 for its
1 kHz fast clock.

## Registers

| Counter / register | Indy IOC PIO6 offset | Indigo2 INT2 offset |
|---|---|---|
| Counter 0 | `IOC_BASE + 0xB0` (`0x1FBD98B0`) | `INT2 + 0x30` |
| Counter 1 | `+0xB4` | `+0x34` |
| Counter 2 | `+0xB8` | `+0x38` |
| Control word | `+0xBC` | `+0x3C` |

Both address windows reach the same chip. Each register is byte-wide but
occupies a dword-aligned slot (`offset & !3`), as on the rest of the PBUS PIO
space. See [the interrupt map](../interrupt_map.md) for the surrounding IOC
registers.

## Clock wiring

- Counter 2 is clocked at 1 MHz, from a divide-by-20 divider in the IOC.
- **Counter 2's OUT clocks counters 0 and 1.** They have no independent clock
  input.
- Terminal counts of counters 0 and 1 drive the IOC timer interrupt logic.

The consequence is easy to miss: programming counter 0 on its own does not
make it count. Counter 2 must be a rate generator first — for example mode 2
with a reload of 1, which passes the 1 MHz clock through unchanged. On real
hardware and in IRIS alike, an unclocked counter holds its loaded value
forever and looks dead.

## Programming, as modelled by IRIS

Control word: `SC1 SC0 RW1 RW0 M2 M1 M0 BCD`.

- `SC` selects a counter; 3 selects the read-back command.
- `RW`: 0 = counter latch command, 1 = LSB only, 2 = MSB only, 3 = LSB then
  MSB.
- Modes 2 (rate generator) and 3 (square wave) are modelled as periodic
  clocks. Modes 0/1/4/5 are also armed periodically, which is an
  approximation; IRIX's timer drivers use mode 2.
- BCD is recorded and reported in the status byte, but counting is always
  binary.
- A reload of 0 is treated as "disarmed"; the chip would count 65536.

Reading:

- **Counter latch command** (`RW = 0`) captures the count; the next one or two
  reads return it and release the latch. A second latch before the value is
  read is ignored.
- **Read-back command** (`SC = 11`): `D5 = 0` latches the selected counters'
  counts, `D4 = 0` latches their status, and `D3/D2/D1` select counters 2/1/0.
  If both are latched, the status byte is read first, then the count.
- Reading a running counter as two bytes without latching first can tear,
  because the high byte is recomputed from the live count. Latch first.

Status byte: `OUT` (D7), null count (D6), then the programmed mode word
(`RW`, `M2..M0`, `BCD`). The OUT bit is coarse in IRIS: high while the channel
is armed and counting, low otherwise. Null count is not modelled, so D6 stays
clear; a channel with no input clock is identified by its zero input frequency
and unarmed state instead. GATE inputs and the output waveform are not
modelled.

## Resolution and time base

Each channel is armed as a recurring host timer on the `hptimer` thread
(`src/hptimer.rs`) for one full period (`reload / input_freq`), and a read
interpolates the remaining count from a host `Instant` and the channel's
period. Consequences:

- One input tick is 1 µs at the 1 MHz counter-2 input.
- A read truncates to whole ticks; 16 bits gives a maximum period of 65.535 ms
  before the counter reloads.
- The per-tick nanosecond value is `1e9 / input_freq` in integers, so a
  counter-2 reload that does not divide 1 MHz evenly rounds the tick to whole
  nanoseconds (the 1 MHz case itself is exact).
- Delivery is host-timed, so it jitters with host scheduling. It does not
  accumulate error over a period: each period re-arms from the current host
  instant rather than by adding ticks.

## Host reference, and what may be calibrated against it

The PIT is a host-timer device. In the default build CP0 Count is also
host-anchored, at `count_hz`, so a guest that compares the two is measuring
the emulator's chosen ratio plus the jitter of two different host paths. That
is useful as a sanity check but is not evidence of hardware accuracy, and the
ratio is not exact.

Under `--features ci_clock` Count is derived from retired instructions while
the PIT remains host-timed. The two then diverge by design: PIT time tracks
the host, Count tracks executed instructions. A guest timing probe that
assumes they agree will see that difference.

The only guest-visible clock that is independent of both is the battery-backed
DS1386 RTC, at 10 ms (hundredths of a second) resolution. That can bound
long-term drift, but it cannot calibrate a microsecond timer. An OS that needs
to calibrate against real time should use something outside the emulated
timing path — NTP over the emulated network, or the RTC for a coarse check.

## Diagnostics

`pit status` prints each channel's mode, reload, live count, input frequency
and armed state. It is the quickest way to tell a counter that is not clocked
from one that is counting. `pit debug on` traces register reads and writes.

The model is in `src/dev/pit8254.rs`; the host timers it arms are in
`src/hptimer.rs`.
