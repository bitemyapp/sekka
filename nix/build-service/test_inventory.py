import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from inventory import _bind_script


class ScriptBindingTests(unittest.TestCase):
    def test_placeholder_and_quote_bearing_path_remains_data(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory) / "x'];raise Exception('PATH_BINDING_INJECTION');#"
            parent.mkdir()
            payload = parent / 'SOURCE_VIEWS"\nCOMPSIZE'
            contents = b"physical inventory must open the selected file, not execute its name\n"
            payload.write_bytes(contents)
            code = _bind_script(
                "import hashlib, pathlib\n"
                "print(hashlib.sha256(pathlib.Path(PATHS[0]).read_bytes()).hexdigest())\n",
                PATHS=[str(payload)],
                COMPSIZE="/nix/store/pinned/bin/compsize",
                SOURCE_VIEWS={str(parent): "/__physical_sources/0"},
            )
            result = subprocess.run(
                [sys.executable, "-I", "-S", "-c", code],
                capture_output=True, text=True, check=True,
            )
            self.assertEqual(result.stdout, hashlib.sha256(contents).hexdigest() + "\n")


if __name__ == "__main__":
    unittest.main()
