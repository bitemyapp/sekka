#!/usr/bin/env python3
"""Opt-in, bounded hardware trials. `plan` and `report` never touch the radio.

The installed copy has an immutable manifest. Only `broker` runs as root;
it accepts fixed case IDs and executes store-owned radio helpers. Capture,
receiver discovery, probes, UI and result files run as the desktop user.
"""
import argparse
import datetime as dt
import ipaddress
import json
import os
from pathlib import Path
import queue
import re
import signal
import select
import socket
import subprocess
import sys
import tempfile
import threading
import time

MANIFEST = "@manifest@"
LIMIT = 65536
RELOAD_HELPERS = {"patched", "performance", "filter"}
MEDIA_PROFILES = ("screen-auto", "screen-low", "screen-noaudio", "screen-software",
                  "screen-latency", "screen-hevc", "synthetic-low", "synthetic-video", "synthetic-av")


def emit(value):
    print(json.dumps(value, separators=(",", ":")), flush=True)


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def write_json(path, value):
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(value, indent=2) + "\n")
    temp.replace(path)


def lines(pipe, events, kind):
    try:
        while True:
            line = pipe.readline(LIMIT + 1)
            if not line:
                break
            if len(line) > LIMIT:
                events.put(("error", "An oversized diagnostic line was rejected"))
                break
            events.put((kind, line.decode("utf-8", "replace").rstrip()))
    finally:
        events.put((kind + "_eof", ""))


def checked_cases(manifest):
    cases = {}
    for case in manifest["cases"]:
        name = case["id"]
        if not re.fullmatch(r"[a-z0-9-]+", name) or name in cases:
            raise ValueError("Invalid or duplicate case ID")
        command = case["radio"]
        if len(command) not in (2, 3) or command[0] not in ("trial", "observe"):
            raise ValueError("Invalid radio command")
        seconds = int(command[1])
        maximum = 60 if command[0] == "observe" else 600
        if not 1 <= seconds <= maximum:
            raise ValueError("Unbounded radio case")
        if case["driver"] in RELOAD_HELPERS and seconds > 120:
            raise ValueError("Driver reload trials are limited to two minutes")
        if case["driver"] not in manifest["helpers"]:
            raise ValueError("Unknown helper")
        if case.get("sender", "default") not in manifest["senders"]:
            raise ValueError("Unknown sender")
        cases[name] = case
    return cases


def selected_cases(manifest, suite, names):
    cases = checked_cases(manifest)
    if names:
        unknown = set(names) - cases.keys()
        if unknown:
            raise ValueError("Unknown cases: " + ", ".join(sorted(unknown)))
        return [cases[n] for n in names]
    return [c for c in cases.values() if suite == "all" or suite in c["suites"]]


def mirroring_case(case):
    """Promote a fixed radio case to a bounded, manually observed media trial."""
    if case["radio"][0] != "trial":
        raise ValueError("Receive-only cases cannot mirror; select a trial case")
    seconds = 120 if case["driver"] in RELOAD_HELPERS else 300
    return {**case, "radio": ["trial", str(seconds), case["radio"][2]],
            "human": True, "mirroring": True, "media": case.get("media", "screen-auto"),
            "instructions": "Click Use Direct Wi-Fi, choose ONLY the requested TV and enter its PIN in the app. "
                            "Observe moving video for at least 60 seconds. Close the window to end early; "
                            "record the TV result in this terminal."}


def read_request(stream):
    raw = stream.readline(4097)
    if not raw:
        return None
    if len(raw) > 4096:
        raise ValueError("Oversized broker request")
    request = json.loads(raw)
    if not isinstance(request, dict):
        raise ValueError("Invalid broker request")
    return request


