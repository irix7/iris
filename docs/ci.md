# Running IRIS in GitHub Actions

The `setup-iris` composite action installs `iris` and `iris-ci` and puts both on
`PATH`. A workflow needs one step:

```yaml
- uses: irix7/iris/.github/actions/setup-iris@main
- run: iris-ci --version
```

`@main` follows the fork's default branch. Pin a commit SHA instead when a
workflow must not change under it. The action is fork CI infrastructure; it is
not published to the Marketplace.

## Inputs

| Input | Default | Meaning |
|---|---|---|
| `version` | `latest` | Release to install: `latest`, `v0.1.6+irix7`, or `0.1.6+irix7`. Ignored in source mode. |
| `from-source` | `false` | Build the checkout at `path` instead of downloading a release. |
| `path` | `.` | Checkout to build when `from-source` is true. Relative paths are from the workflow workspace. |
| `features` | `lightning,gpujit` | Cargo features for a source build. |
| `install-deps` | `true` | On Linux, install the shared libraries the release links (libGL, ALSA, EGL, xkbcommon); in source mode also install the build packages. |
| `smoke` | `true` | Run `iris --help` and `iris-ci --help` after installing, from a clean environment on Linux and macOS. |

## Outputs

| Output | Value |
|---|---|
| `iris` | Absolute path to the installed `iris` binary. |
| `iris-ci` | Absolute path to the installed `iris-ci` binary. |
| `version` | The resolved release version, such as `0.1.6+irix7`; `dev.<short-sha>` in source mode. |

```yaml
- uses: irix7/iris/.github/actions/setup-iris@main
  id: iris
- run: |
    echo "IRIS ${{ steps.iris.outputs.version }}"
    "${{ steps.iris.outputs.iris-ci }}" --version
```

`iris` has no `--version` switch. The action reports the version from the
release tag or the GitHub API; `iris-ci --version` prints the version stamped
into the binary at build time, which matches the tag for a release.

## Release mode

Release mode needs no checkout, no Rust toolchain and no build. The action:

1. resolves `latest` through the GitHub releases API (using the workflow token
   when the rate limit is tight),
2. downloads the CLI asset for the runner,
3. extracts `iris` (or `iris.exe`) and `iris-ci` (or `iris-ci.exe`),
4. adds the install directory to `PATH`.

Release tags use SemVer build metadata, but GitHub rewrites `+` to `.` in asset
names. The action derives both names, so `v0.1.6+irix7` downloads:

| Platform | Asset |
|---|---|
| Linux x64 | `IRIS-cli-linux-x64-0.1.6.irix7.tar.gz` |
| Linux arm64 | `IRIS-cli-linux-arm64-0.1.6.irix7.tar.gz` |
| macOS (universal) | `IRIS-cli-macos-universal-0.1.6.irix7.tar.gz` |
| Windows x64 | `IRIS-cli-windows-x64-0.1.6.irix7.zip` |
| Windows arm64 | `IRIS-cli-windows-arm64-0.1.6.irix7.zip` |

Linux x64 and arm64, macOS (one universal binary) and Windows x64 and arm64 are
supported. Any other combination fails with a clear message instead of
installing the wrong binary.

On Ubuntu the release CLI links `libGL.so.1` and `libasound.so.2` directly.
`install-deps: true` installs them; on Ubuntu 24.04 the ALSA package is
`libasound2t64`, on older releases and Debian it is `libasound2`, and the action
tries the right one. EGL and xkbcommon are installed too because the GL and
window layers load them at run time. On a distribution without `apt-get` the
action warns and leaves the packages to the image.

Windows binaries come from a zip and are named `iris.exe` and `iris-ci.exe`.
The action's steps run under Git Bash, which the GitHub Windows runners provide;
callers should keep `shell: bash` for steps that use the installed binaries
unless they prefer to use the absolute output paths.

## Source mode

Source mode builds the checkout at `path`. The job must have a Rust toolchain:
`rust-toolchain.toml` pins the nightly, so `rustup show` installs the right one
on a GitHub runner. The action does not install a toolchain itself.

```yaml
jobs:
  build:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v5
      - name: Install the pinned toolchain
        run: rustup show
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: .
      - uses: ./.github/actions/setup-iris
        with:
          from-source: true
          features: lightning,gpujit
      - run: iris --help
```

When the checkout is not this repository, use the remote action reference and
point `path` at the IRIS checkout instead of `./.github/actions/setup-iris`.

`lightning,gpujit` is the recommended CI feature set: the fast interpreter and
every graphics JIT. `jitv2` is an experimental MIPS JIT and is not part of the
default here; add it only when the job is meant to exercise it. The distribution
builds also enable `idle-pause`; see Troubleshooting before adding it to a
ROM-only smoke.

