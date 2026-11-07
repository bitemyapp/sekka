#!/usr/bin/env python3
"""Cargo-authoritative target discovery and Buck action execution (no Nix eval)."""
import argparse
import contextlib
import errno
import fcntl
import functools
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import cas_dedupe
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


def configure_startup(root, config_files):
    """Include private files without copying secrets into project state."""
    paths = [Path(os.environ["SEKKA_BUCK2_CONFIG"]).resolve()]
    paths += [Path(path).resolve() for path in config_files]
    for path in paths:
        if not path.is_file():
            raise SystemExit("Buck configuration file is missing: " + str(path))
        if any(char in str(path) for char in "\n\r<>"):
            raise SystemExit("Buck configuration path cannot be represented as an include")
    content = "".join("<file:" + str(path) + ">\n" for path in paths)
    identity = hashlib.sha256(content.encode()).hexdigest()
    directory = root / ".buckal/client-config"
    directory.mkdir(parents=True, exist_ok=True)
    destination = directory / (identity + ".buckconfig")
    if not destination.exists() or destination.read_text() != content:
        with tempfile.NamedTemporaryFile(mode="w", dir=directory, delete=False) as temporary:
            temporary.write(content)
        try:
            os.replace(temporary.name, destination)
        finally:
            Path(temporary.name).unlink(missing_ok=True)
    os.environ["BUCK2_EXTRA_EXTERNAL_CONFIG"] = str(destination)


def local_cas_dirs(config_files):
    """[sekka] local_cas_dir entries of the private client files: a CAS on this host."""
    found = []
    for path in config_files:
        section = None
        for line in Path(path).read_text().splitlines():
            line = line.strip()
            if line.startswith("["):
                section = line
            elif section == "[sekka]" and line.startswith("local_cas_dir") and "=" in line:
                directory = Path(line.split("=", 1)[1].strip())
                if directory.is_dir():
                    found.append(directory)
    return found


def share_with_cas(content_dirs, store_changed, journal):
    """After a build, share extents between the CAS and this host's other copies.

    Uploaded outputs are listed in the journal; content-store objects that the
    CAS also holds are matched by name. One physical copy remains of each."""
    reports = []
    blobs = functools.cache(lambda: cas_dedupe.scan(content_dirs))  # One scan for both passes.

    def timed(run):
        start = time.monotonic()
        reports.append(dict(run(), seconds=round(time.monotonic() - start, 3)))
    # The store pass moves CAS blobs onto store objects' extents, and the journal
    # pass moves outputs onto CAS blobs' extents; in this order all end on one copy.
    store = os.environ.get("BUCK2_LOCAL_CONTENT_STORE")
    if store and store_changed:
        timed(lambda: cas_dedupe.dedupe(content_dirs, [Path(store)], blobs))
    if journal is not None:
        timed(lambda: cas_dedupe.share_journal(content_dirs, journal, blobs))
    if journal is not None and reports:
        # The last pass, for inspection: nothing in it is secret.
        journal.with_name("local-cas-report.json").write_text(json.dumps(reports, indent=2) + "\n")
    if any(report["cross_filesystem"] for report in reports):
        print("Local CAS is on another filesystem than this worktree; extents not shared.", file=sys.stderr)
        return 0
    problems = [problem for report in reports for problem in report.get("differs", []) + report["errors"]]
    if problems:
        print("Local CAS sharing found mismatched or unreadable files: " + json.dumps(problems[:5]), file=sys.stderr)
        return 1
    return 0


