#!/usr/bin/env python3
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

B = Path('/home/callen/sekka-work/relax-cache-comparison-20261111')
E = B / 'evidence'
data = json.loads((E / 'results.json').read_text())
assert data['status'] == 'passed'
environment = dict(os.environ, **data['environment'])
environment.pop('MAKEPAD_RUN_VERBOSE', None)
plain = []
for repeat in range(3):
    for engine in (('buck2', 'relax') if repeat % 2 == 0 else ('relax', 'buck2')):
        source = next(r for r in data['runs'] if r['stage'] == 'warm-a' and r['engine'] == engine)
        row = {'engine': engine, 'repeat': repeat, 'commands': []}
        for original in source['commands']:
            prefix = str(E / f'plain-warm-a-{engine}-{repeat}-{original["package"]}')
            old = original['log'].removesuffix('.log')
            command = [arg.replace(old, prefix) for arg in original['command']]
            with open(prefix + '.log', 'w') as log:
                started = time.monotonic()
                status = subprocess.call(command, cwd=original['cwd'], env=environment, stdout=log, stderr=subprocess.STDOUT)
                elapsed = time.monotonic() - started
            assert status == 0
            row['commands'].append({'package': original['package'], 'command': command, 'seconds': elapsed, 'exit_status': status, 'log': prefix + '.log'})
        row['seconds'] = sum(c['seconds'] for c in row['commands'])
        plain.append(row)
        print('PLAIN', engine, repeat, row['seconds'], flush=True)
(E / 'plain-warm.json').write_text(json.dumps(plain, indent=2) + '\n')

metadata = {'host': subprocess.check_output(['uname', '-a'], text=True).strip(),
            'cpu': subprocess.check_output(['lscpu'], text=True),
            'memory': Path('/proc/meminfo').read_text(),
            'loadavg': Path('/proc/loadavg').read_text(), 'source_status': []}
for tree in data['trees']:
    root = tree['path']
    status = subprocess.check_output(['git', '-C', root, 'status', '--porcelain', '--untracked-files=no'], text=True)
    assert status == ''
    locks = subprocess.check_output(['git', '-C', root, 'ls-files', '*Cargo.lock', '*Cargo.toml'], text=True).splitlines()
    hashes = {p: hashlib.sha256((Path(root) / p).read_bytes()).hexdigest() for p in locks}
    metadata['source_status'].append({'tree': root, 'tracked_status': status, 'manifest_lock_hashes': hashes})
assert all(s['manifest_lock_hashes'] == metadata['source_status'][0]['manifest_lock_hashes'] for s in metadata['source_status'])
manifest = json.loads((B / 'service.json').read_text())
metadata['service'] = {k: v for k, v in manifest.items() if k != 'domains'}
metadata['service']['domains'] = {k: {'port': v['port']} for k, v in manifest['domains'].items()}
daemon_roots = ['/home/callen/.buck/buckd' + t['path'] + '/cache-compare-20261111' for t in data['trees']]
metadata['buck_daemon_allocated_bytes'] = subprocess.check_output(['du', '-s', '-B1', *daemon_roots], text=True)
metadata['buck_daemon_logical_bytes'] = subprocess.check_output(['du', '-s', '-B1', '--apparent-size', *daemon_roots], text=True)
(E / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')

# Preserve raw hashes while exporting only files with credentials removed. Never
# copy the private NativeLink manifest or client configuration into the evidence.
secrets = [v[k].encode() for v in manifest['domains'].values() for k in ('token', 'reader_token')]
dest = B / 'public-evidence'
dest.mkdir()
index = []
for p in sorted(E.iterdir()):
    raw = p.read_bytes()
    decoded = gzip.decompress(raw) if p.suffix == '.gz' else raw
    count = sum(decoded.count(secret) for secret in secrets)
    for secret in secrets:
        decoded = decoded.replace(secret, b'[REDACTED-SERVICE-TOKEN]')
    name = p.name if p.suffix in ('.json', '.gz', '.build-id') else p.name + '.gz'
    stored = gzip.compress(decoded, mtime=0) if name.endswith('.gz') else decoded
    (dest / name).write_bytes(stored)
    index.append({'file': name, 'raw_file': p.name, 'raw_sha256': hashlib.sha256(raw).hexdigest(),
                  'stored_sha256': hashlib.sha256(stored).hexdigest(), 'credential_redactions': count})
(dest / 'index.json').write_text(json.dumps({'files': index}, indent=2) + '\n')
print('EXPORTED', len(index), 'files; redactions', sum(r['credential_redactions'] for r in index), flush=True)
