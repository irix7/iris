#!/usr/bin/env python3
"""aotbench — a repeatable measurement of the jitv2 AOT first-boot win.

The #54 spike (docs/jitv2-aot-cache-spike.md) concluded GO on feasibility but
NO-GO on shipping a prewarmed cache *until a release/CI first-boot win is
measured*. Its measurement needs a booted IRIX and a `.pcp` corpus. This tool
is that measurement, made repeatable: given an IRIX disk config and a corpus,
it

  1. fills the persistent page cache offline with `jitv2_aot` (the AOT fill),
  2. boots the same image once with the persistent cache **off** and once with
     it **prewarmed**, and
  3. reports time to the first framebuffer change and time to the login
     prompt, plus the cache's own lookup/hit totals.

It does not make the end-to-end number appear out of nowhere: **you need IRIX
media and a real guest boot.** Without one this tool cannot run; the guest-free
half of the measurement is the `lookup_cost_microbench` unit test in
`src/cpu/jitv2/pcache.rs` (see the README).

Boots are single-process and isolated per run: the emulator runs from a fresh
working directory, so NVRAM and any relative state files are per-run. The disk
image is *not* copied; pass a config whose disk is read-only or COW-backed so
every run boots the same bytes (see README).

Usage:

  aotbench.py --iris target/release/iris --iris-ci target/release/iris-ci \\
              --config iris.toml --corpus /path/to/corpus

Run `aotbench.py --help` for the full option set.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path


# ─── small process helpers ───────────────────────────────────────────────────

def run(cmd, *, timeout, env=None, cwd=None, check=True, capture=True):
    """Run a command, returning CompletedProcess. Raises on timeout."""
    try:
        p = subprocess.run(
            [str(c) for c in cmd],
            timeout=timeout,
            env=env,
            cwd=cwd,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.STDOUT if capture else None,
            text=True,
        )
    except subprocess.TimeoutExpired as e:
        out = e.stdout or ""
        raise RuntimeError(f"timeout after {timeout}s: {' '.join(map(str, cmd))}\n{out}") from None
    if check and p.returncode != 0:
        raise RuntimeError(
            f"exit {p.returncode}: {' '.join(map(str, cmd))}\n{p.stdout or ''}"
        )
    return p


def ci(iris_ci: Path, sock: Path, *args, timeout=120, check=True):
    """One `iris-ci` invocation against the run's socket."""
    return run([iris_ci, "--socket", sock, *args], timeout=timeout, check=check)


# ─── run model ───────────────────────────────────────────────────────────────

@dataclass
class RunResult:
    arm: str
    rep: int
    first_frame_ms: float | None
    login_ms: float | None
    lookups: int | None
    hits: int | None
    hit_pct: float | None
    stored: int | None
    load_us_avg: float | None
    jitcache_lines: list[str] = field(default_factory=list)


JITCACHE_RE = re.compile(
    r"jitcache: lookups=(\d+) hits=(\d+) \(([\d.]+)%\) stored=(\d+) "
    r"refused=(\d+) union_added=(\d+) compare_fail=(\d+) bad_files=(\d+) "
    r"load_us_avg=([\d.]+)"
)


def parse_jitcache(text: str) -> list[dict]:
    out = []
    for line in text.splitlines():
        m = JITCACHE_RE.search(line)
        if not m:
            continue
        out.append(
            {
                "line": line.strip(),
                "lookups": int(m.group(1)),
                "hits": int(m.group(2)),
                "hit_pct": float(m.group(3)),
                "stored": int(m.group(4)),
                "refused": int(m.group(5)),
                "union_added": int(m.group(6)),
                "compare_fail": int(m.group(7)),
                "bad_files": int(m.group(8)),
                "load_us_avg": float(m.group(9)),
            }
        )
    return out


# ─── one boot ────────────────────────────────────────────────────────────────

def wait_for_socket(sock: Path, proc: subprocess.Popen, timeout_s: float) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if sock.exists():
            return
        if proc.poll() is not None:
            raise RuntimeError(f"iris exited early (rc={proc.returncode}) before the CI socket appeared")
        time.sleep(0.1)
    raise RuntimeError(f"CI socket {sock} never appeared within {timeout_s}s")


def screenshot_hash(iris_ci: Path, sock: Path, png: Path) -> str | None:
    p = ci(iris_ci, sock, "screenshot", str(png), timeout=60, check=False)
    if p.returncode != 0 or not png.exists():
        return None
    return hashlib.sha256(png.read_bytes()).hexdigest()


