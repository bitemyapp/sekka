#!/usr/bin/env python3
"""Prepare the qualified expanded oracle and optional Rust transport wrapper.

No build is started. Build the emitted command under the shared workstation lock.
The production crate remains unchanged; the transport wrapper is diagnostic only.
"""
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True,
                        help="new oracle-expanded directory")
    parser.add_argument("--zig", type=pathlib.Path, required=True)
    parser.add_argument("--candidate-source", type=pathlib.Path,
                        help="optional production hotaru-sprites crate; copied to sibling rust/")
    args = parser.parse_args()
    reference = args.reference.resolve(strict=True)
    output = args.output.resolve()
    build = json.loads((ROOT / "evidence/build.json").read_text())
    powerline = "src/font/sprite/draw/powerline.zig"
    expanded = ROOT / "tools/oracle-expanded.zig"
    if sha(reference / powerline) != build["reference_sources"][powerline]:
        raise SystemExit("modified Powerline reference")
    if sha(expanded) != build["reference_sources"]["oracle.zig"]:
        raise SystemExit("expanded oracle adapter differs from qualification")
    candidate = output.parent / "rust"
    if args.candidate_source and candidate.exists():
        raise SystemExit(f"candidate output already exists: {candidate}")
    result = subprocess.run([sys.executable, str(ROOT / "tools/prepare-oracle.py"),
        "--reference", str(reference), "--output", str(output), "--zig", str(args.zig)],
        check=True, capture_output=True, text=True)
    report = json.loads(result.stdout)
    shutil.copyfile(expanded, output / "oracle.zig")
    shutil.copyfile(reference / powerline, output / powerline)
    report["expanded_sources"] = {powerline: sha(output / powerline),
                                  "oracle.zig": sha(output / "oracle.zig")}
    if args.candidate_source:
        source = args.candidate_source.resolve(strict=True)
        candidate.mkdir()
        for folder in ("src", "tests"):
            shutil.copytree(source / folder, candidate / folder)
        for license_path in source.glob("LICENSE*"):
            shutil.copyfile(license_path, candidate / license_path.name)
        shutil.copyfile(ROOT / "tools/prototype-Cargo.toml", candidate / "Cargo.toml")
        shutil.copyfile(ROOT / "tools/prototype-main.rs", candidate / "src/main.rs")
        report["candidate_root"] = str(candidate)
        report["candidate_build"] = ["cargo", "+1.95.0", "build", "--release",
                                     "--manifest-path", str(candidate / "Cargo.toml")]
    print(json.dumps(report, indent=2))

if __name__ == "__main__":
    main()
