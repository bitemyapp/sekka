#!/usr/bin/env python3
"""Regenerate Bazel's view of Aizome's crates and repin crate_universe.

After a change to a Cargo manifest, desktop/aizome/Cargo.lock or MODULE.bazel's
crate annotations, run in `nix develop .#bazel`:

    python3 build/bazel/repin.py

It runs build/bazel/generate.py, then has crate_universe resolve the mirror
(build/bazel/aizome-cargo) against Cargo.lock into
build/bazel/aizome-crates.lock.json. Cargo's lock is Cargo's: crate_universe
copies it back after resolving, so this fails, restoring it, if a single byte
changed. Builds never repin; a stale lock fails them with crate_universe's
"out of date" message.
"""
import argparse
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
CARGO_LOCK = ROOT / "desktop/aizome/Cargo.lock"


def main():
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        epilog="Other options are Bazel startup options, e.g. --output_user_root=DIR.",
    )
    _, startup = parser.parse_known_args()
    subprocess.run([sys.executable, str(ROOT / "build/bazel/generate.py")], check=True, cwd=ROOT)
    before = CARGO_LOCK.read_bytes()
    env = dict(os.environ, CARGO_BAZEL_REPIN="1")
    try:
        subprocess.run(
            ["bazel", *startup, "query", "@aizome_crates//:all"],
            check=True, cwd=ROOT, env=env, stdout=subprocess.DEVNULL,
        )
    finally:
        if CARGO_LOCK.read_bytes() != before:
            CARGO_LOCK.write_bytes(before)
            sys.exit("crate_universe changed desktop/aizome/Cargo.lock (restored): "
                     "update it with Cargo first, then repin")
    return 0


if __name__ == "__main__":
    sys.exit(main())
