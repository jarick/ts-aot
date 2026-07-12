#!/usr/bin/env python3
"""Test262 harness for ts-aot — multi-emit sampler.

For each test in tests/harness-out, runs `ts-aot compile --emit {rust,hir,mir}`
and records pass/fail. Defaults to a 500-file sample for fast feedback; set
MAX_TESTS = None in __main__ for the full 53k set.
"""
import argparse
import json
import re
import subprocess
import sys
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).parent
HARNESS_DIR = ROOT / "harness-out"
TS_AOT = ROOT.parent / "target" / "release" / "ts-aot.exe"

DIAG_RE = re.compile(r"\b([EPS]\d{4}):")


def compile_one(ts_path: Path, emit: str, timeout_s: int = 15):
    try:
        r = subprocess.run(
            [str(TS_AOT), "compile", str(ts_path), "--emit", emit],
            capture_output=True,
            text=True,
            timeout=timeout_s,
        )
        return r.returncode, r.stderr or ""
    except subprocess.TimeoutExpired:
        return -1, "TIMEOUT"
    except Exception as e:
        return -2, f"EXCEPTION: {e}"


def primary_code(stderr: str) -> str:
    codes = DIAG_RE.findall(stderr)
    return codes[0] if codes else "UNKNOWN"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max", type=int, default=500, help="Max tests to run (None for all)")
    ap.add_argument("--emit", action="append", default=None,
                    help="Emit stages to test (default: rust,hir,mir)")
    ap.add_argument("--workers", type=int, default=8)
    args = ap.parse_args()

    emits = args.emit or ["rust", "hir", "mir"]

    if not TS_AOT.exists():
        print(f"ERROR: ts-aot not found at {TS_AOT}; build it first", file=sys.stderr)
        sys.exit(1)
    if not HARNESS_DIR.exists():
        print(f"ERROR: harness dir not found at {HARNESS_DIR}", file=sys.stderr)
        sys.exit(1)

    test_files = sorted(p for p in HARNESS_DIR.glob("*.ts") if p.name != "runtime.zig")
    if args.max:
        test_files = test_files[: args.max]
    total = len(test_files)
    print(f"Running {total} test files × {len(emits)} emits = {total * len(emits)} compilations",
          file=sys.stderr)

    by_emit = {e: {"pass": 0, "fail": 0, "other": 0,
                   "primary": Counter(), "all_codes": Counter(),
                   "sample": []} for e in emits}
    t0 = time.time()

    def job(ts, emit):
        return ts, emit, compile_one(ts, emit)

    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = [ex.submit(job, ts, e) for ts in test_files for e in emits]
        done = 0
        total_jobs = len(futs)
        for f in as_completed(futs):
            ts, emit, (code, stderr) = f.result()
            slot = by_emit[emit]
            if code == 0:
                slot["pass"] += 1
            elif code in (-1, -2):
                slot["other"] += 1
            else:
                slot["fail"] += 1
                codes = DIAG_RE.findall(stderr)
                primary = codes[0] if codes else "UNKNOWN"
                slot["primary"][primary] += 1
                for c in codes:
                    slot["all_codes"][c] += 1
                if len(slot["sample"]) < 8:
                    err_lines = [ln.strip() for ln in stderr.splitlines() if ln.strip()]
                    slot["sample"].append({
                        "file": ts.name,
                        "primary": primary,
                        "first_err": err_lines[0] if err_lines else "",
                    })
            done += 1
            if done % 500 == 0 or done == total_jobs:
                rate = done / max(time.time() - t0, 0.001)
                print(f"  done {done}/{total_jobs} ({rate:.1f}/s)",
                      file=sys.stderr)

    elapsed = time.time() - t0
    print()
    print(f"=== ts-aot test262 harness — {total} files, emits={emits}, {elapsed:.1f}s ===")
    print()
    for emit in emits:
        s = by_emit[emit]
        run = s["pass"] + s["fail"] + s["other"]
        rate = 100.0 * s["pass"] / max(run, 1)
        print(f"[{emit}] pass={s['pass']}/{run}  fail={s['fail']}  other={s['other']}  ({rate:.2f}%)")
        if s["fail"]:
            print(f"  Primary errors:")
            for c, n in s["primary"].most_common(5):
                print(f"    {c}: {n}")
            print(f"  All diag codes:")
            for c, n in s["all_codes"].most_common(5):
                print(f"    {c}: {n}")
            print(f"  Sample errors:")
            for s2 in s["sample"][:5]:
                print(f"    [{s2['primary']}] {s2['file']}")
                print(f"        {s2['first_err']}")
        print()

    report_path = ROOT / "test262-report.json"
    report_path.write_text(json.dumps({
        "total": total,
        "emits": emits,
        "elapsed_sec": round(elapsed, 1),
        "by_emit": {e: {
            "pass": by_emit[e]["pass"],
            "fail": by_emit[e]["fail"],
            "other": by_emit[e]["other"],
            "pass_rate": round(100.0 * by_emit[e]["pass"] / max(by_emit[e]["pass"] + by_emit[e]["fail"] + by_emit[e]["other"], 1), 2),
            "primary_codes": dict(by_emit[e]["primary"]),
            "all_codes": dict(by_emit[e]["all_codes"]),
            "sample": by_emit[e]["sample"],
        } for e in emits},
    }, indent=2))
    print(f"Report written to {report_path}")


if __name__ == "__main__":
    main()
