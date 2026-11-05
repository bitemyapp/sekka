#!/usr/bin/env python3
"""Copy what why-apps.md cites from run.sh's working directory into the
repository's data directory, compactly and without the session's
environment (paths under /tmp and the store are kept; HOME-relative
paths, variables the session passes on and window titles are not).

    collect.py OUT DATA

For each set, DATA/SET.runs.jsonl.xz has one line a run: variant, rep, first
frame (ms), every thread's name and CPU at the first frame, the shared
objects mapped then (basenames), and after settling, smaps_rollup. Stamps
lines (DATA/stamps/) are copied with store hashes shortened.
"""
import json
import lzma
import os
import re
import sys


def clean(s):
    s = re.sub(r"/nix/store/([a-z0-9]{8})[a-z0-9]{24}-", r"/nix/store/\1…-", s)
    s = re.sub(r"/home/[^/\s\"]+", "~", s)
    s = re.sub(r"/tmp/claude-1000/(wa|ws)\.[A-Za-z0-9_]+", "$XDG_RUNTIME_DIR", s)
    return s


def compact(r):
    out = {"name": r["name"], "rep": r["rep"], "first_frame_ms": r.get("first_frame_ms")}
    if r.get("warmup"):
        out["warmup"] = True
    if r.get("env"):
        out["env"] = {k: (clean(v) if isinstance(v, str) else v) for k, v in r["env"].items()
                      if k not in ("LD_PRELOAD", "STAMPS_FILE", "XDG_DATA_DIRS", "QT_PLUGIN_PATH",
                                   "NIXPKGS_QT6_QML_IMPORT_PATH", "GIO_EXTRA_MODULES", "GDK_PIXBUF_MODULE_FILE")}
        for k in ("XDG_DATA_DIRS", "QT_PLUGIN_PATH", "NIXPKGS_QT6_QML_IMPORT_PATH", "GIO_EXTRA_MODULES",
                  "GDK_PIXBUF_MODULE_FILE"):
            if k in r["env"]:
                out["env"][k] = "(the Nix wrapper's value)" if r["env"][k] is not None else None
        if "LD_PRELOAD" in r["env"]:
            out["env"]["LD_PRELOAD"] = "stamps.so"
    if r.get("threads_at_map"):
        out["threads_at_map"] = [{"pid": t["pid"], "tid": t["tid"], "comm": t["comm"],
                                  "exe": os.path.basename(t["exe"] or ""), "run_ms": round(t["run_ns"] / 1e6, 3),
                                  "wait_ms": round(t["wait_ns"] / 1e6, 3)} for t in r["threads_at_map"]]
        out["window_pid"] = r.get("window_pid")
    if r.get("shared_objects_at_map"):
        out["shared_objects_at_map"] = sorted({os.path.basename(p) for p in r["shared_objects_at_map"]})
    if r.get("rollup_kib"):
        out["rollup_kib"] = r["rollup_kib"]
    return out


def main():
    out, data = sys.argv[1], sys.argv[2]
    os.makedirs(data, exist_ok=True)
    for s in sorted(os.listdir(out)):
        jl = os.path.join(out, s, "runs.jsonl")
        if not os.path.isfile(jl) or s.endswith("-failed"):
            continue
        with lzma.open(os.path.join(data, f"{s}.runs.jsonl.xz"), "wt", preset=9 | lzma.PRESET_EXTREME) as f:
            for line in open(jl):
                r = json.loads(line)
                if r.get("meta"):
                    m = {k: r[k] for k in ("date", "host", "kernel", "load_before", "load_after", "reps", "settle_s",
                                           "sway") if k in r}
                    m["meta"] = True
                    f.write(json.dumps(m) + "\n")
                else:
                    f.write(json.dumps(compact(r)) + "\n")
    st = os.path.join(data, "stamps")
    os.makedirs(st, exist_ok=True)
    src = os.path.join(out, "stamps")
    for fn in sorted(os.listdir(src)):
        if fn.endswith(".stamps"):
            with open(os.path.join(src, fn)) as a, open(os.path.join(st, fn), "w") as b:
                for line in a:
                    b.write(clean(line))


if __name__ == "__main__":
    main()
