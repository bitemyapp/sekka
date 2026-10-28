"""Draw Aizome's app icons.

Every app icon shares the mark's tile (aizome-design/logo/svg/aizome-app-icon.svg):
a 60-unit kon square on a 64 grid, a 1-unit bevel highlight, a kachi edge and a
hard 4-unit shadow down and right. Glyphs sit in the tile's inner 40 units on a
2-unit grid, drawn only with the dye ramp (kamenozoki, hanada, ai) and one
amber accent, from rectangles and straight strokes, so they stay crisp at
every integer scale.

    python3 genicons.py OUTDIR
"""
import sys, pathlib

PALE, MID, DEEP, AI = "#a9c4ff", "#7aa2f7", "#4a67b3", "#3d59a1"
AMBER, KON, KACHI, INK = "#e0af68", "#24283b", "#0b0c12", "#1a1b26"
PAPER, LINE2, CYAN, GREEN, RED = "#e6e9f7", "#3b4261", "#7dcfff", "#9ece6a", "#f7768e"


def R(x, y, w, h, fill, extra=""):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"{extra}/>'


def tile(name, title, glyph):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64" role="img" aria-label="{title}">'
        f"<title>{title}</title>"
        + R(4, 4, 60, 60, KACHI)
        + R(0, 0, 60, 60, KON)
        + R(1, 1, 58, 1, "#ffffff", ' fill-opacity=".08"')
        + f'<rect x=".5" y=".5" width="59" height="59" fill="none" stroke="{KACHI}"/>'
        + glyph
        + "</svg>\n"
    )


ICONS = {}


def icon(app_id, title):
    def wrap(f):
        ICONS[app_id] = (title, f())
        return f
    return wrap


@icon("org.aizome.Settei", "Settei (Settings)")
def settings():
    g = ""
    for i, (y, knob_x, colour) in enumerate([(16, 30, AMBER), (30, 14, PALE), (44, 38, MID)]):
        g += R(10, y - 2, 40, 4, LINE2)
        g += R(knob_x - 1, y - 7, 10, 14, KACHI) + R(knob_x, y - 6, 8, 12, colour)
    return g


@icon("org.aizome.Soroban", "Soroban (Calculator)")
def soroban():
    # Three rods. Above the amber beam, one heaven bead each (worth five,
    # counted when down at the beam); below, three earth beads, those
    # counted pushed up to the beam. This one reads 7 2 6.
    g = ""
    for x in (18, 30, 42):
        g += R(x - 1, 13, 2, 34, LINE2)
    g += R(8, 10, 44, 3, PALE) + R(8, 47, 44, 3, PALE) + R(8, 21, 44, 3, AMBER)
    for x, heaven_down, up in ((18, True, 2), (30, False, 2), (42, True, 1)):
        g += R(x - 5, 16 if heaven_down else 13, 10, 5, MID if heaven_down else DEEP)
        for i in range(up):
            g += R(x - 5, 25 + 6 * i, 10, 5, PALE)
        for j in range(3 - up):
            g += R(x - 5, 42 - 6 * j, 10, 5, DEEP)
    return g


@icon("org.aizome.Koyomi", "Koyomi (Calendar)")
def koyomi():
    g = R(10, 12, 40, 38, PAPER) + R(10, 12, 40, 9, AI)
    g += R(17, 8, 4, 8, AMBER) + R(39, 8, 4, 8, AMBER)
    for row in range(3):
        for col in range(4):
            x, y = 14 + col * 9, 25 + row * 8
            g += R(x, y, 6, 5, AMBER if (row, col) == (1, 2) else MID)
    return g


@icon("org.aizome.Tokei", "Tokei (Clock)")
def tokei():
    g = R(10, 10, 40, 40, PALE) + R(13, 13, 34, 34, INK)
    for x, y, w, h in ((29, 15, 2, 5), (29, 40, 2, 5), (15, 29, 5, 2), (40, 29, 5, 2)):
        g += R(x, y, w, h, PALE)
    # Ten past ten: the hour hand to the ten, the minute hand to the two.
    g += f'<path d="M30 30 L21 24" stroke="{MID}" stroke-width="3" stroke-linecap="square"/>'
    g += f'<path d="M30 30 L39 18" stroke="{AMBER}" stroke-width="3" stroke-linecap="square"/>'
    g += R(28, 28, 4, 4, AMBER)
    return g


