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


def parse_report(md: str) -> dict:
    """Parse the host line and all tables out of a report.md."""
    host = {}
    cells = []
    in_cells = False
    header = None
    for line in md.splitlines():
        m = re.match(r"^Host:\s*(.*)$", line)
        if m:
            host["raw"] = m.group(1).strip()
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
            if all(set(c) <= set("-: ") for c in cols if c):
                continue
            if header is None:
                header = cols
                continue
            if not cols or cols == header:
                continue
            row = dict(zip(header, cols))
            name = row.get("cell")
            if not name or name == "host" or name.endswith("-host"):
                continue
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
                    "efficiency": _f(row.get("efficiency")),
                }
            )

    if not cells:
        raise ValueError("no `## Cells` table found in report")

    groups = parse_groups(md)
    kernels = parse_kernels(md)
    time_share = parse_time_share(md)
    least_eff = parse_least_efficient(md)
    exceptions = parse_exceptions(md)
    mismatches = parse_mismatches(md)

    for c in cells:
        name = c["name"]
        if name in groups:
            c["groups"] = groups[name]
        if name in kernels:
            c["kernels"] = kernels[name]
        if name in time_share:
            c["time_share"] = time_share[name]
        if name in least_eff:
            c["least_efficient"] = least_eff[name]
        if name in exceptions:
            c["exceptions"] = exceptions[name]
        if name in mismatches:
            c["mismatches"] = mismatches[name]

    return {"host": host, "cells": cells}


def parse_groups(md: str) -> dict:
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
            header = cols
            continue
        if len(cols) != len(header):
            continue
        group = cols[0]
        for cell, val in zip(header[1:], cols[1:]):
            v = _f(val)
            if v is not None:
                out.setdefault(cell, {})[group] = v
    return out


def parse_kernels(md: str) -> dict:
    """Parse `## Per-kernel throughput` table into {cell: {kernel: {rate, unit, native, vs_base}}}."""
    lines = md.splitlines()
    start = next((i for i, l in enumerate(lines)
                  if l.startswith("## Per-kernel throughput")), None)
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
            header = cols
            continue
        if len(cols) != len(header):
            continue
        kernel_name = cols[0]
        unit = cols[1]
        # Columns after unit: one per cell, then optionally "vs base", "host", "native"
        cell_cols = header[2:]
        for cell, val in zip(cell_cols, cols[2:]):
            if cell in ("vs base", "host", "native"):
                continue  # skip derived columns for now
            v = _f(val)
            if v is not None:
                out.setdefault(cell, {})[kernel_name] = {
                    "rate": v,
                    "unit": unit,
                }
    return out


def parse_time_share(md: str) -> dict:
    """Parse `## Where <cell> spends its time` tables into {cell: {kernel: {share_pct, mips}}}."""
    lines = md.splitlines()
    out: dict = {}
    i = 0
    while i < len(lines):
        line = lines[i]
        m = re.match(r"^## Where\s+(.+?)\s+spends its time", line)
        if m:
            cell = m.group(1).strip()
            cell_data = {}
            i += 1
            # Skip header lines
            while i < len(lines) and not lines[i].strip().startswith("|"):
                i += 1
            if i < len(lines):
                i += 1  # skip separator
            while i < len(lines) and lines[i].strip().startswith("|"):
                cols = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                if len(cols) >= 3 and not all(set(c) <= set("-: ") for c in cols if c):
                    kernel = cols[0]
                    share = _f(cols[1].replace("%", ""))
                    mips = _f(cols[2])
                    if share is not None:
                        cell_data[kernel] = {"share_pct": share, "mips": mips}
                i += 1
            out[cell] = cell_data
            continue
        i += 1
    return out


def parse_least_efficient(md: str) -> dict:
    """Parse `Least efficient` tables into {cell: {kernel: {mips, vs_avg}}}."""
    lines = md.splitlines()
    out: dict = {}
    i = 0
    while i < len(lines):
        line = lines[i]
        if "Least efficient" in line and line.startswith("##"):
            # Find which cell this belongs to by looking backwards
            cell = None
            for j in range(i - 1, -1, -1):
                m = re.match(r"^## Where\s+(.+?)\s+spends its time", lines[j])
                if m:
                    cell = m.group(1).strip()
                    break
            if not cell:
                i += 1
                continue
            cell_data = {}
            i += 1
            while i < len(lines) and not lines[i].strip().startswith("|"):
                i += 1
            if i < len(lines):
                i += 1  # skip separator
            while i < len(lines) and lines[i].strip().startswith("|"):
                cols = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                if len(cols) >= 3 and not all(set(c) <= set("-: ") for c in cols if c):
                    kernel = cols[0]
                    mips = _f(cols[1])
                    vs_avg = _f(cols[2].replace("x", ""))
                    if mips is not None:
                        cell_data[kernel] = {"mips": mips, "vs_avg": vs_avg}
                i += 1
            out[cell] = cell_data
            continue
        i += 1
    return out