def configure_content_store(root, args):
    """Enable shared reflinks where supported; never substitute copied CAS files."""
    names = ["BUCK2_LOCAL_CONTENT_STORE", "BUCK2_LOCAL_CONTENT_STORE_MAX_BYTES", "BUCK2_LOCAL_CONTENT_STORE_MAX_ENTRIES"]
    selection = args.local_content_store
    if selection is None:
        selection = os.environ.get(names[0], "auto") or "off"
    if selection == "off":
        for name in names:
            os.environ.pop(name, None)
        return
    limits = [
        args.local_content_store_max_bytes if args.local_content_store_max_bytes is not None else int(os.environ.get(names[1], 10 * 1024**3)),
        args.local_content_store_max_entries if args.local_content_store_max_entries is not None else int(os.environ.get(names[2], 100000)),
    ]
    if any(limit <= 0 for limit in limits):
        raise SystemExit("Local content store byte and entry limits must be positive")
    automatic = selection == "auto"
    cache_home = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    if not cache_home.is_absolute():
        cache_home = Path.home() / ".cache"
    directory = (cache_home / "sekka/buck2/content-v1" if automatic else Path(selection).expanduser()).resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    status = directory.stat()
    if status.st_uid != os.getuid() or status.st_mode & 0o777 != 0o700:
        raise SystemExit("Local content store must be owned by this user with mode 0700: " + str(directory))
    outputs = root / "buck-out" / args.isolation_dir
    outputs.mkdir(parents=True, exist_ok=True)
    try:
        with tempfile.TemporaryFile(dir=directory) as source, tempfile.TemporaryFile(dir=outputs) as destination:
            source.write(b"sekka-reflink-probe\n" * 512)
            source.flush()
            fcntl.ioctl(destination.fileno(), 0x40049409, source.fileno())  # Linux FICLONE
    except OSError as error:
        if not automatic or error.errno not in {errno.EXDEV, errno.EOPNOTSUPP, errno.ENOTTY, errno.EINVAL}:
            raise SystemExit("Local content store cannot reflink into buck-out: " + str(error)) from error
        for name in names:
            os.environ.pop(name, None)
        print("Buck local content store disabled: this output/cache filesystem pair does not support reflinks; use --local-content-store=off to select this explicitly.", file=sys.stderr)
        return
    os.environ.update(zip(names, [str(directory), *(str(limit) for limit in limits)]))


