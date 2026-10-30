#!/usr/bin/env python3
"""Generate benchmark graphics and rewrite the README's benchmark section.

Reads `data/bench_history.json` (produced by tools/bench_history.py) and emits:

  * `data/bench_cells.svg`       — grouped bar chart of the latest run's four cells
                                   (MIPS, DMIPS, LINPACK MFLOPS, Whetstone passes/s).
  * `data/bench_history.svg`     — raw guest MIPS over every recorded run (log scale).
  * `data/bench_history_eff.svg` — normalised efficiency (fraction of host native rate).
  * `data/bench_speedup.svg`     — JIT vs interpreter speedup factor over time.
  * `data/bench_groups.svg`      — efficiency by kernel group (int/fpu/mem/img/vid/codec/sys).
  * `data/bench_heatmap.svg`     — heatmap of group efficiency across commits.
  * `data/bench_history.md`      — a table of every recorded run, grouped by host.

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
import math
import os
import re
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
HISTORY = REPO_ROOT / "data" / "bench_history.json"
SVG = REPO_ROOT / "data" / "bench_cells.svg"
HIST_SVG = REPO_ROOT / "data" / "bench_history.svg"
HIST_EFF_SVG = REPO_ROOT / "data" / "bench_history_eff.svg"
SPEEDUP_SVG = REPO_ROOT / "data" / "bench_speedup.svg"
GROUPS_SVG = REPO_ROOT / "data" / "bench_groups.svg"
HEATMAP_SVG = REPO_ROOT / "data" / "bench_heatmap.svg"
HIST_MD = REPO_ROOT / "data" / "bench_history.md"
README = REPO_ROOT / "README.md"

MARKER = "<!-- BENCHMARKS -->"
CELLS = ["r4400-interp", "r4400-jitv2", "r5000-interp", "r5000-jitv2"]
GROUPS = ["int", "fpu", "mem", "img", "vid", "codec", "sys"]
COLORS = {"interp": "#5b8db8", "jitv2": "#c9763f"}
GROUP_COLORS = {
    "int": "#1f77b4", "fpu": "#ff7f0e", "mem": "#2ca02c", "img": "#d62728",
    "vid": "#9467bd", "codec": "#8c564b", "sys": "#e377c2"
}
HOST_COLORS = ["#3b7dd8", "#c9763f", "#4caf50", "#9c27b0",
               "#607d8b", "#e64a19", "#009688", "#795548"]


def load():
    if not HISTORY.exists():
        return {"entries": []}
    return json.loads(HISTORY.read_text())


def _num(v):
    return v if isinstance(v, (int, float)) else 0.0


def latest(entries):
    if not entries:
        return None
    return sorted(entries, key=lambda e: e["date"])[-1]


def _short_host(cpu):
    cpu = (cpu or "?").replace("(R)", "").replace("(TM)", "")
    for drop in ("CPU", "Processor", "@"):
        cpu = cpu.replace(drop, " ")
    bits = cpu.split()
    if bits and bits[0] in ("AMD", "Intel", "ARM"):
        bits = bits[1:]
    return " ".join(bits[:3]) or "?"


def _cell_label(name):
    """Convert cell name to display label."""
    return name.replace("-", " ").upper()


def _group_label(group):
    """Convert group name to display label."""
    return group.upper()


# ──────────────────────────────────────────────────────────────────────────────
# Chart 1: Latest run grouped bar chart (MIPS, DMIPS, LINPACK, Whetstone)
# ──────────────────────────────────────────────────────────────────────────────

def bar_chart_svg(entry):
    """Grouped bar chart: 4 cells x (MIPS, DMIPS, LINPACK, Whetstone)."""
    cells = [c for c in entry["cells"] if c["name"] in CELLS]
    order = {n: i for i, n in enumerate(CELLS)}
    cells.sort(key=lambda c: order.get(c["name"], 99))

    names = [c["name"] for c in cells]
    metrics = {
        "mips": [_num(c["mips"]) for c in cells],
        "dmips": [_num(c["dmips"]) for c in cells],
        "linpack": [_num(c["linpack"]) for c in cells],
        "whet": [_num(c["whet"]) for c in cells],
    }

    # Scale each metric independently to its own max
    metric_info = {}
    for m, vals in metrics.items():
        vmax = max(vals) * 1.15 if vals else 1.0
        metric_info[m] = {"vals": vals, "vmax": vmax}

    W, H = 920, 480
    pad_l, pad_r, pad_t, pad_b = 80, 30, 60, 60
    plot_w = W - pad_l - pad_r
    plot_h = H - pad_t - pad_b
    n_groups = len(cells)
    group_w = plot_w / n_groups
    n_metrics = len(metrics)
    bar_w = group_w * 0.8 / n_metrics

    def y(v, vmax):
        return pad_t + plot_h - (v / vmax) * plot_h if vmax > 0 else pad_t + plot_h

    def x_group(i):
        return pad_l + i * group_w + group_w / 2

    metric_order = ["mips", "dmips", "linpack", "whet"]
    metric_labels = {"mips": "MIPS", "dmips": "DMIPS", "linpack": "LINPACK MFLOPS", "whet": "Whetstone k/s"}
    metric_colors = {
        "mips": {"interp": "#5b8db8", "jitv2": "#c9763f"},
        "dmips": {"interp": "#4a7ab8", "jitv2": "#b86632"},
        "linpack": {"interp": "#3a6ab8", "jitv2": "#a85628"},
        "whet": {"interp": "#2a5ab8", "jitv2": "#98461e"},
    }

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
             f'viewBox="0 0 {W} {H}" font-family="system-ui,sans-serif">']

    # Title
    parts.append(f'<text x="{pad_l}" y="{pad_t - 28}" font-size="18" font-weight="600" fill="#222">'
                 f'Latest Benchmark — {entry["source"]} {entry["commit"][:8]} ({entry["date"][:10]})</text>')
    parts.append(f'<text x="{pad_l}" y="{pad_t - 10}" font-size="12" fill="#666">'
                 f'host: {entry["host"].get("cpu", "?")} · {entry["host"].get("cores", "?")} cores</text>')

    # Gridlines per metric (we'll draw a shared y-axis with the max metric as reference)
    # Actually, let's use a shared y-axis scaled to the tallest bar
    all_vals = [v for vals in metrics.values() for v in vals]
    global_max = max(all_vals) * 1.15 if all_vals else 1.0

    for i in range(6):
        v = global_max * i / 5
        yy = y(v, global_max)
        parts.append(f'<line x1="{pad_l}" y1="{yy:.1f}" x2="{W - pad_r}" y2="{yy:.1f}" '
                     f'stroke="#e5e5e5" stroke-width="1"/>')
        parts.append(f'<text x="{pad_l - 8}" y="{yy + 4:.1f}" font-size="10" fill="#888" text-anchor="end">'
                     f'{v:.0f}</text>')

    # Bars
    for i, name in enumerate(names):
        cx = x_group(i)
        is_jit = "jitv2" in name
        engine = "jitv2" if is_jit else "interp"

        for mi, metric in enumerate(metric_order):
            vals = metric_info[metric]["vals"]
            v = vals[i]
            vmax = metric_info[metric]["vmax"] or global_max

            # Center bars in group
            offset = (mi - (n_metrics - 1) / 2) * (bar_w + 1)
            bx = cx + offset
            by = y(v, vmax)
            bh = plot_h - (by - pad_t)

            color = metric_colors[metric][engine]
            parts.append(f'<rect x="{bx:.1f}" y="{by:.1f}" width="{bar_w:.1f}" '
                         f'height="{bh:.1f}" fill="{color}"/>')
            if v > global_max * 0.03:
                parts.append(f'<text x="{bx + bar_w/2:.1f}" y="{by - 4:.1f}" font-size="9" fill="#333" text-anchor="middle">'
                             f'{v:.0f}</text>')

        # X-axis label
        parts.append(f'<text x="{cx:.1f}" y="{H - pad_b + 18:.1f}" font-size="12" fill="#333" text-anchor="middle">'
                     f'{_cell_label(name)}</text>')

    # Legend
    ly = H - 28
    lx = pad_l
    for metric in metric_order:
        label = metric_labels[metric]
        color = metric_colors[metric]["interp"]  # use interp color as representative
        parts.append(f'<rect x="{lx}" y="{ly}" width="12" height="12" fill="{color}" opacity="0.9"/>')
        parts.append(f'<text x="{lx + 16}" y="{ly + 9}" font-size="11" fill="#444">{label}</text>')
        lx += 16 + 7 * len(label) + 18

    parts.append("</svg>")
    return "\n".join(parts)


# ──────────────────────────────────────────────────────────────────────────────
# Chart 2 & 3: History charts (raw MIPS and normalised efficiency)
# ──────────────────────────────────────────────────────────────────────────────

def history_svg(entries, metric="mips"):
    """Time series of every recorded commit, grouped by CPU (R4400 / R5000)."""
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

    W = 960
    pad_l, pad_r, pad_t, pad_b = 90, 30, 110, 70
    panel_h, gap = 180, 35
    plot_w = W - pad_l - pad_r
    ngroup = len(groups)
    H = pad_t + ngroup * (panel_h + gap) - gap + pad_b

    def xpos(i):
        return pad_l + plot_w * (i + 0.5) / n

    def panel_top(j):
        return pad_t + j * (panel_h + gap)

    if metric == "mips":
        title = f"Guest MIPS Across {n} Recorded Commits — Grouped by Emulated CPU"
        subtitle = "Logarithmic scale · lines break where host CPU changes · faint points = raw samples, bold = moving average"
        ymin, ymax = 15.0, 2500.0
        logspan = math.log10(ymax / ymin)
        grid_values = [15, 30, 50, 100, 200, 500, 1000, 2000]
        break_on_host = True

        def yval(j, v):
            frac = math.log10(max(v, ymin) / ymin) / logspan
            return panel_top(j) + (1 - frac) * panel_h

        def fmt_grid(v):
            return f"{v:.0f}"
    else:
        title = f"Normalised Efficiency Across {n} Commits — Fraction of Host Native Rate"
        subtitle = "100% = same speed as native host code · bold line = centred moving average (window ~5% of points)"
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
    parts.append(f'<text x="{pad_l}" y="28" font-size="18" font-weight="600" fill="#222">{title}</text>')
    parts.append(f'<text x="{pad_l}" y="48" font-size="12" fill="#666">{subtitle}</text>')

    # Legend
    lx = pad_l
    for label, color in (("interp", COLORS["interp"]), ("jitv2", COLORS["jitv2"])):
        parts.append(f'<rect x="{lx}" y="60" width="10" height="10" fill="{color}"/>')
        parts.append(f'<text x="{lx + 14}" y="69" font-size="11" fill="#444">{label}</text>')
        lx += 14 + 7 * len(label) + 22
    parts.append(f'<line x1="{lx}" y1="65" x2="{lx + 24}" y2="65" stroke="#888" '
                 f'stroke-width="2.8" stroke-linecap="round"/>')
    parts.append(f'<text x="{lx + 28}" y="69" font-size="11" fill="#444">trend (moving average)</text>')

    # Host change indicators (only for raw MIPS)
    if show_host:
        host_band_y = pad_t + ngroup * (panel_h + gap) - gap + 18
        seg = plot_w / n
        for i, col in enumerate(columns):
            parts.append(f'<rect x="{pad_l + i * seg:.1f}" y="{host_band_y}" width="{seg:.1f}" '
                         f'height="10" fill="{host_color[col["host"]]}"/>')
        # Host legend below
        lx = pad_l
        for h, c in host_color.items():
            if lx > W - pad_r - 100:
                break
            short = _short_host(h)
            parts.append(f'<rect x="{lx}" y="{host_band_y + 16}" width="8" height="8" fill="{c}"/>')
            parts.append(f'<text x="{lx + 12}" y="{host_band_y + 22}" font-size="9" fill="#666">{short}</text>')
            lx += 12 + 6 * len(short) + 14

    # X-axis ticks
    nticks = min(8, n)
    tick_idx = sorted({round(k * (n - 1) / (nticks - 1)) for k in range(nticks)}) if n > 1 else [0]

    # Shaded regions where JIT < interp (efficiency chart only)
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
                windows.append((s, k - 1))
                s = None

    def rolling(vals, w):
        if w < 2 or len(vals) < 3:
            return vals
        half = w // 2
        return [sum(vals[max(0, i - half):i + half + 1]) /
                len(vals[max(0, i - half):i + half + 1]) for i in range(len(vals))]

    for j, (panel_title, cells) in enumerate(groups):
        top = panel_top(j)
        parts.append(f'<rect x="{pad_l}" y="{top:.1f}" width="{plot_w}" height="{panel_h}" '
                     f'fill="#ffffff" stroke="#e6e6e6"/>')

        # Shaded regression regions
        for (a, b) in windows:
            x0 = xpos(a) - plot_w / n / 2
            x1 = xpos(b) + plot_w / n / 2
            parts.append(f'<rect x="{x0:.1f}" y="{top:.1f}" width="{(x1 - x0):.1f}" '
                         f'height="{panel_h}" fill="#e0662f" opacity="0.08"/>')

        # Gridlines
        for gv in grid_values:
            yy = yval(j, gv)
            parts.append(f'<line x1="{pad_l}" y1="{yy:.1f}" x2="{W - pad_r}" y2="{yy:.1f}" '
                         f'stroke="#ececec" stroke-width="1"/>')
            parts.append(f'<text x="{pad_l - 8}" y="{yy + 4:.1f}" font-size="10" fill="#999" '
                         f'text-anchor="end">{fmt_grid(gv)}</text>')

        # Vertical tick lines
        for i in tick_idx:
            x = xpos(i)
            parts.append(f'<line x1="{x:.1f}" y1="{top:.1f}" x2="{x:.1f}" '
                         f'y2="{top + panel_h:.1f}" stroke="#f3f3f3" stroke-width="1"/>')

        # Panel title
        parts.append(f'<text x="{pad_l + 10}" y="{top + 20:.1f}" font-size="14" '
                     f'font-weight="600" fill="#333">{panel_title}</text>')

        for name in cells:
            color = COLORS["interp"] if "interp" in name else COLORS["jitv2"]
            pts = [(i, col["cells"][name]) for i, col in enumerate(columns)
                   if name in col["cells"]]
            if not pts:
                continue

            # Raw points (faint)
            prev = None
            for i, v in pts:
                x, y = xpos(i), yval(j, v)
                h = columns[i]["host"]
                if prev is not None and (not break_on_host or prev[2] == h):
                    parts.append(f'<line x1="{prev[0]:.1f}" y1="{prev[1]:.1f}" x2="{x:.1f}" '
                                 f'y2="{y:.1f}" stroke="{color}" stroke-width="1" opacity="0.25"/>')
                parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="1.3" fill="{color}" opacity="0.25"/>')
                prev = (x, y, h)

            # Moving average trend line
            vals = [v for _, v in pts]
            window = max(5, len(vals) // 15)
            ys = rolling(vals, window)
            for k in range(1, len(pts)):
                x0, y0 = xpos(pts[k - 1][0]), yval(j, ys[k - 1])
                x1, y1 = xpos(pts[k][0]), yval(j, ys[k])
                parts.append(f'<line x1="{x0:.1f}" y1="{y0:.1f}" x2="{x1:.1f}" y2="{y1:.1f}" '
                             f'stroke="{color}" stroke-width="2.5" stroke-linecap="round"/>')

    # X-axis labels (dates)
    base_y = pad_t + ngroup * (panel_h + gap) - gap + (14 if show_host else 0) + 18
    for i in tick_idx:
        parts.append(f'<text x="{xpos(i):.1f}" y="{base_y + 22:.1f}" font-size="11" fill="#666" '
                     f'text-anchor="middle">{month_day(columns[i]["date"])}</text>')

    if windows:
        parts.append(f'<text x="{W - pad_r}" y="{base_y + 22:.1f}" font-size="11" fill="#b5571f" '
                     f'text-anchor="end">shaded: JIT slower than interpreter</text>')

    parts.append("</svg>")
    return "\n".join(parts)


# ──────────────────────────────────────────────────────────────────────────────
# Chart 4: JIT vs Interpreter Speedup over time
# ──────────────────────────────────────────────────────────────────────────────

def speedup_svg(entries):
    """JIT / Interpreter speedup factor over time for R4400 and R5000."""
    # Group by commit (some entries have only interp or only jitv2)
    by_commit = {}
    for e in entries:
        d = by_commit.setdefault(e["commit"], {"date": e["date"], "cells": {}})
        for c in e["cells"]:
            if c.get("mips") is not None:
                d["cells"][c["name"]] = _num(c["mips"])

    # Compute speedups
    speedups = []
    for commit, data in sorted(by_commit.items(), key=lambda x: x[1]["date"]):
        cells = data["cells"]
        r4400_interp = cells.get("r4400-interp")
        r4400_jit = cells.get("r4400-jitv2")
        r5000_interp = cells.get("r5000-interp")
        r5000_jit = cells.get("r5000-jitv2")

        if r4400_interp and r4400_jit and r4400_interp > 0:
            speedups.append({
                "date": data["date"],
                "cpu": "R4400",
                "speedup": r4400_jit / r4400_interp,
                "interp": r4400_interp,
                "jit": r4400_jit,
            })
        if r5000_interp and r5000_jit and r5000_interp > 0:
            speedups.append({
                "date": data["date"],
                "cpu": "R5000",
                "speedup": r5000_jit / r5000_interp,
                "interp": r5000_interp,
                "jit": r5000_jit,
            })

    if not speedups:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    W, H = 900, 420
    pad_l, pad_r, pad_t, pad_b = 80, 40, 70, 60
    plot_w = W - pad_l - pad_r
    plot_h = H - pad_t - pad_b

    n = len(speedups)
    dates = [s["date"] for s in speedups]

    # Group by CPU
    r4400 = [s for s in speedups if s["cpu"] == "R4400"]
    r5000 = [s for s in speedups if s["cpu"] == "R5000"]

    all_speedups = [s["speedup"] for s in speedups]
    ymin, ymax = 0.5, max(all_speedups) * 1.15
    # Log scale for speedup
    logspan = math.log10(ymax / ymin)

    def xpos(i):
        return pad_l + plot_w * (i + 0.5) / n

    def yval(v):
        frac = math.log10(max(v, ymin) / ymin) / logspan
        return pad_t + plot_h - frac * plot_h

    months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
              "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

    def month_day(date):
        try:
            _, m_, d_ = date[:10].split("-")
            return f"{int(d_)} {months[int(m_) - 1]}"
        except Exception:
            return date[:10]

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
             f'viewBox="0 0 {W} {H}" font-family="system-ui,sans-serif">']

    parts.append(f'<text x="{pad_l}" y="28" font-size="18" font-weight="600" fill="#222">'
                 f'JIT vs Interpreter Speedup Factor Over Time</text>')
    parts.append(f'<text x="{pad_l}" y="48" font-size="12" fill="#666">'
                 f'Ratio of guest MIPS (jitv2 / interp) · log scale · {len(speedups)} data points</text>')

    # Baseline at 1.0
    y1 = yval(1.0)
    parts.append(f'<line x1="{pad_l}" y1="{y1:.1f}" x2="{W - pad_r}" y2="{y1:.1f}" '
                 f'stroke="#888" stroke-width="1.5" stroke-dasharray="6,4"/>')
    parts.append(f'<text x="{pad_l - 8}" y="{y1 - 4:.1f}" font-size="10" fill="#888" text-anchor="end">1.0×</text>')

    # Gridlines
    for v in [1.0, 2.0, 5.0, 10.0, 20.0, 50.0]:
        if v < ymax * 1.05:
            yy = yval(v)
            parts.append(f'<line x1="{pad_l}" y1="{yy:.1f}" x2="{W - pad_r}" y2="{yy:.1f}" '
                         f'stroke="#ececec" stroke-width="1"/>')
            parts.append(f'<text x="{pad_l - 8}" y="{yy + 4:.1f}" font-size="10" fill="#999" text-anchor="end">'
                         f'{v:.0f}×</text>')

    # Plot each CPU
    for cpu_name, data, color in [("R4400", r4400, COLORS["interp"]), ("R5000", r5000, COLORS["jitv2"])]:
        if not data:
            continue
        pts = [(i, s["speedup"]) for i, s in enumerate(speedups) if s["cpu"] == cpu_name]
        if not pts:
            continue

        # Points
        for i, v in pts:
            x, y = xpos(i), yval(v)
            parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="3" fill="{color}" opacity="0.7"/>')
            parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="5" fill="none" stroke="{color}" stroke-width="1.5"/>')

        # Connect lines
        for k in range(1, len(pts)):
            x0, y0 = xpos(pts[k - 1][0]), yval(pts[k - 1][1])
            x1, y1 = xpos(pts[k][0]), yval(pts[k][1])
            parts.append(f'<line x1="{x0:.1f}" y1="{y0:.1f}" x2="{x1:.1f}" y2="{y1:.1f}" '
                         f'stroke="{color}" stroke-width="2" opacity="0.6" stroke-linecap="round"/>')

        # Latest value label
        last = pts[-1]
        x, y = xpos(last[0]), yval(last[1])
        parts.append(f'<text x="{x + 8:.1f}" y="{y + 4:.1f}" font-size="11" font-weight="600" fill="{color}">'
                     f'{last[1]:.1f}×</text>')

    # Legend
    parts.append(f'<rect x="{pad_l}" y="{H - 38}" width="12" height="12" fill="{COLORS["interp"]}"/>')
    parts.append(f'<text x="{pad_l + 16}" y="{H - 28}" font-size="12" fill="#333">R4400 speedup</text>')
    parts.append(f'<rect x="{pad_l + 130}" y="{H - 38}" width="12" height="12" fill="{COLORS["jitv2"]}"/>')
    parts.append(f'<text x="{pad_l + 146}" y="{H - 28}" font-size="12" fill="#333">R5000 speedup</text>')

    # X-axis labels
    nticks = min(8, n)
    tick_idx = sorted({round(k * (n - 1) / (nticks - 1)) for k in range(nticks)}) if n > 1 else [0]
    for i in tick_idx:
        parts.append(f'<text x="{xpos(i):.1f}" y="{H - 10:.1f}" font-size="11" fill="#666" '
                     f'text-anchor="middle">{month_day(dates[i])}</text>')

    parts.append("</svg>")
    return "\n".join(parts)


# ──────────────────────────────────────────────────────────────────────────────
# Chart 5: Efficiency by kernel group
# ──────────────────────────────────────────────────────────────────────────────

def groups_svg(entries):
    """Efficiency by kernel group for the latest run with both interp and jitv2."""
    entry = latest(entries)
    if not entry:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    # Find entries that have group efficiency data
    cells_with_groups = {}
    for c in entry["cells"]:
        if c.get("groups"):
            cells_with_groups[c["name"]] = c["groups"]

    if not cells_with_groups:
        # Check if any entry has group data
        for e in reversed(entries):
            for c in e["cells"]:
                if c.get("groups"):
                    cells_with_groups[c["name"]] = c["groups"]
            if cells_with_groups:
                entry = e
                break

    if not cells_with_groups:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    # We want interp vs jitv2 for each CPU
    W, H = 880, 400
    pad_l, pad_r, pad_t, pad_b = 80, 30, 60, 50
    plot_w = W - pad_l - pad_r
    plot_h = H - pad_t - pad_b

    # Groups to show
    groups_to_show = [g for g in GROUPS if any(g in cg for cg in cells_with_groups.values())]
    if not groups_to_show:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    n_groups = len(groups_to_show)
    group_w = plot_w / n_groups
    bar_w = group_w * 0.3

    # Find max efficiency for scaling
    all_eff = []
    for cg in cells_with_groups.values():
        all_eff.extend(cg.values())
    vmax = max(all_eff) * 100 * 1.2 if all_eff else 100

    def y(v):
        return pad_t + plot_h - (v * 100 / vmax) * plot_h if vmax > 0 else pad_t + plot_h

    def x_group(i):
        return pad_l + i * group_w + group_w / 2

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
             f'viewBox="0 0 {W} {H}" font-family="system-ui,sans-serif">']

    parts.append(f'<text x="{pad_l}" y="28" font-size="18" font-weight="600" fill="#222">'
                 f'Efficiency by Kernel Group — {entry["source"]} {entry["commit"][:8]} ({entry["date"][:10]})</text>')
    parts.append(f'<text x="{pad_l}" y="48" font-size="12" fill="#666">'
                 f'Fraction of native host speed per kernel group · host: {entry["host"].get("cpu", "?")}</text>')

    # Gridlines
    for i in range(5):
        v = vmax * i / 4
        yy = y(v / 100)
        parts.append(f'<line x1="{pad_l}" y1="{yy:.1f}" x2="{W - pad_r}" y2="{yy:.1f}" '
                     f'stroke="#e5e5e5" stroke-width="1"/>')
        parts.append(f'<text x="{pad_l - 8}" y="{yy + 4:.1f}" font-size="10" fill="#888" text-anchor="end">'
                     f'{v:.0f}%</text>')

    # Bars for each group, interp vs jitv2 per CPU
    cpu_pairs = [("R4400", "r4400-interp", "r4400-jitv2"), ("R5000", "r5000-interp", "r5000-jitv2")]
    pair_colors = [("#5b8db8", "#c9763f"), ("#4a7ab8", "#b86632")]

    for pi, (cpu_label, interp_name, jit_name) in enumerate(cpu_pairs):
        interp_groups = cells_with_groups.get(interp_name, {})
        jit_groups = cells_with_groups.get(jit_name, {})

        for gi, group in enumerate(groups_to_show):
            cx = x_group(gi)
            pair_w = group_w * 0.4
            pair_start = cx - pair_w

            # Interp bar
            iev = interp_groups.get(group, 0)
            ix = pair_start + (0 if pi == 0 else pair_w / 2 + 2)
            parts.append(f'<rect x="{ix:.1f}" y="{y(iev):.1f}" width="{bar_w:.1f}" '
                         f'height="{plot_h - (y(iev) - pad_t):.1f}" '
                         f'fill="{pair_colors[pi][0]}" opacity="0.9"/>')
            if iev > 0.001:
                parts.append(f'<text x="{ix + bar_w/2:.1f}" y="{y(iev) - 4:.1f}" font-size="9" fill="#333" text-anchor="middle">'
                             f'{iev*100:.1f}%</text>')

            # JIT bar
            jev = jit_groups.get(group, 0)
            jx = pair_start + bar_w + 4 + (pair_w / 2 if pi == 1 else 0)
            parts.append(f'<rect x="{jx:.1f}" y="{y(jev):.1f}" width="{bar_w:.1f}" '
                         f'height="{plot_h - (y(jev) - pad_t):.1f}" '
                         f'fill="{pair_colors[pi][1]}" opacity="0.9"/>')
            if jev > 0.001:
                parts.append(f'<text x="{jx + bar_w/2:.1f}" y="{y(jev) - 4:.1f}" font-size="9" fill="#333" text-anchor="middle">'
                             f'{jev*100:.1f}%</text>')

            # Speedup factor label
            if iev > 0 and jev > 0:
                speedup = jev / iev
                sx = pair_start + pair_w / 2
                parts.append(f'<text x="{sx:.1f}" y="{min(y(iev), y(jev)) - 18:.1f}" font-size="9" fill="#666" text-anchor="middle">'
                             f'{speedup:.1f}×</text>')

        # CPU label
        lx = pad_l + (0 if pi == 0 else plot_w / 2)
        parts.append(f'<text x="{lx + 10}" y="{pad_t - 8}" font-size="12" font-weight="600" fill="#444">{cpu_label}</text>')

    # Group labels on x-axis
    for gi, group in enumerate(groups_to_show):
        cx = x_group(gi)
        parts.append(f'<text x="{cx:.1f}" y="{H - pad_b + 18:.1f}" font-size="12" fill="#333" text-anchor="middle">'
                     f'{_group_label(group)}</text>')

    # Legend
    ly = H - 28
    parts.append(f'<rect x="{pad_l}" y="{ly}" width="12" height="12" fill="#5b8db8"/>')
    parts.append(f'<text x="{pad_l + 16}" y="{ly + 9}" font-size="11" fill="#444">Interpreter</text>')
    parts.append(f'<rect x="{pad_l + 100}" y="{ly}" width="12" height="12" fill="#c9763f"/>')
    parts.append(f'<text x="{pad_l + 116}" y="{ly + 9}" font-size="11" fill="#444">JIT (jitv2)</text>')
    parts.append(f'<text x="{W - pad_r - 80}" y="{ly + 9}" font-size="11" fill="#666" text-anchor="end">'
                 f'⬆ = JIT faster</text>')

    parts.append("</svg>")
    return "\n".join(parts)


# ──────────────────────────────────────────────────────────────────────────────
# Chart 6: Heatmap of group efficiency across commits
# ──────────────────────────────────────────────────────────────────────────────

def heatmap_svg(entries):
    """Heatmap: commits (x) × kernel groups (y) colored by JIT efficiency."""
    # Collect all entries that have group efficiency data
    heatmap_data = []
    for e in entries:
        groups_by_cell = {}
        for c in e["cells"]:
            if c.get("groups") and "jitv2" in c["name"]:
                groups_by_cell[c["name"]] = c["groups"]
        if groups_by_cell:
            heatmap_data.append({
                "date": e["date"],
                "commit": e["commit"][:8],
                "groups": groups_by_cell,
            })

    if len(heatmap_data) < 3:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    # Limit to last 80 commits for readability
    heatmap_data = heatmap_data[-80:]
    n_commits = len(heatmap_data)

    # Determine which groups appear
    all_groups = set()
    for d in heatmap_data:
        for g in d["groups"].values():
            all_groups.update(g.keys())
    groups_to_show = [g for g in GROUPS if g in all_groups]

    if not groups_to_show:
        return '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'

    W, H = 1000, 300
    pad_l, pad_r, pad_t, pad_b = 140, 20, 50, 40
    cell_w = (W - pad_l - pad_r) / n_commits
    cell_h = (H - pad_t - pad_b) / len(groups_to_show)

    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
             f'viewBox="0 0 {W} {H}" font-family="system-ui,sans-serif">']

    parts.append(f'<text x="{pad_l}" y="28" font-size="16" font-weight="600" fill="#222">'
                 f'JIT Efficiency Heatmap by Kernel Group ({n_commits} recent commits)</text>')
    parts.append(f'<text x="{pad_l}" y="44" font-size="11" fill="#666">'
                 f'Color = efficiency (fraction of native host speed) · only JIT cells shown</text>')

    # Color scale: blue (low) → yellow → red (high)
    def eff_color(v):
        # v is 0-1 efficiency
        v = max(0, min(1, v))
        if v < 0.5:
            # Blue to yellow
            r = int(255 * v * 2)
            g = int(255 * v * 2)
            b = int(255 * (1 - v * 2))
        else:
            # Yellow to red
            r = 255
            g = int(255 * (2 - v * 2))
            b = 0
        return f"rgb({r},{g},{b})"

    # Draw cells
    for gi, group in enumerate(groups_to_show):
        for ci, commit_data in enumerate(heatmap_data):
            # Average efficiency across JIT cells for this group
            effs = []
            for cell_groups in commit_data["groups"].values():
                if group in cell_groups:
                    effs.append(cell_groups[group])
            if not effs:
                continue
            avg_eff = sum(effs) / len(effs)

            x = pad_l + ci * cell_w
            y = pad_t + gi * cell_h
            color = eff_color(avg_eff)

            parts.append(f'<rect x="{x:.1f}" y="{y:.1f}" width="{max(1, cell_w - 1):.1f}" '
                         f'height="{max(1, cell_h - 1):.1f}" fill="{color}"/>')

            # Show value if cell is large enough
            if cell_w > 18 and cell_h > 16:
                parts.append(f'<text x="{x + cell_w/2:.1f}" y="{y + cell_h/2 + 3:.1f}" '
                             f'font-size="8" fill="#000" text-anchor="middle" opacity="0.7">'
                             f'{avg_eff*100:.0f}%</text>')

    # Y-axis labels (groups)
    for gi, group in enumerate(groups_to_show):
        y = pad_t + gi * cell_h + cell_h / 2
        parts.append(f'<text x="{pad_l - 10}" y="{y + 4:.1f}" font-size="11" fill="#333" text-anchor="end">'
                     f'{_group_label(group)}</text>')

    # X-axis labels (every Nth commit)
    step = max(1, n_commits // 20)
    for ci in range(0, n_commits, step):
        x = pad_l + ci * cell_w + cell_w / 2
        date_str = heatmap_data[ci]["date"][:10]
        parts.append(f'<text x="{x:.1f}" y="{H - pad_b + 16:.1f}" font-size="9" fill="#666" '
                     f'text-anchor="middle" transform="rotate(-45 {x:.1f} {H - pad_b + 16})">{date_str}</text>')

    # Color legend
    legend_y = H - pad_b + 30
    for i in range(11):
        v = i / 10
        x = pad_l + i * (W - pad_l - pad_r) / 10
        color = eff_color(v)
        parts.append(f'<rect x="{x:.1f}" y="{legend_y}" width="{(W - pad_l - pad_r) / 10:.1f}" '
                     f'height="12" fill="{color}"/>')
        if i % 2 == 0:
            parts.append(f'<text x="{x:.1f}" y="{legend_y + 24}" font-size="9" fill="#666" text-anchor="middle">'
                         f'{v*100:.0f}%</text>')
    parts.append(f'<text x="{pad_l}" y="{legend_y - 4}" font-size="10" fill="#666">Efficiency →</text>')

    parts.append("</svg>")
    return "\n".join(parts)


# ──────────────────────────────────────────────────────────────────────────────
# History markdown table with sparklines
# ──────────────────────────────────────────────────────────────────────────────

def _sparkline(vals, width=80, height=16):
    """Generate a tiny inline SVG sparkline."""
    if not vals or len(vals) < 2:
        return ""
    vmin, vmax = min(vals), max(vals)
    if vmax == vmin:
        return ""
    scale = height / (vmax - vmin)
    points = []
    for i, v in enumerate(vals):
        x = i * width / (len(vals) - 1)
        y = height - (v - vmin) * scale
        points.append(f"{x:.1f},{y:.1f}")
    path = "M " + " L ".join(points)
    return (f'<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" '
            f'style="vertical-align:middle"><path d="{path}" stroke="#5b8db8" stroke-width="1.5" '
            f'fill="none"/></svg>')


def history_md(entries):
    lines = ["# Benchmark History", "",
             "One row per recorded CI run. Throughput figures are only comparable "
             "within the same host CPU — GitHub's shared runners vary between runs, "
             "so cross-host MIPS deltas are host noise, not code change.", ""]

    # Build sparkline data per cell
    cell_series = {name: [] for name in CELLS}
    cell_dates = {name: [] for name in CELLS}
    for e in sorted(entries, key=lambda e: e["date"]):
        cell_map = {c["name"]: c for c in e["cells"]}
        for n in CELLS:
            c = cell_map.get(n)
            if c and c.get("mips") is not None:
                cell_series[n].append(_num(c["mips"]))
                cell_dates[n].append(e["date"][:10])

    entries = sorted(entries, key=lambda e: e["date"])

    # Header with sparklines
    header = ["date", "source", "commit", "host"] + CELLS
    header_md = "| " + " | ".join(header) + " |"
    sep_md = "|---|---|---|---|" + "|---:".join([""] * len(CELLS)) + "|"
    lines.append(header_md)
    lines.append(sep_md)

    for e in entries:
        cell_map = {c["name"]: c for c in e["cells"]}
        row = [e["date"][:10], e["source"], f'`{e["commit"][:8]}`', e["host"].get("cpu", "?")]
        for n in CELLS:
            c = cell_map.get(n)
            if c and c["mips"] is not None:
                spark = _sparkline(cell_series[n][-20:])  # last 20 points
                row.append(f'{_num(c["mips"]):.1f} {spark}')
            else:
                row.append("—")
        lines.append("| " + " | ".join(row) + " |")

    lines.append("")
    return "\n".join(lines)


def has_efficiency(entries) -> bool:
    return any(c.get("efficiency") is not None for e in entries for c in e["cells"])


def has_groups(entries) -> bool:
    return any(c.get("groups") for e in entries for c in e["cells"])


def groq_analysis(entry, entries):
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
            "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120.0 Safari/537.36",
        },
    )
    try:
        resp = json.load(urllib.request.urlopen(req, timeout=30))
        text = resp["choices"][0]["message"]["content"].strip()
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
                f"({jitv2/interp:.1f}x speedup).")
    return ""


def readme_block(entry, entries, analysis):
    cells_md = "\n".join(
        f"| `{c['name']}` | {c['cpu']} | {c['accuracy']:.1f}% | {_num(c['mips']):.1f} | "
        f"{_num(c['dmips']):.1f} | {_num(c['linpack']):.1f} | {_num(c['whet']):.1f} |"
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

    parts.append("See **[BENCHMARKS.md](BENCHMARKS.md)** for the full benchmark suite documentation: "
                 "bare-metal kernels, IRIX workloads, CI workflows, data pipeline, and generated charts.")
    parts.append("")

    parts.append("![latest benchmark cells](data/bench_cells.svg)")
    parts.append("")
    parts.append("Latest run (4 cells):")
    parts.append("")
    parts.append("| cell | CPU | accuracy | MIPS | DMIPS | LINPACK MFLOPS | Whetstone k/s |")
    parts.append("|---|---|---:|---:|---:|---:|---:|")
    parts.append(cells_md)
    parts.append("")

    parts.append("Charts (auto-regenerated from CI history):")
    parts.append("")
    parts.append("- [Raw MIPS history](data/bench_history.svg) — log scale, host-break lines")
    parts.append("- [Normalised efficiency](data/bench_history_eff.svg) — % of native host speed")
    parts.append("- [JIT speedup](data/bench_speedup.svg) — JIT/interp ratio over time")
    parts.append("- [Efficiency by group](data/bench_groups.svg) — int/fpu/mem/img/vid/codec/sys")
    parts.append("- [Group heatmap](data/bench_heatmap.svg) — 80 commits × 7 groups")
    parts.append("- [Full history table](data/bench_history.md) — with inline sparklines")
    parts.append("")

    parts.append("Profile artifacts (Rust flamegraphs): see `bench-profile.yml` workflow, uploaded per commit.")
    parts.append("")
    parts.append(MARKER)
    return "\n".join(parts)


def rewrite_readme(block):
    text = README.read_text() if README.exists() else ""
    if MARKER in text:
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

    # Generate all SVGs
    SVG.write_text(bar_chart_svg(entry) + "\n")
    print(f"wrote {SVG.name}")

    HIST_SVG.write_text(history_svg(entries, "mips") + "\n")
    print(f"wrote {HIST_SVG.name}")

    if has_efficiency(entries):
        HIST_EFF_SVG.write_text(history_svg(entries, "efficiency") + "\n")
        print(f"wrote {HIST_EFF_SVG.name}")

    SPEEDUP_SVG.write_text(speedup_svg(entries) + "\n")
    print(f"wrote {SPEEDUP_SVG.name}")

    if has_groups(entries):
        GROUPS_SVG.write_text(groups_svg(entries) + "\n")
        print(f"wrote {GROUPS_SVG.name}")
        HEATMAP_SVG.write_text(heatmap_svg(entries) + "\n")
        print(f"wrote {HEATMAP_SVG.name}")

    HIST_MD.write_text(history_md(entries))
    print(f"wrote {HIST_MD.name}")

    if not args.no_readme:
        analysis = groq_analysis(entry, entries) or fallback_analysis(entry)
        rewrite_readme(readme_block(entry, entries, analysis))
        print(f"rewrote {README.name}")


if __name__ == "__main__":
    main()