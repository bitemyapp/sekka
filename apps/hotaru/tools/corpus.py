#!/usr/bin/env python3
"""Generate deterministic VT workloads before running any timing command."""
import argparse
import hashlib
import json
from pathlib import Path

PATTERNS = {
    "ascii": b"The quick brown fox jumps over the lazy dog 0123456789\r\n",
    "styled": b"\x1b[1;38;2;180;200;255mINFO\x1b[0m request=12345 \x1b[4munderlined\x1b[0m\r\n",
    "unicode": "雪花 藍染 蛍 café e\u0301 日本語 👩🏽‍💻\r\n".encode(),
    "cursor": b"\x1b[2J\x1b[Htop\x1b[10;30Hmiddle\x1b[2K\x1b[5;1H\x1b[31mstatus\x1b[0m\r\n",
    "hyperlinks": b"\x1b]8;;https://example.org/path\x1b\\link\x1b]8;;\x1b\\\r\n",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--bytes", type=int, default=8 * 1024 * 1024)
    args = parser.parse_args()
    if args.bytes < 1:
        parser.error("--bytes must be positive")
    args.destination.mkdir(parents=True, exist_ok=True)
    metadata = {"schema": 1, "generator": "hotaru-corpus-v1", "requested_bytes": args.bytes, "files": []}
    for name, pattern in PATTERNS.items():
        # Finish each pattern instead of cutting UTF-8 or an escape sequence.
        data = pattern * max(1, (args.bytes + len(pattern) - 1) // len(pattern))
        path = args.destination / f"{name}.vt"
        path.write_bytes(data)
        metadata["files"].append({"name": name, "path": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    (args.destination / "manifest.json").write_text(json.dumps(metadata, indent=2) + "\n")


if __name__ == "__main__":
    main()
