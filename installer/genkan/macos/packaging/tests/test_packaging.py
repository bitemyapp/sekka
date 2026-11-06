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
import subprocess
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


def universal(*slices):
    """A big-endian universal file holding these slices."""
    header = b"\xca\xfe\xba\xbe" + len(slices).to_bytes(4, "big")
    offset = len(header) + 20 * len(slices)
    table, body = b"", b""
    for s in slices:
        table += (0x0100000C).to_bytes(4, "big") + bytes(4) + (offset + len(body)).to_bytes(4, "big") + len(s).to_bytes(4, "big") + (3).to_bytes(4, "big")
        body += s
    return header + table + body


def tar_gz(members):
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode="w:gz") as t:
        for name, content in members:
            info = tarfile.TarInfo(name)
            info.size = len(content)
            t.addfile(info, io.BytesIO(content))
    return data.getvalue()


class AuditTest(unittest.TestCase):
    def test_universal_archives_and_bitcode_are_not_mach_os(self):
        self.assertEqual(audit.kind(universal(MACHO, MACHO)), "universal")
        self.assertIsNone(audit.kind(universal(b"!<arch>\n" + bytes(64), b"!<arch>\n" + bytes(64))), "libtclstub8.6.a")
        self.assertIsNone(audit.kind(universal(b"\xde\xc0\x17\x0b" + bytes(64))), "python.o")
        self.assertIsNone(audit.kind(JAVA))
        self.assertEqual(audit.kind(MACHO), "macho64")

    def test_the_app_and_the_engine_archive_inside_it(self):
        """The engine is a file in the app, not a member of an archive."""
        with tempfile.TemporaryDirectory() as d:
            app = os.path.join(d, "Sekka Installer.app")
            os.makedirs(os.path.join(app, "Contents", "MacOS"))
            os.makedirs(os.path.join(app, "Contents", "Resources", "Engine"))
            with open(os.path.join(app, "Contents", "MacOS", "sekka-installer"), "wb") as f:
                f.write(MACHO + b"sekka")
            engine = os.path.join(app, "Contents", "Resources", "Engine", "engine.tar.gz")
            with open(engine, "wb") as f:
                f.write(tar_gz([("Frameworks/Python.framework/Versions/3.13/bin/python3.13", universal(MACHO + b"psf")),
                                ("Frameworks/Tcl.framework/libtclstub8.6.a", universal(b"!<arch>\n" + bytes(64))),
                                ("sekka_engine/__main__.py", b"print()")]))
            found = sorted((m["path"], tuple(m["inside"])) for m in audit.scan(app))
            self.assertEqual(found, [
                (os.path.join(app, "Contents", "MacOS", "sekka-installer"), ()),
                ("Frameworks/Python.framework/Versions/3.13/bin/python3.13", (engine,)),
            ])

            def signed_by(data):
                return True, "BMM5U3QVKW" if b"psf" in data else "SEKKATEAM1" if b"sekka" in data else "not set"

            saved = audit.signature
            audit.signature = signed_by
            try:
                self.assertEqual(audit.audit([app], True, "SEKKATEAM1", {"engine.tar.gz": "BMM5U3QVKW"})[1], [])
                [problem] = audit.audit([app], True, "SEKKATEAM1")[1]
                self.assertIn("python3.13 inside", problem)
                self.assertIn("is signed by BMM5U3QVKW, not SEKKATEAM1", problem)
                # Another archive name, or another team, isn't allowed.
                self.assertEqual(len(audit.audit([app], True, "SEKKATEAM1", {"recovery.zip": "BMM5U3QVKW"})[1]), 1)
                self.assertEqual(len(audit.audit([app], True, "SEKKATEAM1", {"engine.tar.gz": "OTHERTEAM1"})[1]), 1)
                # Sekka's own executables are Sekka's team's, whatever the engine's is.
                audit.signature = lambda data: (True, "BMM5U3QVKW")
                [problem] = audit.audit([app], True, "SEKKATEAM1", {"engine.tar.gz": "BMM5U3QVKW"})[1]
                self.assertIn("sekka-installer is signed by BMM5U3QVKW, not SEKKATEAM1", problem)
                audit.signature = lambda data: (True, "not set")
                self.assertEqual(len(audit.audit([app], True, "SEKKATEAM1", {"engine.tar.gz": "BMM5U3QVKW"})[1]), 2)
            finally:
                audit.signature = saved

    def test_archive_teams_name_an_archive_and_a_team(self):
        self.assertEqual(audit.archive_team("engine.tar.gz=BMM5U3QVKW"), ("engine.tar.gz", "BMM5U3QVKW"))
        for bad in ("engine.tar.gz", "=BMM5U3QVKW", "engine.tar.gz=", "Engine/engine.tar.gz=BMM5U3QVKW"):
            with self.assertRaises(Exception):
                audit.archive_team(bad)

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


class BuildTest(unittest.TestCase):
    """build-app.sh's checks of what it is given. A stand-in uname says
    this isn't a Mac, so a run that passes them stops at the platform check
    and builds nothing, on any host."""

    NOT_A_MAC = "Sekka Installer is built on macOS."

    def setUp(self):
        self.t = tempfile.TemporaryDirectory()
        stubs = os.path.join(self.t.name, "bin")
        os.mkdir(stubs)
        with open(os.path.join(stubs, "uname"), "w") as f:
            f.write("#!/bin/sh\necho Linux\n")
        os.chmod(os.path.join(stubs, "uname"), 0o755)
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("SEKKA_")}
        self.env["PATH"] = stubs + os.pathsep + os.environ.get("PATH", "")
        self.engine = os.path.join(self.t.name, "engine.tar.gz")
        with open(self.engine, "wb") as f:
            f.write(b"engine")

    def tearDown(self):
        self.t.cleanup()

    def build(self, *args, **env):
        r = subprocess.run(["bash", os.path.join(HERE, "..", "build-app.sh"), *args],
                           env={**self.env, **env}, capture_output=True, text=True)
        return r.returncode, r.stderr

    def test_a_release_names_the_engine_runtimes_pinned_team(self):
        with open(os.path.join(HERE, "..", "..", "engine", "source-lock.json")) as f:
            pinned = json.load(f)["asahi_installer"]["runtime_team"]
        release = ("--release", "--engine", self.engine, "--icon", self.engine)
        team = {"SEKKA_TEAM_ID": "SEKKATEAM1", "SEKKA_SIGNING_IDENTITY": "Developer ID Application: Sekka (SEKKATEAM1)"}
        code, said = self.build(*release, **team)
        self.assertNotEqual(code, 0)
        self.assertIn("SEKKA_ENGINE_TEAM_ID", said)
        code, said = self.build(*release, **team, SEKKA_ENGINE_TEAM_ID="OTHERTEAM1")
        self.assertEqual(code, 1, said)
        self.assertIn(f"pins the runtime's team as {pinned}", said)
        code, said = self.build(*release, **team, SEKKA_ENGINE_TEAM_ID=pinned)
        self.assertEqual(code, 2, said)
        self.assertIn(self.NOT_A_MAC, said)
        # A development build needs none of it.
        code, said = self.build("--engine", self.engine)
        self.assertIn(self.NOT_A_MAC, said)


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
