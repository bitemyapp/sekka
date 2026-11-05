#!/usr/bin/env python3
"""Record the subjects' identities and the host's relevant state as JSON.

Usage: record-env.py OUTPUT.json (inside the shell.nix environment)
"""
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

SUBJECTS = {"hotaru": "TERMBENCH_HOTARU", "ghostty": "TERMBENCH_GHOSTTY", "gnome-terminal": "TERMBENCH_GNOME_TERMINAL",
            "konsole": "TERMBENCH_KONSOLE", "kgx": "TERMBENCH_KGX", "foot": "TERMBENCH_FOOT",
            "alacritty": "TERMBENCH_ALACRITTY", "kitty": "TERMBENCH_KITTY"}


def run(*command):
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=120).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return None


def store_path(path):
    parts = Path(path).parts
    return str(Path(*parts[:4])) if len(parts) >= 4 and parts[1:3] == ("nix", "store") else path


def main():
    repo = Path(__file__).resolve().parents[3]
    record = {"git_commit": run("git", "-C", str(repo), "rev-parse", "HEAD"),
              "git_status_clean": run("git", "-C", str(repo), "status", "--porcelain", "--", "desktop") == "",
              "kernel": platform.release(), "subjects": {}, "tools": {}}
    for name, variable in SUBJECTS.items():
        value = os.environ.get(variable)
        if not value:
            continue
        path = store_path(value)
        info = run("nix", "path-info", "--json", "--closure-size", path)
        entry = {"store_path": path, "entry": value}
        if info:
            data = json.loads(info)
            item = data[path] if isinstance(data, dict) else data[0]
            entry.update(nar_size=item.get("narSize"), closure_size=item.get("closureSize"))
        record["subjects"][name] = entry
    for tool in ("sway", "grim", "dbus-daemon", "python3", "cc"):
        record["tools"][tool] = {"path": run("sh", "-c", f"readlink -f $(command -v {tool})"),
                                 "version": (run(tool, "--version") or "").splitlines()[0] if run(tool, "--version") else None}
    cpu = Path("/proc/cpuinfo").read_text()
    record["cpu_model"] = next(line.split(":", 1)[1].strip() for line in cpu.splitlines() if line.startswith("model name"))
    record["logical_cpus"] = os.cpu_count()
    record["memory_kib"] = int(next(line.split()[1] for line in Path("/proc/meminfo").read_text().splitlines() if line.startswith("MemTotal")))
    record["governor"] = Path("/sys/devices/system/cpu/cpu2/cpufreq/scaling_governor").read_text().strip()
    epp = Path("/sys/devices/system/cpu/cpu2/cpufreq/energy_performance_preference")
    record["energy_performance_preference"] = epp.read_text().strip() if epp.exists() else None
    record["render_node_driver"] = Path("/sys/class/drm/renderD128/device/driver").resolve().name
    record["mesa_egl_vendor_file"] = "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json"
    record["mesa_driver"] = str(Path("/run/opengl-driver").resolve())
    Path(sys.argv[1]).write_text(json.dumps(record, indent=1) + "\n")


if __name__ == "__main__":
    main()
