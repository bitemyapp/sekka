"""Small synthetic captures exercise restoration without loading benchmark data."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/restore-hotaru-evidence.py'
spec = importlib.util.spec_from_file_location('restore_hotaru_evidence', SCRIPT)
restore = importlib.util.module_from_spec(spec)
spec.loader.exec_module(restore)


class RestoreEvidence(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        self.paths = ['apps/hotaru/upstream/example/first.json.gz',
                      'apps/hotaru/upstream/example/second.log.gz']
        self.data = [b'first synthetic capture\n', b'second synthetic capture\n']
        self.git('init', '-q')
        for name, data in zip(self.paths, self.data):
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.git('add', '.')
        self.git('-c', 'user.name=Evidence test', '-c', 'user.email=test@example.invalid',
                 '-c', 'commit.gpgsign=false', 'commit', '-qm', 'Synthetic evidence')
        self.manifest = {
            'version': 1, 'revision': self.git('rev-parse', 'HEAD').strip(),
            'files': [{'path': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
                      for name, data in zip(self.paths, self.data)],
        }
        self.manifest_path = self.root / 'evidence.json'
        self.destination = self.root / 'restored'
        root_patch = patch.object(restore, 'ROOT', self.repo)
        manifest_patch = patch.object(restore, 'MANIFEST', self.manifest_path)
        root_patch.start()
        manifest_patch.start()
        self.addCleanup(root_patch.stop)
        self.addCleanup(manifest_patch.stop)

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.repo), *args], text=True)

    def run_restore(self, patterns=()):
        self.manifest_path.write_text(json.dumps(self.manifest))
        with contextlib.redirect_stdout(io.StringIO()):
            return restore.restore(self.destination, patterns)

    def test_restores_from_history_after_working_files_are_removed(self):
        for name in self.paths:
            (self.repo / name).unlink()
        self.assertEqual(self.run_restore(), 2)
        for name, data in zip(self.paths, self.data):
            self.assertEqual((self.destination / name).read_bytes(), data)
        original_times = [(self.destination / name).stat().st_mtime_ns for name in self.paths]
        self.assertEqual(self.run_restore(), 2)
        self.assertEqual([(self.destination / name).stat().st_mtime_ns for name in self.paths], original_times)

    def test_subset_does_not_restore_other_captures(self):
        self.assertEqual(self.run_restore(['*.json.gz']), 1)
        self.assertTrue((self.destination / self.paths[0]).is_file())
        self.assertFalse((self.destination / self.paths[1]).exists())

    def test_wrong_hash_or_size_is_rejected_before_installation(self):
        original = self.manifest['files'][0].copy()
        for field, wrong in [('sha256', '0' * 64), ('bytes', original['bytes'] + 1)]:
            with self.subTest(field=field):
                self.manifest['files'][0] = {**original, field: wrong}
                with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                    self.run_restore()
                self.assertFalse(self.destination.exists())

    def test_existing_mismatched_file_is_not_overwritten(self):
        target = self.destination / self.paths[0]
        target.parent.mkdir(parents=True)
        target.write_bytes(b'keep my local file')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            self.run_restore()
        self.assertEqual(target.read_bytes(), b'keep my local file')

    def test_symlinks_are_not_followed(self):
        outside = self.root / 'outside'
        outside.mkdir()
        self.destination.mkdir()
        (self.destination / 'apps').symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'Refusing symlink'):
            self.run_restore()
        self.assertEqual(list(outside.iterdir()), [])

    def test_invalid_paths_revision_and_empty_selection_do_not_write(self):
        original = self.manifest['files'][0]['path']
        for path in ['/tmp/outside.gz', 'apps/hotaru/upstream/../../../outside.gz', 'other/file.gz']:
            with self.subTest(path=path):
                self.manifest['files'][0]['path'] = path
                with self.assertRaisesRegex(ValueError, 'Invalid evidence path'):
                    self.run_restore()
        self.manifest['files'][0]['path'] = original
        with self.assertRaisesRegex(ValueError, 'No captures matched'):
            self.run_restore(['missing.gz'])
        self.manifest['revision'] = 'main'
        with self.assertRaisesRegex(ValueError, 'Invalid evidence revision'):
            self.run_restore()
        self.assertFalse(self.destination.exists())


if __name__ == '__main__':
    unittest.main()
