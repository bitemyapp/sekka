#!/usr/bin/env python3
"""Verify matching symbols in a root-flake Aizome out/debug pair."""
import argparse
import json
from pathlib import Path
import re
import subprocess

p = argparse.ArgumentParser()
p.add_argument("runtime", type=Path)
p.add_argument("debug", type=Path)
a = p.parse_args()


def run(*args):
    return subprocess.check_output(list(map(str, args)), text=True, stderr=subprocess.DEVNULL)


def build_id(path):
    return re.search(r"Build ID: ([0-9a-f]+)", run("readelf", "-n", path))[1]


results = []
for relative in ["bin/aizome", "bin/.aizome-myaku-wrapped", "bin/.aizome-profile-wrapped",
                 "bin/aizome-sumi", "lib/aizome/libaizome_comp_core.so", "lib/aizome/libaizome_comp_logic.so"]:
    binary = a.runtime / relative
    identifier = build_id(binary)
    debug = a.debug / "lib/debug/.build-id" / identifier[:2] / (identifier[2:] + ".debug")
    assert debug.exists(), (binary, identifier)
    assert build_id(debug) == identifier
    sections = run("readelf", "-SW", binary)
    assert ".debug_line" not in sections
    assert ".eh_frame" in sections
    assert ".debug_line" in run("readelf", "-SW", debug)
    addresses = []
    for line in run("nm", "-nC", debug).splitlines():
        fields = line.split(maxsplit=2)
        if len(fields) != 3 or fields[1] not in "Tt" or not any(s in fields[2] for s in ("aizome::", "aizome_", "myaku::", "sumi::")):
            continue
        addresses.append(fields[0])
    # Load compressed DWARF once per binary, rather than once per symbol.
    locations = subprocess.check_output(["addr2line", "-e", str(debug)],
                                       input="\n".join(addresses) + "\n", text=True)
    source = next((line for line in locations.splitlines()
                   if "/build/aizome-source/" in line and ".rs:" in line
                   and not line.endswith(":0")), None)
    assert source, binary
    results.append({"binary": relative, "build_id": identifier, "source_lookup": source,
                    "runtime_has_dwarf": False, "unwind_tables_preserved": True})
print(json.dumps({"runtime": str(a.runtime.resolve()), "debug": str(a.debug.resolve()), "verified": results}, indent=2))