def broker(manifest):
    if os.geteuid() != 0:
        raise ValueError("The radio broker requires administrator authentication")
    cases = checked_cases(manifest)
    events = queue.Queue()
    child = None
    stopping = False
    deadline = 0
    streams = 0
    current = None
    count = 0
    started = time.monotonic()
    def requests():
        try:
            while True:
                request = read_request(sys.stdin.buffer)
                events.put(("request", request))
                if request is None:
                    break
        except (ValueError, OSError) as error:
            events.put(("request_error", str(error)))
    threading.Thread(target=requests, daemon=True).start()
    signal.signal(signal.SIGTERM, lambda *_: events.put(("request", None)))
    signal.signal(signal.SIGINT, lambda *_: events.put(("request", None)))
    emit({"battery": "ready"})
    try:
        while True:
            try:
                kind, value = events.get(timeout=0.2)
            except queue.Empty:
                kind, value = "tick", None
            if kind in ("request_error", "error"):
                emit({"battery": "error", "error": value})
                stopping = True
            elif kind == "request":
                if value is None or value == {"cmd": "quit"}:
                    stopping = True
                elif value == {"cmd": "stop"}:
                    if child and child.stdin:
                        child.stdin.close()
                elif set(value) in ({"cmd", "case"}, {"cmd", "case", "mirror"}) and value["cmd"] == "start":
                    if child is not None or stopping:
                        emit({"battery": "error", "error": "A case is already active or stopping"})
                        continue
                    if not isinstance(value["case"], str) or value["case"] not in cases or count >= 100 or time.monotonic() - started > 14400:
                        emit({"battery": "error", "error": "Unknown case or batch limit reached"})
                        stopping = True
                        continue
                    current = cases[value["case"]]
                    if "mirror" in value:
                        if value["mirror"] is not True or current["radio"][0] != "trial":
                            emit({"battery": "error", "error": "Invalid mirroring trial"})
                            stopping = True
                            continue
                        current = mirroring_case(current)
                    helper = manifest["helpers"][current["driver"]]
                    # Paths come only from the installed manifest, never IPC.
                    child = subprocess.Popen([helper, *current["radio"]], stdin=subprocess.PIPE,
                                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                             env={"PATH": "/run/current-system/sw/bin:/run/wrappers/bin"})
                    streams = 2
                    count += 1
                    deadline = time.monotonic() + int(current["radio"][1]) + 150
                    for pipe, label in ((child.stdout, "stdout"), (child.stderr, "stderr")):
                        threading.Thread(target=lines, args=(pipe, events, label), daemon=True).start()
                    emit({"battery": "case_start", "case": current["id"]})
                else:
                    emit({"battery": "error", "error": "Unsupported broker request"})
                    stopping = True
            elif kind == "stdout":
                try:
                    record = json.loads(value)
                    if not isinstance(record, dict):
                        continue
                except ValueError:
                    record = {"state": "radio_log", "message": value}
                emit(record)
            elif kind == "stderr":
                emit({"state": "radio_log", "message": value})
            elif kind in ("stdout_eof", "stderr_eof"):
                streams -= 1
            if time.monotonic() - started > 14400:
                stopping = True
            if child and (stopping or time.monotonic() > deadline) and child.stdin:
                child.stdin.close()
            if child and time.monotonic() > deadline + 60:
                # Ask the fixed owning system unit to stop and run ExecStopPost.
                subprocess.run([manifest["tools"]["systemctl"], "stop", "sekka-airplay-peer.service"],
                               timeout=60, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                stopping = True
            if child and child.poll() is not None and streams <= 0:
                emit({"battery": "case_end", "case": current["id"], "exitCode": child.returncode})
                for pipe in (child.stdin, child.stdout, child.stderr):
                    pipe.close()
                child = None
            if stopping and child is None:
                return
    finally:
        if child:
            if child.stdin:
                child.stdin.close()
            try:
                child.wait(timeout=60)
            except subprocess.TimeoutExpired:
                subprocess.run([manifest["tools"]["systemctl"], "stop", "sekka-airplay-peer.service"],
                               timeout=60, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


class BrokerClient:
    def __init__(self, manifest):
        self.process = subprocess.Popen([manifest["tools"]["pkexec"], str(Path(__file__).resolve()), "broker"],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.events = queue.Queue()
        self.readers = []
        for pipe, label in ((self.process.stdout, "broker"), (self.process.stderr, "broker_stderr")):
            reader = threading.Thread(target=lines, args=(pipe, self.events, label), daemon=True)
            reader.start(); self.readers.append(reader)

    def send(self, command):
        self.process.stdin.write((json.dumps(command) + "\n").encode())
        self.process.stdin.flush()

    def close(self):
        if self.process.stdin:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=90)
            for reader in self.readers: reader.join(timeout=1)
            self.process.stdout.close(); self.process.stderr.close()
        except subprocess.TimeoutExpired:
            raise RuntimeError("Radio cleanup did not finish. Do not start another test; inspect sekka-airplay-peer.service.")


def command_output(command, timeout=10):
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        return {"exitCode": result.returncode, "stdout": result.stdout[-262144:], "stderr": result.stderr[-8192:]}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"error": str(error)}


def network_state(manifest):
    return command_output([manifest["tools"]["nmcli"], "-t", "-f", "DEVICE,STATE,CONNECTION", "device"])


def recovery_ok(before, after, interface_names, driver_restore_pending=False):
    # Verify the actual prior active Wi-Fi connections, not just exit status.
    if driver_restore_pending or "error" in before or "error" in after or before.get("exitCode") or after.get("exitCode"):
        return False
    active = [line for line in before["stdout"].splitlines()
              if ":connected:" in line and line.split(":", 1)[0].startswith(("wl", "wlan"))]
    return bool(active) and all(line in after["stdout"].splitlines() for line in active) and not any(
        name in interface_names for name in ("owlymon0", "owlyibss0", "owlyap0", "awdl0"))


def target_device(receivers, name):
    matches = [d for d in receivers if d.get("name") == name]
    return matches[0] if len(matches) == 1 else None


def scoped_address(device):
    address = device.get("ip", "")
    host, separator, zone = address.partition("%")
    if separator != "%" or zone != "awdl0" or not ipaddress.ip_address(host).is_link_local:
        raise ValueError("Only the selected TV's scoped AWDL IPv6 endpoint may be probed")
    port = int(device.get("port", 7000))
    if not 1 <= port <= 65535:
        raise ValueError("Invalid AirPlay port")
    return address, port


def probe_target(manifest, device, destination, stop):
    try:
        address, port = scoped_address(device)
        results = {"device": device, "started": utc(), "ping": []}
        for size in (32, 1200):
            if stop.is_set(): break
            results["ping"].append({"size": size, **command_output(
                [manifest["tools"]["ping"], "-6", "-n", "-c", "5", "-i", "0.2", "-W", "1", "-s", str(size), address], 8)})
        results["info"] = []
        for _ in range(3):
            if stop.is_set(): break
            start = time.monotonic()
            try:
                with socket.create_connection((address, port), timeout=3) as connection:
                    connection.sendall(b"GET /info HTTP/1.1\r\nHost: airplay.local\r\nConnection: close\r\n\r\n")
                    response = bytearray()
                    deadline = time.monotonic() + 5
                    while len(response) < 1048576 and time.monotonic() < deadline:
                        connection.settimeout(max(0.01, deadline - time.monotonic()))
                        chunk = connection.recv(min(65536, 1048576 - len(response)))
                        if not chunk:
                            break
                        response.extend(chunk)
                        if b"\r\n\r\n" in response:
                            header, body = response.split(b"\r\n\r\n", 1)
                            match = re.search(br"(?im)^Content-Length:\s*(\d+)", header)
                            if match and len(body) >= int(match.group(1)):
                                break
                    # No response bodies, pairing keys or packet payloads in artifacts.
                    results["info"].append({"milliseconds": round((time.monotonic() - start) * 1000),
                        "bytes": len(response), "status": response.split(b"\r\n", 1)[0].decode("ascii", "replace")[:200]})
            except (OSError, ValueError) as error:
                results["info"].append({"error": str(error)})
        write_json(destination, results)
    except (OSError, ValueError) as error:
        write_json(destination, {"error": str(error)})


def classify(log):
    if "send IDR:" in log and ("i/o timeout" in log or "timed out" in log):
        return "first_keyframe_timeout"
    if "driver may have reloaded" in log or "ADVANCED_SYSASSERT" in log:
        return "radio_reset"
    if "stream error" in log:
        return "stream_error"
    if "mirror setup failed" in log:
        return "session_setup_timeout" if "i/o timeout" in log else "session_setup_error"
    if "reconnect for credential pairing failed" in log:
        return "pairing_transport_timeout" if "i/o timeout" in log else "pairing_transport_error"
    if log.rfind("pin pairing:") > log.rfind("credentials saved"):
        return "pairing_failed"
    if "streaming to " in log:
        return "stream_started_unverified"
    if "credentials saved" in log:
        return "paired_without_stream"
    if "pin required" in log:
        return "pin_prompt"
    return "no_session_evidence"


class AutoTrial:
    """Observe the sender; never turn its streaming state into a TV verdict."""
    def __init__(self):
        self.attempt_at = None
        self.stream_at = None
        self.radio_at = None
        self.reason = None

    def observe(self, record, now):
        state = record.get("state")
        if state == "up":
            self.radio_at = now
        if record.get("attemptStarted"):
            self.attempt_at = now
        if state == "streaming" and self.stream_at is None:
            self.stream_at = now
        if record.get("error"):
            self.reason = "sender_reported_error"
        elif state == "idle" and self.stream_at is not None:
            self.reason = "sender_returned_idle"

    def stop_reason(self, now):
        if self.reason: return self.reason
        if self.stream_at is not None and now - self.stream_at >= 75:
            return "sender_streamed_75s_unverified"
        if self.attempt_at is not None and self.stream_at is None and now - self.attempt_at >= 180:
            return "session_attempt_deadline"
        if self.radio_at is not None and self.attempt_at is None and now - self.radio_at >= 90:
            return "target_not_selected_within_90s"
        return None


def run_case(manifest, case, broker_client, directory, target, before):
    directory.mkdir(mode=0o700)
    write_json(directory / "case.json", case)
    result = {"case": case["id"], "started": utc(), "target": target, "tvVerdict": "unverified"}
    events = broker_client.events
    process = None
    relay = None
    server = None
    probe = None
    probe_stop = threading.Event()
    app_threads = []
    case_started = False
    case_done = False
    requested_stop = False
    auto = AutoTrial() if case.get("autonomous") else None
    kernel_start = dt.datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S")
    # A 0700 private directory keeps local clients from borrowing the radio.
    with tempfile.TemporaryDirectory(prefix="airplay-battery-", dir=os.environ.get("XDG_RUNTIME_DIR")) as temporary:
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(Path(temporary) / "radio.sock"))
        server.listen(1)
        server.settimeout(0.2)
        env = os.environ.copy()
        env.update(SEKKA_AIRPLAY_RADIO_RELAY=str(Path(temporary) / "radio.sock"),
                   SEKKA_AIRPLAY_LOG_RADIO="1", SEKKA_AIRPLAY_TEST_MEDIA_PROFILE=case.get("media", "screen-auto"),
                   SEKKA_AIRPLAY_TEST_TARGET=target, SEKKA_AIRPLAY_TEST_LABEL=case["id"],
                   SEKKA_AIRPLAY_AUTOTEST="1" if auto else "0",
                   SEKKA_AIRPLAY_TEST_DEVICE_ID=case.get("deviceId", ""),
                   SEKKA_AIRPLAY_SENDER=manifest["senders"][case.get("sender", "default")])
        args = [manifest["app"]] if auto else ([manifest["app"], "--discovery-details"] if case["human"] else [manifest["app"], "--discover-peer"])
        print(f"\n{case['id']}: {case['hypothesis']}\n{case['instructions']}", flush=True)
        process = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        for pipe, label in ((process.stdout, "app"), (process.stderr, "app_log")):
            thread = threading.Thread(target=lines, args=(pipe, events, label), daemon=True)
            thread.start(); app_threads.append(thread)
        started_at = time.monotonic()
        deadline = started_at + 180 # time to click Use Direct Wi-Fi
        next_tcp = 0
        log_size = 0
        app_text = []
        try:
            with (directory / "events.jsonl").open("w") as log, (directory / "tcp.jsonl").open("w") as tcp:
                while True:
                    if relay is None and not case_started and process.poll() is None:
                        try:
                            relay, _ = server.accept()
                            relay.settimeout(1)
                            command = {"cmd": "start", "case": case["id"]}
                            if case.get("mirroring"):
                                command["mirror"] = True
                            broker_client.send(command)
                            case_started = True
                            deadline = time.monotonic() + int(case["radio"][1]) + 150
                        except socket.timeout:
                            pass
                    try:
                        kind, value = events.get(timeout=0.1)
                    except queue.Empty:
                        kind, value = "tick", ""
                    if kind in ("app", "app_log", "broker", "broker_stderr", "error"):
                        entry = json.dumps({"time": utc(), "source": kind, "message": value}) + "\n"
                        if log_size < 16 * 1024 * 1024:
                            log.write(entry); log.flush(); log_size += len(entry)
                        if kind in ("app_log", "app"):
                            if auto and value.startswith("[airplay-state] "):
                                state = json.loads(value.removeprefix("[airplay-state] "))
                                auto.observe(state, time.monotonic())
                                result.setdefault("senderStates", []).append({"time": utc(), **state})
                                print(f"{case['id']}: {state.get('state')}" +
                                      (" — enter the TV PIN in the AirPlay window" if state.get('state') == 'pin_required' else ""), flush=True)
                            # Bounded in-memory summary; full bounded log is on disk.
                            if not value.startswith("[direct-wifi]"):
                                app_text.append(value)
                                app_text = app_text[-2000:]
                            try:
                                record = json.loads(value.removeprefix("[direct-receivers] "))
                                device = target_device(record.get("receivers", []), target)
                                if device:
                                    result["discoveredTarget"] = device
                                # Extra probe traffic would contaminate a media trial.
                                if device and probe is None and not case["human"]:
                                    probe = threading.Thread(target=probe_target,
                                        args=(manifest, device, directory / "target-probes.json", probe_stop), daemon=True)
                                    probe.start()
                            except (ValueError, AttributeError, TypeError):
                                pass
                        if kind == "broker":
                            try:
                                record = json.loads(value)
                            except ValueError:
                                record = {"battery": "error", "error": "Invalid broker output"}
                            if record.get("battery") == "case_end":
                                result["radioExitCode"] = record["exitCode"]
                                case_done = True
                                if relay:
                                    relay.close(); relay = None
                            elif record.get("battery") == "error":
                                raise RuntimeError(record["error"])
                            elif "state" in record and relay:
                                if auto and record.get("state") == "up":
                                    auto.observe(record, time.monotonic())
                                if record.get("state") == "error":
                                    result["radioError"] = record.get("error")
                                try:
                                    relay.sendall((value + "\n").encode())
                                except OSError:
                                    relay.close(); relay = None
                                    broker_client.send({"cmd": "stop"})
                                    requested_stop = True
                    if relay:
                        try:
                            ready, _, _ = select.select([relay], [], [], 0)
                            if ready and not relay.recv(1, socket.MSG_PEEK):
                                relay.close(); relay = None
                                broker_client.send({"cmd": "stop"})
                                requested_stop = True
                        except OSError:
                            pass
                    if case_started and not case_done and time.monotonic() >= next_tcp:
                        sample = command_output([manifest["tools"]["ss"], "-tin"], 3)
                        rows = sample.get("stdout", "").splitlines()
                        blocks = [rows[i:i+2] for i, row in enumerate(rows) if "%awdl0" in row]
                        if blocks:
                            tcp.write(json.dumps({"time": utc(), "sockets": blocks}) + "\n"); tcp.flush()
                        next_tcp = time.monotonic() + 1
                    if process.poll() is not None and not case_started:
                        result["skipped"] = "Window closed before the radio started"
                        break
                    if case_done:
                        break
                    if process.poll() is not None and not requested_stop:
                        broker_client.send({"cmd": "stop"}); requested_stop = True
                    if auto and not requested_stop and (reason := auto.stop_reason(time.monotonic())):
                        result["automaticStop"] = reason
                        print(f"{case['id']}: {reason}; restoring Wi-Fi", flush=True)
                        broker_client.send({"cmd": "stop"}); requested_stop = True
                    if time.monotonic() > deadline:
                        raise RuntimeError("Case exceeded its radio/cleanup deadline")
                    if broker_client.process.poll() is not None:
                        raise RuntimeError("Radio broker exited unexpectedly")
        finally:
            if case_started and not case_done:
                try:
                    broker_client.send({"cmd": "stop"})
                except OSError:
                    pass # EOF also releases the broker's helper lease.
            if relay:
                relay.close()
            server.close()
            try:
                # Relay EOF asks the GUI to stop the sender and erase its
                # private pairing directory before it closes normally.
                process.wait(timeout=15 if case_done else 0.1)
            except subprocess.TimeoutExpired:
                process.terminate()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait()
            probe_stop.set()
            if probe:
                probe.join(timeout=15)
            for thread in app_threads:
                thread.join(timeout=1)
            process.stdout.close(); process.stderr.close()
            with (directory / "events.jsonl").open("a") as log:
                while True:
                    try: kind, value = events.get_nowait()
                    except queue.Empty: break
                    if kind in ("app", "app_log", "broker", "broker_stderr"):
                        log.write(json.dumps({"time":utc(), "source":kind, "message":value}) + "\n")
                        if kind in ("app", "app_log") and not value.startswith("[direct-wifi]"):
                            app_text.append(value)
        result["classification"] = classify("\n".join(app_text))
    after = network_state(manifest)
    write_json(directory / "network-after.json", after)
    result["driverRestorePending"] = Path("/run/sekka-airplay-intel-trial").exists()
    result["recoveryVerified"] = recovery_ok(before, after, os.listdir("/sys/class/net"), result["driverRestorePending"])
    kernel = command_output([manifest["tools"]["journalctl"], "-k", "--since", kernel_start,
                             "--no-pager", "-o", "short-iso", "-n", "2000"])
    write_json(directory / "kernel.json", kernel)
    if result["classification"] == "no_session_evidence" and "discoveredTarget" in result:
        result["classification"] = "target_discovered_no_stream"
    result["firmwareAssert"] = "ADVANCED_SYSASSERT" in kernel.get("stdout", "")
    if result["firmwareAssert"]:
        result["classification"] = "radio_reset"
    elif result.get("radioError"):
        result["classification"] = "radio_error"
    elif result.get("radioExitCode", 0) != 0:
        result["classification"] = "helper_failed"
    write_json(directory / "tcp-summary.json", tcp_summary(directory / "tcp.jsonl"))
    # Preserve measured evidence even if the operator interrupts the verdict prompt.
    write_json(directory / "result.json", result)
    if case["human"] and not auto:
        print("TV result: [s] sustained video+audio >=60s; [v] video only >=60s; [b] blank/dropout; [u] unverified")
        verdict = input("> ").strip().lower()
        result["tvVerdict"] = {"s": "sustained_av_60s", "v": "sustained_video_60s", "b": "blank_or_dropout"}.get(verdict, "unverified")
        result["note"] = input("Optional note (no PINs/passwords): ").strip()[:2000]
    result["ended"] = utc()
    write_json(directory / "result.json", result)
    if not result["recoveryVerified"]:
        raise RuntimeError("Wi-Fi recovery was not verified; the battery stopped. See network-after.json and the owning service.")
    return result


