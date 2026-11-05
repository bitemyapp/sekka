"""Read-only lag evidence recorder; trigger labels are not a diagnosis of lag.

Only the version IPC request is sent to the authenticated session compositor.
The single background worker bounds process scans and filtered journal reads;
all sample timestamps describe actual observation, not an assumed one-second tick.
"""

import argparse
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import fcntl
import gzip
import json
import math
import os
from pathlib import Path
import re
import selectors
import signal
import socket
import stat
import struct
import subprocess
import sys
import time
import uuid


PROC = Path("/proc")
SYS = Path("/sys")
CGROUP = SYS / "fs/cgroup"
MAX_READ = 65536
LOG_BYTES = 8 * 1024 * 1024
LOG_BACKUPS = 4
JOURNAL_PATTERN = r"(?i)(watchdog|libinput|stall(?:ed|ing)?)"
VM_KEYS = {
    "pgfault", "pgmajfault", "pswpin", "pswpout", "oom_kill", "pgpgin", "pgpgout",
    "pgactivate", "pgdeactivate", "allocstall_normal", "allocstall_movable",
    "zswpin", "zswpout", "zswpwb", "compact_stall", "compact_fail", "compact_success",
}
VM_PREFIXES = ("pgscan_", "pgsteal_", "workingset_refault", "allocstall_")
MEM_LIMITS = ("memory.low", "memory.min", "memory.high", "memory.max",
              "memory.swap.high", "memory.swap.max", "memory.zswap.max")
DESKTOP_NAMES = {"aizome", "aizome-shell", "Xwayland", "pipewire", "wireplumber",
                 "ibus-daemon", "ibus-wayland", "xdg-desktop-por"}


def utc_now(wall=None):
    return datetime.fromtimestamp(time.time() if wall is None else wall, timezone.utc).isoformat()


def failure(exc):
    return {"status": "unavailable", "error": type(exc).__name__,
            "errno": getattr(exc, "errno", None), "message": str(exc)[:240]}


def read_text(path, limit=MAX_READ):
    """Bound every proc/sys read and retain truncation/permission errors."""
    try:
        with open(path, "rb") as source:
            data = source.read(limit + 1)
        return {"status": "truncated" if len(data) > limit else "ok",
                "value": data[:limit].decode("utf-8", errors="replace")}
    except OSError as exc:
        return failure(exc)


def parsed_read(path, parser, limit=MAX_READ):
    result = read_text(path, limit)
    if result["status"] != "ok":
        result.pop("value", None)
        return result
    try:
        result["value"] = parser(result["value"])
    except (ValueError, IndexError, KeyError, TypeError) as exc:
        return failure(exc)
    return result


def parse_pairs(text):
    result = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        key, value = line.split()
        result[key] = int(value)
    return result


def parse_meminfo(text):
    result = {}
    for line in text.splitlines():
        key, rest = line.split(":", 1)
        parts = rest.split()
        result[key] = int(parts[0]) * (1024 if len(parts) > 1 and parts[1] == "kB" else 1)
    return result


def parse_psi(text):
    result = {}
    for line in text.splitlines():
        scope, *fields = line.split()
        if scope not in {"some", "full"}:
            continue
        result[scope] = {}
        for field in fields:
            key, value = field.split("=", 1)
            number = int(value) if key == "total" else float(value)
            if not math.isfinite(number) or number < 0:
                raise ValueError("invalid pressure counter")
            result[scope][key] = number
    if not result:
        raise ValueError("no pressure counters")
    return result


def counter_delta(current, previous):
    if current is None or previous is None or current < previous:
        return None
    return current - previous


def derive_counters(current, previous, elapsed):
    previous = previous or {}
    delta, rates, unavailable = {}, {}, {}
    for key, value in current.items():
        diff = counter_delta(value, previous.get(key))
        delta[key] = diff
        rates[key] = diff / elapsed if diff is not None and elapsed and elapsed > 0 else None
        if key not in previous:
            unavailable[key] = "no_previous"
        elif diff is None:
            unavailable[key] = "counter_reset"
        elif not elapsed or elapsed <= 0:
            unavailable[key] = "invalid_interval"
    return {"delta": delta, "per_second": rates, "unavailable": unavailable}


def pressure_with_rates(current, previous, elapsed):
    for scope, values in current.items():
        old = (previous or {}).get(scope, {})
        delta = counter_delta(values.get("total"), old.get("total"))
        values["stall_percent"] = (delta / (elapsed * 10000)
                                   if delta is not None and elapsed and elapsed > 0 else None)
        values["delta_total_usec"] = delta
        if delta is None:
            values["interval_status"] = "no_previous" if "total" not in old else "counter_reset"
    return current


def parse_cpu(text):
    fields = text.splitlines()[0].split()
    if fields[0] != "cpu" or len(fields) < 9:
        raise ValueError("missing aggregate CPU counters")
    # guest/guest_nice are already included in user/nice; do not double count.
    names = ("user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal")
    return dict(zip(names, map(int, fields[1:9])))


def cpu_percentages(current, previous):
    deltas = {key: counter_delta(value, (previous or {}).get(key)) for key, value in current.items()}
    if any(value is None for value in deltas.values()):
        return {key: None for key in current}
    total = sum(deltas.values())
    return {key: value * 100 / total if total else None for key, value in deltas.items()}


def parse_diskstats(text):
    names = ("reads", "reads_merged", "sectors_read", "read_ms", "writes", "writes_merged",
             "sectors_written", "write_ms", "in_flight", "io_ms", "weighted_io_ms",
             "discards", "discards_merged", "sectors_discarded", "discard_ms", "flushes", "flush_ms")
    disks = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) < 14:
            raise ValueError("short diskstats row")
        major, minor, name = fields[:3]
        if re.match(r"^(loop|ram)\d", name):
            continue
        disks[f"{major}:{minor}:{name}"] = dict(zip(names, map(int, fields[3:])))
    return disks


