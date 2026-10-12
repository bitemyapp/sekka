#!/usr/bin/env python3
"""The bar's weather in a disposable namespace: `tatami weather` answers at
once from the last report and fetches apart, one fetch at a time, through a
stand-in curl that answers slowly or not at all."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

PYTHON = Path(sys.executable).resolve()
HERE = Path(__file__).resolve()

FAKE_CURL = r'''
import json, pathlib, sys, time
state = pathlib.Path('/state')
url = sys.argv[-1]
with open(state / 'calls', 'a') as out:
    out.write(url + '\n')
offline = state / 'offline'
if offline.exists() and int(offline.read_text() or 0) > 0:
    offline.write_text(str(int(offline.read_text()) - 1))
    sys.exit(7)
delay = state / 'delay'
time.sleep(float(delay.read_text()) if delay.exists() else 0)
if 'wttr.in' in url:
    print(json.dumps({
        'nearest_area': [{'areaName': [{'value': 'Austin'}],
                          'country': [{'value': 'United States of America'}],
                          'latitude': '30.27', 'longitude': '-97.74'}],
        'current_condition': [], 'weather': []}))
elif 'open-meteo' in url:
    print(json.dumps({
        'current': {'temperature_2m': 21, 'apparent_temperature': 20,
                    'relative_humidity_2m': 40, 'wind_speed_10m': 9,
                    'weather_code': 0, 'is_day': 1},
        'daily': {'time': ['2026-10-12', '2026-10-13', '2026-10-14', '2026-10-15'],
                  'weather_code': [0, 1, 2, 3],
                  'temperature_2m_max': [25, 26, 27, 28],
                  'temperature_2m_min': [10, 11, 12, 13]}}))
else:
    sys.exit(22)
'''
QUIET = '#!/bin/sh\nexit 0\n'


# Inside the namespace: one scenario, its facts printed as JSON.
def inner(scenario):
    cache = Path('/home/alice/.cache/tatami/weather.json')
    calls = Path('/state/calls')

    def weather(*args):
        start = time.monotonic()
        out = subprocess.run(['/bin/tatami', 'weather', *args], capture_output=True,
                             text=True, timeout=30)
        return json.loads(out.stdout or 'null'), time.monotonic() - start

    def fetched():
        try:
            return json.loads(cache.read_text())['fetched']
        except (OSError, ValueError, KeyError):
            return None

    def wait_for(test, timeout=15):
        until = time.monotonic() + timeout
        while time.monotonic() < until:
            if test():
                return True
            time.sleep(0.05)
        return False

    def report(age, located_age):
        now = int(time.time())
        cache.parent.mkdir(parents=True, exist_ok=True)
        cache.write_text(json.dumps({
            'fetched': now - age, 'place': 'Austin', 'imperial': True,
            'current': {'temp': 15, 'feels': 14, 'humidity': 60, 'wind': 5, 'glyph': 'OLD'},
            'days': [],
            'located': {'at': now - located_age, 'name': 'Austin',
                        'country': 'United States of America',
                        'latitude': 30.27, 'longitude': -97.74}}))

    def urls():
        return calls.read_text().splitlines() if calls.exists() else []

    facts = {}
    if scenario == 'first':
        Path('/state/delay').write_text('1')
        facts['bar'], facts['seconds'] = weather()
        facts['arrived'] = wait_for(lambda: fetched() is not None)
        facts['then'], _ = weather()
    elif scenario == 'stale':
        report(20 * 60, 10 * 60)
        Path('/state/delay').write_text('1')
        old = fetched()
        times = []
        for _ in range(5):
            facts['bar'], seconds = weather()
            times.append(seconds)
        facts['seconds'] = max(times)
        facts['arrived'] = wait_for(lambda: fetched() != old)
        time.sleep(0.5)
    elif scenario == 'relocate':
        report(20 * 60, 2 * 60 * 60)
        old = fetched()
        weather()
        facts['arrived'] = wait_for(lambda: fetched() != old)
    elif scenario == 'click':
        report(60, 60)
        Path('/state/delay').write_text('1.5')
        click = subprocess.Popen(['/bin/tatami', 'weather', 'refresh'])
        wait_for(lambda: Path('/run/user/1000/tatami/weather-refreshing').exists(), 5)
        facts['during'], facts['seconds'] = weather()
        click.wait(timeout=30)
        facts['after'], _ = weather()
    elif scenario == 'offline':
        Path('/state/offline').write_text('2')
        weather()
        facts['arrived'] = wait_for(lambda: fetched() is not None, 20)
    facts['calls'] = urls()
    print(json.dumps(facts))


class Weather(unittest.TestCase):
    def run_scenario(self, scenario):
        with tempfile.TemporaryDirectory(prefix='tatami-weather-test-') as tmp:
            root = Path(tmp)
            for directory in ['home/alice', 'run/user/1000', 'state', 'fake', 'bin', 'tmp', 'nix/store']:
                (root / directory).mkdir(parents=True, exist_ok=True)
            (root / 'run/user/1000').chmod(0o700)
            curl = root / 'fake/curl'
            curl.write_text(f'#!{PYTHON}\n' + FAKE_CURL)
            curl.chmod(0o755)
            for quiet in ['notify-send']:
                (root / 'fake' / quiet).write_text(QUIET)
                (root / 'fake' / quiet).chmod(0o755)
            command = [
                'bwrap', '--unshare-all', '--die-with-parent', '--bind', str(root), '/',
                '--ro-bind', '/nix/store', '/nix/store', '--ro-bind', str(BIN), '/bin/tatami',
                '--ro-bind', str(HERE), '/test.py',
                '--proc', '/proc', '--dev', '/dev', '--clearenv',
                '--setenv', 'PATH', '/fake:' + str(Path('/bin')),
                '--setenv', 'HOME', '/home/alice', '--setenv', 'XDG_RUNTIME_DIR', '/run/user/1000',
                '--setenv', 'LANG', 'en_US.UTF-8', '--chdir', '/',
                str(PYTHON), '/test.py', '--inner', scenario,
            ]
            result = subprocess.run(command, text=True, capture_output=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return json.loads(result.stdout.strip().splitlines()[-1])

    def test_the_first_report_is_fetched_apart_from_the_bar(self):
        facts = self.run_scenario('first')
        # The bar does not wait for the two slow requests.
        self.assertLess(facts['seconds'], 0.5, facts)
        self.assertEqual(facts['bar']['text'], ' ')
        self.assertTrue(facts['arrived'], facts)
        self.assertEqual(facts['then']['class'], 'weather')

    def test_an_old_report_shows_at_once_and_is_fetched_once(self):
        facts = self.run_scenario('stale')
        self.assertLess(facts['seconds'], 0.5, facts)
        self.assertEqual(facts['bar']['text'], 'OLD')
        self.assertTrue(facts['arrived'], facts)
        # One fetch for five askings, with the place found an hour ago.
        self.assertEqual([url for url in facts['calls'] if 'open-meteo' in url].__len__(), 1, facts)
        self.assertFalse(any('wttr.in' in url for url in facts['calls']), facts)

    def test_the_place_is_found_again_after_an_hour(self):
        facts = self.run_scenario('relocate')
        self.assertTrue(facts['arrived'], facts)
        self.assertTrue(any('wttr.in' in url for url in facts['calls']), facts)

    def test_a_click_dims_the_icon_until_the_report_is_in(self):
        facts = self.run_scenario('click')
        self.assertLess(facts['seconds'], 0.5, facts)
        self.assertEqual(facts['during']['class'], 'refreshing')
        self.assertEqual(facts['during']['text'], 'OLD')
        self.assertEqual(facts['after']['class'], 'weather')
        self.assertNotEqual(facts['after']['text'], 'OLD')
        # A click finds the place again.
        self.assertTrue(any('wttr.in' in url for url in facts['calls']), facts)

    def test_a_network_still_coming_up_is_tried_again(self):
        facts = self.run_scenario('offline')
        self.assertTrue(facts['arrived'], facts)


if __name__ == '__main__':
    if sys.argv[1:2] == ['--inner']:
        inner(sys.argv[2])
    else:
        BIN = Path(sys.argv.pop(1)).resolve()
        if not shutil.which('bwrap'):
            raise SystemExit('bubblewrap is required; do not run these fixtures against the host')
        unittest.main()
