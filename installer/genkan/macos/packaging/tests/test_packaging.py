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
import sys
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


REVISION = "4882dc33652612e7475f467f6373d8b607e5407c"
LOCK = "sha256:" + hashlib.sha256(b"flake.lock").hexdigest()


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def write_candidate(d, serial=2, url_base="https://downloads.sekka.computer/mac/v1/releases/sekka-j514s-x/"):
    """A candidate directory: the catalog's five artifacts, the DMG, which
    the catalog doesn't name, and a ledger whose candidate records them
    all. Its rows are the stand-in tool's business."""
    files = {"payload.zip": b"payload", "installer_data.json": b"{}", "manifest.json": b"{}", "engine.tar.gz": b"engine",
             "recovery.zip": b"recovery", "Sekka-Installer-0.1.0.dmg": b"dmg"}
    for name, data in files.items():
        with open(os.path.join(d, name), "wb") as f:
            f.write(data)
    artifacts = {}
    for role, name in [("payload", "payload.zip"), ("installer_data", "installer_data.json"), ("manifest", "manifest.json"), ("engine", "engine.tar.gz"), ("recovery_app", "recovery.zip")]:
        artifacts[role] = {"url": url_base + name, "size": len(files[name]), "digest": digest(files[name])}
    release = {"id": "sekka-j514s-x", "source": {"repository": "bitemyapp/sekka", "revision": REVISION, "lock": LOCK}, "artifacts": artifacts}
    catalog = {"catalog": {"channel": "candidate", "serial": serial, "releases": [release]}}
    with open(os.path.join(d, "catalog.signed.json"), "w") as f:
        json.dump(catalog, f)
    candidate = {"source_revision": REVISION, "lock": LOCK, "created": "2026-11-06T21:00:00Z", "toolchain": "test",
                 "artifacts": [{"name": n, "size": len(b), "digest": digest(b)} for n, b in sorted(files.items())]}
    write_ledger(d, {"schema": "sekka.mac.ledger/1", "candidate": candidate, "rows": []})


def read_ledger(d):
    with open(os.path.join(d, "ledger.json")) as f:
        return json.load(f)


def write_ledger(d, ledger):
    with open(os.path.join(d, "ledger.json"), "w") as f:
        json.dump(ledger, f)


# Stands in for sekka-mac-tool: records each call, and refuses a command
# when the test has left refuse-<command> beside it, saying what that file
# holds as the tool would.
STUB = """#!{python} -I
import os, sys
here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "calls"), "a") as f:
    f.write(" ".join(sys.argv[1:]) + "\\n")
refusal = os.path.join(here, "refuse-" + sys.argv[1])
if os.path.exists(refusal):
    with open(refusal) as f:
        sys.stdout.write(f.read())
    sys.stderr.write("sekka-mac-tool: refused\\n")
    sys.exit(1)
"""


