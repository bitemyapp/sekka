"""Packaged sender, synthetic screen/audio, real sockets and cryptography.

No host capture or TV connection: the receiver listens only on loopback.
This is component evidence, not interoperability evidence for physical TVs.
"""
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time


def wait_until(check, timeout=30):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            last = check()
            if last:
                return last
        except (FileNotFoundError, ConnectionRefusedError):
            pass
        time.sleep(0.1)
    raise AssertionError(f"timed out: {last!r}")


def stop(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            raise AssertionError("sender/receiver did not shut down gracefully")


def exercise(sender, receiver, profile, auth, root):
    root.mkdir()
    sock = root / "control.sock"
    receiver_log = root / "receiver.log"
    sender_log = root / "sender.log"
    env = dict(os.environ, XDG_RUNTIME_DIR=str(root))
    env.pop("DOUBLETAKE_RECEIVER_CODE", None)
    if auth != "none":
        env["DOUBLETAKE_RECEIVER_CODE"] = "0042"
    # The test flag selects synthetic video AND synthetic audio. Never grant
    # access to a real Wayland screen or an audio monitor here.
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("DISPLAY", None)
    env.pop("DOUBLETAKE_CODE", None)

    def request(cmd, **fields):
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(3)
            s.connect(str(sock))
            s.sendall(json.dumps(dict(cmd=cmd, **fields)).encode() + b"\n")
            with s.makefile("rb") as stream:
                data = stream.readline(262145)
            assert len(data) <= 262144 and data.endswith(b"\n")
            return json.loads(data)

    with receiver_log.open("w") as rlog, sender_log.open("w") as slog:
        recv = subprocess.Popen([receiver, "-listen", "127.0.0.1:0", "-profile", profile,
                                 "-auth", auth, "-stats-interval", "200ms"],
                                stdout=rlog, stderr=subprocess.STDOUT, env=env, start_new_session=True)
        daemon = None
        try:
            match = wait_until(lambda: re.search(r"listening on 127\.0\.0\.1:(\d+)", receiver_log.read_text()))
            port = int(match[1])
            daemon = subprocess.Popen([sender, "-daemonize", "-test", "-hwaccel", "none",
                                       "-socket", str(sock), "-creds", str(root / "pairing.json"),
                                       "-port-range", "60000-60002"],
                                      stdout=slog, stderr=subprocess.STDOUT, env=env, start_new_session=True)
            wait_until(lambda: sock.exists() and request("status")["state"] == "idle")
            assert request("connect", target="127.0.0.1", port=port)["ok"]
            if auth != "none":
                prompt = wait_until(lambda: (s if (s := request("status"))["state"] == "pin_required" else None))
                assert prompt["credential_kind"] == ("password" if auth == "password" else "pin"), prompt
                assert request("connect", target="127.0.0.1", port=port, pin="0042")["ok"]
            state = wait_until(lambda: (s if (s := request("status"))["state"] == "streaming" else None))
            assert state["has_audio"], state
            # The fixture counts received/decrypted media. A status of
            # "streaming" alone is insufficient evidence that bytes arrived.
            wait_until(lambda: re.search(r"video=[1-9]\d*/\d+B audio=[1-9]\d*/\d+B", receiver_log.read_text()))
            assert request("mute")["ok"]
            wait_until(lambda: request("status")["audio_muted"])
            assert request("unmute")["ok"]
            wait_until(lambda: not request("status")["audio_muted"])
            assert request("disconnect")["ok"]
            wait_until(lambda: request("status")["state"] == "idle")
            # Reconnect with saved per-session credentials, then terminate the
            # daemon while streaming to check its real executable's cleanup.
            assert request("connect", target="127.0.0.1", port=port)["ok"]
            wait_until(lambda: request("status")["state"] == "streaming")
            stop(daemon)
            assert daemon.returncode == 0
            assert not sock.exists(), "daemon left its control socket"
            # kill(pid, 0) for every member of the former process group would
            # also find lingering GStreamer children.
            for proc in Path("/proc").iterdir():
                if proc.name.isdigit():
                    try:
                        assert os.getpgid(int(proc.name)) != daemon.pid, "capture process survived shutdown"
                    except ProcessLookupError:
                        pass
            print(f"PASS {profile}/{auth}: paired, received video+audio, muted, disconnected, reconnected, stopped")
        except Exception:
            print(receiver_log.read_text(), file=sys.stderr)
            print(sender_log.read_text(), file=sys.stderr)
            raise
        finally:
            if daemon is not None:
                stop(daemon)
            stop(recv)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="airplay-component-") as tmp:
        for profile, auth in [("roku", "pin"), ("lg", "pin"), ("modern", "none")]:
            exercise(sys.argv[1], sys.argv[2], profile, auth, Path(tmp) / profile)
