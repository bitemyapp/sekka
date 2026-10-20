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

FLAKEHUB = {'type': 'tarball', 'url': 'https://flakehub.com/f/NixOS/nixpkgs/0.1'}


def pinned(rev, count):
    return {'lastModified': 1790000000 + count, 'narHash': 'sha256-' + rev[:8], 'rev': rev, 'type': 'tarball',
            'url': f'https://api.flakehub.com/f/pinned/NixOS/nixpkgs/0.1.{count}%2Brev-{rev}/0199/source.tar.gz'}


# The system's lock before updating, a legacy installation's.
ORIGINAL = json.dumps({'nodes': {'nixpkgs': {'locked': pinned('1' * 40, 1), 'original': FLAKEHUB},
                                 'root': {'inputs': {'nixpkgs': 'nixpkgs'}}},
                       'root': 'root', 'version': 7}, indent=2) + '\n'
# What `nix flake update sekka` makes of it: the newest nightly Sekka.
UPDATED = json.dumps({'nodes': {'nixpkgs': {'locked': pinned('1' * 40, 1), 'original': FLAKEHUB},
                                'sekka': {'inputs': {'nixpkgs': ['nixpkgs']},
                                          'locked': {'lastModified': 1791999999, 'narHash': 'sha256-s', 'owner': 'bitemyapp',
                                                     'repo': 'sekka', 'rev': 'a' * 40, 'type': 'github'},
                                          'original': {'owner': 'bitemyapp', 'ref': 'nightly', 'repo': 'sekka', 'type': 'github'}},
                                'root': {'inputs': {'nixpkgs': 'nixpkgs', 'sekka': 'sekka'}}},
                      'root': 'root', 'version': 7}, indent=2) + '\n'
CACHES = {'substituters': ['https://cache.nixos.org/', 'https://cache.numtide.com?priority=55'],
          'keys': ['cache.nixos.org-1:x=', 'niks3.numtide.com-1:y=']}

FAKE = r'''
import json, os, pathlib, signal, subprocess, sys, time
p = pathlib.Path
name = p(sys.argv[0]).name
a = sys.argv[1:]
with open('/state/commands', 'a') as out: out.write(json.dumps([name] + a) + '\n')
if name == 'nix':
    if 'update' in a:
        assert 'sekka' in a, a
        # The other inputs follow Sekka's own and move with it.
        assert not {'nixpkgs', 'applications', 'ai-apps', 'omp'} & set(a), a
        assert 'inputs.sekka.url' in p('/etc/nixos/flake.nix').read_text()
        p('/etc/nixos/flake.lock').write_text(p('/state/updated.lock').read_text())
    elif 'eval' in a and any(x.endswith('.config.nix.settings') for x in a): print(p('/state/caches.json').read_text())
    elif 'eval' in a: print('["legacy-host"]')
    elif 'build' in a:
        p('/state/building').touch()
        if p('/state/fail').exists(): sys.exit(1)
        if p('/state/wait').exists(): time.sleep(60)
        print('/system-new')
    else: sys.exit(2)
elif name == 'nix-env':
    if p('/state/fail-profile').exists(): sys.exit(1)
    profile = p('/nix/var/nix/profiles/system')
    gen = a[-1] if '--switch-generation' in a else '2'
    profile.unlink()
    profile.symlink_to('system-' + gen + '-link')
elif name == 'systemd-run':
    assert '--pipe' not in a, a
    unit = a[a.index('--unit') + 1]
    command = a[a.index('__worker') - 1:]
    log = next(x.split('append:', 1)[1] for x in a if x.startswith('--property=StandardOutput='))
    with open(log, 'ab') as out:
        child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=out, stderr=out, start_new_session=True)
    p('/state/' + unit).write_text(str(child.pid))
elif name == 'systemctl':
    if '--user' in a:
        if 'wayland-wm@tatami.service' in a:
            print('ActiveState=active\nInvocationID=tatami-instance\nEnvironmentFiles=/run/user/1000/uwsm/env_session.conf (ignore_errors=yes)')
        else: print('ActiveState=inactive\nMainPID=0')
    elif 'kill' in a:
        os.kill(int(p('/state/' + a[-1]).read_text()), signal.SIGINT)
    elif 'show' in a:
        try:
            pid = int(p('/state/' + a[a.index('show') + 1]).read_text())
            state = p(f'/proc/{pid}/stat').read_text().split(') ')[1].split()[0]
            print('inactive' if state == 'Z' else 'active')
        except (OSError, ValueError): print('inactive')
    else: sys.exit(2)
elif name == 'loginctl':
    assert a == ['list-users', '--no-legend'], a
    if p('/state/users').exists(): print(p('/state/users').read_text(), end='')
elif name == 'switch-to-configuration':
    assert a[-1] in ['boot', 'switch'], a
    p('/state/activating').touch()
    if p('/state/slow-activation').exists():
        for _ in range(20):
            print('activation progress', file=sys.stderr, flush=True)
            time.sleep(.1)
    if p('/state/fail-activation').exists(): sys.exit(1)
    p('/state/activation').write_text(json.dumps([sys.argv[0], a[-1]]))
    if a[-1] == 'switch':
        current = p('/run/current-system')
        current.unlink()
        current.symlink_to(p(sys.argv[0]).parent.parent)
else: sys.exit(2)
'''


