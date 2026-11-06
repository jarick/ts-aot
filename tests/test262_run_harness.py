#!/usr/bin/env python3
"""Honest test262 harness for ts-aot.

Unlike test262_harness.py (which only checks that the compiler emits MIR and
therefore "passes" features the Rust backend cannot emit at all), this harness
runs each flattened test end to end:

  1. rewrite the no-op `assert*` shim stubs into real assertions that throw,
  2. `ts-aot compile --emit crate` the test into a Rust crate,
  3. add a `main` that calls the test's `__test262_run` entry,
  4. `cargo run` the crate.

A test counts as *pass* only if the whole chain succeeds and the test exits 0.
Any of: compile failure (e.g. E0300 for unimplemented features), cargo build
failure (generated Rust does not compile), or a non-zero exit (a real assertion
fired, or the runtime panicked) counts as *fail*.

Requires the flattened tests in tests/harness-out (see test262_harness.py).
This is slow (a cargo build per test); use --max for a sample.
"""
import argparse
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).parent
HARNESS_DIR = ROOT / "harness-out"
TS_AOT = ROOT.parent / "target" / "release" / f"ts-aot{'.exe' if os.name == 'nt' else ''}"
RUNTIME = ROOT.parent / "crates" / "ts-aot-runtime"

# All Cargo invocations share one target directory, so they must be serialized:
# otherwise a later Cargo waits on the shared-target lock and that wait would be
# charged against its test timeout. Holding this lock around the timed subprocess
# keeps the timeout measuring only the actual build + test run.
CARGO_LOCK = threading.Lock()

# The flattened tests ship a no-op shim; replace it with real assertions so the
# test can actually fail (the runtime throws -> the process panics -> non-zero).
STUB_ASSERTS = {
    "function assert(mustBeTrue: i32, message: string): i32 { return 0; }":
        "function assert(mustBeTrue: i32, message: string): i32 { "
        "if (mustBeTrue == 0) { Test262Error_throw(message); } return 0; }",
    "function assertNotSameValue(actual: i32, unexpected: i32, message: string): i32 { return 0; }":
        "function assertNotSameValue(actual: i32, unexpected: i32, message: string): i32 { "
        "if (actual == unexpected) { Test262Error_throw(message); } return 0; }",
    "function assertSameValue(actual: i32, expected: i32, message: string): i32 { return 0; }":
        "function assertSameValue(actual: i32, expected: i32, message: string): i32 { "
        "if (actual != expected) { Test262Error_throw(message); } return 0; }",
    "function Test262Error_throw(message: string): i32 { return 0; }":
        "function Test262Error_throw(message: string): i32 { throw message; }",
}


# A leftover no-op assertion shim means the flattened test's shim did not match
# STUB_ASSERTS: its asserts stay no-ops, so the test could exit 0 without checking
# anything. Such tests are reported as *stub* (a failure), never as pass.
NOOP_ASSERT = re.compile(
    r"function\s+(?:assert\w*|Test262Error_throw)\s*\([^)]*\)\s*:\s*\w+\s*"
    r"\{\s*return\s+0\s*;\s*\}"
)


def rewrite_asserts(text: str) -> str:
    for stub, real in STUB_ASSERTS.items():
        text = text.replace(stub, real)
    return text


def kill_tree(proc: subprocess.Popen) -> None:
    """Terminate a Cargo invocation and every descendant (rustc, the test binary)
    so none of them survives to keep the captured pipes open."""
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
            capture_output=True,
            check=False,
        )
    else:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            proc.kill()


def run_cargo_tree(cmd, env, timeout_s):
    """Run Cargo in its own process group; on timeout kill the whole tree
    (Cargo + rustc + the test child) before closing the pipes and re-raising, so a
    stuck descendant cannot keep the pipes open or hold CARGO_LOCK forever."""
    if os.name == "nt":
        group = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    else:
        group = {"start_new_session": True}
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=env,
        **group,
    )
    try:
        _, err = proc.communicate(timeout=timeout_s)
        return proc.returncode, err
    except subprocess.TimeoutExpired:
        kill_tree(proc)
        proc.communicate()
        raise


