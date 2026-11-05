#!/usr/bin/env python3
"""Pixel agreement of each engine's pages with a reference engine's (PDFium
by default), from the PPMs a render pass wrote (render/<engine>/<file>/<page>.ppm).

  compare.py RENDER_DIR OUT.tsv [--reference pdfium] ENGINE...

A pixel differs when its largest per-channel difference exceeds 48 of 255
(the measure of the earlier records); pages are compared when both engines
drew them at the same size within 2 pixels each way, over the area they
share (pages over 100 million pixels are left out). Writes engine, file, page, ref_w, ref_h, w, h, compared (1, or 0 for a
size mismatch), differing pixels, their fraction, the largest difference and
the mean absolute difference.
"""
import argparse
import os
import sys
from concurrent.futures import ProcessPoolExecutor

import numpy as np


def read_ppm(path):
    with open(path, "rb") as f:
        head = b""
        fields = []
        while len(fields) < 4:
            c = f.read(1)
            if not c:
                raise ValueError("short header")
            if c.isspace():
                if head:
                    fields.append(head)
                    head = b""
            else:
                head += c
        w, h = int(fields[1]), int(fields[2])
        data = np.frombuffer(f.read(w * h * 3), dtype=np.uint8)
    return data.reshape(h, w, 3)


def ppm_size(path):
    with open(path, "rb") as f:
        fields = f.read(64).split()
    return int(fields[1]), int(fields[2])


def one(job):
    engine, ref_path, path, name, page = job
    (rw, rh), (w, h) = ppm_size(ref_path), ppm_size(path)
    if rw * rh > 100_000_000 or w * h > 100_000_000:
        # Too large to compare in memory here (issue19517's page, drawn only by hayro).
        return [engine, name, page, rw, rh, w, h, 0, "", "", "", "too large"]
    a = read_ppm(ref_path)
    b = read_ppm(path)
    if abs(rw - w) > 2 or abs(rh - h) > 2:
        return [engine, name, page, rw, rh, w, h, 0, "", "", "", ""]
    ch, cw = min(rh, h), min(rw, w)
    d = np.abs(a[:ch, :cw].astype(np.int16) - b[:ch, :cw].astype(np.int16)).max(axis=2)
    n = int((d > 48).sum())
    return [engine, name, page, rw, rh, w, h, 1, n, f"{n / d.size:.6f}", int(d.max()), f"{d.mean():.4f}"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("render_dir")
    ap.add_argument("out")
    ap.add_argument("engines", nargs="+")
    ap.add_argument("--reference", default="pdfium")
    ap.add_argument("--jobs", type=int, default=6)
    a = ap.parse_args()
    ref_root = os.path.join(a.render_dir, a.reference)
    jobs = []
    for engine in a.engines:
        root = os.path.join(a.render_dir, engine)
        for name in sorted(os.listdir(root)):
            for f in sorted(os.listdir(os.path.join(root, name))):
                ref = os.path.join(ref_root, name, f)
                if f.endswith(".ppm") and os.path.exists(ref):
                    jobs.append((engine, ref, os.path.join(root, name, f), name, int(f[:-4])))
    with open(a.out, "w") as out, ProcessPoolExecutor(a.jobs) as pool:
        out.write("engine\tfile\tpage\tref_w\tref_h\tw\th\tcompared\tdiffering_px\tdiffering_frac\tmax_diff\tmean_abs_diff\n")
        for row in pool.map(one, jobs, chunksize=8):
            out.write("\t".join(map(str, row)) + "\n")


if __name__ == "__main__":
    main()
