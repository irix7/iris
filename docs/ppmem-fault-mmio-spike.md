# Fault-based MMIO — viability spike (issue #52)

Status: **spike complete. Decision: NO-GO** for the "trap device pages and
delete the branch" design as scoped. The stale-code path (`physical.rs`'s
`DecodeMap` + `jitv2`'s memory callouts) stays the correctness fallback.

Companion artefacts:

- prototype: `src/faultmmio.rs`, gated behind the off-by-default `faultmmio`
  feature (`Cargo.toml`). It is **not** wired into `Physical`, `jitv2` or any
  execution path; enabling it changes nothing about how the emulator runs.
- invariant: §7 below, and `rules/ppmem/fault-mmio-restartability.md`.

Prior art this builds on: `docs/ppmem-design.md` §11 (the tabled idea),
`docs/research/b03-softtlb-fastmem-decode.md` (QEMU / Dolphin / MAME /
wasmtime survey), `docs/research/attempts/b03-attempted.md` (what has and has
not been attempted in-tree).

---

## 1. What was asked

Issue #52 asks whether MMIO can be serviced through host page faults behind
`jitv2`:

1. map device pages `PROT_NONE` inside the existing 4 GiB ppmem window;
2. install validated signal/exception handlers;
3. service a fault by **re-executing the guest instruction** (Dolphin's
   patch-and-retry shape), *not* by decoding host machine code;
4. measure the MMIO trace cost on boot/desktop;
5. write the fault/restartability invariant.

The deliverable is a go/no-go decision, not the feature.

---

## 2. The mechanism, and why it does not pay

### 2.1 Device pages are already trapped

The ppmem 4 GiB window (`src/ppmem/ppmem.rs`, `PpMemSpace`) is a `PROT_NONE`
reservation into which only RAM banks are mapped. Every device aperture — MC,
HPC3, PROM, Newport, the GIO slots — is *already* unmapped, i.e. already
`PROT_NONE`. Nothing needs "mapping trapped": the invitation's step 1 is
half true today. What is missing is a handler and a reason to fault.

### 2.2 The branch is not where the cost is

In `jitv2`, RAM accesses whose L1D line is cached are already emitted as plain
host loads/stores. Everything else — uncached RAM, cache-line fills, and all
MMIO — already calls back into Rust (`src/cpu/jitv2/codegen.rs`'s callouts;
`Physical::read*/write*` in `src/physical.rs:1124-1354` does one `u64` bitmap
test and then either a direct host access or the `device_map` dispatch).

So fault-based MMIO would not remove work from the RAM path in any meaningful
way. It would only replace the *MMIO* callout/dispatch with a host fault plus a
handler. That is a straight substitution of one mechanism for another, and the
handler is far more expensive than the callout (§5).

The design doc's stronger claim — "the `device_map` lookup plus the
`*const dyn BusDevice` virtual dispatch vanish from **every** access" — is only
true if `jitv2` emits a plain host access for device addresses too. It cannot
do that safely unless a handler catches the resulting fault, and then every
single MMIO access faults.

### 2.3 Re-execution alone cannot amortise

