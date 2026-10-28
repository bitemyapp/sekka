#!/usr/bin/env python3
"""Read-only process CPU, proportional RAM and DRM accounting sampler.

Usage: measure_yukimi.py --seconds 30 --output result.json PID [PID ...]
CPU is percent of one logical core. RAM fields are KiB. DRM fields retain
kernel names/units; file descriptors sharing a DRM client count once. Shared
GPU buffers may appear in more than one process: do not sum them as private
VRAM. Integrated graphics uses system memory, not a dedicated VRAM pool.
"""
import argparse
import json
import os
from pathlib import Path
import time


def snapshot(pid):
    proc = Path('/proc') / str(pid)
    stat = (proc / 'stat').read_text().rsplit(')', 1)[1].split()
    ram = {}
    for line in (proc / 'smaps_rollup').read_text().splitlines()[1:]:
        key, value = line.split(':', 1)
        ram[key] = int(value.split()[0])
    drm = {}
    for fd in (proc / 'fdinfo').iterdir():
        fields = {}
        try:
            for line in fd.read_text().splitlines():
                if line.startswith('drm-'):
                    key, value = line.split(':', 1)
                    fields[key] = value.strip()
        except FileNotFoundError:
            continue
        if 'drm-client-id' in fields:
            key = fields.get('drm-pdev', '') + ':' + fields['drm-client-id']
            drm.setdefault(key, fields)
    return {'pid': pid, 'time': time.monotonic(),
            'cpu_ticks': int(stat[11]) + int(stat[12]), 'ram_kib': ram, 'drm': drm}


def measure(pids, seconds):
    hz = os.sysconf('SC_CLK_TCK')
    samples = []
    until = time.monotonic() + seconds
    while True:
        samples.append([snapshot(pid) for pid in pids])
        remaining = until - time.monotonic()
        if remaining <= 0:
            break
        time.sleep(min(1, remaining))
    summary = []
    for i, pid in enumerate(pids):
        first, last = samples[0][i], samples[-1][i]
        summary.append({'pid': pid,
                        'cpu_percent_one_core': 100 * (last['cpu_ticks'] - first['cpu_ticks']) / hz
                                                / (last['time'] - first['time']),
                        'mean_pss_mib': sum(s[i]['ram_kib']['Pss'] for s in samples) / len(samples) / 1024,
                        'peak_rss_mib': max(s[i]['ram_kib']['Rss'] for s in samples) / 1024,
                        'mean_private_mib': sum(s[i]['ram_kib']['Private_Clean'] + s[i]['ram_kib']['Private_Dirty']
                                                for s in samples) / len(samples) / 1024})
    return {'clock_ticks_per_second': hz, 'summary': summary, 'samples': samples}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=float, default=30)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('pids', nargs='+', type=int)
    args = parser.parse_args()
    result = measure(args.pids, args.seconds)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result['summary'], indent=2))
