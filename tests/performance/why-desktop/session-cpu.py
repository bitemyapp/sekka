#!/usr/bin/env python3
"""CPU each process of the user's session had used by the start of the
idle window (65 s after the Enter), from the desktop comparison's
per-thread snapshots (idle-tasks-start.tsv.xz: utime and stime in clock
ticks of 10 ms, cumulative since each thread started). Every process in
user-1000.slice started at or after the login, so this is the CPU of the
login, of settling, and of the first minute; threads that had already
exited are missing (they are counted nowhere).

Also the idle window's wakeups and CPU per process (start→end snapshots).

Usage: session-cpu.py TRIALS_DIR OUT.json
"""
import json, lzma, os, sys, collections, statistics as st

def rows(path):
    out = []
    for line in lzma.open(path, 'rt'):
        p = line.rstrip('\n').split('\t')
        if len(p) < 7:
            continue
        pid = p[0].split('/')[0]
        out.append(dict(pid=pid, tid=p[0], comm=p[1], vol=int(p[2]), invol=int(p[3]), ut=int(p[4]), st=int(p[5]), cg=p[6]))
    return out

def proc_names(trial):
    # process name per pid from the memory snapshot at 60 s (argv0 basename)
    names = {}
    f = os.path.join(trial, 'mem-desktop60.tsv.xz')
    for line in lzma.open(f, 'rt'):
        p = line.rstrip('\n').split('\t')
        if len(p) >= 13 and p[0].isdigit():
            names[p[0]] = os.path.basename(p[12]) or p[3]
    return names

def label(name, cg):
    n = name.lstrip('.').replace('-wrapped', '').replace('-wrapp', '')
    return n

def main():
    trials, out = sys.argv[1], sys.argv[2]
    res = {}
    for desk in ('aizome', 'gnome', 'plasma', 'baseline'):
        per = collections.defaultdict(list)
        idle = collections.defaultdict(list)
        totals = []
        for tag in ('r1', 'r2', 'r3'):
            t = os.path.join(trials, f'{desk}-4096-{tag}')
            names = proc_names(t)
            start = rows(os.path.join(t, 'idle-tasks-start.tsv.xz'))
            end = {r['tid']: r for r in rows(os.path.join(t, 'idle-tasks-end.tsv.xz'))}
            acc = collections.Counter(); wk = collections.Counter(); icpu = collections.Counter()
            for r in start:
                if 'user-1000.slice' not in r['cg']:
                    continue
                name = label(names.get(r['pid'], r['comm']), r['cg'])
                acc[name] += (r['ut'] + r['st']) * 10  # ms
                e = end.get(r['tid'])
                if e:
                    wk[name] += (e['vol'] - r['vol']) / 120.0
                    icpu[name] += ((e['ut'] + e['st']) - (r['ut'] + r['st'])) * 10 / 120.0
            totals.append(sum(acc.values()))
            for k, v in acc.items():
                per[k].append(v)
            for k, v in wk.items():
                idle[k].append((v, icpu[k]))
        if not per:
            continue
        med = {k: st.median(v + [0] * (3 - len(v))) for k, v in per.items()}
        top = sorted(med.items(), key=lambda kv: -kv[1])[:25]
        wkm = {k: (st.median([a for a, b in v]), st.median([b for a, b in v])) for k, v in idle.items()}
        topw = sorted(wkm.items(), key=lambda kv: -kv[1][0])[:15]
        res[desk] = dict(session_cpu_ms_total_median=st.median(totals),
                         session_cpu_ms_by_process=dict(top),
                         idle_wakeups_and_cpu_ms_per_s_by_process={k: [round(a, 1), round(b, 3)] for k, (a, b) in topw})
        print(f'== {desk}: session CPU by 65 s after Enter (median of 3): {st.median(totals)/1000:.2f} s')
        for k, v in top[:18]:
            print(f'   {k:32s} {v/1000:6.2f} s')
        print('   idle: wakeups/s, CPU ms/s')
        for k, (a, b) in topw[:12]:
            print(f'   {k:32s} {a:6.1f}  {b:6.3f}')
    json.dump(res, open(out, 'w'), indent=1)

main()