def boot_once(
    *,
    iris: Path,
    iris_ci: Path,
    config: Path,
    cpu: str,
    cache_enabled: bool,
    cache_dir: Path,
    work: Path,
    marker: str,
    timeout_s: float,
    rep: int,
    arm: str,
    poll_s: float,
) -> RunResult:
    run_dir = work / f"{arm}-{rep}"
    run_dir.mkdir(parents=True, exist_ok=True)
    sock = run_dir / "iris.sock"
    serial_log = run_dir / "serial.log"
    png = run_dir / "frame.png"
    stderr_path = run_dir / "iris.stderr"

    env = dict(os.environ)
    env["IRIS_JIT_CACHE"] = "1" if cache_enabled else "off"
    if cache_enabled:
        env["IRIS_JIT_CACHE_DIR"] = str(cache_dir)
    else:
        env.pop("IRIS_JIT_CACHE_DIR", None)

    cmd = [
        iris,
        "--config", config,
        "--cpu", cpu,
        "--ci",
        "--ci-socket", sock,
        "--serial-log", serial_log,
    ]
    with open(stderr_path, "w") as err:
        proc = subprocess.Popen(
            [str(c) for c in cmd],
            cwd=run_dir,
            env=env,
            stdout=err,
            stderr=subprocess.STDOUT,
        )
    try:
        wait_for_socket(sock, proc, timeout_s=min(60.0, timeout_s))

        # Baseline frame: the reset screen, before the CPU starts. The display
        # thread presents it regardless of CPU state, so this is stable.
        baseline = screenshot_hash(iris_ci, sock, png)

        t0 = time.monotonic()
        ci(iris_ci, sock, "start", timeout=30)

        # First framebuffer change from the reset screen. This is the
        # "time to first frame" proxy: the PROM's first drawn output. If the
        # display is unavailable (headless) or the probe fails, it stays None.
        first_frame_ms: float | None = None
        if baseline is not None:
            deadline = t0 + timeout_s
            while time.monotonic() < deadline:
                h = screenshot_hash(iris_ci, sock, png)
                if h is not None and h != baseline:
                    first_frame_ms = (time.monotonic() - t0) * 1000.0
                    break
                time.sleep(poll_s)

        # Boot to the login marker, the endpoint the persistent-cache design
        # measured (docs/jitv2-persistent-cache.md, "boot to login:").
        login_ms: float | None = None
        opt = ci(iris_ci, sock, "serial-wait", "Option?", "--timeout", "30",
                 timeout=40, check=False)
        if opt.returncode == 0:
            # Only answer the PROM menu if we actually saw it; an auto-booting
            # guest would otherwise get a stray "1" typed at it.
            ci(iris_ci, sock, "serial-send", "1", timeout=20, check=False)
        p = ci(iris_ci, sock, "serial-wait", marker, "--timeout", str(int(timeout_s)),
               timeout=timeout_s + 30, check=False)
        if p.returncode == 0:
            login_ms = (time.monotonic() - t0) * 1000.0

        # Exact cache totals: `summary()` prints every 500 lookups, so the
        # last line is usually current but not guaranteed exact. Recorded
        # as-is; the README says so.
        ci(iris_ci, sock, "quit", timeout=60, check=False)
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()

    text = stderr_path.read_text(errors="replace") if stderr_path.exists() else ""
    stats = parse_jitcache(text)
    last = stats[-1] if stats else None
    return RunResult(
        arm=arm,
        rep=rep,
        first_frame_ms=first_frame_ms,
        login_ms=login_ms,
        lookups=last["lookups"] if last else None,
        hits=last["hits"] if last else None,
        hit_pct=last["hit_pct"] if last else None,
        stored=last["stored"] if last else None,
        load_us_avg=last["load_us_avg"] if last else None,
        jitcache_lines=[s["line"] for s in stats],
    )


# ─── offline fill ────────────────────────────────────────────────────────────

