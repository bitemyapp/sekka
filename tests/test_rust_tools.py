"""Account setup and retry policy; network and toolchain changes are simulated."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

PREPARE = sys.argv.pop(1)
MOCK = r'''
import json, os, sys
from pathlib import Path
home = Path(os.environ["RUSTUP_HOME"])
args = sys.argv[1:]
with (home / "calls").open("a") as log:
    log.write(json.dumps(args) + "\n")
settings = home / "settings.toml"
state = json.loads(settings.read_text()) if settings.exists() else {}
if args == ["show", "profile"]:
    settings.write_text(json.dumps(state))
    print(state.get("profile", "default"))
elif args == ["default"]:
    if "default" not in state:
        sys.exit("error: no default toolchain is configured")
    print(state["default"] + " (default)")
elif args[:2] in (["toolchain", "install"], ["component", "add"]):
    if os.environ.get("TEST_OFFLINE"):
        sys.exit("network unavailable")
    if args[:2] == ["toolchain", "install"]:
        state["default"] = "stable-x86_64-unknown-linux-gnu"
    state["editor_components"] = True
    settings.write_text(json.dumps(state))
else:
    sys.exit("unexpected rustup command: " + repr(args))
'''


class RustTools(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "custom-rustup-home"
        self.home.mkdir()
        shim = self.root / "rustup"
        shim.write_text(f"#!{sys.executable}\n" + MOCK)
        shim.chmod(0o755)
        self.env = dict(os.environ, HOME=str(self.root), RUSTUP_HOME=str(self.home),
                        PATH=str(self.root) + os.pathsep + os.environ["PATH"])

    def settings(self, **state):
        (self.home / "settings.toml").write_text(json.dumps(state))

    def run_setup(self, offline=False):
        env = dict(self.env)
        if offline:
            env["TEST_OFFLINE"] = "1"
        return subprocess.run(["bash", "-euo", "pipefail", PREPARE], env=env,
                              capture_output=True, text=True)

    def mutations(self):
        calls = [json.loads(line) for line in (self.home / "calls").read_text().splitlines()]
        return [call for call in calls if call[0] in ("toolchain", "component")]

    def test_fresh_account_gets_complete_stable(self):
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.mutations(), [["toolchain", "install", "stable", "--profile",
                          "default", "--component", "rust-analyzer", "--component", "rust-src"]])
        self.assertFalse((self.home / ".sekka-initializing").exists())

    def test_failed_initial_download_retries_after_settings_created(self):
        self.assertNotEqual(self.run_setup(offline=True).returncode, 0)
        self.assertTrue((self.home / "settings.toml").exists())
        self.assertTrue((self.home / ".sekka-initializing").exists())
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.mutations()), 2)
        self.assertFalse((self.home / ".sekka-initializing").exists())

    def test_existing_default_is_repaired_without_updating_or_switching(self):
        self.settings(default="1.98.0-x86_64-unknown-linux-gnu")
        self.env["RUSTUP_TOOLCHAIN"] = "nightly"
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.mutations(), [["component", "add", "--toolchain",
                          "1.98.0-x86_64-unknown-linux-gnu", "rust-analyzer", "rust-src"]])

    def test_failed_repair_is_retryable(self):
        self.settings(default="stable-x86_64-unknown-linux-gnu")
        self.assertNotEqual(self.run_setup(offline=True).returncode, 0)
        self.assertEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.mutations()[0], self.mutations()[1])

    def test_minimal_profile_is_preserved(self):
        self.settings(default="stable-x86_64-unknown-linux-gnu", profile="minimal")
        self.assertEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.mutations(), [])

    def test_explicitly_unset_default_is_preserved(self):
        self.settings()
        self.assertEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.mutations(), [])

    def test_linked_toolchain_is_preserved(self):
        self.settings(default="local")
        (self.home / "toolchains").mkdir()
        (self.home / "toolchains" / "local").symlink_to(self.root)
        self.assertEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.mutations(), [])

    def test_bad_settings_fail_without_installing(self):
        (self.home / "settings.toml").write_text("invalid")
        self.assertNotEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.mutations(), [])


unittest.main()
