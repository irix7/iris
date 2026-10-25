# Testing and benchmarking

The suite commands and benchmarking guidance previously in README.md.
Build-feature and configuration combinations are listed in
[FEATURES.md](FEATURES.md); outstanding functionality is tracked in
[TODO.md](clanker/TODO.md).

## Testing and benchmarking

Two bare-metal MIPS suites run on the emulated CPU with no operating system in
the way. They answer different questions and neither replaces the other.

**`cpu-tests/`** — is this instruction correct? ~250 self-checking tests over
ALU, FPU, TLB, caches, exceptions and the MIPS IV additions, one instruction at
a time with clean state. The expectations are validated on real Indys (R4400
and R5000, both passing every check); see
[cpu-tests/README.md](cpu-tests/README.md).

```sh
sudo apt-get install gcc-mips-linux-gnu binutils-mips-linux-gnu   # or: make -C cpu-tests toolchain-local
make -C cpu-tests && make -C cpu-tests run
cpu-tests/run/matrix.sh                 # R4400/R5000 x interp/jitv2
```

**`bench/`** — how fast is this build, and is it still right after ten million
of them? 46 kernels covering integer, FPU, the cache hierarchy, image and video
editing inner loops, compression, and the emulator-only paths (TLB refill,
exception round trip, cache maintenance, uncached I/O). Every kernel checksums
its result against a golden value computed by building the same C natively, so
each run reports an accuracy percentage next to its throughput — and per-kernel
**guest instructions per host second**, which is directly comparable between the
interpreter and jitv2.

```sh
cargo build --release --bin iris-bench
./target/release/iris-bench run         # ~60 s; --quick for about half that
./target/release/iris-bench run --quick

make -C bench && make -C bench hostbench            # only to change the suite
./target/release/iris-bench matrix      # builds and runs every CPU x engine cell
./target/release/iris-bench host        # the same kernels, natively, for the ratio
```

`run` needs **no MIPS toolchain and no build step**: a known-good guest binary
is checked in at `bench/prebuilt/` and linked into `iris`, and the run happens
in-process on a headless machine the emulator builds for itself. That is also
what the GUI's **Benchmark tab** does, on every platform and inside the App
Store sandbox — one button, and the accuracy score sits next to the speed.

Includes Dhrystone 2.1 (DMIPS) and LINPACK 100x100 (MFLOPS), so an emulated
Indy can be put next to published figures for a real one, plus a Whetstone mix
(reported in passes/s — see bench/README.md for why not MWIPS). See
[bench/README.md](bench/README.md) — and
[clanker/testing/benchmark-suite-gotchas.md](clanker/testing/benchmark-suite-gotchas.md)
before adding a kernel.

`bench/irix/` is the other half: real workloads under a booted IRIX (filesystem,
buffer cache, IRIX's own tools, X on REX3), driven over `iris-ci`.


## The Nix launcher

On a Nix host the dev shell installs `target/launcher/bin/iris`, which carries
the build's own library search path (see
[HELP.md](HELP.md#running-from-the-nix-dev-shell)). `test/journal/run.sh` and
`test/fleet/run.sh` use it when it exists, so neither test needs an
`LD_LIBRARY_PATH` of its own.

Check the launcher from an empty environment, the way a CI job would start it:

```sh
tools/iris-launcher-smoke.sh                  # or: nix run .#smoke
tools/iris-launcher-smoke.sh result/bin/iris  # a packaged build
```

It runs `--help` and `--print-instance` under `env -i` with an explicitly empty
`LD_LIBRARY_PATH`, from a directory with no `iris.toml`, and fails if either
cannot start. `nix flake check` runs the same idea hermetically against a stub
binary, so a library missing from the flake's `runtimeLibs` — or an `RPATH` that
does not name it — turns the check red without needing a cargo build.

For GitHub Actions, [docs/ci.md](docs/ci.md) covers the `setup-iris` composite
action and a media-free ROM smoke workflow.
