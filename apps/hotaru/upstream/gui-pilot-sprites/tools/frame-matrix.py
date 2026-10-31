#!/usr/bin/env python3
"""Alternate calibrated application runs and reject observed compiler interference.

This outer process owns the shared workstation lock. Each harness subprocess
uses a distinct private lock, avoiding recursive acquisition of the shared lock.
Fresh homes belong to individual runs; no host cache, clock or setting changes.
"""
import argparse
import contextlib
import ctypes
import datetime
import fcntl
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
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            pid = int(fields[1])
        except (OSError, ValueError):
            return False
    return False


def sample(cpus, harness_pid=None):
    result = {"monotonic_ns": time.monotonic_ns(), "load_average": os.getloadavg(), "cpus": {}, "compilers": []}
    for line in Path("/proc/stat").read_text().splitlines():
        fields = line.split()
        if fields[0].startswith("cpu") and fields[0][3:].isdigit() and int(fields[0][3:]) in cpus:
            # guest/guest_nice are already included in user/nice; do not add twice.
            ticks = list(map(int, fields[1:9]))
            result["cpus"][fields[0][3:]] = {"total": sum(ticks), "idle": ticks[3], "iowait": ticks[4]}
    for cpu in cpus:
        freq = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq")
        with contextlib.suppress(OSError, ValueError):
            result["cpus"][str(cpu)]["frequency_khz"] = int(freq.read_text())
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
    result["meminfo"] = {line.split(":")[0]: int(line.split()[1]) for line in Path("/proc/meminfo").read_text().splitlines()
                         if line.startswith(("MemAvailable:", "SwapFree:"))}
    return result


def utilization(before, after):
    result = {}
    for cpu, old in before["cpus"].items():
        new = after["cpus"].get(cpu)
        if new is None:
            raise ValueError("monitored CPU disappeared")
        total = new["total"] - old["total"]
        if total <= 0:
            raise ValueError("CPU interval has no ticks")
        idle, wait = new["idle"] - old["idle"], new["iowait"] - old["iowait"]
        result[cpu] = {"active_percent": 100 * (total - idle - wait) / total,
                       "iowait_percent": 100 * wait / total}
    return result


def interval_samples(samples, start, end):
    before = [i for i, s in enumerate(samples) if s["monotonic_ns"] <= start]
    after = [i for i, s in enumerate(samples) if s["monotonic_ns"] >= end]
    if not before or not after or end < start:
        raise ValueError("activity samples do not bracket the measured interval")
    selected = samples[before[-1]:after[0] + 1]
    if any(not 0 < b["monotonic_ns"] - a["monotonic_ns"] <= 2_500_000_000 for a, b in zip(selected, selected[1:])):
        raise ValueError("activity monitor gap exceeds 2.5 seconds or is nonmonotonic")
    return selected


def compiler_overlap(samples, start, end):
    return [s for s in interval_samples(samples, start, end)
            if any(p.get("origin", "external") != "harness" for p in s["compilers"])]


def enable_subreaper():
    # Linux process-local setting: orphaned descendants of this harness become
    # our children. Unrelated host processes are never adopted or signalled.
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(36, 1, 0, 0, 0):  # PR_SET_CHILD_SUBREAPER
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER")
    if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
        raise RuntimeError("Linux pidfd support required for owned-child cleanup")


def cleanup_descendants():
    owner = os.getpid()
    end = time.monotonic() + 5
    attempts = []
    while time.monotonic() < end:
        with contextlib.suppress(ChildProcessError):
            while os.waitpid(-1, os.WNOHANG)[0]:
                pass
        children = [int(p.name) for p in Path("/proc").iterdir()
                    if p.name.isdecimal() and int(p.name) != owner and descendant(int(p.name), owner)]
        if not children:
            return attempts
        for pid in children:
            try:
                fd = os.pidfd_open(pid)
            except ProcessLookupError:
                continue
            try:
                # Re-check ownership after opening a stable process handle;
                # pidfd signalling cannot hit a later process reusing its PID.
                if descendant(pid, owner):
                    signal.pidfd_send_signal(fd, signal.SIGKILL)
                    attempts.append(pid)
            except ProcessLookupError:
                pass
            finally:
                os.close(fd)
        time.sleep(.02)
    raise RuntimeError("owned benchmark descendants survived bounded cleanup")


def preflight(cpus):
    before = sample(cpus)
    time.sleep(1)
    after = sample(cpus)
    activity = utilization(before, after)
    reasons = []
    if before["compilers"] or after["compilers"]:
        reasons.append("compiler process observed")
    if any(p["active_percent"] > 10 for p in activity.values()):
        reasons.append("monitored CPU or SMT sibling exceeds 10% activity")
    if any(p["iowait_percent"] > 2 for p in activity.values()):
        reasons.append("monitored CPU or SMT sibling exceeds 2% iowait")
    return {"before": before, "after": after, "activity": activity, "reasons": reasons, "passed": not reasons}


