#!/usr/bin/env python3
"""Exercise the real Tatami helper in a private namespace, never host displays.

A fake Hyprland answers on the real command and event sockets, and a fake
busctl reports logind's lid; the host's /proc and /sys are never consulted.
"""
import json
import fcntl
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

BIN = Path(sys.argv.pop(1)).resolve()
PYTHON = Path(sys.executable).resolve()

BUSCTL = r'''
import pathlib, sys
if 'LidClosed' in sys.argv:
    lid = pathlib.Path('/state/lid')
    print('b true' if lid.exists() and lid.read_text().strip() == 'closed' else 'b false')
    sys.exit(0)
sys.exit(1)
'''

# Display paths must use the socket: hyprctl here fails, and is logged.
HYPRCTL = r'''
import json, sys
with open('/state/calls', 'a') as f: f.write(json.dumps(['hyprctl'] + sys.argv[1:]) + '\n')
sys.exit(2)
'''


def monitor(name, ident, disabled, mirror='none', x=0):
    return dict(name=name, id=ident, disabled=disabled, mirrorOf=mirror,
                description=name, width=1920, height=1080, refreshRate=60.0,
                scale=1.0, transform=0, dpmsStatus=True, x=x, y=0)


class FakeHyprland:
    """Hyprland's command socket: one request per connection, answered by
    closing it. eval merges fields as the real Lua API does, so a rule
    omitting `disabled` or `mirror` keeps the earlier value."""

    def __init__(self, root):
        self.root = root
        self.state = root / 'state'
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.server = socket.socket(socket.AF_UNIX)
        self.server.bind(str(root / 'run/user/1000/hypr/test/.socket.sock'))
        self.server.listen(16)
        self.server.settimeout(0.2)
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def close(self):
        self.stop.set()
        self.thread.join(timeout=5)
        self.server.close()

    def serve(self):
        while not self.stop.is_set():
            try:
                connection, _ = self.server.accept()
            except socket.timeout:
                continue
            threading.Thread(target=self.handle, args=(connection,), daemon=True).start()

    def log(self, call):
        with open(self.state / 'calls', 'a') as f:
            f.write(json.dumps(call) + '\n')

    def load(self, name):
        return json.loads((self.state / name).read_text())

    def save(self, name, value):
        temporary = self.state / (name + '.tmp')
        temporary.write_text(json.dumps(value))
        temporary.replace(self.state / name)

    def handle(self, connection):
        with connection:
            request = b''
            while True:
                chunk = connection.recv(65536)
                if not chunk:
                    break
                request += chunk
            reply = self.reply(request.decode())
            if reply is not None:
                connection.sendall(reply.encode())

    def reply(self, request):
        if request == 'j/monitors all':
            self.log(['-j', 'monitors', 'all'])
            if (self.state / 'hang').exists():
                self.stop.wait(60)
                return None
            with self.lock:
                return json.dumps(self.load('monitors'))
        if request == 'j/monitors':
            with self.lock:
                return json.dumps([m for m in self.load('monitors') if not m['disabled']])
        if request == 'j/clients':
            self.log(['-j', 'clients'])
            with self.lock:
                return json.dumps(self.load('clients'))
        if request.startswith('/eval '):
            lua = request[len('/eval '):]
            self.log(['eval', lua])
            if (self.state / 'refuse').exists():
                return 'ok'
            if (self.state / 'reject-saved').exists() and 'mode = "preferred"' not in lua:
                return 'error: invalid mode'
            with self.lock:
                self.evaluate(lua)
            return 'ok'
        if request.startswith('/dispatch '):
            expression = request[len('/dispatch '):]
            self.log(['dispatch', expression])
            with self.lock:
                state = self.load('monitors')
                dpms = re.search(r'action = "(on|off)"(?:, monitor = "([^"]+)")?', expression)
                for m in state:
                    if dpms and dpms[2] in (None, m['name']):
                        m['dpmsStatus'] = dpms[1] == 'on'
                self.save('monitors', state)
            return 'ok'
        return 'unknown request'

    def evaluate(self, lua):
        state = self.load('monitors')
        for body in re.findall(r'hl\.monitor\(\{(.*?)\}\)', lua, re.S):
            name = re.search(r'output = "([^"]+)"', body).group(1)
            m = next(m for m in state if m['name'] == name)
            if 'disabled = false' in body:
                m['disabled'] = False
            if 'disabled = true' in body:
                m['disabled'] = True
            mirror = re.search(r'mirror = "([^"]*)"', body)
            if mirror:
                m['mirrorOf'] = str(next(o['id'] for o in state if o['name'] == mirror[1])) if mirror[1] else 'none'
            if 'mode = "preferred"' in body:
                m.update(width=1920, height=1080, refreshRate=60.0, scale=1.0, transform=0)
        self.save('monitors', state)
        centered = re.findall(r'hl\.dsp\.window\.center\(\{ window = "address:(0x[0-9a-f]+)" \}\)', lua)
        if centered:
            clients = self.load('clients')
            for c in clients:
                if c['address'] in centered:
                    screen = next(m for m in state if m['id'] == c['monitor'])
                    c['at'] = [screen['x'] + (screen['width'] - c['size'][0]) // 2,
                               screen['y'] + (screen['height'] - c['size'][1]) // 2]
            self.save('clients', clients)


class Recovery(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='tatami-display-test-')
        self.root = Path(self.tmp.name)
        for folder in ['state', 'fake', 'bin', 'home/alice', 'run/user/1000/tatami',
                       'run/user/1000/hypr/test', 'nix/store', 'drm']:
            (self.root / folder).mkdir(parents=True, exist_ok=True)
        for name, script in [('busctl', BUSCTL), ('hyprctl', HYPRCTL)]:
            fake = self.root / 'fake' / name
            fake.write_text(f'#!{PYTHON}\n' + script)
            fake.chmod(0o755)
        self.base = ['bwrap', '--unshare-all', '--bind', str(self.root), '/',
                     '--ro-bind', '/nix/store', '/nix/store', '--ro-bind', str(BIN), '/bin/tatami',
                     '--proc', '/proc', '--dev', '/dev', '--tmpfs', '/sys',
                     '--ro-bind', str(self.root / 'drm'), '/sys/class/drm', '--clearenv',
                     '--setenv', 'PATH', '/fake:' + os.environ['PATH'],
                     '--setenv', 'HOME', '/home/alice', '--setenv', 'XDG_RUNTIME_DIR', '/run/user/1000',
                     '--setenv', 'HYPRLAND_INSTANCE_SIGNATURE', 'test', '--chdir', '/']
        self.set_monitors([monitor('eDP-1', 0, True), monitor('FALLBACK', 1, False)])
        self.set_clients([])
        self.runtime = self.root / 'run/user/1000/tatami'
        (self.runtime / 'monitors-off').write_text('eDP-1\n')
        (self.runtime / 'monitors.lua').write_text('hl.monitor({ output = "eDP-1", disabled = true })\n')
        self.hyprland = FakeHyprland(self.root)

    def tearDown(self):
        self.hyprland.close()
        self.tmp.cleanup()

    def set_monitors(self, monitors):
        temporary = self.root / 'state/monitors.input'
        temporary.write_text(json.dumps(monitors))
        temporary.replace(self.root / 'state/monitors')

    def set_clients(self, clients):
        temporary = self.root / 'state/clients.input'
        temporary.write_text(json.dumps(clients))
        temporary.replace(self.root / 'state/clients')

    def set_lid(self, state):
        (self.root / 'state/lid').write_text(state)

    def monitors(self):
        return json.loads((self.root / 'state/monitors').read_text())

    def display(self, name):
        return next(m for m in self.monitors() if m['name'] == name)

    def calls(self):
        path = self.root / 'state/calls'
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def evals(self):
        return [c[1] for c in self.calls() if c[0] == 'eval']

    def switched_off(self):
        path = self.runtime / 'monitors-off'
        return path.read_text().split() if path.exists() else []

    def run_tatami(self, *args, data=None, ok=True, only_fakes=False):
        # Only the fakes on PATH: nothing of the host's to start.
        path = ['--setenv', 'PATH', '/fake'] if only_fakes else []
        result = subprocess.run(self.base + path + ['/bin/tatami', *args],
                                input=json.dumps(data) if data else None, text=True,
                                capture_output=True, timeout=30)
        self.assertEqual(result.returncode == 0, ok, result.stdout + result.stderr)
        self.assertFalse(any(c[0] == 'hyprctl' for c in self.calls()), self.calls())
        return result

    def apply(self, mirror=None):
        displays = [{'name':'eDP-1','enabled':True}, {'name':'DP-1','enabled':True}]
        if mirror: displays[0]['mirror'] = mirror
        self.run_tatami('displays', 'apply', data={'displays':displays, 'main':'eDP-1', 'apply':['eDP-1']})

    def wait_for(self, predicate, seconds=8):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if predicate():
                return
            time.sleep(0.05)
        self.fail(json.dumps(self.monitors()))

    def watch(self):
        """`tatami displays watch` with an event socket the test writes to."""
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(self.root / 'run/user/1000/hypr/test/.socket2.sock'))
        server.listen(1)
        server.settimeout(5)
        process = subprocess.Popen(self.base + ['/bin/tatami', 'displays', 'watch'],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        connection, _ = server.accept()
        def stop():
            # The watcher ends when its event socket closes; bwrap does not
            # pass on signals.
            connection.close()
            server.close()
            try:
                process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.communicate()
        self.addCleanup(stop)
        return process, connection

    # Enabling from the Displays window.

    def test_manual_enable_clears_disabled_and_stale_mirror(self):
        self.set_monitors([monitor('eDP-1', 0, True, '2'), monitor('DP-1', 2, False)])
        self.apply()
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertEqual(self.monitors()[0]['mirrorOf'], 'none')
        self.assertEqual(self.switched_off(), [])

    def test_enable_as_mirror_then_extend_clears_the_mirror(self):
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        self.apply('DP-1')
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertEqual(self.monitors()[0]['mirrorOf'], '2')
        self.apply()
        self.assertEqual(self.monitors()[0]['mirrorOf'], 'none')

    def test_manual_enable_checks_the_result_not_just_the_acknowledgement(self):
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        (self.root / 'state/refuse').touch()
        result = self.run_tatami('displays', 'apply', data={
            'displays':[{'name':'eDP-1','enabled':True}], 'apply':['eDP-1']}, ok=False)
        self.assertIn('did not apply', result.stderr)

    # A screen always on.

    def test_recovers_and_keeps_the_panel_on_through_reload(self):
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertEqual(self.switched_off(), [])
        self.assertEqual((self.runtime / 'monitors.lua').read_text(), '')
        saved = (self.root / 'home/alice/.local/state/tatami/displays.lua').read_text()
        self.assertIn('disabled = false', saved)
        self.assertIn('mirror = ""', saved)
        count = len(self.evals())
        self.run_tatami('displays', 'recover')
        self.assertEqual(len(self.evals()), count)

    def test_recovery_keeps_the_arranged_position(self):
        store = self.root / 'home/alice/.local/state/tatami'
        store.mkdir(parents=True)
        (store / 'displays.json').write_text(json.dumps({'displays': {'desc:eDP-1': {
            'name': 'eDP-1', 'description': 'eDP-1', 'mode': '1920x1080@60', 'position': '1920x0',
            'scale': 1, 'mirror': 'DP-1'}}}))
        self.run_tatami('displays', 'recover')
        saved = json.loads((store / 'displays.json').read_text())['displays']['desc:eDP-1']
        self.assertEqual(saved['position'], '1920x0')
        self.assertNotIn('mirror', saved)

    def test_docked_and_dpms_sleeping_screens_are_unchanged(self):
        external = monitor('DP-1', 2, False)
        external['dpmsStatus'] = False
        original = [monitor('eDP-1', 0, True), external]
        self.set_monitors(original)
        self.run_tatami('displays', 'recover')
        self.assertEqual(self.monitors(), original)
        self.assertEqual(self.evals(), [])

    def test_bad_saved_mode_falls_back_to_preferred(self):
        (self.root / 'state/reject-saved').touch()
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.monitors()[0]['disabled'])
        self.assertTrue(any('mode = "preferred"' in lua for lua in self.evals()))

    def test_acknowledgement_without_enabled_output_is_an_error(self):
        (self.root / 'state/refuse').touch()
        result = self.run_tatami('displays', 'recover', ok=False)
        self.assertIn('did not turn on', result.stderr)
        self.assertTrue(self.monitors()[0]['disabled'])

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
        started = time.monotonic()
        result = self.run_tatami('displays', 'recover', ok=False)
        self.assertIn('timeout', result.stderr)
        self.assertLess(time.monotonic() - started, 10)

    def test_concurrent_display_change_does_not_block_forever(self):
        with (self.runtime / 'displays.lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            result = self.run_tatami('displays', 'recover', ok=False)
            self.assertIn('still in progress', result.stderr)

    def test_watch_recovers_even_when_hotplug_event_is_missed(self):
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        process, connection = self.watch()
        time.sleep(0.2)
        self.set_monitors([monitor('eDP-1', 0, True), monitor('FALLBACK', 1, False)])
        self.wait_for(lambda: not self.display('eDP-1')['disabled'], 6)
        connection.close()
        process.wait(timeout=5)
        self.assertNotEqual(process.returncode, 0) # systemd reconnects it

    # Closed-display mode.

    def docked(self, panel=True):
        self.set_monitors([monitor('eDP-1', 0, not panel), monitor('DP-1', 2, False, x=1920)])
        (self.runtime / 'monitors-off').write_text('')

    def test_closing_the_lid_while_docked_turns_the_panel_off_and_opening_restores_it(self):
        self.docked()
        self.run_tatami('displays', 'recover')
        self.assertEqual(self.evals(), [])
        self.set_lid('closed')
        self.run_tatami('displays', 'recover')
        self.assertTrue(self.display('eDP-1')['disabled'])
        self.assertFalse(self.display('DP-1')['disabled'])
        self.assertEqual(self.switched_off(), ['eDP-1'])
        self.assertIn('"eDP-1", disabled = true', (self.runtime / 'monitors.lua').read_text())
        self.assertEqual(self.run_tatami('displays', 'lid').stdout.strip(), 'closed')
        self.set_lid('open')
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.display('eDP-1')['disabled'])
        self.assertEqual(self.switched_off(), [])

    def test_the_closed_lid_refuses_to_light_the_panel_while_docked(self):
        self.docked(panel=False)
        self.set_lid('closed')
        result = self.run_tatami('displays', 'apply', ok=False, data={
            'displays': [{'name': 'eDP-1', 'enabled': True}, {'name': 'DP-1', 'enabled': True}],
            'apply': ['eDP-1']})
        self.assertIn('lid is closed', result.stderr)
        self.assertTrue(self.display('eDP-1')['disabled'])
        # Other changes still apply, and do not count as turning the panel off.
        self.run_tatami('displays', 'apply', data={
            'displays': [{'name': 'eDP-1', 'enabled': False}, {'name': 'DP-1', 'enabled': True, 'scale': 2}],
            'apply': ['DP-1']})
        self.set_lid('open')
        self.run_tatami('displays', 'recover')
        self.assertTrue(self.display('eDP-1')['disabled'])  # never turned off by the lid here
        session = json.loads((self.runtime / 'displays-session.json').read_text())
        self.assertEqual(session['off'], {})

    def test_mirrors_stop_before_the_display_they_show_goes(self):
        self.set_monitors([monitor('eDP-1', 0, False), monitor('DP-1', 2, False, mirror='0')])
        (self.runtime / 'monitors-off').write_text('')
        self.set_lid('closed')
        self.run_tatami('displays', 'recover')
        self.assertEqual(self.display('DP-1')['mirrorOf'], 'none')
        self.assertTrue(self.display('eDP-1')['disabled'])
        evals = self.evals()
        unmirror = next(i for i, lua in enumerate(evals) if '"DP-1"' in lua and 'mirror = ""' in lua)
        disable = next(i for i, lua in enumerate(evals) if '"eDP-1", disabled = true' in lua)
        self.assertLess(unmirror, disable)
        self.assertNotIn('disabled = true', evals[unmirror])

    def test_a_panel_alone_is_blanked_as_the_lid_closes_and_woken_as_it_opens(self):
        self.set_monitors([monitor('eDP-1', 0, False)])
        (self.runtime / 'monitors-off').write_text('')
        self.run_tatami('displays', 'recover')
        self.set_lid('closed')
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.display('eDP-1')['disabled'])
        self.assertFalse(self.display('eDP-1')['dpmsStatus'])
        # Woken by a key while closed: left on.
        self.set_monitors([monitor('eDP-1', 0, False)])
        self.run_tatami('displays', 'recover')
        self.assertTrue(self.display('eDP-1')['dpmsStatus'])
        dispatches = len([c for c in self.calls() if c[0] == 'dispatch'])
        self.set_lid('open')
        self.run_tatami('displays', 'recover')
        self.assertEqual(len([c for c in self.calls() if c[0] == 'dispatch']), dispatches + 1)

    def test_undocking_with_the_lid_closed_brings_the_panel_back_behind_it(self):
        self.docked()
        self.set_lid('closed')
        self.run_tatami('displays', 'recover')
        self.assertTrue(self.display('eDP-1')['disabled'])
        self.set_monitors([monitor('eDP-1', 0, True)])
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.display('eDP-1')['disabled'])
        self.assertFalse(self.display('eDP-1')['dpmsStatus'])
        self.set_lid('open')
        self.run_tatami('displays', 'recover')
        self.assertTrue(self.display('eDP-1')['dpmsStatus'])

    # Displays turned off stay off with the same displays.

    def test_a_panel_turned_off_stays_off_whenever_the_same_display_is_connected(self):
        self.docked()
        self.run_tatami('displays', 'apply', data={
            'displays': [{'name': 'eDP-1', 'enabled': False}, {'name': 'DP-1', 'enabled': True}],
            'main': 'DP-1', 'apply': ['eDP-1']})
        self.assertTrue(self.display('eDP-1')['disabled'])
        self.set_monitors([monitor('eDP-1', 0, True)])
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.display('eDP-1')['disabled'])
        # Docked again (on another port): Hyprland lights both; the panel
        # goes off again, once the external display is on.
        moved = monitor('DP-3', 4, False, x=1920)
        moved['description'] = 'DP-1'
        self.set_monitors([monitor('eDP-1', 0, False), moved])
        self.run_tatami('displays', 'recover')
        self.assertTrue(self.display('eDP-1')['disabled'])
        # Turned on again: remembered that way.
        self.run_tatami('displays', 'apply', data={
            'displays': [{'name': 'eDP-1', 'enabled': True}, {'name': 'DP-3', 'enabled': True}],
            'apply': ['eDP-1']})
        self.set_monitors([monitor('eDP-1', 0, False)])
        self.run_tatami('displays', 'recover')
        self.set_monitors([monitor('eDP-1', 0, False), moved])
        self.run_tatami('displays', 'recover')
        self.assertFalse(self.display('eDP-1')['disabled'])

    def test_displays_off_before_an_update_stay_off(self):
        # A running session from before the record: its switched-off panel.
        self.set_monitors([monitor('eDP-1', 0, True), monitor('DP-1', 2, False)])
        self.run_tatami('displays', 'recover')
        self.assertEqual(self.evals(), [])
        self.assertTrue(self.display('eDP-1')['disabled'])

    # The watcher.

    def test_lid_bindings_reach_the_watcher_at_once(self):
        self.docked()
        process, connection = self.watch()
        time.sleep(0.5)
        # logind sees no lid change here, only Hyprland's binding does.
        connection.sendall(b'custom>>tatami-lid-closed\n')
        self.wait_for(lambda: self.display('eDP-1')['disabled'], 3)
        time.sleep(6)  # past a logind poll that still says open
        self.assertTrue(self.display('eDP-1')['disabled'])
        connection.sendall(b'custom>>tatami-lid-open\n')
        self.wait_for(lambda: not self.display('eDP-1')['disabled'], 3)

    def test_watcher_polls_logind_for_a_lid_the_bindings_missed(self):
        self.docked()
        process, connection = self.watch()
        time.sleep(0.5)
        self.set_lid('closed')
        self.wait_for(lambda: self.display('eDP-1')['disabled'], 8)
        self.set_lid('open')
        self.wait_for(lambda: not self.display('eDP-1')['disabled'], 8)

    def test_windows_left_off_screen_by_a_screen_change_come_back(self):
        self.docked()
        window = dict(address='0xabc', mapped=True, hidden=False, floating=True,
                      workspace={'id': 3, 'name': '3'}, monitor=0, at=[-1720, 60], size=[880, 600])
        placed = dict(window, address='0xdef', at=[-400, 60])
        self.set_clients([window, placed])
        process, connection = self.watch()
        time.sleep(1.5)
        # No screen change yet: the application's own placement stands.
        self.assertEqual(json.loads((self.root / 'state/clients').read_text())[0]['at'], [-1720, 60])
        self.set_monitors([monitor('eDP-1', 0, False)])
        connection.sendall(b'monitorremoved>>DP-1\n')
        def rescued():
            clients = json.loads((self.root / 'state/clients').read_text())
            return clients[0]['at'] == [520, 240] and clients[1]['at'] == [-400, 60]
        self.wait_for(rescued, 5)

    def test_login_with_the_lid_closed_starts_with_the_panel_off(self):
        for name, status in [('card1-eDP-1', 'connected'), ('card1-DP-1', 'connected'),
                             ('card1-HDMI-A-1', 'disconnected')]:
            (self.root / 'drm' / name).mkdir()
            (self.root / 'drm' / name / 'status').write_text(status + '\n')
        self.set_lid('closed')
        # start-hyprland is absent here, so the session stops after preparing.
        self.run_tatami('session', ok=False, only_fakes=True)
        self.assertEqual(self.switched_off(), ['eDP-1'])
        session = json.loads((self.runtime / 'displays-session.json').read_text())
        self.assertEqual(session['lid_off'], ['eDP-1'])
        # Undocked, or the lid open: every display starts on.
        self.set_lid('open')
        self.run_tatami('session', ok=False, only_fakes=True)
        self.assertEqual(self.switched_off(), [])
        self.assertFalse((self.runtime / 'displays-session.json').exists())


if __name__ == '__main__':
    unittest.main()
