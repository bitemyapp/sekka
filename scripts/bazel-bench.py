#!/usr/bin/env python3
"""Time Aizome's builds with Cargo and with Bazel, scenario by scenario.

The build plan's harness (docs/bazel-build-plan.md): it checks out one commit
into worktrees it owns, runs each scenario's builds, and writes one JSON
record per trial, failures included, with the argv, timing, Bazel's own
metrics (from the Build Event Protocol), the trial's correctness check and
the checksums of its logs. Every deliberate edit has a check of its own:

- cold: a fresh worktree, target directory or output base;
- noop: the same build again;
- leaf: a new static in Soroban's binary, which must appear in it;
- shared: a new static in aizome-theme, under nearly every crate, which must
  appear in its rlib;
- build-script: a new Lucide icon for aizome-kit's build script, which must
  appear in the code it generates;
- revert: the edits undone, which must give the cold build's binary.

    scripts/bazel-bench.py --run-id ID --repo ~/sekka-work/bazel-lab/repo \\
        --commit SHA --root /dev/shm/sekka-bazel-lab/ID \\
        --results ~/sekka-work/bazel-lab/results/ID --tool bazel \\
        --bazel-env BAZEL_ENV.sh --cargo-env AIZOME_ENV.sh

Run it outside any Nix shell: each tool's environment comes from the
sourceable files given (`nix print-dev-env` of `.#bazel` and `.#aizome` at
the commit), so every trial sees the same one. It refuses a root it didn't
create and removes only that root's contents when asked (--clean).

    scripts/bazel-bench.py summarize RESULTS... > summary.md

summarizes trials: per tool and scenario, the median and range of wall
time, the executed actions and the checks that passed.
"""
import statistics
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import shlex
import shutil
import socket
import subprocess
import sys
import time


OWNER = ".sekka-bazel-bench"
SOROBAN = "desktop/aizome/apps/soroban/src/main.rs"
THEME = "desktop/aizome/crates/aizome-theme/src/lib.rs"
ICONS = "desktop/aizome/crates/aizome-kit/icons/lucide"
SCENARIOS = ["cold", "noop", "leaf", "shared", "build-script", "revert"]
WORKSPACE_TARGETS = "kind('rust_binary|rust_library|rust_dylib_library', //desktop/aizome/... + //apps/hotaru/... + //apps/yukimi/...)"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def loadavg():
    return [float(x) for x in Path("/proc/loadavg").read_text().split()[:3]]


def owned(root, run_id, create=False):
    marker = root / OWNER
    if marker.is_file():
        if marker.read_text().strip() != run_id:
            sys.exit(f"{root} belongs to run {marker.read_text().strip()}, not {run_id}")
        return
    if root.exists() and any(root.iterdir()):
        sys.exit(f"refusing {root}: it exists and this harness didn't create it")
    if not create:
        sys.exit(f"{root} isn't this run's")
    root.mkdir(parents=True, exist_ok=True)
    marker.write_text(run_id + "\n")


def shell_env(path):
    """The environment a sourceable file (nix print-dev-env) gives bash."""
    out = subprocess.run(
        ["bash", "-c", f'set -a; . {shlex.quote(str(path))} >/dev/null 2>&1; set +a; env -0'],
        check=True, capture_output=True, env={"HOME": os.environ["HOME"], "PATH": "/usr/bin:/bin"},
    ).stdout
    env = dict(item.split("=", 1) for item in out.decode().split("\0") if "=" in item)
    for name in ["SHLVL", "_", "PWD", "OLDPWD"]:
        env.pop(name, None)
    return env


