#!/usr/bin/env python3
"""Exercise the real Tatami helper in a private namespace, never host displays."""
import json
import fcntl
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest

BIN = Path(sys.argv.pop(1)).resolve()
PYTHON = Path(sys.executable).resolve()
FAKE = r'''
import json, pathlib, re, sys, time
p = pathlib.Path
args = sys.argv[1:]
with open('/state/calls', 'a') as f: f.write(json.dumps(args) + '\n')
state = json.loads(p('/state/monitors').read_text())
def save():
    temporary = p('/state/monitors.tmp')
    temporary.write_text(json.dumps(state))
    temporary.replace('/state/monitors')
if args == ['-j', 'monitors', 'all']:
    if p('/state/hang').exists(): time.sleep(60)
    print(json.dumps(state))
elif args[0] == 'eval':
    rule = args[1]
    if p('/state/refuse').exists(): print('ok'); sys.exit(0)
    if p('/state/reject-saved').exists() and 'mode = "preferred"' not in rule:
        print('invalid mode'); sys.exit(0)
    for body in re.findall(r'hl\.monitor\(\{(.*?)\}\)', rule, re.S):
        name = re.search(r'output = "([^"]+)"', body).group(1)
        monitor = next(m for m in state if m['name'] == name)
        # The real Lua API MERGES fields. Omitting disabled or mirror must
        # preserve the earlier value, reproducing the original enable bug.
        if 'disabled = false' in body: monitor['disabled'] = False
        if 'disabled = true' in body: monitor['disabled'] = True
        mirror = re.search(r'mirror = "([^"]*)"', body)
        if mirror:
            monitor['mirrorOf'] = str(next(m['id'] for m in state if m['name'] == mirror[1])) if mirror[1] else 'none'
        if 'mode = "preferred"' in body:
            monitor.update(width=1920, height=1080, refreshRate=60.0, scale=1.0, transform=0)
    save()
    print('ok')
elif args[0] == 'dispatch':
    for monitor in state: monitor['dpmsStatus'] = True
    save()
    print('ok')
else: sys.exit(2)
'''

def monitor(name, ident, disabled, mirror='none'):
    return dict(name=name, id=ident, disabled=disabled, mirrorOf=mirror,
                description=name, width=1920, height=1080, refreshRate=60.0,
                scale=1.0, transform=0, dpmsStatus=True)

