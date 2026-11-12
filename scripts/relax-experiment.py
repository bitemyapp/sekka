#!/usr/bin/env python3
"""Run isolated rustc/Relax component comparisons; never installs or launches a desktop."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


COMPONENTS = [
    ("apps/hotaru", "hotaru-sprites", []),
    ("apps/hotaru", "hotaru-core", ["--lib"]),
    ("installer/genkan/rust/system/session-env", "calamares-session-env", []),
    ("desktop/aizome", "aizome-ipc", ["--lib"]),
    ("desktop/aizome", "aizome-theme", ["--lib"]),
    ("desktop/aizome", "aizome-config", ["--lib"]),
    ("desktop/aizome", "aizome-shell-proto", ["--lib"]),
    ("rust", "respin-tools", ["--lib"]),
    ("desktop/aizome", "hotaru-link-regex", []),
    ("apps/hotaru", "hotaru-io", ["--lib"]),
    ("desktop/aizome", "aizome-kit", ["--lib"]),
    ("desktop/aizome", "aizome-soroban", ["--lib"]),
]
CHECK_COMPONENTS = COMPONENTS + [("desktop/aizome", "aizome-comp", ["--bin", "aizome"])]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["components", "regressions", "check"])
    parser.add_argument("--relax-root", type=Path, required=True)
    parser.add_argument("--fixtures-root", type=Path)
    parser.add_argument("--sekka-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--engine", choices=["both", "rustc", "relax"], default="both")
    parser.add_argument("--package", action="append", help="Select a component; may be repeated")
    parser.add_argument("--case", action="append", help="Select a reduced regression; may be repeated")
    parser.add_argument("--timeout", type=int, default=900)
    parser.add_argument("--repeat", type=int, default=1)
    args = parser.parse_args()
    for name in ("relax_root", "sekka_root", "output"):
        setattr(args, name, getattr(args, name).resolve())
    if args.repeat < 1 or args.timeout < 1:
        parser.error("repeat and timeout must be positive")
    selected_components = CHECK_COMPONENTS if args.mode == "check" else COMPONENTS
    if args.package and set(args.package) - {p for _, p, _ in selected_components}:
        parser.error("unknown component")
    if args.output.exists():
        parser.error("output must be a new directory; interrupted runs are retained")
    args.output.mkdir(parents=True)
    result_path = args.output / "results.json"
    runner = args.relax_root / "target/release/relax"
    fixtures = (args.fixtures_root or args.relax_root).resolve() / "libs/relax/compiler/tests"
    env = os.environ.copy()
    env["MAKEPAD_RUN_STD"] = str(args.relax_root / "libs/relax/compiler/std")
    env.setdefault("MAKEPAD_CPU_CORES", "8")
    env.setdefault("CARGO_BUILD_JOBS", "8")
    results = {"mode": args.mode, "status": "running", "runs": [], "inputs": {}}
    for name, root in (("sekka", args.sekka_root), ("relax", args.relax_root)):
        results["inputs"][name] = {
            "commit": subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip(),
            "status": subprocess.check_output(["git", "-C", str(root), "status", "--porcelain"], text=True),
        }
    results["inputs"]["rustc"] = subprocess.check_output(["rustc", "-Vv"], text=True)
    if args.engine != "rustc" and not runner.is_file():
        parser.error(f"build the Relax runner first: {runner}")
    results["inputs"]["runner_sha256"] = hashlib.sha256(runner.read_bytes()).hexdigest() if runner.is_file() else None
    results["inputs"]["cpu_budget"] = env["MAKEPAD_CPU_CORES"]
    def lock_hashes():
        return {str(p.relative_to(args.sekka_root)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in sorted({args.sekka_root / d / "Cargo.lock" for d, _, _ in COMPONENTS}) if p.exists()}
    results["inputs"]["lockfiles"] = lock_hashes()
    if args.mode == "regressions":
        files = [fixtures / (n + ".rs") for n in (
            "sse2_safe_byte_intrinsics", "macro_expansion_textual_scope", "exported_macro_and_value_reexport",
            "sse2_load_requires_unsafe_reject", "exported_macro_builtin_reexport_reject", "exported_macro_absolute_import_reject")]
        files += [p for p in (fixtures / "attribute_macro_private_module").rglob("*")
                  if p.is_file() and (p.suffix == ".rs" or p.name == "Cargo.toml") and "target" not in p.parts]
        results["inputs"]["fixtures"] = {str(p.relative_to(fixtures)): hashlib.sha256(p.read_bytes()).hexdigest()
                                             for p in files if p.is_file()}
    result_path.write_text(json.dumps(results, indent=2) + "\n")
    # Python must unwind the owned subprocess group on termination as on Ctrl-C.
    signal.signal(signal.SIGTERM, lambda number, frame: sys.exit(128 + number))

    def run(label, engine, cwd, command, expected=None, cache_key=None, expected_tests=None, require_tests=False):
        run_env = env.copy()
        # Keep native Cargo, Relax's native bridge and compiler revisions separate.
        run_env["CARGO_TARGET_DIR"] = str(args.output / "targets" / engine)
        run_env["MAKEPAD_RUN_CACHE_DIR"] = str(args.output / "cache" / (cache_key or label))
        log = args.output / (label + ".log")
        start = time.monotonic()
        timed_out = False
        startup_error = None
        code = None
        def kill_group(proc, sig):
            try:
                os.killpg(proc.pid, sig)
            except ProcessLookupError:
                pass
        with log.open("w") as stream:
            try:
                proc = subprocess.Popen(command, cwd=cwd, env=run_env, stdout=stream,
                                        stderr=subprocess.STDOUT, start_new_session=True)
            except OSError as error:
                startup_error = str(error)
                stream.write(startup_error + "\n")
            else:
                try:
                    try:
                        code = proc.wait(timeout=args.timeout)
                    except subprocess.TimeoutExpired:
                        timed_out = True
                        kill_group(proc, signal.SIGTERM)
                        try:
                            code = proc.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            pass
                finally:
                    # A parent can exit while a compiler/build-script descendant
                    # remains. Always clean the entire session owned by this run.
                    kill_group(proc, signal.SIGKILL)
                    reaped = proc.wait()
                    if code is None:
                        code = reaped
        content = log.read_text(errors="replace")
        summaries = re.findall(r"test result:.*", content)
        count = sum(int(n) for summary in summaries for n in re.findall(r"(\d+) passed", summary))
        passed = not timed_out and startup_error is None and (code == 0 if expected is None else code in (1, 101) and expected in content)
        if expected_tests is not None:
            passed &= count == expected_tests
        if require_tests:
            passed &= count > 0
        locks_unchanged = lock_hashes() == results["inputs"]["lockfiles"]
        passed &= locks_unchanged
        record = dict(label=label, engine=engine, command=list(map(str, command)),
                      exit_code=code, seconds=round(time.monotonic() - start, 3),
                      timed_out=timed_out, passed=passed, expected_diagnostic=expected,
                      startup_error=startup_error, lockfiles_unchanged=locks_unchanged,
                      tests_passed=count, expected_tests=expected_tests,
                      test_summaries=summaries, log=log.name)
        results["runs"].append(record)
        result_path.write_text(json.dumps(results, indent=2) + "\n")
        print(f"{label}: {'PASS' if passed else 'FAIL'} ({record['seconds']} s)", flush=True)
        return passed

    engines = ["rustc", "relax"] if args.engine == "both" else [args.engine]
    if args.mode == "regressions":
        positive = ["sse2_safe_byte_intrinsics", "macro_expansion_textual_scope", "exported_macro_and_value_reexport"]
        negative = {"sse2_load_requires_unsafe_reject": "E0133",
                    "exported_macro_builtin_reexport_reject": "E0255",
                    "exported_macro_absolute_import_reject": "E0255"}
        if args.case and set(args.case) - set(positive + list(negative) + ["attribute_macro_private_module"]):
            parser.error("unknown regression case")
        for case in positive + list(negative):
            if args.case and case not in args.case:
                continue
            source = fixtures / (case + ".rs")
            for engine in engines:
                label = f"{case}-{engine}"
                if engine == "rustc":
                    binary = args.output / label
                    if run(label + "-compile", engine, fixtures,
                           ["rustc", "--edition=2021", "-O", "--test", str(source), "-o", str(binary)], negative.get(case)) and case not in negative:
                        run(label, engine, fixtures, [str(binary)], expected_tests=2 if case == "macro_expansion_textual_scope" else 1)
                else:
                    run(label, engine, fixtures, [str(runner), "test", str(source)], negative.get(case),
                        expected_tests=None if case in negative else 2 if case == "macro_expansion_textual_scope" else 1)
        for engine in engines:
            if args.case and "attribute_macro_private_module" not in args.case:
                continue
            command = ["cargo", "test", "--offline", "--release", "-p", "apm-app", "--lib"] if engine == "rustc" else [str(runner), "test", "--no-live", "apm-app", "--lib"]
            run("attribute-macro-private-module-" + engine, engine,
                fixtures / "attribute_macro_private_module", command, expected_tests=2)
    else:
        for directory, package, targets in selected_components:
            if args.package and package not in args.package:
                continue
            for engine in engines:
                verb = "check" if args.mode == "check" else "test"
                # Cargo's default also runs doctests; Relax does not. Compare
                # unit/integration targets explicitly and report that boundary.
                cargo_targets = targets or (["--tests"] if verb == "test" else [])
                command = ["cargo", verb, "--locked", "--release", "-p", package, *cargo_targets] if engine == "rustc" else [str(runner), verb, "--no-serve" if verb == "check" else "--no-live", "--allow-c", package, *targets]
                for iteration in range(args.repeat):
                    label = f"{package}-{engine}-{iteration + 1}"
                    native = next((r for r in results["runs"] if r["label"] == f"{package}-rustc-{iteration + 1}"), None)
                    run(label, engine, args.sekka_root / directory, command,
                        cache_key=f"{package}-{engine}",
                        expected_tests=native["tests_passed"] if native and engine == "relax" and verb == "test" else None,
                        require_tests=verb == "test")
    results["status"] = "complete"
    result_path.write_text(json.dumps(results, indent=2) + "\n")
    return int(any(not r["passed"] for r in results["runs"]))


if __name__ == "__main__":
    raise SystemExit(main())
