#!/usr/bin/env python3
"""Assemble the exact raster reference without modifying the Ghostty checkout."""
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
PIN = "6ba144158577d033ccb6ed4b574e4013957c9161"

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--zig", type=pathlib.Path, required=True)
    args = parser.parse_args()
    ref = args.reference.resolve(strict=True)
    output = args.output.resolve()
    evidence = json.loads((ROOT / "evidence/provenance.json").read_text())
    pin = subprocess.check_output(["git", "-C", str(ref), "rev-parse", "HEAD"], text=True).strip()
    if pin != PIN or sha(args.zig) != evidence["zig_sha256"]:
        raise SystemExit("reference/toolchain differs from audited identity")
    z2d = ref / "zig-pkg" / evidence["z2d_package_hash"]
    for relative, expected in evidence["unchanged_raster_sources"].items():
        if sha(ref / relative) != expected:
            raise SystemExit(f"modified reference: {relative}")
    for relative, expected in evidence["z2d_sources"].items():
        if sha(z2d / relative) != expected:
            raise SystemExit(f"modified z2d: {relative}")
    output.mkdir(parents=True, exist_ok=False)
    for relative in evidence["unchanged_raster_sources"]:
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ref / relative, target)
    for source, target in [("oracle.zig", "oracle.zig"), ("font-shim.zig", "src/font/main.zig"),
                           ("global-shim.zig", "src/global.zig"), ("quirks-shim.zig", "src/quirks.zig"),
                           ("z2d-options.zig", "z2d-options.zig")]:
        shutil.copyfile(ROOT / "tools" / source, output / target)
    shutil.copyfile(ref / "LICENSE", output / "LICENSE.ghostty")
    command = [str(args.zig.resolve()), "build-lib", "-dynamic", "-lc", "-O", "ReleaseSafe",
               "--dep", "z2d", "-Mroot=oracle.zig", "--dep", "z2d_options",
               "-Mz2d=" + str(z2d / "src/z2d.zig"), "-Mz2d_options=z2d-options.zig",
               "-femit-bin=libsprites.so"]
    (output / "build-command.json").write_text(json.dumps(command, indent=2) + "\n")
    print(json.dumps({"prepared": str(output), "build_command": command,
                      "note": "execute from prepared directory under the workstation lock"}))


if __name__ == "__main__":
    main()
