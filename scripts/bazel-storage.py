#!/usr/bin/env python3
"""Account for build storage across roots, each physical file once.

For the Bazel build plan's storage gates (docs/bazel-build-plan.md): summing
`du` over output bases, caches and worktrees counts a hard-linked file once
per link, and following symlinks counts Nix's store or another checkout. This
walks each root without following symlinks, records allocated bytes (blocks,
not apparent size) per root and deduplicated by (device, inode) across all
of them, and optionally writes a manifest of the regular files under each
root: size, allocated bytes, link count, inode and SHA-256.

    scripts/bazel-storage.py --json out.json NAME=PATH [NAME=PATH ...]
    scripts/bazel-storage.py --manifest-dir manifests/ --match '*.rlib' ...

It reads only; a root that doesn't exist is reported as missing, not zero.
"""
import argparse
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import time


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def walk(root):
    """Every entry under root, not following symlinks, as (path, lstat)."""
    stack = [root]
    while stack:
        path = stack.pop()
        try:
            st = os.lstat(path)
        except FileNotFoundError:
            continue
        yield path, st
        if stat.S_ISDIR(st.st_mode):
            try:
                with os.scandir(path) as entries:
                    stack.extend(e.path for e in entries)
            except PermissionError:
                continue


def account(roots, manifest_dir=None, patterns=()):
    seen = {}
    report = {"generated": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "roots": {}}
    for name, root in roots:
        entry = {"path": str(root)}
        if not os.path.lexists(root):
            entry["missing"] = True
            report["roots"][name] = entry
            continue
        allocated = unique = files = dirs = links = symlinks = 0
        manifest = []
        for path, st in walk(str(root)):
            if stat.S_ISDIR(st.st_mode):
                dirs += 1
            elif stat.S_ISLNK(st.st_mode):
                symlinks += 1
            elif stat.S_ISREG(st.st_mode):
                files += 1
                if st.st_nlink > 1:
                    links += 1
            else:
                continue
            blocks = st.st_blocks * 512
            allocated += blocks
            key = (st.st_dev, st.st_ino)
            if key not in seen:
                seen[key] = (name, blocks)
                unique += blocks
            if manifest_dir and stat.S_ISREG(st.st_mode) and (
                not patterns or any(fnmatch.fnmatch(os.path.basename(path), p) for p in patterns)
            ):
                manifest.append({
                    "path": os.path.relpath(path, root),
                    "size": st.st_size,
                    "allocated": blocks,
                    "links": st.st_nlink,
                    "inode": [st.st_dev, st.st_ino],
                    "sha256": sha256(path),
                })
        entry.update({
            "allocated_bytes": allocated,
            # Bytes first seen under this root, in the order given: what it
            # adds to the roots before it.
            "unique_allocated_bytes": unique,
            "files": files,
            "multiply_linked_files": links,
            "directories": dirs,
            "symlinks": symlinks,
        })
        if manifest_dir:
            out = Path(manifest_dir) / f"{name}.json"
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(json.dumps(sorted(manifest, key=lambda m: m["path"]), indent=1) + "\n")
            entry["manifest"] = str(out)
        report["roots"][name] = entry
    report["total_allocated_bytes"] = sum(r.get("allocated_bytes", 0) for r in report["roots"].values())
    report["physical_allocated_bytes"] = sum(blocks for _, blocks in seen.values())
    return report


def parse_root(text):
    name, sep, path = text.partition("=")
    if not sep or not name:
        raise argparse.ArgumentTypeError(f"expected NAME=PATH, got {text!r}")
    return name, Path(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("roots", nargs="+", type=parse_root, metavar="NAME=PATH")
    parser.add_argument("--json", help="write the report here (default: stdout)")
    parser.add_argument("--manifest-dir", help="write each root's file manifest here")
    parser.add_argument("--match", action="append", default=[], help="manifest only files matching this glob")
    args = parser.parse_args()
    report = account(args.roots, args.manifest_dir, args.match)
    text = json.dumps(report, indent=2) + "\n"
    if args.json:
        Path(args.json).write_text(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
