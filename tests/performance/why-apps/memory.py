#!/usr/bin/env python3
"""Break a program's memory down by what each mapping holds, from probe.py's
`mem` set (NAME.PID.smaps, .sp and .status, 3 s after the first frame).

    memory.py RUNDIR NAME [NAME...]          tables, in Markdown
    memory.py RUNDIR NAME... --json OUT.json the same, as data

Every mapping of every process of the run is put in one category:

- each shared object's segments, by permission: code (r-x), read-only data
  and RELRO (r--: what the dynamic linker relocated and then protected,
  .data.rel.ro and the GOT, is the dirty part), writable data (rw-: .data,
  .got.plt, and the .bss beside it);
- the main thread's [stack], and the anonymous mappings holding another
  thread's stack pointer;
- malloc: [heap] (the main arena) and per-thread arenas (64 MiB-aligned
  anonymous regions with their reserve);
- GPU: mappings of /dev/dri render nodes and GEM objects;
- shared memory (memfd, /dev/shm: Wayland buffers, Mesa's caches);
- fonts and font caches; other files (locale archive, GResource, icon
  caches, Mesa's shader cache, Qt's caches);
- other anonymous mappings (large malloc chunks, Mesa's and LLVM's own
  mmaps, Rust's allocations above the mmap threshold, JIT memory).

For each category: Pss, Anonymous (the anonymous pages, which smaps_rollup
sums as Pss_Anon, the "private" column of design.md: a process's own
writes, including the relocated pages of its libraries), Private_Dirty and
Rss, in KiB.
"""
import collections
import glob
import json
import os
import re
import sys

FIELDS = ("Rss", "Pss", "Pss_Anon", "Pss_File", "Pss_Shmem", "Private_Clean", "Private_Dirty", "Shared_Clean",
          "Shared_Dirty", "Anonymous", "Swap")


def parse_smaps(path):
    maps, cur = [], None
    head = re.compile(r"^([0-9a-f]+)-([0-9a-f]+) (\S{4}) ([0-9a-f]+) \S+ (\d+)\s*(.*)$")
    for line in open(path):
        m = head.match(line)
        if m:
            cur = {"start": int(m.group(1), 16), "end": int(m.group(2), 16), "perms": m.group(3),
                   "offset": int(m.group(4), 16), "inode": int(m.group(5)), "path": m.group(6).strip()}
            maps.append(cur)
            continue
        parts = line.split()
        if cur is not None and len(parts) >= 3 and parts[2] == "kB":
            cur[parts[0].rstrip(":")] = int(parts[1])
    return maps


def lib_name(path):
    b = os.path.basename(path)
    return re.sub(r"\.so[.\d]*$", ".so", b)


def library_group(path):
    b = os.path.basename(path)
    rules = [
        ("GPU drivers (Mesa, LLVM, NVIDIA, Vulkan loader)", r"libvulkan|libgallium|libLLVM|libnvidia|libGLX_nvidia|libEGL|libGLESv2|libGLX|libGL\.|libGLdispatch|libgbm|libdrm|libVkLayer|libelf|libz3|libSPIRV|libclang|libxcb|libX11|libXext|libxshmfence|libsensors|libwayland-server"),
        ("Qt", r"^libQt6|^libq[a-z]|^libKF6|^libKPim|plugins/"),
        ("GTK, libadwaita, GDK, GSK", r"^libgtk-4|^libadwaita|^libgraphene|^libepoxy|^libappstream"),
        ("GTK 3 (Qt's gtk3 theme)", r"^libgtk-3|^libgdk-3|^libatk|^libgdk_pixbuf"),
        ("GLib, GObject, GIO", r"^libglib|^libgobject|^libgio|^libgmodule|^libgthread"),
        ("fonts and text (fontconfig, FreeType, HarfBuzz, Pango, ICU)", r"^libfontconfig|^libfreetype|^libharfbuzz|^libpango|^libfribidi|^libicu|^libthai|^libdatrie|^libgraphite"),
        ("cairo, pixman, image codecs", r"^libcairo|^libpixman|^libpng|^libjpeg|^libtiff|^libwebp|^librsvg|^libgdk_pixbuf|^libjxl|^libavif|^liblcms|^libLerc|^libsharpyuv|^libdeflate"),
        ("C and C++ runtime", r"^libc\.so|^ld-linux|^libm\.so|^libstdc\+\+|^libgcc_s|^libpthread|^libdl|^librt"),
        ("D-Bus, a11y, portals, input methods", r"^libdbus|^libatspi|^libibus|^libsystemd|^libgvfs|^libdconf|^libgiognutls|^libgnutls|^libproxy"),
        ("Wayland, xkbcommon", r"^libwayland|^libxkbcommon|^libffi"),
    ]
    for g, rx in rules:
        if re.search(rx, b) or re.search(rx, path):
            return g
    return "other libraries"


