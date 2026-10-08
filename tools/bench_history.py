#!/usr/bin/env python3
"""Collect IRIS benchmark results into a persistent JSON history.

Reads the `bench-report` markdown artifact that the Bare-metal suites CI job
produces (or a checked-out report.md), parses the host line and the per-cell
summary, and appends one entry per run to `data/bench_history.json`.

This is the ingestion half of the benchmark-graph pipeline. The report.md is a
stable, self-contained format (host line + a `## Cells` table), so this parser
needs no cross toolchain and no emulator run — just the artifact.

Usage:
    tools/bench_history.py collect --repo techomancer/iris --run 36998769900 --source upstream
    tools/bench_history.py collect --report report.md --source fork --commit <sha> --date <iso>
    tools/bench_history.py backfill --repo techomancer/iris [--limit 30]
"""

import argparse
import datetime
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
HISTORY = REPO_ROOT / "data" / "bench_history.json"

# Each entry is a single CI run (one report), with its per-cell summary.
# `cells` keeps the headline numbers; per-kernel rows are deliberately not
# stored here (they are large and the report.md is regenerated on demand).
# `commit` is a proxy for the guest-suite version: the bench kernels are part of
# the same repo, so a commit pins the exact suite that was measured.


def parse_report(md: str) -> dict:
    """Parse the host line and the `## Cells` table out of a report.md."""
    host = {}
    cells = []
    in_cells = False
    header = None
    for line in md.splitlines():
        m = re.match(r"^Host:\s*(.*)$", line)
        if m:
            host["raw"] = m.group(1).strip()
            # "linux x86_64 / AMD EPYC 7763 64-Core Processor / 4 cores"
            parts = [p.strip() for p in m.group(1).split("/")]
            if len(parts) >= 3:
                host["cpu"] = parts[1]
                host["cores"] = int(re.search(r"\d+", parts[2]).group())
            continue
        if line.startswith("## Cells"):
            in_cells = True
            continue
        if in_cells and line.startswith("## "):
            break
        if in_cells and line.strip().startswith("|"):
            cols = [c.strip() for c in line.strip().strip("|").split("|")]
            # Skip the markdown separator row (`|---|---|...`).
            if all(set(c) <= set("-: ") for c in cols if c):
                continue
            if header is None:
                header = cols
                continue
            if not cols or cols == header:
                continue
            row = dict(zip(header, cols))
            name = row.get("cell")
            # Only emulated cells belong in the history; the host runs (plain or
            # the per-cell `<cell>-host` baselines) are the normaliser, not data.
            if not name or name == "host" or name.endswith("-host"):
                continue
            # accuracy: "100.0% (40/40)"
            acc = re.match(r"([\d.]+)%\s*\((\d+)/(\d+)\)", row.get("accuracy", ""))
            cells.append(
                {
                    "name": name,
                    "cpu": row.get("CPU"),
                    "features": row.get("features", ""),
                    "accuracy": float(acc.group(1)) if acc else None,
                    "mips": _f(row.get("guest MIPS")),
                    "dmips": _f(row.get("DMIPS")),
                    "whet": _f(row.get("whet/s")),
                    "linpack": _f(row.get("LINPACK MFLOPS")),
                    # Fraction of the same runner's native rate. Absent on
                    # reports recorded before the per-cell host baseline existed.
                    "efficiency": _f(row.get("efficiency")),
                }
            )
    if not cells:
        raise ValueError("no `## Cells` table found in report")
    groups = parse_groups(md)
    for c in cells:
        if c["name"] in groups:
            c["groups"] = groups[c["name"]]
    return {"host": host, "cells": cells}


def parse_groups(md: str) -> dict:
    """Parse the `## Efficiency by group` table into {cell: {group: efficiency}}.

    Empty when the report had no host baseline (the table is only emitted then).
    """
    lines = md.splitlines()
    start = next((i for i, l in enumerate(lines)
                  if l.startswith("## Efficiency by group")), None)
    if start is None:
        return {}
    header = None
    out: dict = {}
    for line in lines[start + 1:]:
        if line.startswith("## "):
            break
        if not line.strip().startswith("|"):
            continue
        cols = [c.strip() for c in line.strip().strip("|").split("|")]
        if all(set(c) <= set("-: ") for c in cols if c):
            continue
        if header is None:
            header = cols  # ["group", cell, cell, ...]
            continue
        if len(cols) != len(header):
            continue
        group = cols[0]
        for cell, val in zip(header[1:], cols[1:]):
            v = _f(val)
            if v is not None:
                out.setdefault(cell, {})[group] = v
    return out


