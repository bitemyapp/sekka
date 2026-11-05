"""Frontend contracts that must not silently change Bazel's arguments or trust domain."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("workflow", Path(__file__).with_name("workflow.py"))
workflow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workflow)


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.previous = Path.cwd()
        self.addCleanup(os.chdir, self.previous)
        self.environment = patch.dict(os.environ, {
            "SEKKA_BAZEL_STATE": str(self.root / "state"),
            "SEKKA_BAZEL_REPOSITORIES_RC": "/nix/store/test-repositories.bazelrc",
        })
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def invoke(self, *args):
        with patch("sys.argv", ["sekka-bazel", "--workspace", str(self.root), *args]), \
             patch.object(workflow.subprocess, "run") as run, \
             patch.object(workflow.os, "execvp") as execute:
            run.return_value.returncode = 0
            workflow.main()
            return execute.call_args.args[1]

    def test_bazel_output_is_not_output_base_abbreviation(self):
        argv = self.invoke("cquery", "//desktop/aizome:workspace", "--output=files")
        self.assertIn("--output=files", argv)
        self.assertFalse((self.root / "files").exists())
        self.assertTrue(any(a.startswith("--output_base=" + str(self.root / "state/worktrees")) for a in argv))

    def test_backend_profile_flag_is_passed_after_command(self):
        argv = self.invoke("--profile", "release", "build", "--profile=/tmp/native.profile.gz")
        self.assertIn("--config=release", argv)
        self.assertIn("--profile=/tmp/native.profile.gz", argv)

    def test_default_run_places_target_before_program_arguments(self):
        argv = self.invoke("run", "--", "--backend", "headless")
        self.assertEqual(argv[-4:], ["//desktop/aizome/crates/aizome-comp:aizome", "--", "--backend", "headless"])

    def test_default_build_is_workspace_not_tests(self):
        argv = self.invoke("build", "--jobs=16")
        self.assertIn("//desktop/aizome:workspace", argv)
        self.assertIn("--config=dev", argv)
        self.assertIn("--remote_cache=", argv)

    def test_default_run_has_declared_compositor(self):
        argv = self.invoke("--profile", "release-lto", "run")
        self.assertIn("//desktop/aizome/crates/aizome-comp:aizome", argv)
        self.assertIn("--config=release-lto", argv)

    def test_untrusted_near_cache_is_separate_and_uploads_disabled(self):
        rc = self.root / "private.bazelrc"
        rc.touch(mode=0o600)
        argv = self.invoke("--cache", "untrusted", "--remote-rc", str(rc), "build")
        self.assertIn("--noremote_upload_local_results", argv)
        self.assertIn(f"--disk_cache={self.root}/state/cache/untrusted", argv)
        self.assertIn(f"--bazelrc={rc}", argv)

    def test_remote_requires_explicit_private_credentials(self):
        with self.assertRaises(SystemExit):
            self.invoke("--cache", "trusted", "build")
        rc = self.root / "public.bazelrc"
        rc.touch(mode=0o644)
        with self.assertRaises(SystemExit):
            self.invoke("--cache", "trusted", "--remote-rc", str(rc), "build")

    def test_stale_generation_stops_before_bazel(self):
        with patch("sys.argv", ["sekka-bazel", "--workspace", str(self.root), "build"]), \
             patch.object(workflow.subprocess, "run") as run, \
             patch.object(workflow.os, "execvp") as execute:
            run.return_value.returncode = 1
            with self.assertRaisesRegex(SystemExit, "graph is stale"):
                workflow.main()
            execute.assert_not_called()


if __name__ == "__main__":
    unittest.main()
