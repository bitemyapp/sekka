"""Run the hourly workflow's shell steps with fake Nix and real, disposable Git remotes."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = yaml.load((ROOT / '.github/workflows/hourly.yml').read_text(), Loader=yaml.BaseLoader)
SECRETS = ('CACHE_R2_ACCESS_KEY_ID', 'CACHE_R2_SECRET_ACCESS_KEY', 'CACHE_SIGNING_KEY')
STAGES = ['update', 'kvm', 'uta', 'check', 'upload', 'coverage']

# Only Nix is simulated. The workflow commits, checks its tree, and pushes
# with real Git. No command can reach GitHub or a binary cache in these tests.
FAKE_NIX = r'''
import json, os, pathlib, subprocess, sys
args = sys.argv[1:]
if args == ['--version']:
    sys.exit(0)
if args[:2] == ['flake', 'update']:
    assert args[2:] == ['nixpkgs', 'applications', '--flake', '.'], args
    stage = 'update'
elif '--expr' in args:
    stage = 'kvm'
elif args[0] == 'build' and '.#uta' in args:
    stage = 'uta'
elif args[:2] == ['flake', 'check']:
    stage = 'check'
elif '.#push-cache' in args:
    stage = 'upload'
elif 'scripts/check_cache_coverage.py' in args:
    assert args[-2:] == ['.#uta', '.#installed-systems'], args
    stage = 'coverage'
else:
    raise AssertionError(args)
if stage not in ('update', 'kvm'):
    assert '--no-update-lock-file' in args, args
assert 'GH_TOKEN' not in os.environ
log = pathlib.Path(os.environ['RUNNER_TEMP']) / 'events'
revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
with log.open('a') as out:
    out.write(json.dumps([stage, revision]) + '\n')
if stage == 'upload':
    key = pathlib.Path(os.environ['SEKKA_CACHE_CONFIG']) / 'cache.sekka.computer-1.sec'
    assert key.read_text() == 'test-secret'
    assert key.stat().st_mode & 0o777 == 0o600
    (log.parent / 'key-path').write_text(str(key))
else:
    assert 'AWS_SECRET_ACCESS_KEY' not in os.environ
if os.environ.get('FAIL_STAGE') == stage:
    sys.exit(42)
if stage == 'update' and not os.environ.get('NO_UPDATE'):
    pathlib.Path('flake.lock').write_text('new nixpkgs\n')
if stage == 'coverage' and os.environ.get('DIRTY'):
    pathlib.Path('flake.lock').write_text('untested change\n')
    if os.environ['DIRTY'] == 'staged':
        subprocess.run(['git', 'add', 'flake.lock'], check=True)
'''


class HourlyRefresh(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'checkout'
        self.repo.mkdir()
        self.remote = self.root / 'remote.git'
        self.runner_temp = self.root / 'runner'
        self.runner_temp.mkdir()
        self.env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull,
                        GIT_TERMINAL_PROMPT='0', RUNNER_TEMP=str(self.runner_temp),
                        GITHUB_REPOSITORY='fixture/sekka')
        for name in (*SECRETS, 'AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'GH_TOKEN'):
            self.env.pop(name, None)
        self.git('init', '-b', 'main')
        self.git('config', 'user.name', 'Hourly fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        (self.repo / 'flake.lock').write_text('old nixpkgs\n')
        self.git('add', 'flake.lock')
        self.git('commit', '-m', 'base')
        self.base = self.git('rev-parse', 'HEAD')
        self.git('init', '--bare', str(self.remote))
        self.git('push', str(self.remote), 'HEAD:refs/heads/main')
        self.git('config', f'url.{self.remote}.insteadOf',
                 'https://x-access-token:test-token@github.com/fixture/sekka.git')
        bin_dir = self.root / 'bin'
        bin_dir.mkdir()
        nix = bin_dir / 'nix'
        nix.write_text(f'#!{sys.executable}\n' + FAKE_NIX)
        nix.chmod(0o755)
        self.env['PATH'] = f'{bin_dir}:{self.env["PATH"]}'

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.repo, env=self.env,
                                       stderr=subprocess.DEVNULL, text=True).strip()

    def remote_head(self):
        return self.git('--git-dir', str(self.remote), 'rev-parse', 'refs/heads/main')

    def run_workflow(self, *, missing=None, race=False, **env):
        secrets = {name: 'test-secret' for name in SECRETS}
        if missing:
            secrets[missing] = ''
        for step in WORKFLOW['jobs']['refresh']['steps']:
            if 'uses' in step:
                continue  # setUp supplies the checkout.
            step_env = dict(self.env, **env)
            for name, value in step.get('env', {}).items():
                if value == '${{ github.token }}':
                    step_env[name] = 'test-token'
                else:
                    step_env[name] = secrets[value.removeprefix('${{ secrets.').removesuffix(' }}')]
            if 'GH_TOKEN' in step_env and race:
                # A concurrent merge/nightly is a different child of our base.
                tree = self.git('rev-parse', f'{self.base}^{{tree}}')
                self.racing_head = self.git('commit-tree', tree, '-p', self.base, '-m', 'concurrent merge')
                self.git('push', str(self.remote), f'{self.racing_head}:refs/heads/main')
            result = subprocess.run(['bash', '--noprofile', '--norc', '-eo', 'pipefail', '-c', step['run']],
                                    cwd=self.repo, env=step_env, capture_output=True, text=True)
            if result.returncode:
                return result
        return result

    def events(self):
        path = self.runner_temp / 'events'
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_schedule_and_success_only_steps(self):
        self.assertEqual(WORKFLOW['on'], {'schedule': [{'cron': '17 * * * *'}], 'workflow_dispatch': ''})
        self.assertEqual(WORKFLOW['concurrency']['cancel-in-progress'], 'false')
        self.assertIn('${{ github.ref }}', WORKFLOW['concurrency']['group'])
        self.assertEqual(list(WORKFLOW['jobs']), ['refresh'])
        job = WORKFLOW['jobs']['refresh']
        self.assertIn("github.ref == 'refs/heads/main'", job['if'])
        self.assertEqual(job['environment'], 'nightly')
        for step in job['steps']:
            self.assertNotIn('if', step)
            self.assertNotIn('continue-on-error', step)
            if 'uses' in step:
                self.assertTrue(step['uses'].startswith('actions/checkout@'))
                self.assertEqual(step['with']['ref'], 'main')
                self.assertEqual(step['with']['persist-credentials'], 'false')

    def test_success_publishes_exact_checked_revision(self):
        result = self.run_workflow()
        self.assertEqual(result.returncode, 0, result.stderr)
        candidate = self.git('rev-parse', 'HEAD')
        self.assertNotEqual(candidate, self.base)
        self.assertEqual(self.remote_head(), candidate)
        self.assertEqual(self.git('show', 'HEAD:flake.lock'), 'new nixpkgs')
        self.assertEqual(self.events(), [['update', self.base]] + [[stage, candidate] for stage in STAGES[1:]])
        self.assertFalse(Path((self.runner_temp / 'key-path').read_text()).exists())
        self.assertEqual(self.git('--git-dir', str(self.remote), 'for-each-ref', '--format=%(refname)'),
                         'refs/heads/main')

    def test_each_failure_leaves_main_untouched(self):
        for stage in STAGES:
            with self.subTest(stage=stage):
                self.git('reset', '--hard', self.base)
                (self.runner_temp / 'events').unlink(missing_ok=True)
                result = self.run_workflow(FAIL_STAGE=stage)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.remote_head(), self.base)
                self.assertEqual([event[0] for event in self.events()], STAGES[:STAGES.index(stage) + 1])

    def test_missing_secrets_stop_before_work(self):
        for secret in SECRETS:
            with self.subTest(secret=secret):
                self.assertNotEqual(self.run_workflow(missing=secret).returncode, 0)
                self.assertEqual(self.events(), [])
                self.assertEqual(self.remote_head(), self.base)

    def test_unchanged_inputs_still_build_and_cache_without_a_commit(self):
        result = self.run_workflow(NO_UPDATE='1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.remote_head(), self.base)
        self.assertEqual(self.events(), [[stage, self.base] for stage in STAGES])

    def test_concurrent_merge_is_never_overwritten_or_rebased(self):
        result = self.run_workflow(race=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.remote_head(), self.racing_head)
        self.assertEqual(self.git('rev-parse', 'HEAD^'), self.base)

    def test_post_check_tree_changes_cannot_be_pushed(self):
        for dirty in ('staged', 'unstaged'):
            with self.subTest(dirty=dirty):
                self.git('reset', '--hard', self.base)
                self.assertNotEqual(self.run_workflow(DIRTY=dirty).returncode, 0)
                self.assertEqual(self.remote_head(), self.base)


if __name__ == '__main__':
    unittest.main()
