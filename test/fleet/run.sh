#!/usr/bin/env bash
# Fleet CI test — the parallel-agent workflow, end to end.
#
# Boots N isolated instances (`--instance i`) and drives each over its own CI
# serial socket, the way an agent driving a fleet does. It asserts the things
# parallel agentic runs depend on:
#
#   * `--instance i` derives disjoint state dirs, CI sockets, monitor ports,
#     guest MACs and NAT subnets (via the `--print-instance` surface).
#   * N instances boot together, each reachable on its own CI socket.
#   * Each instance's PROM command monitor answers over its serial console
#     (serial-send + serial-wait round trip), and the instances can be driven
#     concurrently.
#   * The per-instance derived MAC reaches the guest as a distinct `eaddr`.
#   * Quitting one instance does not disturb the others.
#
# It needs no IRIX media and no bare-metal guest: the Indy PROM's own command
# monitor is the serial endpoint. Mirrors the dance in
# cpu-tests/run/run-prom.sh.
#
# Usage: run.sh [IRIS_BIN] [IRIS_CI_BIN]
#   or set $IRIS / $IRIS_CI / $FLEET_N (default 2). $KEEP=1 keeps the temp dir.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
IRIS="${1:-${IRIS:-$REPO/target/debug/iris}}"
IRIS_CI="${2:-${IRIS_CI:-$REPO/target/debug/iris-ci}}"
CONFIG="$HERE/machine.toml"
N="${FLEET_N:-2}"

[[ -x "$IRIS" ]]    || { echo "fleet: no iris binary at $IRIS" >&2; exit 2; }
[[ -x "$IRIS_CI" ]] || { echo "fleet: no iris-ci binary at $IRIS_CI" >&2; exit 2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/iris-fleet.XXXXXX")"
PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do kill "$p" 2>/dev/null || true; done
  wait 2>/dev/null || true
  if [[ "${KEEP:-0}" == "1" ]]; then echo "fleet: kept $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT
fail() { echo "fleet: FAIL: $*" >&2; exit 1; }

# ── 1. derivation must be disjoint ───────────────────────────────────────────
# --print-instance is the discoverability surface agents use to find endpoints.
declare -A DIR SOCK MON MAC SUBNET
for ((i=0;i<N;i++)); do
  out="$("$IRIS" --config "$CONFIG" --instance "$i" --print-instance)"
  DIR[$i]="$(sed -n 's/^state_dir: //p'   <<<"$out")"
  SOCK[$i]="$(sed -n 's/^ci_socket: //p'  <<<"$out")"
  MON[$i]="$(sed -n 's/^monitor: //p'     <<<"$out")"
  MAC[$i]="$(sed -n 's/^mac: //p'         <<<"$out")"
  SUBNET[$i]="$(sed -n 's/^nat_subnet: //p' <<<"$out")"
  [[ -n "${SOCK[$i]}" && -n "${MON[$i]}" && -n "${MAC[$i]}" && -n "${SUBNET[$i]}" ]] \
    || fail "--print-instance $i did not report all endpoints:"$'\n'"$out"
done
for ((i=0;i<N;i++)); do for ((j=i+1;j<N;j++)); do
  [[ "${DIR[$i]}"    != "${DIR[$j]}"    ]] || fail "instances $i/$j share a state dir"
  [[ "${SOCK[$i]}"   != "${SOCK[$j]}"   ]] || fail "instances $i/$j share a CI socket"
  [[ "${MON[$i]}"    != "${MON[$j]}"    ]] || fail "instances $i/$j share the monitor port"
  [[ "${MAC[$i]}"    != "${MAC[$j]}"    ]] || fail "instances $i/$j share a MAC"
  [[ "${SUBNET[$i]}" != "${SUBNET[$j]}" ]] || fail "instances $i/$j share a NAT subnet"
done; done
echo "fleet: derivation is disjoint for $N instances"

# ── 2. boot all instances together ───────────────────────────────────────────
for ((i=0;i<N;i++)); do
  mkdir -p "$WORK/inst$i"
  "$IRIS" --config "$CONFIG" --instance "$i" \
          --state-dir "$WORK/inst$i" \
          --ci-socket "$WORK/inst$i/iris.sock" \
          --serial-log "$WORK/inst$i/serial.log" \
          --ci >"$WORK/inst$i.stdout" 2>&1 &
  PIDS+=("$!")
done

for ((i=0;i<N;i++)); do
  sock="$WORK/inst$i/iris.sock"
  for _ in $(seq 1 60); do [[ -S "$sock" ]] && break; sleep 1; done
  [[ -S "$sock" ]] || { tail -20 "$WORK/inst$i.stdout" >&2; fail "instance $i socket never appeared"; }
done
echo "fleet: $N instances up"

# ── helpers ──────────────────────────────────────────────────────────────────
ci() { local i="$1"; shift; "$IRIS_CI" --socket "$WORK/inst$i/iris.sock" "$@" >/dev/null 2>&1; }
wait_log() { # i  extended-regex  timeout_s
  local i="$1" pat="$2" t="$3" f="$WORK/inst$i/serial.log"
  for _ in $(seq 1 "$((t*5))"); do grep -qE "$pat" "$f" 2>/dev/null && return 0; sleep 0.2; done
  return 1
}
reach_monitor() { # i
  local i="$1"
  ci "$i" start || true
  # No disk attached, so the PROM falls through to its System Maintenance menu.
  # Option 5 is "Enter Command Monitor".
  wait_log "$i" "Option\\?" 120 \
    || { tail -30 "$WORK/inst$i/serial.log" >&2; fail "instance $i never reached the PROM menu"; }
  ci "$i" serial-send "5"
  wait_log "$i" "Command Monitor" 60 \
    || { tail -30 "$WORK/inst$i/serial.log" >&2; fail "instance $i never reached the command monitor"; }
}

# ── 3. every instance answers over its own serial console ────────────────────
for ((i=0;i<N;i++)); do reach_monitor "$i"; done
echo "fleet: $N instances at the PROM command monitor"

# ── 4. drive them together; per-instance eaddr proves MAC isolation ──────────
# The send/read goes through the live CI serial path (not just the log file):
# an agent's `serial-wait` must observe the PROM's answer on this instance.
for ((i=0;i<N;i++)); do ci "$i" serial-send "printenv"; done
declare -A EADDR
for ((i=0;i<N;i++)); do
  ci "$i" serial-wait "eaddr=" --timeout 30 \
    || { tail -30 "$WORK/inst$i/serial.log" >&2; fail "instance $i: serial-wait never saw eaddr="; }
  EADDR[$i]="$( { grep -oE 'eaddr=[0-9A-Fa-f:]+' "$WORK/inst$i/serial.log" | tail -1 | cut -d= -f2; } || true )"
  [[ -n "${EADDR[$i]}" ]] || fail "instance $i: could not parse eaddr"
done
for ((i=0;i<N;i++)); do for ((j=i+1;j<N;j++)); do
  [[ "${EADDR[$i]}" != "${EADDR[$j]}" ]] \
    || fail "instances $i/$j report the same guest eaddr ${EADDR[$i]}"
done; done
echo "fleet: distinct guest eaddr per instance (${EADDR[0]} vs ${EADDR[1]})"

# ── 5. stopping one instance must not disturb the rest ───────────────────────
ci 0 quit || true
sleep 1
for ((i=1;i<N;i++)); do
  ci "$i" ping || fail "instance $i was not reachable after instance 0 quit"
done

echo "fleet: PASS ($N isolated instances)"
