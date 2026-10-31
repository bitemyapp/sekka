#!/usr/bin/env python3
"""Qualify offscreen DMA-BUF exchange using host and explicitly supplied Nix EGL.

This does not start a compositor or change host graphics/driver configuration.
It creates only an isolated ICD file and symlinks to explicitly named drivers.
The Nix probe is built with host GCC, the specified Nix loader/libc and GLVND;
it is a loader compatibility diagnostic, not an application/package benchmark.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--loader", type=Path, required=True)
    parser.add_argument("--glvnd", type=Path, required=True, help="Nix lib directory")
    parser.add_argument("--drm", type=Path, required=True, help="Nix libdrm lib directory")
    parser.add_argument("--driver-library", type=Path, action="append", required=True)
    parser.add_argument("--trace-calls", action="store_true", help="record this probe's openat/ioctl calls with strace")
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).with_name("gpu-buffer-probe.c").resolve(strict=True)
    (output / source.name).write_bytes(source.read_bytes())
    (output / Path(__file__).name).write_bytes(Path(__file__).read_bytes())
    source = output / source.name
    runner = output / Path(__file__).name
    loader = args.loader.resolve(strict=True)
    glvnd = args.glvnd.resolve(strict=True)
    drm = args.drm.resolve(strict=True)
    libc = loader.parent
    drivers = output / "driver-libraries"
    drivers.mkdir()
    libraries = {}
    for provided in args.driver_library:
        if not provided.name.startswith(("libEGL_nvidia.so.", "libnvidia-", "libGLESv2_nvidia.so.")):
            parser.error("only explicitly named NVIDIA driver libraries belong in the isolated directory")
        actual = provided.resolve(strict=True)
        for name in {provided.name, actual.name}:
            target = drivers / name
            if target.exists() and target.resolve() != actual:
                parser.error("conflicting driver library names")
            if not target.exists():
                target.symlink_to(actual)
        libraries[str(actual)] = digest(actual)
    vendor = drivers / "libEGL_nvidia.so.0"
    if not vendor.is_file():
        parser.error("supply the libEGL_nvidia.so.0 driver entry point")
    icd = output / "nvidia.json"
    icd.write_text(json.dumps({"file_format_version": "1.0.0", "ICD": {"library_path": str(vendor)}}) + "\n")
    for directory in ("home", "cache", "external-platforms"):
        (output / directory).mkdir()
    env = dict(os.environ)
    for name in list(env):
        if name.startswith(("LD_", "LIBGL_", "__EGL_", "__GL_", "EGL_", "GBM_", "GALLIUM_", "MESA_")):
            env.pop(name, None)
    for name in ("DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"):
        env.pop(name, None)
    env.update(HOME=str(output / "home"), XDG_CACHE_HOME=str(output / "cache"),
               __EGL_VENDOR_LIBRARY_FILENAMES=str(icd),
               __EGL_EXTERNAL_PLATFORM_CONFIG_DIRS=str(output / "external-platforms"))
    report = {"schema": 1, "scope": "offscreen two-context DMA-BUF correctness and loader compatibility only",
              "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "source_sha256": digest(source), "runner_sha256": digest(runner), "driver_files": libraries,
              "nix_loader": str(loader), "nix_loader_sha256": digest(loader), "nix_glvnd": str(glvnd), "nix_drm": str(drm),
              "headers": {str(p): digest(p) for p in map(Path, ["/usr/include/EGL/egl.h", "/usr/include/EGL/eglext.h",
                  "/usr/include/EGL/eglplatform.h", "/usr/include/GLES3/gl3.h", "/usr/include/GLES2/gl2ext.h",
                  "/usr/include/KHR/khrplatform.h"])},
              "load_average": os.getloadavg(), "runs": {}}
    compiler = subprocess.run(["/usr/bin/gcc", "--version"], capture_output=True, text=True, check=True)
    report["compiler"] = compiler.stdout
    with args.lock.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        for name in ("host", "nix-loader"):
            binary = output / name
            command = ["/usr/bin/gcc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", str(source), "-o", str(binary)]
            if name == "nix-loader":
                command.extend(["-L" + str(glvnd), "-L" + str(libc),
                                "-Wl,--dynamic-linker," + str(loader),
                                "-Wl,-rpath," + str(glvnd) + ":" + str(libc)])
            command.append("-lEGL")
            build = subprocess.run(command, capture_output=True, text=True, env=env, timeout=60)
            (output / (name + "-build.log")).write_text(build.stdout + build.stderr)
            entry = {"build_command": command, "build_returncode": build.returncode}
            report["runs"][name] = entry
            if build.returncode:
                continue
            run_env = dict(env)
            if name == "nix-loader":
                run_env["LD_LIBRARY_PATH"] = ":".join(map(str, (glvnd, libc, drm, drivers)))
            entry.update(binary_sha256=digest(binary),
                         environment={k: run_env[k] for k in ("HOME", "XDG_CACHE_HOME", "__EGL_VENDOR_LIBRARY_FILENAMES", "__EGL_EXTERNAL_PLATFORM_CONFIG_DIRS")},
                         library_path=run_env.get("LD_LIBRARY_PATH"))
            run_command = [str(binary), "0"]
            if args.trace_calls:
                run_command = ["strace", "-f", "-e", "trace=openat,ioctl", "-o", str(output / (name + ".strace")), *run_command]
            entry["run_command"] = run_command
            # Keep the Nix library path scoped to the child binary, never strace.
            if args.trace_calls and "LD_LIBRARY_PATH" in run_env:
                run_command = [*run_command[:-2], "env", "LD_LIBRARY_PATH=" + run_env.pop("LD_LIBRARY_PATH"), *run_command[-2:]]
                entry["run_command"] = run_command
            process = subprocess.Popen(run_command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       text=True, env=run_env, start_new_session=True)
            try:
                stdout, stderr = process.communicate(timeout=30)
                run = subprocess.CompletedProcess(run_command, process.returncode, stdout, stderr)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                stdout, stderr = process.communicate(timeout=5)
                (output / (name + ".jsonl")).write_text(stdout)
                (output / (name + ".stderr")).write_text(stderr)
                entry.update(timeout_seconds=30, returncode=None)
                continue
            (output / (name + ".jsonl")).write_text(run.stdout)
            (output / (name + ".stderr")).write_text(run.stderr)
            events = [json.loads(line) for line in run.stdout.splitlines()]
            mappings = sorted({e["path"] for e in events if e["event"] == "mapping"})
            entry.update(returncode=run.returncode,
                         events=[e for e in events if e["event"] != "mapping"],
                         mapped_files={p: digest(p) for p in mappings if Path(p).is_file()})
            if name == "nix-loader":
                allowed = {str(binary), *libraries}
                outside = [p for p in entry["mapped_files"] if p not in allowed and
                           not any(Path(p).is_relative_to(root) for root in (glvnd, libc, drm))]
                entry["unexpected_mapped_files"] = outside
                entry["non_file_mappings"] = [p for p in mappings if p not in entry["mapped_files"]]
                entry["loader_scope_qualified"] = not outside and str(loader) in mappings
    report["driver_files_unchanged"] = all(digest(p) == sha for p, sha in libraries.items())
    report["status"] = "passed" if report["driver_files_unchanged"] and all(
        r.get("returncode") == 0 and any(e["event"] == "passed" for e in r.get("events", [])) and
        r.get("loader_scope_qualified", True) for r in report["runs"].values()) else "failed"
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"status": report["status"], "output": str(output),
                      "runs": {n: {k: r[k] for k in ("build_returncode", "returncode", "unexpected_mapped_files") if k in r}
                               for n, r in report["runs"].items()}}))
    raise SystemExit(0 if report["status"] == "passed" else 1)


if __name__ == "__main__":
    main()
