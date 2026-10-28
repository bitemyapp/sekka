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
    (sidestep / 'clipboard-reader.txt').write_text('reader: synchronous\n')
    (sidestep / 'clipboard-writer.txt').write_text('writer: unbounded\n')
    (sidestep / 'notes.txt').write_text('developer notes\n')
    git(sidestep, 'add', '.')
    git(sidestep, 'commit', '-q', '-m', 'first')
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


def hook(base, cwd, pinned, *, check=True):
    return subprocess.run(['bash', str(HOOK)], cwd=cwd, check=check, capture_output=True, text=True,
                          env={**os.environ, **GIT_ENV, 'GIT_CEILING_DIRECTORIES': str(base),
                               'AIZOME_SIDESTEP_REV': pinned})


def clipboard_patch(root):
    """A real two-file Git patch for the disposable dependency, not string
    assertions against the production hook's implementation."""
    patch = root / 'apps/hotaru/upstream/sidestep-clipboard.patch'
    patch.parent.mkdir(parents=True)
    patch.write_text('''diff --git a/clipboard-reader.txt b/clipboard-reader.txt
--- a/clipboard-reader.txt
+++ b/clipboard-reader.txt
@@ -1 +1 @@
-reader: synchronous
+reader: asynchronous
diff --git a/clipboard-writer.txt b/clipboard-writer.txt
--- a/clipboard-writer.txt
+++ b/clipboard-writer.txt
@@ -1 +1 @@
-writer: unbounded
+writer: bounded
''')
    return patch


class DevelopmentEnvironment(unittest.TestCase):
    def test_both_native_patches_apply_and_repeat_without_changing_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, first, _ = sekka(base)
            clipboard_patch(root)
            (root / 'apps/hotaru/upstream/sidestep-input.patch').write_text('''diff --git a/notes.txt b/notes.txt
--- a/notes.txt
+++ b/notes.txt
@@ -1 +1 @@
-developer notes
+native keyboard metadata
''')
            initial = hook(base, root, first)
            checkout = root / 'vendor/sidestep'
            self.assertEqual((checkout / 'clipboard-reader.txt').read_text(), 'reader: asynchronous\n')
            self.assertEqual((checkout / 'notes.txt').read_text(), 'native keyboard metadata\n')
            self.assertLess(initial.stderr.index('native clipboard patch'), initial.stderr.index('native input patch'))
            before = git(checkout, 'diff')
            repeated = hook(base, root, first)
            self.assertEqual(git(checkout, 'diff'), before)
            self.assertNotIn('applied Hotaru', repeated.stderr)

    def test_clipboard_patch_is_applied_to_fresh_submodule_and_is_idempotent(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, first, _ = sekka(base)
            clipboard_patch(root)
            initial = hook(base, root / 'desktop/aizome/apps/yukimi', first)
            checkout = root / 'vendor/sidestep'
            self.assertEqual((checkout / 'clipboard-reader.txt').read_text(), 'reader: asynchronous\n')
            self.assertEqual((checkout / 'clipboard-writer.txt').read_text(), 'writer: bounded\n')
            self.assertIn('applied Hotaru', initial.stderr)
            self.assertEqual(initial.stdout, '')
            before = git(checkout, 'diff')
            repeated = hook(base, root, first)
            self.assertEqual(git(checkout, 'diff'), before)
            self.assertEqual(git(checkout, 'rev-parse', 'HEAD'), first)
            self.assertNotIn('applied Hotaru', repeated.stderr)
            self.assertEqual(repeated.stdout, '')

    def test_existing_clipboard_patch_keeps_unrelated_local_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, first, _ = sekka(base)
            patch = clipboard_patch(root)
            git(root, 'submodule', 'update', '--init', '-q')
            checkout = root / 'vendor/sidestep'
            git(checkout, 'apply', str(patch))
            (checkout / 'notes.txt').write_text('unrelated local edit\n')
            (checkout / 'new-notes.txt').write_text('untracked local work\n')
            before = git(checkout, 'diff')
            result = hook(base, root, first)
            self.assertEqual(git(checkout, 'diff'), before)
            self.assertEqual((checkout / 'new-notes.txt').read_text(), 'untracked local work\n')
            self.assertNotIn('applied Hotaru', result.stderr)

    def test_conflicting_clipboard_edit_fails_without_partial_patch_or_reset(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, first, _ = sekka(base)
            clipboard_patch(root)
            git(root, 'submodule', 'update', '--init', '-q')
            checkout = root / 'vendor/sidestep'
            (checkout / 'clipboard-writer.txt').write_text('writer: developer implementation\n')
            before = git(checkout, 'diff')
            result = hook(base, root, first, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('cannot apply', result.stderr)
            self.assertIn('preserving vendor/sidestep', result.stderr)
            self.assertEqual(result.stdout, '')
            self.assertEqual(git(checkout, 'diff'), before)
            self.assertEqual((checkout / 'clipboard-reader.txt').read_text(), 'reader: synchronous\n')
            self.assertEqual(git(checkout, 'rev-parse', 'HEAD'), first)

    def test_partial_clipboard_patch_is_reported_without_overwriting_work(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, first, _ = sekka(base)
            clipboard_patch(root)
            git(root, 'submodule', 'update', '--init', '-q')
            checkout = root / 'vendor/sidestep'
            (checkout / 'clipboard-reader.txt').write_text('reader: asynchronous\n')
            before = git(checkout, 'diff')
            result = hook(base, root, first, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('cannot apply', result.stderr)
            self.assertEqual(git(checkout, 'diff'), before)
            self.assertEqual((checkout / 'clipboard-writer.txt').read_text(), 'writer: unbounded\n')

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