def fill_cache(jitv2_aot: Path, corpus: Path, iris: Path, cache_dir: Path, cpu: str) -> dict:
    cache_dir.mkdir(parents=True, exist_ok=True)
    p = run(
        [jitv2_aot, corpus, "--iris-exe", iris, "--cache-dir", cache_dir, "--cpu", cpu],
        timeout=3 * 3600,
    )
    text = p.stdout or ""
    summary = {}
    m = re.search(
        r"pages=(\d+) entries=(\d+) compiled=(\d+) walk_declined=(\d+) "
        r"codegen_declined=(\d+) relocation_refused=(\d+) stored=(\d+) "
        r"bytes=(\d+) io_failed=(\d+)",
        text,
    )
    if m:
        keys = ["pages", "entries", "compiled", "walk_declined", "codegen_declined",
                "relocation_refused", "stored", "bytes", "io_failed"]
        summary = {k: int(v) for k, v in zip(keys, m.groups())}
    return {"summary": summary, "raw": text}


# ─── corpus capture (optional) ───────────────────────────────────────────────

def capture_corpus(
    *, iris: Path, iris_ci: Path, config: Path, cpu: str, work: Path,
    corpus: Path, marker: str, timeout_s: float, monitor_port: int,
) -> None:
    """Boot once, let the workload run, then `j2 corpus <dir>` over the monitor.

    The monitor is a plain line console on TCP; this connects, waits for the
    `> ` prompt, sends the command and waits for the tool's own "wrote N
    page(s)" line. Best-effort: if the monitor can't be reached, it says so
    and leaves the corpus alone.
    """
    run_dir = work / "capture"
    run_dir.mkdir(parents=True, exist_ok=True)
    sock = run_dir / "iris.sock"
    env = dict(os.environ)
    env["IRIS_JIT_CACHE"] = "off"
    cmd = [iris, "--config", config, "--cpu", cpu, "--ci",
           "--ci-socket", sock, "--monitor-port", str(monitor_port)]
    with open(run_dir / "iris.stderr", "w") as err:
        proc = subprocess.Popen([str(c) for c in cmd], cwd=run_dir, env=env,
                                stdout=err, stderr=subprocess.STDOUT)
    try:
        wait_for_socket(sock, proc, timeout_s=60)
        ci(iris_ci, sock, "start", timeout=30)
        opt = ci(iris_ci, sock, "serial-wait", "Option?", "--timeout", "30",
                 timeout=40, check=False)
        if opt.returncode == 0:
            ci(iris_ci, sock, "serial-send", "1", timeout=20, check=False)
        ci(iris_ci, sock, "serial-wait", marker, "--timeout", str(int(timeout_s)),
           timeout=timeout_s + 30, check=False)
        corpus.mkdir(parents=True, exist_ok=True)
        with socket.create_connection(("127.0.0.1", monitor_port), timeout=30) as s:
            s.settimeout(600)
            buf = b""
            s.sendall(f"j2 corpus {corpus}\n".encode())
            deadline = time.monotonic() + 600
            while time.monotonic() < deadline:
                try:
                    chunk = s.recv(4096)
                except socket.timeout:
                    break
                if not chunk:
                    break
                buf += chunk
                if b"j2 corpus: wrote" in buf:
                    break
            print(buf.decode(errors="replace").strip())
        ci(iris_ci, sock, "quit", timeout=60, check=False)
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()


# ─── reporting ───────────────────────────────────────────────────────────────

def median(vals: list[float | None]) -> float | None:
    present = [v for v in vals if v is not None]
    return statistics.median(present) if present else None


def fmt_ms(v: float | None) -> str:
    return "   n/a" if v is None else f"{v:7.0f}"


