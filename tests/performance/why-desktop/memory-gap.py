#!/usr/bin/env python3
"""The memory gap between Aizome and the other two desktops, in the units of
desktops.md's "memory in use" (MemTotal - MemAvailable), 190 s after login,
4 GiB, medians of r1-r3: anonymous PSS of the session's processes by role,
plus shared memory and the kernel's own (from /proc/meminfo, each desktop
against the no-desktop VM). File-backed pages are page cache the kernel can
reclaim, so they barely enter "in use"; they are reported apart.

Usage: memory-gap.py TRIALS_DIR OUT.json
"""
import json, lzma, os, re, sys, statistics as st, collections

ROLES = [
    ('compositor+shell', r'^(aizome|aizome-shell|gnome-shell|kwin_wayland|plasmashell)$'),
    ('pim+search', r'^(evolution-.*|gnome-shell-calendar-server|goa-daemon|goa-identity-service|localsearch-3|'
                   r'mariadbd|akonadi.*|baloo_file)$'),
    ('input method', r'^ibus-.*$'),
    ('x11 support', r'^(Xwayland|xwayland-satellite|mutter-x11-frames|xembedsniproxy|xsettingsd)$'),
    ('common', r'^(python3|pipewire|pipewire-pulse|wireplumber|dbus-broker|dbus-broker-launch|systemd|\(sd-pam\)|'
               r'gcr-ssh-agent|gnome-keyring-daemon|xdg-desktop-portal|xdg-document-portal|xdg-permission-store|'
               r'xdg-desktop-portal-gtk|gvfsd|gvfsd-fuse|fusermount3|dconf-service|agent|systemctl|sh|'
               r'at-spi-bus-launcher|at-spi2-registryd|greetd|gdm-session-worker|plasmalogin-helper|'
               r'startplasma-wayland|aizome-session|gvfs-.*|gnome-session-.*)$'),
]

def role(name):
    for r, rx in ROLES:
        if re.match(rx, name):
            return r
    return 'desktop helpers'

def procs(path):
    for line in lzma.open(path, 'rt'):
        p = line.rstrip('\n').split('\t')
        if len(p) < 13 or not p[0].isdigit() or 'user-1000.slice' not in p[4]:
            continue
        name = re.sub(r'-wrapped$', '', os.path.basename(p[12]).lstrip('.'))
        yield name, int(p[6]), int(p[7]), int(p[8]), int(p[9])

def meminfo(path):
    m = {}
    for line in lzma.open(path, 'rt'):
        k, _, v = line.partition(':')
        if v.split():
            m[k] = int(v.split()[0])
    return m

trials, out = sys.argv[1], sys.argv[2]
res = {}
for desk in ('aizome', 'gnome', 'plasma', 'baseline'):
    anon = collections.defaultdict(list); pss = collections.defaultdict(list); mi = []
    names = collections.defaultdict(set)
    for tag in ('r1', 'r2', 'r3'):
        t = os.path.join(trials, f'{desk}-4096-{tag}')
        a = collections.Counter(); p = collections.Counter()
        for name, ps, an, fi, sh in procs(os.path.join(t, 'mem-desktop190.tsv.xz')):
            r = role(name); a[r] += an; p[r] += ps; names[r].add(name)
        for r in set(a) | {x for x, _ in ROLES} | {'desktop helpers'}:
            anon[r].append(a[r]); pss[r].append(p[r])
        mi.append(meminfo(os.path.join(t, 'meminfo-desktop190.txt.xz')))
    m = {k: st.median([x[k] for x in mi]) for k in mi[0]}
    res[desk] = dict(
        anon={r: st.median(v) / 1024 for r, v in anon.items()},
        pss={r: st.median(v) / 1024 for r, v in pss.items()},
        used=(m['MemTotal'] - m['MemAvailable']) / 1024,
        shmem=m['Shmem'] / 1024, sunreclaim=m['SUnreclaim'] / 1024, pagetables=m['PageTables'] / 1024,
        kernelstack=m['KernelStack'] / 1024, mapped=m['Mapped'] / 1024, anonpages=m['AnonPages'] / 1024,
        names={r: sorted(v) for r, v in names.items()})
gap = {}
for d in ('gnome', 'plasma'):
    g = {}
    for r in [x for x, _ in ROLES] + ['desktop helpers']:
        g[f'anon: {r}'] = round(res[d]['anon'].get(r, 0) - res['aizome']['anon'].get(r, 0), 1)
    g['shared memory (meminfo Shmem)'] = round(res[d]['shmem'] - res['aizome']['shmem'], 1)
    g['kernel: SUnreclaim + PageTables + KernelStack'] = round(
        sum(res[d][k] - res['aizome'][k] for k in ('sunreclaim', 'pagetables', 'kernelstack')), 1)
    g['sum of the above'] = round(sum(v for v in g.values()), 1)
    g['measured gap in use (MemTotal - MemAvailable)'] = round(res[d]['used'] - res['aizome']['used'], 1)
    g['file-backed mapped pages (not in use; reclaimable)'] = round(res[d]['mapped'] - res['aizome']['mapped'], 1)
    gap[d] = g
json.dump(dict(per_desktop=res, gap_over_aizome=gap), open(out, 'w'), indent=1)
for d in ('aizome', 'gnome', 'plasma'):
    print(d, {r: round(v, 1) for r, v in res[d]['anon'].items()}, 'used', round(res[d]['used'], 1))
for d, g in gap.items():
    print('==', d, 'over aizome')
    for k, v in g.items():
        print(f'   {k:55s} {v:+7.1f}')
print('helpers:', {d: res[d]['names'].get('desktop helpers') for d in ('aizome', 'gnome', 'plasma')})
