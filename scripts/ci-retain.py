#!/usr/bin/env python3
"""Run Nix with bounded, host-visible GC roots (including build dependencies).

Only Nix store paths persist; this is not a shared workspace or executable cache.
The supervisor mounts SEKKA_CI_ROOTS at the same absolute path on the host and
in jobs, and sets the variable only together with that mount. Workflows run
main's copy of this script, never a pull request's.
"""
import argparse
import contextlib
import errno
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time


GROUP = re.compile(r"(main|pr)-(uta|checks|packages|build-clients)")
GENERATION = re.compile(r"[0-9]+-[a-z0-9_]+")
STORE_PATH = re.compile(r"/nix/store/[0-9a-z]{32}-[^/\s]+")
DAY = 86400
# Covers queued and running clients; finalizers release promptly. Abandoned
# workflow attempts cannot pin their closures indefinitely.
CONSUMER_LEASE_SECONDS = 3 * DAY
# Successful and failed generations each group keeps, newest first.
KEEP = {"main": (2, 1), "pr": (4, 2)}
# Below this much free store space, keep only each group's newest successful
# main generation, so the daemon's own collection (scripts/runner-host-setup.sh:
# from 100 GiB free until 200 GiB are) can reclaim the rest.
MIN_FREE = int(os.environ.get("SEKKA_CI_MIN_FREE_BYTES", 200 * 2**30))


def open_lock(path, create=True):
    """Open a lock file, refusing links and FIFOs, which could block or redirect."""
    flags = os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    fd = os.open(path, flags | (os.O_CREAT if create else 0), 0o644)
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        os.close(fd)
        raise OSError(errno.EINVAL, "not a regular lock file", str(path))
    return fd


@contextlib.contextmanager
def locked(path):
    fd = open_lock(path)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield
    finally:
        os.close(fd)


def building(generation):
    """Whether a running build still holds this generation's lock."""
    try:
        fd = open_lock(generation / ".building", create=False)
    except FileNotFoundError:
        return False
    try:
        fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
        return False
    except BlockingIOError:
        return True
    finally:
        os.close(fd)


def consumer_key():
    values = [os.environ.get(name, "") for name in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT")]
    if not all(re.fullmatch(r"[1-9][0-9]*", value) for value in values):
        raise ValueError("consumer leases require a GitHub run ID and attempt")
    return "-".join(values)


def consumer_lease(generation):
    path = generation / ".consumer.json"
    if path.is_symlink():
        raise ValueError("consumer lease must not be a symlink")
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return None


def lease_consumers(generation, key, now=None):
    now = time.time() if now is None else now
    (generation / ".consumer.json").write_text(json.dumps({
        "run": key, "expires": now + CONSUMER_LEASE_SECONDS,
    }) + "\n")


def release_consumers(root, key):
    group = root / "pr-build-clients"
    if group.is_symlink():
        raise ValueError("retention group must not be a symlink")
    if not group.exists():
        return
    for generation in group.iterdir():
        if generation.is_symlink() or not generation.is_dir() or not GENERATION.fullmatch(generation.name):
            continue
        lease = consumer_lease(generation)
        if lease and lease["run"] == key:
            (generation / ".consumer.json").unlink()


def prune(root, now=None, free=None):
    """Bound each group's generations by count, age and free space.

    Successful generations expire after seven days, except the newest
    successful main one. Failed ones, which root the checks that passed, last
    a day. Generations being built or leased to queued/running build clients
    are never touched. Never follow a symlink in this tree.
    """
    now = time.time() if now is None else now
    free = shutil.disk_usage("/nix/store").free if free is None else free
    for group in sorted(root.iterdir()):
        match = GROUP.fullmatch(group.name)
        if not match or group.is_symlink() or not group.is_dir():
            continue
        main = match[1] == "main"
        keep_successful, keep_failed = KEEP[match[1]] if free >= MIN_FREE else (1 if main else 0, 0)
        generations = sorted(
            (p for p in group.iterdir() if GENERATION.fullmatch(p.name)
             and not p.is_symlink() and p.is_dir()),
            key=lambda p: int(p.name.split("-", 1)[0]), reverse=True,
        )
        successful = failed = 0
        for generation in generations:
            if building(generation):
                continue
            lease = consumer_lease(generation)
            if lease and lease["expires"] > now:
                continue
            age = now - int(generation.name.split("-", 1)[0])
            if (generation / "success").is_file():
                successful += 1
                keep = (main and successful == 1) or (
                    successful <= keep_successful and age < 7 * DAY)
            else:
                failed += 1
                keep = failed <= keep_failed and age < DAY
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


def exit_status(code):
    return code if code >= 0 else 128 - code


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scope", choices=("main", "pr", "prune", "release-consumers"))
    parser.add_argument("label", nargs="?", choices=("uta", "checks", "packages", "build-clients"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.scope not in ("prune", "release-consumers"):
        if not args.label or not (command[:1] == ["build"] or command[:2] == ["flake", "check"]):
            parser.error("expected LABEL followed by build or flake check arguments")
        if any(a in ("--no-link", "--out-link", "-o") or a.startswith("--out-link=") for a in command):
            parser.error("ci-retain manages the out-link")
    key = None
    if args.label == "build-clients" or args.scope == "release-consumers":
        try:
            key = consumer_key()
        except ValueError as error:
            parser.error(str(error))
    root_env = os.environ.get("SEKKA_CI_ROOTS")
    if not root_env:
        if args.scope == "prune":
            return 0
        if key:
            parser.error("cross-pool build inputs require mounted persistent CI roots")
        # A runner without the roots mount, or a maintainer's machine: the same
        # build and status as plain Nix, kept by nothing after the job.
        print("::warning::No persistent CI roots mounted; results are not retained between jobs.", file=sys.stderr)
        no_link = ["--no-link"] if command[0] == "build" else []
        return exit_status(subprocess.run(["nix", *command, *no_link]).returncode)
    root = Path(root_env)
    if not root.is_absolute() or root.is_symlink() or not root.is_dir():
        parser.error("SEKKA_CI_ROOTS must be an existing absolute directory")
    # The tree's lock covers only pruning and creating a generation; each build
    # holds its own generation's lock, so concurrent jobs never wait on another.
    with locked(root / ".lock"):
        if args.scope == "release-consumers":
            release_consumers(root, key)
            prune(root)
            return 0
        prune(root)
        if args.scope == "prune":
            return 0
        group = root / f"{args.scope}-{args.label}"
        if group.is_symlink():
            parser.error("retention group must not be a symlink")
        group.mkdir(exist_ok=True)
        generation = Path(tempfile.mkdtemp(prefix=f"{int(time.time())}-", dir=group))
        build_lock = open_lock(generation / ".building")
        fcntl.flock(build_lock, fcntl.LOCK_EX)
    try:
        result = subprocess.run(["nix", *command, "--out-link", str(generation / "result")])
        # A failed `flake check --keep-going` still links the checks that
        # passed; their inputs are worth keeping for the rerun.
        try:
            retain_dependencies(generation)
        except (OSError, subprocess.CalledProcessError) as error:
            print(f"::warning::Results stay rooted, but not all of their build inputs: {error}", file=sys.stderr)
        if result.returncode == 0:
            (generation / "success").touch()
            if key:
                # Protect the handoff before releasing the build lock, including
                # the low-space prune immediately following this preparation.
                lease_consumers(generation, key)
    finally:
        os.close(build_lock)
    with locked(root / ".lock"):
        prune(root)
    return exit_status(result.returncode)


if __name__ == "__main__":
    sys.exit(main())
