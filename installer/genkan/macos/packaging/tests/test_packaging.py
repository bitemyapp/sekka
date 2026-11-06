# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for the packaging scripts that run anywhere (the build, signing
and notarization themselves need a Mac).

    python3 -I -m unittest discover -s installer/genkan/macos/packaging/tests
"""
import hashlib
import importlib.util
import io
import json
import os
import tarfile
import tempfile
import unittest
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), os.path.join(HERE, "..", name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


audit = load("audit-macho")
publish = load("publish-dry-run")
provenance = load("provenance")

MACHO = b"\xcf\xfa\xed\xfe" + b"\x0c\x00\x00\x01" + b"\x00" * 64
JAVA = b"\xca\xfe\xba\xbe" + b"\x00\x00\x00\x41" + b"\x00" * 16


class AuditTest(unittest.TestCase):
    def test_finds_binaries_inside_nested_archives(self):
        with tempfile.TemporaryDirectory() as d:
            inner = io.BytesIO()
            with tarfile.open(fileobj=inner, mode="w:gz") as t:
                for name, data in [("engine/bin/python3", MACHO), ("engine/lib/A.class", JAVA), ("engine/README", b"text")]:
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    t.addfile(info, io.BytesIO(data))
            outer = os.path.join(d, "release.zip")
            with zipfile.ZipFile(outer, "w") as z:
                z.writestr("Engine/engine.tar.gz", inner.getvalue())
                z.writestr("MacOS/app", MACHO)
            found = audit.scan(outer)
            names = sorted((m["path"], tuple(m["inside"])) for m in found)
            self.assertEqual(names, [
                ("MacOS/app", (outer,)),
                ("engine/bin/python3", (outer, "Engine/engine.tar.gz")),
            ])


def write_candidate(d, serial=2, url_base="https://downloads.sekka.computer/mac/v1/releases/sekka-j514s-x/"):
    files = {"payload.zip": b"payload", "installer_data.json": b"{}", "manifest.json": b"{}", "engine.tar.gz": b"engine", "recovery.zip": b"recovery"}
    artifacts = {}
    for role, name in [("payload", "payload.zip"), ("installer_data", "installer_data.json"), ("manifest", "manifest.json"), ("engine", "engine.tar.gz"), ("recovery_app", "recovery.zip")]:
        with open(os.path.join(d, name), "wb") as f:
            f.write(files[name])
        artifacts[role] = {"url": url_base + name, "size": len(files[name]), "digest": "sha256:" + hashlib.sha256(files[name]).hexdigest()}
    catalog = {"catalog": {"channel": "candidate", "serial": serial, "releases": [{"id": "sekka-j514s-x", "artifacts": artifacts}]}}
    json.dump(catalog, open(os.path.join(d, "catalog.signed.json"), "w"))
    rows = [{"id": "M1-01", "status": "pass", "observed": "ok", "evidence": "runs/1"},
            {"id": "M7-01", "status": "not-applicable", "reason": "deferred"}]
    json.dump({"rows": rows}, open(os.path.join(d, "ledger.json"), "w"))


class PublishTest(unittest.TestCase):
    def test_a_qualified_candidate_is_planned_but_not_uploaded(self):
        with tempfile.TemporaryDirectory() as d:
            write_candidate(d)
            uploads, problems = publish.plan(d)
            self.assertEqual(problems, [])
            self.assertEqual(len(uploads), 6)
            self.assertTrue(uploads[-1]["last"], "the catalog goes up after the bytes it names")

    def test_changed_bytes_unqualified_candidates_and_stale_serials_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            write_candidate(d)
            with open(os.path.join(d, "payload.zip"), "wb") as f:
                f.write(b"different")
            self.assertTrue(any("differs" in p for p in publish.plan(d)[1]))
        with tempfile.TemporaryDirectory() as d:
            write_candidate(d)
            ledger = json.load(open(os.path.join(d, "ledger.json")))
            ledger["rows"][0]["status"] = "not-run"
            json.dump(ledger, open(os.path.join(d, "ledger.json"), "w"))
            self.assertTrue(any("not-run" in p for p in publish.plan(d)[1]))
        with tempfile.TemporaryDirectory() as d, tempfile.TemporaryDirectory() as old:
            write_candidate(d, serial=2)
            write_candidate(old, serial=2)
            self.assertTrue(any("isn't newer" in p for p in publish.plan(d, os.path.join(old, "catalog.signed.json"))[1]))
        with tempfile.TemporaryDirectory() as d:
            write_candidate(d, url_base="https://downloads.sekka.computer/mac/v1/latest/")
            self.assertTrue(any("isn't under" in p for p in publish.plan(d)[1]))


class ProvenanceTest(unittest.TestCase):
    def test_records_crates_and_artifacts(self):
        workspace = os.path.join(HERE, "..", "..")
        with tempfile.NamedTemporaryFile(delete=False) as t:
            t.write(b"artifact")
        try:
            doc = provenance.record(workspace, False, [t.name])
        finally:
            os.unlink(t.name)
        self.assertEqual(doc["schema"], "sekka.mac.provenance/1")
        self.assertTrue(any(c["name"] == "ed25519-dalek" for c in doc["crates"]))
        self.assertEqual(doc["artifacts"][0]["digest"], "sha256:" + hashlib.sha256(b"artifact").hexdigest())


if __name__ == "__main__":
    unittest.main()