`install-deps: true` installs `pkg-config`, `libasound2-dev` and `libgl-dev` in
source mode as well as the runtime libraries. If the job installs its own build
packages, set `install-deps: false` to skip the `apt-get` step.

The `version` input is ignored in source mode; the `version` output is
`dev.<short-sha>`, matching `scripts/dist/version.sh dev`.

## A complete workflow

This boots the embedded PROM with no IRIX media, uploads a serial transcript and
a framebuffer screenshot. It is two boots because the PROM's console and the
framebuffer are alternatives: `headless = true` gives serial output but no
REX3 framebuffer, while `--ci` without `--headless` keeps graphics alive
offscreen so `iris-ci screenshot` has something to capture.

```yaml
name: IRIS ROM smoke

on:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  prom:
    runs-on: ubuntu-24.04
    steps:
      - uses: irix7/iris/.github/actions/setup-iris@main
        id: iris
        with:
          version: latest

      - name: Serial transcript from a headless PROM boot
        env:
          # Release builds enable idle-pause; see Troubleshooting.
          IRIS_NO_IDLE: "1"
        run: |
          set -euo pipefail
          work="$RUNNER_TEMP/serial"
          mkdir -p "$work"
          cd "$work"
          # No SCSI targets: the default config attaches scsi1.raw, and a
          # missing disk image is fatal at startup.
          cat > "$work/machine.toml" <<'EOF'
          banks = [128, 128, 0, 0]
          headless = true
          no_audio = true

          [machine]
          profile = "indy_ip24"

          [scsi]
          EOF

          socket="$work/iris.sock"
          iris --config "$work/machine.toml" --ci \
            --ci-socket "$socket" --monitor-port 18888 \
            --serial-log "$RUNNER_TEMP/serial.log" >"$work/iris.out" 2>&1 &
          pid=$!
          trap 'kill "$pid" 2>/dev/null || true' EXIT

          # The CI socket is created only after the monitor binds. Poll it;
          # do not sleep. A fresh --ci machine answers while still `starting`.
          iris-ci --socket "$socket" status --wait-ready 30
          iris-ci --socket "$socket" start
          iris-ci --socket "$socket" serial-wait 'Option?' --timeout 60
          iris-ci --socket "$socket" quit
          wait "$pid"

      - name: Screenshot from an offscreen-graphics boot
        env:
          IRIS_NO_IDLE: "1"
        run: |
          set -euo pipefail
          work="$RUNNER_TEMP/screen"
          mkdir -p "$work"
          cd "$work"
          cat > "$work/machine.toml" <<'EOF'
          banks = [128, 128, 0, 0]
          no_audio = true

          [machine]
          profile = "indy_ip24"

          [scsi]
          EOF

          socket="$work/iris.sock"
          iris --config "$work/machine.toml" --ci \
            --ci-socket "$socket" --monitor-port 18889 >"$work/iris.out" 2>&1 &
          pid=$!
          trap 'kill "$pid" 2>/dev/null || true' EXIT

          iris-ci --socket "$socket" status --wait-ready 30
          iris-ci --socket "$socket" start
          # A screenshot has no wait-for-video-mode flag, so retry until the
          # PROM has set a mode. The first frame takes a few seconds.
          for _ in $(seq 1 30); do
            if iris-ci --socket "$socket" screenshot "$RUNNER_TEMP/screen.png" 2>/dev/null; then
              break
            fi
            sleep 1
          done
          if [ ! -s "$RUNNER_TEMP/screen.png" ]; then
            echo "no screenshot after 30 s; iris output follows"
            tail -20 "$work/iris.out" || true
            exit 1
          fi
          iris-ci --socket "$socket" quit
          wait "$pid"

      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: iris-prom
          if-no-files-found: warn
          path: |
            ${{ runner.temp }}/serial.log
            ${{ runner.temp }}/screen.png
            ${{ runner.temp }}/serial/iris.out
            ${{ runner.temp }}/screen/iris.out
```

Use a different `--monitor-port` for each machine, and a CI socket path in a
scratch directory. Each step `cd`s into its own directory under `$RUNNER_TEMP`,
so the NVRAM file, instance lock and socket stay out of the checkout. A `--ci`
machine can be driven while it is still `starting`: the CPU stays parked until
`iris-ci start` (or `machine-start` on the monitor) and the CI socket appears
only after the monitor has bound.

Both steps redirect the emulator's own output to `iris.out` in their work
directory. The artifact includes it: a boot that never reaches the menu usually
explains itself there (missing files, GIO timeouts), and that file is also why a
failed run does not fill the job log with device warnings.

## Serial console and wait cursors

`iris-ci serial-wait PATTERN` (and `serial-read`) consume the guest console
through a **per-consumer cursor**. A cursor only moves forward: a wait returns
the bytes from where that consumer last stopped through the match, and never
rewinds. Sequential waits on one consumer must therefore name patterns in the
order the guest prints them.

