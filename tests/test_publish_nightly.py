"""Exercise qualification rejection and publication ordering without credentials.

Tiny synthetic fixtures test the gate, and do not certify a real Sekka image.
The recording R2 transport intentionally rejects conditional-write conflicts.
"""
from copy import deepcopy
from contextlib import chdir, redirect_stdout
from datetime import datetime, timezone
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('publish_nightly', ROOT / 'scripts/publish-nightly.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def build(index):
    ident = f'202601{index:02d}-{index:07x}'
    prefix = f'builds/{ident}/'
    return {'id': ident, 'iso': prefix + 'test.iso', 'sha256sums': prefix + 'SHA256SUMS',
            'flake_lock': prefix + 'flake.lock', 'manifest': prefix + 'manifest.json'}


class RecordingR2:
    def __init__(self, builds=()):
        self.current = {'builds': deepcopy(list(builds))}
        self.etag = 'original-etag'
        self.calls = []
        self.objects = {'nightly/unrelated.iso': b'unindexed data'}
        self.fail_upload = None
        self.fail_index = False
        self.conflict_claim = False

    def index(self, path):
        return deepcopy(self.current), self.etag

    def occupied(self, prefix):
        return any(key.startswith(prefix) for key in self.objects)

    def put_small(self, path, key, *, etag=None, new=True, cache=release.FOREVER):
        self.calls.append(('put', key, etag, new, cache))
        if new and (key in self.objects or self.conflict_claim):
            raise RuntimeError('412 PreconditionFailed')
        if key == 'nightly/builds.json':
            if self.fail_index or (not new and etag != self.etag):
                raise RuntimeError('412 PreconditionFailed')
            self.current = json.loads(path.read_text())
        self.objects[key] = path.read_bytes()

    def upload(self, path, key, checksum):
        self.calls.append(('upload', key))
        if self.fail_upload and self.fail_upload in key:
            raise RuntimeError('simulated transfer failure')
        if release.digest(path) != checksum:
            raise RuntimeError('changed payload')
        self.objects[key] = path.read_bytes()

    def delete(self, key):
        self.calls.append(('delete', key))
        self.objects.pop(key, None)


class Qualification(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.iso = self.root / 'candidate.iso'
        self.iso.write_bytes(b'synthetic ISO for publisher gate tests only')
        self.evidence = self.root / 'qualification'
        self.evidence.mkdir()
        (self.evidence / 'ui.txt').write_text('Synthetic interaction fixture; not real VM evidence.\n')
        (self.evidence / 'audit.txt').write_text('Synthetic independent audit fixture.\n')
        self.candidate = {
            'iso_sha256': release.digest(self.iso), 'iso_bytes': self.iso.stat().st_size,
            'sekka_revision': 'a' * 40, 'flake_lock_sha256': release.digest(ROOT / 'flake.lock'),
            'iso_store_path': '/nix/store/synthetic-iso',
        }
        ids = sorted(release.CORE_IDS) + ['Q4.app.test.success', 'Q4.app.test.cancel',
                                        'Q4.app.test.recovery', 'Q5.menu.test.success']
        self.report = {
            'schema_version': 1, 'candidate': self.candidate.copy(),
            'workstation': {'hostname': 'test-workstation', 'kind': 'workstation', 'tools': 'synthetic fixture'},
            'coordinator': 'coordinator-agent', 'started_at': '2026-01-01T00:00:00Z',
            'completed_at': '2026-01-01T01:00:00Z', 'limitations': [],
            'evidence': {name: release.digest(self.evidence / name) for name in ('ui.txt', 'audit.txt')},
            'checks': [dict(id=ident, gate=ident.split('.')[0], candidate_sha256=self.candidate['iso_sha256'],
                            vm='synthetic-vm', precondition='fixture', action='synthetic action',
                            expected='synthetic expected', observed='synthetic observed', status='pass',
                            evidence=['ui.txt']) for ident in ids],
            'inventories': {
                'Q4': {'source_revision': 'a' * 40, 'evidence': ['ui.txt'], 'features': [{
                    'id': 'Q4.app.test', 'name': 'Synthetic app', 'source_refs': ['docs/apps.md'],
                    'cases': {case: f'Q4.app.test.{case}' for case in ('success', 'cancel', 'recovery')}}]},
                'Q5': {'source_revision': 'a' * 40, 'evidence': ['ui.txt'], 'features': [{
                    'id': 'Q5.menu.test', 'name': 'Synthetic menu', 'source_refs': ['config/menu.lua'],
                    'cases': {'success': 'Q5.menu.test.success',
                              'cancel': {'not_applicable': 'Fixture has no stateful operation'},
                              'recovery': {'not_applicable': 'Fixture has no failure mode'}}}]},
            },
            'audit': {'reviewer': 'independent-agent', 'status': 'pass', 'unresolved_findings': [],
                      'completed_at': '2026-01-01T02:00:00Z', 'reviewed_checks': ids,
                      'reproduced_checks': sorted(release.REPRODUCE), 'evidence': ['audit.txt']},
        }
        self.save()

    def save(self, audit=True):
        if audit:
            self.report['audit']['reviewed_report_sha256'] = release.audit_digest(self.report)
        release.write_json(self.evidence / 'report.json', self.report)

    def validate(self):
        return release.validate_report(self.evidence, self.candidate, hostname='test-workstation')

    def publish(self, remote):
        with patch.object(release, 'local_candidate', return_value=(self.iso, self.candidate)), \
             patch.object(release.socket, 'gethostname', return_value='test-workstation'), \
             patch.object(release, 'command', return_value='2026-01-01T00:00:00+00:00'), \
             redirect_stdout(io.StringIO()):
            release.publish(self.root, self.evidence, remote=remote)

    def test_complete_report_and_audit_digest(self):
        self.assertEqual(self.validate(), self.report)
        before = release.audit_digest(self.report)
        self.report['audit']['reviewer'] = 'another-independent-agent'
        self.assertEqual(release.audit_digest(self.report), before)
        self.report['limitations'] = ['A newly recorded limitation']
        self.save(audit=False)
        with self.assertRaisesRegex(ValueError, 'audit is stale'):
            self.validate()

    def focused(self):
        self.report.update(schema_version=2, profile='focused-release-v1', inventories={},
                           limitations=['Exhaustive app and physical hardware testing not performed.'],
                           scope={'authorization': 'Synthetic explicit user approval of focused scope',
                                  'authorization_evidence': ['ui.txt'],
                                  'tested': ['Installation, boot, greeter and core desktops'],
                                  'excluded': ['Exhaustive feature matrix']})
        example = self.report['checks'][0]
        self.report['checks'] = [dict(example, id=ident, gate=ident.split('.')[0])
                                 for ident in sorted(release.FOCUSED_IDS)]
        self.report['audit'].update(mode='coordinator-review', reviewer='coordinator-agent',
                                   reviewed_checks=sorted(release.FOCUSED_IDS),
                                   reproduced_checks=sorted(release.FOCUSED_IDS))
        self.save()

    def test_focused_scope_is_explicit_and_published(self):
        self.focused()
        self.assertEqual(self.validate(), self.report)
        remote = RecordingR2()
        self.publish(remote)
        manifest = remote.current['builds'][0]
        self.assertEqual(manifest['qualification_profile'], 'focused-release-v1')
        self.assertEqual(manifest['limitations'], self.report['limitations'])
        # A focused report cannot masquerade as the existing exhaustive schema.
        self.report['schema_version'] = 1
        self.save()
        with self.assertRaisesRegex(ValueError, 'missing core checks'):
            self.validate()

    def test_focused_scope_rejects_missing_disclosure_or_false_independence(self):
        self.focused()
        baseline = deepcopy(self.report)
        mutations = [lambda r: r.update(profile='unknown'),
                     lambda r: r['scope'].update(authorization=''),
                     lambda r: r['scope'].update(authorization_evidence=[]),
                     lambda r: r['scope'].update(excluded=[]),
                     lambda r: r.update(limitations=[]),
                     lambda r: r['audit'].update(mode='independent'),
                     lambda r: r['audit'].update(reviewer='different-agent')]
        for mutate in mutations:
            self.report = deepcopy(baseline)
            mutate(self.report)
            self.save()
            with self.assertRaises(ValueError):
                self.validate()

    def test_focused_scope_cannot_waive_or_reuse_a_failed_boot(self):
        self.focused()
        check = next(c for c in self.report['checks'] if c['id'] == 'Q3.disk-only')
        check['status'] = 'blocked'
        self.save()
        with self.assertRaisesRegex(ValueError, 'has not passed'):
            self.validate()
        check['status'] = 'pass'
        check['candidate_sha256'] = '0' * 64
        self.save()
        with self.assertRaisesRegex(ValueError, 'stale check'):
            self.validate()
        self.report['checks'].remove(check)
        self.save()
        with self.assertRaisesRegex(ValueError, 'missing core checks'):
            self.validate()

    def test_missing_failed_and_stale_checks(self):
        baseline = deepcopy(self.report)
        for status in ('not-run', 'running', 'fail', 'blocked', 'not-applicable', ''):
            with self.subTest(status=status):
                self.report = deepcopy(baseline)
                self.report['checks'][0]['status'] = status
                self.save()
                with self.assertRaisesRegex(ValueError, 'has not passed'):
                    self.validate()
        self.report = deepcopy(baseline)
        self.report['checks'].pop(0)
        self.save()
        with self.assertRaisesRegex(ValueError, 'missing core checks'):
            self.validate()
        self.report = deepcopy(baseline)
        self.report['checks'][0]['candidate_sha256'] = '0' * 64
        self.save()
        with self.assertRaisesRegex(ValueError, 'stale check'):
            self.validate()

    def test_wrong_iso_source_lock_dependencies_or_workstation(self):
        for field in self.candidate:
            with self.subTest(field=field):
                self.report['candidate'] = self.candidate.copy()
                self.report['candidate'][field] = 'wrong'
                self.save()
                with self.assertRaisesRegex(ValueError, 'candidate differs'):
                    self.validate()
        self.report['candidate'] = self.candidate.copy()
        self.report['workstation']['hostname'] = 'ci-builder'
        self.save()
        with self.assertRaisesRegex(ValueError, 'another workstation'):
            self.validate()

    def test_missing_tampered_empty_and_symlink_evidence(self):
        path = self.evidence / 'ui.txt'
        path.write_text('changed')
        with self.assertRaisesRegex(ValueError, 'hash mismatch'):
            self.validate()
        path.write_text('')
        with self.assertRaisesRegex(ValueError, 'empty evidence'):
            self.validate()
        path.unlink()
        with self.assertRaisesRegex(ValueError, 'missing regular evidence'):
            self.validate()
        path.symlink_to(self.iso)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            self.validate()

    def test_unsafe_paths_and_unreferenced_evidence(self):
        for name in ('../candidate.iso', '/etc/passwd', 'a/../../x', './ui.txt', 'a//b', 'a\\b'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                release.evidence_path(self.evidence, name)
        (self.evidence / 'extra.txt').write_text('must not upload unrelated local files')
        self.report['evidence']['extra.txt'] = release.digest(self.evidence / 'extra.txt')
        self.save()
        with self.assertRaisesRegex(ValueError, 'unreferenced evidence'):
            self.validate()

    def test_missing_report_or_inventory_blocks_before_remote_mutation(self):
        remote = RecordingR2()
        (self.evidence / 'report.json').unlink()
        with self.assertRaises(ValueError):
            self.publish(remote)
        self.assertEqual(remote.calls, [])
        self.report['inventories']['Q4']['features'] = []
        self.save()
        with self.assertRaisesRegex(ValueError, 'empty Q4'):
            self.publish(remote)
        self.assertEqual(remote.calls, [])

    def test_feature_case_cannot_use_summary_or_waive_success(self):
        feature = self.report['inventories']['Q4']['features'][0]
        for value in ('Q4.inventory', 'missing', {'not_applicable': 'not tested'}):
            feature['cases']['success'] = value
            self.save()
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.validate()

    def test_independent_complete_audit_required(self):
        baseline = deepcopy(self.report)
        for field, value in [('reviewer', 'coordinator-agent'), ('status', 'blocked'),
                             ('unresolved_findings', ['missing proof']), ('reviewed_checks', []),
                             ('reproduced_checks', []), ('evidence', [])]:
            self.report = deepcopy(baseline)
            self.report['audit'][field] = value
            self.save()
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.validate()

    def test_upload_complete_evidence_before_index_and_scoped_retention(self):
        old = [build(index) for index in range(1, 9)]
        # New manifests can name their exact evidence files for future retention.
        old[-1]['objects'] = [old[-1][key] for key in ('iso', 'sha256sums', 'flake_lock', 'manifest')]
        old[-1]['objects'].append(f'builds/{old[-1]["id"]}/qualification/report.json')
        remote = RecordingR2(old)
        self.publish(remote)
        self.assertEqual(len(remote.current['builds']), 7)
        new = remote.current['builds'][0]
        self.assertIn(new['qualification'], new['objects'])
        self.assertEqual(remote.objects['nightly/' + new['iso']], self.iso.read_bytes())
        self.assertEqual(json.loads(remote.objects['nightly/' + new['qualification']]), self.report)
        index_position = next(i for i, call in enumerate(remote.calls) if call[:2] == ('put', 'nightly/builds.json'))
        self.assertTrue(all(call[0] != 'delete' for call in remote.calls[:index_position]))
        self.assertTrue(all(call[0] == 'delete' for call in remote.calls[index_position + 1:]))
        self.assertEqual({call[1] for call in remote.calls if call[0] == 'delete'},
                         {key for item in old[6:] for key in release.indexed_objects(item)})
        self.assertIn('nightly/unrelated.iso', remote.objects)
        for path in new['objects']:
            self.assertIn('nightly/' + path, remote.objects)

    def test_collision_and_racing_claim_refuse_overwrite(self):
        for claim_race in (False, True):
            remote = RecordingR2()
            remote.conflict_claim = claim_race
            ident = datetime.now(timezone.utc).strftime('%Y%m%d') + '-aaaaaaa'
            if not claim_race:
                remote.objects[f'nightly/builds/{ident}/leftover.iso'] = b'previous attempt'
            before = deepcopy(remote.objects)
            with self.subTest(claim_race=claim_race), self.assertRaises((ValueError, RuntimeError)):
                self.publish(remote)
            self.assertEqual(remote.objects, before)
            self.assertEqual(remote.current, {'builds': []})
            self.assertFalse(any(call[0] in ('upload', 'delete') for call in remote.calls))

    def test_each_transfer_failure_and_index_race_preserve_current_index(self):
        for filename in ('.iso', 'report.json', 'ui.txt', 'flake.lock', 'SHA256SUMS', 'manifest.json', 'index'):
            remote = RecordingR2([build(index) for index in range(1, 9)])
            before = deepcopy(remote.current)
            remote.fail_index = filename == 'index'
            remote.fail_upload = filename if filename != 'index' else None
            with self.subTest(filename=filename), self.assertRaises(RuntimeError):
                self.publish(remote)
            self.assertEqual(remote.current, before)
            self.assertFalse(any(call[0] == 'delete' for call in remote.calls))

    def test_invalid_retention_path_rejected_before_upload(self):
        for path in ('nightly/builds.json', 'builds/20260101-0000001/../../builds.json',
                     'builds/20260102-0000002/test.iso'):
            old = build(1)
            old['objects'] = [old[key] for key in ('iso', 'sha256sums', 'flake_lock', 'manifest')] + [path]
            remote = RecordingR2([old])
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.publish(remote)
            self.assertEqual(remote.calls, [])


class WorkstationBinding(unittest.TestCase):
    def test_real_git_revision_lock_and_nix_output_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / 'output'
            (output / 'iso').mkdir(parents=True)
            iso = output / 'iso/candidate.iso'
            iso.write_bytes(b'fixture in evaluated output')
            (root / 'result').symlink_to(output, target_is_directory=True)
            (root / 'flake.lock').write_bytes((ROOT / 'flake.lock').read_bytes())
            env = os.environ.copy()
            for name in release.CI_VARIABLES:
                env.pop(name, None)
            env['GIT_CONFIG_GLOBAL'] = '/dev/null'

            def git(*args):
                return subprocess.check_output(['git', *args], cwd=root, env=env, stderr=subprocess.DEVNULL).decode().strip()

            git('init')
            git('config', 'user.name', 'Publisher test fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            git('config', 'core.hooksPath', str(root / 'empty-hooks'))
            git('add', 'flake.lock')
            git('-c', 'commit.gpgsign=false', 'commit', '-m', 'fixture')
            revision = git('rev-parse', 'HEAD')
            git('update-ref', 'refs/remotes/origin/main', revision)
            original_command = release.command
            evaluations = []

            def evaluated_command(*args):
                if args[0] == 'nix':
                    evaluations.append(args)
                    return str(output)
                return original_command(*args)

            with chdir(root), patch.dict(os.environ, env, clear=True), \
                 patch.object(release, 'command', side_effect=evaluated_command):
                actual_iso, candidate = release.local_candidate(root / 'result/iso')
                self.assertEqual(actual_iso, iso)
                self.assertEqual(candidate['sekka_revision'], revision)
                self.assertEqual(candidate['flake_lock_sha256'], release.digest(root / 'flake.lock'))
                self.assertEqual(evaluations[0][-1], f'git+{root.as_uri()}?rev={revision}#iso.outPath')
                (root / 'flake.lock').write_text('{}\n')
                with self.assertRaisesRegex(ValueError, 'tracked changes'):
                    release.local_candidate(root / 'result/iso')
                git('checkout', '--', 'flake.lock')
                git('-c', 'commit.gpgsign=false', 'commit', '--allow-empty', '-m', 'unreviewed')
                with self.assertRaises(subprocess.CalledProcessError):
                    release.local_candidate(root / 'result/iso')

    def test_ci_and_dirty_source_rejected_before_evaluation(self):
        for variable in release.CI_VARIABLES:
            with self.subTest(variable=variable), patch.dict(os.environ, {variable: 'true'}, clear=True), \
                 self.assertRaisesRegex(ValueError, 'workstation-only'):
                release.local_candidate(Path('.'))
        with patch.dict(os.environ, {}, clear=True), patch.object(release, 'command', return_value=' M flake.nix'), \
             self.assertRaisesRegex(ValueError, 'tracked changes'):
            release.local_candidate(Path('.'))

    def test_nix_output_binding_rejects_copied_or_other_image(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'candidate.iso').write_bytes(b'wrong image')
            responses = ['', 'a' * 40, '/tmp/synthetic-checkout', '/nix/store/expected-output']
            with patch.dict(os.environ, {}, clear=True), \
                 patch.object(release, 'command', side_effect=responses), \
                 patch.object(release.subprocess, 'run'), \
                 self.assertRaisesRegex(ValueError, 'current clean flake output'):
                release.local_candidate(root)


class AwsTransport(unittest.TestCase):
    def test_conditional_writes_and_occupied_prefix_query(self):
        remote = release.R2()
        with patch.object(remote, 'api', return_value={'Contents': [{'Key': 'occupied'}]}) as api:
            self.assertTrue(remote.occupied('nightly/builds/example/'))
            api.assert_called_with('list-objects-v2', '--prefix', 'nightly/builds/example/',
                                   '--max-keys', '1', '--no-paginate')
            remote.put_small(Path('/tmp/claim'), 'nightly/builds/example/publication.json')
            self.assertEqual(api.call_args.args[-2:], ('--if-none-match', '*'))
            remote.put_small(Path('/tmp/index'), 'nightly/builds.json', etag='old-etag', new=False, cache='no-cache')
            self.assertEqual(api.call_args.args[-2:], ('--if-match', 'old-etag'))

    def test_only_missing_index_becomes_empty(self):
        remote = release.R2()
        with patch.object(remote, 'api', side_effect=RuntimeError('R2 (NoSuchKey)')):
            self.assertEqual(remote.index(Path('/does/not/exist')), ({'builds': []}, None))
        for error in ('AccessDenied', 'Timeout', 'malformed response'):
            with self.subTest(error=error), patch.object(remote, 'api', side_effect=RuntimeError(error)), \
                 self.assertRaises(RuntimeError):
                remote.index(Path('/does/not/exist'))


if __name__ == '__main__':
    unittest.main()
