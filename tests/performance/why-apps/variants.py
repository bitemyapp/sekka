#!/usr/bin/env python3
"""Write the variant lists probe.py runs, one set at a time.

    variants.py SET HELLO KIT AIZOME GTE KATE FIXTURES STAMPS > set.json

HELLO     the GTK and Qt hello programs (design/hello), bin/ inside
KIT       the kit's hello program
AIZOME    the aizome package
GTE KATE  gnome-text-editor and kate (KWrite) from Sekka's Nixpkgs
FIXTURES  design's fixtures (editor-fixture.rs)
STAMPS    stamps.so

Sets:
  ablate      each hello and each editor, as measured on design.md (the
              session's environment) and with one part taken away at a time
  stamps      every program with stamps.so, for the timeline of its start
  perf        every program, as measured and "lean" (everything ablate found
              removable taken away), under perf record (CPU samples with
              DWARF call chains, CLOCK_MONOTONIC time stamps)
  mem         every program as measured, for smaps after it settles
  memablate   every program as measured, with the ablations that change
              most (lean, cairo, radv-only, software, no-gtk3-theme) and
              with one font package, for smaps after it settles
  heaptrack   every program under heaptrack (needs heaptrack on PATH)
  strace      every program under strace -f, exiting at its first frame
              (stamps.so, STAMPS_EXIT=1) where it uses libwayland
  wldebug     every program with WAYLAND_DEBUG=1 and stamps.so
  fonts       every program as measured and with FONTCONFIG_FILE naming a
              configuration with the same rules but one font package
              (DejaVu): FONTS_CONF, which run.sh writes
  pstat       every program (as measured, lean, and with its GPU ablations)
              under perf stat, exiting at its first frame: user and system
              time, page faults
  kitfonts    the kit's hello and Sumi with and without the session's
              AIZOME_FONT_DIR and with one font package (time and memory)
  lddebug     every program with LD_DEBUG=statistics (the dynamic linker's
              relocation counts and cycles at start-up, on stderr)
"""
import json
import os
import sys

RADV = "/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json"


