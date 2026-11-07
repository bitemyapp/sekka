# SPDX-License-Identifier: GPL-3.0-or-later
"""The command line: a fixed set of arguments and nothing else, and an engine
that can't start still speaks the protocol."""

import os
import shutil
import subprocess
import sys
import unittest

import support as S
from sekka_engine import __main__ as cli
from sekka_engine import canonical
from sekka_engine import contract as c
from sekka_engine import provenance


def run_engine_process(root, argv, password=None):
    """Run `-m sekka_engine` isolated (-I) from a copy of the engine at
    `root`, as the helper does; -I keeps the current directory off
    sys.path, so the copy is put there explicitly, as the archive's .pth
    does."""
    prefix = ""
    pass_fds = ()
    r = None
    if password is not None:
        # The password reaches the child on its descriptor 3, as the helper
        # gives it, without touching this process's own descriptor 3.
        r, w = os.pipe()
        os.write(w, password)
        os.close(w)
        prefix = f"import os; os.dup2({r}, 3); os.close({r}); "
        pass_fds = (r,)
    code = prefix + (
        f"import sys, runpy; sys.path.append({root!r}); sys.argv = ['sekka_engine'] + {argv!r}; "
        "runpy.run_module('sekka_engine', run_name='__main__')"
    )
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"}
    try:
        return subprocess.run([sys.executable, "-I", "-c", code], input=b"", capture_output=True, env=env, pass_fds=pass_fds, timeout=120)
    finally:
        if r is not None:
            os.close(r)


def run_by_path(root, argv):
    """The archive's form: the entry point's path, isolated and writing no
    bytecode, as the helper starts it."""
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LC_ALL": "C"}
    return subprocess.run(
        [sys.executable, "-I", "-B", os.path.join(root, "sekka_engine", "__main__.py")] + argv,
        input=b"",
        capture_output=True,
        env=env,
        cwd="/",
        timeout=120,
    )


