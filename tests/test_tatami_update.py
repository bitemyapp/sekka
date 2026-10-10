#!/usr/bin/env python3
"""Run the real Tatami session/update commands in a disposable namespace."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

BIN = Path(sys.argv.pop(1)).resolve()
PYTHON = Path(sys.executable).resolve()
FAKE = r'''
import json, pathlib, sys
p = pathlib.Path
name, args = p(sys.argv[0]).name, sys.argv[1:]
with open('/state/calls', 'a') as out: out.write(json.dumps([name] + args) + '\n')
if name == 'systemctl':
    if 'is-active' in args: sys.exit(0 if p('/state/awake').exists() else 3)
    if 'list-units' in args:
        for unit in ['tatami-swaybg', 'tatami-keep-awake', 'tatami-keep-awake-temporary']:
            print(unit + '.service loaded active running test')
    if 'try-restart' in args:
        assert not any('keep-awake' in arg for arg in args), args
        if p('/state/fail-reload').exists(): sys.exit(1)
elif name == 'hyprctl':
    if 'instances' in args: print('[]') # Pinned/unknown; must not force logout when unchanged.
    else: print('[]')
elif name == 'busctl': print('b true')
elif name == 'start-hyprland': pass # Session launcher wrote its receipt before exec.
else: sys.exit(2)
'''

class SessionUpdate(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='tatami-update-test-')
        self.root = Path(self.tmp.name)
        for directory in ['etc/xdg/tatami', 'run/user/1000', 'home/alice', 'state', 'fake', 'bin', 'nix/store']:
            (self.root / directory).mkdir(parents=True, exist_ok=True)
        self.desired = {'version': 1, 'session': {'compositor': 'old'},
                        'reload': {'hyprland': 'same', 'background': 'old'}, 'keep_awake': 'old'}
        self.write_manifest()
        for name in ['systemctl', 'hyprctl', 'busctl', 'start-hyprland']:
            file = self.root / 'fake' / name
            file.write_text(f'#!{PYTHON}\n' + FAKE)
            file.chmod(0o755)
        self.base = ['bwrap', '--unshare-all', '--bind', str(self.root), '/',
                     '--ro-bind', '/nix/store', '/nix/store', '--ro-bind', str(BIN), '/bin/tatami',
                     '--proc', '/proc', '--dev', '/dev', '--clearenv',
                     '--setenv', 'PATH', '/fake', '--setenv', 'HOME', '/home/alice',
                     '--setenv', 'XDG_RUNTIME_DIR', '/run/user/1000', '--chdir', '/']
        self.run_tatami('session')
        self.original = self.receipt()
        (self.root / 'state/calls').write_text('')

    def tearDown(self):
        self.tmp.cleanup()

    def write_manifest(self):
        (self.root / 'etc/xdg/tatami/update.json').write_text(json.dumps(self.desired))

    def receipt(self):
        return json.loads((self.root / 'run/user/1000/tatami/update-session.json').read_text())

    def run_tatami(self, command, ok=True):
        result = subprocess.run(self.base + ['/bin/tatami', command], text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)

    def calls(self):
        return [json.loads(line) for line in (self.root / 'state/calls').read_text().splitlines()]

    def test_unchanged_desktop_does_not_reload_or_ask_for_new_login(self):
        self.run_tatami('update-session')
        self.assertEqual(self.receipt(), self.original)
        self.assertFalse(any('try-restart' in call or call[0] == 'hyprctl' for call in self.calls()))

    def test_wallpaper_applies_without_touching_pinned_compositor_or_inhibitors(self):
        (self.root / 'state/awake').touch()
        self.desired['reload']['background'] = 'new'
        self.write_manifest()
        self.run_tatami('update-session')
        self.assertEqual(self.receipt()['reload'], self.desired['reload'])
        self.assertFalse(any(call[0] == 'hyprctl' for call in self.calls()))
        self.assertTrue(any('try-restart' in call for call in self.calls()))

    def test_changed_pinned_configuration_stays_pending(self):
        self.desired['reload']['hyprland'] = 'new'
        self.write_manifest()
        self.run_tatami('update-session')
        self.assertEqual(self.receipt()['reload']['hyprland'], 'same')
        self.run_tatami('session')
        self.assertEqual(self.receipt()['reload']['hyprland'], 'new')

    def test_compositor_and_active_keep_awake_wait_for_one_new_session(self):
        (self.root / 'state/awake').touch()
        self.desired['session']['compositor'] = 'new'
        self.desired['keep_awake'] = 'new'
        self.write_manifest()
        self.run_tatami('update-session')
        self.assertEqual(self.receipt()['session'], self.original['session'])
        self.assertEqual(self.receipt()['keep_awake'], 'old')
        self.run_tatami('session')
        self.assertEqual(self.receipt()['session'], self.desired['session'])
        self.assertEqual(self.receipt()['keep_awake'], 'new')

    def test_failed_reload_records_failure_and_preserves_old_fingerprint(self):
        (self.root / 'state/fail-reload').touch()
        self.desired['reload']['background'] = 'new'
        self.write_manifest()
        self.run_tatami('update-session', ok=False)
        self.assertEqual(self.receipt()['reload'], self.original['reload'])
        self.assertIsNotNone(self.receipt()['error'])

if __name__ == '__main__':
    if not shutil.which('bwrap'):
        raise SystemExit('bubblewrap is required; do not run these fixtures against the host')
    unittest.main()
