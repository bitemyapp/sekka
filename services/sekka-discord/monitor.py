#!/usr/bin/env python3
"""Bounded, one-shot security monitoring; all alert text is locally generated."""

import argparse
import fcntl
import functools
import grp
import hashlib
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import selectors
import shutil
import signal
import ssl
import stat
import subprocess
import sys
import time

BOT = "sekka-discord.service"
TUNNEL = "sekka-discord-tunnel.service"
COOLDOWN = 900
QUIET = 300
MAX_STATE = 65536
MAX_JOURNAL = 1024 * 1024
MAX_RECORDS = 1000
MAX_HTTP = 256 * 1024
LABELS = {
    "bot_unit": "Yuki system service inactive or unavailable",
    "tunnel_unit": "Tunnel system service inactive or unavailable",
    "bot_restart": "Yuki service automatic restarts",
    "tunnel_restart": "Tunnel service automatic restarts",
    "health": "Yuki loopback health check failed",
    "ready": "Tunnel loopback readiness check failed",
    "disk": "Monitor state filesystem has less than 512 MiB or 5% free",
    "journal": "Journal collection failed; security visibility is incomplete",
    "journal_gap": "Journal cursor expired; resumed by timestamp with possible overlap or missing history",
    "journal_backlog": "Journal read limit reached; collection is catching up",
    "journal_record": "Journal records could not be fully interpreted",
    "webhook": "Rejected webhook requests",
    "unresolved": "Unresolved Discord sends require operator review",
    "catchup": "Startup catch-up failures or incomplete retained history",
    "dispatch": "Discord dispatch or reconciliation failures",
    "ssh_failure": "SSH authentication failures",
    "ssh_success": "Successful SSH authentication",
    "sudo": "Sudo activity",
    "egress": "Sandbox egress denied by host firewall",
}
EVENT_KEYS = {"bot_restart", "tunnel_restart", "journal_gap", "journal_record",
              "webhook", "unresolved", "catchup", "dispatch", "ssh_failure",
              "ssh_success", "sudo", "egress"}
COUNT_MAX = 10**12
SSH_ACCEPTED = re.compile(r"Accepted [^ ]+ for [^ ]+ from ([^ ]+) port [0-9]{1,5} ssh2(?::|$)")


class MonitorError(Exception):
    """Only fixed, non-sensitive descriptions may leave the monitor."""


class DeliveryError(MonitorError):
    def __init__(self, message="Discord delivery failed", retry_after=0, uncertain=False):
        super().__init__(message)
        self.retry_after = retry_after
        self.uncertain = uncertain


def numeric_id(value):
    return isinstance(value, str) and re.fullmatch(r"[0-9]{1,20}", value) is not None and int(value) > 0


def bounded_http(function):
    """A slow peer cannot stretch per-socket timeouts by trickling bytes."""
    @functools.wraps(function)
    def call(*args, **kwargs):
        def expired(_number, _frame):
            raise TimeoutError("HTTP wall-clock deadline exceeded")

        previous = signal.signal(signal.SIGALRM, expired)
        started = time.monotonic()
        old_timer = signal.setitimer(signal.ITIMER_REAL, 8)
        try:
            return function(*args, **kwargs)
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, previous)
            if old_timer[0]:
                remaining = max(0.001, old_timer[0] - (time.monotonic() - started))
                signal.setitimer(signal.ITIMER_REAL, remaining, old_timer[1])
    return call


def bounded_command(args, maximum=65536, timeout=10):
    """Drain bounded stdout; never buffer stderr or include it in diagnostics."""
    try:
        process = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, env={"PATH": os.environ.get("PATH", ""),
                                                                  "LC_ALL": "C"})
    except OSError:
        raise MonitorError("Required local command could not start") from None
    data = bytearray()
    capped = False
    deadline = time.monotonic() + timeout
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise MonitorError("Local command timed out")
                if not selector.select(remaining):
                    raise MonitorError("Local command timed out")
                chunk = os.read(process.stdout.fileno(), min(16384, maximum + 1 - len(data)))
                if not chunk:
                    break
                data.extend(chunk)
                if len(data) > maximum:
                    capped = True
                    break
        if capped:
            process.kill()
        code = process.wait(timeout=max(0.1, deadline - time.monotonic()))
        return bytes(data[:maximum]), code, capped
    except subprocess.TimeoutExpired:
        raise MonitorError("Local command timed out") from None
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        process.stdout.close()