def _f(s):
    if s is None:
        return None
    s = s.strip()
    if not s:
        return None
    try:
        return float(s)
    except ValueError:
        return None


def load() -> dict:
    if HISTORY.exists():
        return json.loads(HISTORY.read_text())
    return {"entries": []}


def save(data: dict) -> None:
    HISTORY.parent.mkdir(parents=True, exist_ok=True)
    HISTORY.write_text(json.dumps(data, indent=2) + "\n")


def entry_exists(data, source, commit):
    return any(e["source"] == source and e["commit"] == commit for e in data["entries"])


def add_entry(data, source, commit, ref, date, title, report_md):
    parsed = parse_report(report_md)
    entry = {
        "source": source,
        "commit": commit,
        "ref": ref,
        "date": date,
        "title": title,
        "host": parsed["host"],
        "cells": parsed["cells"],
    }
    if entry_exists(data, source, commit):
        return False
    data["entries"].append(entry)
    return True


def gh_run_list(repo, limit):
    cmd = [
        "gh", "run", "list", "--repo", repo,
        "--workflow", "Bare-metal suites", "--limit", str(limit),
        "--json", "databaseId,status,conclusion,headSha,headBranch,displayTitle,event,createdAt",
    ]
    out = subprocess.check_output(cmd, text=True)
    return json.loads(out)


def gh_run_download(repo, run_id, dest):
    cmd = ["gh", "run", "download", "--repo", repo, str(run_id), "-n", "bench-report", "-D", str(dest)]
    subprocess.run(cmd, check=True, capture_output=True, text=True)
    report = dest / "report.md"
    if not report.exists():
        raise FileNotFoundError(f"no report.md in {dest}")
    return report.read_text()


def collect(repo, run_id, source, ref, date, title):
    import tempfile
    data = load()
    with tempfile.TemporaryDirectory() as d:
        md = gh_run_download(repo, run_id, Path(d))
    added = add_entry(data, source, run_id, ref, date, title, md)
    save(data)
    print(f"{'added' if added else 'skipped (duplicate)'} {source} {run_id}")
    return added


def backfill(repo, limit, dry_run=False):
    import tempfile
    runs = gh_run_list(repo, limit)
    data = load()
    added = 0
    for r in runs:
        if r["status"] != "completed" or r["conclusion"] != "success":
            continue
        if r["event"] != "push" or r["headBranch"] != "main":
            continue
        if entry_exists(data, repo, r["headSha"]):
            continue
        try:
            with tempfile.TemporaryDirectory() as d:
                md = gh_run_download(repo, r["databaseId"], Path(d))
                ok = add_entry(data, repo, r["headSha"], r["headBranch"], r["createdAt"], r["displayTitle"], md)
        except Exception as e:
            print(f"skip {r['databaseId']}: {e}", file=sys.stderr)
            continue
        if ok:
            added += 1
            print(f"added {repo} {r['headSha'][:8]} {r['displayTitle'][:40]}")
    if not dry_run:
        save(data)
    print(f"backfill: {added} new entries, {len(data['entries'])} total")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)

    c = sub.add_parser("collect")
    c.add_argument("--repo")
    c.add_argument("--run", type=int)
    c.add_argument("--report", help="path to a report.md (instead of --repo/--run)")
    c.add_argument("--source", required=True)
    c.add_argument("--commit", default="")
    c.add_argument("--ref", default="main")
    c.add_argument("--date", default="")
    c.add_argument("--title", default="")

    b = sub.add_parser("backfill")
    b.add_argument("--repo", required=True)
    b.add_argument("--limit", type=int, default=30)
    b.add_argument("--dry-run", action="store_true")

    args = ap.parse_args()
    if args.cmd == "collect":
        if args.report:
            md = Path(args.report).read_text()
            date = args.date or datetime.datetime.utcnow().isoformat() + "Z"
            data = load()
            add_entry(data, args.source, args.commit or "local", args.ref, date, args.title, md)
            save(data)
        else:
            if not args.repo or not args.run:
                sys.exit("--repo and --run are required without --report")
            collect(args.repo, args.run, args.source, args.ref, args.date, args.title)
    elif args.cmd == "backfill":
        backfill(args.repo, args.limit, args.dry_run)


if __name__ == "__main__":
    main()

# NOTE: do not put the literal "[skip ci]" anywhere in a commit message — GitHub
# treats it (and "[ci skip]", "[no ci]", "skip-checks: true") as a directive to
# skip that commit's workflows, even when it appears in the body as prose.
