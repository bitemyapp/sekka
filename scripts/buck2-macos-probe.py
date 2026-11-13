#!/usr/bin/env python3
"""Bounded native Mac engine/CAS probe, not the Nix/Aizome build qualification.

Requires explicit Buck2, NativeLink and rustc binaries. Uses a synthetic
19-action Rust graph and a private loopback CAS. Never contacts the Linux cache.
All generated sources, event logs and service state stay under --output.
"""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import statistics
import subprocess
import sys
import time


RULES = '''
RustInfo = provider(fields = ["output", "closure"])
def _rust(ctx):
    name = ctx.attrs.name
    out = ctx.actions.declare_output(name if ctx.attrs.binary else "lib" + name + ".rlib")
    closure = {}
    command = cmd_args(read_root_config("probe", "rustc"), ctx.attrs.src,
        "--crate-name", name, "--edition=2024", "-Copt-level=1", "-Cdebuginfo=0",
        "--remap-path-prefix", ".=.", "-o", out.as_output())
    if not ctx.attrs.binary:
        command.add("--crate-type=rlib")
    for dep in ctx.attrs.deps:
        info = dep[RustInfo]
        closure.update(info.closure)
        closure[dep.label.name] = info.output
        command.add("--extern", cmd_args(dep.label.name, "=", info.output, delimiter = ""))
    for artifact in closure.values():
        command.add("-L", cmd_args(artifact, parent = 1, format = "dependency={}"))
    ctx.actions.run(command, category = "rustc", allow_cache_upload = True)
    return [DefaultInfo(default_output = out), RustInfo(output = out, closure = closure)]
rust = rule(impl = _rust, attrs = {"src": attrs.source(), "deps": attrs.list(attrs.dep(), default = []), "binary": attrs.bool(default = False)})
def _execution(ctx):
    config = CommandExecutorConfig(local_enabled = True, remote_enabled = False,
        remote_cache_enabled = True, allow_cache_uploads = True,
        remote_execution_properties = {"OSFamily": "macos", "ISA": "aarch64", "probe.toolchain": read_root_config("probe", "identity")},
        remote_execution_use_case = "sekka-mac-probe", remote_output_paths = "output_paths")
    return [DefaultInfo(), ExecutionPlatformRegistrationInfo(platforms = [ExecutionPlatformInfo(
        label = ctx.label.raw_target(), configuration = ConfigurationInfo(constraints = {}, values = {}), executor_config = config)])]
execution = rule(impl = _execution, attrs = {})
'''


def run(command, **kwargs):
    return subprocess.run(command, check=True, text=True, capture_output=True, timeout=30, **kwargs)


