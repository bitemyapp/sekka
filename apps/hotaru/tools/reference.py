#!/usr/bin/env python3
"""Fetch a pinned external Zig oracle and build its Linux benchmark tools.

The reference checkout is deliberately outside Hotaru. No Zig or macOS product
sources are vendored. Run this script on wx-workstation, with the pins beside it.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(argv, **kwargs):
    print("+ " + " ".join(map(str, argv)), flush=True)
    return subprocess.run(list(map(str, argv)), check=True, **kwargs)


def capture(argv, **kwargs):
    return subprocess.check_output(list(map(str, argv)), text=True, **kwargs).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare", "build", "metadata"])
    parser.add_argument("--pins", type=Path, default=Path(__file__).resolve().parent.parent / "upstream/ghostty.json")
    parser.add_argument("--root", type=Path, default=Path.home() / "work/ghostty-reference-20261028")
    parser.add_argument("--cache", type=Path, default=Path.home() / ".cache/hotaru-reference")
    parser.add_argument("--jobs", type=int, default=16)
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("this pinned baseline toolchain is for x86_64 Linux")
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    pins = json.loads(args.pins.read_text())
    args.root = args.root.resolve()
    args.cache.mkdir(parents=True, exist_ok=True)
    args.lock.parent.mkdir(parents=True, exist_ok=True)
    zig_dir = args.cache / ("zig-x86_64-linux-" + pins["zig"]["version"])
    zig = zig_dir / "zig"
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if args.action == "prepare":
            if not args.root.exists():
                run(["git", "clone", "--no-checkout", pins["repository"], args.root])
            if capture(["git", "status", "--porcelain", "--untracked-files=no"], cwd=args.root):
                raise SystemExit("reference has tracked edits; refusing to change its checkout")
            run(["git", "fetch", "origin", pins["revision"]], cwd=args.root)
            run(["git", "checkout", "--detach", pins["revision"]], cwd=args.root)
            fork = pins["clipboard_fork"]
            run(["git", "fetch", fork["repository"], fork["revision"]], cwd=args.root)
            if not zig.exists():
                archive = args.cache / "zig-0.17.0.tar.xz"
                if not archive.exists() or sha256(archive) != pins["zig"]["sha256"]:
                    partial = archive.with_suffix(".partial")
                    print("Downloading pinned Zig toolchain", flush=True)
                    with urllib.request.urlopen(pins["zig"]["url"], timeout=60) as source, partial.open("wb") as target:
                        while chunk := source.read(1024 * 1024):
                            target.write(chunk)
                    if sha256(partial) != pins["zig"]["sha256"]:
                        raise SystemExit("Zig archive checksum mismatch")
                    partial.replace(archive)
                with tarfile.open(archive) as tar:
                    tar.extractall(args.cache, filter="data")
            if capture([zig, "version"]) != pins["zig"]["version"]:
                raise SystemExit("unexpected Zig version")
        else:
            actual = capture(["git", "rev-parse", "HEAD"], cwd=args.root)
            if actual != pins["revision"]:
                raise SystemExit(f"reference HEAD {actual} does not match pin")
            if capture(["git", "status", "--porcelain", "--untracked-files=no"], cwd=args.root):
                raise SystemExit("refusing a modified reference baseline")
            if capture([zig, "version"]) != pins["zig"]["version"]:
                raise SystemExit("unexpected Zig version; run prepare first")
            command = [str(zig), "build", "-Demit-bench=true", "-Demit-lib-vt=true",
                       "-Demit-exe=false", "-Dapp-runtime=none", "-Doptimize=ReleaseFast",
                       "-Demit-docs=false", "-Demit-terminfo=false", f"-j{args.jobs}"]
            if args.action == "build":
                run(command, cwd=args.root)
                replay_source = Path(__file__).resolve().parent / "reference-replay.c"
                run(["cc", "-std=c11", "-O3", "-march=native", "-Wall", "-Wextra", "-Werror",
                     "-I" + str(args.root / "include"), replay_source,
                     "-L" + str(args.root / "zig-out/lib"),
                     "-Wl,-rpath," + str(args.root / "zig-out/lib"), "-lghostty-vt",
                     "-o", args.root / "zig-out/bin/ghostty-reference-replay"])
            binaries = {}
            for name, relative in (("ghostty-bench", "bin/ghostty-bench"),
                                   ("ghostty-gen", "bin/ghostty-gen"),
                                   ("libghostty-vt", "lib/libghostty-vt.so"),
                                   ("ghostty-reference-replay", "bin/ghostty-reference-replay")):
                path = args.root / "zig-out" / relative
                if path.exists():
                    binaries[name] = {"path": str(path), "sha256": sha256(path)}
            metadata = {
                "schema": 1, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "revision": actual, "zig": capture([zig, "version"]),
                "zig_sha256": sha256(zig), "build_command": command,
                "host": platform.node(), "platform": platform.platform(), "machine": platform.machine(),
                "binaries": binaries,
                "build_confirmed": args.action == "build",
                "scope": "Linux terminal-core baseline; no GUI or performance parity claim"
            }
            if args.action == "build":
                metadata["adapter"] = {"source_sha256": sha256(replay_source),
                                       "compiler": capture(["cc", "--version"]).splitlines()[0],
                                       "flags": ["-std=c11", "-O3", "-march=native"]}
            destination = args.root / "hotaru-baseline.json"
            destination.write_text(json.dumps(metadata, indent=2) + "\n")
            print(destination)


if __name__ == "__main__":
    main()
