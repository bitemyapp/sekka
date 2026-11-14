#!/usr/bin/env python3
"""Real Nix and Yukimi in a disposable namespace and private local store.

No host configuration, daemon, profiles, credentials or devices are exposed.
Every fixture builder records if it ran; preflight must never execute it.
Usage: python3 tests/test_yukimi_preflight.py /nix/store/...-yukimi-helper/bin
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

BIN = Path(sys.argv.pop(1)).resolve()
NIX = Path(shutil.which('nix')).resolve()
SHELL = Path(shutil.which('sh')).resolve()
GIT = Path(shutil.which('git')).resolve()
RUNTIME = (Path(os.environ['YUKIMI_TEST_RUNTIME']).read_text().split()
           if 'YUKIMI_TEST_RUNTIME' in os.environ else subprocess.check_output(
               ['nix-store', '-qR', str(BIN.parent), str(NIX.parents[1]), str(SHELL.parents[1]), str(GIT.parents[1])], text=True).split())

FLAKE = '''{
  inputs.dependency.url = "path:./dependency";
  outputs = { dependency, ... }: {
    nixosConfigurations.preflight-host.config.system.build.toplevel = derivation {
      name = "system-fixture";
      system = "x86_64-linux";
      builder = "/builder";
      preferLocalBuild = true;
      package = dependency.package;
    };
  };
}
'''
DEPENDENCY = '''{
  outputs = { ... }: {
    package = derivation {
      name = "cargo-geiger-fixture";
      system = "x86_64-linux";
      builder = "/builder";
    };
  };
}
'''


class Preflight(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='yukimi-preflight-test-')
        self.root = Path(self.tmp.name)
        for name in ['etc/nixos/dependency', 'etc/nix', 'run/current-system/sw/bin', 'run/lock',
                     'nix/store', 'nix/var/nix', 'tmp', 'root', 'bin', 'state',
                     'var/lib/yukimi/operations/test']:
            (self.root / name).mkdir(parents=True, exist_ok=True)
        (self.root / 'var/lib/yukimi/operations/test').chmod(0o700)
        self.config = self.root / 'etc/nixos'
        (self.config / 'flake.nix').write_text(FLAKE)
        (self.config / 'dependency/flake.nix').write_text(DEPENDENCY)
        (self.root / 'etc/passwd').write_text('root:x:0:0:root:/root:/bin/sh\n')
        (self.root / 'etc/group').write_text('root:x:0:\n')
        (self.root / 'etc/nix/nix.conf').write_text(
            'experimental-features = nix-command flakes\nbuild-users-group =\n'
            'sandbox = false\nsubstituters =\nflake-registry =\n')
        (self.root / 'run/current-system/sw/bin/nix').symlink_to(NIX)
        (self.root / 'run/current-system/sw/bin/git').symlink_to(GIT)
        (self.root / 'bin/yukimi-helper').symlink_to(BIN / 'yukimi-helper')
        (self.root / 'builder').write_text(f'#!{SHELL}\necho ran > /state/compiled\nexit 91\n')
        (self.root / 'builder').chmod(0o755)
        self.base = ['bwrap', '--unshare-all', '--die-with-parent', '--uid', '0', '--gid', '0',
                     '--hostname', 'preflight-host', '--bind', str(self.root), '/',
                     '--proc', '/proc', '--dev', '/dev', '--clearenv',
                     '--setenv', 'PATH', '/run/current-system/sw/bin:/bin',
                     '--setenv', 'HOME', '/root', '--setenv', 'NIX_REMOTE', 'local', '--chdir', '/']
        for path in RUNTIME:
            self.base += ['--ro-bind', path, path]
        self.run_cmd(str(NIX), 'flake', 'lock', '/etc/nixos')
        self.before = self.configuration()

    def tearDown(self):
        self.tmp.cleanup()

    def configuration(self):
        return {str(p.relative_to(self.config)): p.read_bytes() for p in self.config.rglob('*') if p.is_file() and '.git' not in p.relative_to(self.config).parts}

    def run_cmd(self, *args, ok=True, builds=False):
        result = subprocess.run(self.base + list(args), stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        self.assertEqual((self.root / 'state/compiled').exists(), builds, 'unexpected builder execution')
        return result

    def check(self, ok=True):
        result = self.run_cmd('/bin/yukimi-helper', 'preflight', '--update', 'dependency', ok=ok)
        self.assertEqual(self.configuration(), self.before)
        return json.loads(result.stdout) if ok else result

    def worker(self, report, ok=False, builds=False):
        result = self.run_cmd('/bin/yukimi-helper', '__worker', '/var/lib/yukimi/operations/test',
                             'change', '--update', 'dependency', '--lock', report['lock'],
                             '--checked-source', report['source'], ok=ok, builds=builds)
        self.assertEqual(self.configuration(), self.before)
        return result

    def git(self, *args, directory='/etc/nixos'):
        return self.run_cmd(str(GIT), '-C', directory, '-c', 'user.name=Fixture',
                            '-c', 'user.email=fixture@localhost', *args)

    def commit_config(self, directory='/etc/nixos'):
        self.git('init', '-q', directory=directory)
        self.git('add', '.', directory=directory)
        self.git('commit', '-qm', 'fixture', directory=directory)
        self.before = self.configuration()

    def original_derivation(self):
        result = self.run_cmd(str(NIX), 'build', '--dry-run', '--json', '--no-link',
                              '--no-update-lock-file',
                              '/etc/nixos#nixosConfigurations.preflight-host.config.system.build.toplevel')
        return Path(json.loads(result.stdout)[0]['drvPath']).name

    def assert_same_derivation(self):
        original = self.original_derivation()
        before = set((self.root / 'nix/store').glob('*-system-fixture.drv'))
        self.assertEqual(self.check()['builds'], ['cargo-geiger-fixture'])
        after = set((self.root / 'nix/store').glob('*-system-fixture.drv'))
        self.assertIn(original, {p.name for p in after})
        self.assertEqual(after, before, 'preflight evaluated a different system')

    def test_git_revision_metadata_matches_the_real_build(self):
        flake = FLAKE.replace('outputs = { dependency, ... }', 'outputs = { self, dependency, ... }')
        flake = flake.replace('preferLocalBuild = true;', '''preferLocalBuild = true;
          revision = self.rev or self.dirtyRev;
          shortRevision = self.shortRev or self.dirtyShortRev;
          modified = toString self.lastModified;
          commits = toString (self.revCount or 0);
          clean = self ? rev;
          untracked = assert !(builtins.pathExists ./untracked.nix); "absent";''')
        (self.config / 'flake.nix').write_text(flake)
        self.commit_config()
        (self.config / 'untracked.nix').write_text('throw "must not be copied"')
        self.before = self.configuration()
        index = (self.config / '.git/index').read_bytes()
        self.assert_same_derivation()
        self.assertEqual((self.config / '.git/index').read_bytes(), index)
        (self.config / 'flake.nix').write_text(flake + '# dirty tracked file\n')
        self.before = self.configuration()
        self.assert_same_derivation()
        self.assertEqual((self.config / '.git/index').read_bytes(), index)

    def test_git_subdirectory_preserves_sibling_imports_and_detects_changes(self):
        sibling = self.root / 'etc/shared.nix'
        sibling.write_text('"shared configuration"')
        (self.config / 'flake.nix').write_text(FLAKE.replace(
            'preferLocalBuild = true;', 'preferLocalBuild = true; shared = import ../shared.nix;'))
        self.commit_config('/etc')
        self.assert_same_derivation()
        report = self.check()
        sibling.write_text('"changed after check"')
        self.assertIn('changed after the compatibility check', self.worker(report).stderr)

    def test_new_git_commit_with_identical_files_invalidates_the_check(self):
        self.commit_config()
        report = self.check()
        self.git('commit', '--allow-empty', '-qm', 'new revision, same files')
        self.assertIn('changed after the compatibility check', self.worker(report).stderr)

    def test_inherited_git_index_cannot_redirect_private_writes(self):
        self.commit_config()
        index = (self.config / '.git/index').read_bytes()
        self.base += ['--setenv', 'GIT_INDEX_FILE', '/etc/nixos/.git/index']
        self.assert_same_derivation()
        self.assertEqual((self.config / '.git/index').read_bytes(), index)

    def test_updated_lock_uses_the_same_dirty_revision_as_apply(self):
        dependency = self.root / 'state/dependency'
        dependency.mkdir()
        (dependency / 'flake.nix').write_text(DEPENDENCY)
        self.commit_config('/state/dependency')
        flake = FLAKE.replace('path:./dependency', 'git+file:///state/dependency')
        flake = flake.replace('outputs = { dependency, ... }', 'outputs = { self, dependency, ... }')
        flake = flake.replace('preferLocalBuild = true;',
                              'preferLocalBuild = true; revision = self.rev or self.dirtyRev;')
        (self.config / 'flake.nix').write_text(flake)
        self.run_cmd(str(NIX), 'flake', 'lock', '/etc/nixos')
        self.commit_config()
        (dependency / 'flake.nix').write_text(DEPENDENCY.replace('cargo-geiger-fixture', 'updated-fixture'))
        self.git('commit', '-am', 'update dependency', directory='/state/dependency')
        report = self.check()
        self.assertEqual(report['builds'], ['updated-fixture'])
        checked = set((self.root / 'nix/store').glob('*-system-fixture.drv'))
        proposed = self.root / report['lock'].lstrip('/')
        self.assertNotEqual(proposed.read_bytes(), (self.config / 'flake.lock').read_bytes())
        (self.config / 'flake.lock').write_bytes(proposed.read_bytes())
        actual = self.original_derivation()
        self.assertIn(actual, {p.name for p in checked}, 'apply evaluates a different revision than preflight')

    def test_shallow_git_history_is_preserved(self):
        flake = FLAKE.replace('outputs = { dependency, ... }', 'outputs = { self, dependency, ... }')
        flake = flake.replace('preferLocalBuild = true;',
                              'preferLocalBuild = true; revision = self.rev; commits = toString (self.revCount or 0);')
        (self.config / 'flake.nix').write_text(flake)
        self.commit_config()
        self.git('commit', '--allow-empty', '-qm', 'second commit')
        self.config.rename(self.root / 'state/main')
        self.git('clone', '--depth=1', 'file:///state/main', '/etc/nixos', directory='/state')
        self.before = self.configuration()
        self.assert_same_derivation()

    def test_linked_worktree_preserves_revision_context(self):
        self.commit_config()
        self.config.rename(self.root / 'state/main')
        self.git('worktree', 'add', '--detach', '/etc/nixos', directory='/state/main')
        self.before = self.configuration()
        self.assert_same_derivation()

    def test_uncached_package_is_named_without_a_build_or_configuration_write(self):
        report = self.check()
        self.assertEqual(report['builds'], ['cargo-geiger-fixture'])
        self.assertTrue(report['lock'].startswith('/nix/store/'))
        self.assertFalse(list((self.root / 'tmp').glob('yukimi-preflight-*')), 'scratch configuration leaked')

    def test_generated_configuration_alone_does_not_warn(self):
        (self.config / 'flake.nix').write_text(FLAKE.replace('package = dependency.package;', ''))
        self.before = self.configuration()
        self.assertEqual(self.check()['builds'], [])

    def test_single_configuration_may_differ_from_the_hostname(self):
        (self.config / 'flake.nix').write_text(FLAKE.replace('nixosConfigurations.preflight-host', 'nixosConfigurations.custom'))
        self.before = self.configuration()
        self.assertEqual(self.check()['builds'], ['cargo-geiger-fixture'])

    def test_accepted_lock_reaches_build_only_after_explicit_apply(self):
        report = self.check()
        result = self.worker(report, builds=True)
        # The sentinel compiler fails deliberately; normal rollback restores
        # the fixture configuration. Preflight never ran that compiler.
        self.assertIn('Evaluating your configuration', result.stderr)
        self.assertIn('91', result.stderr)
        self.assertNotIn('checked update was refused', result.stderr)

    def test_evaluation_failure_names_the_package(self):
        (self.config / 'dependency/flake.nix').write_text(
            '{ outputs = { ... }: { package = throw "cargo-geiger is incompatible with this input"; }; }')
        self.run_cmd(str(NIX), 'flake', 'lock', '/etc/nixos')
        self.before = self.configuration()
        result = self.check(ok=False)
        self.assertIn('cargo-geiger is incompatible', result.stderr)
        self.assertIn('No system configuration was changed', result.stderr)

    def test_import_from_derivation_is_refused_without_running_it(self):
        (self.config / 'flake.nix').write_text(FLAKE.replace(
            'package = dependency.package;', 'package = builtins.readFile dependency.package;'))
        self.before = self.configuration()
        result = self.check(ok=False)
        self.assertIn('allow-import-from-derivation', result.stderr)

    def test_stale_source_is_refused_before_writing_configuration(self):
        report = self.check()
        (self.config / 'flake.nix').write_text(FLAKE + '# changed after the check\n')
        self.before = self.configuration()
        result = self.worker(report)
        self.assertIn('changed after the compatibility check', result.stderr)

    def test_proposed_lock_cannot_add_an_unrequested_input(self):
        report = self.check()
        lock = json.loads((self.root / report['lock'].lstrip('/')).read_text())
        lock['nodes'][lock['root']]['inputs']['injected'] = 'dependency'
        (self.root / 'bad.lock').write_text(json.dumps(lock))
        report['lock'] = '/bad.lock'
        result = self.worker(report)
        self.assertIn('checked update was refused', result.stderr)
        self.assertIn('different inputs', result.stderr)

    def test_copied_lock_symlink_cannot_modify_its_target(self):
        lock = self.config / 'flake.lock'
        original = lock.read_bytes()
        (self.root / 'outside.lock').write_bytes(original)
        lock.unlink()
        lock.symlink_to('/outside.lock')
        self.before = self.configuration()
        self.check(ok=False)
        self.assertEqual((self.root / 'outside.lock').read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
