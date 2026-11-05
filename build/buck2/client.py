#!/usr/bin/env python3
"""Cargo-authoritative target discovery and Buck action execution (no Nix eval)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from generate import root_dir


def stale_inputs(root):
    path = root / "build/buck2/graph.lock.json"
    if not path.exists():
        raise SystemExit("No Buck graph: run sekka-buck2-generate first")
    state = json.loads(path.read_text())
    stale = []
    for relative, expected in state["inputs"].items():
        file = root / relative
        if not file.exists() or hashlib.sha256(file.read_bytes()).hexdigest() != expected:
            stale.append(relative)
    tools = json.loads(Path(os.environ["SEKKA_BUCK2_ENV"]).read_text())
    if state["rustc"] != tools["rustc"] or state["cargo"] != tools["cargo"]:
        stale.append("root-lock compiler/Cargo changed")
    if state["vendor"] != os.environ["SEKKA_BUCK2_VENDOR"]:
        stale.append("Cargo vendor inputs changed")
    if stale:
        raise SystemExit("Cargo/Buck inputs changed; run sekka-buck2-generate:\n" + "\n".join(stale))
    return state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", default="dev", choices=["dev", "release", "release-lto"])
    parser.add_argument("--workspace", default="desktop/aizome/Cargo.toml")
    parser.add_argument("-p", "--package", action="append", default=[])
    parser.add_argument("--jobs", "-j", type=int)
    parser.add_argument("--config-file", action="append", default=[], help="Private cache/RBE client configuration")
    parser.add_argument("--isolation-dir", default="sekka", help="Buck daemon isolation name")
    parser.add_argument("--all-units", action="store_true", help="Discover resolved dependency and build-script units too")
    parser.add_argument("--buck-arg", action="append", default=[], help="Forward one exact engine argument; use --buck-arg=--flag")
    parser.add_argument("command", choices=["build", "test", "targets", "run", "outputs", "check"])
    parser.add_argument("targets", nargs="*")
    args, extra = parser.parse_known_args()
    root = root_dir()
    state = stale_inputs(root)
    if state["manifest"] != args.workspace:
        raise SystemExit("Generate this workspace first: sekka-buck2-generate --manifest-path " + args.workspace)
    targets = json.loads((root / "build/buck2/targets.json").read_text())["test" if args.command == "test" else args.profile]
    if args.all_units or any(t.startswith("//build/buck2/graph/") for t in args.targets):
        targets = json.loads((root / "build/buck2/units.json").read_text())["test" if args.command == "test" else args.profile]
    if args.package:
        targets = [t for t in targets if t["package"] in args.package]
    if args.command == "test":
        targets = [t for t in targets if t["mode"] == "test"]
    if args.targets:
        targets = [t for t in targets if t["label"] in args.targets or t["target"] in args.targets or t.get("unit_label") in args.targets]
    if not targets:
        raise SystemExit("No matching Cargo targets")
    if args.command == "targets":
        print(json.dumps(targets, indent=2))
        return
    if args.command == "check":
        for relative, expected in state["generated"].items():
            path = root / relative
            if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                raise SystemExit("Generated file differs: " + relative + "; run sekka-buck2-generate")
        print("Buck graph inputs and generated files are current")
        return
    if args.command == "run" and len(targets) != 1:
        raise SystemExit("Select exactly one binary target to run")
    command = [os.environ["SEKKA_BUCK2_BIN"], "--isolation-dir", args.isolation_dir, "build" if args.command == "outputs" else args.command, "--config-file", os.environ["SEKKA_BUCK2_CONFIG"]]
    if os.environ.get("SEKKA_BUCK2_RUNTIME"):
        command += ["--config", "sekka.runtime=" + os.environ["SEKKA_BUCK2_RUNTIME"]]
    for config in args.config_file:
        command += ["--config-file", str(Path(config).absolute())]
    if args.jobs:
        command += ["--num-threads", str(args.jobs)]
    command += [target["label"] for target in targets]
    if args.command == "outputs":
        command += ["--show-full-json-output"]
    command += args.buck_arg + extra
    os.chdir(root)
    os.execv(command[0], command)


if __name__ == "__main__":
    main()
