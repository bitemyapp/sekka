#!/usr/bin/env python3
"""Build a native candidate and record exact source/toolchain provenance.

Run on the measurement host after syncing the desired source tree. The shared
lock prevents cooperating builds/timers from overlapping. Source hashes are
checked before and after compilation, including Cargo's dependency lockfile.
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


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sources(workspace):
    paths = [workspace / "Cargo.toml", workspace / "Cargo.lock"]
    for directory in (workspace / "crates", workspace / ".cargo", workspace / "vendor"):
        if directory.is_dir():
            paths.extend(path for path in directory.rglob("*")
                         if path.is_file() and not {"target", ".git"}.intersection(path.parts))
    return {str(path.relative_to(workspace)): digest(path) for path in sorted(paths)}


def capture(command, **kwargs):
    return subprocess.check_output(command, text=True, **kwargs).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--revision", required=True, help="base commit plus explanation of uncommitted edits")
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--toolchain", default="1.95.0")
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--package", choices=("hotaru-core", "hotaru-replay"), default="hotaru-replay")
    parser.add_argument("--example", help="Build a named Cargo example instead of the replay executable")
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if args.package == "hotaru-core" and not args.example:
        parser.error("hotaru-core has no executable; supply --example")
    workspace = args.workspace.resolve()
    args.lock.parent.mkdir(parents=True, exist_ok=True)
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        before = sources(workspace)
        rustc = capture(["rustup", "which", "--toolchain", args.toolchain, "rustc"])
        environment = os.environ.copy()
        for name in ("CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_TARGET"):
            environment.pop(name, None)
        settings = {"RUSTC": rustc, "RUSTFLAGS": "-C target-cpu=native",
                    "CARGO_TARGET_DIR": str(workspace / "target"),
                    "CARGO_PROFILE_RELEASE_OPT_LEVEL": "3", "CARGO_PROFILE_RELEASE_LTO": "thin",
                    "CARGO_PROFILE_RELEASE_CODEGEN_UNITS": "1", "CARGO_PROFILE_RELEASE_DEBUG": "0",
                    "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS": "false",
                    "CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS": "false"}
        environment.update(settings)
        # Mirroring with rsync preserves mtimes. Cargo can otherwise accept an
        # older cached artifact even when mirrored source bytes have changed.
        # Recompile our two packages; immutable registry dependencies can stay
        # cached. This makes the source digest identify what rustc actually saw.
        clean_command = ["rustup", "run", args.toolchain, "cargo", "clean", "--release",
                         "-p", "hotaru-core", "-p", "hotaru-replay"]
        subprocess.run(clean_command, cwd=workspace, env=environment, check=True)
        command = ["rustup", "run", args.toolchain, "cargo", "build", "--locked", "--release",
                   "-p", args.package, "-j", str(args.jobs)]
        if args.example:
            command.extend(["--example", args.example])
        subprocess.run(command, cwd=workspace, env=environment, check=True)
        if before != sources(workspace):
            raise SystemExit("source tree changed during compilation; provenance invalid")
        binary = workspace / (f"target/release/examples/{args.example}" if args.example
                              else "target/release/hotaru-replay")
        metadata = {"schema": 1, "build_confirmed": True,
                    "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    "revision": args.revision, "workspace": str(workspace),
                    "source_sha256": hashlib.sha256(json.dumps(before, sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
                    "sources": before, "binary": str(binary), "binary_sha256": digest(binary),
                    "toolchain": args.toolchain, "rustc": capture([rustc, "--version", "--verbose"]),
                    "rustc_sha256": digest(Path(rustc)),
                    "cargo": capture(["rustup", "run", args.toolchain, "cargo", "--version", "--verbose"]),
                    "clean_command": clean_command, "command": command, "environment": settings,
                    "host": platform.node(), "platform": platform.platform()}
        args.metadata.parent.mkdir(parents=True, exist_ok=True)
        args.metadata.write_text(json.dumps(metadata, indent=2) + "\n")
        print(json.dumps({"metadata": str(args.metadata), "source_sha256": metadata["source_sha256"],
                          "binary_sha256": metadata["binary_sha256"]}))


if __name__ == "__main__":
    main()
