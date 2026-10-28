#!/usr/bin/env python3
"""Build unchanged upstream's Linux GUI outside Hotaru, using pinned Nixpkgs.

The separate output preserves the native VT oracle and its recorded hashes.
This is a reference build, not a GTK dependency of the Rust application.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import platform
import re
import shutil
import subprocess
from pathlib import Path


def capture(argv, **kwargs):
    return subprocess.check_output(list(map(str, argv)), text=True, **kwargs).strip()


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nixpkgs", type=Path, required=True,
                        help="store source from Sekka's locked nixpkgs input")
    parser.add_argument("--root", type=Path,
                        default=Path.home() / "work/ghostty-reference-20261028")
    parser.add_argument("--pins", type=Path,
                        default=Path(__file__).resolve().parent.parent / "upstream/ghostty.json")
    parser.add_argument("--out-link", type=Path,
                        default=Path.home() / ".cache/hotaru-reference/gui-result")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--lock", type=Path,
                        default=Path.home() / ".cache/hotaru-workstation.lock")
    parser.add_argument("--jobs", type=int, default=16)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("the reference package currently targets x86_64 Linux")
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    nix = shutil.which("nix") or "/nix/var/nix/profiles/default/bin/nix"
    nix_store = shutil.which("nix-store") or "/nix/var/nix/profiles/default/bin/nix-store"
    root = args.root.resolve(strict=True)
    nixpkgs = args.nixpkgs.resolve(strict=True)
    pins = json.loads(args.pins.read_text())
    output = args.output or root / "hotaru-gui-baseline.json"
    args.out_link.parent.mkdir(parents=True, exist_ok=True)
    args.lock.parent.mkdir(parents=True, exist_ok=True)
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        actual = capture(["git", "rev-parse", "HEAD"], cwd=root)
        if actual != pins["revision"]:
            raise SystemExit("reference HEAD does not match the pin")
        if capture(["git", "status", "--porcelain", "--untracked-files=no"], cwd=root):
            raise SystemExit("refusing a modified reference baseline")
        # Snapshot the terminal-only outputs to prove this build preserves them.
        vt_paths = [root / "zig-out" / name for name in (
            "bin/ghostty-bench", "bin/ghostty-gen", "lib/libghostty-vt.so",
            "bin/ghostty-reference-replay")]
        vt_before = {str(path): sha256(path) for path in vt_paths if path.exists()}
        command = [nix, "build", "--impure", "--file",
                   str(Path(__file__).with_suffix(".nix")),
                   "--argstr", "nixpkgs", str(nixpkgs),
                   "--argstr", "reference", str(root),
                   "--argstr", "revision", actual,
                   "--cores", str(args.jobs), "--max-jobs", "1",
                   "--out-link", str(args.out_link.absolute()), "--json"]
        print("+ " + " ".join(command), flush=True)
        results = json.loads(capture(command))
        if len(results) != 1:
            raise SystemExit("expected one reference derivation")
        result = results[0]
        package = Path(result["outputs"]["out"])
        wrapper = package / "bin/ghostty"
        wrapped = package / "bin/.ghostty-wrapped"
        if not wrapped.is_file():
            raise SystemExit("expected upstream's wrapped Linux executable")
        vt_after = {path: sha256(path) for path in vt_before}
        if vt_before != vt_after:
            raise SystemExit("terminal-only oracle changed during GUI build")
        # The host's ldd may invoke its own older glibc loader and falsely
        # report missing symbol versions for Nix binaries. Use this ELF's loader.
        headers = capture(["readelf", "--program-headers", wrapped])
        interpreter = re.search(r"Requesting program interpreter: (.*?)\]", headers)
        if not interpreter:
            raise SystemExit("missing ELF interpreter")
        dependencies = capture([interpreter[1], "--list", wrapped])
        if "not found" in dependencies:
            raise SystemExit("reference GUI has missing runtime libraries")
        metadata = {
            "schema": 1,
            "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "scope": "Unchanged current Ghostty Linux GUI reference; no parity claim",
            "reference_revision": actual,
            "reference_tracked_clean": True,
            "build_command": command,
            "package_definition_sha256": sha256(root / "nix/package.nix"),
            "nixpkgs": str(nixpkgs),
            "nixpkgs_nar_hash": capture([nix, "hash", "path", str(nixpkgs)]),
            "nix_version": capture([nix, "--version"]),
            "derivation": result["drvPath"],
            "outputs": result["outputs"],
            "package_references": capture([nix_store, "--query", "--references", package]).splitlines(),
            "cpu_target": "baseline (unchanged upstream package default)",
            "optimization": "ReleaseFast",
            "host": platform.node(),
            "platform": platform.platform(),
            "wrapper": {"path": str(wrapper), "sha256": sha256(wrapper)},
            "binary": {"path": str(wrapped), "sha256": sha256(wrapped)},
            "version_output": capture([wrapper, "--version"]),
            "linked_libraries": dependencies,
            "preserved_terminal_oracle": vt_after,
        }
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(metadata, indent=2) + "\n")
        print(output)


if __name__ == "__main__":
    main()
