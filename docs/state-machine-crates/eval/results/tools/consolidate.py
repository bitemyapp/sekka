#!/usr/bin/env python3
"""Folds a criterion or gungraun output directory into JSON Lines on stdout,
so raw/ keeps the numbers without thousands of small files.

  python3 -I consolidate.py criterion DIR
      One line per DIR/<group>/<impl>/new/estimates.json:
      {"group": ..., "impl": ..., "estimates": <the file's object>}.
      criterion's base/, samples, Tukey fences and reports are left out.

  python3 -I consolidate.py gungraun DIR
      One line per summary.json under DIR:
      {"path": <its directory, relative to DIR>, "summary": <the file's object>}.
      The callgrind .out and .log files are left out.

Lines are sorted, so the output depends only on the files' contents.
"""

import json
import sys
from pathlib import Path


def criterion(base):
    for est in sorted(base.glob("*/*/new/estimates.json")):
        group, impl = est.parts[-4], est.parts[-3]
        yield {"group": group, "impl": impl, "estimates": json.loads(est.read_text())}


def gungraun(base):
    for summary in sorted(base.rglob("summary.json")):
        yield {"path": summary.parent.relative_to(base).as_posix(), "summary": json.loads(summary.read_text())}


def main():
    kind, base = sys.argv[1], Path(sys.argv[2])
    rows = {"criterion": criterion, "gungraun": gungraun}[kind](base)
    for row in rows:
        print(json.dumps(row, separators=(",", ":")))


if __name__ == "__main__":
    main()
