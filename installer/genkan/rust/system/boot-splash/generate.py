# The boot splash's pieces, as SVG at 1x: the rail, the password field,
# the labels and the Esc hint. Text is outlined from IBM Plex Mono, as
# docs/brand/generate.py outlines the wordmark, so the splash needs no fonts
# for anything it always shows. package.nix rasterizes each file at 1x, 2x
# and 3x; sekka.script picks the scale and lays the pieces out.
#
#   python3 generate.py OUT IBMPlexMono-Regular.ttf IBMPlexMono-Medium.ttf
import os, sys
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

# Tokens: Sekka's (docs/brand/tokens.json) and Aizome's Night palette.
TRACK = "#2f334d"      # Aizome line: the rail's track
ACCENT = "#7aa2f7"     # Sekka accent and crest: the rail's fill
TEXT = "#c0caf5"       # text-strong: what was typed
TEXT2 = "#a9b1d6"      # text: the mode's label, the keycap
TEXT3 = "#939aba"      # text-secondary: the hint
SUNK = "#16161e"       # Aizome sunk, kachi: the field's well
LINE2 = "#3b4261"      # Aizome line2: the field's and keycap's edges
FOCUS = "#e0af68"      # Aizome focus, kohaku: the field's rail and caret
GROUND = "#1a1b26"     # Sekka surface

# The rail is as wide as the wordmark's ink in sekka-stacked-night.svg.
RAIL_W, RAIL_H = 180, 2
FIELD_W, FIELD_H = 260, 34

LABELS = {
    "shutdown": "Shutting down",
    "reboot": "Restarting",
    "updates": "Installing updates",
    "firmware": "Updating firmware",
    "reset": "Resetting the system",
}
HINTS = {
    "boot": "shows the boot messages",
    "shutdown": "shows the shutdown messages",
}


class Face:
    def __init__(self, path):
        self.font = TTFont(path)
        self.glyphs = self.font.getGlyphSet()
        self.cmap = self.font.getBestCmap()
        self.upm = self.font["head"].unitsPerEm
        self.cap = self.font["OS/2"].sCapHeight

    def advance(self, text, size):
        return sum(self.glyphs[self.cmap[ord(c)]].width for c in text) * size / self.upm

    def run(self, text, size, x, baseline):
        s = size / self.upm
        pen = SVGPathPen(self.glyphs)
        for ch in text:
            g = self.cmap[ord(ch)]
            self.glyphs[g].draw(TransformPen(pen, (s, 0, 0, -s, x, baseline)))
            x += self.glyphs[g].width * s
        return pen.getCommands()

    def baseline(self, size, height):
        """The baseline that centres capitals in a line `height` tall."""
        return round((height + self.cap * size / self.upm) / 2)


def svg(w, h, body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{body}</svg>\n'


def rect(x, y, w, h, fill):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"/>'


def label(face, text, size, ink, height):
    w = round(face.advance(text, size)) + 2
    d = face.run(text, size, 1, face.baseline(size, height))
    return svg(w, height, f'<path d="{d}" fill="{ink}"/>')


def keycap(face, key, size, height):
    """Aizome's keycap: the key in mono on the ground, edged in line2 with
    a 3pt bottom edge (aizome-face's look::keycap)."""
    w = round(face.advance(key, size)) + 12
    d = face.run(key, size, 6, face.baseline(size, height - 2))
    body = (rect(0, 0, w, height, LINE2) + rect(1, 1, w - 2, height - 4, GROUND)
            + f'<path d="{d}" fill="{TEXT2}"/>')
    return w, body


def main(out, regular_path, medium_path):
    regular, medium = Face(regular_path), Face(medium_path)
    os.makedirs(out, exist_ok=True)
    W = lambda name, text: open(f"{out}/{name}.svg", "w").write(text)

    W("track", svg(RAIL_W, RAIL_H, rect(0, 0, RAIL_W, RAIL_H, TRACK)))
    W("fill", svg(RAIL_W, RAIL_H, rect(0, 0, RAIL_W, RAIL_H, ACCENT)))

    # The lock screen's password field (aizome-greeter's boxed_field): the
    # well edged in line2, with the focus rail down its left.
    W("field", svg(FIELD_W, FIELD_H, rect(0, 0, FIELD_W, FIELD_H, LINE2)
                   + rect(1, 1, FIELD_W - 2, FIELD_H - 2, SUNK) + rect(0, 0, 2, FIELD_H, FOCUS)))
    W("caret", svg(2, 18, rect(0, 0, 2, 18, FOCUS)))
    # One dot per character typed, a mono advance apart.
    dot_w = round(regular.advance("•", 15))
    W("bullet", svg(dot_w, FIELD_H, f'<path d="{regular.run("•", 15, 0, regular.baseline(15, FIELD_H))}" fill="{TEXT}"/>'))

    for name, text in LABELS.items():
        W(f"label-{name}", label(regular, text, 14, TEXT2, 20))
    W("caps", label(regular, "Caps Lock is on", 13, FOCUS, 20))

    for name, text in HINTS.items():
        kw, cap = keycap(medium, "Esc", 11, 22)
        tw = round(regular.advance(text, 13)) + 2
        d = regular.run(text, 13, kw + 8, regular.baseline(13, 22))
        W(f"hint-{name}", svg(kw + 8 + tw, 22, cap + f'<path d="{d}" fill="{TEXT3}"/>'))


if __name__ == "__main__":
    main(*sys.argv[1:4])
