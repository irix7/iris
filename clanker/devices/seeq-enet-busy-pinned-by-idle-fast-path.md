# seeq-enet spins at 100% after one frame: `busy` is never cleared on the idle path

## Symptom

An otherwise idle IRIX guest pins one host core in the `seeq-enet` thread.
`top -H` shows `seeq-enet` at ~100% and `MIPS-CPU` at ~100% (before idle
parking), which is the ~200% host CPU an idle 6.5 instance used to burn. With
idle parking fixed, the CPU thread drops to ~2% but `seeq-enet` stays at
~100%, so the process still burns a core.

## Cause

The `seeq-enet` loop sets `busy = tx_moved || rx_moved` at the end of a pass,
and skips its `rx_wake.wait(...)` when `busy` is true (so a burst of frames is
pumped straight through rather than paced at one frame per timeout). But the
"nothing moved" fast path was:

```rust
if tx_result == Nothing && rx_result == Nothing {
    continue;   // skips the `busy = ...` assignment below
}
```

After the first pass that sends or delivers a frame, `busy` stays `true`
forever: every later no-op pass hits the fast path and `continue`s, so the
thread spins on `pump_tx`/`pump_rx` at 100%.

## Fix

Clear `busy` before the fast-path `continue` (`src/dev/seeq8003.rs`). The next
pass then waits on `rx_wake` with its 50 ms backstop again.

## Evidence

`callisto`, IRIX 6.5 IP22 at a getty, `top -H` over 3 s:

| thread | before | after |
|---|---|---|
| `seeq-enet` | ~99% | ~0% (S, sleeping) |
| total process | ~200% | ~90% with parking off, 2% with parking on |

This is independent of idle parking and of the CPU clock model.
