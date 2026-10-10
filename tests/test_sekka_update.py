#!/usr/bin/env python3
"""Exercise the actual updater/helper inside a disposable rootless namespace.

Run after building Yukimi: python3 tests/test_sekka_update.py /nix/store/...-yukimi/bin
Only Nix builds/profile changes and systemd activation are stubbed. No host
configuration, profile, service, device, or credential is writable/visible.
"""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

BIN = Path(sys.argv.pop(1)).resolve()
REPO = Path(__file__).resolve().parents[1]
PYTHON = Path(sys.executable).resolve()

FAKE = r'''
import json, os, pathlib, sys, time
p = pathlib.Path
name = p(sys.argv[0]).name
a = sys.argv[1:]
with open('/state/commands', 'a') as out: out.write(json.dumps([name] + a) + '\n')
if name == 'nix':
    if 'update' in a:
        assert a[-1] == 'sekka', a
        assert 'inputs.sekka.url' in p('/etc/nixos/flake.nix').read_text()
        p('/etc/nixos/flake.lock').write_text('updated lock\n')
    elif 'eval' in a: print('["legacy-host"]')
    elif 'build' in a:
        p('/state/building').touch()
        if p('/state/fail').exists(): sys.exit(1)
        if p('/state/wait').exists(): time.sleep(60)
        print('/system-new')
    else: sys.exit(2)
elif name == 'nix-env':
    profile = p('/nix/var/nix/profiles/system')
    gen = a[-1] if '--switch-generation' in a else '2'
    profile.unlink()
    profile.symlink_to('system-' + gen + '-link')
elif name == 'systemd-run':
    assert a[-1] in ['boot', 'switch'], a
    if p('/state/fail-activation').exists(): sys.exit(1)
    p('/state/activation').write_text(json.dumps(a[-2:]))
else: sys.exit(2)
'''


class Update(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='sekka-update-test-')
        self.root = Path(self.tmp.name)
        for name in ['etc/nixos', 'run/lock', 'run/current-system/sw/bin',
                     'nix/store', 'nix/var/nix/profiles', 'var/lib', 'state', 'tmp', 'root', 'bin']:
            (self.root / name).mkdir(parents=True, exist_ok=True)
        self.config = self.root / 'etc/nixos'
        self.old = (REPO / 'apps/yukimi/crates/yukimi-config/tests/legacy-flake.nix').read_text()
        (self.config / 'flake.nix').write_text(self.old)
        (self.config / 'flake.lock').write_text('original lock\n')
        (self.config / 'configuration.nix').write_text('{ imports = [ ./hardware-configuration.nix ]; }')
        (self.config / 'hardware-configuration.nix').write_text('# hardware untouched')
        for name in ['nix', 'nix-env', 'systemd-run']:
            path = self.root / 'run/current-system/sw/bin' / name
            path.write_text(f'#!{PYTHON}\n' + FAKE)
            path.chmod(0o755)
        profiles = self.root / 'nix/var/nix/profiles'
        for number, target in [(1, '/system-old'), (2, '/system-new')]:
            (profiles / f'system-{number}-link').symlink_to(target)
        (profiles / 'system').symlink_to('system-1-link')
        self.base = ['bwrap', '--unshare-all', '--uid', '0', '--gid', '0', '--hostname', 'legacy-host',
                     '--bind', str(self.root), '/', '--ro-bind', '/nix/store', '/nix/store',
                     '--ro-bind', str(BIN), '/bin', '--proc', '/proc', '--dev', '/dev',
                     '--clearenv', '--setenv', 'PATH', '/run/current-system/sw/bin', '--chdir', '/']

    def tearDown(self):
        self.tmp.cleanup()

    def run_cmd(self, *args, ok=True):
        result = subprocess.run(self.base + list(args), capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        return result

    def unchanged(self):
        self.assertEqual((self.config / 'flake.nix').read_text(), self.old)
        self.assertEqual((self.config / 'flake.lock').read_text(), 'original lock\n')
        self.assertFalse((self.root / 'state/activation').exists())

    def test_preview_and_invalid_args_do_not_mutate(self):
        result = self.run_cmd('/bin/sekka-update', '--print-flake')
        self.assertIn('sekka.nixosModules.default', result.stdout)
        self.run_cmd('/bin/sekka-update', '--invalid', ok=False)
        self.unchanged()

    def test_failed_build_restores_migration_and_lock(self):
        (self.root / 'state/fail').touch()
        self.run_cmd('/bin/sekka-update', ok=False)
        self.unchanged()

    def test_activation_failure_reports_configuration_kept(self):
        (self.root / 'state/fail-activation').touch()
        result = self.run_cmd('/bin/sekka-update', ok=False)
        self.assertIn('configuration was not restored', result.stderr)
        self.assertIn('inputs.sekka.url', (self.config / 'flake.nix').read_text())

    def test_terminal_interrupt_restores_configuration(self):
        (self.root / 'state/wait').touch()
        process = subprocess.Popen(self.base + ['/bin/sekka-update'], start_new_session=True,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                if (self.root / 'state/building').exists(): break
                time.sleep(.05)
            self.assertTrue((self.root / 'state/building').exists())
            os.killpg(process.pid, signal.SIGINT)
            process.communicate(timeout=10)
            self.assertNotEqual(process.returncode, 0)
            self.unchanged()
        finally:
            if process.poll() is None: process.kill()
            process.wait()
            process.stdout.close()
            process.stderr.close()

    def test_success_repeat_and_rollback(self):
        result = self.run_cmd('/bin/sekka-update')
        self.assertIn('Restart when convenient', result.stdout)
        migrated = (self.config / 'flake.nix').read_text()
        self.assertIn('inputs.sekka.url', migrated)
        self.assertEqual(json.loads((self.root / 'state/activation').read_text()),
                         ['/system-new/bin/switch-to-configuration', 'boot'])
        self.assertEqual((self.config / 'hardware-configuration.nix').read_text(), '# hardware untouched')
        self.run_cmd('/bin/sekka-update', '--switch')
        self.assertEqual((self.config / 'flake.nix').read_text(), migrated)
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'switch')
        self.run_cmd('/bin/yukimi-helper', 'rollback', '1')
        self.assertEqual((self.config / 'flake.nix').read_text(), self.old)
        self.assertEqual((self.config / 'flake.lock').read_text(), 'original lock\n')

    def test_cancel_and_concurrent_operation(self):
        (self.root / 'state/wait').touch()
        process = subprocess.Popen(self.base + ['/bin/yukimi-helper', 'change', '--sekka', '--mode', 'boot'],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                if (self.root / 'state/building').exists(): break
                time.sleep(.05)
            self.assertTrue((self.root / 'state/building').exists())
            result = self.run_cmd('/bin/yukimi-helper', 'change', '--sekka', ok=False)
            self.assertIn('Another Yukimi operation', result.stderr)
            process.stdin.close()
            process.wait(timeout=10)
            self.assertNotEqual(process.returncode, 0)
            self.unchanged()
        finally:
            if process.poll() is None: process.kill()
            process.wait()
            process.stdout.close()
            process.stderr.close()


if __name__ == '__main__':
    if not shutil.which('bwrap'):
        raise SystemExit('bubblewrap is required; do not run the helper against the host as a substitute')
    unittest.main()