def stage_runtime(root, directory, targets, outputs):
    """Launch the declared outputs in place; never duplicate libraries or bins."""
    directory.mkdir(parents=True, exist_ok=True)
    tools = json.loads(Path(os.environ["SEKKA_BUCK2_ENV"]).read_text())
    paths = {"//" + label.split("//", 1)[1]: value for label, value in outputs.items()}
    logic = next((paths[t["label"]] for t in targets if t["target"] == "aizome_comp_logic"), None)
    binaries = {}
    for target in targets:
        if target["kind"] != ["bin"]:
            continue
        output = Path(paths[target["label"]])
        launcher = directory / target["target"]
        arguments = [tools["python"], str(root / "build/buck2/run.py"), "--action-module", str(root / "build/buck2/action.py"), "--environment", os.environ["SEKKA_BUCK2_ENV"], "--runtime", os.environ["SEKKA_BUCK2_RUNTIME"], "--run", str(output.parent), "--"]
        script = "#!" + tools["python"] + "\nimport os, sys\nos.chdir(" + repr(str(root)) + ")\n"
        if logic:
            script += "os.environ.setdefault('AIZOME_LOGIC', " + repr(logic) + ")\n"
        script += "os.execv(" + repr(tools["python"]) + ", " + repr(arguments) + " + sys.argv[1:])\n"
        launcher.write_text(script)
        launcher.chmod(0o755)
        binaries[target["target"]] = str(launcher)
    result = {"profile_outputs": paths, "logic": logic, "binaries": binaries}
    (directory / "runtime.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps(result, indent=2, sort_keys=True))


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
    parser.add_argument("--runtime-dir", default=".work/buck2-runtime", help="Directory of runtime launchers (no copied artifacts)")
    parser.add_argument("--features", default="", help="Cargo feature selection when generating")
    parser.add_argument("--no-default-features", action="store_true")
    parser.add_argument("--cache", choices=["local", "cache", "remote"], default="local")
    parser.add_argument("--cache-write", action="store_true", help="Permit uploads with an authenticated writer config")
    parser.add_argument("--local-content-store", metavar="auto|off|PATH", help="Default: per-user shared reflinks when supported; existing BUCK2_LOCAL_CONTENT_STORE overrides auto")
    parser.add_argument("--local-content-store-max-bytes", type=int, help="Shared store logical byte limit (default 10 GiB)")
    parser.add_argument("--local-content-store-max-entries", type=int, help="Shared store object limit (default 100000)")
    parser.add_argument("command", choices=["build", "test", "targets", "run", "outputs", "check", "generate", "runtime"])
    parser.add_argument("targets", nargs="*")
    argv = sys.argv[1:]
    passthrough = []
    if "--" in argv:
        delimiter = argv.index("--")
        argv, passthrough = argv[:delimiter], argv[delimiter:]
    args, extra = parser.parse_known_args(argv)
    root = root_dir()
    if args.command == "generate":
        command = [sys.executable, str(Path(__file__).with_name("generate.py")), "--manifest-path", args.workspace]
        if args.features:
            command += ["--features", args.features]
        if args.no_default_features:
            command += ["--no-default-features"]
        os.execv(command[0], command + extra + passthrough)
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
    if args.command == "runtime":
        targets = [t for t in targets if t["kind"] == ["bin"] or "dylib" in t["kind"]]
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
    configure_startup(root, args.config_file)
    configure_content_store(root, args)
    content_dirs = local_cas_dirs(args.config_file)
    journal = None
    if content_dirs:
        # The daemon clones cache hits from this CAS instead of downloading them.
        os.environ["BUCK2_LOCAL_CAS_DIR"] = str(content_dirs[0])
        if args.cache_write and args.cache != "local":
            # Uploaded outputs skip the content store; the CAS keeps them, and
            # their extents are shared with it after each build. A local build
            # uploads nothing, so its outputs stay in the content store.
            journal = root / "buck-out" / args.isolation_dir / "local-cas-journal"
            journal.parent.mkdir(parents=True, exist_ok=True)
            os.environ["BUCK2_LOCAL_CAS_JOURNAL"] = str(journal)
    command = [os.environ["SEKKA_BUCK2_BIN"], "--isolation-dir", args.isolation_dir, "build" if args.command in ["outputs", "runtime"] else args.command, "--config-file", os.environ["SEKKA_BUCK2_CONFIG"], "--config", "sekka.execution=" + args.cache, "--config", "sekka.cache_write=" + str(args.cache_write).lower()]
    if os.environ.get("SEKKA_BUCK2_RUNTIME"):
        command += ["--config", "sekka.runtime=" + os.environ["SEKKA_BUCK2_RUNTIME"]]
    for config in args.config_file:
        command += ["--config-file", str(Path(config).absolute())]
    if args.jobs:
        command += ["--num-threads", str(args.jobs)]
    command += [target["label"] for target in targets]
    if args.command in ["outputs", "runtime"]:
        command += ["--show-full-json-output"]
    command += args.buck_arg + extra + passthrough
    os.chdir(root)
    if args.command in ["build", "test", "run", "outputs", "runtime"] and content_dirs:
        # Every command that builds shares extents afterwards. New objects are
        # renamed into the store directory; lookups only write worktree views,
        # so an unchanged directory has nothing to share.
        store = os.environ.get("BUCK2_LOCAL_CONTENT_STORE")
        before = Path(store).stat().st_mtime_ns if store else None
        capture = args.command == "runtime"
        with tempfile.TemporaryFile("w+") if capture else contextlib.nullcontext() as output:
            process = subprocess.Popen(command, stdout=output if capture else None)
            while True:
                try:
                    status = process.wait()
                    break
                except KeyboardInterrupt:
                    continue  # Buck receives the same SIGINT and cancels its own command.
            changed = bool(store) and Path(store).stat().st_mtime_ns != before
            shared = share_with_cas(content_dirs, changed, journal)
            if capture and status == 0:
                output.seek(0)
                stage_runtime(root, (root / args.runtime_dir).resolve(), targets, json.load(output))
        raise SystemExit(status or shared)
    if args.command == "runtime":
        completed = subprocess.run(command, check=True, text=True, stdout=subprocess.PIPE)
        stage_runtime(root, (root / args.runtime_dir).resolve(), targets, json.loads(completed.stdout))
        return
    os.execv(command[0], command)


if __name__ == "__main__":
    main()
