# Sekka brand assets: the yukiwa crest, lockups and family icons, as SVG.
#
# Text is converted to outlines, so the files need no installed fonts. From
# the repository root, with the root flake's own Nixpkgs:
#
#   python=$(nix build --no-link --print-out-paths --impure --expr '(builtins.getFlake
#     (toString ./.)).inputs.nixpkgs.legacyPackages.x86_64-linux.python3.withPackages (ps: [ ps.fonttools ])')
#   mono=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#jetbrains-mono)
#   cjk=$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#noto-fonts-cjk-serif)
#   "$python/bin/python3" docs/brand/generate.py docs/brand \
#     "$mono/share/fonts/truetype/JetBrainsMono-ExtraBold.ttf" \
#     "$cjk/share/fonts/opentype/noto-cjk/NotoSerifCJK-VF.otf.ttc"
import math, os, sys
from fontTools.ttLib import TTFont, TTCollection
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen

# Tatami's Tokyo Night colours (desktop/tatami/config) and Tokyo Night Day.
NIGHT, STORM, TEXT, SNOW, BLUE, MUTED = "#1a1b26", "#24283b", "#a9b1d6", "#c0caf5", "#7aa2f7", "#565f89"
INDIGO = "#3d59a1"                # Tokyo Night blue0: the noren cloth
# Day: surface, accent (the crest's ink, 4.5:1 or more on every Day surface)
# and text, from tokens.json.
DAY, DAY_BLUE, DAY_TEXT = "#e1e2e7", "#1357b5", "#3156ab"

def pt(r, deg, c=(128.0, 128.0)):
    a = math.radians(deg)
    return (c[0] + r * math.cos(a), c[1] - r * math.sin(a))

def f(p):
    return f"{p[0]:.2f} {p[1]:.2f}"

def hexafoil(d=36, r=76, first=90):
    """Six circles of radius r, centres d from the middle, unioned: the yukiwa."""
    b = 2 * d * math.cos(math.radians(30))
    t = (b + math.sqrt(b * b - 4 * (d * d - r * r))) / 2
    cusps = [first + 30 + 60 * k for k in range(6)]
    path = f"M{f(pt(t, cusps[-1]))}"
    for k in range(6):
        path += f"A{r:.2f} {r:.2f} 0 0 0 {f(pt(t, cusps[k]))}"
    return path + "Z"

def hexagon(r, first=90):
    return "M" + "L".join(f(pt(r, first + 60 * k)) for k in range(6)) + "Z"

def spokes(start=19, end=84, twig_at=60, twig=20, first=90):
    segs = []
    for k in range(6):
        a = first + 60 * k
        segs.append(f"M{f(pt(start, a))}L{f(pt(end, a))}")
        if twig:
            base = pt(twig_at, a)
            for side in (60, -60):
                segs.append(f"M{f(base)}L{f(pt(twig, a + side, base))}")
    return "".join(segs)

def crest(ink, mid="crest", spoke_w=12, hex_r=19, hex_w=10, twig=20, end=84):
    """The solid crest with the crystal cut out (transparent), as SVG elements."""
    return (
        f'<mask id="{mid}" maskUnits="userSpaceOnUse" x="0" y="0" width="256" height="256">'
        f'<path d="{hexafoil()}" fill="#fff"/>'
        f'<path d="{spokes(hex_r, end, 60, twig)}" stroke="#000" stroke-width="{spoke_w}" stroke-linecap="round" fill="none"/>'
        f'<path d="{hexagon(hex_r)}" fill="#fff" stroke="#000" stroke-width="{hex_w}" stroke-linejoin="round"/>'
        f'</mask><rect width="256" height="256" fill="{ink}" mask="url(#{mid})"/>'
    )

def line_crest(ring=BLUE, snow=SNOW, ground=NIGHT):
    return (
        f'<path d="{hexafoil(36, 72)}" fill="none" stroke="{ring}" stroke-width="12" stroke-linejoin="round"/>'
        f'<path d="{spokes(19, 76, 54, 18)}" stroke="{snow}" stroke-width="11" stroke-linecap="round" fill="none"/>'
        f'<path d="{hexagon(19)}" fill="{ground}" stroke="{snow}" stroke-width="10" stroke-linejoin="round"/>'
        f'<path d="{hexagon(7)}" fill="{ring}"/>'
    )

