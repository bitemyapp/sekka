#!/usr/bin/env python3
"""Search Tatami's launcher the way Walker does, through Elephant's own client.

Elephant runs with Tatami's desktopapplications.toml in a private directory,
over Sekka's first-party desktop entries and stand-ins for third-party ones,
never the host's. The unpatched package, with the same configuration, is the
negative control: it matches Exec lines and keywords joined into one string,
and replaces the generic name with whatever field matched.

usage: test_launcher_search.py ELEPHANT UNPATCHED-ELEPHANT CONFIG ENTRY... [-v]
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ELEPHANT = Path(sys.argv.pop(1))
UNPATCHED = Path(sys.argv.pop(1))
CONFIG = Path(sys.argv.pop(1))
ENTRIES = []
while len(sys.argv) > 1 and not sys.argv[1].startswith('-'):
    ENTRIES.append(Path(sys.argv.pop(1)))

# Third-party entries as Nixpkgs and Steam write them.
STAND_INS = {
    'org.gnome.Characters.desktop': '''\
[Desktop Entry]
Type=Application
Name=Characters
Comment=Character map application
Exec=/nix/store/00000000000000000000000000000000-gnome-characters-50.0/bin/gnome-characters
Keywords=characters;unicode;punctuation;math;letters;emoji;emoticon;symbols;
''',
    'Counter-Strike 2.desktop': '''\
[Desktop Entry]
Type=Application
Name=Counter-Strike 2
Comment=Play this game on Steam
Exec=steam steam://rungameid/730
''',
    'org.gnome.Contacts.desktop': '''\
[Desktop Entry]
Type=Application
Name=Contacts
Comment=A contacts manager for GNOME
Exec=gnome-contacts
Keywords=friends;address book;
''',
    'org.gnome.clocks.desktop': '''\
[Desktop Entry]
Type=Application
Name=Clocks
GenericName=Clocks
Comment=Clocks for world times, plus alarms, stopwatch and a timer
Exec=gnome-clocks
Keywords=time;timer;alarm;world clock;stopwatch;time zone;
''',
}


class Elephant:
    """Elephant's daemon with only the applications provider, its socket,
    cache and configuration in a temporary directory."""

    def __init__(self, package):
        self.tmp = tempfile.TemporaryDirectory(prefix='tatami-launcher-')
        root = Path(self.tmp.name)
        # The wrapper sets the provider directory; the providers it would
        # load besides this one need a Wayland session.
        self.binary = package / 'bin/.elephant-wrapped'
        providers = root / 'providers'
        providers.mkdir()
        (providers / 'desktopapplications.so').symlink_to(
            package / 'lib/elephant/providers/desktopapplications.so')
        config = root / 'config'
        config.mkdir()
        shutil.copy(CONFIG / 'desktopapplications.toml', config)
        applications = root / 'data/applications'
        applications.mkdir(parents=True)
        for entry in ENTRIES:
            shutil.copy(entry, applications)
        for name, text in STAND_INS.items():
            (applications / name).write_text(text)
        for name in ('home', 'cache', 'state', 'run'):
            (root / name).mkdir()
        (root / 'run').chmod(0o700)
        # A relative runtime directory keeps the socket path within the
        # 108-byte limit wherever the temporary directory is.
        self.cwd = root
        self.env = {
            'PATH': os.environ.get('PATH', ''),
            'HOME': str(root / 'home'),
            'XDG_CONFIG_HOME': str(root / 'home/.config'),
            'XDG_CACHE_HOME': str(root / 'cache'),
            'XDG_STATE_HOME': str(root / 'state'),
            'XDG_DATA_HOME': str(root / 'home/.local/share'),
            'XDG_DATA_DIRS': str(root / 'data'),
            'XDG_RUNTIME_DIR': 'run',
            'XDG_CURRENT_DESKTOP': 'Hyprland:Tatami',
            'ELEPHANT_PROVIDER_DIR': str(providers),
        }
        self.log = open(root / 'elephant.log', 'w')
        self.daemon = subprocess.Popen(
            [self.binary, '--config', config], cwd=self.cwd, env=self.env,
            stdout=self.log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 30
        while not self.query('Displays'):
            if self.daemon.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError('Elephant did not list applications:\n'
                                   + (root / 'elephant.log').read_text())
            time.sleep(0.1)

    def query(self, text):
        """[(name, subtext)], best first."""
        if not (self.cwd / 'run/elephant/elephant.sock').exists():
            return []
        out = subprocess.run(
            [self.binary, 'query', '--json', f'desktopapplications;{text};50;false'],
            cwd=self.cwd, env=self.env, capture_output=True, text=True, timeout=10)
        items = [json.loads(line).get('item') for line in out.stdout.splitlines()
                 if line.startswith('{')]
        items = sorted((i for i in items if i), key=lambda i: -i.get('score', 0))
        return [(i['text'], i.get('subtext', '')) for i in items]

    def close(self):
        self.daemon.terminate()
        self.daemon.wait(10)
        self.log.close()
        self.tmp.cleanup()


class Search(unittest.TestCase):
    package = ELEPHANT

    @classmethod
    def setUpClass(cls):
        cls.elephant = Elephant(cls.package)

    @classmethod
    def tearDownClass(cls):
        cls.elephant.close()

    def names(self, text):
        return [name for name, _ in self.elephant.query(text)]

    def first(self, text):
        results = self.elephant.query(text)
        self.assertTrue(results, f'nothing found for {text!r}')
        return results[0]


class Patched(Search):
    def test_keywords_find_yukimi(self):
        for text in ('pack', 'package', 'package manager', 'software center',
                     'app store', 'uninstall', 'store', 'nix'):
            with self.subTest(text=text):
                self.assertIn('Yukimi', self.names(text))
        for text in ('pack', 'package', 'package manager', 'uninstall'):
            with self.subTest(text=text):
                self.assertEqual(self.first(text)[0], 'Yukimi')

    def test_keywords_find_first_party_applications(self):
        for text, name in (('usb', 'Kakehashi'), ('etcher', 'Kakehashi'),
                           ('task manager', 'Activity'), ('htop', 'Activity'),
                           ('wifi', 'Network'), ('internet', 'Network'),
                           ('resolution', 'Displays'), ('hidpi', 'Displays')):
            with self.subTest(text=text):
                self.assertEqual(self.first(text)[0], name)

    def test_generic_name_is_the_subtext(self):
        # Whichever field matched: the name, a keyword or the generic name.
        for text, row in (('yukimi', ('Yukimi', 'Package Manager')),
                          ('pack', ('Yukimi', 'Package Manager')),
                          ('nixpkgs', ('Yukimi', 'Package Manager')),
                          ('usb', ('Kakehashi', 'USB Image Writer')),
                          ('task manager', ('Activity', 'System Monitor')),
                          ('wifi', ('Network', 'Network Settings')),
                          ('display settings', ('Displays', 'Display Settings'))):
            with self.subTest(text=text):
                self.assertEqual(self.first(text), row)

    def test_generic_name_repeating_the_name_is_left_out(self):
        self.assertEqual(self.first('clocks'), ('Clocks', ''))
        self.assertEqual(self.first('stopwatch'), ('Clocks', ''))

    def test_exec_lines_are_not_searched(self):
        for text in ('store', 'nix', 'gnome-characters'):
            with self.subTest(text=text):
                self.assertNotIn('Characters', self.names(text))
        self.assertNotIn('Counter-Strike 2', self.names('set'))
        self.assertNotIn('Counter-Strike 2', self.names('rungameid'))

    def test_keywords_match_one_at_a_time(self):
        # "fire" ran across "friends,address book".
        self.assertNotIn('Contacts', self.names('fire'))
        self.assertEqual(self.first('address book')[0], 'Contacts')


class Unpatched(Search):
    """The same queries against stock Elephant, showing that the checks
    above detect each problem the patch fixes."""
    package = UNPATCHED

    def test_exec_lines_are_searched(self):
        self.assertIn('Characters', self.names('store'))
        self.assertIn('Counter-Strike 2', self.names('set'))

    def test_keywords_are_joined(self):
        self.assertIn('Contacts', self.names('fire'))

    def test_matched_field_replaces_the_generic_name(self):
        name, subtext = self.first('nixpkgs')
        self.assertEqual(name, 'Yukimi')
        self.assertIn('nix,nixos,nixpkgs,', subtext)


if __name__ == '__main__':
    unittest.main()