def report(results: list[RunResult], fills: dict, out_json: Path | None) -> int:
    print()
    print("── jitv2 AOT first-boot measurement ────────────────────────────────────")
    if fills:
        s = fills.get("summary") or {}
        if s:
            print(f"offline fill: pages={s.get('pages')} compiled={s.get('compiled')} "
                  f"stored={s.get('stored')} relocation_refused={s.get('relocation_refused')} "
                  f"bytes={s.get('bytes')}")
        raw = (fills.get("raw") or "").strip()
        print(f"fill raw: {raw.splitlines()[-1] if raw.splitlines() else ''}")
    print()
    hdr = f"{'arm':<12} {'first-frame ms':>14} {'login ms':>10} {'lookups':>9} {'hits':>7} {'hit %':>7} {'load us':>8}"
    print(hdr)
    print("-" * len(hdr))

    arms: dict[str, list[RunResult]] = {}
    for r in results:
        arms.setdefault(r.arm, []).append(r)
    for arm, rs in arms.items():
        ff = median([r.first_frame_ms for r in rs])
        login = median([r.login_ms for r in rs])
        lookups = median([float(r.lookups) for r in rs if r.lookups is not None])
        hits = median([float(r.hits) for r in rs if r.hits is not None])
        hitp = median([r.hit_pct for r in rs if r.hit_pct is not None])
        load = median([r.load_us_avg for r in rs if r.load_us_avg is not None])
        lookups_s = "n/a" if lookups is None else f"{lookups:.0f}"
        hits_s = "n/a" if hits is None else f"{hits:.0f}"
        hitp_s = "n/a" if hitp is None else f"{hitp:.1f}"
        load_s = "n/a" if load is None else f"{load:.0f}"
        print(f"{arm:<12} {fmt_ms(ff):>14} {fmt_ms(login):>10} "
              f"{lookups_s:>9} {hits_s:>7} {hitp_s:>7} {load_s:>8}")

    baseline = arms.get("nocache", [])
    warm = arms.get("warmaot", [])
    if baseline and warm:
        b = median([r.login_ms for r in baseline])
        w = median([r.login_ms for r in warm])
        if b is not None and w is not None:
            print()
            print(f"first-boot win (login): {b - w:+.0f} ms  ({100.0*(b-w)/b:+.1f}%)  "
                  f"without cache {b:.0f} ms -> prewarmed {w:.0f} ms")

    print()
    print("NOTE: a real number requires an IRIX boot; this run is only as good as")
    print("      the guest that produced it. See test/aotbench/README.md.")

    if out_json is not None:
        out_json.write_text(json.dumps(
            {
                "fill": fills,
                "runs": [r.__dict__ for r in results],
            },
            indent=2,
        ))
        print(f"wrote {out_json}")
    return 0


# ─── main ────────────────────────────────────────────────────────────────────