def tcp_summary(path):
    """TCP acknowledgements and loss metadata, never application payloads."""
    streams = {}
    for line in path.read_text().splitlines():
        sample = json.loads(line)
        for block in sample.get("sockets", []):
            if len(block) < 2: continue
            fields = block[0].split()
            if len(fields) < 5: continue
            endpoint = " ".join(fields[-2:])
            row = streams.setdefault(endpoint, {"samples": 0, "maxSendQueueBytes": 0})
            row["samples"] += 1
            row["maxSendQueueBytes"] = max(row["maxSendQueueBytes"], int(fields[2]))
            for key in ("bytes_acked", "bytes_sent", "bytes_received", "bytes_retrans", "unacked", "lost", "segs_in", "segs_out"):
                found = re.search(r"(?:^|\s)" + key + r":(\d+)", block[1])
                if found: row["max_" + key] = max(row.get("max_" + key, 0), int(found.group(1)))
            rtt = re.search(r"(?:^|\s)rtt:([\d.]+)", block[1])
            if rtt: row["maxRttMs"] = max(row.get("maxRttMs", 0), float(rtt.group(1)))
    return streams


def report(path):
    rows = []
    for case_path in sorted(Path(path).glob("*/case.json")):
        directory = case_path.parent
        result_path = directory / "result.json"
        interrupted_path = directory / "interrupted.json"
        row = json.loads(result_path.read_text()) if result_path.exists() else {
            "case": json.loads(case_path.read_text())["id"], "classification": "incomplete",
            "tvVerdict": "unverified", "recoveryVerified": False}
        if interrupted_path.exists():
            row["interrupted"] = json.loads(interrupted_path.read_text())
        rows.append(row)
    write_json(Path(path) / "summary.json", rows)
    text = ["case\tstage\tTV verdict\tWi-Fi restored\thelper exit\tfirmware assertion"]
    for row in rows:
        text.append("\t".join(str(row.get(k, "")) for k in ("case", "classification", "tvVerdict", "recoveryVerified", "radioExitCode", "firmwareAssert")))
    value = "\n".join(text) + "\n"
    (Path(path) / "summary.tsv").write_text(value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["plan", "run", "report", "broker"], nargs="?", default="plan")
    parser.add_argument("--suite", choices=["quick", "radio", "media", "driver", "all"], default="quick")
    parser.add_argument("--case", action="append", dest="names")
    parser.add_argument("--target", help="Exact TV name; automatic connection requires --autonomous and --device-id")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--repeat", type=int, choices=[1, 2, 3], default=1)
    parser.add_argument("--mirror", action="store_true", help="Test selected radio settings with full mirroring (5 min; driver reloads 2 min)")
    parser.add_argument("--media-profile", choices=MEDIA_PROFILES, help="Override the media profile for manual cases; combine with --mirror for radio cases")
    parser.add_argument("--autonomous", action="store_true", help="Start direct Wi-Fi, select the exact receiver and run synthetic media; only PIN input remains manual")
    parser.add_argument("--device-id", help="Expected TV device ID, required for autonomous selection")
    args = parser.parse_args()
    if args.action == "report":
        if not args.output:
            parser.error("report needs --output")
        print(report(args.output)); return
    manifest = json.loads(Path(MANIFEST).read_text())
    if args.action == "broker":
        broker(manifest); return
    cases = selected_cases(manifest, args.suite, args.names)
    if args.mirror or args.autonomous:
        cases = [mirroring_case(c) for c in cases]
    if args.media_profile:
        if not all(c["human"] for c in cases):
            parser.error("--media-profile requires media cases or --mirror")
        cases = [{**c, "media": args.media_profile} for c in cases]
    if args.autonomous:
        if not args.target or not args.device_id or not re.fullmatch(r"(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", args.device_id):
            parser.error("--autonomous requires --target and the expected --device-id")
        media = args.media_profile or "synthetic-low"
        if media not in ("synthetic-low", "synthetic-video", "synthetic-av"):
            parser.error("Autonomous testing uses synthetic media, never desktop capture")
        cases = [{**c, "media": media, "autonomous": True, "deviceId": args.device_id,
                  "instructions": "The test starts and selects the identified TV automatically. Enter a PIN only if the AirPlay window asks."} for c in cases]
    if args.action == "plan":
        for case in cases:
            print(f"{case['id']:25} {int(case['radio'][1]):3}s  {'manual' if case['human'] else 'automatic':9} {case.get('media', 'probes'):16} {case['hypothesis']}")
        print(f"{len(cases) * args.repeat} trials; radio time <= {sum(int(c['radio'][1]) for c in cases) * args.repeat / 60:.1f} minutes, plus setup and recovery.")
        return
    if os.geteuid() == 0:
        parser.error("Run the battery as the desktop user; only its radio broker elevates")
    if not args.target or (not args.autonomous and not sys.stdin.isatty()):
        parser.error("run needs --target and an interactive terminal")
    output = args.output or Path.cwd() / ("airplay-results-" + time.strftime("%Y%m%d-%H%M%S"))
    os.umask(0o077)
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    before = network_state(manifest)
    if not recovery_ok(before, before, os.listdir("/sys/class/net"), Path("/run/sekka-airplay-intel-trial").exists()):
        raise RuntimeError("Start with normal Wi-Fi connected and no other AWDL session active")
    write_json(output / "network-before.json", before)
    write_json(output / "plan.json", {"manifest": MANIFEST, "software": manifest, "kernel": list(os.uname()), "cases": cases, "repeat": args.repeat, "target": args.target})
    print(f"Results: {output.resolve()}\nOne administrator prompt starts the bounded radio broker. TV selection and PIN entry remain in the app.", flush=True)
    client = BrokerClient(manifest)
    try:
        ready_deadline = time.monotonic() + 180
        while True:
            try:
                kind, value = client.events.get(timeout=0.2)
            except queue.Empty:
                if client.process.poll() is not None or time.monotonic() > ready_deadline:
                    raise RuntimeError("Administrator authentication or broker startup did not complete")
                continue
            if kind == "broker" and json.loads(value).get("battery") == "ready":
                break
            if kind == "broker_stderr":
                print(value, file=sys.stderr)
        for repeat in range(args.repeat):
            for index, case in enumerate(cases):
                destination = output / f"{repeat + 1:02d}-{index + 1:02d}-{case['id']}"
                try:
                    run_case(manifest, case, client, destination, args.target, before)
                except BaseException as error:
                    write_json(destination / "interrupted.json", {"time": utc(), "error": str(error)})
                    raise
                report(output)
    finally:
        client.close()
        print(report(output))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, OSError, KeyboardInterrupt, EOFError) as error:
        print(f"Battery stopped: {error or type(error).__name__}", file=sys.stderr)
        sys.exit(1)