@icon("org.aizome.Fusen", "Fusen (Notes)")
def fusen():
    g = R(20, 18, 30, 32, MID)
    g += f'<path d="M10 10 H40 V32 L32 40 H10 Z" fill="{AMBER}"/>'
    g += f'<path d="M32 40 V32 H40 Z" fill="#b8790f"/>'
    g += R(14, 16, 20, 2, INK, ' fill-opacity=".55"') + R(14, 22, 22, 2, INK, ' fill-opacity=".55"') + R(14, 28, 14, 2, INK, ' fill-opacity=".55"')
    return g


@icon("org.aizome.Sumi", "Sumi (TextEdit)")
def sumi():
    g = R(12, 8, 36, 44, PAPER)
    for y, w in ((14, 26), (20, 22), (26, 26), (32, 12), (40, 24), (46, 16)):
        g += R(16, y, w, 3, AI)
    g += R(29, 30, 2, 8, AMBER)
    return g


@icon("org.aizome.Tenki", "Tenki (Weather)")
def tenki():
    g = R(28, 10, 18, 18, AMBER)
    for x, y, w, h in ((35, 4, 4, 4), (48, 17, 4, 4), (22, 17, 4, 4), (44, 7, 4, 4)):
        g += R(x, y, w, h, AMBER)
    # A stepped cloud, in front of the sun.
    g += R(14, 26, 14, 8, PALE) + R(22, 22, 14, 12, PALE) + R(8, 34, 44, 14, PALE)
    g += R(8, 44, 44, 4, MID)
    return g


@icon("org.aizome.Tansu", "Tansu (Disks)")
def tansu():
    g = R(10, 8, 40, 44, DEEP) + R(12, 50, 4, 4, DEEP) + R(44, 50, 4, 4, DEEP)
    for i, colour in enumerate((PALE, MID, MID)):
        y = 11 + i * 13
        g += R(13, y, 34, 11, colour) + R(26, y + 4, 8, 3, AMBER)
    return g


@icon("org.aizome.Kura", "Kura (Disk Usage)")
def kura():
    g = R(10, 10, 22, 40, MID)
    g += R(34, 10, 16, 20, PALE)
    g += R(34, 32, 16, 8, DEEP)
    g += R(34, 42, 7, 8, AMBER) + R(43, 42, 7, 8, CYAN)
    g += R(12, 12, 18, 18, AI) + R(12, 32, 8, 16, AI) + R(22, 32, 8, 16, PALE)
    return g


@icon("org.aizome.Hako", "Hako (Files)")
def hako():
    g = R(10, 12, 18, 6, AMBER)
    g += R(10, 16, 40, 34, DEEP)
    g += R(10, 24, 40, 26, PALE)
    g += R(10, 24, 40, 2, "#ffffff", ' fill-opacity=".5"')
    return g


@icon("org.aizome.Myaku", "Myaku (System Monitor)")
def myaku():
    g = ""
    for i, h in enumerate((10, 16, 8, 22, 14, 26, 12, 18)):
        g += R(10 + i * 5, 50 - h, 4, h, DEEP if i % 2 else AI)
    g += f'<path d="M8 30 H18 L22 20 L28 40 L33 14 L38 34 L41 30 H52" fill="none" stroke="{AMBER}" stroke-width="3" stroke-linejoin="miter" stroke-linecap="square"/>'
    return g