class Recovery(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='tatami-display-test-')
        self.root = Path(self.tmp.name)
        for folder in ['state', 'fake', 'bin', 'home/alice', 'run/user/1000/tatami',
                       'run/user/1000/hypr/test', 'nix/store']:
            (self.root / folder).mkdir(parents=True, exist_ok=True)
        fake = self.root / 'fake/hyprctl'
        fake.write_text(f'#!{PYTHON}\n' + FAKE)
        fake.chmod(0o755)
        self.base = ['bwrap', '--unshare-all', '--bind', str(self.root), '/',
                     '--ro-bind', '/nix/store', '/nix/store', '--ro-bind', str(BIN), '/bin/tatami',
                     '--proc', '/proc', '--dev', '/dev', '--clearenv',
                     '--setenv', 'PATH', '/fake:' + os.environ['PATH'],
                     '--setenv', 'HOME', '/home/alice', '--setenv', 'XDG_RUNTIME_DIR', '/run/user/1000',
                     '--setenv', 'HYPRLAND_INSTANCE_SIGNATURE', 'test', '--chdir', '/']
        self.set_monitors([monitor('eDP-1', 0, True), monitor('FALLBACK', 1, False)])
        (self.root / 'run/user/1000/tatami/monitors-off').write_text('eDP-1\n')
        (self.root / 'run/user/1000/tatami/monitors.lua').write_text('hl.monitor({ output = "eDP-1", disabled = true })\n')

    def tearDown(self):
        self.tmp.cleanup()

    def set_monitors(self, monitors):
        temporary = self.root / 'state/monitors.input'
        temporary.write_text(json.dumps(monitors))
        temporary.replace(self.root / 'state/monitors')

    def monitors(self):
        return json.loads((self.root / 'state/monitors').read_text())

    def calls(self):
        return [json.loads(line) for line in (self.root / 'state/calls').read_text().splitlines()]

    def run_tatami(self, *args, data=None, ok=True):
        result = subprocess.run(self.base + ['/bin/tatami', *args],
                                input=json.dumps(data) if data else None, text=True,
                                capture_output=True, timeout=20)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        return result

    def apply(self, mirror=None):
        displays = [{'name':'eDP-1','enabled':True}, {'name':'DP-1','enabled':True}]
        if mirror: displays[0]['mirror'] = mirror
        self.run_tatami('displays', 'apply', data={'displays':displays, 'main':'eDP-1', 'apply':['eDP-1']})

    def test_manual_enable_clears_disabled_and_stale_mirror(self):
        self.set_monitors([monitor('eDP-1', 0, True, '2'), monitor('DP-1', 2, False)])
        self.apply()
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertEqual(self.monitors()[0]['mirrorOf'], 'none')
        self.assertEqual((self.root / 'run/user/1000/tatami/monitors-off').read_text(), '')

    def test_enable_as_mirror_then_extend_clears_the_mirror(self):
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        self.apply('DP-1')
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertEqual(self.monitors()[0]['mirrorOf'], '2')
        self.apply()
        self.assertEqual(self.monitors()[0]['mirrorOf'], 'none')

    def test_recovers_and_keeps_the_panel_on_through_reload(self):
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertEqual((self.root / 'run/user/1000/tatami/monitors-off').read_text(), '')
        saved = (self.root / 'home/alice/.local/state/tatami/displays.lua').read_text()
        self.assertIn('disabled = false', saved)
        self.assertIn('mirror = ""', saved)
        count = len([c for c in self.calls() if c[0] == 'eval'])
        self.run_tatami('displays', 'recover')
        self.assertEqual(len([c for c in self.calls() if c[0] == 'eval']), count)

    def test_docked_and_dpms_sleeping_screens_are_unchanged(self):
        external = monitor('DP-1', 2, False)
        external['dpmsStatus'] = False
        original = [monitor('eDP-1', 0, True), external]
        self.set_monitors(original)
        self.run_tatami('displays', 'recover')
        self.assertEqual(self.monitors(), original)
        self.assertFalse(any(c[0] == 'eval' for c in self.calls()))

    def test_bad_saved_mode_falls_back_to_preferred(self):
        (self.root / 'state/reject-saved').touch()
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertTrue(any('mode = "preferred"' in c[-1] for c in self.calls()))

    def test_acknowledgement_without_enabled_output_is_an_error(self):
        (self.root / 'state/refuse').touch()
        result = self.run_tatami('displays', 'recover', ok=False)
        self.assertIn('did not turn on', result.stderr)
        self.assertTrue(self.monitors()[0]['disabled'])

    def test_manual_enable_checks_the_result_not_just_the_acknowledgement(self):
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        (self.root / 'state/refuse').touch()
        result = self.run_tatami('displays', 'apply', data={
            'displays':[{'name':'eDP-1','enabled':True}], 'apply':['eDP-1']}, ok=False)
        self.assertIn('did not apply', result.stderr)

    def test_identical_displays_keep_distinct_saved_settings(self):
        twins = [monitor('DP-1', 1, True), monitor('DP-2', 2, True)]
        for m in twins: m['description'] = 'Identical monitor'
        self.set_monitors(twins)
        self.run_tatami('displays', 'recover')
        saved = json.loads((self.root / 'home/alice/.local/state/tatami/displays.json').read_text())
        self.assertIn('DP-1', saved['displays'])
        self.assertNotIn('desc:Identical monitor', saved['displays'])
        self.assertTrue(self.monitors()[1]['disabled'])

    def test_unresponsive_compositor_times_out(self):
        (self.root / 'state/hang').touch()
        result = self.run_tatami('displays', 'recover', ok=False)
        self.assertIn('timeout', result.stderr)

    def test_concurrent_display_change_does_not_block_forever(self):
        with (self.root / 'run/user/1000/tatami/displays.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            result = self.run_tatami('displays', 'recover', ok=False)
            self.assertIn('still in progress', result.stderr)

    def test_watch_recovers_even_when_hotplug_event_is_missed(self):
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(self.root / 'run/user/1000/hypr/test/.socket2.sock'))
        server.listen(1)
        server.settimeout(5)
        process = subprocess.Popen(self.base + ['/bin/tatami', 'displays', 'watch'],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            connection, _ = server.accept()
            time.sleep(0.2)
            self.set_monitors([monitor('eDP-1', 0, True), monitor('FALLBACK', 1, False)])
            until = time.monotonic() + 6
            while self.monitors()[0]['disabled'] and time.monotonic() < until:
                time.sleep(0.05)
            self.assertFalse(self.monitors()[0]['disabled'])
            connection.close()
            process.wait(timeout=5)
            self.assertNotEqual(process.returncode, 0) # systemd reconnects it
        finally:
            process.terminate()
            process.communicate(timeout=5)
            server.close()

if __name__ == '__main__':
    unittest.main()