def run_one(ts_path: Path, work_root: Path, target_dir: Path, timeout_s: int):
    name = ts_path.stem
    crate_dir = work_root / name
    try:
        source = rewrite_asserts(ts_path.read_text(encoding="utf-8", errors="replace"))
        if NOOP_ASSERT.search(source):
            return name, "stub", []
        tmp_ts = work_root / f"{name}.ts"
        tmp_ts.write_text(source, encoding="utf-8")

        r = subprocess.run(
            [str(TS_AOT), "compile", str(tmp_ts), "--module", "--emit", "crate",
             "-o", str(crate_dir), "--crate-name", "t", "--runtime-path", str(RUNTIME)],
            capture_output=True, text=True, encoding="utf-8", errors="replace",
            timeout=timeout_s,
        )
        if r.returncode != 0:
            errs = [ln.strip() for ln in (r.stderr or "").splitlines() if ": error:" in ln]
            return name, "compile", errs[:1] or (r.stderr or "").strip().splitlines()[:1]

        (crate_dir / "src" / "main.rs").write_text(
            "fn main() { std::process::exit(t::__test262_run()); }\n", encoding="utf-8"
        )
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir))
        with CARGO_LOCK:
            code, err = run_cargo_tree(
                ["cargo", "run", "--quiet", "--manifest-path", str(crate_dir / "Cargo.toml")],
                env,
                timeout_s * 6,
            )
        if code == 0:
            return name, "pass", []
        err = err or ""
        stage = "build" if "error[E" in err or "could not compile" in err else "run"
        return name, stage, err.strip().splitlines()[:1]
    except subprocess.TimeoutExpired:
        return name, "timeout", []
    except Exception as e:  # noqa: BLE001
        return name, "error", [str(e)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max", type=int, default=200)
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument(
        "--keep",
        action="store_true",
        help="keep the work directory (rewritten tests, crates, Cargo artifacts)",
    )
    args = ap.parse_args()

    if not TS_AOT.exists():
        print(f"ERROR: ts-aot not found at {TS_AOT}; build it first", file=sys.stderr)
        sys.exit(1)
    if not HARNESS_DIR.exists():
        print(f"ERROR: {HARNESS_DIR} missing", file=sys.stderr)
        sys.exit(1)

    files = sorted(p for p in HARNESS_DIR.glob("*.ts") if p.name != "runtime.zig")
    if args.max and args.max < len(files):
        step = len(files) / args.max
        files = [files[int(i * step)] for i in range(args.max)]

    work_root = Path(tempfile.mkdtemp(prefix="ts262run-"))
    target_dir = work_root / "_target"
    print(
        f"Running {len(files)} tests (compile -> cargo build -> run) in {work_root}",
        file=sys.stderr,
    )

    failed = 0
    try:
        counts = {
            "pass": 0,
            "compile": 0,
            "stub": 0,
            "build": 0,
            "run": 0,
            "timeout": 0,
            "error": 0,
        }
        samples = {}
        t0 = time.time()
        with ThreadPoolExecutor(max_workers=args.workers) as ex:
            futs = [ex.submit(run_one, f, work_root, target_dir, 20) for f in files]
            done = 0
            for fut in as_completed(futs):
                name, status, detail = fut.result()
                counts[status] = counts.get(status, 0) + 1
                if status != "pass" and len(samples.setdefault(status, [])) < 8:
                    samples[status].append((name, detail))
                done += 1
                if done % 25 == 0 or done == len(files):
                    rate = done / max(time.time() - t0, 0.001)
                    print(f"  {done}/{len(files)} ({rate:.1f}/s)", file=sys.stderr)

        failed = len(files) - counts["pass"]
        print(f"pass={counts['pass']}  fail={failed}  (of {len(files)})")
        for k in ("compile", "stub", "build", "run", "timeout", "error"):
            if counts.get(k):
                print(f"  {k}: {counts[k]}")
                for name, detail in samples.get(k, []):
                    suffix = f" -- {detail[0]}" if detail else ""
                    print(f"    [{k}] {name}{suffix}")
    finally:
        if args.keep:
            print(f"kept work directory: {work_root}", file=sys.stderr)
        else:
            shutil.rmtree(work_root, ignore_errors=True)

    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
