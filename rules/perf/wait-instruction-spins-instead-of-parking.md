# WAIT is implemented but spins; route it through the idle-park instead

`exec_wait` (`src/cpu/mips_exec.rs:7465`) already implements WAIT: it stalls the
CPU until an unmasked interrupt or soft-reset arrives, pacing `hot.cycles` at the
wall-clock rate so guest time and the WD33C93A deferred-interrupt spin stay
correct. It is decoded for R4600/R5000/R10000 (`FUNCT_WAIT`,
`mips_isa.rs:167`); on R4400 the function is a no-op.

But the stall is a **spin**: the loop ends in `std::hint::spin_loop()` (`7533`),
re-reading `core.hot.interrupts` in a hot loop. A guest that actually reaches WAIT
therefore still pins a host core at 100%, even though the CPU is architecturally
"halted until interrupt" — the same problem the idle-park detector
(`src/cpu/idle_park.rs`) solves for the *detected* idle loop, but WAIT never
participates in it.

## Scope

IRIX itself never executes WAIT — its `wait_for_interrupt_fix_loc` requires the
`mtc0 a1, C0_SR` that re-enables interrupts to be *adjacent* to the WAIT, and IRIX
does not do that, so it spins the kernel idle loop instead (which idle-park
catches). The WAIT spin only bites on guests that do use WAIT (OpenBSD/NetBSD
boot, or an R10000 guest), but for those it defeats idle-pause entirely.

## Suggested change

Replace the `spin_loop` hot loop with the same park primitive idle-park uses:
compute the next interrupt/Compare deadline, `park_timeout` until it (or an
interrupt write unparks it), and advance `hot.cycles` by the elapsed wall-clock on
wake — preserving the existing pacing logic and the `EXEC_RETRY` guard for
unreleasable WAIT. The Dekker wake already wired into `ioc.rs`/`set_interrupt`
covers the wake path.

Keep the pacing exactly as-is (10 ns/guest-cycle sampled off `Instant`); the only
thing that changes is that the thread sleeps instead of spinning between
observations.

## Sources

- QEMU models HLT/WFI as `EXCP_HLT` → `qemu_cond_wait` (`accel/tcg/cpu-exec.c`,
  `system/cpus.c`), woken by `qemu_cpu_kick`. This is the direct analogue: a
  real halt instruction should block, not spin.
- 86Box `opHLT` blocks to the main loop and re-executes HLT next slice.
- See `rules/perf/idle-pause-work.md` for the detector this should reuse.
