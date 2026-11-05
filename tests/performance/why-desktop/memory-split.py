#!/usr/bin/env python3
"""Where each desktop's memory goes, from the desktop comparison's trials
(4 GiB, rounds r1-r3, the snapshot 190 s after the Enter).

1. Per process: PSS split into anonymous, file-backed and shared memory
   (mem-desktop190.tsv.xz, from /proc/PID/smaps_rollup), grouped by role:
     compositor+shell   the compositor and the shell (one process in GNOME)
     x11                Xwayland and the X11 helpers each desktop runs
     desktop-services   the desktop's own helper daemons and portal backend
     config-pim         services Sekka inherits from NixOS's desktop modules
                        that are not the desktop itself: Akonadi and MariaDB
                        (programs.kde-pim), Evolution's data server,
                        localsearch, online accounts, IBus, Baloo, Discover
     common             what every session runs: PipeWire, D-Bus, systemd
                        --user, keyring, generic portals, gvfs, the harness
2. /proc/meminfo against the no-desktop VM at the same time: which kinds
   of memory the desktop's "used" figure (MemTotal - MemAvailable) is made of.

Usage: memory-split.py TRIALS_DIR OUT.json
"""
import json, lzma, os, sys, re, collections, statistics as st

ROLE_RULES = [
    # (desktop or '*', regex on the process name, role)
    ('aizome', r'^aizome$', 'compositor+shell'),
    ('aizome', r'^aizome-shell$', 'compositor+shell'),
    ('gnome', r'^gnome-shell$', 'compositor+shell'),
    ('plasma', r'^kwin_wayland$', 'compositor+shell'),
    ('plasma', r'^plasmashell$', 'compositor+shell'),
    ('*', r'^(Xwayland|xwayland-satellite|mutter-x11-frames|xembedsniproxy|xsettingsd|ibus-x11|kwin_wayland_wrapper)$', 'x11'),
    ('*', r'^(mariadbd|mariadb.*|akonadi.*)$', 'config-pim'),
    ('*', r'^(evolution-.*|gnome-shell-calendar-server|goa-daemon|goa-identity-service|localsearch-3|ibus-.*|baloo_file|DiscoverNotifier|kunifiedpush-distributor)$', 'config-pim'),
    ('*', r'^(python3|pipewire|pipewire-pulse|wireplumber|dbus-broker|dbus-broker-launch|systemd|\(sd-pam\)|gcr-ssh-agent|gnome-keyring-daemon|xdg-desktop-portal|xdg-document-portal|xdg-permission-store|gvfsd|gvfsd-fuse|fusermount3|dconf-service|agent|systemctl|sh|at-spi-bus-launcher|at-spi2-registryd|greetd|gdm-session-worker|plasmalogin-helper|startplasma-wayland|gvfs-.*|xdg-desktop-portal-gtk|aizome-session|gnome-session-.*|gnome-session-binary|gnome-session-ctl|gnome-session-service)$', 'common'),
]

def role(desk, name):
    for d, rx, r in ROLE_RULES:
        if d in ('*', desk) and re.match(rx, name):
            return r
    return 'desktop-services'

def procs(path):
    out = []
    for line in lzma.open(path, 'rt'):
        p = line.rstrip('\n').split('\t')
        if len(p) < 13 or not p[0].isdigit():
            continue
        name = os.path.basename(p[12]).lstrip('.')
        name = re.sub(r'-wrapped$', '', name)
        out.append(dict(pid=p[0], name=name, cg=p[4], rss=int(p[5]), pss=int(p[6]), anon=int(p[7]),
                        file=int(p[8]), shmem=int(p[9])))
    return out

def meminfo(path):
    m = {}
    for line in lzma.open(path, 'rt'):
        k, _, v = line.partition(':')
        v = v.strip().split()
        if v:
            m[k] = int(v[0])
    return m

MI = ['AnonPages', 'Shmem', 'Mapped', 'Cached', 'Buffers', 'Slab', 'SUnreclaim', 'SReclaimable',
      'KernelStack', 'PageTables', 'VmallocUsed', 'Percpu', 'Unevictable', 'Mlocked']

