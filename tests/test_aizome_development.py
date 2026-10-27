"""Exercise dependency setup in disposable Sekka worktrees."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HOOK = Path(os.environ.get('AIZOME_DEV_HOOK', Path(__file__).resolve().parents[1] / 'desktop/aizome/scripts/dev-environment'))


class DevelopmentEnvironment(unittest.TestCase):
    def test_pinned_source_is_available_from_root_and_nested_directories(self):
        for location in ('', 'desktop/aizome', 'desktop/aizome/apps/yukimi'):
            with self.subTest(location=location), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                component = root / 'desktop/aizome'
                (component / 'apps/yukimi').mkdir(parents=True)
                (component / 'Cargo.toml').touch()
                pinned = root / 'pinned-sidestep'
                pinned.mkdir()
                subprocess.run(['bash', str(HOOK)], cwd=root / location,
                               env={**os.environ, 'AIZOME_SIDESTEP_SOURCE': str(pinned)}, check=True)
                self.assertEqual((root / 'desktop/sidestep-aizome').resolve(), pinned)

    def test_existing_checkout_and_links_are_preserved(self):
        for kind in ('checkout', 'link', 'broken-link'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'desktop/aizome').mkdir(parents=True)
                (root / 'desktop/aizome/Cargo.toml').touch()
                dependency = root / 'desktop/sidestep-aizome'
                if kind == 'checkout':
                    dependency.mkdir()
                    (dependency / 'local-work').write_text('preserve')
                else:
                    target = root / 'developer-sidestep'
                    if kind == 'link':
                        target.mkdir()
                    dependency.symlink_to(target)
                subprocess.run(['bash', str(HOOK)], cwd=root,
                               env={**os.environ, 'AIZOME_SIDESTEP_SOURCE': '/unused-pin'}, check=True)
                if kind == 'checkout':
                    self.assertEqual((dependency / 'local-work').read_text(), 'preserve')
                else:
                    self.assertEqual(dependency.readlink(), target)


if __name__ == '__main__':
    unittest.main()
