#!/usr/bin/env python3
"""Recreate all exact benchmark corpora outside the repository."""
import argparse
import hashlib
import json
from pathlib import Path
p = argparse.ArgumentParser(description=__doc__)
p.add_argument("destination", type=Path)
a = p.parse_args()
m = json.loads((Path(__file__).resolve().parent.parent / "input-manifest.json").read_text())
a.destination.mkdir(parents=True, exist_ok=True)
for c in m["cases"]:
    pattern = bytes.fromhex(c["pattern_hex"])
    assert c["bytes"] % len(pattern) == 0
    data = pattern * (c["bytes"] // len(pattern))
    assert hashlib.sha256(data).hexdigest() == c["sha256"]
    (a.destination / (c["name"] + ".vt")).write_bytes(data)
print(json.dumps({"recreated_and_hash_verified": len(m["cases"]), "destination": str(a.destination)}))
