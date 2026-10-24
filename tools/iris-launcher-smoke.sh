#!/usr/bin/env bash
# Prove that a launcher starts iris from a clean environment (issue #104).
#
#   tools/iris-launcher-smoke.sh [LAUNCHER]
#
# LAUNCHER defaults to target/launcher/bin/iris, the copy the Nix dev shell
# installs; `nix run .#smoke` runs this against the same path, and a nix build
# result can be passed too (`nix run .#smoke -- result/bin/iris`).
#
# Every run is `env -i` with an explicit empty LD_LIBRARY_PATH, from a
# temporary directory with no iris.toml, so whatever the launcher needs it must
# provide itself. iris reads no environment variable for library lookup: on a
# plain cargo build the loader finds the libraries the usual way and none of
# this applies.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"

fail() { echo "launcher-smoke: FAIL: $*" >&2; exit 1; }

# Default to the checkout's installed launcher. `nix run .#smoke` runs this
# script from the store, where $REPO is not the checkout, so the invoking
# directory wins when it has one.
if [ $# -gt 0 ]; then
  LAUNCHER=$1
elif [ -n "${IRIS_LAUNCHER:-}" ]; then
  LAUNCHER=$IRIS_LAUNCHER
elif [ -x "$PWD/target/launcher/bin/iris" ]; then
  LAUNCHER=$PWD/target/launcher/bin/iris
else
  LAUNCHER=$REPO/target/launcher/bin/iris
fi

# Absolute path: `env -i` leaves PATH empty, so a bare name would not be found.
case "$LAUNCHER" in
  */*) ;;
  *) LAUNCHER="$(command -v -- "$LAUNCHER")" || fail "no '$LAUNCHER' on PATH" ;;
esac
[ -e "$LAUNCHER" ] || fail "no launcher at $LAUNCHER (run 'nix develop' to install it)"
LAUNCHER="$(cd "$(dirname "$LAUNCHER")" && pwd)/$(basename "$LAUNCHER")"
[ -x "$LAUNCHER" ] || fail "not executable: $LAUNCHER"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/iris-launcher-smoke.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# One clean run: an empty environment, an explicitly empty search path, and a
# directory with no iris.toml.
run_clean() {
  local err="$WORK/stderr"
  local out
  if ! out="$(cd "$WORK" && env -i LD_LIBRARY_PATH= "$LAUNCHER" "$@" 2>"$err")"; then
    echo "launcher-smoke: $(cat "$err")" >&2
    return 1
  fi
  printf '%s\n' "$out"
}

# --help parses and exits before a machine is built, but the dynamic loader has
# already resolved every DT_NEEDED by then, which is the failure this catches.
help="$(run_clean --help)" || fail "--help failed under env -i"
grep -q '^Usage: iris' <<<"$help" || fail "--help printed no usage line"
grep -q -- '--ci' <<<"$help" || fail "--help printed no option list"

# --print-instance is a real code path: it reads the config, derives the
# endpoints and exits. It needs no disk image and no IRIX media.
inst="$(run_clean --print-instance)" || fail "--print-instance failed under env -i"
grep -q '^ci_socket: ' <<<"$inst" || fail "--print-instance printed no ci_socket line"
grep -q '^monitor: ' <<<"$inst" || fail "--print-instance printed no monitor line"

echo "launcher-smoke: PASS: $LAUNCHER"
echo "launcher-smoke:       --help and --print-instance, no environment, empty LD_LIBRARY_PATH"
