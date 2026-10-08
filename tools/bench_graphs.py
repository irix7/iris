#!/usr/bin/env python3
"""Generate benchmark graphics and rewrite the README's benchmark section.

Reads `data/bench_history.json` (produced by tools/bench_history.py) and emits:

  * `data/bench_cells.svg`  — grouped bar chart of the latest run's four cells
                              (guest MIPS and DMIPS per cell).
  * `data/bench_history.svg` — the same four cells over every recorded run
                              (since the beginning), one panel per cell, points
                              coloured by host CPU and lines broken at host
                              changes so host-to-host steps aren't read as code
                              regressions.
  * `data/bench_history.md` — a table of every recorded run, grouped by host.

It also rewrites the README section between the `<!-- BENCHMARKS -->` markers,
embedding the SVGs and the table. If `GROQ_API_KEY` is set it additionally asks
Groq for a short natural-language analysis paragraph (falling back to a
deterministic summary when the call fails or the key is absent).

Why SVG rather than a charting library: the repo should regenerate its own
graphics in CI without a Python dependency beyond the standard library.

Usage:
    tools/bench_graphs.py            # regenerate graphics + rewrite README
    tools/bench_graphs.py --no-readme  # only write the .svg / .md assets
"""

import argparse
import datetime
import json
import os
import re
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
HISTORY = REPO_ROOT / "data" / "bench_history.json"
SVG = REPO_ROOT / "data" / "bench_cells.svg"
HIST_SVG = REPO_ROOT / "data" / "bench_history.svg"
HIST_EFF_SVG = REPO_ROOT / "data" / "bench_history_eff.svg"
HIST_MD = REPO_ROOT / "data" / "bench_history.md"
README = REPO_ROOT / "README.md"

MARKER = "<!-- BENCHMARKS -->"
CELLS = ["r4400-interp", "r4400-jitv2", "r5000-interp", "r5000-jitv2"]
COLORS = {"interp": "#5b8db8", "jitv2": "#c9763f"}
# One colour per distinct host CPU, assigned in first-seen order. Shared MIPS is
# not comparable across hosts, so the history chart colours by host and breaks
# each run of points where the host changes.
HOST_COLORS = ["#3b7dd8", "#c9763f", "#4caf50", "#9c27b0",
               "#607d8b", "#e64a19", "#009688", "#795548"]


def load():
    if not HISTORY.exists():
        return {"entries": []}
    return json.loads(HISTORY.read_text())


def _num(v):
    return v if isinstance(v, (int, float)) else 0.0


def latest(entries):
    """The newest entry (by date)."""
    if not entries:
        return None
    return sorted(entries, key=lambda e: e["date"])[-1]


