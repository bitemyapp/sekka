#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Find every Mach-O in a release, including those inside zip and tar
archives, and (on macOS, with --verify) check each is signed, by the
release's team when --team is given.

Apple's notarization inspects archived binaries too; an unsigned one
inside the engine archive or recovery.zip fails it. Prints JSON:
{"machos": [{"path", "inside", "kinds", "signed", "team"}], "problems": [...]}.

    audit-macho.py [--verify] [--team TEAMID] PATH...
"""
import io
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import zipfile

MAGIC = {
    b"\xfe\xed\xfa\xce": "macho32-be",
    b"\xce\xfa\xed\xfe": "macho32",
    b"\xfe\xed\xfa\xcf": "macho64-be",
    b"\xcf\xfa\xed\xfe": "macho64",
    b"\xca\xfe\xba\xbe": "universal",
    b"\xbe\xba\xfe\xca": "universal-le",
}


def kind(head):
    """The Mach-O kind of a file's first bytes, or None. A universal
    header's second word is its architecture count, small; Java class files
    share the magic but have a version there."""
    k = MAGIC.get(head[:4])
    if k in ("universal", "universal-le") and len(head) >= 8:
        count = int.from_bytes(head[4:8], "big" if k == "universal" else "little")
        if count == 0 or count > 30:
            return None
    return k


def scan_bytes(name, data, inside, found):
    k = kind(data[:8])
    if k:
        found.append({"path": name, "inside": inside, "kinds": k, "data": data})
    elif name.endswith(".zip"):
        scan_zip(io.BytesIO(data), inside + [name], found)
    elif name.endswith((".tar.gz", ".tgz", ".tar")):
        scan_tar(io.BytesIO(data), inside + [name], found)


def scan_zip(fileobj, inside, found):
    with zipfile.ZipFile(fileobj) as z:
        for info in z.infolist():
            if not info.is_dir():
                scan_bytes(info.filename, z.read(info), inside, found)


def scan_tar(fileobj, inside, found):
    with tarfile.open(fileobj=fileobj, mode="r:*") as t:
        for member in t:
            if member.isfile():
                data = t.extractfile(member).read()
                scan_bytes(member.name, data, inside, found)


def scan(path):
    found = []
    targets = [path]
    if os.path.isdir(path):
        targets = [os.path.join(d, f) for d, _, fs in os.walk(path) for f in fs]
    for p in sorted(targets):
        if os.path.islink(p):
            continue
        with open(p, "rb") as f:
            head = f.read(8)
        if kind(head):
            with open(p, "rb") as f:
                found.append({"path": p, "inside": [], "kinds": kind(head), "data": f.read()})
        elif p.endswith(".zip"):
            scan_zip(p, [p], found)
        elif p.endswith((".tar.gz", ".tgz", ".tar")):
            scan_tar(p, [p], found)
    return found


def signature(data):
    """(signed, team) for a Mach-O's bytes, asking codesign."""
    with tempfile.NamedTemporaryFile(delete=False) as t:
        t.write(data)
    try:
        r = subprocess.run(["/usr/bin/codesign", "-dv", "--verbose=2", t.name],
                           capture_output=True, text=True)
        if r.returncode != 0:
            return False, None
        team = None
        for line in r.stderr.splitlines():
            if line.startswith("TeamIdentifier="):
                team = line.split("=", 1)[1].strip()
        return True, team
    finally:
        os.unlink(t.name)


def main(argv):
    verify = "--verify" in argv
    team = None
    paths = []
    i = 0
    while i < len(argv):
        if argv[i] == "--verify":
            pass
        elif argv[i] == "--team":
            team = argv[i + 1]
            i += 1
        else:
            paths.append(argv[i])
        i += 1
    machos, problems = [], []
    for p in paths:
        for m in scan(p):
            data = m.pop("data")
            if verify:
                signed, signer = signature(data)
                m["signed"], m["team"] = signed, signer
                where = " inside ".join([m["path"]] + list(reversed(m["inside"])))
                if not signed:
                    problems.append(f"{where} isn't signed")
                elif team and signer != team:
                    problems.append(f"{where} is signed by {signer}, not {team}")
            machos.append(m)
    json.dump({"machos": machos, "problems": problems}, sys.stdout, indent=2)
    print()
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
