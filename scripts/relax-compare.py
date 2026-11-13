#!/usr/bin/env python3
"""Interleave warmed theme tests; this is not a full-workspace build benchmark."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import signal
import sys
import threading
import time
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("sekka", "relax", "buck2", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--buck2-bin", type=Path, required=True, help="The pinned engine used by the Buck2 client")
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args()
    if args.rounds < 1 or args.timeout < 1:
        parser.error("rounds and timeout must be positive")
    roots = {name: getattr(args, name).resolve() for name in ("sekka", "relax", "buck2", "output")}
    out = roots["output"]
    if out.exists():
        parser.error("output must be a new directory")
    # The existing Buck2 experiment is at a different repository revision. Only
    # compare this component when its manifest and all Rust sources are identical.
    relative = Path("desktop/aizome/crates/aizome-theme")
    def sources(root):
        directory = root / relative
        return {str(p.relative_to(directory)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in [directory / "Cargo.toml", *sorted((directory / "src").rglob("*.rs"))]}
    hashes = sources(roots["sekka"])
    if hashes != sources(roots["buck2"]):
        parser.error("theme sources differ; choose comparable revisions")
    def dependency_inputs(root):
        workspace = tomllib.loads((root / "desktop/aizome/Cargo.toml").read_text())["workspace"]
        packages = tomllib.loads((root / "desktop/aizome/Cargo.lock").read_text())["package"]
        names, pending = set(), ["aizome-theme"]
        while pending:
            name = pending.pop()
            if name in names:
                continue
            names.add(name)
            for package in packages:
                if package["name"] == name:
                    pending.extend(dep.split()[0] for dep in package.get("dependencies", []))
        return {"workspace_package": workspace["package"],
                "serde": workspace["dependencies"]["serde"],
                "locked_packages": [p for p in packages if p["name"] in names]}
    dependencies = dependency_inputs(roots["sekka"])
    if dependencies != dependency_inputs(roots["buck2"]):
        parser.error("theme workspace metadata or locked dependency closure differs")
    out.mkdir(parents=True)
    isolation = "relax-compare-" + hashlib.sha256(str(out).encode()).hexdigest()[:12]
    env = dict(os.environ, CARGO_BUILD_JOBS="8", MAKEPAD_CPU_CORES="8",
               MAKEPAD_RUN_STD=str(roots["relax"] / "libs/relax/compiler/std"),
               MAKEPAD_RUN_CACHE_DIR=str(out / "relax-cache"),
               CARGO_TARGET_DIR=str(out / "cargo-target"),
               SEKKA_BUCK2_CLIENT_CACHE=str(out / "buck2-bootstrap"))
    commands = {
        "cargo-release": ["cargo", "test", "--locked", "--release", "-p", "aizome-theme", "--lib"],
        "relax": [str(roots["relax"] / "target/release/relax"), "test", "--no-live", "aizome-theme", "--lib"],
        "buck2-dev": [str(roots["buck2"] / "scripts/sekka-buck2"), "--isolation-dir", isolation,
                      "--cache", "local", "--local-content-store", str(out / "buck2-content-store"), "-j", "8",
                      "test", "//desktop/aizome/crates/aizome-theme:aizome_theme-test-test-lib"],
    }
    data = {"status": "running", "source_hashes": hashes, "dependency_inputs": dependencies,
            "commands": commands, "revisions": {}, "runs": [],
            "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
            "runner_sha256": hashlib.sha256((roots["relax"] / "target/release/relax").read_bytes()).hexdigest(),
            "buck2_sha256": hashlib.sha256(args.buck2_bin.read_bytes()).hexdigest()}
    def save():
        (out / "results.json").write_text(json.dumps(data, indent=2) + "\n")
    signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))
    for name in ("sekka", "relax", "buck2"):
        data["revisions"][name] = subprocess.check_output(["git", "-C", str(roots[name]), "rev-parse", "HEAD"], text=True).strip()
    save()
    try:
        for round_number in range(args.rounds + 1):
            for name, command in commands.items():
                cwd = roots["buck2"] if name == "buck2-dev" else roots["sekka"] / "desktop/aizome"
                start = time.monotonic()
                code, error = None, None
                with (out / f"{name}-{round_number}.log").open("w") as log:
                    try:
                        proc = subprocess.Popen(command, cwd=cwd, env=env, stdout=log,
                                                stderr=subprocess.STDOUT, start_new_session=True)
                    except OSError as exc:
                        error = str(exc)
                    else:
                        # Popen.wait(timeout=...) polls on POSIX, adding up to
                        # 50 ms to short runs. A watchdog permits blocking wait
                        # and measures process completion without that bias.
                        expired = threading.Event()
                        def expire():
                            expired.set()
                            try:
                                os.killpg(proc.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                        watchdog = threading.Timer(args.timeout, expire)
                        watchdog.daemon = True
                        watchdog.start()
                        try:
                            code = proc.wait()
                            elapsed = time.monotonic() - start
                        finally:
                            watchdog.cancel()
                            watchdog.join()
                            try:
                                os.killpg(proc.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                            proc.wait()
                        if expired.is_set():
                            error = "command timed out"
                    if code is None:
                        elapsed = time.monotonic() - start
                content = (out / f"{name}-{round_number}.log").read_text(errors="replace")
                count = sum(int(n) for n in re.findall(r"test result:.*?(\d+) passed", content))
                inputs_unchanged = (sources(roots["sekka"]) == hashes == sources(roots["buck2"])
                                    and dependency_inputs(roots["sekka"]) == dependencies == dependency_inputs(roots["buck2"]))
                record = dict(engine=name, round=round_number, prime=round_number == 0,
                              seconds=round(elapsed, 6), exit_code=code,
                              error=error, tests_passed=count, inputs_unchanged=inputs_unchanged,
                              passed=code == 0 and error is None and count == 2 and inputs_unchanged)
                data["runs"].append(record)
                save()
                print(record, flush=True)
                if not record["passed"]:
                    data["status"] = "failed"
                    save()
                    return 1
        data["summary"] = {}
        for name in commands:
            values = [r["seconds"] for r in data["runs"] if r["engine"] == name and not r["prime"]]
            data["summary"][name] = dict(median=statistics.median(values), minimum=min(values), maximum=max(values))
        data["status"] = "complete"
        save()
        return 0
    finally:
        # This checkout and isolation belong to this probe. Do not leave its
        # daemon running or touch another experiment's daemon/cache service.
        subprocess.run([str(args.buck2_bin.resolve()), "--isolation-dir", isolation, "kill"],
                       cwd=roots["buck2"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False, timeout=30)


if __name__ == "__main__":
    raise SystemExit(main())
