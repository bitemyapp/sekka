#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check a Mac release candidate for promotion and print exactly what an
authorized publication would upload. It never uploads anything: it has no
credentials and no upload code. Publication is a separate, explicitly
authorized operation (DEVELOPERS.md, Releases).

    publish-dry-run.py CANDIDATE_DIR [--published CURRENT_SIGNED_CATALOG]

CANDIDATE_DIR holds the signed catalog (catalog.signed.json), the
qualification ledger (ledger.json) and every artifact the catalog's
releases name, under the file names their URLs end with. Refused when:

- the ledger isn't complete (every case passed or doesn't apply);
- an artifact is missing, or its size or digest differs from the catalog;
- an artifact's URL isn't under an immutable release path
  (https://downloads.sekka.computer/mac/v1/releases/<release>/<file>);
- the catalog's serial isn't greater than the published catalog's on the
  same channel (stale or concurrent promotion).
"""
import hashlib
import json
import os
import sys

BASE = "https://downloads.sekka.computer/mac/v1/"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return "sha256:" + h.hexdigest()


def ledger_complete(ledger):
    problems = []
    for row in ledger.get("rows", []):
        status = row.get("status")
        if status == "pass" and (not row.get("observed") or not row.get("evidence")):
            problems.append(f"{row['id']} passes without evidence")
        elif status == "not-applicable" and not row.get("reason"):
            problems.append(f"{row['id']} doesn't apply without a reason")
        elif status not in ("pass", "not-applicable"):
            problems.append(f"{row['id']} is {status}")
    return problems


def plan(candidate, published=None):
    problems = []
    signed = json.load(open(os.path.join(candidate, "catalog.signed.json")))
    catalog = signed["catalog"]
    ledger_path = os.path.join(candidate, "ledger.json")
    if not os.path.exists(ledger_path):
        problems.append("there is no qualification ledger")
    else:
        problems += ledger_complete(json.load(open(ledger_path)))
    uploads = []
    for release in catalog["releases"]:
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
            size = os.path.getsize(path)
            digest = sha256(path)
            if size != artifact["size"] or digest != artifact["digest"]:
                problems.append(f"{release['id']} {role}: {name} differs from the catalog")
            uploads.append({"put": url, "from": path, "size": size, "digest": digest, "immutable": True})
    if published:
        current = json.load(open(published))["catalog"]
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
    published = None
    if "--published" in argv:
        published = argv[argv.index("--published") + 1]
    uploads, problems = plan(argv[0], published)
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
