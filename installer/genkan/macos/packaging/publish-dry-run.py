#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check a Mac release candidate for promotion and print exactly what an
authorized publication would upload. It never uploads anything: it has no
credentials and no upload code. Publication is a separate, explicitly
authorized operation (DEVELOPERS.md, Releases).

    publish-dry-run.py CANDIDATE_DIR [--tool SEKKA_MAC_TOOL] [--published CURRENT_SIGNED_CATALOG]

CANDIDATE_DIR holds the signed catalog (catalog.signed.json), the
qualification ledger (ledger.json) and every artifact the ledger's
candidate records, under the names it records them by. Each artifact a
catalog release names is there under the file name its URL ends with.
--tool names sekka-mac-tool (by default the one on PATH), which decides
whether the ledger is complete; this script doesn't keep its own list of
cases. Refused when:

- the ledger isn't complete: `sekka-mac-tool ledger check` refuses it;
- the ledger is for another candidate: an artifact its candidate records
  isn't here with that size and digest, an artifact the catalog names isn't
  among them with the catalog's size and digest, or a release's source
  revision or lock isn't the candidate's;
- an artifact is missing, or its size or digest differs from the catalog;
- an artifact's URL isn't under an immutable release path
  (https://downloads.sekka.computer/mac/v1/releases/<release>/<file>);
- the catalog's serial isn't greater than the published catalog's on the
  same channel (stale or concurrent promotion).
"""
import argparse
import importlib.util
import json
import os
import subprocess
import sys

BASE = "https://downloads.sekka.computer/mac/v1/"
HERE = os.path.dirname(os.path.abspath(__file__))


def _provenance():
    spec = importlib.util.spec_from_file_location("provenance", os.path.join(HERE, "provenance.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# (size, "sha256:…") of a file, or of a directory such as an app bundle, the
# way build-app.sh's build.json records it.
measure = _provenance().sha256_path


def ask(tool, *args):
    """Whether sekka-mac-tool accepted, and the lines saying why not."""
    try:
        r = subprocess.run([tool, *args], capture_output=True, text=True)
    except OSError as e:
        return False, [f"{tool} can't be run ({e.strerror}); name sekka-mac-tool with --tool"]
    if r.returncode == 0:
        return True, []
    said = []
    for line in (r.stdout + r.stderr).splitlines():
        for prefix in ("problem: ", "sekka-mac-tool: "):
            if line.startswith(prefix):
                said.append(line[len(prefix):])
    return False, said or [f"{tool} exited with status {r.returncode}"]


def read_json(path):
    with open(path, "rb") as f:
        return json.load(f)


def ledger_candidate(candidate, tool):
    """Problems with the ledger, and its candidate record when the tool
    calls the ledger complete."""
    path = os.path.join(candidate, "ledger.json")
    if not os.path.isfile(path):
        return ["there is no qualification ledger (ledger.json)"], None
    ok, said = ask(tool, "ledger", "check", path)
    if not ok:
        return [f"the ledger isn't complete: {line}" for line in said], None
    try:
        return [], read_json(path)["candidate"]
    except (OSError, ValueError, KeyError, TypeError) as e:
        return [f"the ledger's candidate can't be read: {e}"], None


def recorded_artifacts(candidate, record):
    """The candidate's artifacts by name, each checked against the bytes
    here."""
    problems, recorded = [], {}
    for a in record["artifacts"]:
        name = a["name"]
        if name in recorded:
            problems.append(f"the ledger's candidate records {name} twice")
        recorded[name] = (a["size"], a["digest"])
        path = os.path.join(candidate, name)
        if name in ("", ".", "..") or os.path.basename(name) != name:
            problems.append(f"the ledger's candidate records {name!r}, which isn't a file name")
        elif not os.path.exists(path):
            problems.append(f"{name}, which the ledger's candidate records, isn't here")
        elif measure(path) != recorded[name]:
            problems.append(f"{name} isn't the bytes the ledger's candidate records")
    return problems, recorded


def plan(candidate, published=None, tool="sekka-mac-tool"):
    problems, record = ledger_candidate(candidate, tool)
    recorded = None
    if record is not None:
        found, recorded = recorded_artifacts(candidate, record)
        problems += found
    catalog = read_json(os.path.join(candidate, "catalog.signed.json"))["catalog"]
    uploads = []
    for release in catalog["releases"]:
        if record is not None:
            source = release["source"]
            if source["revision"] != record["source_revision"]:
                problems.append(f"{release['id']} is built from {source['revision']}, "
                                f"but the ledger qualified {record['source_revision']}")
            if source["lock"] != record["lock"]:
                problems.append(f"{release['id']} ships the lock {source['lock']}, "
                                f"but the ledger qualified {record['lock']}")
        prefix = f"{BASE}releases/{release['id']}/"
        for role, artifact in sorted(release["artifacts"].items()):
            url = artifact["url"]
            if not url.startswith(prefix) or "/" in url[len(prefix):]:
                problems.append(f"{release['id']} {role}: {url} isn't under {prefix}")
                continue
            name = url[len(prefix):]
            path = os.path.join(candidate, name)
            if not os.path.exists(path):
                problems.append(f"{release['id']} {role}: {name} is missing")
                continue
            size, digest = measure(path)
            if size != artifact["size"] or digest != artifact["digest"]:
                problems.append(f"{release['id']} {role}: {name} differs from the catalog")
            if recorded is not None and recorded.get(name) != (artifact["size"], artifact["digest"]):
                problems.append(f"{release['id']} {role}: the ledger's candidate doesn't record {name} "
                                "with the catalog's size and digest")
            uploads.append({"put": url, "from": path, "size": size, "digest": digest, "immutable": True})
    if published:
        current = read_json(published)["catalog"]
        if current["channel"] == catalog["channel"] and catalog["serial"] <= current["serial"]:
            problems.append(
                f"catalog {catalog['serial']} isn't newer than the published {current['serial']} on {catalog['channel']}")
    uploads.append({
        "put": f"{BASE}catalog-{catalog['channel']}.signed.json",
        "from": os.path.join(candidate, "catalog.signed.json"),
        "last": True,
        "immutable": False,
    })
    return uploads, problems


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    parser = argparse.ArgumentParser(prog="publish-dry-run.py", description="Check a Mac release candidate for promotion.")
    parser.add_argument("candidate", metavar="CANDIDATE_DIR")
    parser.add_argument("--tool", default="sekka-mac-tool", metavar="SEKKA_MAC_TOOL")
    parser.add_argument("--published", metavar="CURRENT_SIGNED_CATALOG")
    args = parser.parse_args(argv)
    uploads, problems = plan(args.candidate, args.published, args.tool)
    for p in problems:
        print(f"refused: {p}")
    if problems:
        return 1
    for u in uploads:
        print(f"would upload {u['from']} to {u['put']}")
    print("Dry run: nothing was uploaded. Publishing needs an explicit, authorized request.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
