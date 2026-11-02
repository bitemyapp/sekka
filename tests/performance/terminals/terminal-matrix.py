#!/usr/bin/env python3
"""Alternate terminals through tools' trials and record what surrounded each.

Extended from apps/hotaru/tools/frame-matrix.py. Each round runs every
terminal once, in an order rotated by the round (A B C D, B C D A, ...).
Before each trial a preflight waits for a quiet machine: the one-minute load
average under --load-max and the measured CPUs (and their SMT siblings) under
10 % busy for a second. Trials that start without one are flagged, not
dropped. An activity monitor samples those CPUs and looks for compilers every
two seconds through each trial.

Wrap the whole matrix in the shared measurement lock
(run.sh does, given LOCKS); the trials take no lock of their own.

  terminal-matrix.py --phase calibrate --output DIR ...    # each terminal's window, cell, warm cache
  terminal-matrix.py --phase startup --calibration DIR/calibration.json --rounds 10 ...
"""
import argparse
import contextlib
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time

COMPILERS = {"rustc", "cargo", "zig", "cc", "cc1", "cc1plus", "gcc", "g++", "clang", "clang++", "ld", "ld.lld", "lld", "ninja"}
TOOLS = ("terminal-bench.py", "terminal-matrix.py", "frame-observer.c", "ppt-sampler.c")


