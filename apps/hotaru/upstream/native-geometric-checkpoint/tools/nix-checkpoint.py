#!/usr/bin/env python3
"""Record a successfully built local Nix package without implying GUI parity."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--nix", default="/nix/var/nix/profiles/default/bin/nix")
    args = parser.parse_args()
    root = args.root.resolve()
    output = (root / "result").resolve(strict=True)

    def run(*command):
        return subprocess.check_output(command, text=True).strip()

    def sha(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    drv = run(args.nix, "path-info", "--derivation", str(output))
    definition = json.loads(run(args.nix, "derivation", "show", drv))
    (root / "derivation.json").write_text(json.dumps(definition, indent=2) + "\n")
    entry = definition.get(drv) or next(iter(definition.get("derivations", definition).values()))
    env = entry["env"]
    source = Path(env["src"])
    clean = dict(os.environ)
    for name in ("LD_LIBRARY_PATH", "AIZOME_FONT_DIR", "HOTARU_FONT_DIR", "HOTARU_RESOURCES_DIR",
                 "HOTARU_TERMINFO_DIR", "TERMINFO", "TERMINFO_DIRS"):
        clean.pop(name, None)
    help_text = subprocess.check_output([str(output / "bin/hotaru"), "--help"], env=clean, text=True)
    licenses = {p.name: sha(p) for p in sorted((output / "share/licenses/hotaru").iterdir())}
    expected = {"Zig-MIT.txt", "Ghostty-MIT.txt", "Unicode-3.0.txt", "uucode-MIT.txt", "Shell-integration-MIT.txt",
                "simdutf8-MIT.txt", "simdutf8-Apache.txt", "z2d-MPL-2.0.txt", "z2d-NOTICES.txt"}
    if set(licenses) != expected:
        raise SystemExit(f"unexpected license inventory: {sorted(licenses)}")
    log = root / "nix-build.log"
    summaries = re.findall(r"test result:.*", log.read_text())
    if not summaries or any("FAILED" in item for item in summaries):
        raise SystemExit("missing or unsuccessful package test summaries")
    report = {
        "schema": 1, "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "host": socket.gethostname(), "status": "passed", "scope": args.scope,
        "derivation": drv, "output": str(output), "source": str(source),
        "source_nar_hash": run(args.nix, "hash", "path", str(source)),
        "binary_sha256": sha(output / "bin/.hotaru-wrapped"),
        "wrapper_sha256": sha(output / "bin/hotaru"), "build_log_sha256": sha(log),
        "test_summaries": summaries, "build_type": env.get("cargoBuildType"),
        "cargo_test_flags": env.get("cargoTestFlags"), "help": help_text,
        "installed_files": sorted(str(p.relative_to(output)) for p in output.rglob("*") if p.is_file()),
        "licenses": licenses,
        "limitations": ["No Aizome VM or physical desktop qualification",
                        "Full Ghostty feature and performance parity remain incomplete",
                        "GUI runtime qualification is recorded separately against this exact package"],
    }
    (root / "package-checkpoint.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: report[key] for key in ("output", "source", "binary_sha256", "wrapper_sha256", "test_summaries")}))


if __name__ == "__main__":
    main()