def derive_disk(current, previous, elapsed):
    counters = {key: value for key, value in current.items() if key != "in_flight"}
    result = derive_counters(counters, previous, elapsed)
    rates, deltas = result["per_second"], result["delta"]
    result["read_bytes_per_second"] = (rates["sectors_read"] * 512
                                        if rates.get("sectors_read") is not None else None)
    result["write_bytes_per_second"] = (rates["sectors_written"] * 512
                                         if rates.get("sectors_written") is not None else None)
    result["busy_percent"] = rates["io_ms"] / 10 if rates.get("io_ms") is not None else None
    for operation, count, duration in (("read", "reads", "read_ms"), ("write", "writes", "write_ms"),
                                       ("discard", "discards", "discard_ms"), ("flush", "flushes", "flush_ms")):
        n, milliseconds = deltas.get(count), deltas.get(duration)
        result[f"{operation}_await_ms"] = milliseconds / n if n and milliseconds is not None else None
    combined = [deltas.get(key) for key in ("reads", "writes", "read_ms", "write_ms")]
    result["await_ms"] = None
    if all(value is not None for value in combined) and combined[0] + combined[1]:
        result["await_ms"] = (combined[2] + combined[3]) / (combined[0] + combined[1])
    return result


def parse_proc_stat(text):
    left, fields = text.rsplit(") ", 1)
    pid, comm = left.split(" (", 1)
    fields = fields.split()
    return {"pid": int(pid), "comm": comm, "state": fields[0], "ppid": int(fields[1]),
            "minor_faults": int(fields[7]), "major_faults": int(fields[9]),
            "cpu_ticks": int(fields[11]) + int(fields[12]), "start_ticks": int(fields[19]),
            "rss_pages": int(fields[21])}


def process_delta(current, previous, elapsed, hz):
    same = previous and (current["pid"], current["start_ticks"]) == (previous["pid"], previous["start_ticks"])
    keys = ("cpu_ticks", "minor_faults", "major_faults")
    result = derive_counters({key: current[key] for key in keys},
                             {key: previous[key] for key in keys} if same else None, elapsed)
    ticks = result["per_second"]["cpu_ticks"]
    result["cpu_percent_one_core"] = ticks * 100 / hz if ticks is not None else None
    result["identity_status"] = "same_process" if same else "new_identity"
    return result


def parse_io_stat(text):
    result = {}
    for line in text.splitlines()[:64]:
        device, *fields = line.split()
        result[device] = {key: int(value) for field in fields for key, value in [field.split("=", 1)]}
    return {"devices": result, "truncated_devices": len(text.splitlines()) > 64}


def cgroup_path(text):
    for line in text.splitlines():
        if line.startswith("0::/"):
            return line[3:]
    raise ValueError("unified cgroup path unavailable")


def process_details(pid, expected_start=None):
    root = PROC / str(pid)
    result = parsed_read(root / "stat", parse_proc_stat, 4096)
    if result["status"] != "ok":
        return {"pid": pid, **result}
    process = result["value"]
    if expected_start is not None and process["start_ticks"] != expected_start:
        return {"pid": pid, "status": "unavailable", "error": "pid_reused"}
    process["rss_bytes"] = process["rss_pages"] * os.sysconf("SC_PAGE_SIZE")
    status = parsed_read(root / "status", lambda text: {
        key: value for key, value in parse_meminfo("\n".join(
            line for line in text.splitlines() if line.startswith(("VmRSS:", "VmSwap:")))).items()
    }, 16384)
    process["memory_status"] = status
    process["wchan"] = parsed_read(root / "wchan", str.strip, 512)
    process["schedstat"] = parsed_read(root / "schedstat", lambda text: dict(zip(
        ("runtime_ns", "runqueue_wait_ns", "timeslices"), map(int, text.split()))), 512)
    process["cgroup"] = parsed_read(root / "cgroup", cgroup_path, 8192)
    try:
        process["exe"] = {"status": "ok", "value": os.readlink(root / "exe")[:4096]}
    except OSError as exc:
        process["exe"] = failure(exc)
    final = parsed_read(root / "stat", parse_proc_stat, 4096)
    if final["status"] != "ok" or final["value"]["start_ticks"] != process["start_ticks"]:
        return {"pid": pid, "status": "unavailable", "error": "process_exited_or_reused_during_read"}
    return {"status": "ok", **process}


def socket_candidates(runtime_dir, explicit=None, wayland_display=None):
    candidates = []
    if explicit:
        candidates.append(Path(explicit))
    root = Path(runtime_dir)
    if wayland_display and "/" not in wayland_display:
        candidates.append(root / f"aizome.{wayland_display}.sock")
    try:
        with os.scandir(root) as entries:
            for index, entry in enumerate(entries):
                if index >= 512:
                    break
                if (entry.name.startswith("aizome.") and entry.name.endswith(".sock")
                        and "headless" not in entry.name and "shell" not in entry.name):
                    candidates.append(Path(entry.path))
                    if len(candidates) >= 16:
                        break
    except OSError:
        pass
    return list(dict.fromkeys(candidates))[:16]


