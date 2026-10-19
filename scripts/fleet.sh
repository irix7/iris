#!/usr/bin/env bash
# Start N isolated IRIS instances and print their endpoints.
#
# Usage: scripts/fleet.sh [-n N] [-c CONFIG] [-b PORT_BASE] [-s STATE_ROOT]
#        scripts/fleet.sh --dry-run ...
#
# Each instance is `iris --instance i`; the config is only used as the base.
# Endpoints come from `--print-instance`, so this script never invents paths or
# ports. Processes are left running in the background; their PIDs are written
# to <state-root>/pids.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
IRIS="${IRIS:-$REPO/target/debug/iris}"
N=2
CONFIG="$REPO/test/fleet/machine.toml"
PORT_BASE=9000
STATE_ROOT="$PWD/iris-fleet"
DRY_RUN=0

while getopts ":n:c:b:s:dh" opt; do
  case "$opt" in
    n) N="$OPTARG" ;;
    c) CONFIG="$OPTARG" ;;
    b) PORT_BASE="$OPTARG" ;;
    s) STATE_ROOT="$OPTARG" ;;
    d) DRY_RUN=1 ;;
    h) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "fleet: unknown option -$OPTARG" >&2; exit 2 ;;
  esac
done

[[ -x "$IRIS" ]] || { echo "fleet: no iris binary at $IRIS" >&2; exit 2; }
[[ "$N" =~ ^[0-9]+$ && "$N" -ge 1 ]] || { echo "fleet: N must be a positive integer" >&2; exit 2; }
mkdir -p "$STATE_ROOT"
PID_FILE="$STATE_ROOT/pids"
: > "$PID_FILE"

declare -A SOCKETS
for ((i=0; i<N; i++)); do
  base=$((PORT_BASE + i * 10))
  state="$STATE_ROOT/inst$i"
  mkdir -p "$state"
  report="$("$IRIS" --config "$CONFIG" --instance "$i" --state-dir "$state" --port-base "$base" --print-instance)"
  sock="$(sed -n 's/^ci_socket: //p' <<<"$report")"
  mon="$(sed -n 's/^monitor: //p' <<<"$report")"
  [[ -n "$sock" && -n "$mon" ]] || { echo "fleet: instance $i has no socket/monitor" >&2; exit 1; }
  [[ -z "${SOCKETS[$sock]:-}" ]] || { echo "fleet: instances share socket $sock" >&2; exit 1; }
  SOCKETS[$sock]=1
  printf '%s\n' "$report"
  if [[ "$DRY_RUN" == 0 ]]; then
    "$IRIS" --config "$CONFIG" --instance "$i" --state-dir "$state" \
      --port-base "$base" --ci --ci-socket "$sock" \
      --serial-log "$state/serial.log" >"$state/stdout" 2>&1 &
    echo "$!" >> "$PID_FILE"
  fi
done

if [[ "$DRY_RUN" == 1 ]]; then
  echo "fleet: dry run; $N instance(s) validated"
else
  echo "fleet: started $N instance(s); PIDs in $PID_FILE"
fi
