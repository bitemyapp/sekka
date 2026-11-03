#!/usr/bin/env python3
"""The why-toolkits runs' environment: store paths and versions of every
subject, compositor and tool, the kernel, the GPU driver, the CPU.

    record_env.py OUT.json"""
import json
import os
import platform
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tk  # noqa: E402


def real(p):
    return os.path.realpath(p) if os.path.exists(p) else None


def version(path):
    m = re.search(r"/nix/store/[a-z0-9]{32}-(.+)$", path or "")
    return m.group(1) if m else None


def main():
    roots = {n: real(f"{tk.ROOTS}/{n}") for n in sorted(os.listdir(tk.ROOTS))}
    out = dict(
        kernel=platform.release(),
        cpu=next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None),
        governor=open("/sys/devices/system/cpu/cpu2/cpufreq/scaling_governor").read().strip(),
        epp=open("/sys/devices/system/cpu/cpu2/cpufreq/energy_performance_preference").read().strip(),
        gpu_render_node=tk.RENDER_NODE,
        gpu_vendor=open("/sys/class/drm/renderD128/device/vendor").read().strip(),
        mesa_egl_vendor=real(tk.MESA_EGL),
        radv_icd=real(tk.RADV),
        opengl_driver=real("/run/opengl-driver"),
        fonts=real(tk.FONTS),
        kit_binary=real(tk.KIT),
        pins={n: dict(path=p, name=version(p)) for n, p in roots.items()},
    )
    try:
        out["kit_commit"] = subprocess.run(["git", "-C", os.path.dirname(os.path.abspath(__file__)), "rev-parse",
                                            "HEAD"], capture_output=True, text=True).stdout.strip()
    except OSError:
        pass
    for lib in ("libwlroots-0.20.so",):
        try:
            ldd = subprocess.run(["ldd", f"{tk.ROOTS}/sway-unwrapped/bin/sway"], capture_output=True, text=True).stdout
            m = re.search(r"(/nix/store/\S+)/lib/" + re.escape(lib), ldd)
            out["wlroots"] = m.group(1) if m else None
        except OSError:
            pass
    out["mesa_driver_lib"] = next((f for f in os.listdir("/run/opengl-driver/lib") if f.startswith("libgallium")), None)
    with open(sys.argv[1], "w") as f:
        json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