def svg(w, h, body, title, desc=None):
    d = f"<desc>{desc}</desc>" if desc else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:g}" height="{h:g}" viewBox="0 0 {w:g} {h:g}" '
            f'role="img" aria-label="{title}"><title>{title}</title>{d}{body}</svg>\n')

class Face:
    def __init__(self, font, location=None):
        self.font = font
        self.glyphs = font.getGlyphSet(location=location) if location else font.getGlyphSet()
        self.cmap = font.getBestCmap()
        self.upm = font["head"].unitsPerEm

    def run(self, text, size, x, baseline, tracking=0.0, vertical=False):
        """Outline text; returns (path, ink bounds in SVG space)."""
        s = size / self.upm
        pen = SVGPathPen(self.glyphs)
        bounds = BoundsPen(self.glyphs)
        cx, cy = x, baseline
        for ch in text:
            g = self.cmap[ord(ch)]
            m = (s, 0, 0, -s, cx, cy)
            self.glyphs[g].draw(TransformPen(pen, m))
            self.glyphs[g].draw(TransformPen(bounds, m))
            if vertical:
                cy += size + tracking
            else:
                cx += self.glyphs[g].width * s + tracking
        return pen.getCommands(), bounds.bounds

def main(out, mono_path, cjk_path):
    mono = Face(TTFont(mono_path))
    jp = next(ft for ft in TTCollection(cjk_path).fonts
              if "JP" in ft["name"].getDebugName(1))
    serif = Face(jp, location={"wght": 600})
    os.makedirs(f"{out}/logos", exist_ok=True)
    os.makedirs(f"{out}/family", exist_ok=True)
    W = lambda name, text: open(f"{out}/{name}", "w").write(text)

    # Crests, single ink, transparent.
    desc = "Sekka's yukiwa (snow ring) crest with a six-armed crystal cut out of it."
    for name, ink in (("blue", BLUE), ("snow", SNOW), ("night", NIGHT), ("day", DAY_BLUE)):
        W(f"logos/sekka-crest-{name}.svg", svg(256, 256, crest(ink), "Sekka", desc))
    W("logos/sekka-crest-line.svg", svg(256, 256, line_crest(), "Sekka", "The crest drawn in line, for dark grounds."))
    # 16px symbolic glyph: thicker cuts, no twigs, so it survives the pixel grid.
    W("logos/sekka-symbolic.svg", svg(256, 256, crest("#2e3436", spoke_w=26, hex_r=26, hex_w=22, twig=0, end=90),
                                      "Sekka", "Single-ink glyph for 16 to 24 px; recolour freely."))
    # App icon: a square Night tile, as Tatami's windows are square.
    W("logos/sekka-app-icon.svg", svg(256, 256,
        f'<rect width="256" height="256" fill="{NIGHT}"/><g transform="translate(24 24) scale(0.8125)">{crest(BLUE)}</g>',
        "Sekka", "Application icon: the blue crest on a square Night tile."))

    # Lockups.
    def wordmark(size, x, baseline):
        return mono.run("sekka", size, x, baseline, tracking=-size * 0.02)

    def lockup(ground_name, crest_ink, word_ink, kanji_ink):
        H = 96.0                         # crest height
        word_size = 76.0
        gap = 26.0
        word, wb = wordmark(word_size, 0, 0)
        ink_h = wb[3] - wb[1]
        # Centre the word's ink on the crest.
        base = H / 2 - (wb[1] + wb[3]) / 2
        x0 = H + gap - wb[0]
        word, wb = wordmark(word_size, x0, base)
        # 雪花 set vertically beside the word, as tall as the crest, like a seal.
        k_size = H * 0.44
        kx = wb[2] + gap
        kanji, kb = serif.run("雪花", k_size, kx, 0, tracking=H * 0.04, vertical=True)
        shift = H / 2 - (kb[1] + kb[3]) / 2
        kanji, kb = serif.run("雪花", k_size, kx, shift, tracking=H * 0.04, vertical=True)
        width = kb[2] + 2
        body = (f'<svg x="0" y="0" width="{H}" height="{H}" viewBox="0 0 256 256">{crest(crest_ink, "c-" + ground_name)}</svg>'
                f'<path d="{word}" fill="{word_ink}"/><path d="{kanji}" fill="{kanji_ink}"/>')
        return svg(round(width, 1), H, body, "Sekka 雪花")

    W("logos/sekka-lockup-night.svg", lockup("night", BLUE, SNOW, TEXT))
    W("logos/sekka-lockup-day.svg", lockup("day", DAY_BLUE, NIGHT, DAY_TEXT))

    def stacked(ground_name, crest_ink, word_ink, kanji_ink):
        Wd = 240.0
        C = 132.0
        word, wb = wordmark(64, 0, 0)
        ww = wb[2] - wb[0]
        word, wb = wordmark(64, (Wd - ww) / 2 - wb[0], C + 30 - wb[1])
        kanji, kb = serif.run("雪花", 22, 0, 0, tracking=10)
        kw = kb[2] - kb[0]
        kanji, kb = serif.run("雪花", 22, (Wd - kw) / 2 - kb[0], wb[3] + 22 - kb[1], tracking=10)
        H = kb[3] + 2
        body = (f'<svg x="{(Wd - C) / 2}" y="0" width="{C}" height="{C}" viewBox="0 0 256 256">{crest(crest_ink, "s-" + ground_name)}</svg>'
                f'<path d="{word}" fill="{word_ink}"/><path d="{kanji}" fill="{kanji_ink}"/>')
        return svg(Wd, round(H, 1), body, "Sekka 雪花")

    W("logos/sekka-stacked-night.svg", stacked("night", BLUE, SNOW, TEXT))
    W("logos/sekka-stacked-day.svg", stacked("day", DAY_BLUE, NIGHT, DAY_TEXT))

    for name, ink in (("night", SNOW), ("day", NIGHT)):
        word, wb = wordmark(76, 0, 0)
        word, wb = wordmark(76, -wb[0] + 1, -wb[1] + 1)
        W(f"logos/sekka-wordmark-{name}.svg", svg(round(wb[2] + 1, 1), round(wb[3] + 1, 1),
                                                  f'<path d="{word}" fill="{ink}"/>', "sekka"))

    # Family: Genkan's noren and Tatami's four-and-a-half-mat room.
    noren = f'<rect width="256" height="256" fill="{NIGHT}"/>'
    noren += f'<rect x="28" y="40" width="200" height="12" fill="{TEXT}"/>'
    # Indigo panels hanging from the rod, with the white crest dyed across
    # them and split by the slits, as on a real noren.
    slits = '<rect x="91" y="58" width="12" height="166" fill="#000"/><rect x="153" y="58" width="12" height="166" fill="#000"/>'
    noren += (f'<mask id="noren" maskUnits="userSpaceOnUse" x="0" y="0" width="256" height="256">'
              f'<rect x="36" y="58" width="184" height="166" fill="#fff"/>{slits}</mask>'
              f'<g mask="url(#noren)"><rect width="256" height="256" fill="{INDIGO}"/>'
              f'<g transform="translate(61 72) scale(0.523)">{crest(SNOW, "noren-mark")}</g></g>')
    W("family/genkan.svg", svg(256, 256, noren, "Genkan", "A noren (entrance curtain) with Sekka's crest."))

    u, o, g = 60.0, 38.0, 8.0          # mat unit, offset, heri gap
    def mat(x, y, w, h, fill, stroke):
        return (f'<rect x="{o + x * u + g / 2}" y="{o + y * u + g / 2}" width="{w * u - g}" height="{h * u - g}" '
                f'fill="{fill}" stroke="{stroke}" stroke-width="8"/>')
    tatami = f'<rect width="256" height="256" fill="{NIGHT}"/>'
    for x, y, w, h in ((0, 0, 2, 1), (2, 0, 1, 2), (1, 2, 2, 1), (0, 1, 1, 2)):
        tatami += mat(x, y, w, h, STORM, TEXT)
    tatami += mat(1, 1, 1, 1, BLUE, BLUE)
    W("family/tatami.svg", svg(256, 256, tatami, "Tatami",
                               "Four mats around a half mat: a 4.5-jō room, and tiled windows with the focused one in blue."))

if __name__ == "__main__":
    main(*sys.argv[1:4])