def categorise(m, stacks, arenas, execs=()):
    p = m["path"]
    perms = m["perms"]
    if p == "[heap]":
        return "malloc: main arena ([heap])"
    if p == "[stack]":
        return "thread stacks"
    if p in ("[vdso]", "[vvar]", "[vsyscall]", "[vvar_vclock]"):
        return "kernel ([vdso], [vvar])"
    if m["start"] in stacks:
        return "thread stacks"
    if m["start"] in arenas:
        return "malloc: thread arenas"
    if p.startswith("/dev/dri") or "drm mm object" in p or p.startswith("anon_inode:dmabuf") or "/dmabuf:" in p:
        return "GPU buffer mappings"
    if p.startswith("/memfd:") or p.startswith("/dev/shm") or p.startswith("/SYSV") or "(deleted)" in p and "memfd" in p:
        return "shared memory (memfd: Wayland buffers, caches)"
    if p in execs and ".so" not in os.path.basename(p):
        return "program binary (code and data)"
    if ".so" in os.path.basename(p) and p.startswith("/"):
        if "x" in perms:
            return "library code (r-x)"
        if perms.startswith("r--"):
            return "library read-only data and RELRO (r--)"
        if perms.startswith("rw"):
            return "library writable data (rw-)"
        return "library other"
    if not p and perms.startswith("rw"):
        return "other anonymous (large malloc chunks, driver and runtime mmaps)"
    if not p:
        return "anonymous reserve (---)"
    if re.search(r"\.(ttf|otf|ttc|pfb|woff2?|pcf(\.gz)?)$", p, re.I):
        return "font files"
    if re.search(r"fontconfig|\.cache-\d+$", p):
        return "fontconfig caches"
    if "locale-archive" in p or "/locale/" in p or "gconv" in p:
        return "locale data"
    if "mesa_shader_cache" in p or "shader" in p:
        return "Mesa shader cache"
    if "icon" in p:
        return "icon caches"
    if "qmlcache" in p or "qtpipelinecache" in p or "/.qmlc" in p or "qml" in p:
        return "Qt caches and QML"
    if p.startswith("/"):
        return "other files"
    return f"other ({p})"


def stacks_and_arenas(maps, sp_file):
    sps = []
    if os.path.exists(sp_file):
        for line in open(sp_file):
            parts = line.split()
            if len(parts) >= 2 and parts[1].startswith("0x"):
                sps.append(int(parts[1], 16))
    stacks = set()
    for m in maps:
        if not m["path"] and any(m["start"] <= sp < m["end"] for sp in sps):
            stacks.add(m["start"])
    arenas = set()
    size = 64 << 20
    for i, m in enumerate(maps):
        if m["path"] or m["start"] % size:
            continue
        # glibc's per-thread heap: an aligned rw- part and a ---p reserve
        # that together span HEAP_MAX_SIZE.
        end = m["end"]
        j = i + 1
        while j < len(maps) and not maps[j]["path"] and maps[j]["start"] == end and end - m["start"] < size:
            end = maps[j]["end"]
            j += 1
        if end - m["start"] == size and m["start"] not in stacks:
            for k in range(i, j):
                arenas.add(maps[k]["start"])
    return stacks, arenas


