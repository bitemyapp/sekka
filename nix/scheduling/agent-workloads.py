"""Keep AI harnesses interactive; isolate their child jobs using systemd.

Runs unprivileged, for processes already owned by this user's manager. No
cgroup files are written and no signals sent. Scopes stay tracked by systemd.
"""

import dataclasses
import os
from pathlib import Path
import subprocess
import sys
import time

AGENTS = {"codex", "claude", "claude-code", "gemini", "omp"}
UID = os.getuid()
MANAGER = f"/user@{UID}.service/"


@dataclasses.dataclass(frozen=True)
class Process:
    pid: int
    parent: int
    start: int
    agent: bool


def basename(value):
    name = os.path.basename(value)
    if name.startswith(".") and name.endswith("-wrapped"):
        name = name[1:-8]
    return name


def snapshot():
    processes = {}
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            if path.stat().st_uid != UID:
                continue
            stat = (path / "stat").read_text()
            comm, fields = stat[stat.index("(") + 1:].rsplit(") ", 1)
            fields = fields.split()
            if fields[0] == "Z":
                continue
            agent = basename(comm) in AGENTS
            if agent or basename(comm) in {"node", "bun", "MainThread"}:
                argv = (path / "cmdline").read_bytes().split(b"\0")
                names = [basename(a.decode(errors="replace")) for a in argv[:2]]
                agent = agent or any(n in AGENTS for n in names)
                if not agent:
                    agent = any(b"/gemini-cli/" in a or b"/claude-code/" in a or b"/oh-my-pi/" in a for a in argv[:2])
            processes[int(path.name)] = Process(int(path.name), int(fields[1]), int(fields[19]), agent)
        except (OSError, ValueError, IndexError):
            continue
    return processes


def group(pid):
    try:
        return Path(f"/proc/{pid}/cgroup").read_text().strip().removeprefix("0::")
    except OSError:
        return ""


def scopes(processes):
    """Nested harnesses are interactive roots; arbitrary job names work."""
    result = {}
    for process in processes.values():
        root = process
        seen = set()
        while not root.agent:
            if root.pid in seen:
                break
            seen.add(root.pid)
            parent = processes.get(root.parent)
            if parent is None or parent.agent:
                break
            root = parent
        if root.agent:
            kind = "agent"
        elif (parent := processes.get(root.parent)) is not None and parent.agent:
            kind = "work"
        else:
            continue
        name = f"app-sekka-{kind}-{root.pid}-{root.start}.scope"
        result.setdefault((name, kind), []).append(process)
    return result


def attach(busctl, name, kind, processes):
    pids = []
    owner = None
    for process in processes:
        current = group(process.pid)
        if MANAGER not in current or current.endswith("/" + name):
            continue
        # Leave delegated subgroups (e.g. containers) to their owner.
        unit = current.rsplit("/", 1)[-1]
        if not unit.endswith((".service", ".scope")):
            continue
        # Keep harnesses in their launcher's unit (and retain its lifecycle).
        # Only a harness launched inside one of our work scopes needs moving.
        if kind == "agent" and "/app-background.slice/app-sekka-work-" not in current:
            continue
        # Descendants already in a work scope stay there after reparenting.
        # A nested harness is the exception: it regains interactive priority.
        if kind == "work" and "/app-background.slice/app-sekka-work-" in current:
            continue
        try:
            fields = Path(f"/proc/{process.pid}/stat").read_text().rsplit(") ", 1)[1].split()
            if int(fields[19]) == process.start:
                pids.append(str(process.pid))
                owner = owner or unit
        except (OSError, ValueError, IndexError):
            pass
    if not pids:
        return True
    base = [busctl, "--user", "--timeout=2", "call", "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager"]
    slice_name = "app.slice" if kind == "agent" else "app-background.slice"
    properties = ["Slice", "s", slice_name, "PIDs", "au", str(len(pids)), *pids,
                  "CollectMode", "s", "inactive-or-failed", "Description", "s",
                  "Interactive AI harness" if kind == "agent" else "AI background workload",
                  "PartOf", "as", "1", owner]
    try:
        created = subprocess.run(base + ["StartTransientUnit", "ssa(sv)a(sa(sv))", name,
                                 "fail", "5", *properties, "0"], capture_output=True, timeout=3)
        if created.returncode:
            # A scope can already exist while newly observed children are
            # still in its parent's group. Attach via the manager's API.
            attached = subprocess.run(base + ["AttachProcessesToUnit", "ssau", name, "", str(len(pids)), *pids],
                                      capture_output=True, timeout=3)
            return attached.returncode == 0
        return True
    except subprocess.TimeoutExpired:
        return False


def main():
    retry = {}
    while True:
        processes = snapshot()
        groups = scopes(processes)
        now = time.monotonic()
        active = {name for name, _ in groups}
        retry = {name: until for name, until in retry.items() if name in active}
        for (name, kind), members in groups.items():
            if retry.get(name, 0) > now:
                continue
            if not attach(sys.argv[1], name, kind, members):
                if name not in retry:
                    print(f"Cannot attach {name}; retrying while its processes remain", file=sys.stderr, flush=True)
                retry[name] = now + 5
            else:
                retry.pop(name, None)
        time.sleep(0.25 if any(p.agent for p in processes.values()) else 2)


if __name__ == "__main__":
    main()
