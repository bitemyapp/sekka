#!/usr/bin/env python3
"""Do a compositor's late presentations come in runs? For each press in
the desktop comparison's latency records: whether its commit missed the
first vblank although it came early enough that a compositor drawing
right away would have made it (phase over 4 ms, "a miss with time to
spare"). A stateful cause (a render-time estimate that stays high for
seconds, or triple buffering held for ten frames) gives runs of such
misses; a random one gives isolated misses.

Prints per trial and condition: the count, the runs, and the probability
that the press after a spare-time miss is one too, against the overall rate.

Usage: latency-streaks.py TRIALS_DIR [OUT.json]
"""
import json, lzma, os, sys

def load(path):
    rows, hdr = [], None
    for line in lzma.open(path, 'rt'):
        if line.startswith('#'):
            continue
        p = line.rstrip('\n').split('\t')
        if hdr is None:
            hdr = p; continue
        rows.append(dict(zip(hdr, p)))
    return rows

out = {}
trials = sys.argv[1]
for desk in ('aizome', 'gnome', 'plasma'):
    for cond in ('idle', 'load'):
        allseq = []
        for tag in ('r1', 'r2', 'r3'):
            f = os.path.join(trials, f'{desk}-4096-{tag}', f'latency-{cond}.tsv.xz')
            seq = []
            for r in load(f):
                if r['state'] != '1':
                    continue
                T = int(r['refresh_ns']); d = int(r['present_ns']) - int(r['commit_ns'])
                k = d // T; ph = d - k * T
                seq.append((ph > 4_000_000, k >= 1 and ph > 4_000_000))
            allseq.append(seq)
        n = sum(1 for s in allseq for e, m in s if e)
        m = sum(1 for s in allseq for e, mm in s if mm)
        # conditional: next eligible press after a spare-time miss
        pairs = after = 0
        runs = []
        for s in allseq:
            el = [mm for e, mm in s if e]
            run = 0
            for i, x in enumerate(el):
                if x:
                    run += 1
                elif run:
                    runs.append(run); run = 0
                if i + 1 < len(el) and x:
                    pairs += 1; after += el[i + 1]
            if run:
                runs.append(run)
        rate = m / n if n else 0
        cond_p = after / pairs if pairs else None
        out[f'{desk}-{cond}'] = dict(eligible=n, spare_time_misses=m, rate=round(rate, 3),
                                     p_next_also=round(cond_p, 3) if cond_p is not None else None,
                                     longest_run=max(runs) if runs else 0, runs=sorted(runs, reverse=True)[:10])
        print(f"{desk}-{cond:4s} spare-time misses {m:3d}/{n:3d} ({rate*100:4.1f}%)  P(next also | miss) "
              f"{(cond_p*100 if cond_p is not None else float('nan')):5.1f}%  longest run {max(runs) if runs else 0}  runs {sorted(runs, reverse=True)[:8]}")
if len(sys.argv) > 2:
    json.dump(out, open(sys.argv[2], 'w'), indent=1)