class Store:
    def __init__(self, directory):
        self.path = Path(directory)
        self.lock = None

    def __enter__(self):
        self.path.mkdir(mode=0o700, parents=True, exist_ok=True)
        info = self.path.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise MonitorError("Monitor state directory must be owned by this user and mode 0700")
        fd = os.open(self.path / "monitor.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        self.lock = os.fdopen(fd, "a+")
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.lock.close()
            self.lock = None
            raise MonitorError("Another monitor invocation holds the process lock") from None
        return self

    def __exit__(self, *_):
        if self.lock:
            self.lock.close()

    def load(self):
        try:
            fd = os.open(self.path / "state.json", os.O_RDONLY | os.O_NOFOLLOW)
        except FileNotFoundError:
            return new_state()
        with os.fdopen(fd, "rb") as source:
            info = os.fstat(source.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
                raise MonitorError("Monitor state file must be private and owned by this user")
            raw = source.read(MAX_STATE + 1)
        if len(raw) > MAX_STATE:
            raise MonitorError("Monitor state exceeds its size limit")
        try:
            state = json.loads(raw)
            validate_state(state)
        except (ValueError, TypeError, KeyError):
            raise MonitorError("Monitor state is invalid; refusing to reset alert history") from None
        return state

    def save(self, state):
        encoded = json.dumps(state, separators=(",", ":"), allow_nan=False).encode()
        if len(encoded) > MAX_STATE:
            raise MonitorError("Monitor state exceeds its size limit")
        temporary = self.path / (".state-" + secrets.token_hex(8))
        try:
            fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "wb") as target:
                target.write(encoded)
                target.flush()
                os.fsync(target.fileno())
            os.replace(temporary, self.path / "state.json")
            directory = os.open(self.path, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            temporary.unlink(missing_ok=True)


def new_state():
    return {"version": 1, "cursor": None, "journal_time": 0, "recent_cursors": [], "restarts": {},
            "incidents": {}, "pending": None, "next_attempt": 0}


def finite_number(value):
    return type(value) in (int, float) and 0 <= value <= 10**15


def validate_state(state):
    if not isinstance(state, dict) or state.get("version") != 1:
        raise ValueError()
    cursor = state["cursor"]
    if cursor is not None and (not isinstance(cursor, str) or len(cursor) > 4096 or "\n" in cursor):
        raise ValueError()
    for field in ("journal_time", "next_attempt"):
        if not finite_number(state[field]):
            raise ValueError()
    recent = state["recent_cursors"]
    if not isinstance(recent, list) or len(recent) > 1024:
        raise ValueError()
    if any(not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{16}", value) is None for value in recent):
        raise ValueError()
    if not isinstance(state["restarts"], dict) or set(state["restarts"]) - {BOT, TUNNEL}:
        raise ValueError()
    if any(not finite_number(value) for value in state["restarts"].values()):
        raise ValueError()
    incidents = state["incidents"]
    if not isinstance(incidents, dict) or set(incidents) - LABELS.keys():
        raise ValueError()
    for record in incidents.values():
        if not isinstance(record, dict):
            raise ValueError()
        for field in ("total", "delivered_total", "last_seen", "last_sent"):
            if not finite_number(record[field]):
                raise ValueError()
        for field in ("active", "delivered_active"):
            if type(record[field]) is not bool:
                raise ValueError()
    pending = state["pending"]
    if pending is not None:
        if not isinstance(pending, dict) or not re.fullmatch(r"[0-9a-f]{24}", pending["reference"]):
            raise ValueError()
        if not finite_number(pending["created"]) or type(pending["uncertain"]) is not bool:
            raise ValueError()
        if not isinstance(pending["items"], list) or not 1 <= len(pending["items"]) <= len(LABELS):
            raise ValueError()
        for item in pending["items"]:
            if item["key"] not in incidents or type(item["active"]) is not bool:
                raise ValueError()
            if not finite_number(item["total"]) or not finite_number(item["count"]):
                raise ValueError()


def read_token(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o007:
            raise MonitorError("Discord token file must not be world-accessible")
        raw = source.read(4097)
    try:
        token = raw.decode("ascii").strip()
    except UnicodeError:
        raise MonitorError("Discord token file is invalid") from None
    if not token or len(raw) > 4096 or any(c.isspace() for c in token):
        raise MonitorError("Discord token file is invalid")
    return token


class Discord:
    def __init__(self, token, recipient):
        self.token = token
        self.recipient = recipient
        self.channel = None

    @bounded_http
    def request(self, method, path, payload=None):
        connection = http.client.HTTPSConnection("discord.com", 443, timeout=8,
                                                 context=ssl.create_default_context())
        try:
            body = None if payload is None else json.dumps(payload).encode()
            connection.request(method, "/api/v10" + path, body=body,
                               headers={"Authorization": "Bot " + self.token,
                                        "Content-Type": "application/json",
                                        "User-Agent": "SekkaSecurityMonitor/1.0"})
            response = connection.getresponse()
            raw = response.read(MAX_HTTP + 1)
            if len(raw) > MAX_HTTP:
                raise DeliveryError("Discord response exceeded limit", uncertain=method == "POST")
            try:
                result = json.loads(raw)
            except (ValueError, UnicodeError):
                result = None
            if response.status == 429:
                delay = result.get("retry_after", 60) if isinstance(result, dict) else 60
                if not finite_number(delay):
                    delay = 60
                raise DeliveryError("Discord rate limit; retry scheduled", min(86400, max(1, delay)))
            if not 200 <= response.status < 300:
                raise DeliveryError("Discord API rejected request", uncertain=response.status >= 500 and method == "POST")
            if result is None:
                raise DeliveryError("Discord response was invalid", uncertain=method == "POST")
            return result
        except (OSError, http.client.HTTPException):
            raise DeliveryError("Discord transport unavailable", uncertain=method == "POST") from None
        finally:
            connection.close()

    def open_dm(self):
        if self.channel is None:
            result = self.request("POST", "/users/@me/channels", {"recipient_id": self.recipient})
            if not isinstance(result, dict) or not numeric_id(result.get("id")):
                raise DeliveryError("Discord DM response was invalid")
            recipients = result.get("recipients")
            if not isinstance(recipients, list) or len(recipients) != 1 or recipients[0].get("id") != self.recipient:
                raise DeliveryError("Discord DM recipient verification failed")
            self.channel = result["id"]
        return self.channel

    def deliver(self, pending):
        channel = self.open_dm()
        content = render(pending)
        if pending["uncertain"]:
            history = self.request("GET", f"/channels/{channel}/messages?limit=100")
            if not isinstance(history, list):
                raise DeliveryError("Discord reconciliation response was invalid")
            for message in history:
                if (isinstance(message, dict) and message.get("content") == content
                        and str(message.get("nonce")) == nonce(pending)
                        and message.get("author", {}).get("bot") is True):
                    return
            # Snowflake creation timestamps bound the scanned history window.
            ids = [int(m["id"]) for m in history if isinstance(m, dict) and numeric_id(m.get("id"))]
            oldest = ((min(ids) >> 22) + 1420070400000) / 1000 if ids else 0
            if len(history) >= 100 and (not ids or oldest > pending["created"]):
                raise DeliveryError("Discord history window cannot safely reconcile pending alert")
        result = self.request("POST", f"/channels/{channel}/messages", {
            "content": content, "allowed_mentions": {"parse": []},
            "nonce": nonce(pending), "enforce_nonce": True,
        })
        if not isinstance(result, dict) or not numeric_id(result.get("id")):
            raise DeliveryError("Discord send acknowledgement was invalid", uncertain=True)

    def smoke(self):
        channel = self.open_dm()
        reference = secrets.token_hex(12)
        content = ("Yuki security monitor — REAL VERIFICATION ONLY\n"
                   "Discord DM delivery/read-back check; not a monitoring health report.\n"
                   "Monitor reference: " + reference)
        result = self.request("POST", f"/channels/{channel}/messages", {
            "content": content, "allowed_mentions": {"parse": []},
            "nonce": str(int(reference, 16) % (2**63)), "enforce_nonce": True,
        })
        if not isinstance(result, dict) or not numeric_id(result.get("id")):
            raise DeliveryError("Smoke send acknowledgement was invalid")
        verified = self.request("GET", f"/channels/{channel}/messages/{result['id']}")
        if not isinstance(verified, dict) or verified.get("content") != content:
            raise DeliveryError("Smoke DM read-back did not match")


def nonce(pending):
    return str(int(pending["reference"], 16) % (2**63))


def render(pending):
    lines = ["Yuki security / availability monitor"]
    for item in pending["items"]:
        label = LABELS[item["key"]]
        if item["active"]:
            lines.append(f"DETECTED: {label}. Observations: {int(item['count'])}.")
        elif item["key"] in EVENT_KEYS:
            lines.append(f"QUIET: {label}. No new observations for at least {QUIET}s; not proof of remediation.")
        else:
            lines.append(f"RECOVERED: {label} — check now passing.")
    lines.append("No log text, identities, addresses, commands or credentials are included.")
    lines.append("Monitor reference: " + pending["reference"])
    return "\n".join(lines)


@bounded_http
def check_loopback(port, path):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
    try:
        connection.request("GET", path)
        response = connection.getresponse()
        return response.status == 200 and len(response.read(4097)) <= 4096
    except (OSError, http.client.HTTPException):
        return False
    finally:
        connection.close()


def observe_units(state, observations, runner):
    for unit, key in ((BOT, "bot"), (TUNNEL, "tunnel")):
        try:
            raw, code, capped = runner(["systemctl", "show", unit, "--no-pager",
                                       "--property=ActiveState,LoadState,NRestarts"])
            values = dict(line.split("=", 1) for line in raw.decode().splitlines() if "=" in line)
            restarts = int(values["NRestarts"])
            if code or capped or restarts < 0 or values.get("LoadState") != "loaded":
                raise ValueError()
            observations[key + "_unit"] = int(values.get("ActiveState") != "active")
            previous = state["restarts"].get(unit)
            if previous is not None and restarts > previous:
                observations[key + "_restart"] = min(COUNT_MAX, restarts - previous)
            state["restarts"][unit] = restarts
        except (MonitorError, ValueError, KeyError, UnicodeError):
            observations[key + "_unit"] = 1


def journal_command(state, cursor=True):
    args = ["journalctl", "--no-pager", "--quiet", "--output=json",
            "--output-fields=__CURSOR,__REALTIME_TIMESTAMP,MESSAGE,_SYSTEMD_UNIT,_COMM,SYSLOG_IDENTIFIER,_TRANSPORT"]
    if cursor and state["cursor"]:
        args.append("--after-cursor=" + state["cursor"])
    else:
        args.append(f"--since=@{state['journal_time']:.6f}"
                    if state["journal_time"] else "--since=-5min")
    args.extend(["_SYSTEMD_UNIT=" + BOT, "+", "_SYSTEMD_UNIT=" + TUNNEL,
                 "+", "_COMM=sshd", "+", "_COMM=sshd-session", "+", "_COMM=sudo",
                 "+", "SYSLOG_IDENTIFIER=sshd", "+", "SYSLOG_IDENTIFIER=sudo",
                 "+", "_TRANSPORT=kernel"])
    return args


def lan_networks(value):
    try:
        return tuple(ipaddress.ip_network(entry) for entry in value.split())
    except ValueError:
        raise MonitorError("SEKKA_MONITOR_LAN_NETWORKS must list network prefixes such as 192.168.1.0/24") from None


def from_lan(message, lan):
    """Unparsed origins are not LAN; loopback never is, since tunnel connectors arrive there."""
    origin = SSH_ACCEPTED.match(message)
    if not lan or not origin:
        return False
    try:
        address = ipaddress.ip_address(origin[1])
    except ValueError:
        return False
    if address.version == 6 and address.ipv4_mapped:
        address = address.ipv4_mapped
    return not address.is_loopback and any(address in network for network in lan)


def classify(record, counts, lan=()):
    message = record.get("MESSAGE")
    if not isinstance(message, str):
        counts["journal_record"] += 1
        return
    if record.get("_TRANSPORT") == "kernel" and "YUKI_EGRESS_DENIED" in message:
        counts["egress"] += 1
    if record.get("_SYSTEMD_UNIT") == BOT:
        rejection = re.fullmatch(r"SECURITY webhook_rejections count=([0-9]{1,16})", message)
        if rejection:
            counts["webhook"] = min(COUNT_MAX, counts["webhook"] + int(rejection[1]))
        if message.startswith("UNRESOLVED send "):
            counts["unresolved"] += 1
        if message.startswith(("Startup catch-up failed", "WARNING: catch-up reached")):
            counts["catchup"] += 1
        if message.startswith(("Discord destination unavailable", "Cannot reconcile interrupted sends",
                               "Discord rejected ", "Uncertain Discord send for ")):
            counts["dispatch"] += 1
    comm = record.get("_COMM")
    identifier = record.get("SYSLOG_IDENTIFIER")
    if comm in ("sshd", "sshd-session") or identifier == "sshd":
        if message.startswith(("Failed ", "Invalid user ", "error: PAM:", "maximum authentication attempts exceeded")) or "authentication failure" in message:
            counts["ssh_failure"] += 1
        if message.startswith("Accepted ") and not from_lan(message, lan):
            counts["ssh_success"] += 1
    if comm == "sudo" or identifier == "sudo":
        counts["sudo"] += 1


def observe_journal(state, observations, runner=bounded_command, lan=()):
    counts = dict.fromkeys(EVENT_KEYS, 0)
    try:
        if os.geteuid() != 0 and grp.getgrnam("systemd-journal").gr_gid not in os.getgroups():
            raise MonitorError("Journal group access is missing")
        raw, code, capped = runner(journal_command(state), maximum=MAX_JOURNAL)
        if code and not capped and state["cursor"]:
            raw, code, capped = runner(journal_command(state, cursor=False), maximum=MAX_JOURNAL)
            if not code or capped:
                counts["journal_gap"] += 1
        if code and not capped:
            raise MonitorError("Journal collection failed")
        lines = raw.splitlines(keepends=True)
        backlog = capped or len(lines) > MAX_RECORDS
        for line in lines[:MAX_RECORDS]:
            if not line.endswith(b"\n"):
                if capped:
                    backlog = True
                    break
                raise MonitorError("Journal output ended with an incomplete record")
            try:
                record = json.loads(line)
                cursor = record["__CURSOR"]
                stamp = int(record["__REALTIME_TIMESTAMP"]) / 1000000
                if not isinstance(cursor, str) or len(cursor) > 4096 or "\n" in cursor or not finite_number(stamp):
                    raise ValueError()
                fingerprint = hashlib.sha256(cursor.encode()).hexdigest()[:16]
                if cursor == state["cursor"] or fingerprint in state["recent_cursors"]:
                    continue
                classify(record, counts, lan)
                state["recent_cursors"].append(fingerprint)
                state["recent_cursors"] = state["recent_cursors"][-1024:]
                state["cursor"] = cursor
                state["journal_time"] = stamp
            except (KeyError, TypeError, ValueError, UnicodeError):
                counts["journal_record"] += 1
        observations["journal_backlog"] = int(backlog)
        observations["journal"] = 0
    except (MonitorError, KeyError, OSError):
        observations["journal"] = 1
    for key, count in counts.items():
        if count:
            observations[key] = min(COUNT_MAX, count)


def collect(state, directory, runner=bounded_command, lan=()):
    observations = {}
    observe_units(state, observations, runner)
    observations["health"] = int(not check_loopback(8787, "/healthz"))
    observations["ready"] = int(not check_loopback(20241, "/ready"))
    try:
        disk = shutil.disk_usage(directory)
        observations["disk"] = int(disk.free < 512 * 1024 * 1024 or disk.free < disk.total * 0.05)
    except OSError:
        observations["disk"] = 1
    observe_journal(state, observations, runner, lan)
    return observations


def update_incidents(state, observations, now):
    for key in LABELS:
        count = min(COUNT_MAX, max(0, observations.get(key, 0)))
        record = state["incidents"].setdefault(key, {
            "active": False, "delivered_active": False, "total": 0,
            "delivered_total": 0, "last_seen": 0, "last_sent": 0,
        })
        if count:
            record["total"] = min(COUNT_MAX, record["total"] + count)
            record["last_seen"] = now
            record["active"] = True
        elif key in EVENT_KEYS:
            # No false all-clear while collection is unavailable or behind.
            if not observations.get("journal") and not observations.get("journal_backlog"):
                record["active"] = record["active"] and now - record["last_seen"] < QUIET
        elif key in observations:
            record["active"] = False


def queue_pending(state, now):
    if state["pending"] is not None:
        return
    pending = {"reference": secrets.token_hex(12), "created": now, "uncertain": False, "items": []}
    # Recovery edges take priority over reminder messages; dictionary size is fixed.
    candidates = []
    for key, record in state["incidents"].items():
        undelivered = record["total"] - record["delivered_total"]
        if not record["active"] and record["delivered_active"] and undelivered == 0:
            candidates.append({"key": key, "active": False, "total": record["total"], "count": 0})
        elif undelivered > 0 and (not record["delivered_active"] or now - record["last_sent"] >= COOLDOWN or not record["active"]):
            candidates.append({"key": key, "active": True, "total": record["total"], "count": undelivered})
    candidates.sort(key=lambda item: (item["active"], state["incidents"][item["key"]]["last_sent"]))
    for item in candidates:
        pending["items"].append(item)
        if len(render(pending)) > 1800:
            pending["items"].pop()
            break
    if pending["items"]:
        state["pending"] = pending


def deliver_pending(state, store, discord, now):
    if not state["pending"]:
        return True
    if now < state["next_attempt"]:
        return False
    pending = state["pending"]
    was_uncertain = pending["uncertain"]
    # A crash after POST must be reconciled rather than blindly resent.
    pending["uncertain"] = True
    store.save(state)
    attempt = dict(pending, uncertain=was_uncertain)
    try:
        discord.deliver(attempt)
    except DeliveryError as error:
        pending["uncertain"] = was_uncertain or error.uncertain
        state["next_attempt"] = now + error.retry_after
        store.save(state)
        print("sekka-discord-monitor: " + str(error), file=sys.stderr)
        return False
    for item in pending["items"]:
        record = state["incidents"][item["key"]]
        record["delivered_total"] = item["total"]
        record["delivered_active"] = item["active"]
        record["last_sent"] = now
    state["pending"] = None
    state["next_attempt"] = 0
    store.save(state)
    return True


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--once", action="store_true", help="collect and deliver one bounded monitoring tick (default)")
    mode.add_argument("--smoke", action="store_true", help="send a labeled real verification DM and read it back; do not collect or change monitoring state")
    args = parser.parse_args(argv)
    os.umask(0o077)
    try:
        recipient = os.environ.get("DISCORD_ALERT_USER_ID", "")
        token_file = os.environ.get("DISCORD_TOKEN_FILE", "")
        directory = os.environ.get("SEKKA_MONITOR_STATE_DIR", "")
        if not numeric_id(recipient) or not token_file or not directory or not os.path.isabs(directory):
            raise MonitorError("Set numeric DISCORD_ALERT_USER_ID, DISCORD_TOKEN_FILE and absolute SEKKA_MONITOR_STATE_DIR")
        lan = lan_networks(os.environ.get("SEKKA_MONITOR_LAN_NETWORKS", ""))
        with Store(directory) as store:
            discord = Discord(read_token(token_file), recipient)
            if args.smoke:
                discord.smoke()
                print("sekka-discord-monitor: real verification DM sent and read back; monitoring state unchanged")
                return 0
            state = store.load()
            now = time.time()
            observations = collect(state, directory, lan=lan)
            update_incidents(state, observations, now)
            queue_pending(state, now)
            store.save(state)
            delivered = deliver_pending(state, store, discord, now)
            if observations.get("journal") or observations.get("journal_record"):
                print("sekka-discord-monitor: journal coverage incomplete; inspect local collection permissions/history", file=sys.stderr)
                return 1
            if not delivered:
                print("sekka-discord-monitor: pending alert retained for a later tick", file=sys.stderr)
                return 1
            print("sekka-discord-monitor: bounded monitoring tick completed")
            return 0
    except MonitorError as error:
        print("sekka-discord-monitor: " + str(error), file=sys.stderr)
    except Exception:
        # Neither traceback nor exception strings: upstream errors may contain secrets.
        print("sekka-discord-monitor: monitor failed; private state retained, check configuration and local service status", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
