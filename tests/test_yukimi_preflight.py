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
RUNTIME = (Path(os.environ['YUKIMI_TEST_RUNTIME']).read_text().split()
           if 'YUKIMI_TEST_RUNTIME' in os.environ else subprocess.check_output(
               ['nix-store', '-qR', str(BIN.parent), str(NIX.parents[1]), str(SHELL.parents[1])], text=True).split())

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
        return {str(p.relative_to(self.config)): p.read_bytes() for p in self.config.rglob('*') if p.is_file()}

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
