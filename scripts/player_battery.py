#!/usr/bin/env python3
"""Measure whole-laptop battery discharge during local fullscreen playback on Aizome.

Requires Python 3.11+, ffprobe, busctl, wpctl, and the installed players. No root,
power-setting changes, network playback, or background-application termination.
See docs/test-results/player-battery-2026-11-02.md for protocol and limitations.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import socket
import statistics
import subprocess
import tempfile
import time


MPRIS = "/org/mpris/MediaPlayer2"
PLAYER = "org.mpris.MediaPlayer2.Player"


def command(*args):
    return subprocess.check_output(args, text=True, stderr=subprocess.PIPE, timeout=15).strip()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def ipc(request, **fields):
    path = os.environ.get("AIZOME_SOCKET") or (
        f"{os.environ['XDG_RUNTIME_DIR']}/aizome.{os.environ['WAYLAND_DISPLAY']}.sock"
    )
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(5)
        sock.connect(path)
        sock.sendall(json.dumps(dict(request=request, **fields)).encode() + b"\n")
        result = json.loads(sock.makefile("rb").readline())
    if result.get("response") == "error":
        raise RuntimeError(result)
    return result


def read(path):
    return Path(path).read_text().strip()


def battery(root):
    values = dict(line.split("=", 1) for line in (root / "uevent").read_text().splitlines())
    def num(key):
        return int(values["POWER_SUPPLY_" + key])
    supplies = {}
    for p in root.parent.iterdir():
        if (p / "online").exists():
            supplies[p.name] = int(read(p / "online"))
    return dict(energy_wh=num("ENERGY_NOW") / 1e6, full_wh=num("ENERGY_FULL") / 1e6,
                power_w=num("POWER_NOW") / 1e6, capacity=num("CAPACITY"),
                status=values["POWER_SUPPLY_STATUS"], online=supplies)


def brightness():
    return {p.name: {k: int(read(p / k)) for k in ("brightness", "actual_brightness", "max_brightness")}
            for p in Path("/sys/class/backlight").iterdir()}


def temperatures():
    result = {}
    for p in Path("/sys/class/thermal").glob("thermal_zone*"):
        try:
            result[read(p / "type")] = int(read(p / "temp")) / 1000
        except (OSError, ValueError):
            pass
    return result


def processes():
    """CPU ticks by PID/start time; names only, never command lines or window titles."""
    result = {}
    for p in Path("/proc").iterdir():
        if not p.name.isdigit():
            continue
        try:
            value = read(p / "stat")
            rest = value[value.rfind(")") + 2:].split()
            result[(p.name, rest[19])] = (value[value.find("(") + 1:value.rfind(")")],
                                         int(rest[11]) + int(rest[12]))
        except (OSError, ValueError, IndexError):
            pass
    return result


def cpu_delta(previous, current, seconds):
    totals = {}
    for key, (name, ticks) in current.items():
        if key in previous:
            delta = max(0, ticks - previous[key][1]) / os.sysconf("SC_CLK_TCK") / seconds * 100
            totals[name] = totals.get(name, 0) + delta
    return dict(sorted(totals.items(), key=lambda x: -x[1])[:12])


def display_state():
    # Exclude monitor serials and enumerated unused modes from published evidence.
    return [{k: p[k] for k in ("name", "make", "model", "enabled", "powered", "current_mode", "scale")}
            for p in ipc("outputs")["outputs"]]


def ensure_environment(sample, initial, minimum):
    b = sample["battery"]
    if b["status"] != "Discharging" or any(b["online"].values()):
        raise RuntimeError("External power or non-discharging battery; run invalid")
    if b["capacity"] <= minimum:
        raise RuntimeError("Battery reserve reached; stopping")
    if sample["brightness"] != initial:
        raise RuntimeError("Brightness changed; run invalid")


def estimate(samples):
    """Time-weighted power, independent energy endpoints and least-squares slope."""
    if len(samples) < 2:
        raise ValueError("Need at least two samples")
    t = [s["elapsed_s"] for s in samples]
    e = [s["battery"]["energy_wh"] for s in samples]
    p = [s["battery"]["power_w"] for s in samples]
    dt = t[-1] - t[0]
    if dt <= 0 or any(b <= a for a, b in zip(t, t[1:])):
        raise ValueError("Time must increase")
    mean_w = sum((a + b) / 2 * (y - x) for a, b, x, y in zip(p, p[1:], t, t[1:])) / dt
    endpoint_w = (e[0] - e[-1]) * 3600 / dt
    slope_w = -statistics.linear_regression(t, e).slope * 3600
    if min(mean_w, endpoint_w, slope_w) <= 0:
        raise ValueError("No measurable positive discharge")
    full = statistics.mean(s["battery"]["full_wh"] for s in samples)
    return dict(seconds=dt, samples=len(samples), mean_power_w=mean_w,
                endpoint_power_w=endpoint_w, slope_power_w=slope_w,
                energy_used_wh=e[0] - e[-1], full_wh=full,
                projected_full_minutes=60 * full / mean_w,
                start_energy_wh=e[0], end_energy_wh=e[-1],
                projected_run_start_minutes=60 * e[0] / mean_w,
                power_energy_disagreement_pct=100 * abs(endpoint_w - mean_w) / mean_w)


def schedule(players, starts, sequence=None):
    """Explicit order allows mirrored/ABBA runs without reusing evidence directories."""
    if sequence:
        if len(starts) != 1:
            raise ValueError("An explicit sequence requires exactly one seek point")
        return [dict(run=i + 1, player=name, start_s=starts[0]) for i, name in enumerate(sequence)]
    return [dict(run=i + 1, player=name, start_s=start)
            for i, start in enumerate(starts)
            for name in players[i % len(players):] + players[:i % len(players)]]


class Playback:
    def __init__(self, name, media, directory, temporary):
        self.name, self.directory = name, directory
        self.mpv_socket = str(temporary / "mpv.sock")
        env = os.environ.copy()
        env["XDG_STATE_HOME"] = str(temporary / "state")
        # Preserve user configuration/installed decoding defaults, isolate resume/history.
        if name == "utsushi":
            argv = ["aizome-utsushi", str(media)]
            env["UTSUSHI_STATS"] = "1"
        elif name == "vlc":
            argv = ["vlc", "--no-one-instance", "--fullscreen", "--no-video-title-show",
                    "--no-qt-privacy-ask", "--no-qt-recentplay", "--extraintf=dbus", str(media)]
        else:
            argv = ["mpv", "--fullscreen", "--no-save-position-on-quit", "--volume=100",
                    "--input-ipc-server=" + self.mpv_socket, str(media)]
            if name == "mpv-auto":
                argv.insert(1, "--hwdec=auto")
        self.argv = argv
        self.existing_windows = {w["id"] for w in ipc("windows")["windows"]}
        self.log = (directory / "player.log").open("w")
        self.process = subprocess.Popen(argv, stdout=self.log, stderr=self.log, env=env, start_new_session=True)
        self.window = None
        self.service = None

    def wait_ready(self):
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"{self.name} exited: see {self.directory}/player.log")
            # Xwayland windows can report the Xwayland server's PID through Aizome.
            # Refuse pre-existing player windows and identify only new app windows.
            app_ids = {"utsushi": "org.aizome.Utsushi", "vlc": "vlc", "mpv": "mpv", "mpv-auto": "mpv"}
            candidates = [w for w in ipc("windows")["windows"]
                          if w["id"] not in self.existing_windows
                          and (w.get("pid") == self.process.pid or w["app_id"] == app_ids[self.name])]
            if candidates:
                self.window = candidates[-1]["id"]
                if self.name.startswith("mpv"):
                    if Path(self.mpv_socket).exists():
                        break
                else:
                    prefix = "org.mpris.MediaPlayer2." + self.name
                    services = [line.split()[0] for line in command("busctl", "--user", "list", "--no-pager").splitlines()
                                if line.startswith(prefix) and str(self.process.pid) in line.split()]
                    if services:
                        self.service = services[0]
                        if self.prop("PlaybackStatus") == "Playing":
                            break
            time.sleep(0.25)
        else:
            raise RuntimeError(f"{self.name} window/playback did not appear")
        ipc("focus", id=self.window)
        if self.name == "utsushi":
            # Its own fullscreen action hides the controls as well as requesting fullscreen.
            ipc("synthesize", events=[dict(type="chord", keys="f")])
        ipc("fullscreen", id=self.window, state=True)
        # Park away from bottom controls (mpv's installed uosc keeps them open on hover).
        output = next(p for p in ipc("outputs")["outputs"] if p["enabled"] and p["powered"])
        rect = output["logical"]
        ipc("synthesize", events=[dict(type="pointer-motion", x=rect["x"] + rect["w"] / 2,
                                        y=rect["y"] + rect["h"] / 2)])

    def mpv(self, *cmd):
        with socket.socket(socket.AF_UNIX) as sock:
            sock.settimeout(5)
            sock.connect(self.mpv_socket)
            sock.sendall(json.dumps(dict(command=cmd, request_id=1)).encode() + b"\n")
            reader = sock.makefile("rb")
            while True:
                line = reader.readline()
                if not line:
                    raise RuntimeError("mpv IPC disconnected")
                result = json.loads(line)
                if result.get("request_id") == 1:
                    if result["error"] != "success":
                        raise RuntimeError(result)
                    return result.get("data")

    def prop(self, name):
        return json.loads(command("busctl", "--user", "--json=short", "get-property",
                                  self.service, MPRIS, PLAYER, name))["data"]

    def seek(self, seconds):
        if self.name.startswith("mpv"):
            self.mpv("seek", seconds, "absolute+exact")
        else:
            track = self.prop("Metadata")["mpris:trackid"]["data"]
            command("busctl", "--user", "call", self.service, MPRIS, PLAYER,
                    "SetPosition", "ox", track, str(int(seconds * 1e6)))
            command("busctl", "--user", "set-property", self.service, MPRIS, PLAYER, "Volume", "d", "1.0")

    def wait_for_position(self, target):
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            pos = self.mpv("get_property", "time-pos") if self.name.startswith("mpv") else self.prop("Position") / 1e6
            if pos >= target:
                if pos > target + 0.75:
                    raise RuntimeError(f"Missed requested movie interval: {pos:.3f} > {target:.3f}")
                return
            time.sleep(min(0.1, target - pos))
        raise RuntimeError("Playback did not reach requested measurement start")

    def state(self):
        window = next((w for w in ipc("windows")["windows"] if w["id"] == self.window), None)
        if not window or not window["fullscreen"] or not window["focused"] or window["hidden"] or window["minimised"]:
            raise RuntimeError("Player is no longer visible, focused and fullscreen")
        window = {k: window[k] for k in ("id", "app_id", "pid", "fullscreen", "focused", "geometry", "output")}
        if self.name.startswith("mpv"):
            state = {k: self.mpv("get_property", k) for k in (
                "time-pos", "pause", "speed", "volume", "mute", "hwdec-current",
                "video-codec", "video-params", "frame-drop-count", "decoder-frame-drop-count")}
            position = state["time-pos"]
            if state["pause"] or state["speed"] != 1 or state["mute"] or state["volume"] != 100:
                raise RuntimeError("Playback paused, speed changed, or audio changed")
        else:
            state = {k: self.prop(k) for k in ("PlaybackStatus", "Position", "Rate", "Volume")}
            position = state["Position"] / 1e6
            if state["PlaybackStatus"] != "Playing" or state["Rate"] != 1 or abs(state["Volume"] - 1) > 0.01:
                raise RuntimeError("Playback stopped, speed changed, or audio changed")
        return dict(window=window, position_s=position, player=state)

    def close(self):
        if self.process.poll() is None:
            if self.window:
                try:
                    ipc("close", id=self.window)
                    self.process.wait(timeout=5)
                except (RuntimeError, OSError, subprocess.TimeoutExpired):
                    pass
            if self.process.poll() is None:
                os.killpg(self.process.pid, signal.SIGTERM)
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(self.process.pid, signal.SIGKILL)
                    self.process.wait()
        self.log.close()


def sample(root, started):
    return dict(utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                elapsed_s=time.monotonic() - started, battery=battery(root),
                brightness=brightness(), temperatures_c=temperatures())


def run(args):
    runs = schedule(args.players, args.starts, args.sequence)
    if args.sequence:
        args.players = list(dict.fromkeys(args.sequence))
    args.output.mkdir(parents=True, exist_ok=False)
    initial_brightness = brightness()
    initial_display = display_state()
    initial_audio = command("wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@")
    initial_battery = battery(args.battery)
    ensure_environment(dict(battery=initial_battery, brightness=initial_brightness), initial_brightness, args.minimum_battery)
    existing = ipc("windows")["windows"]
    if any(w["app_id"].lower() in ("org.aizome.utsushi", "mpv", "vlc") for w in existing):
        raise RuntimeError("A comparison player is already open; close it before running")
    focused = next((w["id"] for w in existing if w["focused"]), None)
    probe = json.loads(command(args.ffprobe, "-v", "error", "-show_format", "-show_streams", "-of", "json", str(args.media)))
    video = next(s for s in probe["streams"] if s["codec_type"] == "video")
    if int(video["width"]) < 1920 or int(video["height"]) < 1080 or float(probe["format"]["duration"]) < 5400:
        raise RuntimeError("Movie must be at least 1920x1080 and 90 minutes")
    measurement_start = args.measure_at if args.measure_at is not None else max(args.starts) + args.warmup + 4
    if measurement_start + args.seconds >= float(probe["format"]["duration"]):
        raise RuntimeError("Segment exceeds movie duration")
    digest = hashlib.file_digest(args.media.open("rb"), "sha256").hexdigest()
    save(args.output / "media.json", probe)
    manifest = dict(protocol=2, started_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    media=str(args.media), sha256=digest, battery=initial_battery,
                    brightness=initial_brightness, displays=initial_display, audio=initial_audio,
                    system=os.path.realpath("/run/current-system"), kernel=os.uname().release,
                    compositor=ipc("version"), players=args.players, starts=args.starts,
                    schedule=runs, measure_at_s=args.measure_at,
                    harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    warmup_s=args.warmup, measurement_s=args.seconds, interval_s=args.interval,
                    binaries={p: os.path.realpath(command("which", p)) for p in ("aizome-utsushi", "vlc", "mpv")},
                    versions={p: command(p, "--version") for p in ("vlc", "mpv")},
                    power_profile=read("/sys/firmware/acpi/platform_profile"),
                    epp=read("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference"),
                    background_names=sorted({v[0] for v in processes().values()}))
    save(args.output / "manifest.json", manifest)
    summaries = []
    try:
        starts = [r["start_s"] for r in runs] if args.sequence else args.starts
        for round_index, start in enumerate(starts):
            order = ([args.sequence[round_index]] if args.sequence else
                     args.players[round_index % len(args.players):] + args.players[:round_index % len(args.players)])
            for name in order:
                directory = args.output / f"{round_index + 1:02d}-{name}"
                directory.mkdir()
                print(f"START {directory.name}: movie {start}s; warmup {args.warmup}s; measure {args.seconds}s", flush=True)
                with tempfile.TemporaryDirectory(prefix="player-battery-", dir=os.environ["XDG_RUNTIME_DIR"]) as temp:
                    playback = Playback(name, args.media, directory, Path(temp))
                    try:
                        playback.wait_ready()
                        playback.seek(start)
                        warm_started = time.monotonic()
                        while time.monotonic() - warm_started < args.warmup:
                            s = sample(args.battery, warm_started)
                            ensure_environment(s, initial_brightness, args.minimum_battery)
                            playback.state()
                            time.sleep(min(5, max(0, args.warmup - (time.monotonic() - warm_started))))
                        before = playback.state()
                        if abs(before["position_s"] - (start + args.warmup)) > 4:
                            raise RuntimeError("Seek/warmup playback position mismatch")
                        ipc("screenshot", path=str(directory / "fullscreen-before.png"), output=before["window"]["output"], clipboard=False)
                        # Let the one-time screenshot/encoding finish before the measured interval.
                        time.sleep(3)
                        if args.measure_at is not None:
                            playback.wait_for_position(args.measure_at)
                        before = playback.state()
                        if args.measure_at is not None and abs(before["position_s"] - args.measure_at) > 0.75:
                            raise RuntimeError("Measurement did not start within 750 ms of requested movie position")
                        save(directory / "before.json", dict(argv=playback.argv, **before))
                        started = time.monotonic()
                        previous_cpu, previous_time = processes(), started
                        samples = []
                        with (directory / "samples.jsonl").open("w", buffering=1) as out:
                            for index in range(math.ceil(args.seconds / args.interval) + 1):
                                deadline = started + min(index * args.interval, args.seconds)
                                time.sleep(max(0, deadline - time.monotonic()))
                                s = sample(args.battery, started)
                                ensure_environment(s, initial_brightness, args.minimum_battery)
                                if playback.process.poll() is not None:
                                    raise RuntimeError("Player exited during measurement")
                                if index % max(1, round(10 / args.interval)) == 0 or index == math.ceil(args.seconds / args.interval):
                                    state = playback.state()
                                    if abs(state["position_s"] - before["position_s"] - s["elapsed_s"]) > 3:
                                        raise RuntimeError("Playback stalled or skipped")
                                    if display_state() != initial_display or command("wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@") != initial_audio:
                                        raise RuntimeError("Display or system volume changed")
                                    now = time.monotonic()
                                    current = processes()
                                    s["cpu_percent_one_core"] = cpu_delta(previous_cpu, current, now - previous_time)
                                    previous_cpu, previous_time = current, now
                                    s["playback"] = state
                                out.write(json.dumps(s) + "\n")
                                samples.append(s)
                        after = playback.state()
                        save(directory / "after.json", after)
                        ipc("screenshot", path=str(directory / "fullscreen-after.png"), output=after["window"]["output"], clipboard=False)
                        summary = dict(player=name, round=round_index + 1, segment_start_s=start,
                                       measured_movie_start_s=before["position_s"], **estimate(samples))
                        save(directory / "summary.json", summary)
                        summaries.append(summary)
                        save(args.output / "summaries.json", summaries)
                        print(f"DONE {directory.name}: {summary['mean_power_w']:.2f} W, {summary['projected_full_minutes']:.0f} min/full charge", flush=True)
                    except BaseException as exc:
                        save(directory / "invalid.json", dict(error=str(exc), type=type(exc).__name__))
                        raise
                    finally:
                        playback.close()
    finally:
        if focused is not None:
            ipc("focus", id=focused)
        save(args.output / "end.json", dict(battery=battery(args.battery), brightness=brightness(), displays=display_state()))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("media", type=Path)
    p.add_argument("--output", type=Path, required=True, help="New directory, refuses overwriting evidence")
    p.add_argument("--battery", type=Path, default=Path("/sys/class/power_supply/BAT0"))
    p.add_argument("--ffprobe", default="ffprobe")
    p.add_argument("--players", nargs="+", choices=["utsushi", "vlc", "mpv", "mpv-auto"], default=["utsushi", "vlc", "mpv"])
    p.add_argument("--sequence", nargs="+", choices=["utsushi", "vlc", "mpv", "mpv-auto"],
                   help="Explicit player order; requires exactly one --starts value")
    p.add_argument("--measure-at", type=float, help="Align measurement to this movie position in seconds (within 750 ms)")
    p.add_argument("--starts", nargs="+", type=float, default=[600, 2400, 4200])
    p.add_argument("--warmup", type=float, default=60)
    p.add_argument("--seconds", type=float, default=120)
    p.add_argument("--interval", type=float, default=2)
    p.add_argument("--minimum-battery", type=int, default=15)
    args = p.parse_args()
    if not all(math.isfinite(v) and v > 0 for v in [args.warmup, args.seconds, args.interval]) or args.interval > args.seconds:
        p.error("Durations must be positive and finite; interval cannot exceed measurement")
    if not all(math.isfinite(v) and v >= 0 for v in args.starts):
        p.error("Start times must be finite and nonnegative")
    if args.sequence and len(args.starts) != 1:
        p.error("--sequence requires exactly one --starts value")
    if args.measure_at is not None and (not math.isfinite(args.measure_at) or
            any(not 4 <= args.measure_at - start - args.warmup <= 25 for start in args.starts)):
        p.error("--measure-at must be 4–25 seconds after seek point plus warmup, allowing screenshot and alignment")
    args.media, args.output = args.media.resolve(), args.output.resolve()
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    run(args)


if __name__ == "__main__":
    main()