def self_test():
    before = {"cpus": {"8": {"total": 100, "idle": 20, "iowait": 10}}}
    after = {"cpus": {"8": {"total": 200, "idle": 105, "iowait": 15}}}
    assert utilization(before, after)["8"] == {"active_percent": 10.0, "iowait_percent": 5.0}
    rows = [{"monotonic_ns": n, "compilers": [{"pid": 1, "name": "rustc"}]} for n in [1, 2_000_000_000, 4_000_000_000]]
    assert compiler_overlap(rows, 2_000_000_000, 2_100_000_000) == rows[1:]
    assert compiler_overlap(rows, 3_500_000_000, 3_600_000_000) == rows[1:]
    for start, end in [(0, 1), (4_000_000_000, 5_000_000_000)]:
        try:
            interval_samples(rows, start, end)
        except ValueError:
            pass
        else:
            raise AssertionError("missing coverage accepted")
    try:
        interval_samples([rows[0], rows[2]], 2_000_000_000, 3_000_000_000)
    except ValueError:
        pass
    else:
        raise AssertionError("oversized gap accepted")
    own = [{"monotonic_ns": n, "compilers": [{"origin": "harness"}]} for n in [1, 2_000_000_000]]
    assert not compiler_overlap(own, 2, 1_000_000_000)
    assert cpu_list("8-11,14,16") == {8, 9, 10, 11, 14, 16}
    enable_subreaper()
    fixture = "import subprocess; p=subprocess.Popen(['sleep','60'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,start_new_session=True); print(p.pid,flush=True)"
    child = subprocess.Popen([sys.executable, "-c", fixture], stdout=subprocess.PIPE, text=True, start_new_session=True)
    output, _ = child.communicate(timeout=5)
    orphan = int(output.strip())
    assert descendant(orphan, os.getpid())
    assert orphan in cleanup_descendants()
    assert not Path(f"/proc/{orphan}").exists()
    print("passed activity, interval coverage and independent-session orphan cleanup checks")


