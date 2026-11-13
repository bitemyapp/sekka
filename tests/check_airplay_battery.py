"""Offline tests: fake helpers and loopback Unix sockets, no Wi-Fi operations."""
import importlib.util
import io
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('battery', ROOT / 'scripts/airplay_battery.py')
battery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(battery)


class Contract(unittest.TestCase):
    def manifest(self):
        cases = json.loads((ROOT / 'tests/airplay-battery-cases.json').read_text())
        return {'cases': cases, 'helpers': {c['driver']: '/immutable/helper' for c in cases}, 'senders': {c.get('sender','default'): '/immutable/sender' for c in cases}}

    def test_all_cases_are_unique_bounded_and_selectable(self):
        manifest = self.manifest()
        self.assertEqual(len(battery.checked_cases(manifest)), 34)
        self.assertEqual(len(battery.selected_cases(manifest, 'quick', None)), 3)
        self.assertEqual(len(battery.selected_cases(manifest, 'media', None)), 11)
        self.assertTrue(all(not c['human'] for c in battery.selected_cases(manifest, 'quick', None)))
        with self.assertRaises(ValueError):
            battery.selected_cases(manifest, 'quick', ['../run-command'])
        manifest['cases'][0]['radio'][1] = '999999'
        with self.assertRaises(ValueError): battery.checked_cases(manifest)

    def test_manifest_profiles_match_compiled_helpers_and_sender(self):
        radio = (ROOT / 'services/airplay-radio/src/profiles.rs').read_text()
        names = re.findall(r'"([^"\n]+)"', radio.split('pub const NAMES:')[1].split(';')[0])
        media = (ROOT / 'desktop/aizome/apps/kagami/src/battery.rs').read_text()
        profiles = re.findall(r'"([a-z-]+)" =>', media)
        for case in self.manifest()['cases']:
            if case['radio'][0] == 'trial': self.assertIn(case['radio'][2], names)
            if case['human']: self.assertIn(case['media'], profiles)

    def test_no_pairing_or_stream_setup_can_be_reported_as_a_tv_pass(self):
        self.assertEqual(battery.classify('streaming to Test TV'), 'stream_started_unverified')
        self.assertEqual(battery.classify('credentials saved for Test TV'), 'paired_without_stream')
        self.assertEqual(battery.classify('streaming to Test TV\nsend IDR: i/o timeout'), 'first_keyframe_timeout')
        self.assertEqual(battery.classify('ADVANCED_SYSASSERT'), 'radio_reset')
        self.assertEqual(battery.classify('credentials saved\nmirror setup failed: i/o timeout'), 'session_setup_timeout')
        self.assertEqual(battery.classify('reconnect for credential pairing failed: i/o timeout'), 'pairing_transport_timeout')

    def test_automatic_controller_waits_for_pin_and_stops_on_terminal_error(self):
        trial = battery.AutoTrial()
        trial.observe({'state':'up'}, 0)
        self.assertIsNone(trial.stop_reason(89))
        trial.observe({'state':'connecting','attemptStarted':True}, 20)
        trial.observe({'state':'idle'}, 21) # an older queued status poll is not failure
        self.assertIsNone(trial.stop_reason(21))
        trial.observe({'state':'pin_required'}, 30)
        self.assertIsNone(trial.stop_reason(100))
        trial.observe({'state':'idle','error':'setup timeout'}, 105)
        self.assertEqual(trial.stop_reason(106), 'sender_reported_error')

    def test_automatic_streaming_is_unverified_and_time_bounded(self):
        trial = battery.AutoTrial()
        trial.observe({'state':'up'}, 0)
        self.assertEqual(trial.stop_reason(90), 'target_not_selected_within_90s')
        trial.observe({'state':'connecting','attemptStarted':True}, 100)
        self.assertEqual(trial.stop_reason(280), 'session_attempt_deadline')
        trial.observe({'state':'streaming'}, 150)
        self.assertIsNone(trial.stop_reason(224))
        self.assertEqual(trial.stop_reason(225), 'sender_streamed_75s_unverified')

    def test_target_selection_is_exact_and_unambiguous(self):
        d = {'name': 'Test TV', 'ip': 'fe80::123%awdl0', 'port': 7000}
        self.assertEqual(battery.target_device([d], 'Test TV'), d)
        self.assertIsNone(battery.target_device([d, d], 'Test TV'))
        self.assertIsNone(battery.target_device([d], 'Other TV'))
        self.assertEqual(battery.scoped_address(d), ('fe80::123%awdl0', 7000))
        for address in ('127.0.0.1', '172.16.1.1', 'fe80::123%wlan0', '::1%awdl0', '2001:db8::1%awdl0'):
            with self.assertRaises(ValueError): battery.scoped_address({**d, 'ip': address})

    def test_recovery_requires_old_wifi_and_no_radio_interfaces(self):
        before = {'exitCode': 0, 'stdout': 'wlp0:connected:Guest\nlo:connected (externally):lo'}
        self.assertTrue(battery.recovery_ok(before, before, ['wlp0', 'lo']))
        self.assertFalse(battery.recovery_ok(before, before, ['awdl0', 'wlp0']))
        self.assertFalse(battery.recovery_ok(before, {'exitCode': 0, 'stdout': 'wlp0:disconnected:'}, []))
        self.assertFalse(battery.recovery_ok({'error': 'failed'}, before, []))
        self.assertFalse(battery.recovery_ok(before, before, ['wlp0'], driver_restore_pending=True))

    def test_broker_rejects_large_or_non_object_requests(self):
        for value in (b'[]\n', b'x' * 4097):
            with self.assertRaises(ValueError): battery.read_request(io.BytesIO(value))
        self.assertIsNone(battery.read_request(io.BytesIO(b'')))

    def test_mirroring_preserves_radio_settings_and_bounds_driver_reload(self):
        for case in self.manifest()['cases']:
            if case['radio'][0] == 'observe':
                with self.assertRaises(ValueError): battery.mirroring_case(case)
                continue
            manual = battery.mirroring_case(case)
            self.assertTrue(manual['human'])
            self.assertEqual(manual['radio'][2], case['radio'][2])
            self.assertEqual(manual['driver'], case['driver'])
            self.assertEqual(manual['radio'][1], '120' if case['driver'] in battery.RELOAD_HELPERS else '300')
        media = (ROOT / 'desktop/aizome/apps/kagami/src/battery.rs').read_text()
        self.assertEqual(set(battery.MEDIA_PROFILES), set(re.findall(r'"([a-z-]+)" =>', media)))

    def test_tcp_summary_keeps_acknowledged_bytes_separate_from_queued_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'tcp.jsonl'
            path.write_text(json.dumps({'sockets':[[
                'ESTAB 0 118508 [fe80::1]%awdl0:123 [fe80::2]:49187',
                'cubic rtt:86.705/40.938 bytes_sent:49786 bytes_retrans:15158 bytes_acked:23605 unacked:8 lost:8'
            ]]})+'\n')
            result=next(iter(battery.tcp_summary(path).values()))
            self.assertEqual(result['max_bytes_acked'],23605)
            self.assertEqual(result['maxSendQueueBytes'],118508)
            self.assertEqual(result['max_bytes_retrans'],15158)
            self.assertEqual(result['maxRttMs'],86.705)

    def test_report_preserves_unverified_results(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp); case = path / '01-test'; case.mkdir()
            battery.write_json(case / 'case.json', {'id': 'test'})
            battery.write_json(case / 'result.json', {'case': 'test', 'classification': 'stream_started_unverified',
                                                      'tvVerdict': 'unverified', 'recoveryVerified': True})
            self.assertIn('unverified', battery.report(path))
            self.assertEqual(json.loads((path / 'summary.json').read_text())[0]['tvVerdict'], 'unverified')

    def test_report_includes_interrupted_case_without_claiming_recovery(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp); case = path / '01-test'; case.mkdir()
            battery.write_json(case / 'case.json', {'id': 'test'})
            battery.write_json(case / 'interrupted.json', {'error': 'Interrupted'})
            self.assertIn('incomplete', battery.report(path))
            result = json.loads((path / 'summary.json').read_text())[0]
            self.assertFalse(result['recoveryVerified'])
            self.assertEqual(result['tvVerdict'], 'unverified')
            self.assertEqual(result['interrupted']['error'], 'Interrupted')


