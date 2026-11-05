"""Cargo-authoritative Bazel frontend; invoked by the pinned Nix client."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(prog="sekka-bazel", description=__doc__, allow_abbrev=False)
    parser.add_argument("--workspace", type=Path, help="checkout root (default: nearest enclosing Sekka checkout)")
    parser.add_argument("--profile", choices=("dev", "release", "release-lto"), default="dev")
    parser.add_argument("--cache", choices=("local", "trusted", "ci", "untrusted"), default="local")
    parser.add_argument("--remote-rc", type=Path, help="private rc from scripts/build-service client; required for remote domains")
    parser.add_argument("--output-base", type=Path, help="override this worktree's mutable Bazel state")
    parser.add_argument("--disk-cache", type=Path, help="shared immutable near-cache; never share across trust domains")
    parser.add_argument("command", choices=("generate", "check", "query", "cquery", "aquery", "build", "test", "run", "info", "shutdown", "clean", "fetch"))
    args, rest = parser.parse_known_args()
    root = args.workspace
    if root is None:
        root = next((p for p in [Path.cwd(), *Path.cwd().parents]
                     if (p / "MODULE.bazel").is_file() and (p / "build/bazel/generate.py").is_file()), None)
    if root is None:
        parser.error("run inside a Sekka checkout or pass --workspace")
    root = root.resolve()
    os.chdir(root)
    state = Path(os.environ.get("SEKKA_BAZEL_STATE", Path.home() / ".cache/sekka/bazel"))
    identity = hashlib.sha256(os.fsencode(root)).hexdigest()[:20]
    output = args.output_base or state / "worktrees" / identity / args.cache
    output.mkdir(parents=True, exist_ok=True)
    startup = ["--max_idle_secs=300", f"--output_base={output}", f"--bazelrc={os.environ['SEKKA_BAZEL_REPOSITORIES_RC']}"]
    if args.remote_rc:
        rc = args.remote_rc.resolve()
        if rc.stat().st_mode & 0o077:
            parser.error("--remote-rc must be private (chmod 600); it may contain credentials")
        startup.append(f"--bazelrc={rc}")
    if args.cache != "local" and not args.remote_rc:
        parser.error("remote domains require --remote-rc from scripts/build-service client")
    if args.cache == "local" and args.remote_rc:
        parser.error("select an explicit trust domain with --cache when using --remote-rc")
    generator = [sys.executable, str(root / "build/bazel/generate.py")]
    if args.command == "generate":
        subprocess.run([*generator, *rest], check=True)
        return subprocess.call([sys.executable, str(root / "build/bazel/repin.py"), *startup, "--skip-generate"])
    if args.command not in ("shutdown", "clean"):
        result = subprocess.run([*generator, "--check" if args.command == "check" else "--check-fast"])
        if result.returncode:
            sys.exit("Bazel graph is stale. Run scripts/sekka-bazel generate; Cargo.lock is never updated by that command.")
    if args.command == "check":
        return subprocess.call(["bazel", *startup, "query", "@aizome_crates//:all", "--output=label"], stdout=subprocess.DEVNULL)
    options = []
    if args.command in ("build", "test", "run", "cquery", "aquery"):
        options += [f"--config={args.profile}"]
        # A domain-private near-cache also retains remote hits for reflink
        # materialization. Never share its mutable AC index across trust domains.
        disk = args.disk_cache or state / "cache" / args.cache
        disk.mkdir(parents=True, exist_ok=True)
        options += [f"--disk_cache={disk}", "--remote_download_outputs=toplevel"]
        if args.cache == "local":
            options += ["--remote_cache=", "--remote_executor="]
        else:
            options += ["--remote_accept_cached"]
            if args.cache == "untrusted":
                options.append("--noremote_upload_local_results")
        if not rest or all(x.startswith("-") for x in rest):
            rest += [("//desktop/aizome/crates/aizome-comp:aizome" if args.command == "run"
                      else "//desktop/aizome/..." if args.command == "test"
                      else "//desktop/aizome:workspace")]
    os.execvp("bazel", ["bazel", *startup, args.command, *options, *rest])


if __name__ == "__main__":
    sys.exit(main())
