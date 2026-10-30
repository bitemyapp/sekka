#!/usr/bin/env python3
"""Real headless component exercise, not ISO or hardware qualification.

Run in `nix develop .#aizome`: python3 scripts/test-profile-session.py
--bin target/release --output /absolute/new/result-directory
"""
import argparse
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument("--bin", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
bin_dir = args.bin.resolve()
out = args.output.resolve()
out.mkdir(mode=0o700, parents=True, exist_ok=False)
runtime = out / "run"
runtime.mkdir(mode=0o700)
(out / "inspect").mkdir()
env = dict(os.environ, HOME=str(out), XDG_RUNTIME_DIR=str(runtime),
           XDG_CONFIG_HOME=str(out / "config"), XDG_STATE_HOME=str(out / "state"),
           WAYLAND_DISPLAY="profile-test", AIZOME_SOCKET=str(runtime / "aizome.profile-test.sock"),
           AIZOME_SHELL_SOCKET=str(runtime / "aizome-shell.profile-test.sock"),
           PATH=str(bin_dir) + ":" + os.environ["PATH"], AIZOME_RENDERER="pixman")
for key in ("DISPLAY", "WAYLAND_SOCKET", "DBUS_SESSION_BUS_ADDRESS", "AIZOME_KIT_INSPECT"):
    env.pop(key, None)
children = []
report = {}


def launch(name, *argv, extra=None):
    log = open(out / (name + ".log"), "ab")
    p = subprocess.Popen([str(bin_dir / name), *map(str, argv)], env=env | (extra or {}),
                         stdout=log, stderr=log, start_new_session=True)
    children.append(p)
    return p


def until(label, predicate, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError("timed out: " + label)


def ipc(request):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(3)
        s.connect(env["AIZOME_SOCKET"])
        s.sendall(json.dumps(request).encode() + b"\n")
        return json.loads(s.makefile("rb").readline())


def ctl(*argv):
    return subprocess.check_output([str(bin_dir / "aizomectl"), *map(str, argv)], env=env, text=True)


def window(pid):
    return next((w for w in ipc({"request": "windows"})["windows"] if w["pid"] == pid), None)


def elements():
    result = []
    for path in (out / "inspect").glob("org.aizome.Myaku.*.json"):
        try:
            doc = json.loads(path.read_text())
            result += [e for e in doc["elements"] if e["visible"]]
        except (OSError, json.JSONDecodeError):
            pass
    return result


def click_record():
    button = until("record control", lambda: next((e for e in elements() if e.get("name") == "profile-record" and not e.get("disabled")), None))
    w = window(myaku.pid)
    ctl("focus", w["id"])
    x, y, width, height = button["rect"]
    ctl("input", "motion", w["geometry"]["x"] + x + width / 2, w["geometry"]["y"] + y + height / 2)
    ctl("input", "click", "left")


def interact():
    ctl("focus", window(sumi.pid)["id"])
    for _ in range(4):
        ctl("input", "type", "profiling fixture ")
        ctl("input", "key", "Return")


def captures():
    return sorted((out / "state/aizome/profiles").glob("capture-*/manifest.json"),
                  key=lambda p: int(p.parent.name.rsplit("-", 1)[1]))


def metrics(pid):
    status = Path(f"/proc/{pid}/status").read_text().splitlines()
    smaps = Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines()
    wanted = ("Threads:", "voluntary_ctxt_switches:", "nonvoluntary_ctxt_switches:", "Pss:")
    return {line.split(":")[0]: int(line.split()[1]) for line in status + smaps if line.startswith(wanted)}


try:
    comp = launch("aizome", "--backend", "headless", "--renderer", "pixman", "--headless-output", "1280x800@1",
                  "--socket", "profile-test", "--no-session", "--no-xwayland")
    until("compositor and shell", lambda: Path(env["AIZOME_SOCKET"]).exists() and Path(env["AIZOME_SHELL_SOCKET"]).exists())
    (out / "fixture.txt").write_text("A disposable profiling fixture.\n")
    sumi = launch("aizome-sumi", out / "fixture.txt")
    until("Sumi window", lambda: window(sumi.pid))
    time.sleep(1)
    report["native_before"] = metrics(sumi.pid)
    myaku = launch("aizome-myaku", "--pid", sumi.pid, extra={"AIZOME_KIT_INSPECT": str(out / "inspect")})
    until("Myaku window", lambda: window(myaku.pid))
    time.sleep(1)
    report["myaku_before"] = metrics(myaku.pid)
    click_record()
    until("active capture", lambda: ipc({"request": "profile-state"}).get("session"))
    interact()
    click_record()
    until("Myaku saved capture", lambda: any(e.get("text", "").startswith("Saved capture:") for e in elements()))
    time.sleep(0.2)
    report["myaku_after"] = metrics(myaku.pid)
    assert report["myaku_after"]["Threads"] == report["myaku_before"]["Threads"]
    ctl("screenshot", out / "myaku-profile.png")
    first = json.loads(captures()[0].read_text())
    trace = json.loads((captures()[0].parent / "trace.json").read_text())
    names = {e["name"] for e in trace["traceEvents"]}
    assert first["status"] == "finished", first
    assert sumi.pid in first["native_observed"], first
    assert {"app event dispatch", "view and layout", "paint recording (before raster)",
            "surface commit (correlation only)", "presentation estimate (no hardware timestamp)"} <= names, names
    assert "DRM presentation" not in names
    assert not any("profiling fixture" in json.dumps(e) for e in trace["traceEvents"])
    report["myaku_capture"] = {"manifest": str(captures()[0]), "events": len(trace["traceEvents"]), "phases": sorted(names)}
    report["cycles"] = []
    for i in range(10):
        capture = launch("aizome-profile", "timeline", 10, sumi.pid)
        until("cycle active", lambda: ipc({"request": "profile-state"}).get("session"))
        interact()
        time.sleep(0.5)
        if i == 0:
            logic = bin_dir / "libaizome_comp_logic.so"
            if not logic.exists():
                logic = bin_dir.parent / "lib/aizome/libaizome_comp_logic.so"
            ctl("reload", "--logic", "--from", logic)
        capture.send_signal(signal.SIGINT)
        assert capture.wait(timeout=8) == 0
        assert ipc({"request": "profile-state"})["session"] is None
        report["cycles"].append(json.loads(captures()[-1].read_text())["status"])
    # Default denial is a first-class result on this workstation; a successful
    # permissive host capture is accepted too, without changing kernel policy.
    sample = launch("aizome-profile", "cpu", 10, sumi.pid)
    assert sample.wait(timeout=18) in (0, 1)
    report["cpu"] = json.loads(captures()[-1].read_text())
    click_record()
    until("capture before closing Myaku", lambda: ipc({"request": "profile-state"}).get("session"))
    ctl("close", window(myaku.pid)["id"])
    assert myaku.wait(timeout=8) == 0
    report["myaku_close"] = json.loads(captures()[-1].read_text())["status"]
    assert report["myaku_close"] == "finished"
    myaku = launch("aizome-myaku", "--pid", sumi.pid, extra={"AIZOME_KIT_INSPECT": str(out / "inspect")})
    until("Myaku reopened", lambda: window(myaku.pid))
    # Collector crash: producer errors disable capture; it can be rearmed.
    crashed = launch("aizome-profile", "timeline", 10, sumi.pid)
    until("crash capture active", lambda: ipc({"request": "profile-state"}).get("session"))
    crashed.kill(); crashed.wait(timeout=5)
    until("dead collector disarmed", lambda: ipc({"request": "profile-state"}).get("session") is None)
    # A target exit must finalize promptly instead of following a reused PID.
    exit_capture = launch("aizome-profile", "timeline", 10, sumi.pid)
    until("exit capture active", lambda: ipc({"request": "profile-state"}).get("session"))
    interact()
    report["native_after"] = metrics(sumi.pid)
    sumi.terminate(); sumi.wait(timeout=5)
    assert exit_capture.wait(timeout=8) == 0
    report["target_exit"] = json.loads(captures()[-1].read_text())
    assert sumi.pid in report["target_exit"]["native_observed"], "an idle producer must recover from a dead collector"
    assert "process exited" in report["target_exit"]["stop_reason"]
    assert ipc({"request": "profile-state"})["session"] is None
    lock_capture = launch("aizome-profile", "timeline", 10, myaku.pid)
    until("lock capture active", lambda: ipc({"request": "profile-state"}).get("session"))
    ctl("lock")
    until("headless session locked", lambda: ipc({"request": "profile-state"})["locked"])
    assert lock_capture.wait(timeout=8) == 0
    report["lock"] = json.loads(captures()[-1].read_text())
    assert "locked" in report["lock"]["stop_reason"]
    denied = launch("aizome-profile", "timeline", 10, myaku.pid)
    assert denied.wait(timeout=5) == 1
    report["locked_start_refused"] = True
    report["result"] = "passed"
finally:
    for p in reversed(children):
        if p.poll() is None:
            os.killpg(p.pid, signal.SIGTERM)
            try:
                p.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(p.pid, signal.SIGKILL)
                p.wait()
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
