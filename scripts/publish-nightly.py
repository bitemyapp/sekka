#!/usr/bin/env python3
"""Publish reviewed exact-image evidence; structural validation is not a VM test.

All remote mutations live in publish(). Validation and audit-digest do not use
R2. No credentials are read by this script: aws uses its local profile/environment.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import mimetypes
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import socket
import stat
import subprocess
import sys
import tempfile

BUCKET = 'sekka-downloads'
ENDPOINT = 'https://c408b91e58b3aa56c4f71046fcd416b6.r2.cloudflarestorage.com'
FOREVER = 'public, max-age=31536000, immutable'
CORE = {
    'Q0': ['provenance', 'capability'],
    'Q1': ['uefi-usb', 'bios-usb', 'uefi-optical', 'boot-entries', 'live-functionality'],
    'Q2': ['gui-uefi', 'gui-bios', 'default-install', 'storage-matrix'],
    'Q3': ['disk-only', 'greeter', 'session-lifecycle'],
    'Q4': ['inventory'], 'Q5': ['inventory'],
    'Q6': ['applications', 'updates', 'rollback', 'persistence', 'recovery'],
    'Q7': ['final-rerun'],
}
CORE_IDS = {f'{gate}.{name}' for gate, names in CORE.items() for name in names}
REPRODUCE = {'Q1.uefi-usb', 'Q2.gui-uefi', 'Q3.greeter', 'Q4.inventory',
             'Q5.inventory', 'Q6.updates'}
FOCUSED_IDS = {
    'Q0.provenance', 'Q0.capability', 'Q1.uefi-usb', 'Q1.bios-usb',
    'Q1.live-functionality', 'Q2.gui-uefi', 'Q2.gui-bios', 'Q2.default-install',
    'Q3.disk-only', 'Q3.greeter', 'Q3.session-lifecycle', 'Q4.core-desktop',
    'Q5.core-desktop', 'Q6.applications', 'Q6.network', 'Q6.persistence',
    'Q7.final-rerun',
}
CI_VARIABLES = ('CI', 'GITHUB_ACTIONS', 'GITLAB_CI', 'BUILDKITE', 'JENKINS_URL',
                'TEAMCITY_VERSION', 'TF_BUILD', 'CIRCLECI')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def audit_digest(report):
    return hashlib.sha256(canonical({k: v for k, v in report.items() if k != 'audit'})).hexdigest()


def text(value, label):
    require(isinstance(value, str) and bool(value.strip()), f'{label} must be nonempty text')
    return value


def sha(value, label):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value), f'{label}: invalid SHA-256')
    return value


def timestamp(value):
    require(isinstance(value, str) and value.endswith('Z'), 'timestamps must be UTC with Z suffix')
    return datetime.fromisoformat(value.removesuffix('Z') + '+00:00')


def relative_path(value):
    text(value, 'path')
    path = PurePosixPath(value)
    require(not path.is_absolute() and path.as_posix() == value and
            all(part not in ('.', '..') for part in path.parts) and
            re.fullmatch(r'[A-Za-z0-9_./-]+', value), f'unsafe relative path: {value!r}')
    return path


def evidence_path(root, name):
    path = root
    for part in relative_path(name).parts:
        path = path / part
        require(not path.is_symlink(), f'evidence symlink is forbidden: {name}')
    require(path.is_file() and stat.S_ISREG(path.stat().st_mode), f'missing regular evidence file: {name}')
    require(path.stat().st_size > 0, f'empty evidence file: {name}')
    return path


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def local_candidate(source):
    for name in CI_VARIABLES:
        require(not os.environ.get(name), f'publication is workstation-only ({name} is set)')
    require(not command('git', 'status', '--porcelain', '--untracked-files=no'),
            'checkout has tracked changes; use the clean tested revision')
    revision = command('git', 'rev-parse', 'HEAD')
    subprocess.run(['git', 'merge-base', '--is-ancestor', revision, 'refs/remotes/origin/main'], check=True)
    checkout = command('git', 'rev-parse', '--show-toplevel')
    # Pin evaluation to the commit, including if someone edits/switches the
    # worktree while the long ISO hash or upload is running.
    flake = f'git+{Path(checkout).as_uri()}?rev={revision}#iso.outPath'
    output = Path(command('nix', 'eval', '--raw', '--no-update-lock-file', flake))
    images = sorted(source.glob('*.iso'))
    require(len(images) == 1, 'expected exactly one ISO in ISO_DIRECTORY')
    iso = images[0].resolve(strict=True)
    # Evaluating this clean source's immutable output binds bytes to source.
    # A report alone, or an arbitrary same-named file, cannot establish that.
    require(iso.parent == output / 'iso' and iso.is_file(),
            'ISO must be the current clean flake output (or a symlink to it); do not rebuild after testing')
    lock_path = Path('flake.lock')
    lock_bytes = subprocess.check_output(['git', 'show', f'{revision}:flake.lock'])
    require(lock_path.read_bytes() == lock_bytes, 'lock changed during source validation')
    lock = json.loads(lock_bytes)
    # Aizome is Sekka's own source (desktop/aizome); Sidestep, which it builds
    # against, is the input it pins.
    sidestep = lock['nodes'][lock['nodes'][lock['root']]['inputs']['sidestep']]
    candidate = {
        'iso_sha256': digest(iso), 'iso_bytes': iso.stat().st_size,
        'sekka_revision': revision, 'flake_lock_sha256': digest(lock_path),
        'sidestep_revision': sidestep['locked']['rev'], 'iso_store_path': str(output),
    }
    require(candidate['iso_bytes'] > 0, 'empty ISO')
    return iso, candidate


def validate_report(root, candidate, hostname=None, now=None):
    report = json.loads(evidence_path(root, 'report.json').read_text())
    focused = report['schema_version'] == 2
    require(report['schema_version'] in (1, 2), 'unsupported qualification schema')
    if focused:
        require(report.get('profile') == 'focused-release-v1', 'unsupported focused release profile')
    require(report['candidate'] == candidate, 'qualification candidate differs from ISO, source or lock')
    require(report['workstation']['hostname'] == (hostname or socket.gethostname()),
            'qualification belongs to another workstation')
    require(report['workstation']['kind'] == 'workstation', 'qualification must run on a workstation')
    text(report['workstation']['tools'], 'workstation tool versions')
    coordinator = text(report['coordinator'], 'coordinator')
    started, completed = timestamp(report['started_at']), timestamp(report['completed_at'])
    require(started <= completed <= (now or datetime.now(timezone.utc)), 'invalid qualification time range')
    require(isinstance(report['limitations'], list), 'limitations must be an explicit list')
    for limitation in report['limitations']:
        text(limitation, 'public limitation')
    files = report['evidence']
    require(isinstance(files, dict) and files, 'evidence inventory is empty')
    for name, checksum in files.items():
        require(name != 'report.json', 'report cannot hash itself')
        require(digest(evidence_path(root, name)) == sha(checksum, name), f'evidence hash mismatch: {name}')
    referenced = set()

    def references(names):
        require(isinstance(names, list) and names, 'evidence references must be a nonempty list')
        for name in names:
            require(name in files, f'unlisted evidence: {name}')
            referenced.add(name)

    checks = report['checks']
    require(isinstance(checks, list) and checks, 'checks must be a nonempty list')
    by_id = {}
    for check in checks:
        ident = text(check['id'], 'check ID')
        require(ident not in by_id, f'duplicate check: {ident}')
        require(check['gate'] in CORE and ident.startswith(check['gate'] + '.'), f'invalid gate: {ident}')
        require(check['candidate_sha256'] == candidate['iso_sha256'], f'stale check: {ident}')
        require(check['status'] == 'pass', f'check has not passed: {ident}')
        for field in ('vm', 'precondition', 'action', 'expected', 'observed'):
            text(check[field], f'{ident}.{field}')
        references(check['evidence'])
        by_id[ident] = check
    required = FOCUSED_IDS if focused else CORE_IDS
    require(required <= by_id.keys(), f'missing core checks: {sorted(required - by_id.keys())}')
    if focused:
        scope = report['scope']
        text(scope['authorization'], 'focused scope authorization')
        references(scope['authorization_evidence'])
        for field in ('tested', 'excluded'):
            require(isinstance(scope[field], list) and scope[field], f'scope.{field} must be nonempty')
            for item in scope[field]:
                text(item, f'scope.{field}')
        require(report['limitations'], 'focused release requires explicit public limitations')
        require(report.get('inventories', {}) == {}, 'focused report must not claim exhaustive inventories')
        inventories = {}
    else:
        inventories = report['inventories']
        require(set(inventories) == {'Q4', 'Q5'}, 'both desktop feature inventories are required')
    for gate, inventory in inventories.items():
        require(inventory['source_revision'] == candidate['sekka_revision'], f'stale {gate} inventory')
        references(inventory['evidence'])
        features = inventory['features']
        require(isinstance(features, list) and features, f'empty {gate} feature inventory')
        seen = set()
        for feature in features:
            ident = text(feature['id'], 'feature ID')
            require(ident.startswith(gate + '.') and ident not in seen, f'invalid/duplicate feature: {ident}')
            seen.add(ident)
            text(feature['name'], f'{ident}.name')
            require(isinstance(feature['source_refs'], list) and feature['source_refs'], f'{ident}: source references missing')
            for ref in feature['source_refs']:
                text(ref, f'{ident}.source_refs')
            require(set(feature['cases']) == {'success', 'cancel', 'recovery'}, f'{ident}: missing feature cases')
            for case, value in feature['cases'].items():
                if isinstance(value, dict):
                    require(case != 'success' and set(value) == {'not_applicable'}, f'{ident}: success cannot be waived')
                    text(value['not_applicable'], f'{ident}.{case} reason')
                else:
                    require(value == f'{ident}.{case}' and value in by_id and by_id[value]['gate'] == gate,
                            f'{ident}.{case}: requires an individual passing {gate} check')
    audit = report['audit']
    reviewer = text(audit['reviewer'], 'reviewer')
    if focused:
        require(audit.get('mode') == 'coordinator-review', 'focused report must label coordinator review')
        require(reviewer == coordinator, 'coordinator review identity mismatch')
    else:
        require(reviewer != coordinator, 'coordinator cannot independently audit their own run')
    require(audit['status'] == 'pass' and audit['unresolved_findings'] == [], 'audit has unresolved findings')
    require(audit['reviewed_report_sha256'] == audit_digest(report), 'audit is stale; review the final report')
    require(completed <= timestamp(audit['completed_at']) <= (now or datetime.now(timezone.utc)), 'invalid audit time')
    require(set(audit['reviewed_checks']) == set(by_id), 'audit must account for every check')
    reproduced = set(audit['reproduced_checks'])
    require((FOCUSED_IDS if focused else REPRODUCE) <= reproduced <= set(by_id),
            'audit lacks critical reproduced checks')
    references(audit['evidence'])
    require(referenced == files.keys(), f'unreferenced evidence would be uploaded: {sorted(files.keys() - referenced)}')
    return report


class R2:
    def __init__(self):
        self.base = ['aws', '--region', 'auto', '--endpoint-url',
                     os.environ.get('SEKKA_DOWNLOADS_ENDPOINT', ENDPOINT)]

    def api(self, operation, *args):
        result = subprocess.run(self.base + ['s3api', operation, '--bucket', BUCKET, *map(str, args)],
                                text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(f'R2 {operation} failed: {result.stderr.strip()}')
        return json.loads(result.stdout or '{}')

    def index(self, path):
        try:
            response = self.api('get-object', '--key', 'nightly/builds.json', path)
        except RuntimeError as error:
            if '(NoSuchKey)' not in str(error):
                raise
            return {'builds': []}, None
        return json.loads(path.read_text()), response['ETag']

    def occupied(self, prefix):
        result = self.api('list-objects-v2', '--prefix', prefix, '--max-keys', '1', '--no-paginate')
        return bool(result.get('Contents'))

    def put_small(self, path, key, *, etag=None, new=True, cache=FOREVER):
        condition = ['--if-none-match', '*'] if new else ['--if-match', etag]
        return self.api('put-object', '--key', key, '--body', path,
                        '--content-type', 'application/json', '--cache-control', cache, *condition)

    def upload(self, path, key, checksum):
        content_type = 'application/x-iso9660-image' if path.suffix == '.iso' else mimetypes.guess_type(path.name)[0]
        subprocess.run(self.base + ['s3', 'cp', '--no-progress', '--only-show-errors',
                       '--content-type', content_type or 'application/octet-stream',
                       '--cache-control', FOREVER, '--metadata', f'sha256={checksum}',
                       str(path), f's3://{BUCKET}/{key}'], check=True)
        remote = self.api('head-object', '--key', key)
        require(remote['ContentLength'] == path.stat().st_size and
                remote.get('Metadata', {}).get('sha256') == checksum, f'uploaded object verification failed: {key}')

    def delete(self, key):
        self.api('delete-object', '--key', key)


def indexed_objects(build):
    """Only exact indexed paths, confined to this build; never discover deletions."""
    ident = build['id']
    require(isinstance(ident, str) and re.fullmatch(r'[0-9]{8}-[0-9a-f]{7,40}', ident), 'invalid indexed build ID')
    prefix = f'builds/{ident}/'
    legacy = [build[field] for field in ('iso', 'sha256sums', 'flake_lock', 'manifest')]
    names = build.get('objects', legacy)
    require(isinstance(names, list) and set(legacy) <= set(names), 'incomplete indexed object inventory')
    for name in names:
        relative_path(name)
        require(name.startswith(prefix) and name != prefix, f'indexed object escapes build: {name}')
    require(len(names) == len(set(names)), 'duplicate indexed objects')
    return ['nightly/' + name for name in names]


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def publish(source, root, *, remote=None):
    iso, candidate = local_candidate(source)
    report = validate_report(root, candidate)
    remote = remote or R2()
    with tempfile.TemporaryDirectory(prefix='sekka-publish-') as directory:
        stage = Path(directory)
        current, etag = remote.index(stage / 'current.json')
        require(isinstance(current.get('builds'), list), 'invalid current build index')
        require(len({build['id'] for build in current['builds']}) == len(current['builds']), 'duplicate indexed build IDs')
        # Validate all retention inputs before any mutation.
        for build in current['builds']:
            indexed_objects(build)
        ident = datetime.now(timezone.utc).strftime('%Y%m%d') + '-' + candidate['sekka_revision'][:7]
        prefix = f'builds/{ident}/'
        require(all(build['id'] != ident for build in current['builds']) and
                not remote.occupied('nightly/' + prefix), f'immutable build path already exists: {ident}')
        name = f'sekka-nightly-{ident}-x86_64-linux.iso'
        payloads = {name: (iso, candidate['iso_sha256'])}
        # Snapshot only validated evidence, then verify the copied bytes. Edits
        # during upload cannot silently replace reviewed report/evidence.
        for relative in ['report.json', *report['evidence']]:
            destination = stage / 'qualification' / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(evidence_path(root, relative), destination)
        snapshot = stage / 'qualification'
        require(json.loads((snapshot / 'report.json').read_text()) == report, 'report changed during staging')
        validate_report(snapshot, candidate)
        for relative in ['report.json', *report['evidence']]:
            path = snapshot / relative
            payloads['qualification/' + relative] = (path, digest(path))
        shutil.copyfile('flake.lock', stage / 'flake.lock')
        require(digest(stage / 'flake.lock') == candidate['flake_lock_sha256'], 'lock changed during staging')
        (stage / 'SHA256SUMS').write_text(f'{candidate["iso_sha256"]}  {name}\n')
        for filename in ('flake.lock', 'SHA256SUMS'):
            payloads[filename] = (stage / filename, digest(stage / filename))
        manifest = {
            'id': ident, 'revision': candidate['sekka_revision'],
            'committed': command('git', 'show', '-s', '--format=%cI', candidate['sekka_revision']),
            'uploaded': datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
            'system': 'x86_64-linux', 'iso': prefix + name,
            'sha256sums': prefix + 'SHA256SUMS', 'manifest': prefix + 'manifest.json',
            'flake_lock': prefix + 'flake.lock', 'bytes': candidate['iso_bytes'],
            'sha256': candidate['iso_sha256'], 'qualification': prefix + 'qualification/report.json',
            'qualification_profile': 'focused-release-v1' if report['schema_version'] == 2 else 'exhaustive-v1',
            'limitations': report['limitations'],
            'objects': [prefix + path for path in [*payloads, 'manifest.json', 'publication.json']],
        }
        write_json(stage / 'manifest.json', manifest)
        payloads['manifest.json'] = (stage / 'manifest.json', digest(stage / 'manifest.json'))
        # Atomically reserve the immutable namespace. Failed/uncertain runs stay
        # occupied for review: retry never overwrites or auto-cleans a prefix.
        write_json(stage / 'publication.json', {'candidate': candidate, 'report_sha256': digest(snapshot / 'report.json')})
        remote.put_small(stage / 'publication.json', 'nightly/' + prefix + 'publication.json')
        require(digest(iso) == candidate['iso_sha256'], 'ISO changed before upload')
        for filename, (path, checksum) in payloads.items():
            remote.upload(path, 'nightly/' + prefix + filename, checksum)
        retained = [manifest, *current['builds']][:7]
        write_json(stage / 'builds.json', {'builds': retained})
        # Compare-and-swap prevents concurrent publication/pruning from losing
        # index changes. An error exposes no partial new build.
        remote.put_small(stage / 'builds.json', 'nightly/builds.json', etag=etag,
                         new=etag is None, cache='no-cache')
        for build in current['builds'][6:]:
            for key in indexed_objects(build):
                remote.delete(key)
        print(f'Published {ident}: https://downloads.sekka.computer/nightly/{prefix}{name}')
        print('Now independently download/hash the public ISO and verify the page and Kakehashi (Q8).')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='operation', required=True)
    for name in ('validate', 'publish'):
        sub = commands.add_parser(name)
        sub.add_argument('iso_directory', type=Path)
        sub.add_argument('qualification_directory', type=Path)
    sub = commands.add_parser('audit-digest')
    sub.add_argument('qualification_directory', type=Path)
    args = parser.parse_args()
    if args.operation == 'audit-digest':
        report = json.loads(evidence_path(args.qualification_directory, 'report.json').read_text())
        print(audit_digest(report))
    elif args.operation == 'validate':
        _, candidate = local_candidate(args.iso_directory)
        validate_report(args.qualification_directory, candidate)
        print('Qualification structure, exact-image binding and evidence hashes pass; review remains required.')
    else:
        publish(args.iso_directory, args.qualification_directory)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, TypeError, OSError, RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(f'sekka-publish-nightly: {error}')
