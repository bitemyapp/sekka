#!/usr/bin/env python3
"""Remove named processes from Ananicy's JSON-lines rule files.

Comments and surviving records retain their original bytes. Invalid upstream
records are left for Ananicy to diagnose, rather than guessing their meaning,
except one that still begins with a record for a removed name (followed by a
comment, say): Ananicy might apply it, so the build stops instead.
"""
import io
import json
from pathlib import Path
import sys


def excluded_name(rule, excluded):
    name = rule.get("name") if isinstance(rule, dict) else None
    return isinstance(name, str) and name in excluded


def filtered(text, excluded):
    kept = []
    for line in io.StringIO(text):
        try:
            rule = json.loads(line)
        except json.JSONDecodeError:
            try:
                leading, _ = json.JSONDecoder().raw_decode(line.lstrip())
            except json.JSONDecodeError:
                leading = None
            if excluded_name(leading, excluded):
                raise ValueError(f"cannot remove this rule: {line.strip()}")
            kept.append(line)
            continue
        if not excluded_name(rule, excluded):
            kept.append(line)
    return "".join(kept)


def main():
    root, *names = sys.argv[1:]
    for path in Path(root).rglob("*.rules"):
        text = path.read_bytes().decode("utf-8")
        try:
            result = filtered(text, set(names))
        except ValueError as error:
            sys.exit(f"{path}: {error}")
        if result != text:
            path.write_bytes(result.encode("utf-8"))


if __name__ == "__main__":
    main()
