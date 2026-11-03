#!/usr/bin/env python3
"""Syscalls from launch to the first frame, from probe.py's `strace` set
(strace -f -tt -T -y; libwayland programs exit right after their first
commit, the kit's are cut at sway's window event).

    syscalls.py RUNDIR [NAME...]

For each program (median over runs): syscalls made, by name; files opened
and failed opens (ENOENT: a search through a path list); mmaps of files;
sockets connected; and the time strace saw spent inside each syscall
(inflated by tracing: for comparison between programs, not as a timing).
"""
import collections
import datetime
import json
import re
import statistics
import sys

LINE = re.compile(r"^(\d+)\s+(\d\d):(\d\d):(\d\d\.\d+) (.*)$")
CALL = re.compile(r"^(?:<\.\.\. )?(\w+)(?:\(| resumed>)")


def parse(path, cutoff_s):
    calls = collections.Counter()
    time_in = collections.Counter()
    opens_ok = opens_fail = 0
    mmap_files = 0
    connects = []
    exts = collections.Counter()
    for line in open(path, errors="replace"):
        m = LINE.match(line)
        if not m:
            continue
        secs = int(m.group(2)) * 3600 + int(m.group(3)) * 60 + float(m.group(4))
        if cutoff_s is not None and secs > cutoff_s:
            continue
        rest = m.group(5)
        if rest.startswith(("+++", "---")):
            continue
        c = CALL.match(rest)
        if not c:
            continue
        name = c.group(1)
        if "<unfinished" in rest:
            continue
        calls[name] += 1
        d = re.search(r"<(\d+\.\d+)>\s*$", rest)
        if d:
            time_in[name] += float(d.group(1))
        if name in ("openat", "open"):
            if "= -1 ENOENT" in rest:
                opens_fail += 1
            elif re.search(r"= \d+", rest):
                opens_ok += 1
                f = re.search(r'"([^"]+)"', rest)
                if f:
                    p = f.group(1)
                    if re.search(r"\.so(\.|$)", p):
                        exts["shared objects"] += 1
                    elif re.search(r"\.(conf|xml|ini|json|desktop|theme|css|qml|ui|rc)$", p) or "/etc/" in p:
                        exts["configuration (conf, xml, ini, json, css, qml)"] += 1
                    elif re.search(r"fontconfig|\.cache-\d+", p):
                        exts["fontconfig caches"] += 1
                    elif re.search(r"\.(ttf|otf|ttc)$", p, re.I):
                        exts["font files"] += 1
                    elif re.search(r"icon|\.png$|\.svg$", p):
                        exts["icons and images"] += 1
                    elif p.startswith(("/proc", "/sys", "/dev")):
                        exts["/proc, /sys, /dev"] += 1
                    else:
                        exts["other files"] += 1
        if name == "mmap" and re.search(r"<[^>]*/[^>]*>", rest.split("=")[0]):
            mmap_files += 1
        if name == "connect" and "= -1" not in rest:
            s = re.search(r'sun_path="([^"]+)"', rest)
            connects.append(re.sub(r"^/tmp/claude-1000/[^/]+/", "$XDG_RUNTIME_DIR/", s.group(1)) if s else "?")
    return calls, time_in, opens_ok, opens_fail, mmap_files, connects, exts


def main():
    rundir = sys.argv[1]
    runs = [json.loads(l) for l in open(f"{rundir}/runs.jsonl")]
    runs = [r for r in runs if not r.get("meta") and not r.get("warmup")]
    names = sys.argv[2:] or list(dict.fromkeys(r["name"] for r in runs))
    rows = {}
    detail = {}
    for name in names:
        per = collections.defaultdict(list)
        callsets = []
        for r in [r for r in runs if r["name"] == name]:
            cutoff = None
            if r.get("first_frame_ms"):
                t = datetime.datetime.fromtimestamp((r["t0_real_ns"] / 1e9) + r["first_frame_ms"] / 1000)
                cutoff = t.hour * 3600 + t.minute * 60 + t.second + t.microsecond / 1e6
            calls, time_in, ok, fail, mm, conn, exts = parse(f"{rundir}/{name}-{r['rep']}.strace", cutoff)
            per["syscalls"].append(sum(calls.values()))
            per["openat ok"].append(ok)
            per["openat ENOENT"].append(fail)
            per["mmap of a file"].append(mm)
            per["mmap"].append(calls["mmap"])
            per["mprotect"].append(calls["mprotect"])
            per["munmap"].append(calls["munmap"])
            per["read"].append(calls["read"] + calls["pread64"])
            per["newfstatat/statx"].append(calls["newfstatat"] + calls["statx"] + calls["fstat"])
            per["connect"].append(len(conn))
            per["futex"].append(calls["futex"])
            per["poll/ppoll/epoll_wait"].append(calls["poll"] + calls["ppoll"] + calls["epoll_wait"] + calls["epoll_pwait"])
            per["sendmsg/recvmsg"].append(calls["sendmsg"] + calls["recvmsg"])
            per["clone/clone3"].append(calls["clone"] + calls["clone3"])
            for k, v in exts.items():
                per[f"opened: {k}"].append(v)
            callsets.append((calls, time_in, conn))
        rows[name] = {k: statistics.median(v) for k, v in per.items()}
        if callsets:
            calls, time_in, conn = callsets[0]
            detail[name] = (calls.most_common(12), sorted(set(conn)))
    keys = list(dict.fromkeys(k for v in rows.values() for k in v))
    print("Syscalls from launch to the first frame (median over runs, under strace):\n")
    print("| | " + " | ".join(rows) + " |")
    print("|---|" + "---:|" * len(rows))
    for k in keys:
        print(f"| {k} | " + " | ".join(f"{rows[n].get(k, 0):.0f}" for n in rows) + " |")
    print()
    for name, (top, conn) in detail.items():
        print(f"{name}: most frequent: " + ", ".join(f"{c} {n}" for c, n in top))
        print(f"    sockets connected: {', '.join(conn) if conn else 'none'}")


if __name__ == "__main__":
    main()
