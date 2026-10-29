"""Exercise the shipped launcher with isolated stand-ins for session services."""

import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import unittest


LAUNCHER = Path(os.environ.get("AIZOME_SESSION_LAUNCHER", Path(__file__).parent / "../session/aizome-session")).resolve()

# No command can reach the host's user manager, activation bus or compositor.
SYSTEMCTL = r'''#!/bin/sh
case "$*" in
  '--user is-system-running') echo running ;;
  '--user -q is-active aizome.service') test -f "$TEST_DIR/active" ;;
  '--user --wait start aizome.service')
    echo start >> "$TEST_DIR/calls"
    touch "$TEST_DIR/active" "$TEST_DIR/ready"
    if test "${TEST_RESULT:-}" = watchdog; then
      rm -f "$TEST_DIR/active"
      exit 1
    fi
    while test -f "$TEST_DIR/active"; do sleep 0.05; done
    exit 7 ;;
  '--user show -p Result --value aizome.service') echo "${TEST_RESULT:-exit-code}" ;;
  '--user start --job-mode=replace-irreversibly aizome-shutdown.target')
    echo shutdown >> "$TEST_DIR/calls"
    rm -f "$TEST_DIR/active" ;;
  '--user unset-environment '*) echo unset >> "$TEST_DIR/calls" ;;
esac
'''


class Launcher(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        # Override any system environment file's PATH through the launcher's
        # supported user environment, as well as the initial environment.
        config = self.root / "config/aizome"
        config.mkdir(parents=True)
        path = f"{self.bin}:{os.environ['PATH']}"
        (config / "environment").write_text(f"PATH={path}\nTEST_DIR={self.root}\n")
        for name, body in (("systemctl", SYSTEMCTL), ("dbus-update-activation-environment", "#!/bin/sh\nexit 0\n")):
            script = self.bin / name
            script.write_text(body)
            script.chmod(0o755)
        self.env = {"HOME": str(self.root), "XDG_CONFIG_HOME": str(config.parent),
                    "PATH": path, "TEST_DIR": str(self.root)}
        self.processes = []

    def tearDown(self):
        # Clean up a failing negative control without touching host sessions.
        (self.root / "active").unlink(missing_ok=True)
        for process in self.processes:
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            process.communicate()
        self.temp.cleanup()

    def start(self):
        process = subprocess.Popen(["sh", str(LAUNCHER), "--no-login-shell"],
                                   env=self.env, start_new_session=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.processes.append(process)
        return process

    def ready(self):
        deadline = time.monotonic() + 3
        while not (self.root / "ready").exists():
            if time.monotonic() > deadline:
                self.fail("launcher did not start the service")
            time.sleep(0.01)

    def calls(self):
        path = self.root / "calls"
        return path.read_text().splitlines() if path.exists() else []

    def test_compositor_exit_preserves_status_and_cleans_up_once(self):
        process = self.start()
        self.ready()
        (self.root / "active").unlink()
        self.assertEqual(process.wait(timeout=3), 7)
        self.assertEqual(self.calls(), ["start", "shutdown", "unset"])

    def check_signal(self, sig, scope=False):
        process = self.start()
        self.ready()
        if scope:
            os.killpg(process.pid, sig)
        else:
            process.send_signal(sig)
        # The stand-in compositor remains alive until cleanup stops it:
        # this catches traps deferred behind a foreground systemctl --wait.
        process.wait(timeout=3)
        self.assertFalse((self.root / "active").exists())
        self.assertEqual(self.calls(), ["start", "shutdown", "unset"])
        self.assertEqual(process.returncode, 128 + sig)
        (self.root / "ready").unlink()
        next_login = self.start()
        self.ready()
        (self.root / "active").unlink()
        self.assertEqual(next_login.wait(timeout=3), 7)
        self.assertEqual(self.calls(), ["start", "shutdown", "unset"] * 2)

    def test_terminate_launcher(self):
        self.check_signal(signal.SIGTERM)

    def test_hangup_launcher(self):
        self.check_signal(signal.SIGHUP)

    def test_interrupt_launcher(self):
        self.check_signal(signal.SIGINT)

    def test_terminate_login_scope(self):
        self.check_signal(signal.SIGTERM, scope=True)

    def test_duplicate_login_does_not_stop_existing_session(self):
        (self.root / "active").touch()
        process = self.start()
        _, error = process.communicate(timeout=3)
        self.assertEqual(process.returncode, 1)
        self.assertIn(b"already running", error)
        self.assertTrue((self.root / "active").exists())
        self.assertEqual(self.calls(), [])

    def test_watchdog_restart_limit_still_ends_with_cleanup(self):
        self.env["TEST_RESULT"] = "watchdog"
        process = self.start()
        self.assertEqual(process.wait(timeout=3), 1)
        self.assertEqual(self.calls(), ["start"] * 4 + ["shutdown", "unset"])


if __name__ == "__main__":
    unittest.main()
