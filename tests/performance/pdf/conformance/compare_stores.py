#!/usr/bin/env python3
"""Compares two hayro checkouts' stored drawings (`cargo test -p hayro-tests
--release -- --store --exact TEST...` writes tests/store/pdf/<test>_<page>.png)
page by page: for each test, the share of pixels on its worst page whose
largest channel difference exceeds 48, and the largest difference. Used for
the fork's tests whose reference images can no longer be downloaded, against
the base revision's drawings of them.

  compare_stores.py FORK_STORE_DIR BASE_STORE_DIR
"""
import os
import sys
from collections import defaultdict

import numpy as np
from PIL import Image


def main():
    a_dir, b_dir = sys.argv[1:3]
    worst, largest = defaultdict(float), defaultdict(int)
    for f in sorted(os.listdir(a_dir)):
        a = np.asarray(Image.open(os.path.join(a_dir, f)).convert("RGBA")).astype(int)
        b = np.asarray(Image.open(os.path.join(b_dir, f)).convert("RGBA")).astype(int)
        test = f.rsplit("_", 1)[0]
        if a.shape != b.shape:
            worst[test] = 1.0
            continue
        d = np.abs(a - b).max(axis=2)
        worst[test] = max(worst[test], float((d > 48).mean()))
        largest[test] = max(largest[test], int(d.max()))
    for test in sorted(worst):
        print(test, round(worst[test] * 100, 3), largest[test])


if __name__ == "__main__":
    main()
