#!/usr/bin/env python3
"""Create an auditable per-file port ledger from `git ls-tree -r -l PIN src`.

Paths and Git blob identities only: this does not copy upstream source files.
Existing evidence survives regeneration only if its upstream blob is unchanged.
"""
import argparse
from collections import Counter
import json
from pathlib import Path


def classify(path):
    lower = path.lower()
    if any(part in lower for part in ("/metal/", "/coretext.", "/keymapdarwin.", "/os/macos.", "/os/mach.", "/os/cf_release_thread.", "/build/metallib", "/build/xcframework", "/build/macos")) or path in {"src/renderer/Metal.zig", "src/renderer/DisplayLink.zig", "src/apprt/embedded.zig", "src/renderer/shaders/shaders.metal"}:
        return "excluded-platform", "macOS only; no product source imported"
    if any(part in lower for part in ("/wasm", "_wasm.", "/os/windows.", "/tinyio/windows.", "/lib/windows_dll.", "/kitty/windows.")) or path == "src/apprt/browser.zig":
        return "excluded-platform", "non-Linux target; no product source imported"
    if path.startswith("src/apprt/gtk"):
        return "native-ui-replacement", "Aizome native replacement; preserve applicable user behavior"
    if path.startswith("src/build/"):
        return "build-replacement", "Cargo/Nix replacement"
    if path.startswith("src/benchmark/"):
        return "validation", "external Zig oracle and equivalent Rust measurements"
    return "linux-port", "Linux or shared behavior requires port and differential evidence"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tree", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    previous = {}
    if args.output.exists():
        previous = {entry["path"]: entry for entry in json.loads(args.output.read_text())["files"]}
    entries = []
    for line in args.tree.read_text().splitlines():
        description, path = line.split("\t", 1)
        mode, kind, blob, size = description.split()
        if not path.startswith("src/") or Path(path).suffix not in {".zig", ".c", ".cpp", ".h", ".glsl", ".metal"}:
            continue
        scope, note = classify(path)
        entry = {"path": path, "blob": blob, "bytes": int(size), "scope": scope,
                 "status": "excluded" if scope == "excluded-platform" else "pending",
                 "rust_paths": [], "evidence": [], "note": note}
        old = previous.get(path)
        if old and old["blob"] == blob and old["scope"] == scope:
            for field in ("status", "rust_paths", "evidence", "note"):
                entry[field] = old[field]
        entries.append(entry)
    entries.sort(key=lambda entry: entry["path"])
    result = {"schema": 1, "revision": args.revision,
              "coverage": "Every tracked .zig/.c/.cpp/.h/.glsl/.metal file under src; non-code assets, include ABI and external integration tests remain separate acceptance gates",
              "status_rule": "pending remains pending until named Rust code and reproducible behavior evidence exist; excluded describes platform scope, not completed porting",
              "counts": dict(sorted(Counter(entry["scope"] for entry in entries).items())), "files": entries}
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["counts"]))


if __name__ == "__main__":
    main()
