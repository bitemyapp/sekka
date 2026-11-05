#!/usr/bin/env python3
"""Side-by-side images of the PDFium pixel tests Shiori's renderer (the hayro
fork) misses at 1%: PDFium's expected image, the fork's drawing, and a
difference map, with the starkest divergence circled in red on each.

  make.py PDF_WORK OUT_DIR

PDF_WORK holds pixel/pixel-tests.tsv, pixel/gen/*.pdf, bin-pixel/,
src/pdfium (testing/ at chromium/8110) and test_fonts_dl/test_fonts.
"""
import html
import os
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

WORK, OUT = sys.argv[1], sys.argv[2]
PIXEL = os.path.join(WORK, "src/pdfium/testing/resources/pixel")
FONTS = os.path.join(WORK, "test_fonts_dl/test_fonts")
FORK = os.path.join(WORK, "bin-pixel/pdfbench-hayro-fork-pixel")
PDFIUM = os.path.join(WORK, "bin-pixel/pdfbench-pdfium-pixel")
SUFFIXES = ["_agg_linux", "_agg", "_linux", ""]
os.makedirs(OUT, exist_ok=True)
LABEL_FONT = ImageFont.truetype(os.path.join(FONTS, "Arimo-Bold.ttf"), 15)
SMALL_FONT = ImageFont.truetype(os.path.join(FONTS, "Arimo-Regular.ttf"), 13)


def read_ppm(path):
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.int16)


def read_expected(path):
    im = Image.open(path)
    if im.mode in ("RGBA", "LA", "P"):
        im = im.convert("RGBA")
        bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
        im = Image.alpha_composite(bg, im)
    return np.asarray(im.convert("RGB"), dtype=np.int16)


def find_test(rel):
    d = os.path.join(PIXEL, os.path.dirname(rel))
    stem = os.path.basename(rel).rsplit(".", 1)[0]
    expected = []
    for page in range(100):
        found = next((os.path.join(d, f"{stem}_expected{s}.pdf.{page}.png") for s in SUFFIXES
                      if os.path.exists(os.path.join(d, f"{stem}_expected{s}.pdf.{page}.png"))), None)
        if not found:
            break
        expected.append(found)
    return d, stem, expected


def render(program, pdf, out, font_dir, no_rename):
    os.makedirs(out, exist_ok=True)
    env = dict(os.environ)
    if no_rename:
        env["PDFBENCH_NO_RENAME"] = "1"
    p = subprocess.run(["timeout", "30", program, pdf, out, font_dir], capture_output=True, env=env)
    return p.returncode, p.stdout.decode("utf-8", "replace") + p.stderr.decode("utf-8", "replace")