def bar_chart_svg(entry):
    """Grouped bar chart: 4 cells x (MIPS, DMIPS)."""
    cells = [c for c in entry["cells"] if c["name"] in CELLS]
    order = {n: i for i, n in enumerate(CELLS)}
    cells.sort(key=lambda c: order.get(c["name"], 99))

    names = [c["name"] for c in cells]
    mips = [_num(c["mips"]) for c in cells]
    dmips = [_num(c["dmips"]) for c in cells]
    vmax = max(max(mips, default=0), max(dmips, default=0)) * 1.15

    W, H = 760, 420
    pad_l, pad_r, pad_t, pad_b = 70, 20, 46, 46
    plot_w = W - pad_l - pad_r
    plot_h = H - pad_t - pad_b
    group = plot_w / len(cells)
    bar_w = group * 0.30

    def y(v):
        return pad_t + plot_h - (v / vmax) * plot_h

    def x_group(i):
        return pad_l + i * group + group / 2

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
             f'viewBox="0 0 {W} {H}" font-family="system-ui,sans-serif">']
    parts.append(f'<text x="{pad_l}" y="{pad_t - 22}" font-size="17" font-weight="600" fill="#222">'
                 f'Guest MIPS / DMIPS — {entry["source"]} {entry["commit"][:8]} ({entry["date"][:10]})</text>')
    parts.append(f'<text x="{pad_l}" y="{pad_t - 6}" font-size="12" fill="#666">'
                 f'host: {entry["host"].get("cpu", "?")} · {entry["host"].get("cores", "?")} cores</text>')

    # gridlines + y labels
    for i in range(5):
        v = vmax * i / 4
        yy = y(v)
        parts.append(f'<line x1="{pad_l}" y1="{yy:.1f}" x2="{W - pad_r}" y2="{yy:.1f}" '
                     f'stroke="#e5e5e5" stroke-width="1"/>')
        parts.append(f'<text x="{pad_l - 8}" y="{yy + 4:.1f}" font-size="11" fill="#888" text-anchor="end">'
                     f'{v:.0f}</text>')

    for i, (n, m, d) in enumerate(zip(names, mips, dmips)):
        cx = x_group(i)
        # MIPS bar
        parts.append(f'<rect x="{cx - bar_w - 2:.1f}" y="{y(m):.1f}" width="{bar_w:.1f}" '
                     f'height="{plot_h - (y(m) - pad_t):.1f}" fill="{COLORS["interp"] if "interp" in n else COLORS["jitv2"]}"/>')
        parts.append(f'<text x="{cx - bar_w/2 - 2:.1f}" y="{y(m) - 6:.1f}" font-size="11" fill="#333" text-anchor="middle">{m:.0f}</text>')
        # DMIPS bar
        parts.append(f'<rect x="{cx + 2:.1f}" y="{y(d):.1f}" width="{bar_w:.1f}" '
                     f'height="{plot_h - (y(d) - pad_t):.1f}" fill="#9aa7b0"/>')
        parts.append(f'<text x="{cx + bar_w/2 + 2:.1f}" y="{y(d) - 6:.1f}" font-size="11" fill="#555" text-anchor="middle">{d:.0f}</text>')
        # x label
        parts.append(f'<text x="{cx:.1f}" y="{H - pad_b + 16:.1f}" font-size="12" fill="#333" text-anchor="middle">{n}</text>')

    # legend
    lx = pad_l
    parts.append(f'<rect x="{lx}" y="{H - 14}" width="12" height="12" fill="{COLORS["interp"]}"/>')
    parts.append(f'<text x="{lx + 16}" y="{H - 3}" font-size="12" fill="#333">MIPS (interp)</text>')
    lx += 118
    parts.append(f'<rect x="{lx}" y="{H - 14}" width="12" height="12" fill="{COLORS["jitv2"]}"/>')
    parts.append(f'<text x="{lx + 16}" y="{H - 3}" font-size="12" fill="#333">MIPS (jitv2)</text>')
    lx += 112
    parts.append(f'<rect x="{lx}" y="{H - 14}" width="12" height="12" fill="#9aa7b0"/>')
    parts.append(f'<text x="{lx + 16}" y="{H - 3}" font-size="12" fill="#333">DMIPS</text>')

    parts.append("</svg>")
    return "\n".join(parts)


def _short_host(cpu):
    """A compact host label for the history legend."""
    cpu = (cpu or "?").replace("(R)", "").replace("(TM)", "")
    for drop in ("CPU", "Processor", "@"):
        cpu = cpu.replace(drop, " ")
    bits = cpu.split()
    if bits and bits[0] in ("AMD", "Intel", "ARM"):
        bits = bits[1:]
    return " ".join(bits[:3]) or "?"


