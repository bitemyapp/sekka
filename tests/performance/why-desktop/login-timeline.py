#!/usr/bin/env python3
"""Login timelines from the desktop comparison's trials: what each desktop
did between the Enter and a settled screen, on one clock.

The guest journal (CLOCK_MONOTONIC) is put on the Enter's time through the
harness's clock offset (result.json: login.host_enter + clock.offset_s), and
the screendump record (frames.json: seconds since the Enter, pixels changed
since the previous dump, pixels differing from the settled reference) is
merged in. Prints the screen's changes and every journal entry from the
user's session (or naming a unit start) from the Enter to END seconds.

Usage: login-timeline.py TRIAL_DIR [END_S] [--all]
"""
import json, lzma, sys, os

trial = sys.argv[1]
end = float(sys.argv[2]) if len(sys.argv) > 2 and not sys.argv[2].startswith('--') else 6.0
show_all = '--all' in sys.argv
res = json.load(lzma.open(os.path.join(trial, 'result.json.xz'), 'rt'))
enter = res['login']['host_enter'] + res['clock']['offset_s']
frames = json.load(lzma.open(os.path.join(trial, 'frames.json.xz'), 'rt'))['login']
events = []
for line in lzma.open(os.path.join(trial, 'journal.json.xz'), 'rt'):
    try:
        e = json.loads(line)
    except json.JSONDecodeError:
        continue
    t = int(e['__MONOTONIC_TIMESTAMP']) / 1e6 - enter
    if -0.3 <= t <= end:
        events.append((t, e))

rows = []
for t, ch, ref, before in frames:
    if t <= end and ch > 2500:
        rows.append((t, f"SCREEN changed {ch:>8d} px; {ref:>8d} px from the settled desktop"))
for t, e in events:
    msg = str(e.get('MESSAGE', ''))
    uid = e.get('_UID')
    comm = e.get('_COMM') or e.get('SYSLOG_IDENTIFIER') or '?'
    uu = e.get('USER_UNIT') or e.get('_SYSTEMD_USER_UNIT') or e.get('UNIT') or e.get('_SYSTEMD_UNIT') or ''
    job = e.get('JOB_TYPE')
    keep = show_all or job or uid == '1000' or 'demo' in msg or e.get('_SYSTEMD_USER_UNIT') \
        or comm in ('gdm', 'gdm-session-worker', 'greetd', 'plasmalogin', 'kwin_wayland', 'gnome-shell', 'systemd-logind')
    if keep:
        rows.append((t, f"{comm[:22]:22s} {uu[:44]:44s} {msg[:150]}"))
rows.sort(key=lambda r: r[0])
# Collapse repeats of one message (Mesa's and QML's chatter) into a count.
noise = ('Ignoring duplicate name', 'is not named after the D-Bus name', 'removed \'/home/demo/.cache/ksycoca')
last, count = None, 0
def flush():
    if last is not None:
        t, s = last
        print(f"{t:7.3f}  {s}" + (f"   [x{count}]" if count > 1 else ""))
for t, s in rows:
    if any(n in s for n in noise):
        continue
    if last is not None and s == last[1]:
        count += 1
        continue
    flush()
    last, count = (t, s), 1
flush()