def starkest(diff_mask):
    """Bounding box of the densest cluster of differing pixels."""
    h, w = diff_mask.shape
    b = max(4, min(h, w) // 40)
    gh, gw = (h + b - 1) // b, (w + b - 1) // b
    pad = np.zeros((gh * b, gw * b), dtype=bool)
    pad[:h, :w] = diff_mask
    grid = pad.reshape(gh, b, gw, b).sum(axis=(1, 3))
    if grid.max() == 0:
        return None
    seed = np.unravel_index(np.argmax(grid), grid.shape)
    keep = grid >= max(1, grid.max() * 0.15)
    # Bridge gaps of a block or two (between words, along a dashed edge),
    # walking through the dilated mask but boxing only real cells.
    walk = keep.copy()
    for _ in range(2):
        grown = walk.copy()
        grown[1:, :] |= walk[:-1, :]; grown[:-1, :] |= walk[1:, :]
        grown[:, 1:] |= walk[:, :-1]; grown[:, :-1] |= walk[:, 1:]
        walk = grown
    seen = np.zeros_like(keep)
    stack = [seed]
    seen[seed] = True
    cells = []
    while stack:
        y, x = stack.pop()
        cells.append((y, x))
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                ny, nx = y + dy, x + dx
                if 0 <= ny < gh and 0 <= nx < gw and walk[ny, nx] and not seen[ny, nx]:
                    seen[ny, nx] = True
                    stack.append((ny, nx))
    cells = [c for c in cells if keep[c]]
    ys = [c[0] for c in cells]
    xs = [c[1] for c in cells]
    return (min(xs) * b, min(ys) * b, min(w, (max(xs) + 1) * b), min(h, (max(ys) + 1) * b))


def circle(img, box, scale):
    if box is None:
        return
    x0, y0, x1, y1 = (v * scale for v in box)
    pad = 10 * scale
    d = ImageDraw.Draw(img)
    width = max(3, 2 * scale)
    d.ellipse((x0 - pad, y0 - pad, x1 + pad, y1 + pad), outline=(230, 20, 20), width=width)


def panel(arr, title, scale, box, note=None):
    im = Image.fromarray(arr.astype(np.uint8), "RGB")
    if scale > 1:
        im = im.resize((im.width * scale, im.height * scale), Image.NEAREST)
    circle(im, box, scale)
    head = 26 if not note else 44
    out = Image.new("RGB", (max(im.width, 220), im.height + head), (255, 255, 255))
    d = ImageDraw.Draw(out)
    d.text((4, 4), title, fill=(20, 20, 20), font=LABEL_FONT)
    if note:
        d.text((4, 24), note, fill=(90, 90, 90), font=SMALL_FONT)
    out.paste(im, (0, head))
    # A thin frame so white pages stand out from the white sheet.
    ImageDraw.Draw(out).rectangle((0, head, im.width - 1, head + im.height - 1), outline=(160, 160, 160))
    return out


def placeholder(text, size):
    w, h = size
    im = Image.new("RGB", (max(w, 220), h + 26), (255, 255, 255))
    d = ImageDraw.Draw(im)
    d.text((4, 4), "Shiori (hayro fork)", fill=(20, 20, 20), font=LABEL_FONT)
    d.rectangle((0, 26, w - 1, 26 + h - 1), outline=(230, 20, 20), width=3)
    d.multiline_text((12, 40), text, fill=(150, 20, 20), font=LABEL_FONT)
    return im


def sheet(panels):
    gap = 16
    w = sum(p.width for p in panels) + gap * (len(panels) - 1)
    h = max(p.height for p in panels)
    out = Image.new("RGB", (w, h), (255, 255, 255))
    x = 0
    for p in panels:
        out.paste(p, (x, 0))
        x += p.width + gap
    return out


rows = []
for line in open(os.path.join(WORK, "pixel/pixel-tests.tsv")).read().splitlines()[1:]:
    c = line.split("\t")
    if len(c) >= 12 and c[11].strip():
        rows.append({"test": c[0], "frac": c[6], "category": c[11].split(":")[0], "note": c[11]})

items = []
for row in rows:
    rel = row["test"]
    key = rel.replace("/", "__").rsplit(".", 1)[0]
    pdf = os.path.join(WORK, "pixel/gen", key + ".pdf")
    d, stem, expected = find_test(rel)
    font_dir = FONTS
    if "use_ahem" in d:
        font_dir = os.path.join(WORK, "src/pdfium/testing/resources/fonts/ahem")
    elif "use_symbolneu" in d:
        font_dir = os.path.join(WORK, "src/pdfium/testing/resources/fonts/symbolneu")
    rend = os.path.join(OUT, "render", key)
    code_f, log_f = render(FORK, pdf, os.path.join(rend, "fork"), font_dir, "use_" in d)
    code_p, _ = render(PDFIUM, pdf, os.path.join(rend, "pdfium"), font_dir, "use_" in d)
    pages = []
    for i, exp in enumerate(expected):
        e = read_expected(exp)
        fork_ppm = os.path.join(rend, "fork", f"{i}.ppm")
        scale = 2 if max(e.shape[:2]) < 420 else 1
        if not os.path.exists(fork_ppm):
            why = "refused to open the file" if code_f not in (0,) else "drew no page"
            img = sheet([panel(e, "PDFium's expected image", scale, None),
                         placeholder(f"hayro {why}\n(exit {code_f})", (e.shape[1] * scale, e.shape[0] * scale))])
            frac, box = None, None
        else:
            f = read_ppm(fork_ppm)
            h, w = min(e.shape[0], f.shape[0]), min(e.shape[1], f.shape[1])
            diff = np.abs(e[:h, :w] - f[:h, :w]).max(axis=2)
            mask = diff > 48
            size_note = None
            if e.shape[:2] != f.shape[:2]:
                size_note = f"expected {e.shape[1]}×{e.shape[0]}, fork drew {f.shape[1]}×{f.shape[0]}"
                # The starkest divergence is the page area one of them lacks.
                full = np.zeros((max(e.shape[0], f.shape[0]), max(e.shape[1], f.shape[1])), dtype=bool)
                full[:h, :w] = mask
                full[h:, :] = True
                full[:, w:] = True
                mask_for_box = full
            else:
                mask_for_box = mask
            box = starkest(mask_for_box)
            if box is None:
                box = starkest(diff > 16)
            frac = float(mask.mean()) if mask.size else None
            # Difference map: grey drawing, red where the fork is off by more than 48.
            base = (e[:h, :w].mean(axis=2) * 0.35 + 160).clip(0, 255)
            heat = np.stack([base, base, base], axis=2)
            heat[mask] = (220, 30, 30)
            soft = (diff > 16) & ~mask
            heat[soft] = (245, 170, 60)
            img = sheet([panel(e, "PDFium's expected image", scale, box, size_note),
                         panel(f, "Shiori (hayro fork)", scale, box),
                         panel(heat, "Difference", scale, box, "red: off by >48 levels; orange: >16")])
        name = f"{key}.{i}.png"
        img.save(os.path.join(OUT, name), optimize=True)
        pages.append({"png": name, "frac": frac, "scale": scale})
    items.append({**row, "key": key, "pages": pages, "fork_exit": code_f, "pdfium_exit": code_p})

order = ["drawn differently", "no appearance stream", "damaged content", "page size", "not opened", "edges"]
titles = {
    "drawn differently": "Drawn differently: content missing, extra, or in other colours or places",
    "no appearance stream": "No appearance stream: annotations and form fields PDFium draws an appearance for and hayro leaves blank",
    "damaged content": "Damaged content: a stream whose /Length is wrong, which PDFium reads to its endstream and hayro cuts short",
    "page size": "Page size: no valid page box, so hayro falls back to A4 and PDFium to US Letter (or they choose different boxes)",
    "not opened": "Not opened: damaged files PDFium repairs and hayro refuses",
    "edges": "Edges: glyph and stroke edges at 72 dpi, the drawing otherwise the same",
}
parts = ["""<!doctype html><meta charset="utf-8"><title>Shiori's renderer against PDFium's pixel tests</title>
<style>body{font:15px/1.5 system-ui,sans-serif;margin:24px;background:#f4f4f6;color:#222;max-width:1500px}
h1{font-size:22px}h2{font-size:18px;margin-top:40px;border-top:1px solid #ccc;padding-top:16px}
figure{background:#fff;border:1px solid #ddd;margin:16px 0;padding:12px}figcaption{margin-bottom:8px}
code{background:#eee;padding:0 3px}img{max-width:100%;height:auto;display:block}</style>
<h1>Shiori's renderer (hayro, Aizome's fork a2e9e62b) against PDFium's own pixel tests</h1>
<p>The 50 tests of PDFium's <code>testing/resources/pixel</code> (chromium/8110) where the fork differs from PDFium's
expected image on more than 1% of pixels, drawn on runnymede with the same programs, settings and fonts as the
conformance run in <code>docs/performance/pdf.md</code>. Left: PDFium's expected image. Middle: the fork's drawing.
Right: where they differ (red: a channel off by more than 48 levels, orange: more than 16). The red circle marks the
densest cluster of differences. Small pages are shown at 2× with no smoothing. PDFium itself (8110, the control)
matches all 182 applicable tests within 1%.</p>"""]
for cat in order:
    group = [it for it in items if it["category"] == cat]
    if not group:
        continue
    parts.append(f"<h2>{html.escape(titles[cat])} ({len(group)})</h2>")
    for it in sorted(group, key=lambda x: -(float(x["frac"]) if x["frac"] else 1.0)):
        for pg in it["pages"]:
            frac = "n/a" if pg["frac"] is None else f"{pg['frac'] * 100:.1f}% of pixels off by more than 48"
            parts.append(f"""<figure><figcaption><code>{html.escape(it['test'])}</code>, page {pg['png'].rsplit('.', 2)[1]}: {frac}{' (shown at 2×)' if pg['scale'] > 1 else ''}</figcaption><img src="{html.escape(pg['png'])}" alt="{html.escape(it['test'])}"></figure>""")
open(os.path.join(OUT, "index.html"), "w").write("\n".join(parts))
print(len(items), "tests,", sum(len(i["pages"]) for i in items), "pages")
