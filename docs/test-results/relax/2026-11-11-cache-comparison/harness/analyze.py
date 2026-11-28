#!/usr/bin/env python3
"""Validate retained observations and summarize native and JIT cache counters."""
import collections
import gzip
import hashlib
import json
from pathlib import Path
import re
import statistics
import sys

root = Path(sys.argv[1])
e = root / 'evidence'
index = json.loads((e / 'index.json').read_text())
for row in index['files']:
    assert hashlib.sha256((e / row['file']).read_bytes()).hexdigest() == row['stored_sha256'], row['file']
data = json.loads((e / 'results.json').read_text())
assert data['status'] == 'passed'
expected_stages = {'cold': 1, 'warm-a': 3, 'identical-b': 1, 'warm-b': 3,
                   'unrelated-c': 1, 'warm-c': 3, 'leaf-edit-c': 1, 'leaf-warm-c': 3,
                   'shared-edit-c': 1, 'shared-warm-c': 3, 'fresh-leaf-edit-d': 1,
                   'fresh-leaf-warm-d': 3}
assert collections.Counter((r['stage'], r['engine']) for r in data['runs']) == {
    (stage, engine): count for stage, count in expected_stages.items() for engine in ('relax', 'buck2')}
def text_file(name):
    p = e / name
    if not p.exists():
        p = e / (name + '.gz')
    return gzip.decompress(p.read_bytes()).decode() if p.suffix == '.gz' else p.read_text()

summary = {'runs': [], 'groups': [], 'storage': [], 'plain_warm': [], 'validation': {}}
for r in data['runs']:
    row = {k: r[k] for k in ('stage', 'engine', 'repeat', 'seconds', 'settled_seconds')}
    row['packages'] = []
    for c in r['commands']:
        assert c['exit_status'] == 0 and all(s['status'] == 0 for s in c['orphan_status'])
        name = Path(c['log']).name
        log = text_file(name)
        matches = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', log)
        assert len(matches) == 1 and matches[0][1] == '0', name
        expected = 40 if c['package'] == 'aizome-kit' else (12 if any(x in r['stage'] for x in ('leaf', 'shared')) else 11)
        assert int(matches[0][0]) == expected, (name, matches)
        p = {'package': c['package'], 'seconds': c['seconds'], 'passed': expected}
        if r['engine'] == 'buck2':
            events = [json.loads(line) for line in text_file(name.removesuffix('.log') + '.events.json-lines.gz').splitlines()]
            trace_ids = {event['Event']['trace_id'] for event in events if 'Event' in event}
            expected_id = text_file(name.removesuffix('.log') + '.build-id').strip()
            assert trace_ids == {expected_id}
            assert re.findall(r'Build ID: ([a-z0-9-]+)', log) == [expected_id], 'Unexpected native retry'
            kinds = collections.Counter()
            compiler = 0
            local_tests = 0
            action_keys = set()
            result_seen = False
            for event in events:
                result = event.get('Result', {}).get('result', {}).get('test_response')
                if result is not None:
                    assert result.get('executor_exit_code') == 0 and not result.get('build_errors') and not result.get('test_errors')
                    result_seen = True
                end = event.get('Event', {}).get('data', {}).get('SpanEnd', {}).get('data', {})
                action = end.get('ActionExecution')
                if action:
                    assert not action.get('failed')
                    kind = action['execution_kind']
                    kinds[str(kind)] += 1
                    action_keys.add(json.dumps(action['key'], sort_keys=True))
                    compiler += (kind in (1, 2, 8, 11) and action.get('name', {}).get('category') in ('cargo_build', 'cargo_test'))
                if 'TestRun' in end:
                    report = end['TestRun']['command_report']
                    assert report['status'] == {'Success': {}}
                    assert 'LocalCommand' in report['details']['command_kind']['command']
                    local_tests += 1
            assert result_seen and local_tests == 1
            p.update(action_execution_kinds=dict(kinds), compiler_executions=compiler,
                     local_test_executions=local_tests, unique_action_keys=len(action_keys), trace_id=expected_id)
        else:
            stats = re.findall(r'makepad-run cache: (\d+) hits, (\d+) misses, (\d+) link failures, (\d+) refused', log)
            assert len(stats) == 1, name
            hits, misses, failed, refused = map(int, stats[0])
            assert failed == refused == 0
            prelinked = sum(map(int, re.findall(r'prelinked (\d+) startup slots', log)))
            p.update(lazy_code_hits=hits, lazy_code_misses=misses, prelinked_startup_slots=prelinked)
        row['packages'].append(p)
    summary['runs'].append(row)

for stage in dict.fromkeys(r['stage'] for r in summary['runs']):
    for engine in ('relax', 'buck2'):
        rows = [r for r in summary['runs'] if r['stage'] == stage and r['engine'] == engine]
        values = [r['seconds'] for r in rows]
        summary['groups'].append({'stage': stage, 'engine': engine, 'n': len(values),
                                  'median_seconds': statistics.median(values), 'range_seconds': [min(values), max(values)]})

for s in data['snapshots']:
    log = text_file(Path(s['physical_receipt']).name)
    receipt = json.loads(log.splitlines()[0])
    assert receipt['exit_status'] == 0 and receipt['unresolved'] == []
    totals = re.search(r'^TOTAL\s+\d+%\s+(\d+)\s+(\d+)\s+(\d+)', log, re.M)
    assert totals
    summary['storage'].append({'stage': s['stage'], 'engine': s['engine'],
                               **dict(zip(('physical_bytes', 'uncompressed_unique_bytes', 'referenced_bytes'), map(int, totals.groups())))})

plain = json.loads((e / 'plain-warm.json').read_text())
assert len(plain) == 6
for row in plain:
    assert len(row['commands']) == 2
    for command in row['commands']:
        assert command['exit_status'] == 0
        count = 40 if command['package'] == 'aizome-kit' else 11
        assert re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', text_file(Path(command['log']).name)) == [(str(count), '0')]
for engine in ('relax', 'buck2'):
    values = [r['seconds'] for r in plain if r['engine'] == engine]
    summary['plain_warm'].append({'engine': engine, 'n': len(values), 'median_seconds': statistics.median(values),
                                  'range_seconds': [min(values), max(values)]})
summary['validation'] = {'indexed_files_verified': len(index['files']), 'package_invocations': sum(len(r['packages']) for r in summary['runs']),
                         'plain_warm_package_invocations': 12,
                         'every_test_count_verified': True, 'all_native_attempts_successful_without_retries': True,
                         'every_native_test_invocation_executed_locally': True,
                         'all_relax_link_failures_and_certificate_refusals_zero': True}
(root / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
for row in summary['groups'] + summary['storage'] + summary['plain_warm']:
    print(row)
for row in summary['runs']:
    if row['repeat'] == 0 and 'warm' not in row['stage']:
        print(row['stage'], row['engine'], row['packages'])
