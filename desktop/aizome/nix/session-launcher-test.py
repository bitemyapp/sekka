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
  # Retained so the pre-fix launcher can serve as a negative control.
  '--user -q is-active aizome.service') test -f "$TEST_DIR/active" ;;
  '--user show -p ActiveState --value aizome.service')
    if test -f "$TEST_DIR/active"; then echo active
    elif test -f "$TEST_DIR/activating"; then echo activating
    else echo inactive; fi ;;
  '--user --wait start aizome.service')
    echo $$ > "$TEST_DIR/client-pid"
    touch "$TEST_DIR/start-entered"
    while test -f "$TEST_DIR/hold-start"; do sleep 0.01; done
    echo start >> "$TEST_DIR/calls"
    touch "$TEST_DIR/activating"
    while test -f "$TEST_DIR/hold-ready"; do sleep 0.01; done
    rm -f "$TEST_DIR/activating"
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
    touch "$TEST_DIR/cleanup-entered"
    while test -f "$TEST_DIR/hold-cleanup"; do sleep 0.01; done
    rm -f "$TEST_DIR/active" "$TEST_DIR/activating" ;;
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
                    "XDG_RUNTIME_DIR": str(self.root), "PATH": path, "TEST_DIR": str(self.root)}
        self.processes = []

    def tearDown(self):
        # Clean up a failing negative control without touching host sessions.
        for process in self.processes:
            # Include surviving children even when the launcher already exited.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate(timeout=3)
        self.temp.cleanup()

    def start(self, launcher=None):
        process = subprocess.Popen(["sh", str(launcher or LAUNCHER), "--no-login-shell"],
                                   env=self.env, start_new_session=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.processes.append(process)
        return process

    def ready(self):
        self.await_file("ready")

    def await_file(self, name):
        deadline = time.monotonic() + 3
        while not (self.root / name).exists():
            if time.monotonic() > deadline:
                self.fail(f"launcher did not reach {name}")
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

    def test_unowned_activating_service_is_also_refused(self):
        (self.root / "activating").touch()
        process = self.start()
        _, error = process.communicate(timeout=3)
        self.assertEqual(process.returncode, 1)
        self.assertIn(b"already running", error)
        self.assertTrue((self.root / "activating").exists())
        self.assertEqual(self.calls(), [])

    def check_pending_start(self, sig, scope=False):
        (self.root / "hold-start").touch()
        process = self.start()
        self.await_file("start-entered")
        child = int((self.root / "client-pid").read_text())
        (os.killpg if scope else os.kill)(process.pid, sig)
        self.assertEqual(process.wait(timeout=3), 128 + sig)
        self.assertFalse(Path(f"/proc/{child}").exists(), "start client must be reaped before exit")
        (self.root / "hold-start").unlink()
        self.assertEqual(self.calls(), ["shutdown", "unset"])
        self.assertFalse((self.root / "active").exists())
        # The dead client cannot submit its start, or retain the lock, later.
        next_login = self.start()
        self.ready()
        (self.root / "active").unlink()
        self.assertEqual(next_login.wait(timeout=3), 7)

    def test_terminate_before_start_submission(self):
        self.check_pending_start(signal.SIGTERM)

    def test_hangup_before_start_submission(self):
        self.check_pending_start(signal.SIGHUP)

    def test_interrupt_group_before_start_submission(self):
        self.check_pending_start(signal.SIGINT, scope=True)

    def test_signal_between_fork_and_pid_capture(self):
        # Force the instruction-sized window rather than relying on scheduler
        # luck. Before exec, the child can still have the shell's signal
        # handlers; TERM alone need not end it, so cleanup's wait can hang.
        source = LAUNCHER.read_text()
        capture = "      wait_pid=$!\n"
        self.assertIn(capture, source)
        (self.root / "hold-start").touch()
        for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
            with self.subTest(signal=sig):
                launcher = self.root / f"launcher-{sig}"
                launcher.write_text(source.replace(capture, f"      kill -{sig} $$\n" + capture))
                process = self.start(launcher)
                self.assertEqual(process.wait(timeout=3), 128 + sig)
                self.assertFalse((self.root / "active").exists())
        self.assertEqual(self.calls(), ["shutdown", "unset"] * 3)

    def check_duplicate_gate(self, gate, entered):
        (self.root / gate).touch()
        owner = self.start()
        if gate == "hold-cleanup":
            self.ready()
            owner.send_signal(signal.SIGTERM)
        self.await_file(entered)
        calls = self.calls()
        duplicate = self.start()
        _, error = duplicate.communicate(timeout=3)
        self.assertEqual(duplicate.returncode, 1)
        self.assertIn(b"already running", error)
        self.assertIsNone(owner.poll())
        self.assertEqual(self.calls(), calls)
        (self.root / gate).unlink()
        if gate != "hold-cleanup":
            self.ready()
            (self.root / "active").unlink()
            self.assertEqual(owner.wait(timeout=3), 7)
        else:
            self.assertEqual(owner.wait(timeout=3), 143)

    def test_duplicate_before_start_submission(self):
        self.check_duplicate_gate("hold-start", "start-entered")

    def test_duplicate_before_compositor_readiness(self):
        self.check_duplicate_gate("hold-ready", "activating")

    def test_duplicate_during_cleanup(self):
        self.check_duplicate_gate("hold-cleanup", "cleanup-entered")

    def test_watchdog_restart_limit_still_ends_with_cleanup(self):
        self.env["TEST_RESULT"] = "watchdog"
        process = self.start()
        self.assertEqual(process.wait(timeout=3), 1)
        self.assertEqual(self.calls(), ["start"] * 4 + ["shutdown", "unset"])


if __name__ == "__main__":
    unittest.main()