def probe_socket(path, allowed_pids, timeout=0.1):
    """Bounded read-only request, authenticated before sending any bytes.

    RTT includes recorder scheduling delay. Missing/refused/untrusted sockets
    are unavailable, not a compositor-stall measurement.
    """
    started = time.monotonic()
    deadline = started + timeout
    authenticated = False
    peer_pid = None
    try:
        info = os.stat(path, follow_symlinks=False)
        if not stat.S_ISSOCK(info.st_mode) or info.st_uid != os.getuid():
            return {"status": "unavailable", "error": "not_owned_socket"}
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(max(0.000001, deadline - time.monotonic()))
            client.connect(str(path))
            peer_pid, peer_uid, _ = struct.unpack("3i", client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            if peer_uid != os.getuid() or peer_pid not in allowed_pids:
                return {"status": "unavailable", "error": "untrusted_peer", "peer_pid": peer_pid}
            if isinstance(allowed_pids, dict):
                identity = parsed_read(PROC / str(peer_pid) / "stat", parse_proc_stat, 4096)
                group = parsed_read(PROC / str(peer_pid) / "cgroup", cgroup_path, 8192)
                if (identity["status"] != "ok" or identity["value"]["start_ticks"] != allowed_pids[peer_pid]
                        or group["status"] != "ok" or "aizome.service" not in group["value"].split("/")):
                    return {"status": "unavailable", "error": "peer_identity_changed", "peer_pid": peer_pid}
            authenticated = True
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("version request deadline exceeded")
            client.settimeout(remaining)
            client.sendall(b'{"request":"version"}\n')
            response = bytearray()
            while b"\n" not in response:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("version request deadline exceeded")
                client.settimeout(remaining)
                chunk = client.recv(min(4096, 16384 - len(response)))
                if not chunk:
                    return {"status": "unavailable", "error": "incomplete_reply", "peer_pid": peer_pid}
                response.extend(chunk)
                if len(response) >= 16384 and b"\n" not in response:
                    return {"status": "unavailable", "error": "reply_limit", "peer_pid": peer_pid}
            reply = json.loads(response.split(b"\n", 1)[0])
            if not isinstance(reply, dict) or reply.get("response") != "version":
                return {"status": "unavailable", "error": "unexpected_reply", "peer_pid": peer_pid}
            if "pid" in reply and reply["pid"] != peer_pid:
                return {"status": "unavailable", "error": "reply_pid_mismatch", "peer_pid": peer_pid}
            latency = time.monotonic() - started
            return {"status": "ok", "latency_seconds": latency, "peer_pid": peer_pid,
                    "version": {key: reply[key] for key in ("version", "protocol", "pid", "exe", "build") if key in reply}}
    except (TimeoutError, socket.timeout):
        return {"status": "timeout" if authenticated else "unavailable", "error": "deadline_exceeded",
                "latency_seconds": time.monotonic() - started, "peer_pid": peer_pid}
    except (OSError, ValueError, TypeError) as exc:
        return {**failure(exc), "peer_pid": peer_pid, "latency_seconds": time.monotonic() - started}


class Sampler:
    def __init__(self):
        self.uid = os.getuid()
        self.hz = os.sysconf("SC_CLK_TCK")
        self.previous = {}
        self.previous_disks = {}
        self.previous_processes = {}
        self.last_time = None
        self.manager = f"/user.slice/user-{self.uid}.slice/user@{self.uid}.service"
        self.session = self.manager + "/session.slice"
        self.paths = ["/", "/system.slice", "/system.slice/nix-daemon.service", "/user.slice",
                      f"/user.slice/user-{self.uid}.slice", self.manager, self.session,
                      self.session + "/aizome.service", self.manager + "/app.slice",
                      self.manager + "/app.slice/app-background.slice"]
        own = parsed_read(PROC / "self/cgroup", cgroup_path, 8192)
        if own["status"] == "ok":
            parts = own["value"].strip("/").split("/")
            self.paths.extend("/" + "/".join(parts[:end]) for end in range(1, len(parts) + 1))
        self.paths = list(dict.fromkeys(self.paths))[:32]
        self.socket_path = None
        self.next_socket_discovery = 0

    def pressure(self, path, elapsed):
        key = str(path)
        result = parsed_read(path, parse_psi, 4096)
        previous = self.previous.pop(key, None)
        if result["status"] == "ok":
            self.previous[key] = {scope: dict(values) for scope, values in result["value"].items()}
            result["value"] = pressure_with_rates(result["value"], previous, elapsed)
        return result

    def cgroups(self, elapsed):
        groups = {}
        for path in self.paths:
            root = CGROUP / path.lstrip("/")
            group = {}
            try:
                identity = root.stat().st_ino
                previous_identity = self.previous.get((path, "inode"))
                self.previous[(path, "inode")] = identity
                group["inode"] = identity
                if previous_identity != identity:
                    for name in ("memory.pressure", "cpu.pressure", "io.pressure"):
                        self.previous.pop(str(root / name), None)
            except OSError as exc:
                groups[path] = failure(exc)
                self.previous.pop((path, "inode"), None)
                for name in ("memory.pressure", "cpu.pressure", "io.pressure"):
                    self.previous.pop(str(root / name), None)
                continue
            for name in MEM_LIMITS:
                group[name] = parsed_read(root / name, lambda text: "max" if text.strip() == "max" else int(text), 128)
            for name in ("memory.current", "memory.swap.current", "memory.zswap.current"):
                group[name] = parsed_read(root / name, int, 128)
            for name in ("memory.stat", "memory.events", "memory.events.local", "cpu.stat"):
                group[name] = parsed_read(root / name, parse_pairs, 16384)
            for name in ("memory.pressure", "cpu.pressure", "io.pressure"):
                group[name] = self.pressure(root / name, elapsed)
            group["io.stat"] = parsed_read(root / "io.stat", parse_io_stat, 16384)
            groups[path] = group
        for path, group in groups.items():
            if "inode" not in group:
                continue
            ancestors = [ancestor for ancestor in groups if ancestor == "/" or path == ancestor or path.startswith(ancestor + "/")]
            effective = {}
            for name in ("memory.high", "memory.max", "memory.swap.high", "memory.swap.max", "memory.zswap.max"):
                values, missing = [], []
                for ancestor in ancestors:
                    # The root cgroup does not have memory resource limits.
                    if ancestor == "/":
                        continue
                    value = groups[ancestor].get(name, {})
                    if value.get("status") != "ok":
                        missing.append(ancestor)
                    elif isinstance(value["value"], int):
                        values.append(value["value"])
                effective[name] = {"known_ceiling": min(values) if values else "max",
                                   "complete": not missing, "unavailable_ancestors": missing}
            group["ancestor_limits"] = effective
            group["protection_ancestors"] = [ancestor for ancestor in ancestors if ancestor != "/"]
            group["protection_note"] = "memory.low/min are configured values; effective protection also depends on ancestor and sibling usage and memory_recursiveprot"
        return groups

    def aizome_processes(self, elapsed):
        session_root = CGROUP / self.session.lstrip("/")
        pending = [(session_root, 0), (session_root / "aizome.service", 1)]
        found, service_pids, errors = {}, {}, []
        visited_paths = set()
        visited = 0
        deadline = time.monotonic() + 0.05
        capped = False
        while pending:
            if visited >= 64 or len(found) >= 64 or time.monotonic() >= deadline:
                capped = True
                break
            root, depth = pending.pop()
            if root in visited_paths:
                continue
            visited_paths.add(root)
            visited += 1
            members = parsed_read(root / "cgroup.procs", lambda text: list(map(int, text.split())), 8192)
            if members["status"] != "ok":
                errors.append({"path": str(root.relative_to(CGROUP)), **members})
            else:
                for pid in members["value"][:256]:
                    if time.monotonic() >= deadline:
                        capped = True
                        break
                    process_root = PROC / str(pid)
                    try:
                        if process_root.stat().st_uid != self.uid:
                            continue
                    except OSError as exc:
                        if len(errors) < 16:
                            errors.append({"pid": pid, **failure(exc)})
                        continue
                    name = parsed_read(process_root / "comm", str.strip, 256)
                    if name["status"] != "ok":
                        if len(errors) < 16:
                            errors.append({"pid": pid, **name})
                        continue
                    in_service = "aizome.service" in root.parts
                    if not name["value"].startswith("aizome"):
                        continue
                    process = process_details(pid)
                    if process["status"] == "ok":
                        process["interval"] = process_delta(process, self.previous_processes.get(pid), elapsed, self.hz)
                        found[pid] = process
                        if (in_service and process["comm"] == "aizome"
                                and process["cgroup"].get("status") == "ok"
                                and "aizome.service" in process["cgroup"]["value"].split("/")):
                            service_pids[pid] = process["start_ticks"]
                    elif len(errors) < 16:
                        errors.append(process)
                    if len(found) >= 64 or time.monotonic() >= deadline:
                        capped = True
                        break
                if len(members["value"]) > 256:
                    capped = True
            if depth < 4:
                try:
                    with os.scandir(root) as entries:
                        for index, entry in enumerate(entries):
                            if index >= 128 or len(pending) >= 64:
                                capped = True
                                break
                            if entry.is_dir(follow_symlinks=False):
                                pending.append((Path(entry.path), depth + 1))
                except OSError as exc:
                    if len(errors) < 16:
                        errors.append({"path": str(root.relative_to(CGROUP)), **failure(exc)})
        self.previous_processes = found
        return {"processes": list(found.values()), "errors": errors[:16], "bounded_scan": capped}, service_pids

    def probe(self, allowed_pids, now):
        if not allowed_pids:
            self.socket_path = None
            return {"status": "unavailable", "error": "no_authenticated_session_compositor"}
        if self.socket_path is not None:
            result = probe_socket(self.socket_path, allowed_pids)
            if result["status"] in {"ok", "timeout"}:
                return result
            self.socket_path = None
            self.next_socket_discovery = now + 10
            return result
        if now < self.next_socket_discovery:
            return {"status": "unavailable", "error": "socket_discovery_backoff"}
        self.next_socket_discovery = now + 10
        started = time.monotonic()
        attempts = []
        runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{self.uid}")
        for path in socket_candidates(runtime, os.environ.get("AIZOME_SOCKET"), os.environ.get("WAYLAND_DISPLAY")):
            remaining = 0.1 - (time.monotonic() - started)
            if remaining <= 0:
                break
            result = probe_socket(path, allowed_pids, remaining)
            if result["status"] in {"ok", "timeout"}:
                self.socket_path = path
                return result
            attempts.append({"socket": str(path), **result})
        return {"status": "unavailable", "error": "session_socket_unavailable", "attempts": attempts}

    def sample(self, now):
        elapsed = now - self.last_time if self.last_time is not None else None
        self.last_time = now
        sample = {"elapsed_seconds": elapsed}
        sample["pressure"] = {name: self.pressure(PROC / "pressure" / name, elapsed) for name in ("cpu", "memory", "io")}
        cpu = parsed_read(PROC / "stat", parse_cpu, 262144)
        old_cpu = self.previous.pop("cpu", None)
        if cpu["status"] == "ok":
            cpu["percent"] = cpu_percentages(cpu["value"], old_cpu)
            self.previous["cpu"] = cpu["value"]
        sample["cpu"] = cpu
        sample["loadavg"] = parsed_read(PROC / "loadavg", lambda text: {
            "one": float(text.split()[0]), "five": float(text.split()[1]), "fifteen": float(text.split()[2]),
            "runnable": int(text.split()[3].split("/")[0]), "tasks": int(text.split()[3].split("/")[1])}, 512)
        sample["meminfo_bytes"] = parsed_read(PROC / "meminfo", parse_meminfo, 16384)
        memory = sample["meminfo_bytes"]
        if memory["status"] == "ok":
            memory["not_reported"] = [key for key in ("MemAvailable", "SwapFree", "Zswap", "Zswapped")
                                      if key not in memory["value"]]
        vm = parsed_read(PROC / "vmstat", lambda text: {key: value for key, value in parse_pairs(text).items()
                                                     if key in VM_KEYS or key.startswith(VM_PREFIXES)}, 65536)
        previous_vm = self.previous.pop("vmstat", None)
        if vm["status"] == "ok":
            vm["interval"] = derive_counters(vm["value"], previous_vm, elapsed)
            vm["not_reported"] = sorted(VM_KEYS - vm["value"].keys())
            self.previous["vmstat"] = vm["value"]
        sample["vmstat"] = vm
        disks = parsed_read(PROC / "diskstats", parse_diskstats, 65536)
        current_disks = {}
        if disks["status"] == "ok":
            entries, skipped = {}, []
            for identity, values in disks.pop("value").items():
                major, minor, name = identity.split(":", 2)
                try:
                    device = SYS / "dev/block" / f"{major}:{minor}"
                    inode = device.stat().st_ino
                    if (device / "partition").exists():
                        continue
                except OSError as exc:
                    skipped.append({"device": identity, **failure(exc)})
                    continue
                key = (identity, inode)
                entries[identity] = {"counters": values, "sysfs_inode": inode,
                                     "interval": derive_disk(values, self.previous_disks.get(key), elapsed)}
                current_disks[key] = values
                if len(entries) >= 64:
                    disks["truncated_devices"] = True
                    break
            disks["devices"] = entries
            disks["unavailable_devices"] = skipped[:16]
        self.previous_disks = current_disks
        sample["diskstats"] = disks
        sample["cgroups"] = self.cgroups(elapsed)
        sample["aizome"], allowed_pids = self.aizome_processes(elapsed)
        sample["compositor_ipc"] = self.probe(allowed_pids, now)
        return sample


class IncidentTracker:
    def __init__(self, interval, quiet_seconds=15, quiet_sample_seconds=10, history_size=60):
        self.interval = interval
        self.quiet_seconds = quiet_seconds
        self.quiet_sample_seconds = quiet_sample_seconds
        self.history = deque(maxlen=history_size)
        self.active = None
        self.last_trigger = None
        self.last_quiet_sample = None
        self.last_detail = None
        self.event_counter = 0

    def details_due(self, now):
        if self.last_detail is None or now - self.last_detail >= 30:
            self.last_detail = now
            return True
        return False

    def observe(self, sample, reasons, manual=False):
        now = sample["monotonic_seconds"]
        reasons = list(reasons)
        if manual and "manual" not in reasons:
            reasons.append("manual")
        records = []
        if reasons:
            self.last_trigger = now
            if self.active is None:
                self.event_counter += 1
                self.active = self.event_counter
                records.append({"type": "event_start", "event_id": self.active,
                                "monotonic_seconds": now, "reasons": reasons,
                                "note": "Recording trigger, not proof of user-visible lag or its cause"})
                for previous in self.history:
                    records.append({**previous, "type": "sample", "event_id": self.active, "context": "prehistory"})
        if self.active is not None:
            records.append({**sample, "type": "sample", "event_id": self.active,
                            "context": "incident", "trigger_reasons": reasons})
            if not reasons and now - self.last_trigger >= self.quiet_seconds:
                records.append({"type": "event_end", "event_id": self.active, "monotonic_seconds": now,
                                "quiet_seconds": now - self.last_trigger})
                self.active = None
                self.last_quiet_sample = now
        elif self.last_quiet_sample is None or now - self.last_quiet_sample >= self.quiet_sample_seconds:
            records.append({**sample, "type": "sample", "context": "quiet", "trigger_reasons": reasons})
            self.last_quiet_sample = now
        self.history.append(sample)
        return records


def trigger_reasons(sample, interval, memory_full=10, io_full=10, cpu_some=50):
    reasons = []
    for resource, scope, threshold in (("memory", "full", memory_full), ("io", "full", io_full),
                                        ("cpu", "some", cpu_some)):
        pressure = sample.get("pressure", {}).get(resource, {})
        avg = pressure.get("value", {}).get(scope, {}).get("avg10")
        if pressure.get("status") == "ok" and avg is not None and avg >= threshold:
            reasons.append(f"{resource}.{scope}.avg10>={threshold:g}%")
    if sample.get("sampling_gap_seconds", 0) >= max(1, 2 * interval):
        reasons.append("sampling_delay")
    probe = sample.get("compositor_ipc", {})
    if probe.get("status") == "timeout":
        reasons.append("compositor_ipc_timeout")
    elif probe.get("status") == "ok" and probe.get("latency_seconds", 0) >= 0.05:
        reasons.append("compositor_ipc_rtt>=50ms")
    return reasons


class PrivateLog:
    def __init__(self, output_dir):
        self.directory = Path(output_dir)
        self.directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        info = self.directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
            raise RuntimeError("output directory must be a real directory owned by the current user")
        self.directory.chmod(0o700)
        self.path = self.directory / "recorder.jsonl"
        self.lock = None
        self.stream = None
        try:
            self.lock = os.open(self.directory / "recorder.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
            try:
                fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as exc:
                raise RuntimeError("another lag recorder already owns this output directory") from exc
            os.fchmod(self.lock, 0o600)
            for path in [self.path, *(Path(f"{self.path}.{number}") for number in range(1, LOG_BACKUPS + 1))]:
                if path.is_symlink():
                    raise RuntimeError("refusing a symlink in recorder log files")
                if path.exists():
                    path.chmod(0o600)
            self._open()
        except BaseException:
            self.close()
            raise

    def _open(self):
        fd = os.open(self.path, os.O_CREAT | os.O_APPEND | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
        os.fchmod(fd, 0o600)
        self.size = os.fstat(fd).st_size
        self.stream = os.fdopen(fd, "ab", buffering=0)

    def write(self, record):
        data = (json.dumps(record, ensure_ascii=True, separators=(",", ":"), allow_nan=False) + "\n").encode()
        if len(data) > LOG_BYTES:
            raise ValueError("one recorder record exceeds the log size bound")
        if self.size and self.size + len(data) > LOG_BYTES:
            self.stream.close()
            self.stream = None
            for number in range(LOG_BACKUPS, 0, -1):
                source = self.path if number == 1 else Path(f"{self.path}.{number - 1}")
                if source.exists():
                    os.replace(source, Path(f"{self.path}.{number}"))
            self._open()
        self.stream.write(data)
        self.size += len(data)

    def close(self):
        if self.stream is not None:
            self.stream.close()
            self.stream = None
        if self.lock is not None:
            os.close(self.lock)
            self.lock = None

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()


def bounded_command(command, timeout=1, max_bytes=65536):
    """Bound both pipes; terminate only the journalctl child we create."""
    started = time.monotonic()
    output, errors = bytearray(), bytearray()
    process = None
    reason = None
    try:
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   stdin=subprocess.DEVNULL, close_fds=True)
        with selectors.DefaultSelector() as selector:
            for stream, target in ((process.stdout, output), (process.stderr, errors)):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, target)
            while selector.get_map():
                remaining = timeout - (time.monotonic() - started)
                if remaining <= 0:
                    reason = "timeout"
                    break
                events = selector.select(remaining)
                if not events:
                    reason = "timeout"
                    break
                for key, _ in events:
                    chunk = os.read(key.fileobj.fileno(), min(8192, max_bytes + 1 - len(output) - len(errors)))
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    key.data.extend(chunk)
                    if len(output) + len(errors) > max_bytes:
                        reason = "output_limit"
                        break
                if reason:
                    break
        if reason is not None:
            process.kill()
        try:
            code = process.wait(timeout=max(0.01, timeout - (time.monotonic() - started)))
        except subprocess.TimeoutExpired:
            process.kill()
            code = process.wait()
            reason = "timeout"
        return {"status": reason or ("ok" if code == 0 else "unavailable"), "returncode": code,
                "duration_seconds": time.monotonic() - started,
                "stderr": bytes(errors[:1024]).decode(errors="replace"),
                "stderr_truncated": len(errors) > 1024,
                "output": bytes(output[:max_bytes]).decode(errors="replace")}
    except OSError as exc:
        return failure(exc)
    finally:
        if process is not None:
            if process.poll() is None:
                process.kill()
                process.wait()
            for stream in (process.stdout, process.stderr):
                if stream is not None:
                    stream.close()


class DetailWorker:
    def __init__(self, settings):
        self.previous_processes = {}
        self.previous_scan_time = None
        self.journal_since = time.time() - 10
        self.journal_cursors = deque(maxlen=128)
        self.settings = settings
        self.hz = os.sysconf("SC_CLK_TCK")
        self.page_size = os.sysconf("SC_PAGE_SIZE")

    def scan(self, details):
        started = time.monotonic()
        elapsed = started - self.previous_scan_time if self.previous_scan_time is not None else None
        current, errors = {}, []
        visited = 0
        truncated = False
        try:
            with os.scandir(PROC) as entries:
                for entry in entries:
                    if not entry.name.isdigit():
                        continue
                    if visited >= 2048 or time.monotonic() - started >= 0.25:
                        truncated = True
                        break
                    visited += 1
                    result = parsed_read(Path(entry.path) / "stat", parse_proc_stat, 4096)
                    if result["status"] != "ok":
                        if len(errors) < 16:
                            errors.append({"pid": int(entry.name), **result})
                        continue
                    process = result["value"]
                    process["rss_bytes"] = process["rss_pages"] * self.page_size
                    process["interval"] = process_delta(process, self.previous_processes.get(process["pid"]), elapsed, self.hz)
                    current[process["pid"]] = process
        except OSError as exc:
            errors.append(failure(exc))
        self.previous_processes = current
        self.previous_scan_time = started
        if not details:
            return None
        top_memory = sorted(current.values(), key=lambda item: item["rss_bytes"], reverse=True)[:10]
        top_cpu = sorted((process for process in current.values()
                          if process["interval"]["cpu_percent_one_core"] is not None),
                         key=lambda item: item["interval"]["cpu_percent_one_core"], reverse=True)[:10]
        desktop = [process for process in current.values() if process["comm"] in DESKTOP_NAMES
                   or process["comm"].startswith("aizome")][:32]
        selected = {process["pid"]: process for process in top_memory + top_cpu + desktop}
        enriched = []
        for process in selected.values():
            if time.monotonic() - started >= 0.35:
                truncated = True
                break
            detail = process_details(process["pid"], process["start_ticks"])
            if detail["status"] == "ok":
                detail["interval"] = process["interval"]
            enriched.append(detail)
        return {"type": "process_details", "monotonic_seconds": started,
                "duration_seconds": time.monotonic() - started, "comparison_elapsed_seconds": elapsed,
                "visited": visited, "bounded_scan": truncated, "errors": errors,
                "top_memory_pids": [item["pid"] for item in top_memory],
                "top_cpu_pids": [item["pid"] for item in top_cpu],
                "desktop_pids": [item["pid"] for item in desktop], "processes": enriched}

    def journal(self):
        until = time.time()
        since = self.journal_since
        if until < since:
            since = until - 10
        # Overlap one second and deduplicate cursor identities across bounded reads.
        stamp = lambda value: datetime.fromtimestamp(value, timezone.utc).strftime("%Y-%m-%d %H:%M:%S.%f UTC")
        command = ["journalctl", "--user", "--unit=aizome.service", "--no-pager", "--quiet",
                   "--since=" + stamp(since - 1), "--until=" + stamp(until), "--grep=" + JOURNAL_PATTERN,
                   "--lines=20", "--output=json", "--output-fields=MESSAGE,__REALTIME_TIMESTAMP,__MONOTONIC_TIMESTAMP,_BOOT_ID,__CURSOR"]
        result = bounded_command(command)
        if (result.get("status") == "unavailable" and result.get("returncode") == 1
                and not result.get("stderr") and not result.get("output")):
            result["status"] = "no_matches"
        # Retry failed windows after transient journal failures. A byte-limited
        # window advances explicitly, rather than repeatedly reading one huge
        # message forever; the record identifies that evidence may be omitted.
        advance = result["status"] in {"ok", "no_matches", "output_limit"}
        if advance:
            self.journal_since = until
        messages, parse_errors = [], 0
        lines = result.pop("output", "").splitlines()[:20]
        for line in lines:
            try:
                item = json.loads(line)
                message = item.get("MESSAGE")
                if not isinstance(message, str) or not re.search(JOURNAL_PATTERN, message):
                    continue
                cursor = item.get("__CURSOR")
                cursor = cursor[:512] if isinstance(cursor, str) else None
                if cursor is not None and cursor in self.journal_cursors:
                    continue
                if cursor is not None:
                    self.journal_cursors.append(cursor)
                messages.append({"message": message[:2048], "message_truncated": len(message) > 2048,
                                 "realtime_usec": item.get("__REALTIME_TIMESTAMP"),
                                 "monotonic_usec": item.get("__MONOTONIC_TIMESTAMP"), "boot_id": item.get("_BOOT_ID")})
            except (ValueError, TypeError, AttributeError):
                parse_errors += 1
        return {"type": "journal", "since_utc": utc_now(since), "until_utc": utc_now(until),
                "messages": messages, "parse_errors": parse_errors, "line_limit": 20,
                "possible_line_limit": len(lines) == 20, "window_advanced": advance,
                "window_truncated": result["status"] == "output_limit" or parse_errors > 0, **result}

    def metadata(self):
        uname = os.uname()
        params = {}
        root = SYS / "module/zswap/parameters"
        try:
            with os.scandir(root) as entries:
                for index, entry in enumerate(entries):
                    if index >= 32:
                        params["_truncated"] = True
                        break
                    if entry.is_file(follow_symlinks=False):
                        params[entry.name] = parsed_read(entry.path, str.strip, 4096)
        except OSError as exc:
            params = failure(exc)
        config = None
        config_errors = []
        for path in (PROC / "config.gz", Path("/run/booted-system/kernel-modules/lib/modules") / uname.release / "build/.config"):
            try:
                opener = gzip.open if path.suffix == ".gz" else open
                with opener(path, "rb") as source:
                    raw = source.read(1024 * 1024 + 1)
                wanted = ("CONFIG_PSI", "CONFIG_MEMCG", "CONFIG_CGROUP", "CONFIG_ZSWAP", "CONFIG_ZSMALLOC",
                          "CONFIG_ZPOOL", "CONFIG_SWAP", "CONFIG_PREEMPT", "CONFIG_HZ", "CONFIG_SCHEDSTATS")
                config = {"status": "truncated" if len(raw) > 1024 * 1024 else "ok", "source": str(path),
                          "lines": [line for line in raw[:1024 * 1024].decode(errors="replace").splitlines()
                                    if line.removeprefix("# ").startswith(wanted)][:128]}
                break
            except (OSError, EOFError) as exc:
                config_errors.append({"source": str(path), **failure(exc)})
        return {"type": "metadata", "kernel": {"release": uname.release, "version": uname.version,
                "machine": uname.machine}, "proc_version": parsed_read(PROC / "version", str.strip, 4096),
                "kernel_config": config or {"status": "unavailable", "attempts": config_errors},
                "zswap_parameters": params, "swappiness": parsed_read(PROC / "sys/vm/swappiness", int, 128),
                "cgroup_mounts": parsed_read(PROC / "mounts", lambda text: [line for line in text.splitlines()
                                                        if len(line.split()) > 2 and line.split()[2] == "cgroup2"], 65536),
                "clock_ticks_per_second": self.hz, "page_size": self.page_size, "settings": self.settings,
                "ipc_note": "Version-only request; RTT includes client scheduling. Pressure and gaps alone do not prove visible lag.",
                "vmstat_units": "pages except pgpgin/pgpgout (KiB), faults/events are counts; zswap counts are pages",
                "disk_note": "Whole block devices, no loop/ram/partitions; stacked devices may double count. Disk sectors are 512 bytes."}

    def collect(self, journal=False, details=False, metadata=False, event_id=None):
        started = time.monotonic()
        reports = []
        scan = self.scan(details)
        if scan is not None:
            reports.append({**scan, "event_id": event_id})
        if metadata:
            reports.append(self.metadata())
        if journal:
            reports.append(self.journal())
        return {"reports": reports, "duration_seconds": time.monotonic() - started}


def finite_interval(value):
    number = float(value)
    if not math.isfinite(number) or number < 0.1:
        raise argparse.ArgumentTypeError("interval must be finite and at least 0.1 seconds")
    return number


def threshold(value):
    number = float(value)
    if not math.isfinite(number) or not 0 <= number <= 100:
        raise argparse.ArgumentTypeError("threshold must be a finite percentage from 0 to 100")
    return number


def positive_count(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("samples must be positive")
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    default_state = Path(os.environ.get("XDG_STATE_HOME", str(Path.home() / ".local/state")))
    parser.add_argument("--output-dir", type=Path, default=default_state / "sekka/lag-recorder")
    parser.add_argument("--interval", type=finite_interval, default=1.0)
    parser.add_argument("--samples", type=positive_count)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--memory-full-threshold", type=threshold, default=10.0)
    parser.add_argument("--io-full-threshold", type=threshold, default=10.0)
    parser.add_argument("--cpu-some-threshold", type=threshold, default=50.0)
    args = parser.parse_args(argv)
    if args.once:
        args.samples = 1
    os.umask(0o077)
    stopping = False
    manual = False

    def stop(signum, frame):
        nonlocal stopping
        stopping = True

    def mark(signum, frame):
        nonlocal manual
        manual = True

    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, stop)
    signal.signal(signal.SIGUSR1, mark)
    run_id = str(uuid.uuid4())
    boot = parsed_read(PROC / "sys/kernel/random/boot_id", str.strip, 128)
    settings = {"interval_seconds": args.interval, "memory_full_avg10_threshold": args.memory_full_threshold,
                "io_full_avg10_threshold": args.io_full_threshold, "cpu_some_avg10_threshold": args.cpu_some_threshold,
                "sampling_gap_threshold_seconds": max(1, 2 * args.interval), "quiet_sample_seconds": 10,
                "history_samples": 60, "incident_quiet_seconds": 15, "details_cooldown_seconds": 30,
                "journal_seconds": 10, "metadata_seconds": 300, "log_bytes": LOG_BYTES,
                "log_backups": LOG_BACKUPS, "ipc_deadline_seconds": 0.1, "ipc_trigger_seconds": 0.05}
    sampler, tracker, worker = Sampler(), IncidentTracker(args.interval), DetailWorker(settings)
    executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="lag-details")
    future = None
    next_journal = next_metadata = 0
    pending_details = False
    pending_event = None
    count = 0
    last_time = None
    deadline = time.monotonic()
    pending_reasons = []
    try:
        with PrivateLog(args.output_dir) as logger:
            def emit(record):
                logger.write({"schema": 1, "run_id": run_id, "boot_id": boot.get("value"),
                              "wall_utc": utc_now(), "monotonic_seconds": time.monotonic(), **record})

            def receive(result):
                for report in result["reports"]:
                    emit(report)
                    if report["type"] == "journal" and report.get("messages"):
                        pending_reasons.append("aizome_watchdog_libinput_stall_journal")
                emit({"type": "worker_timing", "duration_seconds": result["duration_seconds"]})

            emit({"type": "recorder_start", "pid": os.getpid(), "settings": settings, "boot_id_read": boot})
            while not stopping and (args.samples is None or count < args.samples):
                now = time.monotonic()
                if now < deadline and not manual:
                    time.sleep(min(deadline - now, 0.1))
                    continue
                started = time.monotonic()
                regular_tick = started >= deadline
                wall = utc_now()
                if future is not None and future.done():
                    try:
                        receive(future.result())
                    except Exception as exc:
                        emit({"type": "worker_error", **failure(exc)})
                    future = None
                sample = sampler.sample(started)
                count += 1
                elapsed = started - last_time if last_time is not None else None
                last_time = started
                sample.update({"sample_id": count, "wall_utc": wall, "monotonic_seconds": started,
                               "elapsed_seconds": elapsed,
                               "sampling_gap_seconds": max(0, elapsed - args.interval) if elapsed is not None else 0,
                               "schedule_lateness_seconds": max(0, started - deadline),
                               "sample_duration_seconds": time.monotonic() - started,
                               "worker_busy": future is not None})
                reasons = trigger_reasons(sample, args.interval, args.memory_full_threshold,
                                          args.io_full_threshold, args.cpu_some_threshold) + pending_reasons
                pending_reasons.clear()
                manual_now = manual
                manual = False
                records = tracker.observe(sample, reasons, manual_now)
                for record in records:
                    emit(record)
                if (reasons or manual_now) and tracker.details_due(started):
                    pending_details = True
                    pending_event = tracker.active
                if future is None and (started >= next_journal or started >= next_metadata or pending_details):
                    journal_due, metadata_due = started >= next_journal, started >= next_metadata
                    future = executor.submit(worker.collect, journal_due, pending_details, metadata_due, pending_event)
                    if journal_due:
                        next_journal = started + 10
                    if metadata_due:
                        next_metadata = started + 300
                    pending_details = False
                finished = time.monotonic()
                if regular_tick:
                    deadline += args.interval
                if deadline <= finished:
                    deadline = finished
            if future is not None:
                try:
                    receive(future.result())
                except Exception as exc:
                    emit({"type": "worker_error", **failure(exc)})
            if pending_details:
                try:
                    receive(executor.submit(worker.collect, False, True, False, pending_event).result())
                except Exception as exc:
                    emit({"type": "worker_error", **failure(exc)})
            if count and not any(record.get("type") == "sample" and record.get("sample_id") == count
                                 for record in records):
                emit({**sample, "type": "sample", "context": "shutdown"})
            if tracker.active is not None:
                emit({"type": "event_end", "event_id": tracker.active, "reason": "shutdown"})
            emit({"type": "recorder_stop", "samples": count, "reason": "signal" if stopping else "sample_limit"})
    except (OSError, RuntimeError, ValueError) as exc:
        print(f"sekka-lag-recorder: {exc}", file=sys.stderr)
        return 1
    finally:
        executor.shutdown(wait=True, cancel_futures=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
