#!/usr/bin/env python3
"""Remove named processes from Ananicy's JSON-lines rule files.

Comments and surviving records retain their original bytes. Invalid upstream
records are left for Ananicy to diagnose, rather than guessing their meaning.
"""
import io
import json
from pathlib import Path
import sys


def filtered(text, excluded):
    kept = []
    for line in io.StringIO(text):
        try:
            rule = json.loads(line)
        except json.JSONDecodeError:
            kept.append(line)
            continue
        name = rule.get("name") if isinstance(rule, dict) else None
        if not isinstance(name, str) or name not in excluded:
            kept.append(line)
    return "".join(kept)


def main():
    root, *names = sys.argv[1:]
    for path in Path(root).rglob("*.rules"):
        text = path.read_bytes().decode("utf-8")
        result = filtered(text, set(names))
        if result != text:
            path.write_bytes(result.encode("utf-8"))


if __name__ == "__main__":
    main()