@icon("org.aizome.Yukimi", "Yukimi (Software)")
def yukimi():
    # A yukimi-shōji: paper and lattice above, a pane of glass set low,
    # through which snow falls on a ridge at night.
    g = R(10, 8, 40, 44, PAPER)
    for x in (20, 30, 40):
        g += R(x - 1, 8, 2, 20, "#9aa0c2")
    g += R(10, 17, 40, 2, "#9aa0c2")
    g += R(10, 28, 40, 24, INK)
    g += R(10, 27, 40, 2, "#9aa0c2")
    # The snowflake: three crossing strokes.
    for d in ("M30 31 V45", "M24 34 L36 42", "M24 42 L36 34"):
        g += f'<path d="{d}" stroke="{PALE}" stroke-width="2"/>'
    g += R(14, 32, 2, 2, "#ffffff") + R(44, 34, 2, 2, "#ffffff") + R(18, 42, 2, 2, "#ffffff")
    g += R(10, 47, 40, 5, "#ffffff") + R(10, 45, 16, 2, "#ffffff") + R(36, 46, 14, 1, "#ffffff")
    return g


@icon("org.aizome.Shashin", "Shashin (Images)")
def shashin():
    # A print in its white border, a deeper margin below: a stepped
    # mountain under a pale sky, and the amber sun.
    g = R(10, 10, 40, 40, PAPER)
    g += R(13, 13, 34, 28, PALE)
    g += R(34, 16, 8, 8, AMBER)
    for x, y, w in ((13, 35, 34), (17, 31, 22), (21, 27, 12), (25, 23, 4)):
        g += R(x, y, w, 41 - y, AI)
    g += R(33, 33, 14, 8, DEEP) + R(37, 30, 6, 3, DEEP)
    g += R(13, 44, 12, 2, LINE2)
    return g


@icon("org.aizome.Utsushi", "Utsushi (Media Player)")
def utsushi():
    # A frame of film: sprockets along the strip, the picture deep in it,
    # and amber light playing through it.
    g = R(10, 12, 40, 36, MID)
    for x in range(13, 48, 6):
        g += R(x, 14, 3, 3, KON) + R(x, 43, 3, 3, KON)
    g += R(12, 19, 36, 22, AI)
    for i, (y, h) in enumerate(((22, 16), (24, 12), (26, 8), (28, 4))):
        g += R(28 + 2 * i, y, 2, h, AMBER)
    return g


@icon("org.aizome.Annai", "Annai (Welcome)")
def annai():
    # A signpost made of the mark: the amber tab, then boards in the dye
    # ramp pointing on, on a post.
    g = R(28, 10, 4, 42, LINE2)
    g += R(14, 12, 14, 7, AMBER)
    g += f'<path d="M14 21 H44 L48 25 L44 29 H14 Z" fill="{PALE}"/>'
    g += f'<path d="M16 31 H46 V39 H16 L12 35 Z" fill="{MID}"/>'
    g += f'<path d="M14 41 H40 L44 45 L40 49 H14 Z" fill="{DEEP}"/>'
    g += R(22, 50, 16, 2, DEEP)
    return g


@icon("org.aizome.Koe", "Koe (Dictation)")
def koe():
    # A microphone on its stand, the grille in the deepest dye, and the
    # voice beside it as two amber level bars.
    g = R(22, 10, 8, 2, PALE) + R(20, 12, 12, 18, PALE) + R(22, 30, 8, 2, PALE)
    for y in (16, 20, 24):
        g += R(22, y, 8, 2, AI)
    g += R(14, 22, 2, 12, MID) + R(36, 22, 2, 12, MID) + R(16, 34, 20, 2, MID)
    g += R(24, 36, 4, 8, MID) + R(18, 44, 16, 4, DEEP)
    g += R(42, 20, 2, 8, AMBER) + R(46, 16, 2, 16, AMBER)
    return g


@icon("org.aizome.Toolchest", "Toolchest")
def toolchest():
    return R(10, 10, 20, 10, AMBER) + R(10, 20, 40, 10, PALE) + R(10, 30, 40, 10, MID) + R(10, 40, 40, 10, DEEP)


out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
for app_id, (title, glyph) in ICONS.items():
    (out / f"{app_id}.svg").write_text(tile(app_id, title, glyph))
print(f"wrote {len(ICONS)} icons to {out}")
