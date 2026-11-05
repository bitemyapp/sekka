#!/usr/bin/env python3
"""Where the key-to-screen time goes, from the desktop comparison's probe
records (docs/performance/data/desktops/trials/*/latency-*.tsv.xz).

Every record has the probe's commit time and the presentation time the
compositor reported, both CLOCK_MONOTONIC, and presentation times fall on
the emulated 75 Hz vblanks. So for each press:

  phase  = time from the commit to the first vblank after it (0..T)
  missed = whole refreshes the commit waited beyond that vblank

A compositor that draws as soon as a commit arrives shows a commit at the
first vblank unless the commit came less than its drawing time before it.
One that starts drawing at a fixed point before each vblank (a deadline)
shows commits that came after that point a refresh later. The phase at
which commits start to miss is that compositor's latch point.

Usage: latency-phase.py TRIALS_DIR OUT.json
"""
import json, lzma, sys, os, statistics as st

def load(path):
    rows = []
    with lzma.open(path, 'rt') as f:
        hdr = None
        for line in f:
            if line.startswith('#'):
                continue
            p = line.rstrip('\n').split('\t')
            if hdr is None:
                hdr = p
                continue
            rows.append(dict(zip(hdr, p)))
    return rows

def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return None
    k = (len(xs) - 1) * q
    lo = int(k)
    hi = min(lo + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)

def analyse(rows, inject):
    out = []
    presses = {int(r['idx']): int(r['press_ns']) for r in inject}
    for r in rows:
        if r['state'] != '1':
            continue
        T = int(r['refresh_ns'])
        c = int(r['commit_ns']); p = int(r['present_ns']); rv = int(r['recv_ns'])
        d = p - c
        k = d // T
        phase = d - k * T
        press = presses.get(int(r['idx']))
        out.append(dict(T=T, d=d, k=k, phase=phase, recv=rv - press if press else None,
                        client=c - rv, total=p - press if press else None))
    return out

def summary(recs):
    T = recs[0]['T']
    ms = lambda ns: round(ns / 1e6, 3)
    made = [r['phase'] for r in recs if r['k'] == 0]
    missed = [r['phase'] for r in recs if r['k'] >= 1]
    # bins of phase (0..T) in 1 ms steps: share that missed
    bins = []
    for b in range(0, (T // 1_000_000) + 1):
        lo, hi = b * 1_000_000, (b + 1) * 1_000_000
        inb = [r for r in recs if lo <= r['phase'] < hi]
        if inb:
            bins.append(dict(phase_ms=[b, b + 1], n=len(inb),
                             missed=sum(1 for r in inb if r['k'] >= 1),
                             missed2=sum(1 for r in inb if r['k'] >= 2)))
    return dict(
        n=len(recs), refresh_ms=ms(T),
        key_to_present_p50_ms=ms(pct([r['total'] for r in recs if r['total']], .5)),
        key_to_client_p50_ms=ms(pct([r['recv'] for r in recs if r['recv'] is not None], .5)),
        client_recv_to_commit_p50_ms=ms(pct([r['client'] for r in recs], .5)),
        commit_to_present_p50_ms=ms(pct([r['d'] for r in recs], .5)),
        commit_to_next_vblank_p50_ms=ms(pct([r['phase'] for r in recs], .5)),
        extra_refreshes_mean=round(sum(r['k'] for r in recs) / len(recs), 3),
        share_missing_first_vblank=round(sum(1 for r in recs if r['k'] >= 1) / len(recs), 3),
        share_missing_two=round(sum(1 for r in recs if r['k'] >= 2) / len(recs), 3),
        # the latest a commit made its vblank, and the earliest it missed one
        smallest_phase_that_made_it_ms=ms(min(made)) if made else None,
        largest_phase_that_missed_ms=ms(max(missed)) if missed else None,
        # phase below which >half of commits missed
        latch_estimate_ms=None,
        bins=bins,
    )

def main():
    trials, out_path = sys.argv[1], sys.argv[2]
    result = {}
    for desk in ('aizome', 'gnome', 'plasma'):
        for cond in ('idle', 'load'):
            recs = []
            for tag in ('r1', 'r2', 'r3'):
                d = os.path.join(trials, f'{desk}-4096-{tag}')
                f = os.path.join(d, f'latency-{cond}.tsv.xz')
                if not os.path.exists(f):
                    continue
                recs += analyse(load(f), load(os.path.join(d, f'latency-{cond}.inject.tsv.xz')))
            s = summary(recs)
            # latch: the largest 1 ms phase bin where at least half missed
            lat = [b['phase_ms'][1] for b in s['bins'] if b['missed'] * 2 >= b['n']]
            s['latch_estimate_ms'] = max(lat) if lat else 0
            result[f'{desk}-{cond}'] = s
    json.dump(result, open(out_path, 'w'), indent=1)
    for k, s in result.items():
        print(f"{k:14s} n={s['n']} key->present p50 {s['key_to_present_p50_ms']:6.2f}  key->client {s['key_to_client_p50_ms']:.2f}  client {s['client_recv_to_commit_p50_ms']:.3f}  "
              f"commit->present {s['commit_to_present_p50_ms']:6.2f}  commit->vblank {s['commit_to_next_vblank_p50_ms']:5.2f}  "
              f"missed first vblank {s['share_missing_first_vblank']*100:5.1f}%  two {s['share_missing_two']*100:4.1f}%  "
              f"latch ~{s['latch_estimate_ms']} ms  made-it-min {s['smallest_phase_that_made_it_ms']}  missed-max {s['largest_phase_that_missed_ms']}")
        print('   bins:', ' '.join(f"{b['phase_ms'][0]}:{b['missed']}/{b['n']}" for b in s['bins']))

main()