class Bench:
    def __init__(self, args):
        self.args = args
        self.root = Path(args.root).resolve()
        self.results = Path(args.results).resolve()
        self.tree = self.root / "worktrees" / args.tool
        self.logs = self.results / "logs"
        self.logs.mkdir(parents=True, exist_ok=True)
        self.env = shell_env(args.bazel_env if args.tool == "bazel" else args.cargo_env)
        self.env["HOME"] = os.environ["HOME"]
        nix_bin = "/nix/var/nix/profiles/default/bin"
        self.env["PATH"] = self.env.get("PATH", "") + ":" + nix_bin
        self.reference = None

    # The worktree, at the commit, with nothing else in it.
    def checkout(self):
        if self.tree.exists():
            subprocess.run(["git", "-C", self.args.repo, "worktree", "remove", "--force", str(self.tree)], check=True)
        self.tree.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "-C", self.args.repo, "worktree", "add", "--detach", str(self.tree), self.args.commit],
                       check=True, capture_output=True)

    def reset(self):
        subprocess.run(["git", "-C", str(self.tree), "checkout", "-q", "--", "."], check=True)
        subprocess.run(["git", "-C", str(self.tree), "clean", "-qfd", "--", "desktop"], check=True)

    def patch(self):
        out = subprocess.run(["git", "-C", str(self.tree), "diff", "--binary"], check=True, capture_output=True).stdout
        new = subprocess.run(["git", "-C", str(self.tree), "ls-files", "--others", "--exclude-standard", "desktop"],
                             check=True, capture_output=True, text=True).stdout.split()
        h = hashlib.sha256(out)
        for f in sorted(new):
            h.update(f.encode())
            h.update((self.tree / f).read_bytes())
        return h.hexdigest()

    # Edits: each leaves a marker its check looks for.
    def edit(self, scenario, rep):
        marker = f"sekka-bench-{scenario}-{rep:04d}"
        if scenario == "leaf":
            path = self.tree / SOROBAN
            path.write_text(path.read_text() + f'\n#[used]\nstatic SEKKA_BENCH: [u8; {len(marker)}] = *b"{marker}";\n')
        elif scenario == "shared":
            path = self.tree / THEME
            path.write_text(path.read_text() + f'\n#[used]\nstatic SEKKA_BENCH: [u8; {len(marker)}] = *b"{marker}";\n')
        elif scenario == "build-script":
            (self.tree / ICONS / f"{marker}.svg").write_text(
                '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">'
                '<circle cx="12" cy="12" r="10" /></svg>\n')
        return marker

    # Artifacts.
    def soroban(self):
        if self.args.tool == "bazel":
            return self.tree / "bazel-bin/desktop/aizome/apps/soroban/aizome-soroban"
        return self.cargo_target() / "debug/aizome-soroban"

    def cargo_target(self):
        return self.root / "cargo-target"

    def theme_rlib(self):
        if self.args.tool == "bazel":
            found = sorted((self.tree / "bazel-bin/desktop/aizome/crates/aizome-theme").glob("libaizome_theme-*.rlib"))
        else:
            found = sorted((self.cargo_target() / "debug/deps").glob("libaizome_theme-*.rlib"), key=os.path.getmtime)
        return found[-1] if found else None

    def kit_icons(self):
        if self.args.tool == "bazel":
            path = self.tree / "bazel-bin/desktop/aizome/crates/aizome-kit/build_script.out_dir/icons.rs"
            return path if path.exists() else None
        found = sorted((self.cargo_target() / "debug/build").glob("aizome-kit-*/out/icons.rs"), key=os.path.getmtime)
        return found[-1] if found else None

    def check(self, scenario, marker):
        def contains(path, text):
            return path is not None and path.exists() and text.encode() in path.read_bytes()
        if scenario == "leaf":
            ok = contains(self.soroban(), marker)
            return {"check": f"{marker} in aizome-soroban", "passed": ok}
        if scenario == "shared":
            rlib = self.theme_rlib()
            return {"check": f"{marker} in {rlib.name if rlib else 'libaizome_theme-*.rlib'}", "passed": contains(rlib, marker)}
        if scenario == "build-script":
            return {"check": f'"{marker}" in aizome-kit\'s generated icons.rs', "passed": contains(self.kit_icons(), f'"{marker}"')}
        binary = self.soroban()
        digest = sha256(binary) if binary.exists() else None
        if scenario == "cold":
            self.reference = digest
            return {"check": "aizome-soroban built", "passed": digest is not None, "sha256": digest}
        if scenario == "revert":
            return {"check": "aizome-soroban identical to the cold build's", "passed": digest is not None and digest == self.reference,
                    "sha256": digest, "reference": self.reference}
        return {"check": "build succeeded", "passed": digest is not None, "sha256": digest}

    # One build.
    def argv(self, trial):
        a = self.args
        if a.tool == "bazel":
            bep = self.logs / f"{trial}.bep.json"
            return [
                "bazel", f"--output_user_root={self.root / 'bazel'}",
                "build", f"--config={a.profile}", "--config=wx-local", f"--jobs={a.jobs}",
                f"--repository_cache={self.root / 'repos'}",
                f"--build_event_json_file={bep}", f"--profile={self.logs / (trial + '.profile.gz')}",
                "--target_pattern_file=" + str(self.results / "targets.txt"),
            ]
        return [
            "cargo", "build", "--locked", "--offline", "--manifest-path", "desktop/aizome/Cargo.toml",
            "--workspace", "-j", str(a.jobs),
        ]

    def build(self, scenario, rep, marker=None):
        trial = f"{self.args.tool}-{scenario}-{rep:02d}"
        argv = self.argv(trial)
        env = dict(self.env)
        if self.args.tool == "cargo":
            env["CARGO_TARGET_DIR"] = str(self.cargo_target())
        log = self.logs / f"{trial}.log"
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        load_before = loadavg()
        start = time.monotonic()
        with open(log, "wb") as out:
            status = subprocess.run(argv, cwd=self.tree, env=env, stdout=out, stderr=subprocess.STDOUT,
                                    timeout=self.args.timeout).returncode
        wall = time.monotonic() - start
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        record = {
            "source": {"commit": self.args.commit, "patch_sha256": self.patch()},
            "host": {"hostname": socket.gethostname(), "nproc": os.cpu_count(),
                     "loadavg_before": load_before, "loadavg_after": loadavg()},
            "scenario": scenario, "rep": rep, "tool": self.args.tool, "profile": self.args.profile,
            "jobs": self.args.jobs, "toolchain": self.toolchain(),
            "argv": argv, "exit_status": status, "wall_seconds": round(wall, 3),
        }
        log_text = log.read_text(errors="replace")
        if self.args.tool == "cargo":
            record["action_cpu_seconds"] = round(after.ru_utime - before.ru_utime + after.ru_stime - before.ru_stime, 3)
            # getrusage keeps the largest child's since the harness started:
            # a bound for the run so far, not this trial's own peak.
            record["run_peak_child_rss_kib"] = after.ru_maxrss
            record["executed_actions"] = {"Compiling": log_text.count("   Compiling ")}
            record["cache_hits"] = None
            record["cache_hits_reason"] = "Cargo has no action cache; its target directory is per tree"
        else:
            record.update(bep_metrics(self.logs / f"{trial}.bep.json"))
        record["download_bytes"] = None
        record["download_bytes_reason"] = "local trial: nothing downloaded during the build"
        record["storage_peak_bytes"] = None
        record["storage_peak_reason"] = "not sampled during the build in this run"
        record["correctness"] = self.check(scenario, marker) if status == 0 else {"check": "build", "passed": False}
        raw = [log] + [p for p in [self.logs / f"{trial}.bep.json", self.logs / f"{trial}.profile.gz"] if p.exists()]
        record["raw_logs"] = {p.name: sha256(p) for p in raw}
        (self.results / f"{trial}.json").write_text(json.dumps(record, indent=1) + "\n")
        print(f"{trial}: exit {status}, {wall:.2f} s, check {'passed' if record['correctness']['passed'] else 'FAILED'}", flush=True)
        return record

    def toolchain(self):
        keys = ["SEKKA_BAZEL_RUST_TOOLCHAIN", "SEKKA_BAZEL_NATIVE_ENV"] if self.args.tool == "bazel" else ["RUSTC_WRAPPER", "RUSTFLAGS"]
        out = {k: self.env.get(k) for k in keys}
        rustc = shutil.which("rustc", path=self.env.get("PATH"))
        out["rustc"] = subprocess.run([rustc, "-vV"], capture_output=True, text=True).stdout if rustc else None
        return out

    def storage(self, label):
        roots = [f"worktree={self.tree}"]
        if self.args.tool == "bazel":
            roots += [f"bazel={self.root / 'bazel'}", f"repos={self.root / 'repos'}"]
        else:
            roots += [f"cargo-target={self.cargo_target()}"]
        out = self.results / f"{self.args.tool}-storage-{label}.json"
        subprocess.run([sys.executable, str(Path(__file__).with_name("bazel-storage.py")), "--json", str(out), *roots], check=True)
        return json.loads(out.read_text())

    def run(self):
        a = self.args
        self.checkout()
        if a.tool == "bazel":
            self.targets()
        for scenario in a.scenarios:
            reps = 1 if scenario in ("cold", "revert") else a.reps
            for rep in range(1, reps + 1):
                if scenario in ("leaf", "shared", "build-script"):
                    self.reset()
                marker = self.edit(scenario, rep)
                self.build(scenario, rep, marker)
            if scenario in ("leaf", "shared", "build-script", "revert"):
                self.reset()
            self.storage(scenario)

    def targets(self):
        """The trial's Bazel targets: the workspace's libraries and binaries."""
        out = subprocess.run(
            ["bazel", f"--output_user_root={self.root / 'bazel'}", "query",
             f"--repository_cache={self.root / 'repos'}", self.args.query],
            cwd=self.tree, env=self.env, check=True, capture_output=True, text=True,
        ).stdout
        (self.results / "targets.txt").write_text(out)
        # Fetching is setup, not the cold build: timed apart.
        start = time.monotonic()
        subprocess.run(["bazel", f"--output_user_root={self.root / 'bazel'}", "fetch", "--config=" + self.args.profile,
                        f"--repository_cache={self.root / 'repos'}", "--target_pattern_file=" + str(self.results / "targets.txt")],
                       cwd=self.tree, env=self.env, check=True, capture_output=True)
        (self.results / "bazel-setup.json").write_text(json.dumps({"fetch_wall_seconds": round(time.monotonic() - start, 3)}) + "\n")