def parse_exceptions(md: str) -> dict:
    """Parse `## Unexpected exceptions` table into {cell: {kernel: count}}."""
    lines = md.splitlines()
    start = next((i for i, l in enumerate(lines)
                  if l.startswith("## Unexpected exceptions")), None)
    if start is None:
        return {}
    out: dict = {}
    for line in lines[start + 1:]:
        if line.startswith("## "):
            break
        if not line.strip().startswith("|"):
            continue
        cols = [c.strip() for c in line.strip().strip("|").split("|")]
        if all(set(c) <= set("-: ") for c in cols if c):
            continue
        if len(cols) >= 3 and cols[0] != "cell":
            cell = cols[0]
            kernel = cols[1]
            exc = _f(cols[2])
            if exc is not None:
                out.setdefault(cell, {})[kernel] = int(exc)
    return out


def parse_mismatches(md: str) -> dict:
    """Parse `## Checksum mismatches` table into {cell: {kernel: {got, want}}}."""
    lines = md.splitlines()
    start = next((i for i, l in enumerate(lines)
                  if l.startswith("## Checksum mismatches")), None)
    if start is None:
        return {}
    out: dict = {}
    for line in lines[start + 1:]:
        if line.startswith("## "):
            break
        if not line.strip().startswith("|"):
            continue
        cols = [c.strip() for c in line.strip().strip("|").split("|")]
        if all(set(c) <= set("-: ") for c in cols if c):
            continue
        if len(cols) >= 4 and cols[0] != "cell":
            cell = cols[0]
            kernel = cols[1]
            got = cols[2].strip("`")
            want = cols[3].strip("`")
            out.setdefault(cell, {})[kernel] = {"got": got, "want": want}
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


def load(path=HISTORY) -> dict:
    if path.exists():
        return json.loads(path.read_text())
    return {"entries": []}


def save(data: dict, path=HISTORY) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")


def entry_key(entry):
    return (entry.get("source"), entry.get("commit"),
            tuple(sorted(c.get("name") for c in entry.get("cells", []))))


def entry_exists(data, entry):
    k = entry_key(entry)
    return any(entry_key(e) == k for e in data["entries"])


def commit_seen(data, source, commit):
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
    if entry_exists(data, entry):
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


def merge_into(into_path: Path, from_paths) -> None:
    into = load(into_path)
    before = len(into["entries"])
    seen = {entry_key(e) for e in into["entries"]}
    for f in from_paths:
        for e in load(Path(f))["entries"]:
            key = entry_key(e)
            if key not in seen:
                into["entries"].append(e)
                seen.add(key)
    save(into, into_path)
    print(f"merge: {len(into['entries']) - before} new entries, {len(into['entries'])} total")


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
        if commit_seen(data, repo, r["headSha"]):
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
    c.add_argument("--history", default=str(HISTORY),
                   help="history file to append to (default data/bench_history.json); "
                        "a sharded backfill writes its own fragment here")

    b = sub.add_parser("backfill")
    b.add_argument("--repo", required=True)
    b.add_argument("--limit", type=int, default=30)
    b.add_argument("--dry-run", action="store_true")

    m = sub.add_parser("merge")
    m.add_argument("--into", default=str(HISTORY))
    m.add_argument("--from", dest="from_", nargs="+", required=True,
                   help="history fragment files to fold in (deduped by commit)")

    args = ap.parse_args()
    if args.cmd == "collect":
        if args.report:
            md = Path(args.report).read_text()
            date = args.date or datetime.datetime.utcnow().isoformat() + "Z"
            hist = Path(args.history)
            data = load(hist)
            add_entry(data, args.source, args.commit or "local", args.ref, date, args.title, md)
            save(data, hist)
        else:
            if not args.repo or not args.run:
                sys.exit("--repo and --run are required without --report")
            collect(args.repo, args.run, args.source, args.ref, args.date, args.title)
    elif args.cmd == "backfill":
        backfill(args.repo, args.limit, args.dry_run)
    elif args.cmd == "merge":
        merge_into(Path(args.into), args.from_)


if __name__ == "__main__":
    main()

# NOTE: do not put the literal "[skip ci]" anywhere in a commit message — GitHub
# treats it (and "[ci skip]", "[no ci]", "skip-checks: true") as a directive to
# skip that commit's workflows, even when it appears in the body as prose.