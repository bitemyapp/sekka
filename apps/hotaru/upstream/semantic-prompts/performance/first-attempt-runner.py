#!/usr/bin/env python3
"""Strict paired native preflight and feed-only comparisons; abort on build load."""
import datetime
import fcntl
import hashlib
import itertools
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

from differential import Oracle, differences

ROOT = Path(__file__).resolve().parent.parent
BASE = Path('/home/callen/work/hotaru-ascii-cell-store-20261028')
LIBRARY = Path('/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so')
OUT = ROOT / ('native-forty-' + os.environ.get('HOTARU_BENCH_ATTEMPT', 'a'))
REFERENCE = Path('/home/callen/work/hotaru-unicode-mode-20261028')
CPU = int(os.environ.get('HOTARU_BENCH_CPU','24'))
BUILDERS = {'rustc', 'clang', 'gcc', 'g++', 'cc1', 'cc1plus', 'ld', 'ld.lld',
            'ld.mold', 'cargo', '.cargo-wrapped', 'zig', 'ninja', 'make', 'cmake'}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot():
    cpus = {}
    for line in Path('/proc/stat').read_text().splitlines():
        name, *values = line.split()
        if name.startswith('cpu') and name[3:].isdigit():
            cpus[name] = list(map(int, values[:8]))
    processes = []
    for entry in Path('/proc').iterdir():
        if entry.name.isdigit():
            try:
                name = (entry / 'comm').read_text().strip()
                stat = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
                processes.append({'pid': int(entry.name), 'name': name,
                                  'cpu_ticks': int(stat[11]) + int(stat[12])})
            except (OSError, ValueError, IndexError):
                pass
    frequencies = {}
    for name in cpus:
        path = Path('/sys/devices/system/cpu') / name / 'cpufreq/scaling_cur_freq'
        try:
            frequencies[name] = int(path.read_text())
        except (OSError, ValueError):
            pass
    return {'time_ns': time.monotonic_ns(), 'cpu_ticks': cpus, 'processes': processes,
            'frequency_khz': frequencies, 'load_average': os.getloadavg()}


def utilization(before, after):
    result = {}
    for name, last in after['cpu_ticks'].items():
        delta = [b - a for a, b in zip(before['cpu_ticks'][name], last)]
        total = sum(delta)
        result[name] = 100 * (total - delta[3] - delta[4]) / total if total else None
    return result


def source_check(root, metadata):
    for name, expected in metadata['sources'].items():
        if sha(root / name) != expected:
            raise RuntimeError(f'source changed: {root / name}')


