#!/usr/bin/env python3
"""Repair the executor's plugin copy without moving ASAR file offsets.

The executor copies codex-app-tools separately from the marketplace. Handle
both the original copy and upstream's newer chmod-based repair: Sekka's helper
also serializes overlapping copies and leaves the manifest writable. Fail on
unrecognized code so a package update cannot silently drop those guarantees.
"""

import re
import sys
from pathlib import Path

COPY = re.compile(
    rb"await [\w$]+\.default\.cp\((?P<config>[\w$]+)\.cwd,"
    rb"(?P<dest>[\w$]+),\{recursive:!0\}\),process\.platform!==`win32`&&"
)
# The refreshed ai-apps package repairs the executor's directory permissions
# before fs.cp and runs chmod afterwards. Replace that whole operation with
# our serialized helper; retain upstream's subsequent command-path rewrite.
COPY_UPSTREAM_WRITABLE = re.compile(
    rb"await [\w$]+\((?P<dest>[\w$]+)\),"
    rb"await [\w$]+\.default\.cp\((?P<config>[\w$]+)\.cwd,"
    rb"(?P=dest),\{recursive:!0\}\),"
    rb"await [\w$]+\(`chmod`,\[`-R`,`u\+w`,(?P=dest)\]\),"
)


def patch(data: bytes) -> bytes:
    matches = [
        match
        for pattern in (COPY, COPY_UPSTREAM_WRITABLE)
        for match in pattern.finditer(data)
    ]
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
