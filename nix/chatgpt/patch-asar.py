#!/usr/bin/env python3
"""Repair the executor's plugin copy without moving ASAR file offsets.

The upstream package repairs the marketplace copy, but the executor copies
codex-app-tools separately. Linux has no Windows command-path exception; use
that space for a helper outside the archive. Fail when the upstream code changes
so a package update cannot silently drop the fix.
"""

import re
import sys
from pathlib import Path

COPY = re.compile(
    rb"await [\w$]+\.default\.cp\((?P<config>[\w$]+)\.cwd,"
    rb"(?P<dest>[\w$]+),\{recursive:!0\}\),process\.platform!==`win32`&&"
)


def patch(data: bytes) -> bytes:
    matches = list(COPY.finditer(data))
    if len(matches) != 1:
        raise ValueError(f"expected one executor plugin copy, found {len(matches)}")
    match = matches[0]
    replacement = (
        b"await require(`../../../sekka-copy-plugin.cjs`)("
        + match["config"]
        + b".cwd,"
        + match["dest"]
        + b"),"
    )
    if len(replacement) > len(match[0]):
        raise ValueError("executor plugin patch exceeds its ASAR byte budget")
    return (
        data[: match.start()]
        + replacement.ljust(len(match[0]), b" ")
        + data[match.end() :]
    )


if __name__ == "__main__":
    archive = Path(sys.argv[1])
    archive.write_bytes(patch(archive.read_bytes()))
