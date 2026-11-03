#!/usr/bin/env python3
"""Run Nix with bounded, host-visible GC roots (including build dependencies).

Only Nix store paths persist; this is not a shared workspace or executable cache.
The runner mounts SEKKA_CI_ROOTS at the same absolute path on the host and in jobs.
"""
import argparse
import fcntl
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


GROUP = re.compile(r"(main|pr)-(uta|checks|packages)")
GENERATION = re.compile(r"[0-9]+-[a-z0-9_]+")
STORE_PATH = re.compile(r"/nix/store/[0-9a-z]{32}-[^/\s]+")
DAY = 86400


def prune(root, now=None):
    """Keep two successful main generations, four PRs, and one recent failure.

    The latest successful main generation has no age limit. Other successes
    expire after seven days; incomplete/failed generations after one day.
    Never follow a directory symlink in this job-writable tree.
    """
    now = time.time() if now is None else now
    for group in root.iterdir():
        match = GROUP.fullmatch(group.name)
        if not match or group.is_symlink() or not group.is_dir():
            continue
        generations = sorted(
            (p for p in group.iterdir() if GENERATION.fullmatch(p.name)
             and not p.is_symlink() and p.is_dir()),
            key=lambda p: int(p.name.split("-", 1)[0]), reverse=True,
        )
        successful = incomplete = 0
        for generation in generations:
            age = now - int(generation.name.split("-", 1)[0])
            if (generation / "success").is_file():
                successful += 1
                main = match[1] == "main"
                keep = (main and successful == 1) or (
                    successful <= (2 if main else 4) and age < 7 * DAY)
            else:
                incomplete += 1
                keep = incomplete == 1 and age < DAY
            if not keep:
                print(f"ci-retain: expiring {generation}", file=sys.stderr)
                shutil.rmtree(generation)


def nix_lines(*args):
    return subprocess.check_output(args, text=True).splitlines()


def retain_dependencies(generation):
    outputs = [str(p.resolve()) for p in generation.glob("result*") if p.is_symlink()]
    if not outputs:
        return
    # Runtime references alone omit compilers, Cargo dependencies and VM images
    # used by a check that only writes a success marker. Retain existing outputs
    # of its derivation closure too, without globally setting keep-outputs.
    derivers = nix_lines("nix-store", "--query", "--deriver", *outputs)
    derivers = [p for p in derivers if STORE_PATH.fullmatch(p) and Path(p).exists()]
    paths = nix_lines("nix-store", "--query", "--requisites", "--include-outputs", *derivers) if derivers else []
    dependencies = generation / "dependencies"
    dependencies.mkdir(exist_ok=True)
    count = 0
    for path in set(paths):
        if STORE_PATH.fullmatch(path) and Path(path).exists():
            (dependencies / Path(path).name).symlink_to(path)
            count += 1
    print(f"ci-retain: rooted {len(outputs)} results and {count} build inputs", file=sys.stderr)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scope", choices=("main", "pr", "prune"))
    parser.add_argument("label", nargs="?", choices=("uta", "checks", "packages"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    root_env = os.environ.get("SEKKA_CI_ROOTS")
    if root_env:
        root = Path(root_env)
        if not root.is_absolute() or not root.is_dir() or root.is_symlink():
            parser.error("SEKKA_CI_ROOTS must be an existing absolute directory")
    else:
        # Workflows can land before the runner image is deployed. Keep the same
        # checks and error status then, with an explicit loss of durable reuse.
        root = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir())) / "sekka-ci-roots"
        root.mkdir(exist_ok=True)
        print("::warning::No persistent CI roots mounted; results last only for this job.", file=sys.stderr)
    with (root / ".lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        prune(root)
        if args.scope == "prune":
            return 0
        command = args.command
        if command[:1] == ["--"]:
            command = command[1:]
        if not args.label or not (command[:1] == ["build"] or command[:2] == ["flake", "check"]):
            parser.error("expected LABEL followed by build or flake check arguments")
        if any(a in ("--no-link", "--out-link", "-o") or a.startswith("--out-link=") for a in command):
            parser.error("ci-retain manages the out-link")
        group = root / f"{args.scope}-{args.label}"
        if group.is_symlink():
            parser.error("retention group must not be a symlink")
        group.mkdir(exist_ok=True)
        generation = Path(tempfile.mkdtemp(prefix=f"{int(time.time())}-", dir=group))
        result = subprocess.run(["nix", *command, "--out-link", str(generation / "result")])
        retain_dependencies(generation)
        if result.returncode == 0:
            (generation / "success").touch()
        prune(root)
        return result.returncode if result.returncode >= 0 else 128 - result.returncode


if __name__ == "__main__":
    sys.exit(main())