class PublishTest(unittest.TestCase):
    def setUp(self):
        self.bin = tempfile.TemporaryDirectory()
        self.tool = os.path.join(self.bin.name, "sekka-mac-tool")
        with open(self.tool, "w") as f:
            f.write(STUB.format(python=sys.executable))
        os.chmod(self.tool, 0o755)
        self.dirs = []

    def tearDown(self):
        self.bin.cleanup()
        for d in self.dirs:
            d.cleanup()

    def candidate(self, **kw):
        d = tempfile.TemporaryDirectory()
        self.dirs.append(d)
        write_candidate(d.name, **kw)
        return d.name

    def plan(self, d, published=None):
        return publish.plan(d, published, tool=self.tool)

    def refuse(self, command, says=""):
        with open(os.path.join(self.bin.name, "refuse-" + command), "w") as f:
            f.write(says)

    def calls(self):
        with open(os.path.join(self.bin.name, "calls")) as f:
            return f.read().splitlines()

    def test_a_qualified_candidate_is_planned_but_not_uploaded(self):
        d = self.candidate()
        uploads, problems = self.plan(d)
        self.assertEqual(problems, [])
        self.assertEqual(len(uploads), 6)
        self.assertTrue(uploads[-1]["last"], "the catalog goes up after the bytes it names")
        self.assertIn(f"ledger check {os.path.join(d, 'ledger.json')}", self.calls())

    def test_the_tool_decides_whether_the_ledger_is_complete(self):
        d = self.candidate()
        self.refuse("ledger", "M0: 0 pass, 0 not applicable, 0 other, of 0\nproblem: M0-01 is missing: no required case may be dropped\n")
        problems = self.plan(d)[1]
        self.assertIn("the ledger isn't complete: M0-01 is missing: no required case may be dropped", problems)
        self.assertIn("the ledger isn't complete: refused", problems)
        self.assertFalse(any("M0: " in p for p in problems), problems)
        os.unlink(os.path.join(d, "ledger.json"))
        self.assertIn("there is no qualification ledger (ledger.json)", self.plan(d)[1])

    def test_the_channel_file_is_planned_only_if_it_verifies(self):
        d = self.candidate()
        self.plan(d)
        self.assertIn(f"verify {os.path.join(d, 'catalog.signed.json')}", self.calls())
        self.refuse("verify")
        uploads, problems = self.plan(d)
        self.assertEqual(uploads, [])
        self.assertEqual(problems, ["catalog.signed.json doesn't verify, so no installer would accept it: refused"])
        os.unlink(os.path.join(d, "catalog.signed.json"))
        self.assertIn("there is no signed catalog (catalog.signed.json)", self.plan(d)[1])

    def test_without_the_tool_nothing_is_promoted(self):
        d = self.candidate()
        problems = publish.plan(d, tool=os.path.join(self.bin.name, "missing"))[1]
        self.assertTrue(any("can't be run" in p for p in problems), problems)

    def test_a_ledger_for_other_bytes_or_another_candidate_is_refused(self):
        def changed(edit):
            d = self.candidate()
            ledger = read_ledger(d)
            edit(ledger["candidate"])
            write_ledger(d, ledger)
            return self.plan(d)[1]

        def payload(c):
            return [a for a in c["artifacts"] if a["name"] == "payload.zip"][0]

        self.assertTrue(any("doesn't record payload.zip" in p for p in changed(lambda c: payload(c).update(digest=digest(b"rc1")))))
        self.assertTrue(any("isn't the bytes the ledger's candidate records" in p for p in changed(lambda c: payload(c).update(size=1))))
        self.assertTrue(any("doesn't record payload.zip" in p for p in changed(lambda c: c["artifacts"].remove(payload(c)))))
        self.assertTrue(any("records payload.zip twice" in p for p in changed(lambda c: c["artifacts"].append(dict(payload(c))))))
        self.assertTrue(any("isn't a file name" in p for p in changed(lambda c: payload(c).update(name="../payload.zip"))))
        self.assertTrue(any("is built from" in p for p in changed(lambda c: c.update(source_revision="0" * 40))))
        self.assertTrue(any("ships the lock" in p for p in changed(lambda c: c.update(lock=digest(b"other")))))
        d = self.candidate()
        os.unlink(os.path.join(d, "Sekka-Installer-0.1.0.dmg"))
        self.assertIn("Sekka-Installer-0.1.0.dmg, which the ledger's candidate records, isn't here", self.plan(d)[1])

    def test_changed_bytes_stale_serials_and_mutable_paths_are_refused(self):
        d = self.candidate()
        with open(os.path.join(d, "payload.zip"), "wb") as f:
            f.write(b"different")
        self.assertTrue(any("differs" in p for p in self.plan(d)[1]))
        d, old = self.candidate(serial=2), self.candidate(serial=2)
        self.assertTrue(any("isn't newer" in p for p in self.plan(d, os.path.join(old, "catalog.signed.json"))[1]))
        d = self.candidate(url_base="https://downloads.sekka.computer/mac/v1/latest/")
        self.assertTrue(any("isn't under" in p for p in self.plan(d)[1]))


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
