"""Sekka's agent skills: their format, the command that keeps them in a home,
and the skills' scripts against a fake /etc/nixos, Nix flake, pkexec and
Yukimi helper. Plain Nix expressions go to a real nix (REAL_NIX, or nix on
PATH); nothing here asks for root or touches the running system."""
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SKILLS = ROOT / 'agent-skills/skills'
COMMAND = os.environ.get('SEKKA_AGENT_SKILLS_COMMAND')
REAL_NIX = os.environ.get('REAL_NIX') or shutil.which('nix')


def frontmatter(path):
    text = path.read_text()
    match = re.match(r'---\n(.*?)\n---\n', text, re.S)
    assert match, f'{path} has no frontmatter'
    fields = {}
    for line in match.group(1).splitlines():
        key, _, value = line.partition(':')
        fields[key.strip()] = value.strip()
    return fields, text[match.end():]


class Format(unittest.TestCase):
    def test_skills_follow_the_agent_skills_format(self):
        names = sorted(p.name for p in SKILLS.iterdir())
        self.assertEqual(names, ['aizome', 'sekka', 'tatami', 'yukimi'])
        for name in names:
            fields, body = frontmatter(SKILLS / name / 'SKILL.md')
            self.assertEqual(set(fields), {'name', 'description'}, name)
            # agentskills.io: name matches the folder, a-z0-9 and single
            # hyphens, at most 64; description 1-1024 characters.
            self.assertEqual(fields['name'], name)
            self.assertRegex(name, r'^[a-z0-9]+(-[a-z0-9]+)*$')
            self.assertLessEqual(len(name), 64)
            self.assertTrue(0 < len(fields['description']) <= 1024, name)
            self.assertLess(len(body.splitlines()), 500, name)
            for script in re.findall(r'`(scripts/[a-z-]+)', body):
                path = SKILLS / name / script
                self.assertTrue(path.is_file(), path)
                self.assertTrue(os.access(path, os.X_OK), path)