def sha(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def cpu_list(text):
    result = set()
    for part in text.strip().split(","):
        edges = [int(v) for v in part.split("-")]
        result.update(range(edges[0], edges[-1] + 1))
    return result


def descendant(pid, root):
    if root is None:
        return False
    seen = set()
    while pid > 1 and pid not in seen:
        if pid == root:
            return True
        seen.add(pid)
        try:
            pid = int(Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[1])
        except (OSError, ValueError):
            return False
    return False


def sample(cpus, harness_pid=None):
    result = {"monotonic_ns": time.monotonic_ns(), "load_average": os.getloadavg(), "cpus": {}, "compilers": []}
    for line in Path("/proc/stat").read_text().splitlines():
        fields = line.split()
        if fields[0].startswith("cpu") and fields[0][3:].isdigit() and int(fields[0][3:]) in cpus:
            ticks = list(map(int, fields[1:9]))
            result["cpus"][fields[0][3:]] = {"total": sum(ticks), "idle": ticks[3], "iowait": ticks[4]}
    for cpu in cpus:
        with contextlib.suppress(OSError, ValueError):
            result["cpus"][str(cpu)]["frequency_khz"] = int(Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq").read_text())
    for proc in Path("/proc").iterdir():
        if not proc.name.isdecimal():
            continue
        with contextlib.suppress(OSError, ValueError):
            name = (proc / "comm").read_text().strip()
            if name in COMPILERS or name.startswith(("clang-", "gcc-", "g++-")):
                result["compilers"].append({"pid": int(proc.name), "name": name,
                                            "origin": "harness" if descendant(int(proc.name), harness_pid) else "external"})
    result["pressure"] = {}
    for name in ("cpu", "memory", "io"):
        with contextlib.suppress(OSError):
            result["pressure"][name] = Path("/proc/pressure", name).read_text()
    return result


def utilization(before, after):
    result = {}
    for cpu, old in before["cpus"].items():
        new = after["cpus"][cpu]
        total = new["total"] - old["total"]
        if total <= 0:
            raise ValueError("CPU interval has no ticks")
        idle, wait = new["idle"] - old["idle"], new["iowait"] - old["iowait"]
        result[cpu] = {"active_percent": 100 * (total - idle - wait) / total, "iowait_percent": 100 * wait / total}
    return result


def enable_subreaper():
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    if libc.prctl(36, 1, 0, 0, 0):  # PR_SET_CHILD_SUBREAPER
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER")


def cleanup_descendants():
    """Kill whatever the trial left behind: only this process's own descendants."""
    owner = os.getpid()
    end = time.monotonic() + 5
    killed = []
    while time.monotonic() < end:
        with contextlib.suppress(ChildProcessError):
            while os.waitpid(-1, os.WNOHANG)[0]:
                pass
        children = [int(p.name) for p in Path("/proc").iterdir()
                    if p.name.isdecimal() and int(p.name) != owner and descendant(int(p.name), owner)]
        if not children:
            return killed
        for pid in children:
            try:
                fd = os.pidfd_open(pid)
            except ProcessLookupError:
                continue
            try:
                if descendant(pid, owner):
                    signal.pidfd_send_signal(fd, signal.SIGKILL)
                    killed.append(pid)
            except ProcessLookupError:
                pass
            finally:
                os.close(fd)
        time.sleep(.02)
    raise RuntimeError("owned benchmark descendants survived bounded cleanup")


def preflight(cpus, wait, load_max):
    """Wait (up to `wait` seconds) for a one-minute load under `load_max` and a
    quiet second on the measured CPUs."""
    deadline = time.monotonic() + wait
    attempts = 0
    while True:
        attempts += 1
        before = sample(cpus)
        time.sleep(1)
        after = sample(cpus)
        activity = utilization(before, after)
        reasons = []
        if [c for c in before["compilers"] + after["compilers"]]:
            reasons.append("compiler process observed")
        if any(p["active_percent"] > 10 for p in activity.values()):
            reasons.append("measured CPU or SMT sibling over 10% busy")
        if after["load_average"][0] >= load_max:
            reasons.append(f"one-minute load {after['load_average'][0]:.2f} >= {load_max}")
        if not reasons or time.monotonic() >= deadline:
            return {"activity": activity, "load_average": after["load_average"], "compilers": after["compilers"],
                    "reasons": reasons, "passed": not reasons, "attempts": attempts}
        time.sleep(4)


def run_trial(command, directory, cpus, timeout):
    stop = threading.Event()
    samples, errors = [], []
    process = None

    def monitor():
        try:
            with (directory / "activity.jsonl").open("w") as stream:
                while not stop.is_set():
                    value = sample(cpus, process.pid)
                    samples.append(value)
                    stream.write(json.dumps(value) + "\n"); stream.flush()
                    stop.wait(2)
        except Exception as error:
            errors.append(repr(error))

    worker = threading.Thread(target=monitor, daemon=True)
    code = None
    try:
        with (directory / "controller.log").open("wb") as stream:
            process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
            worker.start()
            code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        errors.append("trial timed out")
    finally:
        if process is not None and process.poll() is None:
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
        killed = cleanup_descendants()
        stop.set()
        if worker.ident is not None:
            worker.join(timeout=5)
    if killed:
        errors.append(f"cleanup killed {len(killed)} leftover processes")
    compilers = sorted({c["name"] for s in samples for c in s["compilers"] if c["origin"] != "harness"})
    busiest = max((u["active_percent"] for a, b in zip(samples, samples[1:]) for u in utilization(a, b).values()), default=None)
    return code, errors, {"samples": len(samples), "external_compilers": compilers,
                          "load_average_max": max((s["load_average"][0] for s in samples), default=None),
                          "busiest_measured_cpu_percent": busiest}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--phase", choices=("calibrate", "startup", "steady", "throughput", "energy", "idle-default"), required=True)
    parser.add_argument("--apps", default="hotaru,ghostty,gnome-terminal,konsole,kgx,foot")
    parser.add_argument("--binary", action="append", default=[], metavar="APP=PATH")
    parser.add_argument("--calibration", type=Path, help="calibration.json from a calibrate phase")
    parser.add_argument("--observer", type=Path, required=True)
    parser.add_argument("--font-dir", type=Path, required=True); parser.add_argument("--fallback-dir", type=Path, required=True)
    parser.add_argument("--fontconfig-cache", type=Path, required=True)
    parser.add_argument("--corpus", action="append", default=[], type=Path)
    parser.add_argument("--throughput-repeat", type=int, default=8)
    parser.add_argument("--keys", type=int, default=500); parser.add_argument("--frames", type=int, default=120)
    parser.add_argument("--idle-seconds", type=float, default=60); parser.add_argument("--typing-seconds", type=float, default=60)
    parser.add_argument("--baseline-seconds", type=float, default=20); parser.add_argument("--baseline-post-seconds", type=float, default=10)
    parser.add_argument("--rounds", type=int, default=1); parser.add_argument("--start-round", type=int, default=0)
    parser.add_argument("--quiet-wait", type=float, default=120); parser.add_argument("--load-max", type=float, default=1.5)
    parser.add_argument("--retries", type=int, default=1)
    parser.add_argument("--trial-timeout", type=float, default=1200)
    parser.add_argument("--initial-window", default="1349x1077")
    for name, default in [("app", "2-5"), ("compositor", "6,14"), ("observer", "7"), ("controller", "15"), ("fixture", "1")]:
        parser.add_argument("--" + name + "-cpus", default=default)
    args = parser.parse_args()
    apps = args.apps.split(",")
    binaries = dict(item.split("=", 1) for item in args.binary)
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    snapshot = output / "tools"; snapshot.mkdir()
    here = Path(__file__).resolve().parent
    for name in TOOLS:
        (snapshot / name).write_bytes((here / name).read_bytes())
    roles = {name: getattr(args, name + "_cpus") for name in ("app", "compositor", "observer", "controller", "fixture")}
    cpus = set().union(*(cpu_list(v) for v in roles.values()))
    for cpu in list(cpus):
        cpus.update(cpu_list(Path(f"/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list").read_text()))
    calibration = json.loads(args.calibration.read_text()) if args.calibration else None
    report = {"schema": 2, "status": "running", "phase": args.phase, "apps": apps, "rounds": args.rounds,
              "start_round": args.start_round, "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "host": os.uname().nodename, "kernel": os.uname().release, "cpu_sets": roles, "monitored_cpus": sorted(cpus),
              "sources": {name: sha(snapshot / name) for name in TOOLS}, "observer_build": json.loads((args.observer / "build.json").read_text()),
              "calibration": calibration, "arguments": {k: str(v) for k, v in vars(args).items()}, "trials": []}

    def save():
        (output / "result.json").write_text(json.dumps(report, indent=1) + "\n")

    def common(app):
        command = [sys.executable, str(snapshot / "terminal-bench.py"), app]
        if calibration:
            spec = calibration["apps"][app]
            command += [spec["binary"], "--window-width", str(spec["window"][0]), "--window-height", str(spec["window"][1]),
                        "--cell-width", str(spec["cell"][0]), "--cell-height", str(spec["cell"][1])]
            if spec.get("cache_template"):
                command += ["--cache-template", spec["cache_template"]]
        else:
            width, height = args.initial_window.split("x")
            command += [binaries[app], "--window-width", width, "--window-height", height]
        command += ["--runtime-dir", f"/tmp/tb.{app}",
                    "--observer", str(args.observer), "--font-dir", str(args.font_dir), "--fallback-dir", str(args.fallback_dir),
                    "--fontconfig-cache", str(args.fontconfig_cache)]
        for name, value in roles.items():
            command += ["--" + name + "-cpus", value]
        return command

    try:
        enable_subreaper()
        if args.phase == "calibrate":
            # Twice each: the first finds the window and warms the caches,
            # the second starts warm at that window and is the calibration.
            result = {"apps": {}, "host": os.uname().nodename}
            for app in apps:
                cache = output / "cache" / app
                spec = None
                for attempt in range(2):
                    directory = output / f"{app}-{attempt}"
                    directory.mkdir()
                    command = common(app)
                    if spec:
                        command[command.index("--window-width") + 1] = str(spec["window_pixels"][0])
                        command[command.index("--window-height") + 1] = str(spec["window_pixels"][1])
                        command += ["--cache-template", str(cache)]
                    command += ["--phase", "calibrate", "--output", str(directory / "run"), "--save-cache", str(cache) + ".new"]
                    cache.parent.mkdir(exist_ok=True)
                    code, errors, activity = run_trial(command, directory, cpus, args.trial_timeout)
                    path = directory / "run/result.json"
                    run = json.loads(path.read_text()) if path.exists() else {}
                    report["trials"].append({"app": app, "attempt": attempt, "returncode": code, "errors": errors, "status": run.get("status"),
                                             "error": run.get("error"), "activity": activity})
                    save()
                    if run.get("status") != "passed":
                        break
                    spec = run
                    if Path(str(cache) + ".new").exists():
                        subprocess.run(["rm", "-rf", str(cache)], check=True)
                        Path(str(cache) + ".new").rename(cache)
                if spec and spec.get("status") == "passed":
                    result["apps"][app] = {"binary": binaries[app], "window": spec["window_pixels"],
                                           "cell": [spec["geometry"]["cell_width"], spec["geometry"]["cell_height"]],
                                           "grid": [spec["grid"]["cols"], spec["grid"]["rows"]], "window_rect": spec.get("window_rect"),
                                           "cache_template": str(cache), "terminal_processes": spec.get("terminal_processes"),
                                           "terminal_executables_sha256": spec.get("terminal_executables_sha256"),
                                           "gpu": spec.get("gpu"), "configuration": spec.get("configuration"),
                                           "dconf_keyfile": spec.get("dconf_keyfile"), "fonts": spec.get("fonts"),
                                           "cache_files": sorted(str(p.relative_to(cache)) for p in cache.rglob("*") if p.is_file())}
            (output / "calibration.json").write_text(json.dumps(result, indent=1) + "\n")
            report["status"] = "passed" if set(result["apps"]) == set(apps) else "incomplete"
            return 0 if report["status"] == "passed" else 1
        for round_id in range(args.start_round, args.start_round + args.rounds):
            shift = round_id % len(apps)
            order = apps[shift:] + apps[:shift]
            for app in order:
                if calibration and app not in calibration["apps"]:
                    report["trials"].append({"round": round_id, "app": app, "attempt": 0, "accepted": False,
                                             "rejection_reasons": ["no calibration for this terminal"]})
                    save()
                    continue
                for attempt in range(1 + args.retries):
                    receipt = {"round": round_id, "app": app, "attempt": attempt,
                               "preflight": preflight(cpus, args.quiet_wait, args.load_max)}
                    report["trials"].append(receipt); save()
                    directory = output / f"{round_id:03}-{app}" / f"try{attempt}"
                    directory.mkdir(parents=True)
                    command = common(app) + ["--phase", args.phase, "--purpose", "measurement", "--output", str(directory / "run"),
                                             "--keys", str(args.keys), "--frames", str(args.frames),
                                             "--idle-seconds", str(args.idle_seconds), "--typing-seconds", str(args.typing_seconds),
                                             "--baseline-seconds", str(args.baseline_seconds),
                                             "--baseline-post-seconds", str(args.baseline_post_seconds)]
                    if args.phase == "throughput":
                        command += ["--throughput-repeat", str(args.throughput_repeat)]
                        for corpus in args.corpus:
                            command += ["--throughput", str(corpus.resolve())]
                    receipt["command"] = command; save()
                    code, errors, activity = run_trial(command, directory, cpus, args.trial_timeout)
                    path = directory / "run/result.json"
                    result = json.loads(path.read_text()) if path.exists() else {}
                    reasons = list(errors)
                    if code or result.get("status") != "passed":
                        reasons.append(f"harness failed: {result.get('error')}")
                    if calibration and args.phase != "idle-default":
                        spec = calibration["apps"][app]
                        if result.get("terminal_executables_sha256") and spec.get("terminal_executables_sha256") and \
                                result["terminal_executables_sha256"] != spec["terminal_executables_sha256"]:
                            reasons.append("terminal executable identity differs from the calibration")
                        if result.get("grid") and (result["grid"]["cols"], result["grid"]["rows"]) != (120, 40):
                            reasons.append("grid is not 120x40")
                    if activity["external_compilers"]:
                        reasons.append("external compiler observed during the trial")
                    receipt.update(returncode=code, rejection_reasons=reasons, accepted=not reasons,
                                   quiet_start=receipt["preflight"]["passed"], activity=activity,
                                   result_sha256=sha(path) if path.exists() else None)
                    save()
                    if not reasons:
                        break
        report["status"] = "passed"
    except BaseException as error:
        report.update(status="failed", error=repr(error)); raise
    finally:
        report["finished_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        save()
        print(json.dumps({"status": report["status"], "output": str(output), "trials": len(report["trials"]),
                          "accepted": sum(1 for t in report["trials"] if t.get("accepted"))}))


if __name__ == "__main__":
    raise SystemExit(main())
