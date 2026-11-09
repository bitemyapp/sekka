"""Run both real supervisor loops against controlled GitHub/Docker endpoints.

The stand-ins record registration and mounts, and hold jobs open, so the tests
exercise overlapping jobs, restart recovery and exclusive slot ownership.
"""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"


class Supervisor(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                        FIXTURE=str(self.root), SEKKA_RUNNER_UID="30500",
                        SEKKA_CI_GIT_CACHE_BASE=str(self.root / "cache"),
                        SEKKA_RUNNER_TOKEN_FILE=str(self.root / "token"),
                        SEKKA_RUNNER_GIT_CACHE_SCRIPT=str(self.bin / "git-cache"))
        (self.root / "token").write_text("test-token")
        self.tool("curl", '''
args = sys.argv[1:]
request = json.loads(args[args.index('--data') + 1])
slot = os.environ['SEKKA_RUNNER_SLOT']
(root / (slot + '-registration')).write_text(json.dumps(request))
print(json.dumps({'encoded_jit_config': 'test-registration'}))
''')
        self.tool("docker", '''
args = sys.argv[1:]
slot = os.environ['SEKKA_RUNNER_SLOT']
if args[0] == 'wait':
    (root / (slot + '-wait')).write_text(args[1])
    while (root / (slot + '-surviving-job')).exists():
        time.sleep(.02)
elif args[0] == 'run':
    (root / (slot + '-run')).write_text(json.dumps(args))
    while not (root / (slot + '-finish')).exists():
        time.sleep(.02)
else:
    sys.exit('unexpected docker command')
''')
        (self.bin / "git-cache").write_text('''
set -eu
printf '%s' "$SEKKA_CI_GIT_CACHE" > "$FIXTURE/$SEKKA_RUNNER_SLOT-cache"
test ! -e "$FIXTURE/$SEKKA_RUNNER_SLOT-cache-fails"
''')

    def tool(self, name, body):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\nimport json, os, sys, time\n"
                        "from pathlib import Path\nroot = Path(os.environ['FIXTURE'])\n" + body)
        path.chmod(0o755)

    def start(self, slot):
        log = (self.root / f"{slot}-{time.monotonic_ns()}.log").open("w+")
        self.addCleanup(log.close)
        process = subprocess.Popen([shutil.which("bash"), str(SCRIPTS / "runner-supervisor.sh")],
                                   env=dict(self.env, SEKKA_RUNNER_SLOT=slot),
                                   stdout=log, stderr=log, start_new_session=True)
        def stop():
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=5)
        self.addCleanup(stop)
        return process, log

    def wait_for(self, filename):
        path = self.root / filename
        deadline = time.monotonic() + 10
        while not path.exists() and time.monotonic() < deadline:
            time.sleep(.02)
        self.assertTrue(path.exists(), filename)
        return path

    def test_two_slots_share_build_artifacts_and_accept_identical_workloads(self):
        first, _ = self.start("1")
        second, _ = self.start("2")
        for slot, name in [("1", "sekka-runner-job"), ("2", "sekka-runner-2-job")]:
            args = json.loads(self.wait_for(slot + "-run").read_text())
            registration = json.loads((self.root / (slot + "-registration")).read_text())
            self.assertEqual(args[args.index("--name") + 1], name)
            self.assertEqual((self.root / (slot + "-wait")).read_text(), name)
            self.assertEqual(registration["labels"], ["self-hosted", "linux", "x64", "sekka-builder"])
            cache = str(self.root / "cache" / slot)
            self.assertEqual((self.root / (slot + "-cache")).read_text(), cache)
            self.assertIn(f"type=bind,src={cache},dst=/var/cache/sekka-ci-git,readonly", args)
            self.assertIn(f"SEKKA_RUNNER_SLOT={slot}", args)
            self.assertIn("/nix/store:/nix/store:ro", args)
            self.assertIn("/nix/var/nix/daemon-socket:/nix/var/nix/daemon-socket:ro", args)
            self.assertIn("type=bind,src=/nix/var/nix/gcroots/sekka-ci,dst=/nix/var/nix/gcroots/sekka-ci", args)
            self.assertIn("type=volume,dst=/home/runner", args)
            self.assertIn("no-new-privileges", args)
            self.assertIn("--read-only", args)
            self.assertEqual(args[args.index("--user") + 1], "30500:30500")
        self.assertIsNone(first.poll())
        self.assertIsNone(second.poll())

    def test_duplicate_slot_cannot_refresh_or_register_during_an_active_job(self):
        self.start("2")
        self.wait_for("2-run")
        cache = self.root / "2-cache"
        before = cache.stat().st_mtime_ns
        duplicate, log = self.start("2")
        self.assertEqual(duplicate.wait(timeout=5), 1)
        log.seek(0)
        self.assertIn("already has a supervisor", log.read())
        self.assertEqual(cache.stat().st_mtime_ns, before)

    def test_restart_waits_for_its_surviving_job_without_blocking_other_slot(self):
        surviving = self.root / "1-surviving-job"
        surviving.touch()
        self.start("1")
        self.wait_for("1-wait")
        self.start("2")
        self.wait_for("2-run")
        self.assertFalse((self.root / "1-cache").exists())
        self.assertFalse((self.root / "1-registration").exists())
        surviving.unlink()
        self.wait_for("1-run")

    def test_cache_refresh_failure_still_allows_checkout_fallback(self):
        (self.root / "2-cache-fails").touch()
        self.start("2")
        self.wait_for("2-run")

    def test_unknown_slot_fails_before_registration(self):
        process, _ = self.start("typo")
        self.assertEqual(process.wait(timeout=5), 2)
        self.assertFalse((self.root / "cache").exists())


if __name__ == "__main__":
    unittest.main()
