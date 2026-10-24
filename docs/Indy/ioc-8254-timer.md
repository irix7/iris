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
(`RW`, `M2..M0`, `BCD`). D6 is set by the control or count write and clears
once the count reaches the counting element. IRIS clears it when the channel
is armed, so a counter with no input clock keeps D6 set — the test for "not
clocked" rather than "stuck". The OUT bit is coarse in IRIS: high while a
mode 2/3 channel is armed, low otherwise. GATE inputs and the output waveform
are not modelled.

## Resolution and time base

IRIS derives every channel deadline from the guest cycle counter
(`hot.cycles`) and never from the host clock:

- One input tick is 1 µs at the 1 MHz counter-2 input, which is **100 guest
  cycles** at the emulator's 10 ns per retired cycle.
- A read truncates to whole ticks; 16 bits gives a maximum period of 65.535 ms
  before the counter reloads.
- The per-tick nanosecond value is `1e9 / input_freq` in integers, so a
  counter-2 reload that does not divide 1 MHz evenly rounds the tick to whole
  nanoseconds (the 1 MHz case itself is exact).
- Deadlines are absolute (`deadline + period`), so there is no per-tick
  accumulation error. A 100 Hz counter produces exactly 1000 interrupts in
  10 s of guest time.

## It is not an independent reference

This is the part that matters to a guest timing probe.

- CP0 Count is derived from the **same** `hot.cycles` clock, scaled by
  `count_hz` (33 MHz by default). The PIT-to-Count ratio is therefore a
  constant chosen by the emulator — 33 Counts per PIT tick at the default —
  and calibrating one against the other measures that constant, not the
  machine. It cannot detect drift, and it will always pass.
- Against host wall time the rate is undefined while executing: one guest
  cycle is one retired instruction, so a PIT second is however long it takes
  the host to retire 100 million instructions. Only while the CPU is parked
  idle (or stalled in `WAIT`) does the cycle clock advance at 10 ns per host
  nanosecond.
- The only host-referenced clock the guest has is the battery-backed DS1386
  RTC, at 10 ms (hundredths of a second) resolution. That can bound
  long-term drift, but it cannot calibrate a microsecond timer.

IRIS therefore offers **no independent microsecond timing source**. An OS
that needs to calibrate against real time must use something outside the
emulated timing path — NTP over the emulated network, or the RTC for a coarse
check. Calibrating CP0 Count against the PIT is circular and should be
skipped.

## Diagnostics

`pit status` prints the reference, the clock chain and each channel's mode,
reload, live count, input frequency and armed/null state. It is the quickest
way to tell a counter that is not clocked from one that is counting.
`pit debug on` traces register reads and writes.

The model is in `src/dev/pit8254.rs`; the shared guest-time queue it arms
deadlines on is in `src/cpu/guest_timer.rs`.
