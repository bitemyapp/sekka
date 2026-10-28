#!/usr/bin/env python3
"""Summarize the exact three-engine raw report without changing samples."""
from pathlib import Path
import hashlib
import json
import statistics

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'direct-expanded'
raw = OUT / 'summary.json'
d = json.loads(raw.read_text())
assert d['status'] == 'completed' and len(d['runs']) == 28
builders = {'rustc', 'clang', 'gcc', 'g++', 'cc1', 'cc1plus', 'ld', 'ld.lld',
            'ld.mold', 'cargo', '.cargo-wrapped', 'zig', 'ninja', 'make', 'cmake'}
observations = [d['preflight']['before'], d['preflight']['after']] + d['snapshots'] + [
    value for run in d['runs'] for value in (run['before'], run['after'])]
frequencies = [s['frequency_khz']['cpu8'] for s in observations if 'cpu8' in s['frequency_khz']]
result = {'schema': 1, 'status': 'completed diagnostic; candidate self-regression and rejection preserved',
          'scope': d['scope'], 'raw_sha256': hashlib.sha256(raw.read_bytes()).hexdigest(),
          'source_sha256': {name: build['source_sha256'] for name, build in d['builds'].items()},
          'binary_sha256': {name: build['binary_sha256'] for name, build in d['builds'].items()},
          'reference': d['reference_build'], 'library_sha256': d['library_sha256'],
          'protocol': {'cpu': 8, 'sibling': 72, 'rows': 12, 'cols': 120, 'history': 0,
                       'chunk_bytes': 65536, 'rounds': 6, 'samples_per_round': 10, 'warmups': 3,
                       'order': 'all six permutations; every engine occupies each position twice'},
          'host': {'preflight_active_percent': {cpu: d['preflight']['active_percent'][cpu] for cpu in ('cpu8', 'cpu72')},
                   'observations': len(observations), 'between_batch_observations': len(d['snapshots']),
                   'observed_builders': [p for s in observations for p in s['processes'] if p['name'] in builders],
                   'cpu8_frequency_khz_min_max': [min(frequencies), max(frequencies)],
                   'cpu8_missing_frequency_count': len(observations) - len(frequencies),
                   'scope': 'Periodic observations cannot exclude every brief outside disturbance.'},
          'state_omissions': d['state_omissions'], 'reviewed_input_scope': d['reviewed_input_scope'],
          'cases': []}
for run in d['runs']:
    assert run['full_baseline_candidate_state_equal'] and run['pinned_reference_common_state_equal']
    case = {key: run[key] for key in ('mode', 'input', 'input_bytes', 'input_sha256', 'statistics',
        'baseline_over_ghostty_statistics', 'candidate_over_ghostty_statistics', 'candidate_over_baseline_statistics')}
    case['round_median_ratios'] = {}
    for numerator, denominator in [('baseline', 'ghostty'), ('candidate', 'ghostty'), ('candidate', 'baseline')]:
        a, b = run['samples_ns'][numerator], run['samples_ns'][denominator]
        assert len(a) == len(b) == 60
        case['round_median_ratios'][numerator + '_over_' + denominator] = [
            statistics.median(a[i:i+10]) / statistics.median(b[i:i+10]) for i in range(0, 60, 10)]
    result['cases'].append(case)
result['all_28_complete_state_and_reference_gates_passed'] = True
(OUT / 'compact.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({'host': result['host'], 'source_sha256': result['source_sha256'],
                  'binary_sha256': result['binary_sha256']}, indent=2))
for case in result['cases']:
    print(case['mode'], case['input'], 'baseline/Ghostty',
          round(case['baseline_over_ghostty_statistics']['median_feed_ns'], 4),
          'candidate/Ghostty', round(case['candidate_over_ghostty_statistics']['median_feed_ns'], 4),
          'candidate/baseline', round(case['candidate_over_baseline_statistics']['median_feed_ns'], 4))
