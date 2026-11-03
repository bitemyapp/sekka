#!/usr/bin/env python3
"""Run Nix with bounded, host-visible GC roots (including build dependencies).

Only Nix store paths persist; this is not a shared workspace or executable cache.
The supervisor mounts SEKKA_CI_ROOTS at the same absolute path on the host and
in jobs, and sets the variable only together with that mount. Workflows run
main's copy of this script, never a pull request's.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import contextlib
import errno
import fcntl
import gzip
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time


GROUP = re.compile(r"(main|pr)-(uta|checks|packages|iso)")
GENERATION = re.compile(r"[0-9]+-[a-z0-9_]+")
STORE_PATH = re.compile(r"/nix/store/[0-9a-z]{32}-[^/\s]+")
DAY = 86400
# Each derivation's build inputs, queried once and reused by later runs that
# build the same derivation: a query costs about half a second.
CLOSURES = ".closures"
# Successful and failed generations each group keeps, newest first.
KEEP = {"main": (2, 1), "pr": (4, 2)}
# Below this much free store space, keep only each group's newest successful
# main generation, so the daemon's own collection (scripts/runner-host-setup.sh:
# from 100 GiB free until 200 GiB are) can reclaim the rest.
MIN_FREE = int(os.environ.get("SEKKA_CI_MIN_FREE_BYTES", 200 * 2**30))


def open_regular(path, flags):
    """Open a file in the tree, refusing links and FIFOs, which could block or redirect."""
    fd = os.open(path, flags | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, 0o644)
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        os.close(fd)
        raise OSError(errno.EINVAL, "not a regular file", str(path))
    return fd


def open_lock(path, create=True):
    return open_regular(path, os.O_RDWR | (os.O_CREAT if create else 0))


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


def prune(root, now=None, free=None):
    """Bound each group's generations by count, age and free space.

    Successful generations expire after seven days, except the newest
    successful main one. Failed ones, which root the checks that passed, last
    a day. A generation still being built is never touched. Never follow a
    symlink in this tree.
    """
    now = time.time() if now is None else now
    free = shutil.disk_usage("/nix/store").free if free is None else free
    closures = root / CLOSURES
    if closures.is_dir() and not closures.is_symlink():
        # Reuse refreshes a record; after a week unused, drop it.
        for record in closures.iterdir():
            info = record.lstat()
            if stat.S_ISREG(info.st_mode) and now - info.st_mtime >= 7 * DAY:
                record.unlink()
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


def build_inputs(closures, deriver):
    """What building one derivation used: its derivation closure's outputs.

    Recorded after a build, when every input is present, and reused for as
    long as runs keep building the same derivation. Returns the paths and
    whether Nix had to be asked.
    """
    record = closures / Path(deriver).name
    try:
        with gzip.open(os.fdopen(open_regular(record, os.O_RDONLY), "rb"), "rt") as f:
            paths = f.read().split()
        os.utime(record, follow_symlinks=False)
        return paths, False
    except FileNotFoundError:
        pass
    except (OSError, EOFError) as error:
        print(f"ci-retain: replacing unreadable record {record}: {error}", file=sys.stderr)
    paths = nix_lines("nix-store", "--query", "--requisites", "--include-outputs", deriver)
    partial = closures / f".{record.name}.{os.getpid()}"
    with gzip.open(partial, "wt") as f:
        f.write("\n".join(paths))
    os.replace(partial, record)
    return paths, True


def retain_dependencies(generation):
    outputs = [str(p.resolve()) for p in generation.glob("result*") if p.is_symlink()]
    if not outputs:
        return
    # Runtime references alone omit compilers, Cargo dependencies and VM images
    # used by a check that only writes a success marker. Retain existing outputs
    # of its derivation closure too, without globally setting keep-outputs.
    derivers = nix_lines("nix-store", "--query", "--deriver", *outputs)
    derivers = sorted({p for p in derivers if STORE_PATH.fullmatch(p) and Path(p).exists()})
    closures = generation.parent.parent / CLOSURES
    if closures.is_symlink():
        raise OSError(errno.EINVAL, "closure records must not be a symlink", str(closures))
    closures.mkdir(exist_ok=True)
    with ThreadPoolExecutor(8) as pool:
        found = list(pool.map(lambda deriver: build_inputs(closures, deriver), derivers))
    dependencies = generation / "dependencies"
    dependencies.mkdir(exist_ok=True)
    count = 0
    for path in {path for paths, _ in found for path in paths}:
        if STORE_PATH.fullmatch(path) and Path(path).exists():
            (dependencies / Path(path).name).symlink_to(path)
            count += 1
    queried = sum(new for _, new in found)
    print(f"ci-retain: rooted {len(outputs)} results and {count} build inputs"
          f" (queried {queried} of {len(derivers)} derivations)", file=sys.stderr)


def exit_status(code):
    return code if code >= 0 else 128 - code


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scope", choices=("main", "pr", "prune"))
    parser.add_argument("label", nargs="?", choices=("uta", "checks", "packages", "iso"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if args.scope != "prune":
        if not args.label or not (command[:1] == ["build"] or command[:2] == ["flake", "check"]):
            parser.error("expected LABEL followed by build or flake check arguments")
        if any(a in ("--no-link", "--out-link", "-o") or a.startswith("--out-link=") for a in command):
            parser.error("ci-retain manages the out-link")
    root_env = os.environ.get("SEKKA_CI_ROOTS")
    if not root_env:
        if args.scope == "prune":
            return 0
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
    finally:
        os.close(build_lock)
    with locked(root / ".lock"):
        prune(root)
    return exit_status(result.returncode)


if __name__ == "__main__":
    sys.exit(main())