def tree_bytes(path):
    files = [p for p in path.rglob("*") if p.is_file() and not p.is_symlink()]
    return {"files": len(files), "logical_bytes": sum(p.stat().st_size for p in files),
            "allocated_bytes_sum_not_unique_extents": sum(p.stat().st_blocks * 512 for p in files)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("buck2", "nativelink", "rustc", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--local-cas", action="store_true", help="Exercise native APFS CAS sharing")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("this probe declares a macOS execution platform")
    for name in ("buck2", "nativelink", "rustc"):
        setattr(args, name, getattr(args, name).resolve(strict=True))
    args.output.mkdir(parents=True, exist_ok=False)
    root = args.output.resolve()
    workspace = root / "workspace"
    workspace.mkdir()
    env = dict(os.environ, BUCK2_TEST_SKIP_DEFAULT_EXTERNAL_CONFIG="true")
    for key in ("BUCK2_EXTRA_EXTERNAL_CONFIG", "BUCK2_LOCAL_CONTENT_STORE", "BUCK2_DAEMON_DIR",
                "BUCK2_LOCAL_CONTENT_STORE_MAX_BYTES", "BUCK2_LOCAL_CONTENT_STORE_MAX_ENTRIES",
                "BUCK2_LOCAL_CAS_DIR", "BUCK2_LOCAL_CAS_JOURNAL"):
        env.pop(key, None)
    if args.local_cas:
        env["BUCK2_LOCAL_CAS_DIR"] = str(root / "cas/content")
        env["BUCK2_LOCAL_CAS_JOURNAL"] = str(root / "unused-apfs-journal")
    # Port is private and short-lived; no credentials or production cache.
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    def filesystem(name):
        return {"filesystem": {"content_path": str(root / name / "content"), "temp_path": str(root / name / "tmp"), "eviction_policy": {"max_bytes": 2 * 1024**3}}}
    services = {kind: [{"instance_name": "probe", field: store}] for kind, field, store in
                [("cas", "cas_store", "cas"), ("bytestream", "cas_store", "cas"), ("ac", "ac_store", "ac")]}
    services["capabilities"] = [{"instance_name": "probe", "remote_execution": {"scheduler": "scheduler"}}]
    services["health"] = {}
    config = {"stores": [
        {"name": "cas", "verify": {"verify_size": True, "verify_hash": True, "backend": filesystem("cas")}},
        {"name": "ac", "completeness_checking": {"backend": filesystem("ac"), "cas_store": {"ref_store": {"name": "cas"}}}}],
        "schedulers": [{"name": "scheduler", "simple": {"supported_platform_properties": {"OSFamily": "exact", "ISA": "exact", "probe.toolchain": "exact"}}}],
        "servers": [{"name": "probe", "listener": {"http": {"socket_address": f"127.0.0.1:{port}"}}, "services": services}]}
    config_path = root / "nativelink.json"
    config_path.write_text(json.dumps(config, indent=2))
    versions = {name: run([str(getattr(args, name)), "--version"]).stdout.strip() for name in ("buck2", "nativelink", "rustc")}
    identity = hashlib.sha256(json.dumps(versions, sort_keys=True).encode()).hexdigest()
    (workspace / ".buckconfig").write_text(f'''[cells]
root = .
[build]
execution_platforms = root//:execution
[buck2]
restarter = true
[project]
ignore = buck-out
[probe]
rustc = {args.rustc.resolve()}
identity = {identity}
[buck2_re_client]
engine_address = grpc://127.0.0.1:{port}
action_cache_address = grpc://127.0.0.1:{port}
cas_address = grpc://127.0.0.1:{port}
tls = false
instance_name = probe
''')
    (workspace / "rules.bzl").write_text(RULES)
    targets = ['load(":rules.bzl", "rust", "execution")', 'execution(name="execution")']
    def source(name, text, deps=(), binary=False):
        (workspace / (name + ".rs")).write_text(text)
        targets.append(f'rust(name="{name}", src="{name}.rs", deps={json.dumps([":" + d for d in deps])}, binary={binary})')
    base = '#[inline(never)] pub fn value(x: u64) -> u64 { x.wrapping_add(1) }\n'
    source("base", base)
    source("middle", 'pub fn value(x: u64) -> u64 { base::value(x) }\n', ["base"])
    for i in range(16):
        functions = ''.join(f'#[inline(never)] pub fn f{j}(x:u64)->u64 {{ middle::value(x).wrapping_mul({j+1}) }}\n' for j in range(250))
        source(f"leaf{i}", functions, ["middle"])
    main_text = 'fn main() { println!("{}", ' + '+'.join(f'leaf{i}::f0({i})' for i in range(16)) + '); }\n'
    source("app", main_text, [f"leaf{i}" for i in range(16)], True)
    (workspace / "BUCK").write_text('\n'.join(targets) + '\n')
    results = {"versions": versions, "local_cas": args.local_cas, "kind": "synthetic 19-action Rust DAG; no Cargo incremental state", "runs": [], "storage": []}
    def build(label, expected, cwd=workspace):
        log = root / (label + ".events.json-lines")
        start = time.perf_counter()
        # Request every output: otherwise fresh-root dependencies are lazy and
        # an apparent storage saving merely means they were never downloaded.
        requested = ["app", "base", "middle"] + [f"leaf{i}" for i in range(16)]
        command = [str(args.buck2), "build", *("//:" + name for name in requested), "--show-full-json-output", "--event-log", str(log)]
        completed = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True, timeout=120)
        elapsed = time.perf_counter() - start
        (root / (label + ".stderr")).write_text(completed.stderr)
        if completed.returncode:
            raise RuntimeError(completed.stderr)
        output = Path(next(value for key, value in json.loads(completed.stdout).items() if key.endswith(":app")))
        actual = run([str(output)], cwd=cwd).stdout.strip()
        assert actual == str(expected), (label, actual, expected)
        commands = run([str(args.buck2), "log", "what-ran", str(log), "--format", "json"], cwd=cwd, env=env).stdout
        (root / (label + ".commands.jsonl")).write_text(commands)
        counts = collections.Counter(json.loads(line)["reproducer"]["executor"] for line in commands.splitlines())
        expected_counts = ({"Cache": 19} if label in {"revert", "fresh-workspace"} else
                           {} if label.startswith("no-change") else
                           {"Local": 1} if label.startswith("leaf-edit") else {"Local": 19})
        assert counts == expected_counts, (label, counts, expected_counts)
        entry = {"label": label, "seconds": elapsed, "executors": dict(counts), "output": actual}
        results["runs"].append(entry)
        print(json.dumps(entry), flush=True)
        (root / "summary.json").write_text(json.dumps(results, indent=2) + '\n')
    def storage(label):
        results["storage"].append({"label": label, "cas": tree_bytes(root / "cas/content"),
                                   "artifacts": tree_bytes(workspace / "buck-out/v2/art"),
                                   "buck_out_including_logs": tree_bytes(workspace / "buck-out")})
    log_handle = (root / "nativelink.log").open("w")
    service = subprocess.Popen([str(args.nativelink), str(config_path)], stdout=log_handle, stderr=subprocess.STDOUT)
    workspaces = [workspace]
    try:
        for _ in range(100):
            if service.poll() is not None:
                raise RuntimeError((root / "nativelink.log").read_text())
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=.1):
                    break
            except OSError:
                time.sleep(.1)
        build("cold", 136)
        storage("cold")
        for i in range(5):
            build(f"no-change-{i}", 136)
        storage("no-change")
        for i in range(3):
            (workspace / "app.rs").write_text(main_text.replace('); }', f' + {i+1}); }}'))
            build(f"leaf-edit-{i}", 137+i)
        storage("leaf-edits")
        for i in range(3):
            (workspace / "base.rs").write_text(base.replace('wrapping_add(1)', f'wrapping_add({i+2})'))
            build(f"deep-edit-{i}", 136 + 16*(i+1) + 3)
        storage("deep-edits")
        (workspace / "app.rs").write_text(main_text)
        (workspace / "base.rs").write_text(base)
        build("revert", 136)
        fresh = root / "fresh"
        shutil.copytree(workspace, fresh, ignore=shutil.ignore_patterns("buck-out"))
        workspaces.append(fresh)
        build("fresh-workspace", 136, fresh)
        results["fresh_buck_out"] = tree_bytes(fresh / "buck-out")
        results["fresh_artifacts"] = tree_bytes(fresh / "buck-out/v2/art")
        cas = {path.name.split("-")[0]: path for path in (root / "cas/content").rglob("*") if path.is_file()}
        matched = []
        for path in (fresh / "buck-out/v2/art").rglob("*"):
            if not path.is_file() or path.is_symlink():
                continue
            with path.open("rb") as stream:
                digest = hashlib.file_digest(stream, "sha256").hexdigest()
            if digest in cas:
                matched.append(path.stat().st_size)
        results["fresh_artifacts_matching_cas"] = {"files": len(matched), "logical_bytes": sum(matched),
            "note": "byte identity, not proof of shared physical extents"}
        for group in ("no-change", "leaf-edit", "deep-edit"):
            values = [r["seconds"] for r in results["runs"] if r["label"].startswith(group)]
            results[group] = {"samples": len(values), "median_seconds": statistics.median(values), "min_seconds": min(values), "max_seconds": max(values)}
        (root / "summary.json").write_text(json.dumps(results, indent=2) + '\n')
    finally:
        for cwd in workspaces:
            subprocess.run([str(args.buck2), "kill"], cwd=cwd, env=env, capture_output=True, timeout=30)
        service.terminate()
        service.wait(timeout=15)
        log_handle.close()


if __name__ == "__main__":
    main()