class CommandLine(unittest.TestCase):
    def test_arguments(self):
        self.assertEqual(cli.parse(["inspect"]), ("inspect", {}))
        mode, options = cli.parse(["execute", "--plan", "p", "--assets", "a", "--installation", "u", "--owner", "o"])
        self.assertEqual(mode, "execute")
        self.assertEqual(options["--owner"], "o")
        mode, options = cli.parse(["startup-disk", "--plan", "p", "--installation", "u", "--owner", "o", "--esp-inventory", "sha256:x"])
        self.assertEqual((mode, sorted(options)), ("startup-disk", ["--esp-inventory", "--installation", "--owner", "--plan"]))
        for bad in (
            ["startup-disk", "--plan", "p", "--assets", "a", "--installation", "u", "--owner", "o"],
            ["startup-disk", "--plan", "p", "--installation", "u"],
            ["startup-disk", "--plan", "p", "--installation", "u", "--owner", "o"],
            ["startup-disk", "--plan", "p", "--installation", "u", "--stage", "root"],
            [],
            ["simulate"],
            ["inspect", "--simulate"],
            ["execute", "--plan", "p"],
            ["execute", "--plan", "p", "--assets", "a", "--installation", "u", "--owner", "o", "--engine", "x"],
            ["execute", "--plan", "p", "--plan", "q", "--installation", "u", "--owner", "o"],
            ["execute", "--plan", "-x", "--assets", "a", "--installation", "u", "--owner", "o"],
            ["execute", "--asahi-root", "/tmp", "--assets", "a", "--installation", "u", "--owner", "o"],
        ):
            with self.assertRaises(ValueError, msg=bad):
                cli.parse(bad)

    def test_a_usage_error_writes_nothing_to_the_transcript(self):
        t = S.TempDir()
        try:
            root = t.sub("engine")
            shutil.copytree(os.path.join(S.ENGINE, "sekka_engine"), os.path.join(root, "sekka_engine"))
            p = run_engine_process(root, ["inspect", "--simulate"])
            self.assertEqual(p.returncode, 2)
            self.assertEqual(p.stdout, b"")
            self.assertIn(b"usage", p.stderr)
        finally:
            t.cleanup()

    def test_an_incomplete_engine_fails_in_the_protocol(self):
        """Without the pinned installer and its lock beside it, the engine
        says so in a transcript the helper can read."""
        t = S.TempDir()
        try:
            root = t.sub("engine")
            shutil.copytree(os.path.join(S.ENGINE, "sekka_engine"), os.path.join(root, "sekka_engine"))
            for argv, password in (
                (["inspect"], None),
                (["startup-disk", "--plan", "/nonexistent", "--installation", "1f7d9c62-4a1b-4c8e-9d3f-2b6a8e0c5d47", "--owner", "callen", "--esp-inventory", "sha256:" + "0" * 64], b"secret"),
                (["execute", "--plan", "/nonexistent", "--assets", "/nonexistent", "--installation", "1f7d9c62-4a1b-4c8e-9d3f-2b6a8e0c5d47", "--owner", "callen"], b"secret"),
            ):
                with self.subTest(mode=argv[0]):
                    p = run_engine_process(root, argv, password)
                    self.assertEqual(p.returncode, 1, p.stderr.decode()[-2000:])
                    lines = p.stdout.splitlines()
                    messages = [canonical.loads(line, c.LIMIT_ENGINE_LINE, c.ENGINE_SCHEMA) for line in lines]
                    self.assertEqual([m["message"]["type"] for m in messages], ["hello", "failure", "completion"])
                    hello = messages[0]["message"]["payload"]
                    self.assertEqual(hello["mode"], argv[0])
                    # The real adapter digest of the files that ran.
                    self.assertEqual(hello["adapter"], provenance.adapter_digest(os.path.join(root, "sekka_engine")))
                    self.assertEqual(messages[1]["message"]["payload"]["failure"]["code"], "engine-damaged")
                    self.assertNotIn(b"secret", p.stdout + p.stderr)
                    validator = os.environ.get("SEKKA_TRANSCRIPT_VALIDATOR")
                    if validator and argv[0] == "inspect":
                        v = subprocess.run([validator, "inspect"], input=p.stdout, capture_output=True)
                        self.assertEqual(v.returncode, 0, v.stdout)
        finally:
            t.cleanup()

    def test_run_by_path_as_the_archive_does(self):
        t = S.TempDir()
        try:
            root = t.sub("engine")
            shutil.copytree(os.path.join(S.ENGINE, "sekka_engine"), os.path.join(root, "sekka_engine"))
            p = run_by_path(root, ["inspect"])
            self.assertEqual(p.returncode, 1, p.stderr.decode()[-2000:])
            types = [canonical.loads(line, c.LIMIT_ENGINE_LINE, c.ENGINE_SCHEMA)["message"]["type"] for line in p.stdout.splitlines()]
            self.assertEqual(types, ["hello", "failure", "completion"])
            self.assertEqual(run_by_path(root, ["inspect", "--simulate"]).returncode, 2)
            # The unpacked engine is left as it was unpacked.
            written = [d for d, _, _ in os.walk(root) if os.path.basename(d) == "__pycache__"]
            self.assertEqual(written, [])
        finally:
            t.cleanup()

    def test_the_diagnostic_log_names_no_administrator(self):
        """Asahi's OSEnum logs every administrator's name; the helper keeps
        this log for diagnostics."""
        import logging

        record = logging.LogRecord("root", logging.INFO, __file__, 1, "  Admin users: %s", (["callen", "other"],), None)
        self.assertTrue(cli.WithoutAccounts().filter(record))
        self.assertEqual(record.getMessage(), "  Admin users: (not logged)")
        record = logging.LogRecord("root", logging.INFO, __file__, 1, "  Found %s", ("Macintosh HD",), None)
        cli.WithoutAccounts().filter(record)
        self.assertEqual(record.getMessage(), "  Found Macintosh HD")

    def test_the_adapter_digest_covers_every_source_file(self):
        files = provenance.adapter_files()
        names = [f[0] for f in files]
        self.assertIn("sekka_engine/engine.py", names)
        self.assertIn("sekka_engine/__main__.py", names)
        self.assertEqual(names, sorted(names))
        self.assertEqual(provenance.adapter_digest(), canonical.digest({"domain": "sekka.mac.adapter/1", "files": files}))


if __name__ == "__main__":
    unittest.main()