def summarise(rundir, name):
    files = sorted(glob.glob(f"{rundir}/{name}.*.smaps"))
    cats = collections.defaultdict(lambda: collections.Counter())
    libs = collections.defaultdict(lambda: collections.Counter())
    groups = collections.defaultdict(lambda: collections.Counter())
    procs = []
    for f in files:
        base = f[:-len(".smaps")]
        maps = parse_smaps(f)
        stacks, arenas = stacks_and_arenas(maps, base + ".sp")
        status = {}
        if os.path.exists(base + ".status"):
            for line in open(base + ".status"):
                k, _, v = line.partition(":")
                status[k] = v.strip()
        procs.append({"pid": base.rsplit(".", 1)[1], "name": status.get("Name"), "threads": status.get("Threads")})
        # ELF files mapped executable that aren't shared objects: the
        # program (and a Nix wrapper, until it execs).
        execs = {m["path"] for m in maps if "x" in m["perms"] and m["path"].startswith("/")}
        for m in maps:
            c = categorise(m, stacks, arenas, execs)
            for k in FIELDS:
                cats[c][k] += m.get(k, 0)
            cats[c]["mappings"] += 1
            if c.startswith("library"):
                ln = lib_name(m["path"])
                g = library_group(m["path"])
                for k in FIELDS:
                    libs[ln][k] += m.get(k, 0)
                    groups[g][k] += m.get(k, 0)
                libs[ln]["group"] = 0
    return {"processes": procs, "categories": {c: dict(v) for c, v in cats.items()},
            "libraries": {l: dict(v) for l, v in libs.items()},
            "library_groups": {g: dict(v) for g, v in groups.items()}}


def mib(k):
    return f"{k / 1024:.2f}"


def main():
    args = sys.argv[1:]
    rundir = args.pop(0)
    out_json = None
    if "--json" in args:
        i = args.index("--json")
        out_json = args[i + 1]
        del args[i:i + 2]
    result = {}
    for name in args:
        s = summarise(rundir, name)
        result[name] = s
        tot = collections.Counter()
        for v in s["categories"].values():
            tot.update({k: v.get(k, 0) for k in FIELDS})
        print(f"## {name}: {len(s['processes'])} process(es) "
              f"({', '.join(str(p['name']) for p in s['processes'])}); PSS {mib(tot['Pss'])} MiB, "
              f"Anonymous {mib(tot['Anonymous'])} MiB, Private_Dirty {mib(tot['Private_Dirty'])} MiB, RSS {mib(tot['Rss'])} MiB\n")
        print("| Category | PSS MiB | Anonymous MiB | Private_Dirty MiB | RSS MiB | mappings |")
        print("|---|---:|---:|---:|---:|---:|")
        for c, v in sorted(s["categories"].items(), key=lambda kv: -kv[1].get("Pss", 0)):
            if v.get("Rss", 0) == 0 and v.get("Pss", 0) == 0:
                continue
            print(f"| {c} | {mib(v.get('Pss', 0))} | {mib(v.get('Anonymous', 0))} | {mib(v.get('Private_Dirty', 0))} | "
                  f"{mib(v.get('Rss', 0))} | {v.get('mappings', 0)} |")
        print("\nShared objects by group (all segments):\n")
        print("| Group | PSS MiB | Anonymous MiB (relocated and written pages) | Private_Dirty MiB | RSS MiB |")
        print("|---|---:|---:|---:|---:|")
        for g, v in sorted(s["library_groups"].items(), key=lambda kv: -kv[1].get("Pss", 0)):
            print(f"| {g} | {mib(v.get('Pss', 0))} | {mib(v.get('Anonymous', 0))} | {mib(v.get('Private_Dirty', 0))} | {mib(v.get('Rss', 0))} |")
        print("\nShared objects with the most dirty (relocated or written) pages:\n")
        for l, v in sorted(s["libraries"].items(), key=lambda kv: -kv[1].get("Anonymous", 0))[:15]:
            print(f"  {mib(v.get('Anonymous', 0)):>6} MiB anonymous, {mib(v.get('Pss', 0)):>6} MiB PSS  {l}")
        print()
    if out_json:
        with open(out_json, "w") as f:
            json.dump(result, f, indent=1)


if __name__ == "__main__":
    main()
