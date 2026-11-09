"""Exercise the harness's export through real, disposable D-Bus activation.

Run with Python 3, dbus-daemon, dbus-send and dbus-update-activation-environment
on PATH. No compositor, GPU or login session is needed.
"""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest

from aizome_session import private_bus_shell


class PrivateBusShell(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="azbus.", dir="/tmp")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.address = f"unix:path={self.root}/bus"
        self.env = {"PATH": os.environ["PATH"], "HOME": str(self.root),
                    "DBUS_SESSION_BUS_ADDRESS": self.address}
        self.record = self.root / "activated.json"
        self.shell_record = self.root / "shell.json"
        services = self.root / "services"
        services.mkdir()
        probe = self.root / "probe.py"
        probe.write_text("import json, os, sys\nfrom pathlib import Path\n"
                         "Path(sys.argv[1]).write_text(json.dumps(dict(os.environ)))\n")
        (services / "org.aizome.TestEnvironment.service").write_text(
            "[D-BUS Service]\nName=org.aizome.TestEnvironment\n"
            f"Exec={sys.executable} {probe} {self.record}\n")
        config = self.root / "bus.conf"
        config.write_text(
            f"<busconfig><type>session</type><listen>{self.address}</listen>"
            f"<servicedir>{services}</servicedir><auth>EXTERNAL</auth>"
            '<policy context="default"><allow send_destination="*"/>'
            '<allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        self.bus = subprocess.Popen(
            ["dbus-daemon", "--nofork", "--nopidfile", "--print-address", f"--config-file={config}"],
            env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(self.stop_bus)
        self.assertTrue(self.bus.stdout.readline().startswith(self.address))
        self.shell = self.root / "shell"
        # Activation deliberately exits after recording its environment. It
        # need not acquire a name: StartServiceByName still launches it, and
        # its exit lets the synchronous call finish with an expected error.
        self.shell.write_text(
            "#!/bin/sh\nset -eu\n"
            "dbus-send --session --print-reply --reply-timeout=3000 "
            "--dest=org.freedesktop.DBus /org/freedesktop/DBus "
            "org.freedesktop.DBus.StartServiceByName "
            "string:org.aizome.TestEnvironment uint32:0 >/dev/null 2>&1 || true\n"
            f"test -f {shlex.quote(str(self.record))}\n"
            f"exec {shlex.quote(sys.executable)} {shlex.quote(str(probe))} "
            f"{shlex.quote(str(self.shell_record))}\n")
        self.shell.chmod(0o700)
        self.wrapper = private_bus_shell(self.root, self.address, self.shell)

    def stop_bus(self):
        self.bus.terminate()
        self.bus.communicate(timeout=5)

    def check_activation(self, display):
        expected = {
            "WAYLAND_DISPLAY": "wayland-test",
            "AIZOME_SOCKET": f"{self.root}/aizome.wayland-test.sock",
            "AIZOME_SHELL_SOCKET": f"{self.root}/aizome-shell.wayland-test.sock",
            "XDG_CURRENT_DESKTOP": "Aizome",
            "XDG_SESSION_DESKTOP": "aizome",
            "XDG_SESSION_TYPE": "wayland",
            "AIZOME_SESSION": "0",
        }
        if display:
            expected["DISPLAY"] = ":42"
        subprocess.run([self.wrapper], env=self.env | expected, check=True, capture_output=True, timeout=5)
        activated = json.loads(self.record.read_text())
        shell = json.loads(self.shell_record.read_text())
        for key, value in expected.items():
            self.assertEqual(activated.get(key), value, key)
            self.assertEqual(shell.get(key), value, key)
        if not display:
            self.assertNotIn("DISPLAY", activated)

    def test_private_activation_before_shell_with_xwayland(self):
        self.check_activation(display=True)

    def test_private_activation_before_shell_without_xwayland(self):
        self.check_activation(display=False)

    def test_different_bus_is_rejected_before_export_or_shell(self):
        result = subprocess.run([self.wrapper], env=self.env | {"DBUS_SESSION_BUS_ADDRESS": "unix:path=/absent"},
                                capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to export", result.stderr)
        self.assertFalse(self.record.exists())
        self.assertFalse(self.shell_record.exists())

    def test_failed_export_does_not_start_shell(self):
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        exporter = bin_dir / "dbus-update-activation-environment"
        exporter.write_text("#!/bin/sh\nexit 42\n")
        exporter.chmod(0o700)
        result = subprocess.run([self.wrapper], env=self.env | {"PATH": f"{bin_dir}:{self.env['PATH']}"},
                                capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 42)
        self.assertFalse(self.shell_record.exists())


if __name__ == "__main__":
    unittest.main()