def main():
    trials, out_path = sys.argv[1], sys.argv[2]
    result = {}
    base_mi = []
    for tag in ('r1', 'r2', 'r3'):
        base_mi.append(meminfo(os.path.join(trials, f'baseline-4096-{tag}', 'meminfo-desktop190.txt.xz')))
    bmi = {k: st.median([m[k] for m in base_mi]) for k in MI + ['MemTotal', 'MemAvailable']}
    for desk in ('aizome', 'gnome', 'plasma'):
        roles = collections.defaultdict(lambda: collections.defaultdict(list))
        byproc = collections.defaultdict(lambda: collections.defaultdict(list))
        mis = []
        for when in ('desktop190', 'greeter'):
            pass
        for tag in ('r1', 'r2', 'r3'):
            t = os.path.join(trials, f'{desk}-4096-{tag}')
            acc = collections.defaultdict(lambda: collections.Counter())
            pacc = collections.defaultdict(lambda: collections.Counter())
            for p in procs(os.path.join(t, 'mem-desktop190.tsv.xz')):
                if 'user-1000.slice' not in p['cg']:
                    continue
                r = role(desk, p['name'])
                for k in ('pss', 'anon', 'file', 'shmem', 'rss'):
                    acc[r][k] += p[k]
                    pacc[(r, p['name'])][k] += p[k]
                pacc[(r, p['name'])]['n'] += 1
            for r, c in acc.items():
                for k, v in c.items():
                    roles[r][k].append(v)
            for key, c in pacc.items():
                for k, v in c.items():
                    byproc[key][k].append(v)
            mis.append(meminfo(os.path.join(t, 'meminfo-desktop190.txt.xz')))
        mib = lambda kb: round(kb / 1024, 1)
        med = lambda xs: st.median(xs + [0] * (3 - len(xs)))
        r_out = {r: {k: mib(med(v)) for k, v in c.items()} for r, c in roles.items()}
        p_out = sorted(({'role': r, 'process': n, **{k: (mib(med(v)) if k != 'n' else max(v)) for k, v in c.items()}}
                        for (r, n), c in byproc.items()), key=lambda d: -d['pss'])
        mi = {k: st.median([m[k] for m in mis]) for k in MI + ['MemTotal', 'MemAvailable']}
        used = (mi['MemTotal'] - mi['MemAvailable']) - (bmi['MemTotal'] - bmi['MemAvailable'])
        mi_delta = {k: mib(mi[k] - bmi[k]) for k in MI}
        result[desk] = dict(roles=r_out, processes=p_out[:60], meminfo_over_baseline=mi_delta,
                            used_over_baseline_mib=mib(used))
        print(f'== {desk}: used over baseline {mib(used)} MiB')
        print('   meminfo delta:', ', '.join(f'{k} {v:+.0f}' for k, v in mi_delta.items()))
        tot = collections.Counter()
        for r in ('compositor+shell', 'x11', 'desktop-services', 'config-pim', 'common'):
            c = r_out.get(r, {})
            print(f"   {r:18s} PSS {c.get('pss',0):7.1f}  anon {c.get('anon',0):6.1f}  file {c.get('file',0):6.1f}  shmem {c.get('shmem',0):5.1f}")
            for k in ('pss', 'anon', 'file', 'shmem'):
                tot[k] += c.get(k, 0)
        print(f"   {'session total':18s} PSS {tot['pss']:7.1f}  anon {tot['anon']:6.1f}  file {tot['file']:6.1f}  shmem {tot['shmem']:5.1f}")
        for p in p_out[:22]:
            print(f"      {p['role']:17s} {p['process'][:30]:30s} x{p['n']:<2d} PSS {p['pss']:6.1f} anon {p['anon']:6.1f} file {p['file']:6.1f} shm {p['shmem']:5.1f}")
    json.dump(result, open(out_path, 'w'), indent=1)

main()
