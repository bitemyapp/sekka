#!/usr/bin/env python3
"""Real Buck2/NativeLink APFS storage and failure checks on a private disk image.

--output must be a new directory immediately below a disposable image mount.
Only this run's CAS is modified for corruption/loss injection, while stopped.
Run once with --local-cas and once without to compare physical allocation.
"""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import random
import signal
import socket
import subprocess
import sys
import time

RULES = '''
def _payload(ctx):
    out = ctx.actions.declare_output("blob")
    ctx.actions.run(cmd_args(read_root_config("probe", "python"), ctx.attrs.script,
        out.as_output(), ctx.attrs.seed, str(ctx.attrs.mib), ctx.attrs.behavior),
        category = "payload", allow_cache_upload = ctx.attrs.upload)
    return [DefaultInfo(default_output = out)]
payload = rule(impl = _payload, attrs = {"script": attrs.source(), "seed": attrs.source(),
    "mib": attrs.int(default = 64), "upload": attrs.bool(default = True), "behavior": attrs.string(default = "ok")})
def _execution(ctx):
    limit = read_root_config("probe", "limit", "")
    executor = CommandExecutorConfig(local_enabled = True, remote_enabled = False,
        remote_cache_enabled = True, allow_cache_uploads = read_root_config("probe", "writer", "true") == "true",
        max_cache_upload_mebibytes = int(limit) if limit else None,
        remote_execution_properties = {"OSFamily": "macos", "ISA": "aarch64"},
        remote_execution_use_case = "apfs-test", remote_output_paths = "output_paths")
    return [DefaultInfo(), ExecutionPlatformRegistrationInfo(platforms = [ExecutionPlatformInfo(
        label = ctx.label.raw_target(), configuration = ConfigurationInfo(constraints = {}, values = {}), executor_config = executor)])]
execution = rule(impl = _execution, attrs = {})
'''
WRITER = '''import pathlib, random, sys, time
output, seed, mib, behavior = sys.argv[1:]
rng = random.Random(int(pathlib.Path(seed).read_text()))
with open(output, "wb") as stream:
    for _ in range(int(mib)): stream.write(rng.randbytes(1024**2))
if behavior == "fail": sys.exit(1)
if behavior == "wait":
    pathlib.Path(output + ".ready").touch()
    time.sleep(120)
'''


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("buck2", "nativelink", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--local-cas", action="store_true")
    args = parser.parse_args()
    root = args.output.resolve()
    if sys.platform != "darwin" or not root.parent.is_mount() or root.parent == Path("/"):
        parser.error("use a new directory immediately below a disposable APFS image mount")
    args.buck2 = args.buck2.resolve(strict=True)
    args.nativelink = args.nativelink.resolve(strict=True)
    root.mkdir(exist_ok=False)
    results = {"local_cas": args.local_cas, "runs": [], "checks": []}
    env = dict(os.environ, BUCK2_TEST_SKIP_DEFAULT_EXTERNAL_CONFIG="true")
    for key in list(env):
        if key.startswith("BUCK2_LOCAL_") or key in {"BUCK2_EXTRA_EXTERNAL_CONFIG", "BUCK2_DAEMON_DIR"}:
            env.pop(key)
    if args.local_cas:
        env.update(BUCK2_LOCAL_CAS_DIR=str(root / "cas/content"), BUCK2_LOCAL_CAS_JOURNAL=str(root / "unused-journal"))
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    def store(name):
        return {"filesystem": {"content_path": str(root / name / "content"), "temp_path": str(root / name / "tmp"), "eviction_policy": {"max_bytes": 1024**3}}}
    services = {kind: [{"instance_name": "probe", key: name}] for kind, key, name in
                [("cas", "cas_store", "cas"), ("bytestream", "cas_store", "cas"), ("ac", "ac_store", "ac")]}
    services["capabilities"] = [{"instance_name": "probe", "remote_execution": {"scheduler": "scheduler"}}]
    config = {"stores": [{"name": "cas", "verify": {"verify_size": True, "verify_hash": True, "backend": store("cas")}},
                         {"name": "ac", "completeness_checking": {"backend": store("ac"), "cas_store": {"ref_store": {"name": "cas"}}}}],
              "schedulers": [{"name": "scheduler", "simple": {"supported_platform_properties": {"OSFamily": "exact", "ISA": "exact"}}}],
              "servers": [{"name": "probe", "listener": {"http": {"socket_address": f"127.0.0.1:{port}"}}, "services": services}]}
    (root / "service.json").write_text(json.dumps(config))
    service_log = (root / "service.log").open("w")
    service = None
    workspaces = []
    def start():
        nonlocal service
        service = subprocess.Popen([str(args.nativelink), str(root / "service.json")], stdout=service_log, stderr=subprocess.STDOUT)
        for _ in range(100):
            assert service.poll() is None, (root / "service.log").read_text()
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=.1): return
            except OSError: time.sleep(.05)
        raise TimeoutError("NativeLink did not start")
    def stop():
        if service is not None and service.poll() is None:
            service.terminate()
            service.wait(timeout=15)
    def workspace(name):
        path = root / name
        path.mkdir()
        workspaces.append(path)
        (path / "rules.bzl").write_text(RULES)
        (path / "writer.py").write_text(WRITER)
        (path / "seed").write_text("731")
        (path / "other").write_text("732")
        (path / "BUCK").write_text('load(":rules.bzl", "payload", "execution")\nexecution(name="execution")\n'
            'payload(name="data", script="writer.py", seed="seed")\n'
            'payload(name="readonly", script="writer.py", seed="other", mib=2, upload=False)\n'
            'payload(name="limit", script="writer.py", seed="other", mib=2)\n'
            'payload(name="failed", script="writer.py", seed="other", mib=2, behavior="fail")\n'
            'payload(name="cancelled", script="writer.py", seed="other", mib=2, behavior="wait")\n')
        (path / ".buckconfig").write_text(f'''[cells]
root = .
[build]
execution_platforms = root//:execution
[buck2]
restarter = true
[project]
ignore = buck-out
[probe]
python = {sys.executable}
[buck2_re_client]
engine_address = grpc://127.0.0.1:{port}
action_cache_address = grpc://127.0.0.1:{port}
cas_address = grpc://127.0.0.1:{port}
tls = false
instance_name = probe
''')
        return path
    def free():
        os.sync()
        stat = os.statvfs(root)
        return stat.f_bfree * stat.f_frsize
    def inventory():
        return {str(p.relative_to(root / "cas/content")) for p in (root / "cas/content").rglob("*") if p.is_file()}
    def command(path, label, target="data", extra=()):
        return [str(args.buck2), "build", "//:" + target, "--show-full-json-output", "--event-log", str(root / (label + ".json-lines")), *extra]
    def build(path, label, target="data", extra=(), succeeds=True):
        begin = time.monotonic()
        completed = subprocess.run(command(path, label, target, extra), cwd=path, env=env, capture_output=True, text=True, timeout=60)
        (root / (label + ".stderr")).write_text(completed.stderr)
        assert (completed.returncode == 0) == succeeds, completed.stderr
        results["runs"].append({"label": label, "seconds": time.monotonic()-begin, "success": succeeds})
        (root / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
        if succeeds:
            return Path(next(iter(json.loads(completed.stdout).values())))
        return completed.stderr
    try:
        start()
        cold = workspace("cold")
        before = free()
        first = build(cold, "cold")
        results["cold_physical_bytes"] = before - free()
        expected = hashlib.sha256()
        rng = random.Random(731)
        for _ in range(64): expected.update(rng.randbytes(1024**2))
        expected = expected.hexdigest()
        assert digest(first) == expected
        new = workspace("fresh")
        before = free()
        second = build(new, "fresh")
        results["fresh_physical_bytes"] = before - free()
        assert digest(second) == expected
        blobs = list((root / "cas/content").rglob(expected + "-67108864*"))
        assert len(blobs) == 1, blobs
        blob = blobs[0]
        assert len({first.stat().st_ino, second.stat().st_ino, blob.stat().st_ino}) == 3
        before = free()
        with second.open("r+b") as stream: stream.write(b"X" * 1024**2)
        results["one_mib_write_physical_bytes"] = before - free()
        assert digest(first) == digest(blob) == expected
        results["checks"].append("distinct inodes and writable clone isolation")
        if args.local_cas:
            assert results["cold_physical_bytes"] < 80 * 1024**2, results
            assert results["fresh_physical_bytes"] < 8 * 1024**2, results
            assert 1024**2 <= results["one_mib_write_physical_bytes"] < 4 * 1024**2, results
        # Parallel materializers must not scavenge each other's active stages.
        parallel = [workspace("parallel-" + str(i)) for i in range(2)]
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            views = list(pool.map(lambda p: build(p, p.name), parallel))
        assert all(digest(p) == expected for p in views)
        results["checks"].append("concurrent fresh materialization")
        for label, target, extra, succeeds in [
            ("action-readonly", "readonly", (), True),
            ("platform-readonly", "limit", ("--config", "probe.writer=false"), True),
            ("upload-limit", "limit", ("--config", "probe.limit=1"), True),
            ("failed", "failed", (), False),
        ]:
            before = inventory()
            build(workspace(label), label, target, extra, succeeds)
            assert inventory() == before, label + " wrote CAS blobs"
            results["checks"].append(label + " writes no CAS blobs")
        cancelled = workspace("cancelled")
        before = inventory()
        process = subprocess.Popen(command(cancelled, "cancelled", "cancelled"), cwd=cancelled, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 30
            while not list((cancelled / "buck-out").rglob("blob.ready")):
                assert process.poll() is None
                if time.monotonic() > deadline: raise TimeoutError("producer did not start")
                time.sleep(.05)
            process.send_signal(signal.SIGINT)
            stdout, stderr = process.communicate(timeout=30)
            (root / "cancelled.stderr").write_text(stderr)
            assert process.returncode != 0
        finally:
            if process.poll() is None: process.kill(); process.wait()
        assert inventory() == before
        results["checks"].append("cancelled producer writes no CAS blobs")
        # Simulated CAS data loss with the service stopped; its startup index
        # and completeness checking must cause a rebuild, not a stale success.
        stop()
        blob.unlink()
        start()
        recovered = build(workspace("lost-cas"), "lost-cas")
        assert digest(recovered) == digest(first) == expected
        results["checks"].append("CAS loss/restart rebuilds and preserves existing clones")
        stop()
        blob = next((root / "cas/content").rglob(expected + "-67108864*"))
        blob_mode = blob.stat().st_mode & 0o777
        blob.chmod(0o600)  # Fault injection in this stopped, disposable CAS only.
        with blob.open("r+b") as stream:
            saved = stream.read(1); stream.seek(0); stream.write(bytes([saved[0] ^ 255]))
        blob.chmod(blob_mode)
        start()
        message = build(workspace("corrupt"), "corrupt", succeeds=False)
        assert "digest mismatch" in message.lower(), message
        assert digest(first) == expected
        results["checks"].append("same-size corrupt CAS blob rejected before publication")
        stop()
        blob.chmod(0o600)
        with blob.open("r+b") as stream: stream.write(saved)
        blob.chmod(blob_mode)
        start()
        assert digest(build(workspace("repaired"), "repaired")) == expected
        assert not list(root.rglob(".buck2-apfs-*")), "leaked staging directories"
        results["checks"].append("repaired CAS restores correctly; no staging leaks")
        (root / "summary.json").write_text(json.dumps(results, indent=2) + "\n")
        print(json.dumps(results, indent=2))
    finally:
        for path in workspaces:
            subprocess.run([str(args.buck2), "kill"], cwd=path, env=env, capture_output=True, timeout=30)
        stop()
        service_log.close()


if __name__ == "__main__":
    main()