def bep_metrics(path):
    """Bazel's own counts for a build, from its Build Event Protocol file."""
    metrics = {}
    if not path.exists():
        return {"bep_missing": True}
    for line in path.read_text().splitlines():
        event = json.loads(line)
        if "buildMetrics" in event:
            metrics = event["buildMetrics"]
    summary = metrics.get("actionSummary", {})
    runners = {r["name"]: int(r.get("count", 0)) for r in summary.get("runnerCount", [])}
    cache = summary.get("actionCacheStatistics", {})
    executed = {d["mnemonic"]: int(d.get("actionsExecuted", 0)) for d in summary.get("actionData", []) if int(d.get("actionsExecuted", 0))}
    timing = metrics.get("timingMetrics", {})
    return {
        "executed_actions": executed,
        "runners": runners,
        "cache_hits": int(cache.get("hits", 0)),
        "cache_misses": int(cache.get("misses", 0)),
        # timingMetrics.cpuTimeInMs counts Bazel's own CPU, not the
        # sandboxed actions' (47.8 s for a cold build that took Cargo
        # 1,803 s on 2026-11-03): kept apart, under its own name.
        "action_cpu_seconds": None,
        "action_cpu_reason": "not in the BEP; needs the execution log or a cgroup",
        "bazel_cpu_seconds": round(int(timing.get("cpuTimeInMs", 0)) / 1000, 3) if "cpuTimeInMs" in timing else None,
        "analysis_seconds": round(int(timing.get("analysisPhaseTimeInMs", 0)) / 1000, 3),
        "execution_seconds": round(int(timing.get("executionPhaseTimeInMs", 0)) / 1000, 3),
        "actions_created": int(summary.get("actionsCreated", 0)),
    }


