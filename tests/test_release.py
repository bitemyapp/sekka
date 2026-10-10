"""Exercise release asset boundaries and byte-for-byte ISO reconstruction."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('prepare_release', ROOT / 'scripts/prepare-release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseAssets(unittest.TestCase):
    def test_round_trip(self):
        for size, chunk, count in [(7, 8, 1), (8, 8, 1), (9, 8, 2), (24, 8, 3)]:
            with self.subTest(size=size), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                source = root / 'source'
                source.mkdir()
                data = bytes(range(size))
                (source / 'test.iso').write_bytes(data)
                with patch.object(release.subprocess, 'check_output', return_value='a' * 40):
                    release.prepare(source, root / 'output', 'v0.0.1-preview', chunk)
                assets = root / 'output/assets'
                manifest = json.loads((assets / 'manifest.json').read_text())
                self.assertEqual(len(manifest['parts']), count)
                self.assertEqual(b''.join((assets / name).read_bytes() for name in manifest['parts']), data)
                self.assertEqual(manifest['sha256'], hashlib.sha256(data).hexdigest())
                self.assertEqual(manifest['revision'], 'a' * 40)
                for line in (assets / 'SHA256SUMS').read_text().splitlines():
                    checksum, name = line.split('  ')
                    self.assertEqual(release.digest(assets / name), checksum)
                self.assertEqual((source / 'test.iso').read_bytes(), data)
                self.assertEqual((assets / 'flake.lock').read_bytes(), (ROOT / 'flake.lock').read_bytes())
                with self.assertRaises(ValueError):
                    release.prepare(source, root / 'output', 'v0.0.1', chunk)

    def test_rejects_missing_multiple_empty_and_invalid_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / 'source'
            source.mkdir()
            with self.assertRaises(ValueError):
                release.prepare(source, root / 'output', 'v1')
            (source / 'one.iso').touch()
            with self.assertRaises(ValueError):
                release.prepare(source, root / 'output', 'v1')
            (source / 'one.iso').write_bytes(b'iso')
            for tag in ['main', 'v1/../../x', 'v1;echo bad', '-v1']:
                with self.assertRaises(ValueError):
                    release.prepare(source, root / 'output', tag)
            with self.assertRaises(ValueError):
                release.prepare(source, root / 'output', 'v1', 0)
            (source / 'two.iso').write_bytes(b'iso')
            with self.assertRaises(ValueError):
                release.prepare(source, root / 'output', 'v1')
            self.assertFalse((root / 'output').exists())


if __name__ == '__main__':
    unittest.main()
