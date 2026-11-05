#!/usr/bin/env python3
"""What a program's heap holds 3 s after its first frame, by purpose, from
probe.py's `heaptrack` set: the allocations still live when the program
was stopped (heaptrack's "leaked": a GLib, Qt or Rust program killed by
SIGTERM frees nothing on the way out).

    heapattr.py RUNDIR NAME [NAME...] [--folded-dir DIR]

Each allocation's call stack is given a purpose by attribute.py's rules
(the nearest frame they recognise, from the allocation up), so bytes are
charged to what asked for them. heaptrack_print must be on PATH.
"""
import collections
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from attribute import classify  # noqa: E402


def folded(path, out):
    subprocess.run(["heaptrack_print", "-f", path, "--flamegraph-cost-type", "leaked", "-F", out,
                    "-p", "0", "-a", "0", "-T", "0", "-l", "0"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def summary_lines(path):
    r = subprocess.run(["heaptrack_print", "-f", path, "-p", "0", "-a", "0", "-T", "0", "-l", "0"],
                       capture_output=True, text=True)
    return [l for l in r.stdout.splitlines() if l.startswith(("total runtime", "calls to allocation", "peak heap",
                                                              "peak RSS", "total memory leaked", "suppressed"))]


def main():
    args = sys.argv[1:]
    rundir = args.pop(0)
    fdir = None
    if "--folded-dir" in args:
        i = args.index("--folded-dir")
        fdir = args[i + 1]
        del args[i:i + 2]
    for name in args:
        path = f"{rundir}/{name}-0.heaptrack.zst"
        if not os.path.exists(path):
            print(f"## {name}: no data\n")
            continue
        out = os.path.join(fdir or rundir, f"{name}.heap.folded")
        folded(path, out)
        purpose = collections.Counter()
        top = collections.Counter()
        total = 0
        for line in open(out):
            stack, n = line.rstrip("\n").rsplit(" ", 1)
            n = int(n)
            frames = [(f, "") for f in reversed(stack.split(";"))]
            # heaptrack's stacks start at the allocation function; the
            # program's own frames above main are its call chain.
            p, _ = classify(frames + [("main", "")])
            purpose[p] += n
            total += n
            caller = next((f for f, _ in frames if not f.startswith(("malloc", "calloc", "realloc", "operator new",
                                                                        "g_malloc", "g_slice", "g_realloc", "g_try",
                                                                        "<unresolved", "alloc::", "__rust", "_int_",
                                                                        "g_memdup", "g_strdup", "g_strndup"))), "?")
            top[caller] += n
        print(f"## {name}: {total / 1048576:.2f} MiB live in malloc 3 s after the first frame")
        for l in summary_lines(path):
            print(f"    {l}")
        print("\n| Purpose | live heap, MiB | share |")
        print("|---|---:|---:|")
        for p, n in purpose.most_common():
            print(f"| {p} | {n / 1048576:.2f} | {100 * n / max(total, 1):.1f}% |")
        print("\nLargest direct callers of the allocator (MiB):")
        for f, n in top.most_common(15):
            print(f"  {n / 1048576:6.2f}  {f[:150]}")
        print()


if __name__ == "__main__":
    main()