def summarize(dirs):
    """Per tool and scenario: wall time (median, range), actions, checks."""
    trials = {}
    for d in dirs:
        for path in sorted(p for p in Path(d).rglob("*.json") if "logs" not in p.parts):
            record = json.loads(path.read_text())
            if "scenario" in record:
                trials.setdefault((record["tool"], record["scenario"]), []).append(record)
    rows = {}
    for (tool, scenario), records in sorted(trials.items()):
        walls = [r["wall_seconds"] for r in records if r["exit_status"] == 0]
        executed = [sum(n for m, n in r["executed_actions"].items() if m not in INTERNAL)
                    for r in records if isinstance(r.get("executed_actions"), dict)]
        rows[f"{tool}/{scenario}"] = {
            "trials": len(records),
            "failed_builds": sum(r["exit_status"] != 0 for r in records),
            "checks_passed": sum(bool(r["correctness"]["passed"]) for r in records),
            "wall_seconds": {"median": statistics.median(walls), "min": min(walls), "max": max(walls)} if walls else None,
            "executed_actions_median": statistics.median(executed) if executed else None,
            "cache_hits_median": statistics.median([r["cache_hits"] for r in records]) if all(r.get("cache_hits") is not None for r in records) else None,
            "loadavg_before": [r["host"]["loadavg_before"][0] for r in records],
        }
    return rows