def run_trial(command, directory, cpus):
    stop = threading.Event()
    samples, errors = [], []
    cleanup = []
    def monitor():
        try:
            with (directory / "activity.jsonl").open("w") as stream:
                while not stop.is_set():
                    value = sample(cpus, process.pid)
                    samples.append(value)
                    stream.write(json.dumps(value) + "\n"); stream.flush()
                    stop.wait(1)
        except Exception as error:
            errors.append(repr(error))
    worker = threading.Thread(target=monitor, daemon=True)
    process = None
    try:
        with (directory / "controller.log").open("wb") as stream:
            process = subprocess.Popen(command, stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
            worker.start()
            code = process.wait(timeout=600)
    finally:
        if process is not None and process.poll() is None:
            # SIGINT lets the harness's finally block stop its private process
            # groups. An unresponsive harness gets a bounded final termination.
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
        cleanup = cleanup_descendants()
        stop.set()
        if worker.ident is not None:
            worker.join(timeout=5)
            if worker.is_alive():
                errors.append("activity monitor did not stop")
        final = sample(cpus, process.pid if process else None)
        samples.append(final)
        with (directory / "activity.jsonl").open("a") as stream:
            stream.write(json.dumps(final) + "\n")
    if cleanup:
        errors.append("harness left live owned descendants requiring cleanup")
    return code, samples, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--calibration", type=Path, required=True, help="qualified calibration directory with summary.json and original runs")
    parser.add_argument("--font-dir", type=Path, required=True)
    parser.add_argument("--fallback-dir", type=Path, required=True)
    parser.add_argument("--mesa", type=Path, required=True)
    parser.add_argument("--phase", choices=("startup", "steady"), required=True)
    parser.add_argument("--rounds", type=int, required=True)
    parser.add_argument("--start-round", type=int, default=0)
    parser.add_argument("--preflight-only", action="store_true")
    for name, default in [("app", "8-11"), ("compositor", "12-13"), ("observer", "14"), ("controller", "15"), ("fixture", "16")]:
        parser.add_argument("--" + name + "-cpus", default=default)
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/hotaru-workstation.lock")
    args = parser.parse_args()
    if not 1 <= args.rounds <= 30 or not 0 <= args.start_round <= 1000:
        parser.error("round range outside bounded batch limits")
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    snapshot = output / "tools"; snapshot.mkdir()
    for name in ("frame-matrix.py", "frame-benchmark.py", "frame-observer.c"):
        (snapshot / name).write_bytes(Path(__file__).with_name(name).read_bytes())
    calibration = json.loads((args.calibration / "summary.json").read_text())
    if calibration["status"] != "qualified_for_future_matched_measurements":
        raise RuntimeError("calibration is not qualified")
    original_calibration = args.calibration
    args.calibration = output / "calibration"
    args.calibration.mkdir()
    (args.calibration / "summary.json").write_bytes((original_calibration / "summary.json").read_bytes())
    for spec in calibration["apps"].values():
        saved = args.calibration / spec["run"]; saved.mkdir()
        (saved / "result.json").write_bytes((original_calibration / spec["run"] / "result.json").read_bytes())
    roles = {name: getattr(args, name + "_cpus") for name in ("app", "compositor", "observer", "controller", "fixture")}
    masks = [cpu_list(v) for v in roles.values()]
    cpus = set.union(*masks)
    if not cpus <= os.sched_getaffinity(0) or sum(map(len, masks)) != len(cpus):
        parser.error("CPU sets must be available and disjoint")
    topology = {}
    for cpu in list(cpus):
        siblings = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list").read_text().strip()
        topology[str(cpu)] = siblings
        cpus.update(cpu_list(siblings))
    report = {"schema": 1, "status": "running", "phase": args.phase, "requested_rounds": args.rounds,
              "start_round": args.start_round, "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "scope": "software private-compositor observations, no hardware or exclusive-host claim",
              "shared_lock": str(args.lock), "private_child_lock": str(output / "child.lock"),
              "calibration_sha256": sha(args.calibration / "summary.json"), "topology": topology,
              "cpu_sets": roles,
              "sources": {p.name: sha(p) for p in snapshot.iterdir()}, "trials": []}
    def save():
        (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    try:
        enable_subreaper()
        with args.lock.open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            for round_id in range(args.start_round, args.start_round + args.rounds):
                for kind in (("hotaru", "ghostty") if round_id % 2 == 0 else ("ghostty", "hotaru")):
                    receipt = {"round": round_id, "kind": kind, "preflight": preflight(cpus)}
                    report["trials"].append(receipt); save()
                    if not receipt["preflight"]["passed"]:
                        report["status"] = "stopped_for_interference"; save(); return 75
                    if args.preflight_only:
                        report["status"] = "preflight_passed_no_app_run"; save(); return 0
                    spec = calibration["apps"][kind]
                    original = json.loads((args.calibration / spec["run"] / "result.json").read_text())
                    if sha(spec["runtime_executable"]) != spec["runtime_executable_sha256"]:
                        raise RuntimeError("calibrated application executable changed")
                    directory = output / f"{round_id:03}-{kind}"; directory.mkdir()
                    command = [sys.executable, str(snapshot / "frame-benchmark.py"), kind, spec["binary"],
                        "--purpose", "measurement", "--output", str(directory / "run"), "--lock", str(output / "child.lock"),
                        "--font-dir", str(args.font_dir.resolve()), "--fallback-dir", str(args.fallback_dir.resolve()),
                        "--mesa", str(args.mesa.resolve()), "--window-width", str(spec["window_pixels"][0]),
                        "--window-height", str(spec["window_pixels"][1]), "--cell-width", "11", "--cell-height", "24",
                        "--keys", "1" if args.phase == "startup" else "500",
                        "--frames", "1" if args.phase == "startup" else "120",
                        "--control-seconds", ".1" if args.phase == "startup" else "30"]
                    if kind == "ghostty":
                        command.extend(["--ghostty-cell-height=-1"])
                    for name, value in roles.items():
                        command.extend(["--" + name + "-cpus", value])
                    receipt["command"] = command; save()
                    code, observations, errors = run_trial(command, directory, cpus)
                    result_path = directory / "run/result.json"
                    result = json.loads(result_path.read_text()) if result_path.exists() else {}
                    reasons = list(errors)
                    if code or result.get("status") != "passed":
                        reasons.append("application harness failed")
                    if result.get("running_executable_sha256") != spec["runtime_executable_sha256"]:
                        reasons.append("runtime executable identity mismatch")
                    if result.get("binary_sha256") != original["binary_sha256"]:
                        reasons.append("application wrapper identity mismatch")
                    if result.get("fonts") != original["fonts"] or result.get("configuration") != original["configuration"]:
                        reasons.append("font or application configuration mismatch")
                    startup = result.get("startup", {})
                    geometry = startup.get("geometry", {})
                    if not startup.get("qualified") or not result.get("physical_geometry_qualified") or (
                            geometry.get("cell_width"), geometry.get("cell_height"), geometry.get("rows")) != (11, 24, 40):
                        reasons.append("startup or final physical geometry unqualified")
                    start = startup.get("launch_ns", 0)
                    end = max([startup.get("matching_frame", {}).get("ready_ns", 0)] +
                              [s["last_capture_ns"] for s in result.get("streams", [])])
                    try:
                        overlapping = compiler_overlap(observations, start, end)
                    except ValueError as error:
                        overlapping = []
                        reasons.append(str(error))
                    if overlapping:
                        reasons.append("compiler activity overlaps measurement interval")
                    receipt.update(returncode=code, rejection_reasons=reasons, accepted=not reasons,
                                   result_sha256=sha(result_path) if result_path.exists() else None,
                                   measured_start_ns=start, measured_end_ns=end,
                                   compiler_overlap_samples=len(overlapping), activity_sha256=sha(directory / "activity.jsonl"))
                    save()
                    if reasons:
                        report["status"] = "stopped_after_rejected_trial"; save(); return 2
            report["status"] = "passed"
    except BaseException as error:
        report.update(status="failed", error=repr(error)); raise
    finally:
        save()
        print(json.dumps({"status": report["status"], "output": str(output), "trials": len(report["trials"])}))


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        raise SystemExit(main())