class FixtureClient:
    def __init__(self, manifest):
        code = ('import importlib.util,json,os,sys; '
                's=importlib.util.spec_from_file_location("b",sys.argv[1]); '
                'b=importlib.util.module_from_spec(s);s.loader.exec_module(b); '
                'b.os.geteuid=lambda:0; b.broker(json.loads(sys.argv[2]))')
        self.process = subprocess.Popen([sys.executable, '-c', code, str(ROOT / 'scripts/airplay_battery.py'), json.dumps(manifest)],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.events = queue.Queue()
        for pipe, label in ((self.process.stdout, 'broker'), (self.process.stderr, 'broker_stderr')):
            threading.Thread(target=battery.lines, args=(pipe, self.events, label), daemon=True).start()
        self.next_record('ready')

    def next_record(self, name):
        for _ in range(100):
            kind, value = self.events.get(timeout=5)
            if kind == 'broker':
                record = json.loads(value)
                if record.get('battery') == name or record.get('state') == name:
                    return record
        raise AssertionError('No ' + name)

    def send(self, command):
        self.process.stdin.write((json.dumps(command) + '\n').encode()); self.process.stdin.flush()

    def close(self):
        self.process.stdin.close(); self.process.wait(timeout=8)
        self.process.stdout.close(); self.process.stderr.close()


class Processes(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.helper = self.root / 'helper'
        self.helper.write_text(f'#!{sys.executable}\n' + '''import json,sys,select,time
print(json.dumps({'state':'starting','args':sys.argv[1:]}),flush=True)
print(json.dumps({'state':'up','interface':'awdl0','index':99}),flush=True)
select.select([sys.stdin],[],[], 0.2 if sys.argv[-1]=='automatic' else 5)
print(json.dumps({'state':'idle'}),flush=True)
''')
        self.helper.chmod(0o700)
        self.app = self.root / 'app'
        self.app.write_text(f'#!{sys.executable}\n' + '''import socket,os,json,sys
s=socket.socket(socket.AF_UNIX);s.connect(os.environ['SEKKA_AIRPLAY_RADIO_RELAY'])
print(json.dumps({'receivers':[]}),flush=True)
while s.recv(65536): pass
''')
        self.app.chmod(0o700)
        self.case = {'id':'fixture','radio':['trial','1','automatic'],'driver':'stock','human':False,
                     'suites':['quick'],'hypothesis':'fixture','instructions':'No real radio is touched.'}
        self.manifest = {'cases':[self.case], 'helpers':{'stock':str(self.helper)}, 'app':str(self.app),
                         'senders':{'default':'/not-called'}, 'tools':{'ss':'/not-called','journalctl':'/not-called','systemctl':'/not-called'}}

    def tearDown(self): self.temp.cleanup()

    def test_broker_eof_releases_child_lease(self):
        self.case['radio'][-1] = 'wait'
        client = FixtureClient(self.manifest)
        client.send({'cmd':'start','case':'fixture'})
        client.next_record('up')
        client.close()
        self.assertEqual(client.process.returncode, 0)
        records=[]
        while not client.events.empty(): records.append(client.events.get())
        self.assertTrue(any('case_end' in value for _,value in records))

    def test_broker_unknown_case_never_starts_helper(self):
        client = FixtureClient(self.manifest)
        client.send({'cmd':'start','case':'../../bin/sh'})
        self.assertIn('Unknown case', client.next_record('error')['error'])
        client.close()
        self.assertEqual(client.process.returncode, 0)

    def test_broker_promotes_only_fixed_mirroring_duration(self):
        client = FixtureClient(self.manifest)
        try:
            client.send({'cmd':'start','case':'fixture','mirror':True})
            self.assertEqual(client.next_record('starting')['args'], ['trial','300','automatic'])
            client.next_record('case_end')
            client.send({'cmd':'start','case':'fixture','mirror':99999})
            self.assertIn('Invalid mirroring', client.next_record('error')['error'])
        finally: client.close()

    def test_complete_automatic_case_relay_logs_and_cleanup(self):
        client = FixtureClient(self.manifest)
        network = {'exitCode':0,'stdout':'wlp0:connected:Guest','stderr':''}
        destination = self.root / 'results'
        try:
            with patch.object(battery, 'network_state', return_value=network), \
                 patch.object(battery, 'command_output', return_value={'exitCode':0,'stdout':'','stderr':''}), \
                 patch.object(battery.os, 'listdir', return_value=['wlp0','lo']):
                result = battery.run_case(self.manifest, self.case, client, destination, 'Test TV', network)
            self.assertTrue(result['recoveryVerified'])
            self.assertEqual(result['tvVerdict'], 'unverified')
            self.assertEqual(result['radioExitCode'], 0)
            self.assertTrue((destination / 'events.jsonl').read_text())
            self.assertTrue((destination / 'result.json').exists())
        finally: client.close()

    def test_window_exit_releases_active_radio_lease(self):
        self.case['radio'][-1] = 'wait'
        self.app.write_text(f'#!{sys.executable}\n' + '''import socket,os
s=socket.socket(socket.AF_UNIX);s.connect(os.environ['SEKKA_AIRPLAY_RADIO_RELAY'])
s.recv(65536)
s.close()
''')
        client = FixtureClient(self.manifest)
        network = {'exitCode':0,'stdout':'wlp0:connected:Guest','stderr':''}
        try:
            with patch.object(battery, 'network_state', return_value=network), \
                 patch.object(battery, 'command_output', return_value={'exitCode':0,'stdout':'','stderr':''}), \
                 patch.object(battery.os, 'listdir', return_value=['wlp0','lo']):
                result = battery.run_case(self.manifest, self.case, client, self.root / 'results', 'Test TV', network)
            self.assertEqual(result['radioExitCode'], 0)
            self.assertTrue(result['recoveryVerified'])
        finally: client.close()

    def test_autonomous_error_stops_lease_without_operator_verdict(self):
        self.case.update(autonomous=True, human=True, media='synthetic-low', deviceId='00:11:22:33:44:55')
        self.case['radio'][-1] = 'wait'
        self.app.write_text(f'#!{sys.executable}\n' + '''import socket,os,json
s=socket.socket(socket.AF_UNIX);s.connect(os.environ['SEKKA_AIRPLAY_RADIO_RELAY'])
s.recv(65536)
print('[airplay-state] '+json.dumps({'state':'connecting','attemptStarted':True}),flush=True)
print('[airplay-state] '+json.dumps({'state':'idle','error':'fixture timeout'}),flush=True)
while s.recv(65536): pass
''')
        client = FixtureClient(self.manifest)
        network = {'exitCode':0,'stdout':'wlp0:connected:Guest','stderr':''}
        try:
            with patch.object(battery, 'network_state', return_value=network), \
                 patch.object(battery, 'command_output', return_value={'exitCode':0,'stdout':'','stderr':''}), \
                 patch.object(battery.os, 'listdir', return_value=['wlp0','lo']), \
                 patch('builtins.input', side_effect=AssertionError('No manual verdict in autonomous mode')):
                result = battery.run_case(self.manifest, self.case, client, self.root / 'results', 'Test TV', network)
            self.assertEqual(result['automaticStop'], 'sender_reported_error')
            self.assertEqual(result['tvVerdict'], 'unverified')
            self.assertTrue(result['recoveryVerified'])
        finally: client.close()

    def test_failed_recovery_stops_case_and_retains_evidence(self):
        client = FixtureClient(self.manifest)
        before = {'exitCode':0,'stdout':'wlp0:connected:Guest','stderr':''}
        after = {'exitCode':0,'stdout':'wlp0:disconnected:','stderr':''}
        destination = self.root / 'results'
        try:
            with patch.object(battery, 'network_state', return_value=after), \
                 patch.object(battery, 'command_output', return_value={'exitCode':0,'stdout':'','stderr':''}), \
                 patch.object(battery.os, 'listdir', return_value=['wlp0','lo']):
                with self.assertRaisesRegex(RuntimeError, 'Wi-Fi recovery was not verified'):
                    battery.run_case(self.manifest, self.case, client, destination, 'Test TV', before)
            self.assertFalse(json.loads((destination / 'result.json').read_text())['recoveryVerified'])
            self.assertTrue((destination / 'events.jsonl').read_text())
        finally: client.close()


if __name__ == '__main__': unittest.main()
