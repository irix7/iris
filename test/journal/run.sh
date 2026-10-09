#!/usr/bin/env bash
# Record/replay journal CI test (issue #47) — the real headless run-loop path.
#
# Boots one headless instance to the Indy PROM command monitor (no IRIX media),
# snapshots the machine there, then records a guest-cycle journal from the
# snapshot and replays it. `iris-ci journal-replay` exits non-zero unless the
# replay's event stream is byte-identical to the recording, so the replay
# equality is the assertion.
#
# What this exercises that `cargo test` cannot: the CI socket's journal commands
# run against a real, booted machine whose snapshot carries the CPU interrupt
# word, and the record/replay pass is driven through the CPU's run-loop
# dispatch/timer-drain path. It does NOT boot IRIX; see the "Remaining evidence"
# note in the issue/commit. A full threaded IRIX boot and a host-independent
# replay of device I/O (disk DMA, RTC, network) are still unverified.
#
# Usage: run.sh [IRIS_BIN] [IRIS_CI_BIN]
#   or set $IRIS / $IRIS_CI / $JOURNAL_N (default 200000). $KEEP=1 keeps the temp dir.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
IRIS="${1:-${IRIS:-$REPO/target/debug/iris}}"
IRIS_CI="${2:-${IRIS_CI:-$REPO/target/debug/iris-ci}}"
# The emulator runs from $WORK (snapshots live under cwd-relative `saves/`), so
# resolve relative binary arguments against the caller's cwd first.
IRIS="$(cd "$(dirname "$IRIS")" && pwd)/$(basename "$IRIS")"
IRIS_CI="$(cd "$(dirname "$IRIS_CI")" && pwd)/$(basename "$IRIS_CI")"
CONFIG="$HERE/machine.toml"
JOURNAL_N="${JOURNAL_N:-200000}"

[[ -x "$IRIS" ]]    || { echo "journal: no iris binary at $IRIS" >&2; exit 2; }
[[ -x "$IRIS_CI" ]] || { echo "journal: no iris-ci binary at $IRIS_CI" >&2; exit 2; }

# Inside `nix develop` the binaries link nix-store libs that are not on the
# default loader path. No-op on a plain host (CI installs libasound directly).
if [ -n "${NIX_LDFLAGS:-}" ]; then
  p=$(printf '%s' "$NIX_LDFLAGS" | tr ' ' '\n' | sed -n 's/^-L//p' | paste -sd:)
  export LD_LIBRARY_PATH="$p${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
gccdir=$(dirname "$(gcc -print-file-name=libstdc++.so.6 2>/dev/null)" 2>/dev/null)
if [ -n "$gccdir" ] && [ -e "$gccdir/libstdc++.so.6" ]; then
  export LD_LIBRARY_PATH="${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}$gccdir"
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/iris-journal.XXXXXX")"
cleanup() {
  [[ -n "${PID:-}" ]] && kill "$PID" 2>/dev/null || true
  wait 2>/dev/null || true
  if [[ "${KEEP:-0}" == "1" ]]; then echo "journal: kept $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT
fail() { echo "journal: FAIL: $*" >&2; exit 1; }

# Snapshots live under cwd-relative `saves/`, so run the emulator from $WORK.
( cd "$WORK" && exec "$IRIS" --config "$CONFIG" \
    --state-dir "$WORK/state" \
    --ci-socket "$WORK/iris.sock" \
    --serial-log "$WORK/serial.log" \
    --ci >"$WORK/iris.stdout" 2>&1 ) &
PID=$!

for _ in $(seq 1 60); do [[ -S "$WORK/iris.sock" ]] && break; sleep 1; done
[[ -S "$WORK/iris.sock" ]] || { tail -20 "$WORK/iris.stdout" >&2; fail "CI socket never appeared"; }

ci() { "$IRIS_CI" --socket "$WORK/iris.sock" "$@"; }
wait_log() { # pattern timeout_s
  local pat="$1" t="$2"
  for _ in $(seq 1 "$((t*5))"); do grep -qE "$pat" "$WORK/serial.log" 2>/dev/null && return 0; sleep 0.2; done
  return 1
}

# ── 1. boot to the PROM command monitor (no disk: option 5) ──────────────────
ci start || true
wait_log "Option\\?" 120 \
  || { tail -30 "$WORK/serial.log" >&2; fail "never reached the PROM menu"; }
ci serial-send "5"
wait_log "Command Monitor" 60 \
  || { tail -30 "$WORK/serial.log" >&2; fail "never reached the command monitor"; }
echo "journal: at the PROM command monitor"

# ── 2. snapshot the booted machine ───────────────────────────────────────────
ci save jtest || fail "save jtest failed"
echo "journal: snapshotted jtest"

# ── 3. record a guest-cycle journal from the snapshot ────────────────────────
rec="$(ci journal-record jtest -n "$JOURNAL_N")" || fail "journal-record failed"
echo "journal: recorded: $rec"

# ── 4. replay and assert equality (iris-ci exits non-zero on divergence) ─────
rep="$(ci journal-replay jtest)" || fail "journal-replay diverged from the recording"
echo "journal: replayed:  $rep"
case "$rep" in
  *"matches recording"*) ;;
  *) fail "replay did not report a match: $rep" ;;
esac

echo "journal: PASS (record == replay over a headless run, $JOURNAL_N cycles)"
