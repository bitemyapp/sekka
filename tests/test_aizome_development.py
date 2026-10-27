"""Exercise Sidestep's submodule setup in disposable Sekka checkouts."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
HOOK = Path(os.environ.get('AIZOME_DEV_HOOK', ROOT / 'desktop/aizome/scripts/dev-environment'))
# Commits and local file:// submodules, without the developer's own configuration.
GIT_ENV = {
    'GIT_CONFIG_NOSYSTEM': '1', 'GIT_CONFIG_GLOBAL': os.devnull,
    'GIT_CONFIG_COUNT': '2', 'GIT_CONFIG_KEY_0': 'protocol.file.allow', 'GIT_CONFIG_VALUE_0': 'always',
    'GIT_CONFIG_KEY_1': 'init.defaultBranch', 'GIT_CONFIG_VALUE_1': 'main',
    'GIT_AUTHOR_NAME': 'Sekka', 'GIT_AUTHOR_EMAIL': 'sekka@example.invalid',
    'GIT_COMMITTER_NAME': 'Sekka', 'GIT_COMMITTER_EMAIL': 'sekka@example.invalid',
}


def git(cwd, *args):
    return subprocess.run(['git', *args], cwd=cwd, env={**os.environ, **GIT_ENV}, check=True,
                          capture_output=True, text=True).stdout.strip()


def sekka(base):
    """A Sidestep with two commits, and a fresh Sekka clone whose
    vendor/sidestep submodule records the first and isn't checked out."""
    sidestep = base / 'sidestep'
    sidestep.mkdir()
    git(sidestep, 'init', '-q')
    git(sidestep, 'commit', '-q', '--allow-empty', '-m', 'first')
    first = git(sidestep, 'rev-parse', 'HEAD')
    git(sidestep, 'commit', '-q', '--allow-empty', '-m', 'second')
    second = git(sidestep, 'rev-parse', 'HEAD')
    origin = base / 'origin'
    (origin / 'desktop/aizome/apps/yukimi').mkdir(parents=True)
    (origin / 'desktop/aizome/Cargo.toml').touch()
    (origin / 'desktop/aizome/apps/yukimi/Cargo.toml').touch()
    git(origin, 'init', '-q')
    git(origin, 'submodule', 'add', '-q', sidestep.as_uri(), 'vendor/sidestep')
    git(origin / 'vendor/sidestep', 'checkout', '-q', first)
    git(origin, 'add', '.')
    git(origin, 'commit', '-q', '-m', 'sekka')
    root = base / 'sekka'
    git(base, 'clone', '-q', str(origin), str(root))
    return root, first, second


def hook(base, cwd, pinned):
    return subprocess.run(['bash', str(HOOK)], cwd=cwd, check=True, capture_output=True, text=True,
                          env={**os.environ, **GIT_ENV, 'GIT_CEILING_DIRECTORIES': str(base),
                               'AIZOME_SIDESTEP_REV': pinned})


class DevelopmentEnvironment(unittest.TestCase):
    def test_submodule_is_checked_out_from_root_and_nested_directories(self):
        for location in ('', 'desktop/aizome', 'desktop/aizome/apps/yukimi'):
            with self.subTest(location=location), tempfile.TemporaryDirectory() as directory:
                base = Path(directory)
                root, first, _ = sekka(base)
                result = hook(base, root / location, first)
                self.assertEqual(git(root / 'vendor/sidestep', 'rev-parse', 'HEAD'), first)
                # aizome-dev reads the shell's environment from standard output.
                self.assertEqual(result.stdout, '')
                self.assertNotIn('builds against', result.stderr)

    def test_existing_checkout_is_kept_and_a_different_revision_named(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, first, second = sekka(base)
            git(root, 'submodule', 'update', '--init', '-q')
            checkout = root / 'vendor/sidestep'
            git(checkout, 'checkout', '-q', second)
            (checkout / 'local-work').write_text('preserve')
            result = hook(base, root, first)
            self.assertEqual(git(checkout, 'rev-parse', 'HEAD'), second)
            self.assertEqual((checkout / 'local-work').read_text(), 'preserve')
            self.assertEqual(result.stdout, '')
            self.assertIn(f'vendor/sidestep is at {second}', result.stderr)
            self.assertIn(f'Sidestep {first}', result.stderr)

    def test_source_without_git_is_reported_without_failing(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / 'sekka'
            (root / 'desktop/aizome').mkdir(parents=True)
            (root / 'desktop/aizome/Cargo.toml').touch()
            (root / 'vendor/sidestep').mkdir(parents=True)
            result = hook(base, root, 'a' * 40)
            self.assertEqual(result.stdout, '')
            self.assertIn('git submodule update --init vendor/sidestep', result.stderr)


class SidestepPin(unittest.TestCase):
    @unittest.skipUnless((ROOT / '.git').exists(), 'needs a Sekka Git checkout')
    def test_submodule_records_the_flake_pin(self):
        # Nix builds the flake's input, since GitHub's tarballs of Sekka leave
        # submodules empty; development builds the submodule. They must agree.
        recorded = git(ROOT, 'rev-parse', ':vendor/sidestep')
        named = re.search(r'github:bitemyapp/sidestep/([0-9a-f]{40})', (ROOT / 'flake.nix').read_text())
        lock = json.loads((ROOT / 'flake.lock').read_text())
        locked = lock['nodes'][lock['nodes'][lock['root']]['inputs']['sidestep']]['locked']['rev']
        self.assertIsNotNone(named)
        self.assertEqual(named.group(1), locked)
        self.assertEqual(recorded, locked)


if __name__ == '__main__':
    unittest.main()