class Update(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='sekka-update-test-')
        self.root = Path(self.tmp.name)
        for name in ['etc/nixos', 'run/lock', 'system-old/sw/bin', 'system-new/sw/bin', 'artifacts',
                     'nix/store', 'nix/var/nix/profiles', 'var/lib', 'state', 'tmp', 'root', 'bin']:
            (self.root / name).mkdir(parents=True, exist_ok=True)
        self.config = self.root / 'etc/nixos'
        self.old = (REPO / 'apps/yukimi/crates/yukimi-config/tests/legacy-flake.nix').read_text()
        (self.config / 'flake.nix').write_text(self.old)
        (self.config / 'flake.lock').write_text(ORIGINAL)
        (self.root / 'state/updated.lock').write_text(UPDATED)
        (self.root / 'state/caches.json').write_text(json.dumps(CACHES))
        (self.config / 'configuration.nix').write_text('{ imports = [ ./hardware-configuration.nix ]; }')
        (self.config / 'hardware-configuration.nix').write_text('# hardware untouched')
        for name in ['nix', 'nix-env', 'systemd-run', 'systemctl', 'loginctl']:
            path = self.root / 'system-old/sw/bin' / name
            path.write_text(f'#!{PYTHON}\n' + FAKE)
            path.chmod(0o755)
            shutil.copy2(path, self.root / 'system-new/sw/bin' / name)
        (self.root / 'run/current-system').symlink_to('/system-old')
        (self.root / 'run/booted-system').symlink_to('/system-old')
        for part in ['kernel', 'initrd', 'kernel-modules']:
            (self.root / 'artifacts' / part).touch()
        for system in ['system-old', 'system-new']:
            base = self.root / system
            (base / 'etc/yukimi').mkdir(parents=True)
            (base / 'bin').mkdir()
            switch = base / 'bin/switch-to-configuration'
            switch.write_text(f'#!{PYTHON}\n' + FAKE)
            switch.chmod(0o755)
            (base / 'kernel-params').write_text('quiet')
            (base / 'init-interface-version').write_text('2')
            (base / 'switch-inhibitors').write_text('{}')
            for part in ['kernel', 'initrd', 'kernel-modules']:
                (base / part).symlink_to('/artifacts/' + part)
        (self.root / 'system-new/etc/yukimi/activation.json').write_text('{"version":1}')
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
        self.assertEqual((self.config / 'flake.lock').read_text(), ORIGINAL)
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
        self.assertIn('Restart to finish updating', result.stdout)
        migrated = (self.config / 'flake.nix').read_text()
        self.assertIn('inputs.sekka.url', migrated)
        self.assertEqual(json.loads((self.root / 'state/activation').read_text()),
                         ['/system-new/bin/switch-to-configuration', 'boot'])
        self.assertEqual((self.config / 'hardware-configuration.nix').read_text(), '# hardware untouched')
        self.compatible_live_system()
        self.run_cmd('/bin/sekka-update', '--switch')
        self.assertEqual((self.config / 'flake.nix').read_text(), migrated)
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'switch')
        self.run_cmd('/bin/yukimi-helper', 'rollback', '1')
        self.assertEqual((self.config / 'flake.nix').read_text(), self.old)
        self.assertEqual((self.config / 'flake.lock').read_text(), ORIGINAL)


    def compatible_live_system(self):
        (self.root / 'system-old/etc/yukimi/activation.json').write_text('{"version":1}')

    def test_safe_live_update_needs_no_reboot(self):
        self.compatible_live_system()
        result = self.run_cmd('/bin/sekka-update')
        self.assertIn('No computer restart is needed', result.stdout)
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'switch')
        self.assertEqual(json.loads((self.root / 'var/lib/yukimi/update.json').read_text())['state'], 'activated')

    def test_all_selected_sources_build_and_activate_once(self):
        self.compatible_live_system()
        result = self.run_cmd('/bin/yukimi-helper', 'change', '--sekka', '--update', 'sekka,nixpkgs,applications', '--mode', 'auto')
        self.assertEqual(json.loads(result.stdout)['completion']['action'], 'none')
        commands = [json.loads(line) for line in (self.root / 'state/commands').read_text().splitlines()]
        self.assertEqual(sum(command[0] == 'nix' and 'build' in command for command in commands), 1)
        self.assertEqual(sum(command[0] == 'systemd-run' for command in commands), 1)

    def test_sekka_brings_what_it_pins_and_the_new_systems_caches(self):
        self.compatible_live_system()
        self.run_cmd('/bin/yukimi-helper', 'change', '--sekka', '--update', 'sekka,nixpkgs,applications,ai-apps', '--mode', 'auto')
        flake = (self.config / 'flake.nix').read_text()
        self.assertIn('"github:bitemyapp/sekka/nightly"', flake)
        for name in ['nixpkgs', 'determinate', 'fh', 'applications', 'ai-apps', 'omp']:
            self.assertIn(f'inputs.{name}.follows = "sekka/{name}";', flake)
        commands = [json.loads(line) for line in (self.root / 'state/commands').read_text().splitlines()]
        update = next(command for command in commands if command[0] == 'nix' and 'update' in command)
        self.assertEqual(update[-1], 'sekka')
        build = next(command for command in commands if command[0] == 'nix' and 'build' in command)
        option = lambda name: build[build.index(name) + 1]
        self.assertEqual(option('extra-substituters').split(), CACHES['substituters'])
        self.assertEqual(option('extra-trusted-public-keys').split(), CACHES['keys'])

    def test_a_nixpkgs_of_ones_own_is_kept(self):
        self.compatible_live_system()
        own = self.old.replace('https://flakehub.com/f/NixOS/nixpkgs/0.1', 'github:NixOS/nixpkgs/nixos-25.05')
        (self.config / 'flake.nix').write_text(own)
        self.run_cmd('/bin/yukimi-helper', 'change', '--sekka', '--mode', 'auto')
        flake = (self.config / 'flake.nix').read_text()
        self.assertIn('inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";', flake)
        self.assertIn('inputs.sekka.inputs.nixpkgs.follows = "nixpkgs";', flake)
        self.assertIn('inputs.applications.follows = "sekka/applications";', flake)

    def declare_choices(self):
        # What the system's Nix module declares (programs.yukimi.choices).
        (self.root / 'etc/yukimi').mkdir(exist_ok=True)
        (self.root / 'etc/yukimi/system.json').write_text(json.dumps({'choices': [
            {'setting': 'calamares.loginScreen', 'title': 'Login screen', 'value': 'gdm',
             'options': [{'id': 'aizome', 'title': 'Aizome'}, {'id': 'gdm', 'title': 'GNOME'}]},
            {'setting': 'calamares.desktops', 'title': 'Desktops', 'many': True, 'value': ['tatami'],
             'options': [{'id': 'aizome', 'title': 'Aizome'}, {'id': 'tatami', 'title': 'Tatami'}]}]}))

    def test_early_updates_and_declared_choices_in_one_update(self):
        self.declare_choices()
        self.compatible_live_system()
        self.run_cmd('/bin/sekka-update', '--early', '--choose', 'calamares.loginScreen=aizome',
                     '--choose', 'calamares.desktops=aizome,tatami')
        self.assertIn('inputs.sekka.url = "github:bitemyapp/sekka?ref=main";', (self.config / 'flake.nix').read_text())
        configuration = (self.config / 'configuration.nix').read_text()
        self.assertIn('calamares.loginScreen = "aizome";', configuration)
        self.assertIn('"aizome"', configuration.split('calamares.desktops', 1)[1].split(';', 1)[0])
        self.assertIn('./hardware-configuration.nix', configuration)
        commands = [json.loads(line) for line in (self.root / 'state/commands').read_text().splitlines()]
        self.assertEqual(sum(command[0] == 'nix' and 'build' in command for command in commands), 1)
        # Early updates stay chosen, and --nightly goes back.
        self.run_cmd('/bin/sekka-update')
        self.assertIn('"github:bitemyapp/sekka?ref=main"', (self.config / 'flake.nix').read_text())
        self.run_cmd('/bin/sekka-update', '--nightly')
        self.assertIn('"github:bitemyapp/sekka/nightly"', (self.config / 'flake.nix').read_text())

    def test_only_declared_choices_and_values_are_written(self):
        self.declare_choices()
        for choice in ['services.openssh.enable=true', 'calamares.loginScreen=lightdm',
                       'calamares.loginScreen=aizome,gdm', 'calamares.desktops=aizome,aizome',
                       'calamares.loginScreen=$(reboot)']:
            result = self.run_cmd('/bin/sekka-update', '--choose', choice, ok=False)
            self.assertRegex(result.stderr, 'choice|one value|chosen twice|valid')
        self.run_cmd('/bin/sekka-update', '--early', '--nightly', ok=False)
        self.unchanged()
        self.assertFalse((self.root / 'state/building').exists())

    def test_no_update_without_room_for_it(self):
        self.compatible_live_system()
        tiny = self.base[:self.base.index('--proc')] + ['--size', '1048576', '--tmpfs', '/tmp'] + self.base[self.base.index('--proc'):]
        result = subprocess.run(tiny + ['/bin/sekka-update'], capture_output=True, text=True, timeout=30)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('updating needs 10.0 GB', result.stdout + result.stderr)
        self.unchanged()
        self.assertFalse((self.root / 'state/building').exists())

    def launcher_restarts(self):
        commands = [json.loads(line) for line in (self.root / 'state/commands').read_text().splitlines()]
        return [command for command in commands if 'tatami-elephant.service' in command]

    def applications(self, old, new):
        # Each system's application folder holds links to entry files, as
        # NixOS builds it; `old` and `new` name the entries each has.
        (self.root / 'state/users').write_text('1000 alice no active\n1001 bob yes active\n')
        (self.root / 'entries').mkdir(exist_ok=True)
        for system, names in [('system-old', old), ('system-new', new)]:
            folder = self.root / system / 'sw/share/applications'
            folder.mkdir(parents=True)
            for name in names:
                (self.root / 'entries' / name).write_text('[Desktop Entry]\n')
                (folder / name).symlink_to('/entries/' + name)
            # Generated in every new folder, so never the same file.
            (folder / 'mimeinfo.cache').write_text(system)

    def test_new_applications_refresh_each_users_launcher(self):
        self.compatible_live_system()
        self.applications(['firefox.desktop'], ['firefox.desktop', 'discord.desktop'])
        self.run_cmd('/bin/sekka-update')
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'switch')
        self.assertEqual(self.launcher_restarts(), [
            ['systemctl', '--user', f'--machine={user}@.host', 'try-restart', 'tatami-elephant.service']
            for user in ['alice', 'bob']])

    def test_same_applications_leave_the_launcher_alone(self):
        # Another package changed, so the folder is a new one, but every
        # entry in it is the same file.
        self.compatible_live_system()
        self.applications(['firefox.desktop'], ['firefox.desktop'])
        self.run_cmd('/bin/sekka-update')
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'switch')
        self.assertEqual(self.launcher_restarts(), [])

    def test_an_update_for_the_next_start_leaves_the_launcher_alone(self):
        self.applications(['firefox.desktop'], ['firefox.desktop', 'discord.desktop'])
        self.run_cmd('/bin/sekka-update')
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'boot')
        self.assertEqual(self.launcher_restarts(), [])

    def test_kernel_update_prepares_once_then_reboot_clears_request(self):
        self.compatible_live_system()
        (self.root / 'system-new/kernel-params').write_text('quiet new-setting=1')
        result = self.run_cmd('/bin/sekka-update')
        self.assertIn('Restart to finish updating', result.stdout)
        self.assertEqual(json.loads((self.root / 'state/activation').read_text())[-1], 'boot')
        for name in ['current-system', 'booted-system']:
            (self.root / 'run' / name).unlink()
            (self.root / 'run' / name).symlink_to('/system-new')
        # Repeating the update after a successful boot must not ask again.
        result = self.run_cmd('/bin/sekka-update')
        self.assertIn('No computer restart is needed', result.stdout)
        self.assertNotIn('Restart to finish', result.stdout)

    def test_activation_failure_leaves_an_unverified_receipt(self):
        (self.root / 'state/fail-activation').touch()
        self.run_cmd('/bin/sekka-update', ok=False)
        receipt = json.loads((self.root / 'var/lib/yukimi/update.json').read_text())
        self.assertEqual(receipt['state'], 'activating')

    def test_failure_before_profile_change_is_not_reported_as_applied(self):
        (self.root / 'state/fail-profile').touch()
        self.run_cmd('/bin/sekka-update', ok=False)
        result = self.run_cmd('/bin/sekka-update', '--status', '--json')
        self.assertEqual(json.loads(result.stdout)['action'], 'unknown')

    def test_reboot_supersedes_unverified_desktop_without_another_request(self):
        result = self.run_cmd('--setenv', 'XDG_CURRENT_DESKTOP', 'Tatami', '/bin/sekka-update')
        self.assertIn('Restart to finish updating', result.stdout)
        result = self.run_cmd('--setenv', 'XDG_CURRENT_DESKTOP', 'Tatami', '/bin/sekka-update', '--status', '--json')
        self.assertEqual(json.loads(result.stdout)['action'], 'reboot')

    def test_explicit_switch_cannot_bypass_session_safety(self):
        result = self.run_cmd('/bin/sekka-update', '--switch', ok=False)
        self.assertIn('Live activation was refused', result.stderr)
        self.assertFalse((self.root / 'state/activation').exists())
        self.assertEqual((self.root / 'nix/var/nix/profiles/system').readlink(), Path('system-1-link'))
        self.unchanged()

    def test_rollback_stages_a_disruptive_transition(self):
        self.compatible_live_system()
        self.run_cmd('/bin/sekka-update')
        (self.root / 'system-old/etc/yukimi/activation.json').write_text('{"version":2}')
        result = self.run_cmd('/bin/yukimi-helper', 'rollback', '1')
        self.assertEqual(json.loads(result.stdout)['mode'], 'boot')
        self.assertEqual((self.root / 'run/current-system').readlink(), Path('/system-new'))

    def test_target_precheck_protects_legacy_live_activation(self):
        result = self.run_cmd('/bin/yukimi-helper', 'check-activation', '/system-new', 'switch', ok=False)
        self.assertIn('Live activation was refused', result.stderr)
        self.run_cmd('/bin/yukimi-helper', 'check-activation', '/system-new', 'boot')
        self.unchanged()

    def test_closed_frontend_pipes_do_not_interrupt_committed_activation(self):
        self.compatible_live_system()
        (self.root / 'state/slow-activation').touch()
        process = subprocess.Popen(self.base + ['/bin/yukimi-helper', 'change', '--sekka'],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for _ in range(200):
                if (self.root / 'state/activating').exists(): break
                time.sleep(.05)
            self.assertTrue((self.root / 'state/activating').exists())
            process.stdout.close()
            process.stderr.close()
            process.stdin.close()
            self.assertEqual(process.wait(timeout=10), 0)
            self.assertEqual(json.loads((self.root / 'var/lib/yukimi/update.json').read_text())['state'], 'activated')
            logs = list((self.root / 'var/lib/yukimi/operations').glob('*/progress.log'))
            self.assertTrue(any('activation progress' in log.read_text() for log in logs))
        finally:
            if process.poll() is None: process.kill()
            process.wait()

    def session_fixture(self):
        self.compatible_live_system()
        self.run_cmd('/bin/sekka-update')
        manifest = {'version': 1, 'session_id': '3', 'session_invocation': 'tatami-instance',
                    'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
                    'session': {}, 'reload': {}, 'keep_awake': 'tool:systemd'}
        runtime = self.root / 'run/user/1000'
        (runtime / 'tatami').mkdir(parents=True)
        (runtime / 'uwsm').mkdir()
        (runtime / 'tatami/update-session.json').write_text(json.dumps(manifest))
        (runtime / 'uwsm/env_session.conf').write_text('XDG_SESSION_ID=3\n')
        target = self.root / 'system-new/etc/xdg/tatami'
        target.mkdir(parents=True)
        (target / 'update.json').write_text(json.dumps(manifest))
        self.base += ['--setenv', 'XDG_RUNTIME_DIR', '/run/user/1000', '--setenv', 'XDG_CURRENT_DESKTOP', 'Tatami']
        return runtime

    def test_uwsm_launched_application_without_session_id_is_verified(self):
        self.session_fixture()
        result = self.run_cmd('/bin/sekka-update', '--status', '--json')
        self.assertEqual(json.loads(result.stdout)['action'], 'none')

    def test_different_session_and_stale_invocation_are_not_accepted(self):
        runtime = self.session_fixture()
        result = self.run_cmd('--setenv', 'XDG_SESSION_ID', '99', '/bin/sekka-update', '--status', '--json')
        self.assertEqual(json.loads(result.stdout)['action'], 'unknown')
        file = runtime / 'tatami/update-session.json'
        manifest = json.loads(file.read_text())
        manifest['session_invocation'] = 'earlier-session'
        file.write_text(json.dumps(manifest))
        result = self.run_cmd('/bin/sekka-update', '--status', '--json')
        self.assertEqual(json.loads(result.stdout)['action'], 'unknown')

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
