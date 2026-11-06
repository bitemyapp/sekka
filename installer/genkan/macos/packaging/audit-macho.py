#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Find every Mach-O in a release, including those inside zip and tar
archives, and (on macOS, with --verify) check each is signed, by the
release's team when --team is given.

Apple's notarization inspects archived binaries too; an unsigned one
inside the engine archive or recovery.zip fails it. Prints JSON:
{"machos": [{"path", "inside", "kinds", "signed", "team", "allowed"}], "problems": [...]}.

    audit-macho.py [--verify] [--team TEAMID] [--archive-team ARCHIVE=TEAMID]... PATH...

--archive-team allows one more team for the Mach-Os inside archives named
ARCHIVE (by file name, at any depth): the engine archive carries Asahi's
Python runtime exactly as Asahi ships it, signed by its own team, and
re-signing it would change the engine's digest. Every other Mach-O must
be signed by --team.
"""
import argparse
import io
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import zipfile

THIN = {
    b"\xfe\xed\xfa\xce": "macho32-be",
    b"\xce\xfa\xed\xfe": "macho32",
    b"\xfe\xed\xfa\xcf": "macho64-be",
    b"\xcf\xfa\xed\xfe": "macho64",
}
FAT = {
    b"\xca\xfe\xba\xbe": "universal",
    b"\xbe\xba\xfe\xca": "universal-le",
}
MAGIC = {**THIN, **FAT}


def kind(data):
    """The Mach-O kind of a file's bytes, or None. A universal file counts
    only when one of its slices is a Mach-O: Java class files share the
    magic, and universal static archives (`.a`) and bitcode (`python.o`)
    hold no Mach-O and carry no signature of their own."""
    magic = data[:4]
    if magic not in FAT:
        return THIN.get(magic)
    k = FAT[magic]
    if len(data) < 8:
        return None
    order = "big" if k == "universal" else "little"
    count = int.from_bytes(data[4:8], order)
    if count == 0 or count > 30 or len(data) < 8 + 20 * count:
        return None
    for i in range(count):
        entry = data[8 + 20 * i:28 + 20 * i]
        offset = int.from_bytes(entry[8:12], order)
        size = int.from_bytes(entry[12:16], order)
        if size >= 4 and offset + size <= len(data) and data[offset:offset + 4] in THIN:
            return k
    return None


def scan_bytes(name, data, inside, found):
    k = kind(data)
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
            head = f.read(4)
        if head in MAGIC:
            with open(p, "rb") as f:
                data = f.read()
            if kind(data):
                found.append({"path": p, "inside": [], "kinds": kind(data), "data": data})
        elif p.endswith(".zip"):
            scan_zip(p, [p], found)
        elif p.endswith((".tar.gz", ".tgz", ".tar")):
            with open(p, "rb") as f:
                scan_tar(f, [p], found)
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


def allowed_teams(team, archive_teams, inside):
    """The teams that may sign a Mach-O found inside these archives."""
    teams = [team]
    for archive in inside:
        extra = archive_teams.get(os.path.basename(archive))
        if extra and extra not in teams:
            teams.append(extra)
    return teams


def audit(paths, verify=False, team=None, archive_teams=None):
    archive_teams = archive_teams or {}
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
                elif team:
                    teams = allowed_teams(team, archive_teams, m["inside"])
                    m["allowed"] = teams
                    if signer not in teams:
                        problems.append(f"{where} is signed by {signer}, not {' or '.join(teams)}")
            machos.append(m)
    return machos, problems


def archive_team(value):
    archive, sep, team = value.partition("=")
    if not sep or not archive or not team or "/" in archive:
        raise argparse.ArgumentTypeError(f"{value!r} isn't ARCHIVE=TEAMID")
    return archive, team


def main(argv):
    parser = argparse.ArgumentParser(prog="audit-macho.py", description="Find and check every Mach-O in a release.")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--team")
    parser.add_argument("--archive-team", type=archive_team, action="append", default=[], metavar="ARCHIVE=TEAMID")
    parser.add_argument("paths", nargs="+", metavar="PATH")
    args = parser.parse_args(argv)
    machos, problems = audit(args.paths, args.verify, args.team, dict(args.archive_team))
    json.dump({"machos": machos, "problems": problems}, sys.stdout, indent=2)
    print()
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