A line printed before the previous wait's match will not satisfy the next wait,
even though it is still in the transcript:

```sh
iris-ci serial-wait 'BOOT-READY'          # moves the default cursor past the line
iris-ci serial-wait 'module: loaded'      # times out if it was printed earlier
```

Give independent assertions their own consumer, so each starts at its own
cursor (a new consumer starts at the beginning of the buffer), or assert the
patterns in log order on one consumer:

```sh
iris-ci serial-wait --consumer boot   'BOOT-READY'
iris-ci serial-wait --consumer module 'module: loaded'
```

`--consumer` names the cursor only. The `--serial-log` file remains the full
transcript; consumers do not get separate files.

## Caching

Release mode downloads one archive of roughly 8 MB and does not cache it; the
download is cheaper than restoring a cache entry. Add an `actions/cache` step
keyed on the resolved version if a job needs to avoid the network entirely.

Source mode benefits from `Swatinem/rust-cache`, placed before the action so it
can save and restore `~/.cargo` and `target/`:

```yaml
- uses: Swatinem/rust-cache@v2
  with:
    workspaces: .
```

A first release build takes a few minutes on a hosted runner; later runs reuse
the cached dependency artefacts. The action itself does not cache anything.

## Troubleshooting

**The PROM menu never appears.** Release builds currently enable the
`idle-pause` optimisation. A no-disk PROM boot can park in a recognised idle
loop that is not woken, so the menu never prints; setting `IRIS_NO_IDLE=1` in
the machine's environment turns parking off and the boot completes. Source
builds with the recommended `lightning,gpujit` features do not enable
`idle-pause`.

**Startup fails with `could not attach scsi1.raw`.** The default machine
config attaches a disk and a CD-ROM. A CI machine with no media needs an empty
`[scsi]` table as in the workflow above.

**The serial log is empty.** When graphics are present, the PROM and IRIX use
the graphics console; the serial log mirrors ttyd1, not the screen. Set
`headless = true` (which also disables REX3) for a serial transcript.

**`screenshot: the display has no video mode yet`.** The PROM has not set a
video mode, or the machine is headless. Boot without `headless = true` and
retry the screenshot until it succeeds, as the workflow does.

**`iris-ci: connect ... No such file or directory`.** The CI socket is created
only after the monitor binds. Poll `iris-ci status --wait-ready N` instead of
sleeping before the first command. A `--ci` machine reporting `starting` is
ready to accept commands.

**`latest` cannot be resolved.** The action asks the GitHub API and falls back
to the `/releases/latest` redirect. Anonymous API calls are rate-limited; the
action passes the workflow token, which raises the limit. Off the runner, set
`IRIS_GITHUB_TOKEN` (or `GITHUB_TOKEN`) for the install script. Passing an
explicit `version` avoids the lookup entirely.

**A release asset is missing.** Use the exact version, without or with the
leading `v`: both `0.1.6+irix7` and `v0.1.6+irix7` work. Older releases have the
same asset layout. The tag keeps the `+`; only the asset name has `.`.

**The binary cannot load a shared library.** `ldd "$(command -v iris)"` lists
what is missing. On Ubuntu install `libgl1` and `libasound2t64` (24.04) or
`libasound2` (older), or leave `install-deps: true`. On a distribution without
`apt-get`, install the equivalent packages in the image first.

**Linux reports a missing `GLIBC_2.xx`.** Release binaries are built on Ubuntu
24.04, which is the glibc floor the release workflow targets. Run on
`ubuntu-24.04` or newer, or use `from-source: true` on an older image and build
against that image's glibc.

**A source build fails with no `cargo`.** The action does not install Rust.
Add `run: rustup show` (which honours `rust-toolchain.toml`) or
`dtolnay/rust-toolchain` before the action. `Swatinem/rust-cache` caches the
build without installing anything.

**`iris --version` fails.** It does not exist. Use the action's `version`
output, or `iris-ci --version` for the stamp in the binary.

**Windows steps cannot find the binaries.** The action appends the install
directory to `PATH` for later steps; if a later step cannot use `PATH`, use the
`iris` and `iris-ci` outputs instead. The Windows CI socket is TCP
(`127.0.0.1:19851` by default) rather than a Unix socket; pass
`--ci-socket host:port` when a step needs a specific port.

**macOS refuses to run a downloaded binary.** The CLI is signed (ad hoc when
the release job has no Developer ID credentials), and curl does not set the
quarantine attribute, so it normally runs. If a copy does carry it, clear it
with `xattr -d com.apple.quarantine iris iris-ci`.

## Files

| Path | Purpose |
|---|---|
| `.github/actions/setup-iris/action.yml` | The composite action. |
| `tools/install-iris-ci.sh` | The install logic; runnable by hand, with `--help`. |
| `.github/workflows/setup-iris-selftest.yml` | Self-test for release, source and macOS installs. |
