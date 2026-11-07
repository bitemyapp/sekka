# SPDX-License-Identifier: GPL-3.0-or-later
"""The adapter's identity, and the lock the engine archive carries.

The adapter digest covers every Python source file of this package, by
path and SHA-256, in canonical form. The archive's producer
(installer/genkan/macos/engine/default.nix) writes it into
`engine-lock.json` with this same function; the engine computes it again
when it starts, reports it in its Hello, and refuses to run if the two
differ.
"""

import hashlib
import os

from . import canonical

LOCK_SCHEMA = "sekka.mac.engine-lock/1"
PROFILE_DIR = "sekka-profiles"
LOCK_FILE = "engine-lock.json"


def package_dir():
    return os.path.dirname(os.path.abspath(__file__))


def adapter_files(directory=None):
    directory = directory or package_dir()
    name = os.path.basename(directory)
    files = []
    for entry in sorted(os.listdir(directory)):
        if not entry.endswith(".py"):
            continue
        path = os.path.join(directory, entry)
        with open(path, "rb") as f:
            files.append([f"{name}/{entry}", "sha256:" + hashlib.sha256(f.read()).hexdigest()])
    return files


def adapter_digest(directory=None):
    return canonical.digest({"domain": "sekka.mac.adapter/1", "files": adapter_files(directory)})