def history_svg(entries, metric="mips"):
    """Time series of every recorded commit, grouped by CPU (R4400 / R5000).

    `metric="mips"` plots raw guest MIPS on a log axis, breaking a line where the
    host CPU changes. `metric="efficiency"` plots the host-normalised fraction of
    the runner's own native rate as a percentage: the host is divided out, so the
    lines are comparable across runners and carry no host detail. Each cell is
    faint raw points under a bold centred moving average.
    """
    import math

    months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
              "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

    def month_day(date):
        try:
            _, m_, d_ = date[:10].split("-")
            return f"{int(d_)} {months[int(m_) - 1]}"
        except Exception:
            return date[:10]

    if metric == "mips":
        columns = [{"date": e["date"], "host": e["host"].get("cpu", "?"),
                    "cells": {c["name"]: _num(c.get(metric))
                              for c in e["cells"] if c.get(metric) is not None}}
                   for e in entries]
        columns = [c for c in columns if c["cells"]]
    else:
        # Efficiency entries carry one cell each (the interp and jitv2 backfills
        # are separate runs), so join them into one column per commit — that is
        # what puts both lines on a shared x axis.
        by_commit = {}
        for e in entries:
            d = by_commit.setdefault(e["commit"], {
                "date": e["date"], "host": e["host"].get("cpu", "?"), "cells": {}})
            for c in e["cells"]:
                if c.get(metric) is not None:
                    d["cells"][c["name"]] = _num(c.get(metric))
        columns = sorted((c for c in by_commit.values() if c["cells"]),
                         key=lambda c: c["date"])
    n = len(columns)
    if n == 0:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    hosts = []
    for col in columns:
        if col["host"] not in hosts:
            hosts.append(col["host"])
    host_color = {h: HOST_COLORS[i % len(HOST_COLORS)] for i, h in enumerate(hosts)}

    all_groups = [("R4400", ["r4400-interp", "r4400-jitv2"]),
                  ("R5000", ["r5000-interp", "r5000-jitv2"])]
    groups = [g for g in all_groups
              if any(name in col["cells"] for col in columns for name in g[1])]
    if not groups:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'
    show_host = metric == "mips"

    W = 900
    pad_l, pad_r, pad_t, pad_b = 84, 22, 100, 56
    panel_h, gap = 168, 30
    plot_w = W - pad_l - pad_r
    ngroup = len(groups)
    H = pad_t + ngroup * (panel_h + gap) - gap + pad_b

    def xpos(i):
        return pad_l + plot_w * (i + 0.5) / n

    def panel_top(j):
        return pad_t + j * (panel_h + gap)

    if metric == "mips":
        title = f"Guest MIPS across {n} recorded commits — grouped by CPU"
        subtitle = "log scale · a line breaks where the host CPU changes"
        ymin, ymax = 20.0, 2000.0
        logspan = math.log10(ymax / ymin)
        grid_values = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0]
        break_on_host = True

        def yval(j, v):
            frac = math.log10(max(v, ymin) / ymin) / logspan
            return panel_top(j) + (1 - frac) * panel_h

        def fmt_grid(v):
            return f"{v:.0f}"
    else:
        title = f"Normalised benchmark across {n} commits — grouped by CPU"
        subtitle = ("percentage of the runner's own native rate; the bold line is a "
                    "centred moving average")
        vals = [v * 100.0 for col in columns for v in col["cells"].values()]
        lo, hi = min(vals), max(vals)
        pad = max((hi - lo) * 0.12, hi * 0.08, 1e-6)
        ymin, ymax = max(0.0, lo - pad), hi + pad
        grid_values = [ymin + (ymax - ymin) * k / 4 for k in range(5)]
        break_on_host = False

        def yval(j, v):
            frac = (v * 100.0 - ymin) / (ymax - ymin) if ymax > ymin else 0.0
            return panel_top(j) + (1 - frac) * panel_h

        def fmt_grid(v):
            return f"{v:.0f}%"

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
             f'viewBox="0 0 {W} {H}" font-family="system-ui,sans-serif">']
    parts.append(f'<text x="{pad_l}" y="26" font-size="17" font-weight="600" fill="#222">{title}</text>')
    parts.append(f'<text x="{pad_l}" y="44" font-size="12" fill="#666">{subtitle}</text>')
    lx = pad_l
    for label, color in (("interp", COLORS["interp"]), ("jitv2", COLORS["jitv2"])):
        parts.append(f'<rect x="{lx}" y="56" width="10" height="10" fill="{color}"/>')
        parts.append(f'<text x="{lx + 14}" y="65" font-size="11" fill="#444">{label}</text>')
        lx += 14 + 7 * len(label) + 22
    parts.append(f'<line x1="{lx}" y1="61" x2="{lx + 20}" y2="61" stroke="#888" '
                 f'stroke-width="2.8" stroke-linecap="round"/>')
    parts.append(f'<text x="{lx + 25}" y="65" font-size="11" fill="#444">trend (moving average)</text>')

    nticks = min(6, n)
    tick_idx = sorted({round(k * (n - 1) / (nticks - 1)) for k in range(nticks)}) if n > 1 else [0]

    windows = []
    if not show_host:
        flags = []
        for col in columns:
            iv = col["cells"].get("r4400-interp")
            jv = col["cells"].get("r4400-jitv2")
            flags.append(iv is not None and jv is not None and jv < iv)
        s = None
        for k, f in enumerate(flags + [False]):
            if f and s is None:
                s = k
            elif not f and s is not None:
                windows.append((s, k - 1)); s = None

    def rolling(vals, w):
        if w < 2 or len(vals) < 3:
            return vals
        half = w // 2
        return [sum(vals[max(0, i - half):i + half + 1]) /
                len(vals[max(0, i - half):i + half + 1]) for i in range(len(vals))]

    band_y = panel_top(ngroup - 1) + panel_h + 14
    base = band_y + (12 if show_host else 0)

    for j, (panel_title, cells) in enumerate(groups):
        top = panel_top(j)
        parts.append(f'<rect x="{pad_l}" y="{top:.1f}" width="{plot_w}" height="{panel_h}" '
                     f'fill="#ffffff" stroke="#e6e6e6"/>')
        for (a, b) in windows:
            x0 = xpos(a) - plot_w / n / 2
            x1 = xpos(b) + plot_w / n / 2
            parts.append(f'<rect x="{x0:.1f}" y="{top:.1f}" width="{(x1 - x0):.1f}" '
                         f'height="{panel_h}" fill="#e0662f" opacity="0.09"/>')
        for gv in grid_values:
            yy = yval(j, gv)
            parts.append(f'<line x1="{pad_l}" y1="{yy:.1f}" x2="{W - pad_r}" y2="{yy:.1f}" '
                         f'stroke="#ececec" stroke-width="1"/>')
            parts.append(f'<text x="{pad_l - 8}" y="{yy + 4:.1f}" font-size="10" fill="#999" '
                         f'text-anchor="end">{fmt_grid(gv)}</text>')
        for i in tick_idx:
            x = xpos(i)
            parts.append(f'<line x1="{x:.1f}" y1="{top:.1f}" x2="{x:.1f}" '
                         f'y2="{top + panel_h:.1f}" stroke="#f3f3f3" stroke-width="1"/>')
        parts.append(f'<text x="{pad_l + 8}" y="{top + 18:.1f}" font-size="13" '
                     f'font-weight="600" fill="#333">{panel_title}</text>')

        for name in cells:
            color = COLORS["interp"] if "interp" in name else COLORS["jitv2"]
            pts = [(i, col["cells"][name]) for i, col in enumerate(columns)
                   if name in col["cells"]]
            if not pts:
                continue
            prev = None
            for i, v in pts:
                x, y = xpos(i), yval(j, v)
                h = columns[i]["host"]
                if prev is not None and (not break_on_host or prev[2] == h):
                    parts.append(f'<line x1="{prev[0]:.1f}" y1="{prev[1]:.1f}" x2="{x:.1f}" '
                                 f'y2="{y:.1f}" stroke="{color}" stroke-width="1" '
                                 f'opacity="0.30"/>')
                parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="1.5" fill="{color}" '
                             f'opacity="0.30"/>')
                prev = (x, y, h)
            ys = rolling([v for _, v in pts], max(5, len(pts) // 20))
            for k in range(1, len(pts)):
                x0, y0 = xpos(pts[k - 1][0]), yval(j, ys[k - 1])
                x1, y1 = xpos(pts[k][0]), yval(j, ys[k])
                parts.append(f'<line x1="{x0:.1f}" y1="{y0:.1f}" x2="{x1:.1f}" y2="{y1:.1f}" '
                             f'stroke="{color}" stroke-width="2.8" stroke-linecap="round"/>')

    if show_host:
        seg = plot_w / n
        for i, col in enumerate(columns):
            parts.append(f'<rect x="{pad_l + i * seg:.1f}" y="{band_y}" width="{seg:.1f}" '
                         f'height="12" fill="{host_color[col["host"]]}"/>')

    for i in tick_idx:
        parts.append(f'<text x="{xpos(i):.1f}" y="{base + 22:.1f}" font-size="11" fill="#666" '
                     f'text-anchor="middle">{month_day(columns[i]["date"])}</text>')
    if windows:
        parts.append(f'<text x="{W - pad_r}" y="{base + 22:.1f}" font-size="11" fill="#b5571f" '
                     f'text-anchor="end">shaded: jitv2 slower than the interpreter</text>')

    parts.append("</svg>")
    return "\n".join(parts)


def has_efficiency(entries) -> bool:
    """True once any recorded cell carries a host-normalised efficiency."""
    return any(c.get("efficiency") is not None for e in entries for c in e["cells"])


def history_md(entries):
    lines = ["# Benchmark history", "",
             "One row per recorded CI run. Throughput figures are only comparable",
             "within the same host CPU — GitHub's shared runners vary between runs,",
             "so cross-host MIPS deltas are host noise, not code change.", ""]
    entries = sorted(entries, key=lambda e: e["date"])
    lines.append("| date | source | commit | host | " + " | ".join(CELLS) + " |")
    lines.append("|---|---|---|---|" + "|---:" * len(CELLS) + "|")
    for e in entries:
        cell_map = {c["name"]: c for c in e["cells"]}
        row = [e["date"][:10], e["source"], f'`{e["commit"][:8]}`', e["host"].get("cpu", "?")]
        for n in CELLS:
            c = cell_map.get(n)
            row.append(f'{_num(c["mips"]):.1f}' if c and c["mips"] is not None else "—")
        lines.append("| " + " | ".join(row) + " |")
    lines.append("")
    return "\n".join(lines)


def groq_analysis(entry, entries):
    """Ask Groq for a short analysis; returns '' on any failure."""
    key = os.environ.get("GROQ_API_KEY")
    if not key:
        return ""
    cells = ", ".join(
        f'{c["name"]} {_num(c["mips"]):.1f} MIPS / {_num(c["dmips"]):.1f} DMIPS / {c["accuracy"]}%'
        for c in entry["cells"]
    )
    prompt = (
        "You are summarising SGI Indy emulator (IRIS) benchmark results for a README. "
        f"Latest run (host {entry['host'].get('cpu','?')}): {cells}. "
        f"{len(entries)} runs recorded in total. "
        "Write a 2-3 sentence plain-English paragraph, no markdown headers, highlighting "
        "the interpreter-vs-JIT speedup and anything else notable. Be precise with the numbers given."
    )
    body = json.dumps({
        "model": "qwen/qwen3.8-27b",
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.4,
        "max_tokens": 300,
    }).encode()
    req = urllib.request.Request(
        "https://api.groq.com/openai/v1/chat/completions",
        data=body,
        headers={
            "Authorization": f"Bearer {key}",
            "Content-Type": "application/json",
            # Cloudflare blocks urllib's default UA with error 1010.
            "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0 Safari/537.36",
        },
    )
    try:
        resp = json.load(urllib.request.urlopen(req, timeout=30))
        text = resp["choices"][0]["message"]["content"].strip()
        # The model's output lands raw in the README, so treat it as untrusted:
        # strip any HTML/markup it might emit and cap the length.
        text = re.sub(r"<[^>]+>", "", text)
        return text[:1500]
    except Exception:
        return ""


def fallback_analysis(entry):
    cells = {c["name"]: c for c in entry["cells"]}
    interp = _num(cells["r4400-interp"]["mips"]) if "r4400-interp" in cells else 0
    jitv2 = _num(cells["r4400-jitv2"]["mips"]) if "r4400-jitv2" in cells else 0
    if interp and jitv2:
        return (f"On this host the JIT (jitv2) runs the R4400 guest at "
                f"{jitv2:.0f} MIPS versus {interp:.0f} MIPS interpreted "
                f"({jitv2/interp:.1f}x).")
    return ""


def readme_block(entry, entries, analysis):
    cells = "\n".join(
        f"| `{c['name']}` | {c['cpu']} | {c['accuracy']:.1f}% | {_num(c['mips']):.1f} | "
        f"{_num(c['dmips']):.1f} | {_num(c['linpack']):.1f} |"
        for c in entry["cells"]
    )
    parts = []
    parts.append(MARKER)
    parts.append("")
    parts.append("## Benchmarks")
    parts.append("")
    if analysis:
        parts.append(analysis)
        parts.append("")
    parts.append("![latest benchmark cells](data/bench_cells.svg)")
    parts.append("")
    parts.append("Latest run's four cells:")
    parts.append("")
    parts.append("| cell | CPU | accuracy | MIPS | DMIPS | LINPACK MFLOPS |")
    parts.append("|---|---|---:|---:|---:|---:|")
    parts.append(cells)
    parts.append("")
    if has_efficiency(entries):
        parts.append(f"Normalised benchmark across all {len(entries)} recorded runs — each run "
                     f"divided by its own runner's native rate, so the same number means the same "
                     f"thing on every CI runner (interpreter vs jitv2):")
        parts.append("")
        parts.append("![normalised benchmark history](data/bench_history_eff.svg)")
        parts.append("")
    parts.append(f"Full history table: [data/bench_history.md](data/bench_history.md) "
                 f"({len(entries)} runs). Regenerated from `data/bench_history.json` "
                 f"by `tools/bench_graphs.py`.")
    parts.append("")
    parts.append(MARKER)
    return "\n".join(parts)


def rewrite_readme(block):
    text = README.read_text() if README.exists() else ""
    if MARKER in text:
        # replace between first and second marker inclusive
        start = text.index(MARKER)
        end = text.index(MARKER, start + len(MARKER)) + len(MARKER)
        new = text[:start] + block + text[end:]
    else:
        new = text.rstrip() + "\n\n" + block + "\n"
    README.write_text(new)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-readme", action="store_true")
    args = ap.parse_args()

    data = load()
    entries = data["entries"]
    entry = latest(entries)
    if entry is None:
        print("no entries; nothing to do")
        return

    SVG.write_text(bar_chart_svg(entry) + "\n")
    print(f"wrote {SVG.name}")

    HIST_SVG.write_text(history_svg(entries, "mips") + "\n")
    print(f"wrote {HIST_SVG.name}")

    if has_efficiency(entries):
        HIST_EFF_SVG.write_text(history_svg(entries, "efficiency") + "\n")
        print(f"wrote {HIST_EFF_SVG.name}")

    HIST_MD.write_text(history_md(entries))
    print(f"wrote {HIST_MD.name}")

    if not args.no_readme:
        analysis = groq_analysis(entry, entries) or fallback_analysis(entry)
        rewrite_readme(readme_block(entry, entries, analysis))
        print(f"rewrote {README.name}")


if __name__ == "__main__":
    main()