def main():
    OUT.mkdir(exist_ok=True)
    report = {'schema': 1, 'recorded_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'scope': 'three-engine native in-process feed, zero history; isolated semantic metadata performance qualification; no GUI or universal parity claim',
              'state_omissions': ['hyperlinks', 'other modes/effects', 'pending wrap', 'graphics', 'scrollback', 'renderer'],
              'reviewed_input_scope': 'Four existing fixed protocol patterns; one dense-wide pattern; ten exact malformed UTF-8 plus printable ASCII/CRLF patterns; one long dense-valid prefix followed by malformed UTF-8. Adds matched80-row8MiB versions of the four common patterns. No OSC133 is in these throughput inputs; new semantic behavior is covered separately by the actual-library fixture gate.',
              'cpu': CPU, 'rounds': 6, 'iterations_per_round': 10, 'warmup_iterations': 3,
              'status': 'preparing', 'runs': [], 'snapshots': []}
    report_path = OUT / 'summary.json'
    monitor = None
    monitor_file = None
    try:
        with (Path.home() / '.cache/hotaru-workstation.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            allowed = os.sched_getaffinity(0)
            assert CPU in allowed
            os.sched_setaffinity(0, {CPU})
            siblings = [int(value) for value in (Path('/sys/devices/system/cpu') / f'cpu{CPU}' /
                        'topology/thread_siblings_list').read_text().strip().split(',')]
            report['observed_sibling_cpus'] = siblings
            first = snapshot()
            time.sleep(1)
            second = snapshot()
            report['preflight'] = {'before': first, 'after': second,
                                   'active_percent': utilization(first, second)}
            busy = [p for p in second['processes'] if p['name'] in BUILDERS]
            if busy or any((report['preflight']['active_percent'][f'cpu{cpu}'] or 0) > 10 for cpu in siblings):
                report['status'] = 'blocked_preflight_no_timings'
                report['builders'] = busy
                return
            roots = {'baseline': BASE, 'candidate': ROOT}
            builds = {'baseline': json.loads((ROOT / 'evidence/native-baseline-build.json').read_text()),
                      'candidate': json.loads((ROOT / 'evidence/native-build.json').read_text())}
            binaries = {name: root / 'target/release/hotaru-replay' for name, root in roots.items()}
            for name in roots:
                source_check(roots[name], builds[name])
                assert sha(binaries[name]) == builds[name]['binary_sha256']
            report['builds'] = builds
            report['reference_build'] = json.loads((REFERENCE / 'reference-build.json').read_text())
            reference_build = report['reference_build']
            assert reference_build['build_confirmed'] and reference_build['revision'] == '6ba144158577d033ccb6ed4b574e4013957c9161'
            binaries['ghostty'] = REFERENCE / 'ghostty-reference-replay'
            reference_sha = sha(binaries['ghostty'])
            assert reference_sha == reference_build['binaries']['ghostty-reference-replay']['sha256'] == 'dc687741377b3058f7b06bcdf287df9990c183eadd571a1fbe835fb3869625cd'
            assert sha(Path(reference_build['adapter']['source'])) == reference_build['adapter']['source_sha256']
            report['runner_sha256'] = sha(Path(__file__))
            report['helper_sha256'] = sha(ROOT / 'tools/differential.py')
            report['library_sha256'] = sha(LIBRARY)
            assert report['library_sha256'] == '70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639'
            corpus = Path('/home/callen/work/hotaru-rep-last-cell-20261028/measurements')
            inputs = {name: corpus / f'{name}.vt' for name in ['ascii', 'unicode', 'styled', 'cursor']}
            inputs.update({name: ROOT / 'evidence/profiles' / f'{name}.vt'
                           for name in ['short-invalid-leads', 'short-invalid-suffix', 'short-invalid-second', 'short-overlong-three', 'short-surrogate', 'short-too-large', 'short-interior-error', 'short-restricted-tail', 'short-truncated-two', 'short-truncated-three']})
            prior_path = Path('/home/callen/work/hotaru-short-utf8-complete-tail-20261028/direct-expanded/summary.json')
            prior = json.loads(prior_path.read_text())
            assert prior['status'] == 'completed' and len(prior['runs']) == 28
            report['prior_28_input_report_sha256'] = sha(prior_path)
            from corpus import PATTERNS
            patterns = {name: PATTERNS[name] for name in ('ascii', 'unicode', 'styled', 'cursor')}
            patterns.update({'short-invalid-leads': b'\x80' * 53 + b'\r\n',
                             'short-invalid-suffix': '雪'.encode() + b'a' * 48 + b'\xf0\x9f\r\n'})
            expanded = json.loads((ROOT / 'evidence/profiles/expanded-manifest.json').read_text())
            patterns.update({case['name']: bytes.fromhex(case['pattern_hex']) for case in expanded['cases']})
            inputs['dense-wide'] = ROOT / 'evidence/profiles/dense-wide.vt'
            patterns['dense-wide'] = ('雪' * 36 + '\r\n').encode()
            inputs['long-dense-error-prefix'] = ROOT / 'evidence/profiles/long-dense-error-prefix.vt'
            patterns['long-dense-error-prefix'] = '雪'.encode() * 16384 + b'\xf0\x9f\r\n'
            for name in ['ascii','unicode','styled','cursor']:
                label = 'large-' + name
                patterns[label] = patterns[name]
                inputs[label] = ROOT / 'evidence' / (label + '.vt')
                pattern = patterns[label]
                inputs[label].write_bytes(pattern * ((8 * 1024 * 1024 + len(pattern) - 1) // len(pattern)))
            for name, path in inputs.items():
                raw = path.read_bytes()
                pattern = patterns[name]
                assert raw and len(raw) % len(pattern) == 0 and raw == pattern * (len(raw) // len(pattern))
                assert all(r['input_sha256'] == sha(path) for r in prior['runs'] if r['input'] == name)
                if name.startswith('short-'):
                    assert all(byte >= 32 or byte in (10, 13) for byte in raw)
            report['input_patterns_hex'] = {name: pattern.hex() for name, pattern in patterns.items()}
            oracle = Oracle(LIBRARY)
            monitor_file = (OUT / 'per-core-mpstat.log').open('w')
            monitor = subprocess.Popen(['taskset', '-c', str(max(allowed)), 'mpstat', '-P', 'ALL', '1'],
                                       stdout=monitor_file, stderr=subprocess.STDOUT)
            for mode in ['legacy', 'unicode']:
                for label, data in inputs.items():
                    rows = 80 if label.startswith('large-') else 12
                    parameters = ['--data', str(data), '--rows', str(rows), '--cols', '120',
                                  '--chunk', '65536', '--grapheme-width', mode]
                    before = snapshot()
                    if any(p['name'] in BUILDERS for p in before['processes']):
                        raise RuntimeError('compiler process appeared before workload')
                    raw = data.read_bytes()
                    expected = oracle.snapshot(raw, rows, 120, 65536, {2027: mode == 'unicode'})
                    states = {name: json.loads(subprocess.check_output(
                        [str(binary), *parameters, '--format', 'json'], text=True))
                        for name, binary in binaries.items() if name != 'ghostty'}
                    assert states['baseline'] == states['candidate'], (mode, label, 'full state mismatch')
                    delta = differences(expected, states['baseline'])
                    assert not delta, (mode, label, delta)
                    samples = {name: [] for name in binaries}
                    batches = []
                    order = []
                    commands = {name: [str(binary), *parameters, '--bench-json'] for name, binary in binaries.items()}

                    def measure(name, iterations):
                        value = json.loads(subprocess.check_output(
                            [*commands[name], '--iterations', str(iterations)], text=True))
                        for key, want in {'engine': 'ghostty' if name == 'ghostty' else 'hotaru', 'timed_scope': 'feed-only',
                                          'input_bytes': len(raw), 'rows': rows, 'cols': 120,
                                          'scrollback': 0, 'chunk_bytes': 65536,
                                          'grapheme_width_method': mode}.items():
                            assert value[key] == want, (key, value[key], want)
                        assert value['response_bytes'] == [len(expected['responses'])] * iterations
                        assert len(value['samples_ns']) == iterations
                        return value

                    for name in binaries:
                        measure(name, 3)
                    for round_index, permutation in enumerate(itertools.permutations(binaries)):
                        for name in permutation:
                            observed = snapshot()
                            report['snapshots'].append(observed)
                            if any(p['name'] in BUILDERS for p in observed['processes']):
                                raise RuntimeError('compiler process appeared during timing window')
                            batch = measure(name, 10)
                            samples[name].extend(batch['samples_ns'])
                            batches.append({'engine': name, 'round': round_index, 'value': batch})
                            order.append(name)
                    after = snapshot()
                    if any(p['name'] in BUILDERS for p in after['processes']):
                        raise RuntimeError('compiler process appeared after workload')
                    stats = {name: {'median_feed_ns': statistics.median(values),
                             'p95_feed_ns': sorted(values)[(95 * len(values) + 99) // 100 - 1],
                             'max_feed_ns': max(values)} for name, values in samples.items()}
                    ratios = {key: stats['candidate'][key] / stats['baseline'][key] for key in stats['baseline']}
                    report['runs'].append({'mode': mode, 'input': label, 'rows': rows, 'cols': 120, 'input_bytes': len(raw),
                        'input_sha256': sha(data), 'full_baseline_candidate_state_equal': True,
                        'pinned_reference_common_state_equal': True,
                        'state_sha256': hashlib.sha256(json.dumps(states['baseline'], sort_keys=True).encode()).hexdigest(),
                        'commands': commands, 'order': order, 'samples_ns': samples, 'raw_batches': batches,
                        'statistics': stats, 'candidate_over_baseline_statistics': ratios,
                        'baseline_over_ghostty_statistics': {key: stats['baseline'][key] / stats['ghostty'][key] for key in stats['ghostty']},
                        'candidate_over_ghostty_statistics': {key: stats['candidate'][key] / stats['ghostty'][key] for key in stats['ghostty']},
                        'tail_method': 'nearest-rank95th percentile',
                        'candidate_over_baseline': statistics.median(samples['candidate']) / statistics.median(samples['baseline']),
                        'before': before, 'after': after, 'per_core_active_percent': utilization(before, after)})
                    report_path.write_text(json.dumps(report, indent=2) + '\n')
            for name in roots:
                source_check(roots[name], builds[name])
                assert sha(binaries[name]) == builds[name]['binary_sha256']
            assert sha(binaries['ghostty']) == reference_sha
            assert sha(LIBRARY) == report['library_sha256']
            assert sha(Path(reference_build['adapter']['source'])) == reference_build['adapter']['source_sha256']
            for name, path in inputs.items():
                assert all(r['input_sha256'] == sha(path) for r in report['runs'] if r['input'] == name)
            report['status'] = 'completed'
    except Exception as error:
        report['status'] = 'failed_or_contaminated_not_acceptance'
        report['error'] = repr(error)
        raise
    finally:
        if monitor is not None:
            monitor.terminate()
            monitor.wait()
        if monitor_file is not None:
            monitor_file.close()
        report_path.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'status': report['status'], 'report': str(report_path)}))


if __name__ == '__main__':
    main()
