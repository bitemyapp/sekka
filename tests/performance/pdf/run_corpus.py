#!/usr/bin/env python3
"""Runs one engine's benchmark program over a list of PDFs, each file in a
process of its own under `timeout`, N at a time, each process pinned to a
core of its own (`taskset -c CORE`).

  run_corpus.py --engine NAME --bin PROGRAM --list FILES.txt --tag RUN \
      --pages-tsv PAGES.tsv --files-tsv FILES.tsv [--cores 2,3,4,5]
      [--timeout 30] [--render-dir DIR]

Appends a line per page drawn to PAGES.tsv (tag, engine, file, page, ms,
width, height) and a line per file to FILES.tsv (tag, engine, file, status,
exit, wall_ms, maxrss_kib, open_ms, pages_in_file, pages_drawn,
pages_failed, detail). status is ok, refused (the engine wouldn't open the
file), timeout, or crash (a signal or a non-zero exit before `done`).
maxrss_kib is the peak resident set of the file's process (wait4's
ru_maxrss over `timeout` and the program it waited for). With --render-dir,
pages are written to DIR/<file stem>/<page>.ppm.
"""
import argparse
import os
import queue
import signal
import subprocess
import sys
import threading
import time


def classify(status, lines):
    sig = os.WIFSIGNALED(status) and os.WTERMSIG(status)
    code = os.WIFEXITED(status) and os.WEXITSTATUS(status)
    kinds = [l.split("\t", 1)[0] for l in lines]
    if code == 124 or sig == signal.SIGKILL:
        return "timeout", 124 if code == 124 else -sig
    if sig:
        return "crash", -sig
    if code != 0:
        return "crash", code
    if "error" in kinds:
        return "refused", 0
    if "done" not in kinds:
        return "crash", code
    return "ok", 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", required=True)
    ap.add_argument("--bin", required=True)
    ap.add_argument("--list", required=True)
    ap.add_argument("--tag", required=True)
    ap.add_argument("--pages-tsv", required=True)
    ap.add_argument("--files-tsv", required=True)
    ap.add_argument("--cores", default="2,3,4,5")
    ap.add_argument("--timeout", type=int, default=30)
    ap.add_argument("--render-dir")
    ap.add_argument("--stderr-dir", default="/dev/shm")
    a = ap.parse_args()

    files = [l.strip() for l in open(a.list) if l.strip()]
    cores = [int(c) for c in a.cores.split(",")]
    jobs = queue.Queue()
    for f in files:
        jobs.put(f)
    lock = threading.Lock()
    pages_out = open(a.pages_tsv, "a")
    files_out = open(a.files_tsv, "a")

    def worker(core):
        errpath = os.path.join(a.stderr_dir, f"pdfbench-{os.getpid()}-{core}.err")
        while True:
            try:
                path = jobs.get_nowait()
            except queue.Empty:
                return
            name = os.path.basename(path)
            argv = ["taskset", "-c", str(core), "timeout", "-k", "5", str(a.timeout), a.bin, path]
            if a.render_dir:
                d = os.path.join(a.render_dir, os.path.splitext(name)[0])
                os.makedirs(d, exist_ok=True)
                argv.append(d)
            with open(errpath, "wb") as err:
                t0 = time.monotonic()
                p = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=err, stdin=subprocess.DEVNULL)
                out = p.stdout.read()
                _, status, ru = os.wait4(p.pid, 0)
                wall = (time.monotonic() - t0) * 1000
                p.returncode = os.waitstatus_to_exitcode(status)
                p.stdout.close()
            lines = out.decode("utf-8", "replace").splitlines()
            state, code = classify(status, lines)
            open_ms, npages, drawn, failed, detail = "", "", 0, 0, ""
            page_rows = []
            for l in lines:
                f = l.split("\t")
                if f[0] == "open" and len(f) >= 3:
                    open_ms, npages = f[1], f[2]
                elif f[0] == "page" and len(f) >= 5:
                    drawn += 1
                    page_rows.append("\t".join([a.tag, a.engine, name, f[1], f[2], f[3], f[4]]))
                elif f[0] == "fail":
                    failed += 1
                elif f[0] == "error":
                    detail = " ".join(f[1:])
            if state == "crash":
                try:
                    with open(errpath, "rb") as e:
                        e.seek(0, 2)
                        e.seek(max(0, e.tell() - 400))
                        tail = e.read().decode("utf-8", "replace").strip().splitlines()
                    detail = (tail[-1] if tail else "")[:200]
                except OSError:
                    pass
            detail = detail.replace("\t", " ").replace("\n", " ")
            row = [a.tag, a.engine, name, state, str(code), f"{wall:.1f}", str(ru.ru_maxrss),
                   open_ms, npages, str(drawn), str(failed), detail]
            with lock:
                for r in page_rows:
                    pages_out.write(r + "\n")
                files_out.write("\t".join(row) + "\n")
        try:
            os.unlink(errpath)
        except OSError:
            pass

    threads = [threading.Thread(target=worker, args=(c,)) for c in cores]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    pages_out.close()
    files_out.close()


if __name__ == "__main__":
    main()
