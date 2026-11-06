#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Record what a Mac build was made from and what it produced: the source
revision, every locked Rust crate (name, version, source, checksum), the
engine's source lock, the toolchain, and each artifact's size and SHA-256.
A software bill of materials and a build record in one canonical JSON
document, `sekka.mac.provenance/1`.

    provenance.py --workspace DIR --release true|false --artifact PATH...
"""
import hashlib
import json
import os
import platform
import subprocess
import sys
import tomllib


def sha256_path(path):
    """A file's digest, or a directory's: the digest of its sorted
    (relative path, size, file digest) list."""
    if os.path.isdir(path):
        entries = []
        for d, _, files in os.walk(path):
            for f in files:
                p = os.path.join(d, f)
                if os.path.islink(p):
                    entries.append([os.path.relpath(p, path), "link", os.readlink(p)])
                else:
                    size, digest = sha256_path(p)
                    entries.append([os.path.relpath(p, path), size, digest])
        entries.sort()
        blob = json.dumps(entries, separators=(",", ":"), sort_keys=True).encode()
        return sum(e[1] for e in entries if isinstance(e[1], int)), "sha256:" + hashlib.sha256(blob).hexdigest()
    h = hashlib.sha256()
    size = 0
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
            size += len(chunk)
    return size, "sha256:" + h.hexdigest()


def run(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def crates(lock_path):
    with open(lock_path, "rb") as f:
        lock = tomllib.load(f)
    out = []
    for p in lock.get("package", []):
        out.append({
            "name": p["name"],
            "version": p["version"],
            "source": p.get("source", "local"),
            "checksum": p.get("checksum"),
        })
    return sorted(out, key=lambda c: (c["name"], c["version"]))


def record(workspace, release, artifacts):
    lock = os.path.join(workspace, "Cargo.lock")
    engine_lock = os.path.join(workspace, "engine", "source-lock.json")
    doc = {
        "schema": "sekka.mac.provenance/1",
        "release": release,
        "source": {
            "revision": run("git", "-C", workspace, "rev-parse", "HEAD"),
            "clean": run("git", "-C", workspace, "status", "--porcelain", "--", ".") == "",
            "cargo_lock": sha256_path(lock)[1],
        },
        "crates": crates(lock),
        "engine": json.load(open(engine_lock)) if os.path.exists(engine_lock) else None,
        "toolchain": {
            "rustc": run("rustc", "-V"),
            "cargo": run("cargo", "-V"),
            "host": platform.platform(),
            "macos": run("sw_vers", "-productVersion"),
            "xcode": run("xcodebuild", "-version"),
            "sdk": run("xcrun", "--show-sdk-version"),
        },
        "artifacts": [],
    }
    for a in artifacts:
        size, digest = sha256_path(a)
        doc["artifacts"].append({"name": os.path.basename(a.rstrip("/")), "size": size, "digest": digest})
    return doc


def main(argv):
    workspace, release, artifacts = ".", False, []
    i = 0
    while i < len(argv):
        if argv[i] == "--workspace":
            workspace = argv[i + 1]
        elif argv[i] == "--release":
            release = argv[i + 1] == "true"
        elif argv[i] == "--artifact":
            artifacts.append(argv[i + 1])
        else:
            print(f"unknown argument {argv[i]}", file=sys.stderr)
            return 2
        i += 2
    print(json.dumps(record(workspace, release, artifacts), sort_keys=True, separators=(",", ":"), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