"Re-execute the guest instruction on fault" (Dolphin's shape) does not by
itself make faults cheap. A re-execution per access means **one host fault per
MMIO access**, forever. To pay once per *site*, the handler must patch the
faulting host instruction into a slow-path call — Dolphin's
`Jit64::BackPatch`, which overwrites the faulting instruction with
`JMP trampoline` and rewinds the instruction's side effects
(`Source/Core/Core/PowerPC/Jit64/Jit.cpp:166-254`).

For IRIS that needs `jitv2` to:

- register per-access trap sites at codegen, mapping host PC → (guest PC,
  access kind, width, destination register) — wasmtime's
  `lookup_code`/`lookup_trap_code` discipline;
- emit each potentially-faulting access in a form that leaves room for a patch
  and is safe to rewrite live;
- make partial stores restartable (§7).

That is a substantial, risky change to the hottest code in the emulator, and
its end state is a trampoline call that costs about what the existing callout
already costs. The expected steady-state win is close to zero; the expected
first-fault and patching hazards are real.

### 2.4 The pattern is already covered

The branchy `device_map` path and the `jitv2` callouts already service MMIO
correctly and cheaply, on all hosts, with no signal-handler semantics, no
debugger interference and no portability matrix. Fault MMIO has to beat that by
the margin in §5 to be worth anything; it does not.

---

## 3. Prototype notes

`src/faultmmio.rs`, built only with `--features faultmmio`:

- `classify_fault(pc, addr, arena, sites)` — the validation predicate: a fault
  is ours only if the *address* is in the trapped arena **and** the *PC* is a
  registered host-code site. This is the wasmtime gate; without it a wild
  pointer in the emulator is silently serviced as MMIO.
- `crosses_page` / `plan_store` — the restartability decomposition: a store
  spanning a page boundary is split into single-page, all-or-nothing commits,
  so the piece that can fault is committed last.
- A Linux x86-64 `SIGSEGV` handler (`sa_sigaction`,
  `SA_SIGINFO | SA_ONSTACK`) that services a validated fault and **forwards**
  an unvalidated one by restoring `SIG_DFL`, unblocking and re-raising.
- Tests (10, all green) covering: the validation predicate's three cases; the
  boundary decomposition; a real serviced fault; a real forwarded fault (the
  child dies by `SIGSEGV`, proving no swallowing); a naive boundary store that
  tears; a validated boundary store that does not; and the latency
  measurement.

What the prototype deliberately does **not** do: it does not decode host machine
code, does not touch `jitv2`, and does not re-enter the guest. It proves the
mechanism is implementable and the invariant is testable; the measurement in §5
is what rules the feature out.

---

## 4. The measurement that matters

Run the latency test (prints under `--nocapture`):

```
nix develop --command bash /mnt/europa/iris-worktrees/run.sh \
  test -p iris --lib faultmmio --features faultmmio -- --nocapture --test-threads=1
```

Single-threaded, three runs on the development host:

| | host fault round trip | branchy virtual bus dispatch | ratio |
|---|---:|---:|---:|
| run 1 | 21 126 ns | 15.62 ns | 1 353× |
| run 2 | 32 996 ns | 30.28 ns | 1 090× |
| run 3 | 20 189 ns | 29.09 ns | 694× |

"Round trip" is `t0` taken in the forked child immediately before the faulting
volatile load, to `t1` at entry to the `SIGSEGV` handler. It therefore includes
signal delivery and any preemption between the load and handler entry; it is an
upper bound on the pure fault cost and a lower bound on nothing. Even the most
generous reading is two to three orders of magnitude above the dispatch it
would replace.

The raw numbers are noisy (parallel test execution pushed one run to ~250 µs),
so treat the *ratio*, not the absolute, as the finding: **a fault costs roughly
700–1500 branchy dispatches.**

### 4.1 The break-even, and the trace I could not take

If a fault costs `F` and a bus dispatch `D`, per-access fault servicing wins
only while the MMIO fraction `p` satisfies

```
p · F  <  D      ⇒      p < D/F  ≈  1/1000 … 1/700
```

i.e. **MMIO must be well under ~0.1 % of all memory accesses** to break even —
and that is before counting the `jitv2` per-site metadata, the code padding and
the restartability machinery. With Dolphin-style backpatching the per-access
fault becomes a one-off, but the steady-state trampoline cost is comparable to
the existing callout, so the ceiling on the win stays near zero.

I could not take a boot or desktop MMIO trace: this spike had no IRIX media or
PROM-boot instrumentation, and `cargo test` cannot boot the guest. **Say so
plainly rather than imply a measurement that was not made.** The relevant prior
belief, from `docs/ppmem-design.md:600-608`, is that MMIO is *not* rare — REX3
and HPC3 DMA registers are hit hard during X11 — and PROM POST is almost
entirely device programming. Both are orders of magnitude above the 0.1 %
break-even, which is why the decision does not depend on the missing trace: a
workload would have to be ~1000× more RAM-heavy than a boot or an X11 session
for the trade to invert. `[Q2]` below records the trace as the one piece of
evidence that would reopen the question.

---

## 5. Portability and operational costs

- **Three unrelated mechanisms.** Linux `SIGSEGV`/`SIGBUS`; macOS Mach
  exception ports (not signals — wasmtime uses these); Windows vectored
  exception handling. IRIS runs on all three. Only the Linux path is
  prototyped here.
- **Debuggers stop on every `SIGSEGV`.** Exactly the debuggability trade the
  project already refuses for `lightning`/`opcodefusion`, and it would break
  the GDB stub's expected signal behaviour.
- **Async-signal-safety.** Handlers can only touch atomics, raw memory and the
  resumption path. Interoperating with `jitv2`'s existing trap handling and the
  compile pool's state is non-trivial.
- **The window is a shared resource.** Faulting through it interacts with
  `remap_banks`/`clear_mappings` and the per-page generation counters.

---

## 6. The restartability / undo invariant

This is the durable output, independent of the go/no-go.

> **Any store that can partially complete before faulting must
> compute-then-commit or record undo.**
>
> A guest store may be wider than the gap to a page boundary, and the host may
> commit the words on the present page before faulting on the trapped page.
> Re-executing that guest instruction after servicing the fault would then
> commit the present page's bytes a second time — a torn write that no
> retry can repair. Every such store must either
>
> - be decomposed so each committing step lies wholly within one page and is
>   all-or-nothing (compute-then-commit), with the possibly-faulting piece
>   committed last; **or**
> - record enough undo information at codegen (the original bytes, and the
>   destination register/offset) for the handler to roll the partial write back
>   before retrying.
>
> The same rule applies to `jitv2`'s own non-atomic stores today, independent
> of fault MMIO: `PpMemory::write64_masked` is a read-modify-write of a single
> qword (`src/ppmem/ppmem.rs:602-611`) and `swap_word_halves_store` writes a
> multi-word block (`src/ppmem/ppmem.rs:443-453`). Neither currently crosses a
> device mapping, but the invariant should be honoured before either is allowed
> to touch one.

The prototype's `naive_boundary_store_can_tear` and
`validated_boundary_store_is_atomic` tests are the executable form of this
invariant.

---

## 7. Decision

**NO-GO.**

Fault-based MMIO, for this emulator, on these hosts:

1. **does not remove the branch** unless `jitv2` emits plain accesses for
   device addresses, which just moves the cost from a callout to a fault;
2. **cannot amortise** without a Dolphin-style registered-site backpatcher, a
   large risky change whose steady state costs about what the existing callout
   already costs;
3. **costs ~700–1500× a dispatch per fault**, so it needs MMIO below ~0.1 % of
   accesses, which boot and X11 are not;
4. **triples the portability surface** and degrades interactive debugging.

Keep `physical.rs`'s `DecodeMap` and the `jitv2` callouts as the serviced path.
Write the invariant (§6) down so a future attempt — and the existing
`write64_masked`/`swap_word_halves_store` stores — inherit it.

### Conditions to revisit

Reopen only with **both**:

- a real boot/desktop MMIO trace showing MMIO below the break-even, and
- a decision to build `jitv2` registered-site metadata and a backpatcher for
  its own sake (e.g. to delete the RAM bitmap branch), with fault MMIO as a
  rider rather than the reason.

`[Q2]` remains open: the boot/desktop MMIO trace was not obtainable in this
spike.

---

## 8. How to reproduce

```
# default check (feature off: nothing in this spike is compiled)
env CARGO_TARGET_DIR=/mnt/europa/iris/target nix develop --command cargo check -p iris

# feature check + prototype tests
env CARGO_TARGET_DIR=/mnt/europa/iris/target nix develop --command \
  cargo check -p iris --features faultmmio
nix develop --command bash /mnt/europa/iris-worktrees/run.sh \
  test -p iris --lib faultmmio --features faultmmio -- --nocapture --test-threads=1
```

Observed: `cargo check -p iris` clean; `cargo check -p iris --features
faultmmio` clean; prototype suite `10 passed; 0 failed`.