def read_repo_iris_exe() -> Path | None:
    # Best-effort default: the release build this repo's own measurements use.
    for p in ("target/release/iris", "target/debug/iris"):
        if Path(p).exists():
            return Path(p)
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--iris", type=Path, default=read_repo_iris_exe(),
                    help="emulator binary (default: target/release/iris if present)")
    ap.add_argument("--iris-ci", type=Path, default=Path("target/release/iris-ci"),
                    help="iris-ci binary")
    ap.add_argument("--jitv2-aot", type=Path, default=Path("target/release/jitv2_aot"),
                    help="offline AOT filler (build with --features jitv2)")
    ap.add_argument("--config", type=Path, required=True, help="iris.toml for the IRIX image")
    ap.add_argument("--corpus", type=Path, required=True,
                    help="directory of .pcp files (from `j2 corpus`), or one .pcp file")
    ap.add_argument("--cpu", default="r10000", choices=["r4400", "r5000", "r10000"])
    ap.add_argument("--marker", default="IRIS console login",
                    help="serial login marker (default: 'IRIS console login')")
    ap.add_argument("--reps", type=int, default=3, help="boots per arm (default 3)")
    ap.add_argument("--timeout", type=float, default=900.0,
                    help="per-boot timeout seconds (default 900)")
    ap.add_argument("--poll", type=float, default=0.15,
                    help="screenshot poll interval seconds (default 0.15)")
    ap.add_argument("--work-dir", type=Path, default=None,
                    help="scratch directory (default: mkdtemp)")
    ap.add_argument("--json", type=Path, default=None, help="write results JSON here")
    ap.add_argument("--capture-corpus", action="store_true",
                    help="boot once and `j2 corpus <--corpus>` before measuring")
    ap.add_argument("--monitor-port", type=int, default=8888,
                    help="monitor TCP port for --capture-corpus (default 8888)")
    ap.add_argument("--no-warm", action="store_true", help="skip the AOT fill arm")
    ap.add_argument("--with-cold-cache", action="store_true",
                    help="also measure cache-on-but-empty (isolates AOT from lookup overhead)")
    ap.add_argument("--keep", action="store_true", help="keep the scratch directory")
    ap.add_argument("--tests-only", action="store_true",
                    help="validate binaries/config and exit; no guest needed")
    args = ap.parse_args()

    # Boots run with cwd set to a per-run scratch dir, so every path handed to
    # a subprocess (and to `jitv2_aot`) must be absolute first.
    for field_name in ("iris", "iris_ci", "jitv2_aot", "config", "corpus",
                       "json", "work_dir"):
        v = getattr(args, field_name)
        if v is not None:
            setattr(args, field_name, v.resolve())

    for name, p in [("--iris", args.iris), ("--iris-ci", args.iris_ci),
                    ("--jitv2-aot", args.jitv2_aot), ("--config", args.config)]:
        if p is None or not Path(p).exists():
            print(f"aotbench: {name} not found: {p}", file=sys.stderr)
            return 2
    if not args.corpus.exists():
        print(f"aotbench: --corpus not found: {args.corpus}", file=sys.stderr)
        return 2

    # A config that turns the persistent cache on itself would override the
    # env the arms depend on (config wins over env for a nonempty value). Fail
    # loud rather than silently measure one arm twice.
    cfg_text = args.config.read_text(errors="replace")
    if re.search(r"^\s*cache\s*=\s*true", cfg_text, re.M):
        print("aotbench: config sets `[jitv2] cache = true`; remove it — the arms "
              "control the cache by environment", file=sys.stderr)
        return 2
    if re.search(r"^\s*cache_dir\s*=\s*\"[^\"]+\"", cfg_text, re.M):
        print("aotbench: config sets a nonempty `[jitv2] cache_dir`; remove it — "
              "the harness owns the cache directory", file=sys.stderr)
        return 2
    if args.tests_only:
        print("aotbench: binaries and config present; no guest run performed "
              "(that is the part needing IRIX).")
        return 0

    work = args.work_dir or Path(tempfile.mkdtemp(prefix="iris-aotbench."))
    work.mkdir(parents=True, exist_ok=True)
    print(f"aotbench: work = {work}")

    try:
        if args.capture_corpus:
            print(f"aotbench: capturing corpus to {args.corpus} ...")
            capture_corpus(iris=args.iris, iris_ci=args.iris_ci, config=args.config,
                           cpu=args.cpu, work=work, corpus=args.corpus,
                           marker=args.marker, timeout_s=args.timeout,
                           monitor_port=args.monitor_port)

        fills: dict = {}
        results: list[RunResult] = []

        # Arm 1: persistent cache off — the "without the cache" baseline.
        print("aotbench: arm nocache (persistent cache off) ...")
        for rep in range(args.reps):
            r = boot_once(iris=args.iris, iris_ci=args.iris_ci, config=args.config, cpu=args.cpu,
                          cache_enabled=False, cache_dir=work / "cache-off", work=work,
                          marker=args.marker, timeout_s=args.timeout, rep=rep,
                          arm="nocache", poll_s=args.poll)
            results.append(r)
            print(f"  rep {rep}: first-frame={fmt_ms(r.first_frame_ms)}ms login={fmt_ms(r.login_ms)}ms")

        if args.with_cold_cache:
            print("aotbench: arm coldcache (cache on, empty) ...")
            for rep in range(args.reps):
                # Fresh empty namespace each rep, or rep 2 would inherit rep 1's
                # writes and stop being "cold".
                shutil.rmtree(work / "cache-empty", ignore_errors=True)
                r = boot_once(iris=args.iris, iris_ci=args.iris_ci, config=args.config, cpu=args.cpu,
                              cache_enabled=True, cache_dir=work / "cache-empty", work=work,
                              marker=args.marker, timeout_s=args.timeout, rep=rep,
                              arm="coldcache", poll_s=args.poll)
                results.append(r)
                print(f"  rep {rep}: first-frame={fmt_ms(r.first_frame_ms)}ms login={fmt_ms(r.login_ms)}ms")

        if not args.no_warm:
            print(f"aotbench: filling cache offline from {args.corpus} ...")
            fills = fill_cache(args.jitv2_aot, args.corpus, args.iris,
                               work / "cache-aot", args.cpu)
            print(f"  {fills['raw'].strip()}")
            print("aotbench: arm warmaot (cache prewarmed) ...")
            for rep in range(args.reps):
                r = boot_once(iris=args.iris, iris_ci=args.iris_ci, config=args.config, cpu=args.cpu,
                              cache_enabled=True, cache_dir=work / "cache-aot", work=work,
                              marker=args.marker, timeout_s=args.timeout, rep=rep,
                              arm="warmaot", poll_s=args.poll)
                results.append(r)
                print(f"  rep {rep}: first-frame={fmt_ms(r.first_frame_ms)}ms login={fmt_ms(r.login_ms)}ms")

        return report(results, fills, args.json)
    finally:
        if args.keep:
            print(f"aotbench: kept {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
