#!/usr/bin/env python3
"""Time from launch to the first video frame on screen, under the same
private headless Aizome session as bench.py.

    ttff.py --clip FILE --out DIR [--reps N] [--subjects a,b,c]

Each player opens FILE (a colourful test pattern; see encode notes in
docs/performance/media.md) in a window, not full screen. From the launch,
the harness takes screenshots of a 96 x 54 area at the middle of the
player's window through the compositor (`aizomectl screenshot --area`)
as fast as they come back (every 15-30 ms) and stops at the first one
in which most pixels are strongly coloured: the pattern, not a black
or grey window. The time reported is when that screenshot was asked for,
so it's late by at most one polling interval (also recorded).
"""

import argparse
import json
import os
import random
import struct
import subprocess
import sys
import time
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench  # noqa: E402


def png_pixels(data):
    """Decode an 8-bit RGB/RGBA PNG: (width, height, channels, rows)."""
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w = 8, b"", None
    while pos < len(data):
        n, typ = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if typ == b"IHDR":
            w, h, depth, ctype = struct.unpack(">IIBB", body[:10])
            assert depth == 8 and ctype in (2, 6), (depth, ctype)
            ch = 3 if ctype == 2 else 4
        elif typ == b"IDAT":
            idat += body
        pos += 12 + n
    raw = zlib.decompress(idat)
    stride = w * ch
    rows, prev = [], bytearray(stride)
    i = 0
    for _ in range(h):
        f = raw[i]
        line = bytearray(raw[i + 1:i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - ch] if x >= ch else 0
            b = prev[x]
            c = prev[x - ch] if x >= ch else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line)
        prev = line
    return w, h, ch, rows


def coloured_fraction(path):
    with open(path, "rb") as f:
        w, h, ch, rows = png_pixels(f.read())
    n = sat = 0
    for line in rows:
        for x in range(0, w * ch, ch):
            r, g, b = line[x], line[x + 1], line[x + 2]
            n += 1
            if max(r, g, b) - min(r, g, b) > 120:
                sat += 1
    return sat / n if n else 0


def one(subject, clip, tdir):
    os.makedirs(tdir, exist_ok=True)
    session = bench.Session("aizome", tdir, xwayland=subject.startswith("vlc"))
    rec = {"subject": subject}
    player = None
    try:
        session.start()
        time.sleep(3)
        player = bench.make_player(subject, clip, False, tdir)
        argv = player.argv()
        # Windowed: drop the full-screen flags.
        argv = [a for a in argv if a not in ("--fs", "--fullscreen")]
        player.argv = lambda: argv
        t0 = time.monotonic()
        pid = player.start(session.env)
        win = None
        shot = f"{bench.RUN}/ttff.png"
        polls, last_poll = 0, None
        while time.monotonic() - t0 < 15:
            if win is None:
                pids = set(bench.tree(pid))
                for w in session.windows():
                    if (w["pid"] in pids or (subject.startswith("vlc") and "vlc" in str(w["app_id"]).lower())) \
                            and w["geometry"]["w"] > 200:
                        win = w
                        rec["window_s"] = round(time.monotonic() - t0, 3)
                if win is None:
                    time.sleep(0.01)
                    continue
            g = win["geometry"]
            area = f"{g['x'] + g['w'] // 2 - 48},{g['y'] + g['h'] // 2 - 27},96,54"
            tc = time.monotonic()
            r = bench.run_cmd([f"{bench.AIZOME}/bin/aizomectl", "screenshot", shot, "--area", area], session.env)
            polls += 1
            if r.returncode == 0 and os.path.exists(shot):
                frac = coloured_fraction(shot)
                if frac > 0.5:
                    rec["first_frame_s"] = round(tc - t0, 3)
                    rec["poll_interval_s"] = round(tc - last_poll, 3) if last_poll else None
                    rec["coloured"] = round(frac, 3)
                    break
                # The window may have moved or resized as it mapped.
                for w in session.windows():
                    if w["id"] == win["id"]:
                        win = w
            last_poll = tc
        rec["polls"] = polls
        rec["ok"] = "first_frame_s" in rec
    except Exception as e:  # noqa: BLE001
        rec["ok"] = False
        rec["error"] = f"{type(e).__name__}: {e}"
    finally:
        if player:
            player.stop()
        session.stop()
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--clip", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("--subjects", default="utsushi,mpv-default,mpv-vaapi-gpunext,mpv-vaapi-dmabuf,vlc-default")
    args = ap.parse_args()
    subjects = args.subjects.split(",")
    os.makedirs(args.out, exist_ok=True)
    rng = random.Random(7)
    with open(f"{args.out}/ttff.jsonl", "a") as f:
        for rep in range(args.reps):
            order = list(subjects)
            rng.shuffle(order)
            for s in order:
                rec = one(s, args.clip, f"{args.out}/trials/{rep}-{s}")
                rec["rep"] = rep
                bench.log("ttff", s, rec.get("first_frame_s"), rec.get("window_s"), rec.get("error", ""))
                f.write(json.dumps(rec) + "\n")
                f.flush()
                time.sleep(1)


if __name__ == "__main__":
    main()
