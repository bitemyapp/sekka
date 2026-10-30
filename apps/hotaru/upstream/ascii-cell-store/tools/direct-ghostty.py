#!/usr/bin/env python3
"""Run the established reviewed corpus benchmark inside one observed lock window."""
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import statistics
import subprocess
import threading
import time

_spec = importlib.util.spec_from_file_location('paired_helpers', Path(__file__).with_name('native-paired.py'))
assert _spec is not None and _spec.loader is not None
_helpers = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_helpers)
BUILDERS, snapshot, source_check, utilization = (
    _helpers.BUILDERS, _helpers.snapshot, _helpers.source_check, _helpers.utilization)

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / 'direct-ghostty'
REFERENCE = Path('/home/callen/work/hotaru-unicode-mode-20261028')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    OUT.mkdir(exist_ok=True)
    observations = []
    stop = threading.Event()
    observer = None
    report = {'schema': 1, 'status': 'preparing', 'cpu': 8, 'observed_siblings': [8, 72],
              'scope': 'direct native zero-history feed comparison, common reviewed4corpora; not GUI or universal parity',
              'commands': [], 'results': []}
    try:
        with (Path.home() / '.cache/hotaru-workstation.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            allowed = os.sched_getaffinity(0)
            os.sched_setaffinity(0, {8})
            first = snapshot()
            time.sleep(1)
            second = snapshot()
            report['preflight'] = {'before': first, 'after': second, 'active_percent': utilization(first, second)}
            if any(p['name'] in BUILDERS for p in second['processes']) or any(
                    (report['preflight']['active_percent'][f'cpu{cpu}'] or 0) > 10 for cpu in [8, 72]):
                report['status'] = 'rejected_preflight_no_timings'
                return
            build = json.loads((ROOT / 'evidence/native-build.json').read_text())
            source_check(ROOT, build)
            assert sha(ROOT / 'target/release/hotaru-replay') == build['binary_sha256']
            assert sha(REFERENCE / 'ghostty-reference-replay') == 'dc687741377b3058f7b06bcdf287df9990c183eadd571a1fbe835fb3869625cd'
            report['candidate_build'] = build
            report['reference_build'] = json.loads((REFERENCE / 'reference-build.json').read_text())
            report['runner_sha256'] = sha(Path(__file__))
            report['benchmark_helper_sha256'] = sha(ROOT / 'tools/benchmark.py')

            def observe():
                os.sched_setaffinity(threading.get_native_id(), {max(allowed)})
                prior = snapshot()
                with (OUT / 'host-observations.jsonl').open('w') as stream:
                    while not stop.wait(0.5):
                        current = snapshot()
                        item = {'snapshot': current, 'active_percent': utilization(prior, current)}
                        observations.append(item)
                        stream.write(json.dumps(item) + '\n')
                        stream.flush()
                        prior = current

            observer = threading.Thread(target=observe)
            observer.start()
            for mode in ['legacy', 'unicode']:
                for workload in ['ascii', 'unicode', 'styled', 'cursor']:
                    destination = OUT / f'{mode}-{workload}.json'
                    command = ['python3', str(ROOT / 'tools/benchmark.py'),
                        '--library', '/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so',
                        '--reference', str(REFERENCE / 'ghostty-reference-replay'),
                        '--baseline-metadata', str(REFERENCE / 'reference-build.json'),
                        '--replay', str(ROOT / 'target/release/hotaru-replay'),
                        '--candidate-metadata', str(ROOT / 'evidence/native-build.json'),
                        '--revision', '842d1e2-identical-source-7e36bc31',
                        '--data', f'/home/callen/work/hotaru-rep-last-cell-20261028/measurements/{workload}.vt',
                        '--report', str(destination), '--rows', '12', '--cols', '120',
                        '--chunk', '65536', '--grapheme-width', mode, '--cpu', '8',
                        '--rounds', '6', '--iterations', '10', '--warmup', '3',
                        # The outer shared lock protects the entire window.
                        # A distinct inner lock avoids recursively flocking it.
                        '--lock', str(OUT / 'inner-helper.lock')]
                    report['commands'].append(command)
                    subprocess.run(command, check=True)
                    observed_builders = [p for item in observations for p in item['snapshot']['processes']
                                         if p['name'] in BUILDERS]
                    if observed_builders:
                        report['observed_builders'] = observed_builders
                        raise RuntimeError('external compiler observed; timing not accepted')
                    case = json.loads(destination.read_text())
                    assert case['performance_comparable'] and case['state_matched']
                    stats = {engine: {'median': statistics.median(values['samples_ns']),
                             'p95': sorted(values['samples_ns'])[56], 'maximum': max(values['samples_ns'])}
                             for engine, values in case['measurements'].items()}
                    pairs = []
                    for at in range(0, 60, 10):
                        pairs.append(statistics.median(case['measurements']['hotaru']['samples_ns'][at:at+10]) /
                                     statistics.median(case['measurements']['ghostty']['samples_ns'][at:at+10]))
                    report['results'].append({'mode': mode, 'workload': workload, 'statistics_ns': stats,
                        'hotaru_over_ghostty': {key: stats['hotaru'][key] / stats['ghostty'][key] for key in stats['hotaru']},
                        'per_round_median_ratios': pairs, 'report': str(destination), 'report_sha256': sha(destination)})
            source_check(ROOT, build)
            assert sha(ROOT / 'target/release/hotaru-replay') == build['binary_sha256']
            report['status'] = 'completed'
    except Exception as error:
        report['status'] = 'failed_or_contaminated_not_acceptance'
        report['error'] = repr(error)
        raise
    finally:
        stop.set()
        if observer is not None:
            observer.join()
        report['observations'] = len(observations)
        if observations:
            report['observed_frequency_khz'] = {cpu: [min(x['snapshot']['frequency_khz'].get(cpu, 0) for x in observations),
                                                   max(x['snapshot']['frequency_khz'].get(cpu, 0) for x in observations)]
                                                 for cpu in ['cpu8', 'cpu72']}
        (OUT / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'status': report['status'], 'report': str(OUT / 'summary.json')}))


if __name__ == '__main__':
    main()
