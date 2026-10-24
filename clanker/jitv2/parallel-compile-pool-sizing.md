# The compile pool defaults to one worker; parallel compilation is cheap and bounded

The jitv2 compile pool runs a single worker by default: `default_jitv2_threads() -> 1`
(`src/config.rs:711`). The multi-worker machinery already exists — per-worker
`Codegen`, shared-arena reservation by a leader, a barrier that rebuilds the arena
while other workers park, and an N=4 concurrency test — but production never
turns it on.

## Evidence the single worker falls behind

The `COMPILE_QUEUE_CAPACITY` comment (`src/cpu/jitv2/jitv2.rs:169-175`) records a
live `j2 status` reading where the one compile thread "genuinely fell behind":
**20.9% of dispatches dropped** for a full queue (average depth 248.6/1024 across
1,166,218 dispatches in one session). The queue was doubled from 1024 to 2048 as a
stopgap — but that treats the symptom (a full ring), not the cause (a serialised
compiler). An IRIX boot triggers hundreds of thousands of region compiles, so
boot and process-start compile latency is FIFO-bound on one thread.

## Prior art on sizing

No major JIT uses `num_cpus` for compilation. The dynamic-language JITs scale
sub-linearly and cap:

| system | worker count |
|---|---|
| HotSpot (C1/C2) | `⌊1.5·log₂n·log₂log₂n⌋`, memory-capped — 4c→3, 8c→4, 16c→12 |
| Graal/Truffle | `min(n/4 + loglog n, 16)` — 4c→3, 8c→6, 16c→10 |
| wasmtime/Cranelift | `available_parallelism()` (rayon pool), *uncapped* |
| V8 (bytecode) | 1 background thread |

The dominant cost of parallelising is **per-worker compiler-arena memory**
(Cranelift's `CompilerContext` is "quite large"; wasmtime issue #10576), which is
why a cap matters — it bounds memory to a fixed small multiple. Correctness
machinery for N>1 is already in-tree and tested.

## Suggested change

Default to `available_parallelism().min(4)` (container-aware, unlike `num_cpus`),
keeping the existing `[jitv2] threads` knob as an override for high-core machines:

```rust
fn default_jitv2_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get().clamp(1, 4)).unwrap_or(1)
}
```

A cap of 4 is the conservative end of the HotSpot/Truffle range for 4–16-core
hosts, removes the documented queue backlog, and bounds arena memory. Do not go
uncapped (wasmtime's choice) — IRIS's self-modifying-code invalidation and bounded
memory goal favour the HotSpot/Truffle shape.

Measure boot wall-clock and `j2 status` queue depth before/after; correctness is
unaffected (compilation is async off the CPU thread).
