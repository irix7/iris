#!/usr/bin/env bash
# Backfill the benchmark history by benchmarking historical EMULATOR builds with
# the CURRENT measurement apparatus.
#
# The apparatus — the bench/ guest suite, iris-bench, the config, and the host
# baseline — stays fixed at whatever is checked out here. Only the emulator
# varies: each commit is built into a throwaway worktree and driven by the same
# suite. Old commits therefore need no new code; they only have to accept
# --load-elf/--config/--test-device and run the bare-metal guest.
#
# Everything runs on the invoking machine, sequentially, so the whole backfill
# history shares one host and is directly comparable. Commits that do not build,
# or that cannot run the suite, are skipped with a message (a gap), never
# measured wrong.
#
# Usage:
#   tools/bench-backfill.sh <rev-list-spec> [step] [cpu] [engine]
#   tools/bench-backfill.sh 'HEAD~30..HEAD' 1 r4400 interp
#
# Env:
#   BACKFILL_REPEAT  samples per commit        (default 3)
#   BACKFILL_TARGET  shared CARGO_TARGET_DIR   (default target/backfill)
#   BACKFILL_OUT     results dir               (default bench/build/results)
set -euo pipefail

SPEC="${1:?usage: bench-backfill.sh <rev-list-spec> [step] [cpu] [engine]}"
STEP="${2:-1}"
CPU="${3:-r4400}"
ENGINE="${4:-interp}"
REPEAT="${BACKFILL_REPEAT:-3}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
TARGET="${BACKFILL_TARGET:-$ROOT/target/backfill}"
OUT="${BACKFILL_OUT:-$ROOT/bench/build/results}"
EMU="$(mktemp -d "${TMPDIR:-/tmp}/iris-backfill-emu.XXXXXX")"
EMU="${EMU}/emu"                     # worktree dir inside the temp dir

FEATURES=""
[ "$ENGINE" = "jitv2" ] && FEATURES="jitv2"

# ── apparatus: current suite, current reporter, one host baseline ────────────
# The guest suite is the checked-in prebuilt (its sources are verified against
# it by `make -C bench check-prebuilt`), so no MIPS cross toolchain is needed —
# only the native host build for the baseline.
SUITE_ELF="$ROOT/bench/prebuilt/irisbench.elf"
[ -f "$SUITE_ELF" ] || { echo "backfill: no prebuilt suite at $SUITE_ELF" >&2; exit 2; }
make -C bench hostbench
# shellcheck disable=SC2086
cargo build --release --bin iris-bench ${FEATURES:+--features $FEATURES}
rm -rf "$OUT"; mkdir -p "$OUT"
./target/release/iris-bench host --label host --out "$OUT"
echo "backfill: apparatus ready; host baseline recorded"

# ── commit list, oldest first ────────────────────────────────────────────────
mapfile -t COMMITS < <(git rev-list --reverse "$SPEC" | awk -v s="$STEP" 'NR % s == 1')
total="${#COMMITS[@]}"
if [ "$total" -eq 0 ]; then
  echo "backfill: '$SPEC' matched no commits (does the ref exist in this checkout?)" >&2
  exit 2
fi
echo "backfill: $total commits, cpu=$CPU engine=$ENGINE repeat=$REPEAT"

i=0
for sha in "${COMMITS[@]}"; do
  i=$((i + 1))
  subj="$(git show -s --format=%s "$sha")"
  date="$(git show -s --format=%cI "$sha")"
  printf '== [%d/%d] %.8s %s\n' "$i" "$total" "$sha" "${subj:0:60}"

  git worktree remove --force "$EMU" >/dev/null 2>&1 || true
  if ! git worktree add --detach --force "$EMU" "$sha" >/dev/null 2>&1; then
    echo "   skip: cannot check out $sha"; continue
  fi

  # Build this commit's emulator, sharing the target dir so registry deps
  # compile once across the whole backfill.
  # shellcheck disable=SC2086
  if ! CARGO_TARGET_DIR="$TARGET" cargo build --release ${FEATURES:+--features $FEATURES} \
        --manifest-path "$EMU/Cargo.toml" --bin iris >/tmp/backfill-build.log 2>&1; then
    echo "   skip: does not build on this toolchain (/tmp/backfill-build.log)"; continue
  fi
  EMU_BIN="$TARGET/release/iris"

  # Run the CURRENT suite against the OLD emulator; the host baseline stays.
  # shellcheck disable=SC2086
  if ! ./target/release/iris-bench run --iris "$EMU_BIN" \
        --elf "$SUITE_ELF" --config "$ROOT/bench/run/bare.toml" \
        --label "$CPU-$ENGINE" --repeat "$REPEAT" --out "$OUT" \
        -- --cpu "$CPU" >/tmp/backfill-run.log 2>&1; then
    echo "   skip: did not complete the suite (/tmp/backfill-run.log)"; continue
  fi

  ./target/release/iris-bench report --format md --dir "$OUT" >/tmp/backfill-report.md
  python3 tools/bench_history.py collect --report /tmp/backfill-report.md \
      --source "irix7/iris" --commit "$sha" --ref "$(git rev-parse --abbrev-ref HEAD)" \
      --date "$date" --title "$subj" >/dev/null \
    || echo "   (history already had $sha, or the report was empty)"
done

git worktree remove --force "$EMU" >/dev/null 2>&1 || true
rmdir "$(dirname "$EMU")" >/dev/null 2>&1 || true

python3 tools/bench_graphs.py
echo "backfill: done — data/bench_history.json updated"