class Command(unittest.TestCase):
    """sekka-agent-skills in a temporary home."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        root = Path(self.directory.name)
        self.home = root / 'home'
        self.home.mkdir()
        self.shipped = root / 'shipped'
        for name in ['alpha', 'beta']:
            (self.shipped / name).mkdir(parents=True)
            (self.shipped / name / 'SKILL.md').write_text(f'---\nname: {name}\ndescription: {name}\n---\n\nFirst.\n')
        script = self.shipped / 'beta/scripts/run'
        script.parent.mkdir()
        script.write_text('#!/bin/sh\n')
        script.chmod(0o755)
        self.freeze()
        self.agents = self.home / '.agents/skills'
        self.claude = self.home / '.claude/skills'
        self.state = self.home / '.local/state/sekka/agent-skills'

    def tearDown(self):
        for path in [self.directory.name, *Path(self.directory.name).rglob('*')]:
            if Path(path).is_dir() and not Path(path).is_symlink():
                Path(path).chmod(0o755)
        self.directory.cleanup()

    def freeze(self, writable=False):
        """Make the shipped skills read-only, as they are in the store."""
        for path in [self.shipped, *self.shipped.rglob('*')]:
            mode = path.stat().st_mode
            if writable:
                path.chmod(mode | stat.S_IWUSR)
            else:
                path.chmod(mode & ~(stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH))

    def ship(self, name, text):
        self.freeze(writable=True)
        (self.shipped / name / 'SKILL.md').write_text(text)
        self.freeze()

    def run_command(self, *args, check=True, **env):
        command = [COMMAND] if COMMAND else ['bash', str(ROOT / 'agent-skills/sekka-agent-skills')]
        environment = {
            'PATH': os.environ['PATH'],
            'HOME': str(self.home),
            'SEKKA_AGENT_SKILLS': str(self.shipped),
            **env,
        }
        return subprocess.run(command + list(args), env=environment, check=check, capture_output=True, text=True)

    def test_installs_writable_copies_and_links_them_for_claude(self):
        self.run_command('sync')
        skill = self.agents / 'alpha/SKILL.md'
        self.assertTrue(skill.is_file() and not skill.is_symlink())
        self.assertTrue(os.access(skill, os.W_OK))
        self.assertTrue(os.access(self.agents / 'beta/scripts/run', os.X_OK))
        for name in ['alpha', 'beta']:
            self.assertTrue((self.claude / name).is_symlink())
            self.assertEqual(os.readlink(self.claude / name), str(self.agents / name))
        self.assertIn("alpha      Sekka's current version, linked for Claude", self.run_command('status').stdout)

    def test_unchanged_copies_follow_sekka(self):
        self.run_command('sync')
        self.ship('alpha', 'Second.\n')
        self.run_command('sync')
        self.assertEqual((self.agents / 'alpha/SKILL.md').read_text(), 'Second.\n')

    def test_changed_copies_stay_until_restored(self):
        self.run_command('sync')
        (self.agents / 'alpha/SKILL.md').write_text('Mine.\n')
        self.ship('alpha', 'Second.\n')
        self.run_command('sync')
        self.assertEqual((self.agents / 'alpha/SKILL.md').read_text(), 'Mine.\n')
        self.assertIn('alpha      changed', self.run_command('status').stdout)
        diff = self.run_command('diff', 'alpha').stdout
        self.assertIn('-Second.', diff)
        self.assertIn('+Mine.', diff)
        restored = self.run_command('restore', 'alpha').stdout
        self.assertEqual((self.agents / 'alpha/SKILL.md').read_text(), 'Second.\n')
        kept = list(self.state.glob('backups/*/alpha/SKILL.md'))
        self.assertEqual([p.read_text() for p in kept], ['Mine.\n'])
        self.assertIn(str(kept[0].parent), restored)
        # Restored, it follows Sekka again.
        self.ship('alpha', 'Third.\n')
        self.run_command('sync')
        self.assertEqual((self.agents / 'alpha/SKILL.md').read_text(), 'Third.\n')

    def test_removed_skills_stay_removed_until_restored(self):
        self.run_command('sync')
        shutil.rmtree(self.agents / 'beta')
        self.ship('beta', 'Second.\n')
        self.run_command('sync')
        self.assertFalse((self.agents / 'beta').exists())
        self.assertFalse((self.claude / 'beta').is_symlink())
        self.assertIn('beta       removed', self.run_command('status').stdout)
        self.run_command('restore')
        self.assertEqual((self.agents / 'beta/SKILL.md').read_text(), 'Second.\n')
        self.assertTrue((self.claude / 'beta').is_symlink())
        self.assertEqual(list(self.state.glob('backups/*')), [])

    def test_a_removed_claude_link_stays_removed(self):
        self.run_command('sync')
        (self.claude / 'alpha').unlink()
        self.ship('alpha', 'Second.\n')
        self.run_command('sync')
        self.assertFalse((self.claude / 'alpha').exists())
        self.assertEqual((self.agents / 'alpha/SKILL.md').read_text(), 'Second.\n')
        self.assertIn('not linked for Claude', self.run_command('status').stdout)

    def test_other_skills_of_the_same_name_are_left_alone(self):
        (self.agents / 'alpha').mkdir(parents=True)
        (self.agents / 'alpha/SKILL.md').write_text('Theirs.\n')
        (self.claude / 'beta').mkdir(parents=True)
        (self.claude / 'beta/SKILL.md').write_text('Theirs too.\n')
        self.run_command('sync')
        self.assertEqual((self.agents / 'alpha/SKILL.md').read_text(), 'Theirs.\n')
        self.assertFalse((self.claude / 'alpha').exists())
        self.assertEqual((self.claude / 'beta/SKILL.md').read_text(), 'Theirs too.\n')
        status = self.run_command('status').stdout
        self.assertIn("someone else's skill", status)
        self.assertIn("is not Sekka's link", status)
        self.run_command('restore', 'alpha', 'beta')
        self.assertTrue((self.claude / 'beta').is_symlink())
        self.assertEqual(len(list(self.state.glob('backups/*/alpha/SKILL.md'))), 1)
        self.assertEqual(len(list(self.state.glob('backups/*/claude/beta/SKILL.md'))), 1)

    def test_skills_sekka_drops_go_unless_changed(self):
        self.run_command('sync')
        (self.agents / 'beta/SKILL.md').write_text('Mine.\n')
        self.freeze(writable=True)
        shutil.rmtree(self.shipped / 'alpha')
        shutil.rmtree(self.shipped / 'beta')
        (self.shipped / 'gamma').mkdir()
        (self.shipped / 'gamma/SKILL.md').write_text('Gamma.\n')
        self.freeze()
        self.run_command('sync')
        self.assertFalse((self.agents / 'alpha').exists())
        self.assertFalse((self.claude / 'alpha').is_symlink())
        self.assertEqual((self.agents / 'beta/SKILL.md').read_text(), 'Mine.\n')
        self.assertIn('beta       no longer part of Sekka', self.run_command('status').stdout)

    def test_claude_config_dir_moves_the_links(self):
        self.run_command('sync', CLAUDE_CONFIG_DIR=str(self.home / 'claude-config'))
        self.assertTrue((self.home / 'claude-config/skills/alpha').is_symlink())
        self.assertFalse((self.claude / 'alpha').exists())

    def test_nothing_is_removed_without_sekkas_skills(self):
        self.run_command('sync')
        result = self.run_command('sync', check=False, SEKKA_AGENT_SKILLS=str(self.home / 'missing'))
        self.assertEqual(result.returncode, 1)
        self.assertTrue((self.agents / 'alpha/SKILL.md').is_file())
        result = self.run_command('restore', 'nonexistent', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('no skill named nonexistent', result.stderr)


FAKE_NIX = r'''#!@bash@
# The skills' Nix calls: plain expressions go to the real nix; the system
# flake is the fixture's files.
set -eu
fixture=@fixture@
case "$*" in
  *--expr*) exec @real@ "$@" ;;
esac
if [ "$1" = build ]; then
  printf '%s\n' "$*" >>"$fixture/builds"
  if [ -e "$fixture/build-fails" ]; then
    echo "error: builder for nixos-system failed" >&2
    exit 1
  fi
  echo /nix/store/00000000000000000000000000000000-nixos-system-machine
  exit 0
fi
ref= apply='x: x'
shift
while [ $# -gt 0 ]; do
  case $1 in
    --apply) apply=$2; shift 2 ;;
    --raw) shift ;;
    *) ref=$1; shift ;;
  esac
done
attr=${ref#*#}
case $attr in
  nixosConfigurations) value="import $fixture/configurations.nix" ;;
  *.config.calamares.applications) value="import $fixture/applications.nix" ;;
  *.pkgs.*.drvPath)
    name=${attr#*.pkgs.}
    name=${name%.drvPath}
    if grep -qxF "$name" "$fixture/available"; then
      printf '/nix/store/00000000000000000000000000000000-%s.drv' "$name"
      exit 0
    fi
    echo "error: attribute '$name' missing" >&2
    exit 1
    ;;
  *) echo "fake nix: unexpected $attr" >&2; exit 1 ;;
esac
exec @real@ eval --raw --impure --expr "($apply) ($value)"
'''

FAKE_PKEXEC = r'''#!@bash@
set -eu
fixture=@fixture@
[ "$1" = --disable-internal-agent ] || { echo "fake pkexec: no --disable-internal-agent" >&2; exit 127; }
shift
[ ! -p /dev/stdin ] || { echo "fake pkexec: input is a pipe" >&2; exit 127; }
printf '%s\n' "$1" >>"$fixture/pkexec"
[ ! -e "$fixture/dismiss" ] || exit 126
exec "$@"
'''

FAKE_HELPER = r'''#!@bash@
fixture=@fixture@
printf '%s\n' "$*" >>"$fixture/helper"
cp "$fixture/nixos/configuration.nix" "$fixture/seen.nix"
echo '@nix {"action":"start","fields":[],"id":3,"level":0,"text":"Evaluating your configuration","type":0}' >&2
echo '@nix {"action":"start","id":9,"level":3,"parent":0,"text":"building noise","type":105}' >&2
echo '@nix {"action":"stop","id":3}' >&2
if [ -e "$fixture/helper-fails" ]; then
  message=$(cat "$fixture/helper-fails")
  echo "@nix {\"action\":\"msg\",\"level\":0,\"msg\":\"error: $message\\nsecond line\"}" >&2
  echo "{\"ok\":false,\"error\":\"$message\"}"
  exit 1
fi
echo '{"ok":true,"system":"/nix/store/x","mode":"switch"}'
'''

# As Yukimi writes it (yukimi-config's packages::render).
YUKIMI_NIX = '''# Packages and programs for everyone on this computer, added with Yukimi.
# Yukimi rewrites this file when they are added or removed there.
{ lib, pkgs, ... }:
let
  # Attribute names in Nixpkgs, the names `nix search` shows.
  packages = [
    "ripgrep"
    "python3Packages.requests"
  ];
  # NixOS programs, each switched on with programs.<name>.enable.
  programs = [ "steam" ];
in
{
  environment.systemPackages = map (name: lib.getAttrFromPath (lib.splitString "." name) pkgs) packages;
  programs = lib.genAttrs programs (_: { enable = true; });
}
'''


@unittest.skipUnless(REAL_NIX, 'needs nix for the scripts\' expressions')
class Scripts(unittest.TestCase):
    """The skills' scripts, with the root side and the system flake faked."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.fixture = Path(self.directory.name)
        bin = self.fixture / 'bin'
        bin.mkdir()
        fakes = {'nix': FAKE_NIX, 'pkexec': FAKE_PKEXEC, 'yukimi-helper': FAKE_HELPER,
                 'sekka-update': '#!/bin/sh\necho "Up to date."\n'}
        for name, text in fakes.items():
            path = bin / name
            path.write_text(text.replace('@fixture@', str(self.fixture)).replace('@real@', REAL_NIX)
                            .replace('@bash@', shutil.which('bash')))
            path.chmod(0o755)
        self.nixos = self.fixture / 'nixos'
        self.nixos.mkdir()
        (self.nixos / 'flake.nix').write_text('{ outputs = _: { }; }\n')
        self.configuration = '{ ... }: {\n  imports = [ ./yukimi.nix ];\n  calamares.applications = [ "codex" "fish" ];\n}\n'
        (self.nixos / 'configuration.nix').write_text(self.configuration)
        (self.nixos / 'yukimi.nix').write_text(YUKIMI_NIX)
        (self.fixture / 'configurations.nix').write_text('{ machine = { }; }\n')
        (self.fixture / 'applications.nix').write_text('[ "codex" "fish" ]\n')
        (self.fixture / 'available').write_text('ripgrep\nhello\njq\npython3Packages.requests\n')
        (self.fixture / 'applications.json').write_text(json.dumps([
            {'id': 'codex', 'name': 'Codex', 'description': 'An agent', 'category': 'AI', 'packages': ['codex']},
            {'id': 'fish', 'name': 'fish', 'description': 'A shell', 'category': 'Tools', 'packages': ['fish']},
            {'id': 'docker', 'name': 'Docker', 'description': 'Containers', 'category': 'Tools', 'packages': ['docker']},
        ]))
        (self.fixture / 'system.json').write_text(json.dumps({'catalogs': [
            {'file': str(self.fixture / 'applications.json'), 'setting': 'calamares.applications', 'title': 'Apps'},
            {'file': str(self.fixture / 'other.json'), 'title': 'Not a setting'},
        ]}))

    def tearDown(self):
        self.directory.cleanup()

    def run_script(self, skill, script, *args):
        environment = {
            'PATH': f"{self.fixture / 'bin'}:{os.environ['PATH']}",
            'HOME': str(self.fixture),
            'TMPDIR': str(self.fixture),
            'NIX_CONFIG': 'experimental-features = nix-command flakes\nstore = dummy://',
            'SEKKA_NIXOS': str(self.nixos),
            'SEKKA_YUKIMI_FACTS': str(self.fixture / 'system.json'),
            'YUKIMI_HELPER': str(self.fixture / 'bin/yukimi-helper'),
        }
        # An agent's tool runs commands with a pipe for input, which must
        # never reach the helper.
        return subprocess.run([str(SKILLS / skill / 'scripts' / script), *args], env=environment,
                              stdin=subprocess.PIPE, capture_output=True, text=True)

    def calls(self, name):
        path = self.fixture / name
        return path.read_text().splitlines() if path.exists() else []

    def test_lists_what_yukimi_and_the_catalog_installed(self):
        result = self.run_script('yukimi', 'software', 'list')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Packages for everyone (yukimi.nix): ripgrep python3Packages.requests', result.stdout)
        self.assertIn('NixOS programs (yukimi.nix): steam', result.stdout)
        self.assertIn('Catalog applications (calamares.applications): codex fish', result.stdout)
        catalog = self.run_script('yukimi', 'software', 'catalog').stdout
        self.assertEqual(catalog.splitlines()[2], 'docker\tDocker\tContainers')

    def test_adding_a_package_keeps_the_others(self):
        result = self.run_script('yukimi', 'software', 'add', 'jq', 'ripgrep', 'hello')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls('helper'),
                         ['change --packages ripgrep,python3Packages.requests,jq,hello --mode auto'])
        self.assertEqual(self.calls('pkexec'), [str(self.fixture / 'bin/yukimi-helper')])
        self.assertIn('Evaluating your configuration', result.stderr)
        self.assertNotIn('building noise', result.stderr)
        self.assertNotIn('@nix', result.stderr)
        self.assertIn('Up to date.', result.stdout)

    def test_adding_without_yukimi_nix_starts_its_list(self):
        (self.nixos / 'yukimi.nix').unlink()
        self.run_script('yukimi', 'software', 'add', 'hello')
        self.assertEqual(self.calls('helper'), ['change --packages hello --mode auto'])

    def test_installed_or_unknown_packages_ask_for_nothing(self):
        result = self.run_script('yukimi', 'software', 'add', 'ripgrep')
        self.assertEqual(result.returncode, 0)
        self.assertIn('Already installed', result.stdout)
        result = self.run_script('yukimi', 'software', 'add', 'nonexistent')
        self.assertEqual(result.returncode, 1)
        self.assertIn("nonexistent can't be installed", result.stderr)
        result = self.run_script('yukimi', 'software', 'add', 'a;b')
        self.assertEqual(result.returncode, 2)
        result = self.run_script('yukimi', 'software', 'remove', 'hello')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(self.calls('pkexec'), [])

    def test_removing_a_package_keeps_the_others(self):
        self.run_script('yukimi', 'software', 'remove', 'ripgrep')
        self.assertEqual(self.calls('helper'), ['change --packages python3Packages.requests --mode auto'])

    def test_unreadable_yukimi_nix_changes_nothing(self):
        (self.nixos / 'yukimi.nix').write_text('{ broken')
        result = self.run_script('yukimi', 'software', 'add', 'hello')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls('pkexec'), [])

    def test_catalog_applications_keep_the_others(self):
        self.run_script('yukimi', 'software', 'add-app', 'docker')
        self.run_script('yukimi', 'software', 'remove-app', 'codex')
        self.assertEqual(self.calls('helper'), [
            'change --setting calamares.applications --applications codex,fish,docker --mode auto',
            'change --setting calamares.applications --applications fish --mode auto',
        ])
        result = self.run_script('yukimi', 'software', 'add-app', 'steam')
        self.assertEqual(result.returncode, 1)
        self.assertIn("steam isn't in the catalog", result.stderr)
        self.assertIn('Already installed', self.run_script('yukimi', 'software', 'add-app', 'fish').stdout)

    def test_a_dismissed_dialog_changes_nothing(self):
        (self.fixture / 'dismiss').touch()
        result = self.run_script('yukimi', 'software', 'add', 'hello')
        self.assertEqual(result.returncode, 126)
        self.assertIn('dismissed', result.stderr)
        self.assertEqual(self.calls('helper'), [])
        new = self.fixture / 'new.nix'
        new.write_text('{ ... }: { services.openssh.enable = true; }\n')
        result = self.run_script('sekka', 'apply-configuration', f'configuration.nix={new}')
        self.assertEqual(result.returncode, 126)
        self.assertEqual((self.nixos / 'configuration.nix').read_text(), self.configuration)

    def test_applying_a_configuration_builds_first_then_installs_it(self):
        new = self.fixture / 'edit/configuration.nix'
        new.parent.mkdir()
        new.write_text(self.configuration.replace('}\n', '  services.openssh.enable = true;\n}\n'))
        module = self.fixture / 'edit/extra.nix'
        module.write_text('{ ... }: { }\n')
        result = self.run_script('sekka', 'apply-configuration', f'configuration.nix={new}', f'modules/extra.nix={module}')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('+  services.openssh.enable = true;', result.stdout)
        self.assertIn('+++ b/modules/extra.nix', result.stdout)
        [build] = self.calls('builds')
        self.assertIn('#nixosConfigurations."machine".config.system.build.toplevel', build)
        # The dialog shows this command line, so it names a script, not a
        # shell program.
        self.assertEqual(self.calls('pkexec'), [str(SKILLS / 'sekka/scripts/install-configuration')])
        self.assertEqual(self.calls('helper'), ['change --mode auto'])
        self.assertEqual((self.fixture / 'seen.nix').read_text(), new.read_text())
        self.assertEqual((self.nixos / 'configuration.nix').read_text(), new.read_text())
        self.assertTrue((self.nixos / 'modules/extra.nix').is_file())
        self.assertIn('{"ok":true', result.stdout)
        self.assertIn('Evaluating your configuration', result.stderr)

    def test_a_configuration_that_does_not_build_is_never_installed(self):
        (self.fixture / 'build-fails').touch()
        new = self.fixture / 'new.nix'
        new.write_text('{ ... }: { }\n')
        result = self.run_script('sekka', 'apply-configuration', f'configuration.nix={new}')
        self.assertEqual(result.returncode, 1)
        self.assertIn("doesn't build; nothing was changed", result.stderr)
        self.assertIn('builder for nixos-system failed', result.stderr)
        self.assertEqual(self.calls('pkexec'), [])
        result = self.run_script('sekka', 'apply-configuration', '--check')
        self.assertEqual(result.returncode, 1)

    def test_check_only_builds(self):
        result = self.run_script('sekka', 'apply-configuration', '--check')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls('builds')), 1)
        self.assertEqual(self.calls('pkexec'), [])

    def test_failed_changes_put_the_files_back(self):
        (self.fixture / 'helper-fails').write_text('Building the new system failed')
        new = self.fixture / 'new.nix'
        new.write_text('{ ... }: { services.openssh.enable = true; }\n')
        module = self.fixture / 'extra.nix'
        module.write_text('{ ... }: { }\n')
        result = self.run_script('sekka', 'apply-configuration', f'configuration.nix={new}', f'extra.nix={module}')
        self.assertEqual(result.returncode, 1)
        self.assertEqual((self.fixture / 'seen.nix').read_text(), new.read_text())
        self.assertEqual((self.nixos / 'configuration.nix').read_text(), self.configuration)
        self.assertFalse((self.nixos / 'extra.nix').exists())
        self.assertIn('error: Building the new system failed\nsecond line', result.stderr)

    def test_files_stay_once_the_switch_has_begun(self):
        (self.fixture / 'helper-fails').write_text('System changes had already begun; the configuration was not restored.')
        new = self.fixture / 'new.nix'
        new.write_text('{ ... }: { services.openssh.enable = true; }\n')
        result = self.run_script('sekka', 'apply-configuration', f'configuration.nix={new}')
        self.assertEqual(result.returncode, 1)
        self.assertEqual((self.nixos / 'configuration.nix').read_text(), new.read_text())

    def test_sekkas_and_yukimis_files_are_refused(self):
        new = self.fixture / 'new.nix'
        new.write_text('{ }\n')
        for name in ['flake.nix', 'flake.lock', 'yukimi.nix', '../etc/passwd', '/etc/passwd', 'a b.nix']:
            result = self.run_script('sekka', 'apply-configuration', f'{name}={new}')
            self.assertEqual(result.returncode, 2, name)
        self.assertEqual(self.calls('builds'), [])


if __name__ == '__main__':
    unittest.main()
