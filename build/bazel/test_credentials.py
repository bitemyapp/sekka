"""Credential origin, rotation and diagnostic confidentiality contracts."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from credential_helper import CONFIG_ENV, MARKER


class CredentialTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.path = Path(temporary.name) / "private.bazelrc"
        self.path.touch(mode=0o600)

    def configure(self, token, endpoint="grpc://cache.example:17841", extra=""):
        self.path.write_text(MARKER + json.dumps({"endpoint": endpoint, "token": token}) + "\n" + extra)

    def invoke(self, uri):
        return subprocess.run(
            [sys.executable, str(Path(__file__).with_name("credential_helper.py")), "get"],
            input=json.dumps({"uri": uri}), text=True, capture_output=True,
            env={**os.environ, CONFIG_ENV: str(self.path)},
        )

    def test_origin_isolation_and_same_path_rotation(self):
        self.configure("first-fixture-token")
        first = self.invoke("https://cache.example:17841/build.bazel.remote.execution.v2.Capabilities/GetCapabilities")
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(json.loads(first.stdout)["headers"]["Authorization"], ["Bearer first-fixture-token"])
        for uri in ("https://other.example:17841/", "https://cache.example:17842/", "file:///cache.example:17841/", "https://user@cache.example:17841/"):
            with self.subTest(uri=uri):
                result = self.invoke(uri)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["headers"], {})
        self.configure("second-fixture-token")
        rotated = self.invoke("grpcs://cache.example:17841/")
        self.assertEqual(rotated.returncode, 0, rotated.stderr)
        self.assertEqual(json.loads(rotated.stdout)["headers"]["Authorization"], ["Bearer second-fixture-token"])
        self.assertNotIn("first-fixture-token", rotated.stdout + rotated.stderr)

    def test_default_ports_preserve_service_and_transport_boundaries(self):
        self.configure("default-port-fixture-token", "https://cache.example")
        for uri in ("http://cache.example/", "http://cache.example:443/", "https://cache.example:80/"):
            with self.subTest(uri=uri):
                result = self.invoke(uri)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["headers"], {})
        for endpoint, uri in (
            ("https://cache.example", "https://cache.example:443/"),
            ("grpcs://cache.example:443", "https://cache.example/"),
            ("grpc://cache.example", "https://cache.example:80/"),
            ("http://cache.example", "http://cache.example:80/"),
        ):
            with self.subTest(endpoint=endpoint, uri=uri):
                self.configure("default-port-fixture-token", endpoint)
                result = self.invoke(uri)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["headers"]["Authorization"], ["Bearer default-port-fixture-token"])

    def test_invalid_private_configuration_never_discloses_values(self):
        secret = "never-disclose-fixture-token"
        cases = (
            (0o644, "grpc://cache.example:17841", ""),
            (0o600, 123, ""),
            (0o600, "grpc://cache.example:17841", 'build "--remote_header=authorization=Bearer ' + secret + '"\n'),
            (0o600, "grpc://cache.example:17841", "import private-header.bazelrc\n"),
        )
        for mode, endpoint, extra in cases:
            with self.subTest(mode=mode, endpoint=endpoint, extra=bool(extra)):
                self.configure(secret, endpoint, extra)
                self.path.chmod(mode)
                result = self.invoke("https://cache.example:17841/")
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertNotIn(secret, result.stderr)
        self.path.chmod(0o600)
        self.path.write_text(MARKER + '{"token":"' + secret + '", invalid}\n')
        result = self.invoke("https://cache.example:17841/")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(secret, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