# Bazel's bookkeeping, not compilation: counted apart from executed actions.
INTERNAL = {"RunfilesTree", "BazelWorkspaceStatusAction", "Symlink", "SymlinkTree", "FileWrite",
            "TemplateExpand", "SourceSymlinkManifest", "ExecutableSymlink", "Middleman"}


def main():
    if sys.argv[1:2] == ["summarize"]:
        rows = summarize(sys.argv[2:])
        print(json.dumps(rows, indent=1))
        return 0
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--repo", required=True, help="the checkout to make worktrees from")
    parser.add_argument("--commit", required=True)
    parser.add_argument("--root", required=True, help="where the run's worktrees and build state go")
    parser.add_argument("--results", required=True)
    parser.add_argument("--tool", choices=["bazel", "cargo"], required=True)
    parser.add_argument("--profile", default="dev", choices=["dev"])
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--reps", type=int, default=5)
    parser.add_argument("--scenarios", default=",".join(SCENARIOS))
    parser.add_argument("--query", default=WORKSPACE_TARGETS, help="Bazel's targets (default: the workspace's libraries and binaries)")
    parser.add_argument("--bazel-env", help="sourceable `nix print-dev-env` of .#bazel")
    parser.add_argument("--cargo-env", help="sourceable `nix print-dev-env` of .#aizome")
    parser.add_argument("--timeout", type=int, default=7200, help="seconds per build")
    parser.add_argument("--clean", action="store_true", help="remove the run's root afterwards")
    args = parser.parse_args()
    args.scenarios = [s for s in args.scenarios.split(",") if s]
    unknown = set(args.scenarios) - set(SCENARIOS)
    if unknown:
        parser.error(f"unknown scenarios: {', '.join(sorted(unknown))}")
    if args.tool == "bazel" and not args.bazel_env or args.tool == "cargo" and not args.cargo_env:
        parser.error(f"--{args.tool}-env is required")
    args.commit = subprocess.run(["git", "-C", args.repo, "rev-parse", args.commit], check=True,
                                 capture_output=True, text=True).stdout.strip()
    root = Path(args.root).resolve()
    owned(root, args.run_id, create=True)
    bench = Bench(args)
    try:
        bench.run()
    finally:
        if args.tool == "bazel":
            subprocess.run(["bazel", f"--output_user_root={root / 'bazel'}", "shutdown"], cwd=bench.tree, env=bench.env,
                           capture_output=True)
        if args.clean:
            subprocess.run(["git", "-C", args.repo, "worktree", "remove", "--force", str(bench.tree)], capture_output=True)
            subprocess.run(["chmod", "-R", "u+w", str(root)], capture_output=True)
            shutil.rmtree(root)
    return 0


if __name__ == "__main__":
    sys.exit(main())