def unwrap(cmd):
    """A Nix wrapper's program and the environment it sets, from its execve
    under strace: heaptrack doesn't follow exec, so it runs the wrapped
    program directly with the wrapper's variables."""
    import re
    import subprocess
    import tempfile
    with tempfile.NamedTemporaryFile("r", suffix=".strace") as f:
        env = {k: v for k, v in os.environ.items() if k not in ("WAYLAND_DISPLAY", "DISPLAY")}
        subprocess.run(["strace", "-f", "-e", "trace=execve", "-v", "-s", "1000000", "-o", f.name, cmd[0], "--help"],
                       env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
        calls = [l for l in f.read().splitlines() if "execve(" in l and "= 0" in l]
    if len(calls) < 2:
        return cmd, {}
    m = re.search(r'execve\("([^"]+)", \[.*?\], \[(.*)\]\) = 0', calls[-1])
    target, envs = m.group(1), m.group(2)
    final = dict(kv.encode().decode("unicode_escape").split("=", 1)
                 for kv in re.findall(r'"((?:[^"\\]|\\.)*)"', envs) if "=" in kv)
    diff = {k: v for k, v in final.items() if os.environ.get(k) != v}
    return [target] + cmd[1:], diff


def programs(hello, kit, aizome, gte, kate, fx):
    return {
        "kit": [kit],
        "gtk4": [f"{hello}/bin/hello-gtk4"],
        "adw": [f"{hello}/bin/hello-adw"],
        "qtwidgets": [f"{hello}/bin/hello-qtwidgets"],
        "qtquick": [f"{hello}/bin/hello-qtquick"],
        "sumi": [f"{aizome}/bin/aizome-sumi", f"{fx}/editor-fixture.rs"],
        "gnome-text-editor": [f"{gte}/bin/gnome-text-editor", f"{fx}/editor-fixture.rs"],
        "kwrite": [f"{kate}/bin/kwrite", f"{fx}/editor-fixture.rs"],
    }


# One part of a toolkit's start taken away at a time. None unsets a
# variable the session's environment passes on.
GTK = {
    "cairo": {"GSK_RENDERER": "cairo"},
    "gl": {"GSK_RENDERER": "gl"},
    "radv-only": {"VK_DRIVER_FILES": RADV},
    "no-a11y": {"GTK_A11Y": "none"},
    "no-portals": {"GDK_DEBUG": "no-portals"},
    "no-gvfs": {"GIO_EXTRA_MODULES": None},
    "bind-now": {"LD_BIND_NOW": "1"},
    "lean": {"GSK_RENDERER": "cairo", "GTK_A11Y": "none", "GDK_DEBUG": "no-portals", "GIO_EXTRA_MODULES": None},
}
QT = {
    "no-gtk3-theme": {"QT_QPA_PLATFORMTHEME": None},
    "no-ibus": {"QT_IM_MODULE": None},
    "no-a11y": {"QT_ACCESSIBILITY": "0"},
    "bind-now": {"LD_BIND_NOW": "1"},
    "lean": {"QT_QPA_PLATFORMTHEME": None, "QT_IM_MODULE": None, "QT_ACCESSIBILITY": "0"},
}
QUICK = dict(QT, **{
    "software": {"QT_QUICK_BACKEND": "software"},
    "basic-style": {"QT_QUICK_CONTROLS_STYLE": "Basic"},
    "lean": {"QT_QPA_PLATFORMTHEME": None, "QT_IM_MODULE": None, "QT_ACCESSIBILITY": "0",
             "QT_QUICK_BACKEND": "software"},
})
KIT = {"bind-now": {"LD_BIND_NOW": "1"}}
ABLATIONS = {"kit": KIT, "gtk4": GTK, "adw": GTK, "qtwidgets": QT, "qtquick": QUICK, "sumi": KIT,
             "gnome-text-editor": GTK, "kwrite": QT}


def main():
    which, hello, kit, aizome, gte, kate, fx, stamps = sys.argv[1:9]
    progs = programs(hello, kit, aizome, gte, kate, fx)
    st = {"LD_PRELOAD": stamps, "STAMPS_FILE": "{out}/{tag}.stamps"}
    out = []
    if which == "ablate":
        for name, cmd in progs.items():
            out.append({"name": name, "cmd": cmd})
            for a, env in ABLATIONS[name].items():
                out.append({"name": f"{name}.{a}", "cmd": cmd, "env": env})
    elif which == "stamps":
        for name, cmd in progs.items():
            out.append({"name": name, "cmd": cmd, "env": st})
    elif which == "perf":
        # As measured, and with everything ablate finds removable taken
        # away ("lean"), to profile what is left.
        todo = []
        for name, cmd in progs.items():
            todo.append((name, cmd, {}))
            if "lean" in ABLATIONS[name]:
                todo.append((f"{name}.lean", cmd, ABLATIONS[name]["lean"]))
        # perffonts: the kit's programs with one font package, to see what
        # is left of their start without the system's 1,623 faces.
        if os.environ.get("PERF_FONTS"):
            todo = [(f"{n}.dejavu-only", progs[n], {"FONTCONFIG_FILE": os.environ["FONTS_CONF"]})
                    for n in ("kit", "sumi")]
        for name, cmd, env in todo:
            out.append({"name": name, "env": dict(st, **env), "kill": "window",
                        "cmd": ["perf", "record", "-q", "-o", "{out}/{tag}.perf", "-k", "mono", "-e", "cpu-clock:u",
                                "-c", "100000", "--user-callchains", "--call-graph", "dwarf,32768"] + cmd})
    elif which == "mem":
        for name, cmd in progs.items():
            out.append({"name": name, "cmd": cmd})
    elif which == "memablate":
        # Memory with the parts the ablations found expensive taken away.
        for name, cmd in progs.items():
            for a in [""] + [a for a in ("lean", "cairo", "radv-only", "software", "no-gtk3-theme")
                             if a in ABLATIONS[name]]:
                out.append({"name": f"{name}.{a}" if a else name, "cmd": cmd,
                            "env": ABLATIONS[name][a] if a else {}})
            out.append({"name": f"{name}.dejavu-only", "cmd": cmd,
                        "env": {"FONTCONFIG_FILE": os.environ["FONTS_CONF"]}})
    elif which == "heaptrack":
        for name, cmd in progs.items():
            real, env = unwrap(cmd)
            out.append({"name": name, "kill": "window", "env": env,
                        "cmd": ["heaptrack", "--record-only", "-o", "{out}/{tag}.heaptrack"] + real})
    elif which == "strace":
        for name, cmd in progs.items():
            out.append({"name": name, "env": dict(st, STAMPS_EXIT="1"), "kill": "window",
                        "cmd": ["strace", "-f", "-tt", "-T", "-y", "-o", "{out}/{tag}.strace"] + cmd})
    elif which == "wldebug":
        for name, cmd in progs.items():
            out.append({"name": name, "cmd": cmd, "env": dict(st, WAYLAND_DEBUG="1")})
    elif which == "fonts":
        for name, cmd in progs.items():
            out.append({"name": name, "cmd": cmd})
            out.append({"name": f"{name}.dejavu-only", "cmd": cmd,
                        "env": {"FONTCONFIG_FILE": os.environ["FONTS_CONF"]}})
    elif which == "pstat":
        # perf stat to the first frame: the process exits right after its
        # first commit of a buffer (stamps.so, STAMPS_EXIT=1; the kit, which
        # doesn't use libwayland, is stopped at the window event instead),
        # so rusage's user and system time (1 kHz tick sampling, scaled to
        # the exact run time) and the page faults cover the start alone.
        for name, cmd in progs.items():
            for a in [""] + [a for a in ("lean", "cairo", "radv-only", "software", "no-gtk3-theme")
                             if a in ABLATIONS[name]]:
                env = dict(st, STAMPS_EXIT="1", **(ABLATIONS[name][a] if a else {}))
                out.append({"name": f"{name}.{a}" if a else name, "env": env, "kill": "window",
                            "cmd": ["perf", "stat", "-o", "{out}/{tag}.pstat", "-e",
                                    "task-clock,page-faults,major-faults,context-switches,cycles:u,instructions:u",
                                    "--"] + cmd})
    elif which == "kitfonts":
        # The kit's two font costs apart: the faces it reads into memory
        # from the session's AIZOME_FONT_DIR, and the system's 1,623 faces
        # it asks fontconfig about.
        few = {"FONTCONFIG_FILE": os.environ["FONTS_CONF"]}
        for name in ("kit", "sumi"):
            out.append({"name": name, "cmd": progs[name]})
            out.append({"name": f"{name}.no-font-dir", "cmd": progs[name], "env": {"AIZOME_FONT_DIR": None}})
            out.append({"name": f"{name}.dejavu-only", "cmd": progs[name], "env": few})
            out.append({"name": f"{name}.dejavu-only.no-font-dir", "cmd": progs[name],
                        "env": dict(few, AIZOME_FONT_DIR=None)})
    elif which == "lddebug":
        for name, cmd in progs.items():
            out.append({"name": name, "cmd": cmd, "env": {"LD_DEBUG": "statistics"}})
    else:
        sys.exit(f"unknown set {which}")
    json.dump(out, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
