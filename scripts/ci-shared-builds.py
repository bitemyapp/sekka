#!/usr/bin/env python3
"""Prepare candidate closures in Nix; execute them only in the isolated client pool.

The workflow copies this helper and ci-retain.py from main. `prepare` never
executes a candidate output. `run` requires the compute-only container, which
has no daemon, shared roots, Docker socket or host home and only reader tokens.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile

STORE_PATH = re.compile(r"/nix/store/[0-9a-z]{32}-[^/\s]+")
GROUPS = [
    ("crates/aizome-kit", "lib_test", "aizome_kit-test-test-lib"),
    ("crates/aizome-sys", "lib_test", "aizome_sys-test-test-lib"),
    ("apps/hako", "lib_test", "hako-test-test-lib"),
    ("apps/hako", "file_manager", "file_manager-test-test-test"),
    ("crates/aizome-shell", "lib_test", "aizome_shell-test-test-lib"),
    ("apps/yukimi", "lib_test", "aizome_yukimi-test-test-lib"),
    ("apps/yukimi", "look", "look-test-test-test"),
    ("apps/yukimi", "headless", "headless-test-test-test"),
    ("apps/yukimi", "golden", "golden-test-test-test"),
    ("apps/soroban", "lib_test", "aizome_soroban-test-test-lib"),
    ("apps/soroban", "soroban", "soroban-test-test-test"),
]


def store_directory(value):
    if not STORE_PATH.fullmatch(value):
        raise ValueError("expected one absolute Nix store root")
    path = Path(value)
    if path.is_symlink() or not path.is_dir():
        raise ValueError("expected a realized Nix store directory")
    return path


def bundle_paths(value):
    bundle = store_directory(value)
    return bundle, {name: store_directory(str((bundle / name).resolve(strict=True)))
                    for name in ("source", "bazel", "buck2")}


def prepare():
    result = subprocess.run([
        sys.executable, str(Path(__file__).with_name("ci-retain.py")),
        "pr", "build-clients", "build", "--no-update-lock-file",
        "--print-build-logs", "--print-out-paths", ".#shared-build-inputs",
    ], check=True, text=True, stdout=subprocess.PIPE)
    lines = result.stdout.splitlines()
    if len(lines) != 1:
        raise ValueError("Nix did not return exactly one candidate bundle")
    bundle, _ = bundle_paths(lines[0])
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"bundle={bundle}\n")
        output.write(f"attempt={os.environ['GITHUB_RUN_ATTEMPT']}\n")
    print(f"Prepared candidate inputs: {bundle}")


def require_prepared_attempt(prepared, current):
    if not prepared or prepared != current:
        raise RuntimeError(
            "This partial rerun reuses inputs whose consumer lease was released. "
            "Use Re-run all jobs to prepare fresh protected inputs."
        )


def execute(args):
    require_prepared_attempt(args.prepared_attempt, os.environ.get("GITHUB_RUN_ATTEMPT"))
    # A role label is not the isolation boundary: the immutable runner image
    # supplies the mounts. Refuse obvious misdeployment before candidate code.
    for forbidden in ("/nix/var/nix/daemon-socket/socket", "/nix/var/nix/gcroots/sekka-ci",
                      "/run/docker.sock", "/var/cache/sekka-ci-git"):
        if Path(forbidden).exists():
            raise RuntimeError(f"candidate execution refused with host capability mounted: {forbidden}")
    bundle, paths = bundle_paths(args.bundle)
    root = Path(tempfile.mkdtemp(prefix=f"shared-build-{args.tool}-", dir=os.environ.get("RUNNER_TEMP")))
    configuration = root / ("reader.bazelrc" if args.tool == "bazel" else "reader.buckconfig")
    shutil.copyfile(Path("/run/sekka") / configuration.name, configuration)
    configuration.chmod(0o600)
    workspace = root / "source"
    shutil.copytree(paths["source"], workspace, symlinks=True)
    for directory, _, files in os.walk(workspace, followlinks=False):
        parent = Path(directory)
        parent.chmod(parent.stat().st_mode | stat.S_IWUSR)
        for name in files:
            path = parent / name
            if not path.is_symlink():
                path.chmod(path.stat().st_mode | stat.S_IWUSR)
    env = dict(os.environ, TMPDIR=str(root / "tmp"), XDG_RUNTIME_DIR=str(root / "runtime"),
               DBUS_SESSION_BUS_ADDRESS=f"unix:path={root}/no-session-bus",
               DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={root}/no-system-bus")
    for name in ("tmp", "runtime"):
        (root / name).mkdir(mode=0o700)
    if args.tool == "bazel":
        prefix = [str(paths["bazel"] / "bin/sekka-bazel-client"), "--workspace", str(workspace),
                  "--cache", "ci", "--remote-rc", str(configuration)]
        # Docker is the security boundary; no nested user-namespace permission
        # or weakened host seccomp policy is needed for action execution.
        common = ["--jobs=16", "--spawn_strategy=standalone", "--genrule_strategy=standalone"]
        targets = [f"//desktop/aizome/{path}:{bazel}" for path, bazel, _ in GROUPS]
        commands = [prefix + ["build", *common],
                    prefix + ["test", *targets, *common, "--test_output=errors"],
                    prefix + ["run", *common, "--", "--build-info"]]
    else:
        prefix = [str(paths["buck2"] / "bin/sekka-buck2"), "--cache", "cache",
                  "--local-content-store", "off",
                  "--config-file", str(configuration), "--jobs", "16"]
        targets = [f"//desktop/aizome/{path}:{buck}" for path, _, buck in GROUPS]
        commands = [prefix + ["build"], prefix + ["test", *targets],
                    prefix + ["run", "//desktop/aizome/crates/aizome-comp:aizome-build-dev", "--", "--build-info"]]
    report = {"bundle": str(bundle), "tool": args.tool, "commands": []}
    try:
        for command in commands:
            print("Running:", " ".join(command), flush=True)
            result = subprocess.run(command, cwd=workspace, env=env)
            report["commands"].append({"argv": command, "exit_status": result.returncode})
            result.check_returncode()
    finally:
        (root / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"Build evidence: {root / 'result.json'}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("prepare")
    run = commands.add_parser("run")
    run.add_argument("--bundle", required=True)
    run.add_argument("--tool", choices=("bazel", "buck2"), required=True)
    run.add_argument("--prepared-attempt", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare()
    else:
        execute(args)


if __name__ == "__main__":
    main()
