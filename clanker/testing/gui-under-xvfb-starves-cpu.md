# GUI reproduction under Xvfb/llvmpipe starves the emulator CPU thread

**Symptom.** Booting a PROM in `iris-gui` under `xvfb-run` with software GL, the
serial log advances a character every tens of seconds and the guest looks hung.
`regs` on the monitor shows the PC parked in the ROM's DRAM write/verify loop
(`0xBFC00504` in the IRIX 7 ROM), and `top -H` shows the `MIPS-CPU` thread at
0 % while llvmpipe threads hold ~250 % of a core. Under `--ci` the same image
boots in seconds.

**Cause.** The GUI's compositor renders the 1280x1024 Newport framebuffer with
llvmpipe on the host, and the guest CPU thread is starved behind it. The guest
is not trapped and the emulator has not lost the CPU; it is simply not being
scheduled often enough to finish the RAM test.

**Rule.** Do not use software-GL Xvfb as evidence for a graphics- or
timing-dependent emulator bug. Before calling a GUI hang a guest trap:

- read `regs` twice and check whether the PC advances;
- check the `MIPS-CPU` thread's CPU time in `top -H`;
- reproduce the graphics shape offscreen with `iris --ci` and the same JIT
  feature set, which is what `jit_full_screen_aux_clear_like_irix7` does for
  the auxiliary-plane clear in #108.

A window-only bug needs a real GPU/display to reproduce; under Xvfb the guest
is too slow to reach the code under test.

**Seen.** 2026-10-25, IRIX 7 v0.1.2 PROM, iris `0.1.3+irix7`, GUI built with
`premiere,hostgl,iris/jitv2`, `LIBGL_ALWAYS_SOFTWARE=1` under Xvfb.
