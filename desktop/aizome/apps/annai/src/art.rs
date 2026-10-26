//! The pages' illustrations: small scenes of Aizome drawn with the kit's
//! canvas in its own look (tabs, frames, menus, the palette, the Deskbar,
//! keycaps), each playing out what its page says on a loop, or holding its
//! key moment under reduced motion. No screenshots: everything is drawn
//! from the theme, so Night and Day each get their own.

use aizome_kit::draw;
use aizome_kit::layout::{Point, Rect, Size, Vec2};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle};
use aizome_kit::sidestep_ui::{Canvas, Color, StrokeStyle, TextLayout};
use aizome_kit::theme::{FontSpec, Theme, color};
use aizome_theme::{Brand, FocusColor, Palette, ThemeName};

/// The scene's size: every page draws in a 640 × 260 box, centred.
pub const SCENE: Size = Size::new(640.0, 260.0);

/// How long each page's scene takes before it plays again, in ms.
pub fn loop_ms(page: usize) -> u32 {
    [6000, 8000, 7000, 6500, 8000, 6000, 6000].get(page).copied().unwrap_or(6000)
}

/// The moment each scene holds under reduced motion: the one that says
/// most.
pub fn key_ms(page: usize) -> u32 {
    [5000, 2000, 2600, 2500, 2000, 700, 500].get(page).copied().unwrap_or(0)
}

/// What a scene shows: its page and the moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Art {
    pub page: usize,
    pub t: u32,
}

/// Draw `art` in `b`: the scene centred on `sunk`.
pub fn paint(c: &mut Canvas, b: Rect, art: &Art, theme: &Theme) {
    c.fill_rect(b, theme.sunk());
    let o = Point::new(
        b.x0 + ((b.width() - SCENE.width) / 2.0).round(),
        b.y0 + ((b.height() - SCENE.height) / 2.0).round(),
    );
    c.save();
    c.translate(o.to_vec2());
    let t = f64::from(art.t);
    match art.page {
        0 => welcome(c, theme, t),
        1 => tabs(c, theme, t),
        2 => menus(c, theme, t),
        3 => palette(c, theme, t),
        4 => desks(c, theme, t),
        5 => command_key(c, theme, t),
        _ => yours(c, theme, t),
    }
    c.restore();
}

// Time.

/// How far through `a`..`b` the moment `t` is, eased (0 before, 1 after).
fn seg(t: f64, a: f64, b: f64) -> f64 {
    let x = ((t - a) / (b - a)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn lerp(a: f64, b: f64, x: f64) -> f64 {
    a + (b - a) * x
}

fn lerp_pt(a: Point, b: Point, x: f64) -> Point {
    Point::new(lerp(a.x, b.x, x).round(), lerp(a.y, b.y, x).round())
}

// Type.

fn ui(size: f64, weight: f32) -> FontSpec {
    FontSpec { weight, ..FontSpec::ui(size) }
}

fn condensed(size: f64, weight: f32) -> FontSpec {
    FontSpec { condensed: true, ..ui(size, weight) }
}

fn mono(size: f64) -> FontSpec {
    FontSpec::mono(size)
}

fn width(theme: &Theme, s: &str, spec: FontSpec) -> f64 {
    aizome_kit::sidestep_ui::measure(s, &theme.style(spec, theme.text())).width
}

fn height(theme: &Theme, spec: FontSpec) -> f64 {
    aizome_kit::sidestep_ui::measure("Ag", &theme.style(spec, theme.text())).height
}

/// `s` with its top left at `at`.
fn text(c: &mut Canvas, theme: &Theme, s: &str, at: Point, spec: FontSpec, col: Color) {
    c.draw_label(s, &theme.style(spec, col), at);
}

/// `s` from `x`, centred on `y`; its width.
fn text_mid(c: &mut Canvas, theme: &Theme, s: &str, x: f64, y: f64, spec: FontSpec, col: Color) -> f64 {
    let h = height(theme, spec);
    text(c, theme, s, Point::new(x, (y - h / 2.0).round()), spec, col);
    width(theme, s, spec)
}

fn pal(theme: &Theme) -> Palette {
    theme.palette
}

// Pieces of Aizome.

/// The mark, `m` a module, its top left at `at`; `dips` how much of the
/// three stripes is dyed (0 to 3), and whether the tab shows.
fn mark(c: &mut Canvas, theme: &Theme, at: Point, m: f64, tab: bool, dips: f64) {
    let brand = Brand::for_theme(pal(theme).name);
    for (i, (rect, col)) in brand.mark_rects().into_iter().enumerate() {
        let [x, y, w, h] = rect.map(f64::from);
        let w = if i == 0 {
            if !tab {
                continue;
            }
            w
        } else {
            w * (dips - (i - 1) as f64).clamp(0.0, 1.0)
        };
        if w > 0.0 {
            c.fill_rect(Rect::new(at.x + x * m, at.y + y * m, at.x + (x + w) * m, at.y + (y + h) * m), color(col));
        }
    }
}

/// The hard 3pt shadow under `r`.
fn shadow(c: &mut Canvas, theme: &Theme, r: Rect) {
    if theme.shadows() {
        c.fill_rect(Rect::new(r.x0 + 3.0, r.y1, r.x1 + 3.0, r.y1 + 3.0), theme.slab());
        c.fill_rect(Rect::new(r.x1, r.y0 + 3.0, r.x1 + 3.0, r.y1), theme.slab());
    }
}

const TAB: f64 = 22.0;

/// A Haiku tab's width for `title`.
fn tab_width(theme: &Theme, title: &str) -> f64 {
    (8.0 + 10.0 + 7.0 + width(theme, title, condensed(12.0, 650.0)) + 7.0 + 10.0 + 8.0).round()
}

/// A tab at `x`, its bottom on `y`.
fn tab(c: &mut Canvas, theme: &Theme, x: f64, y: f64, title: &str, focused: bool) -> Rect {
    let p = pal(theme);
    let w = tab_width(theme, title);
    let r = Rect::new(x, y - TAB, x + w, y);
    let (fill, ink) =
        if focused { (theme.focus(), color(p.on_focus)) } else { (color(p.tab_idle), color(p.tab_idle_text)) };
    c.fill_rect(r, fill);
    draw::top(c, r, theme.bevel_hi(), 1.0);
    let mid = r.y0 + TAB / 2.0;
    let boxr = Rect::new(r.x0 + 8.0, (mid - 5.0).round(), r.x0 + 18.0, (mid + 5.0).round());
    if focused {
        draw::border(c, boxr, ink, 1.0);
    } else {
        c.fill_rect(boxr.inset(-2.0), ink.with_alpha(0.7));
    }
    text_mid(c, theme, title, r.x0 + 25.0, mid, condensed(12.0, 650.0), ink);
    let zoom = Rect::new(r.x1 - 18.0, boxr.y0, r.x1 - 8.0, boxr.y1);
    draw::border(c, zoom, ink, 1.0);
    draw::border(c, Rect::new(zoom.x0, zoom.y0, zoom.x0 + 6.0, zoom.y0 + 6.0), ink, 1.0);
    r
}

/// A window's frame (its tabs drawn apart), with lines of content.
fn frame(c: &mut Canvas, theme: &Theme, r: Rect, focused: bool, terminal: bool) {
    let p = pal(theme);
    c.fill_rect(r, if terminal { color(p.sunk) } else { theme.surface() });
    draw::border(c, r, theme.bevel_lo(), 1.0);
    draw::top(c, r.inset(-1.0), theme.bevel_hi(), 1.0);
    if focused {
        c.fill_rect(Rect::new(r.x0, r.y0, r.x1, r.y0 + 2.0), theme.focus());
    }
    let inner = r.inset(-12.0);
    let mut y = r.y0 + 16.0;
    let mut i = 0usize;
    while y + 6.0 < inner.y1 {
        if terminal {
            let w = [0.5, 0.8, 0.35, 0.7, 0.6][i % 5] * inner.width();
            c.fill_rect(Rect::new(inner.x0, y, inner.x0 + 10.0, y + 5.0), color(p.green));
            c.fill_rect(
                Rect::new(inner.x0 + 14.0, y, (inner.x0 + 14.0 + w * 0.8).min(inner.x1), y + 5.0),
                theme.text3().with_alpha(0.6),
            );
        } else {
            let w = [0.9, 0.75, 0.95, 0.4, 0.85, 0.6][i % 6] * inner.width();
            let col = if i == 0 { color(p.blue) } else { theme.faint().with_alpha(0.55) };
            c.fill_rect(Rect::new(inner.x0, y, inner.x0 + w, y + 5.0), col);
        }
        y += 14.0;
        i += 1;
    }
}

/// A window: shadow (tab included), tab, frame.
fn window(c: &mut Canvas, theme: &Theme, r: Rect, title: &str, focused: bool, terminal: bool) {
    let tw = tab_width(theme, title);
    shadow(c, theme, r);
    shadow(c, theme, Rect::new(r.x0, r.y0 - TAB, r.x0 + tw, r.y0));
    frame(c, theme, r, focused, terminal);
    tab(c, theme, r.x0, r.y0, title, focused);
}

/// The pointer, its tip at `at`.
fn pointer(c: &mut Canvas, at: Point) {
    let pts = [(0.0, 0.0), (0.0, 17.0), (4.5, 13.0), (7.5, 19.5), (10.0, 18.5), (7.0, 12.0), (12.5, 12.0)];
    let mut path = BezPath::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let p = Point::new(at.x + x, at.y + y);
        if i == 0 { path.move_to(p) } else { path.line_to(p) }
    }
    path.close_path();
    c.fill(&path, Color::rgb8(14, 14, 20));
    c.stroke(&path, &StrokeStyle::new(1.2), Color::rgb8(240, 242, 255));
}

/// A keycap in `r`, lit (pressed) or not.
fn keycap(c: &mut Canvas, theme: &Theme, r: Rect, label: &str, lit: bool) {
    let p = pal(theme);
    let r = if lit { r + Vec2::new(0.0, 1.0) } else { r };
    let (fill, ink) = if lit { (theme.focus(), color(p.on_focus)) } else { (theme.raised(), theme.text2()) };
    c.fill_rect(r, fill);
    draw::border(c, r, theme.line2(), 1.0);
    if !lit {
        c.fill_rect(Rect::new(r.x0 + 1.0, r.y1 - 3.0, r.x1 - 1.0, r.y1 - 1.0), theme.line2().with_alpha(0.6));
    }
    let spec = if label.chars().count() > 1 { ui(11.0, 600.0) } else { ui(14.0, 600.0) };
    let w = width(theme, label, spec);
    text_mid(c, theme, label, (r.x0 + (r.width() - w) / 2.0).round(), r.y0 + r.height() / 2.0 - 1.0, spec, ink);
}

/// Keys side by side from `at` (each `h` tall): the rectangle they take.
fn chord(c: &mut Canvas, theme: &Theme, at: Point, keys: &[&str], h: f64, lit: bool) -> Rect {
    let mut x = at.x;
    for k in keys {
        let w = if k.chars().count() > 2 { (width(theme, k, ui(11.0, 600.0)) + 16.0).round() } else { h };
        keycap(c, theme, Rect::new(x, at.y, x + w, at.y + h), k, lit);
        x += w + 4.0;
    }
    Rect::new(at.x, at.y, x - 4.0, at.y + h)
}

/// A row of a menu.
enum Row<'a> {
    Item { label: &'a str, keys: &'a str, lit: bool, sub: bool },
    Sep,
}

fn item<'a>(label: &'a str, keys: &'a str) -> Row<'a> {
    Row::Item { label, keys, lit: false, sub: false }
}

fn sub(label: &str, lit: bool) -> Row<'_> {
    Row::Item { label, keys: "", lit, sub: true }
}

const ROW: f64 = 22.0;

/// A menu at `at`, `w` wide, with a title strip (a menu at the pointer,
/// a torn-off palette) or not: its rectangle.
fn menu(c: &mut Canvas, theme: &Theme, at: Point, w: f64, title: Option<&str>, pinned: bool, rows: &[Row]) -> Rect {
    let p = pal(theme);
    let strip = if title.is_some() { 20.0 } else { 0.0 };
    let h: f64 = rows.iter().map(|r| if matches!(r, Row::Sep) { 9.0 } else { ROW }).sum::<f64>() + 8.0 + strip;
    let r = Rect::new(at.x, at.y, at.x + w, at.y + h);
    draw::floating(c, theme, r);
    if let Some(t) = title {
        let s = Rect::new(r.x0 + 1.0, r.y0 + 1.0, r.x1 - 1.0, r.y0 + strip);
        c.fill_rect(s, color(p.menu_title));
        let ink = color(p.on_menu_title);
        let mid = s.y0 + s.height() / 2.0;
        draw::border(c, Rect::new(s.x0 + 6.0, mid - 4.0, s.x0 + 14.0, mid + 4.0), ink, 1.0);
        text_mid(c, theme, t, s.x0 + 20.0, mid, ui(12.0, 650.0), ink);
        // The pin: a head and a point.
        let pin = if pinned { theme.focus() } else { ink.with_alpha(0.7) };
        c.fill_rect(Rect::new(s.x1 - 14.0, mid - 5.0, s.x1 - 8.0, mid), pin);
        c.fill_rect(Rect::new(s.x1 - 12.0, mid, s.x1 - 10.0, mid + 5.0), pin);
    }
    let mut y = r.y0 + strip + 4.0;
    for row in rows {
        match row {
            Row::Sep => {
                draw::separator(c, theme, r.x0 + 8.0, r.x1 - 8.0, y + 4.0);
                y += 9.0;
            }
            Row::Item { label, keys, lit, sub } => {
                let rr = Rect::new(r.x0 + 1.0, y, r.x1 - 1.0, y + ROW);
                let ink = if *lit {
                    c.fill_rect(rr, theme.selection());
                    theme.on_selection()
                } else {
                    theme.text()
                };
                text_mid(c, theme, label, rr.x0 + 11.0, rr.y0 + ROW / 2.0, ui(13.0, 400.0), ink);
                if *sub {
                    let x = rr.x1 - 14.0;
                    let mid = rr.y0 + ROW / 2.0;
                    let mut chev = BezPath::new();
                    chev.move_to((x, mid - 4.0));
                    chev.line_to((x + 4.0, mid));
                    chev.line_to((x, mid + 4.0));
                    c.stroke(&chev, &StrokeStyle::new(1.5), if *lit { ink } else { theme.text3() });
                } else if !keys.is_empty() {
                    let kw = width(theme, keys, mono(11.0));
                    text_mid(
                        c,
                        theme,
                        keys,
                        (rr.x1 - 10.0 - kw).round(),
                        rr.y0 + ROW / 2.0,
                        mono(11.0),
                        if *lit { ink } else { theme.text3() },
                    );
                }
                y += ROW;
            }
        }
    }
    r
}

/// The global menu bar in `r`: the Toolchest button (lit when open), the
/// app's name and menus, and the Desks strip with `desk` current.
fn menubar(
    c: &mut Canvas,
    theme: &Theme,
    r: Rect,
    app: &str,
    menus: &[&str],
    toolchest: bool,
    desk: Option<usize>,
) -> Rect {
    let p = pal(theme);
    c.fill_rect(r, color(p.bar));
    draw::bottom(c, r, theme.bevel_lo(), 1.0);
    let mid = r.y0 + r.height() / 2.0;
    let tw = width(theme, "Toolchest", ui(13.0, 650.0));
    let button = Rect::new(r.x0, r.y0, r.x0 + 8.0 + 16.0 + 6.0 + tw + 10.0, r.y1 - 1.0);
    if toolchest {
        c.fill_rect(button, theme.selection());
    }
    mark(c, theme, Point::new(r.x0 + 8.0, (mid - 8.0).round()), 4.0, true, 3.0);
    text_mid(
        c,
        theme,
        "Toolchest",
        r.x0 + 30.0,
        mid,
        ui(13.0, 650.0),
        if toolchest { theme.on_selection() } else { theme.text() },
    );
    c.fill_rect(Rect::new(button.x1, r.y0 + 4.0, button.x1 + 1.0, r.y1 - 4.0), theme.line());
    let mut x = button.x1 + 12.0;
    x += text_mid(c, theme, app, x, mid, ui(13.0, 700.0), theme.text()) + 14.0;
    for m in menus {
        x += text_mid(c, theme, m, x, mid, ui(13.0, 400.0), theme.text2()) + 14.0;
    }
    if let Some(cur) = desk {
        let names = ["1 Mail", "2 Build", "3 Write"];
        let total: f64 = names.iter().map(|n| width(theme, n, ui(12.0, 600.0)) + 16.0).sum::<f64>() + 22.0;
        let mut x = r.x1 - total - 8.0;
        for (i, n) in names.iter().enumerate() {
            let w = width(theme, n, ui(12.0, 600.0)) + 16.0;
            let cell = Rect::new(x, r.y0 + 2.0, x + w, r.y1 - 3.0);
            let ink = if i == cur {
                c.fill_rect(cell, theme.selection());
                theme.on_selection()
            } else {
                theme.text2()
            };
            text_mid(c, theme, n, x + 8.0, mid, ui(12.0, if i == cur { 650.0 } else { 450.0 }), ink);
            x += w;
        }
        text_mid(c, theme, "+", x + 7.0, mid, ui(13.0, 400.0), theme.text3());
    }
    button
}

/// A small chip of text under the pointer, as drags show.
fn chip(c: &mut Canvas, theme: &Theme, at: Point, s: &str) {
    let w = width(theme, s, ui(12.0, 500.0)) + 16.0;
    let r = Rect::new(at.x, at.y, at.x + w, at.y + 22.0);
    draw::floating(c, theme, r);
    text_mid(c, theme, s, r.x0 + 8.0, r.y0 + 11.0, ui(12.0, 500.0), theme.text());
}

// The scenes.

/// Welcome: the lockup, its stripes dyed one dip at a time, the i's dot
/// last; the dye ramp under it.
fn welcome(c: &mut Canvas, theme: &Theme, t: f64) {
    let brand = Brand::for_theme(pal(theme).name);
    let m = 12.0;
    let size = m / 0.17567;
    let spec = FontSpec { weight: 800.0, condensed: true, ..FontSpec::ui(size) };
    let word = "aızome";
    let ww = width(theme, word, spec);
    let seal_w = 2.0 * m;
    let total = 4.0 * m + m + ww + 1.2 * m + seal_w;
    let x0 = ((SCENE.width - total) / 2.0).round();
    let baseline = 128.0;
    let top = baseline - 4.0 * m;
    let dips = seg(t, 600.0, 1100.0) + seg(t, 1300.0, 1800.0) + seg(t, 2000.0, 2500.0);
    mark(c, theme, Point::new(x0, top), m, t >= 250.0, dips);
    // The word, set on the mark's baseline.
    let style = theme.style(spec, color(brand.wordmark));
    let layout = TextLayout::new(word, &style);
    let wx = x0 + 5.0 * m;
    c.draw_text(&layout, Point::new(wx, (baseline - layout.first_baseline()).round()));
    // The i's dot: an amber tab whose top meets the mark's top.
    if t >= 2800.0 {
        let a = width(theme, "a", spec);
        let i = width(theme, "ı", spec);
        let cx = wx + a + i / 2.0;
        c.fill_rect(
            Rect::new((cx - 0.6 * m).round(), top.round(), (cx + 0.6 * m).round(), (top + 0.6 * m).round()),
            color(brand.amber),
        );
    }
    // 藍染, set down the right.
    let seal = ui(2.0 * m * 0.95, 600.0);
    let sx = wx + ww + 1.2 * m;
    for (k, ch) in ["藍", "染"].iter().enumerate() {
        text(c, theme, ch, Point::new(sx, top + k as f64 * 2.0 * m - 2.0), seal, color(brand.seal));
    }
    // The dye ramp, by its old names.
    let ramp = [
        ("甕覗", "kamenozoki", brand.pale),
        ("縹", "hanada", brand.mid),
        ("藍", "ai", brand.deep),
        ("琥珀", "kohaku", brand.amber),
    ];
    let cell = 132.0;
    let rx = ((SCENE.width - cell * 4.0) / 2.0).round();
    for (k, (kanji, name, col)) in ramp.iter().enumerate() {
        let x = rx + k as f64 * cell;
        let shown =
            if k < 3 { seg(t, 600.0 + 700.0 * k as f64, 1100.0 + 700.0 * k as f64) } else { seg(t, 2800.0, 3100.0) };
        c.fill_rect(Rect::new(x, 182.0, x + 24.0, 206.0), color(*col).with_alpha(shown as f32 * 0.85 + 0.15));
        text_mid(c, theme, kanji, x + 32.0, 188.0, ui(12.0, 500.0), theme.text2());
        text_mid(c, theme, name, x + 32.0, 202.0, mono(10.0), theme.text3());
    }
}

/// Tabs: a window's tab dragged onto another's stacks them; ⇧-dragging
/// a tab slides it along the top edge.
fn tabs(c: &mut Canvas, theme: &Theme, t: f64) {
    let a = Rect::new(40.0, 60.0, 340.0, 236.0);
    let a_tab = tab_width(theme, "notes.md");
    let b_size = Size::new(290.0, 150.0);
    let b_start = Point::new(300.0, 96.0);
    // B's tab lands just right of A's.
    let b_dest = Point::new(a.x0 + a_tab, a.y0);
    let drag = seg(t, 700.0, 2300.0);
    let stacked = t >= 2300.0;
    let grab = Vec2::new(30.0, -TAB / 2.0);
    if !stacked {
        let b_at = lerp_pt(b_start, b_dest, drag);
        let b = Rect::from_origin_size(b_at, b_size);
        // Over A's edge: the dashed slot, and what releasing does.
        if t >= 700.0 {
            // Pressed on its tab: Terminal comes forward with the focus.
            window(c, theme, a, "notes.md", false, false);
            if drag > 0.62 {
                let bw = tab_width(theme, "Terminal");
                let slot = Rect::new(b_dest.x + 1.0, a.y0 - TAB + 1.0, b_dest.x + bw - 1.0, a.y0 - 1.0);
                c.stroke(&slot, &StrokeStyle::new(1.5).with_dash(vec![4.0, 3.0], 0.0), theme.focus());
            }
            window(c, theme, b, "Terminal", true, true);
            if drag > 0.62 {
                chip(c, theme, Point::new(b.x0 + 70.0, b.y0 - TAB - 34.0), "Release to stack with notes.md");
            }
        } else {
            window(c, theme, b, "Terminal", false, true);
            window(c, theme, a, "notes.md", true, false);
        }
        if t >= 400.0 {
            pointer(c, b_at + grab);
        }
        return;
    }
    // Stacked: one frame, the tabs side by side, Terminal showing; then
    // its tab slides right with ⇧.
    let slide = seg(t, 3400.0, 5000.0) * 130.0;
    let bw = tab_width(theme, "Terminal");
    shadow(c, theme, a);
    shadow(c, theme, Rect::new(a.x0, a.y0 - TAB, a.x0 + a_tab, a.y0));
    shadow(c, theme, Rect::new(b_dest.x + slide, a.y0 - TAB, b_dest.x + slide + bw, a.y0));
    frame(c, theme, a, true, true);
    tab(c, theme, a.x0, a.y0, "notes.md", false);
    tab(c, theme, (b_dest.x + slide).round(), a.y0, "Terminal", true);
    let shift = (3000.0..5300.0).contains(&t);
    let p = Point::new(b_dest.x + slide + 30.0, a.y0 - TAB / 2.0);
    if shift {
        keycap(c, theme, Rect::new(p.x + 16.0, p.y + 16.0, p.x + 42.0, p.y + 42.0), "⇧", true);
    }
    if t < 5600.0 {
        pointer(c, p);
    }
}

/// The menu bar; ❖-right-click opens the whole menu at the pointer; a
/// submenu torn off by its title becomes a palette that stays.
fn menus(c: &mut Canvas, theme: &Theme, t: f64) {
    let names = ["File", "Edit", "View", "Image", "Go", "Window", "Help"];
    menubar(c, theme, Rect::new(0.0, 0.0, SCENE.width, 24.0), "Shashin", &names, false, None);
    // A window behind, to click on.
    window(c, theme, Rect::new(30.0, 72.0, 250.0, 230.0), "IMG_2.jpg", false, false);
    let start = Point::new(470.0, 220.0);
    let at = Point::new(232.0, 58.0);
    let p = lerp_pt(start, at, seg(t, 0.0, 900.0));
    let open = (1250.0..4200.0).contains(&t);
    let view_lit = t >= 1800.0;
    let sub_open = t >= 2000.0;
    let tear = seg(t, 3300.0, 4200.0);
    let main = if open {
        let rows = [
            sub("File", false),
            sub("Edit", false),
            sub("View", view_lit),
            sub("Image", false),
            sub("Go", false),
            Row::Sep,
            item("Hide Shashin", "❖H"),
            item("Quit Shashin", "❖Q"),
        ];
        Some(menu(c, theme, at, 170.0, Some("Shashin"), false, &rows))
    } else {
        None
    };
    let mut pointer_at = p;
    if sub_open {
        let rows = [
            item("Zoom In", "❖+"),
            item("Zoom Out", "❖−"),
            item("Actual Size", "❖0"),
            item("Zoom to Fit", "❖9"),
            Row::Sep,
            item("Slideshow", "⇧❖F"),
        ];
        let base = Point::new(at.x + 170.0 - 2.0, at.y + 20.0 + 4.0 + 2.0 * ROW - 20.0);
        let torn = Point::new(base.x + 140.0, base.y + 40.0);
        let sub_at = lerp_pt(base, torn, tear);
        menu(c, theme, sub_at, 190.0, Some("View"), t >= 4200.0, &rows);
        if t >= 3000.0 {
            // The pointer takes the title strip and drags it.
            let grip = Point::new(sub_at.x + 90.0, sub_at.y + 10.0);
            pointer_at = if t < 3300.0 {
                lerp_pt(Point::new(at.x + 60.0, at.y + 75.0), grip, seg(t, 3000.0, 3300.0))
            } else {
                grip
            };
        } else if view_lit {
            pointer_at = Point::new(at.x + 60.0, at.y + 75.0);
        }
    }
    let _ = main;
    // ❖ held for the right-click.
    if (800.0..1500.0).contains(&t) {
        keycap(c, theme, Rect::new(p.x + 18.0, p.y + 18.0, p.x + 44.0, p.y + 44.0), "❖", true);
    }
    if (1000.0..1400.0).contains(&t) {
        let r = 4.0 + 14.0 * seg(t, 1000.0, 1400.0);
        c.stroke(
            &Circle::new(p, r),
            &StrokeStyle::new(2.0),
            theme.focus().with_alpha((1.0 - seg(t, 1000.0, 1400.0)) as f32),
        );
    }
    pointer(c, pointer_at);
}

/// The palette: ❖Space, a few letters, every command, window, file and
/// setting that matches, with its keys.
fn palette(c: &mut Canvas, theme: &Theme, t: f64) {
    let p = pal(theme);
    if t < 600.0 {
        let r = chord(c, theme, Point::new(0.0, 0.0), &["❖", "Space"], 32.0, false);
        let o = Point::new(((SCENE.width - r.width()) / 2.0).round(), 110.0);
        chord(c, theme, o, &["❖", "Space"], 32.0, t >= 250.0);
        return;
    }
    let r = Rect::new(50.0, 8.0, 590.0, 250.0);
    draw::floating(c, theme, r);
    // The query.
    let typed = match t {
        x if x < 900.0 => "",
        x if x < 1150.0 => "r",
        x if x < 1400.0 => "ro",
        _ => "rot",
    };
    let qy = r.y0 + 20.0;
    text_mid(c, theme, "›", r.x0 + 14.0, qy, ui(18.0, 700.0), theme.focus());
    let qx = r.x0 + 32.0;
    let tw = if typed.is_empty() {
        text_mid(c, theme, "Search commands, windows, files…", qx, qy, ui(16.0, 400.0), theme.faint());
        0.0
    } else {
        text_mid(c, theme, typed, qx, qy, ui(16.0, 400.0), theme.text())
    };
    if ((t / 500.0) as u32).is_multiple_of(2) || typed.len() < 3 {
        c.fill_rect(Rect::new(qx + tw + 1.0, qy - 9.0, qx + tw + 2.5, qy + 9.0), theme.focus());
    }
    let ctx = "in Shashin · IMG_2.jpg";
    let cw = width(theme, ctx, mono(10.0));
    text_mid(c, theme, ctx, r.x1 - 12.0 - cw, qy, mono(10.0), theme.text3());
    draw::separator(c, theme, r.x0 + 1.0, r.x1 - 1.0, r.y0 + 38.0);
    // The scopes.
    let mut x = r.x0 + 12.0;
    for (i, s) in ["All", "Commands", "Windows", "Files", "Apps", "Settings"].iter().enumerate() {
        let w = width(theme, s, ui(11.0, 500.0)) + 14.0;
        let chip = Rect::new(x, r.y0 + 46.0, x + w, r.y0 + 64.0);
        let ink = if i == 0 {
            c.fill_rect(chip, theme.selection());
            theme.on_selection()
        } else {
            draw::border(c, chip, theme.line2(), 1.0);
            theme.text2()
        };
        text_mid(c, theme, s, x + 7.0, chip.y0 + 9.0, ui(11.0, 500.0), ink);
        x += w + 6.0;
    }
    if typed.is_empty() {
        return;
    }
    // Results, under section heads.
    let list_x1 = r.x0 + 300.0;
    let selected = if (3000.0..3800.0).contains(&t) { 1 } else { 0 };
    let mut y = r.y0 + 74.0;
    let head = |c: &mut Canvas, y: &mut f64, s: &str| {
        text_mid(c, theme, s, r.x0 + 12.0, *y + 7.0, mono(9.0), theme.text3());
        *y += 16.0;
    };
    let row = |c: &mut Canvas, y: &mut f64, i: usize, name: &str, path: &str, keys: &[&str]| {
        let rr = Rect::new(r.x0 + 1.0, *y, list_x1, *y + 24.0);
        let lit = i == selected;
        if lit {
            c.fill_rect(rr, theme.selection());
            c.fill_rect(Rect::new(rr.x0, rr.y0, rr.x0 + 2.0, rr.y1), theme.focus());
        }
        let ink = if lit { theme.on_selection() } else { theme.text() };
        // The letters typed, in blue and underlined.
        let n = typed.len().min(name.len());
        let (hit, rest) = name.split_at(n);
        let mx = rr.x0 + 12.0;
        let hw = text_mid(c, theme, hit, mx, rr.y0 + 12.0, ui(13.0, 500.0), if lit { ink } else { color(p.blue) });
        c.fill_rect(Rect::new(mx, rr.y0 + 19.0, mx + hw, rr.y0 + 20.0), if lit { ink } else { color(p.blue) });
        let w2 = text_mid(c, theme, rest, mx + hw, rr.y0 + 12.0, ui(13.0, 500.0), ink);
        text_mid(
            c,
            theme,
            path,
            mx + hw + w2 + 8.0,
            rr.y0 + 12.0,
            ui(11.0, 400.0),
            if lit { ink.with_alpha(0.75) } else { theme.text3() },
        );
        if !keys.is_empty() {
            let kw = keys.len() as f64 * 22.0;
            chord(c, theme, Point::new(list_x1 - 10.0 - kw, rr.y0 + 3.0), keys, 18.0, false);
        }
        *y += 24.0;
    };
    head(c, &mut y, "COMMANDS");
    row(c, &mut y, 0, "Rotate Right", "Image › Rotate Right", &["❖", "R"]);
    row(c, &mut y, 1, "Rotate Left", "Image › Rotate Left", &["❖", "L"]);
    head(c, &mut y, "FILES");
    row(c, &mut y, 2, "rotation-notes.md", "~/Notes", &[]);
    head(c, &mut y, "SETTINGS");
    row(c, &mut y, 3, "Rotation", "Displays › Rotation", &[]);
    // The detail pane: the command, its keys, and the coach.
    c.fill_rect(Rect::new(list_x1, r.y0 + 70.0, list_x1 + 1.0, r.y1 - 26.0), theme.line());
    let dx = list_x1 + 16.0;
    let (name, key) = if selected == 1 { ("Rotate Left", "L") } else { ("Rotate Right", "R") };
    text(c, theme, name, Point::new(dx, r.y0 + 78.0), ui(15.0, 650.0), theme.text());
    chord(c, theme, Point::new(dx, r.y0 + 104.0), &["❖", key], 26.0, false);
    let coach = "You've run this from menus 3 times this week. The keys above do it without leaving the picture.";
    let layout = TextLayout::wrapped(coach, &theme.style(ui(11.5, 400.0), theme.text2()), r.x1 - dx - 14.0);
    c.draw_text(&layout, Point::new(dx, r.y0 + 142.0));
    draw::separator(c, theme, r.x0 + 1.0, r.x1 - 1.0, r.y1 - 24.0);
    text_mid(
        c,
        theme,
        "↑↓ move   ↩ run   ⇥ scope   ❖K actions   esc close",
        r.x0 + 12.0,
        r.y1 - 11.0,
        mono(9.5),
        theme.text3(),
    );
}

/// The Toolchest open from the bar, the Desks strip moving with ⌃❖→, and
/// the Deskbar with its Shelf.
fn desks(c: &mut Canvas, theme: &Theme, t: f64) {
    let p = pal(theme);
    let desk = match t {
        x if x < 3500.0 => 0,
        x if x < 5000.0 => 1,
        x if x < 6500.0 => 2,
        _ => 0,
    };
    let open = (500.0..3000.0).contains(&t);
    let button =
        menubar(c, theme, Rect::new(0.0, 0.0, SCENE.width, 24.0), "Hako", &["File", "Edit", "Go"], open, Some(desk));
    // The Deskbar, at the top right under the bar.
    let db = Rect::new(408.0, 34.0, SCENE.width - 4.0, 252.0);
    shadow(c, theme, db);
    c.fill_rect(db, theme.chrome());
    draw::border(c, db, theme.bevel_lo(), 1.0);
    type App<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str)]);
    let apps: [App; 2] = [
        ("Hako", "2", &[("Pictures", ""), ("Downloads", "desk 2")]),
        ("Terminal", "2", &[("cargo test", "stack"), ("sekka", "hidden")]),
    ];
    let mut y = db.y0 + 6.0;
    for (ai, (name, n, wins)) in apps.iter().enumerate() {
        c.fill_rect(
            Rect::new(db.x0 + 10.0, y + 5.0, db.x0 + 22.0, y + 17.0),
            color([p.blue, p.green][ai]).with_alpha(0.9),
        );
        text_mid(c, theme, name, db.x0 + 30.0, y + 11.0, ui(13.0, 650.0), theme.text());
        let nw = width(theme, n, mono(11.0));
        text_mid(c, theme, n, db.x1 - 10.0 - nw, y + 11.0, mono(11.0), theme.text3());
        y += 22.0;
        for (wi, (title, tag)) in wins.iter().enumerate() {
            let focused = ai == 0 && wi == 0;
            if focused {
                c.fill_rect(Rect::new(db.x0 + 1.0, y, db.x1 - 1.0, y + 20.0), theme.raised());
                c.fill_rect(Rect::new(db.x0 + 22.0, y + 3.0, db.x0 + 25.0, y + 17.0), theme.focus());
            }
            let hidden = *tag == "hidden";
            let spec = FontSpec { italic: hidden, ..ui(12.5, 400.0) };
            let ink = if focused {
                theme.text()
            } else if hidden {
                theme.text3()
            } else {
                theme.text2()
            };
            text_mid(c, theme, title, db.x0 + 32.0, y + 10.0, spec, ink);
            if !tag.is_empty() {
                let tw = width(theme, tag, ui(11.0, 400.0));
                text_mid(c, theme, tag, db.x1 - 10.0 - tw, y + 10.0, ui(11.0, 400.0), theme.text3());
            }
            y += 20.0;
        }
        y += 2.0;
    }
    draw::separator(c, theme, db.x0 + 1.0, db.x1 - 1.0, y + 2.0);
    y += 10.0;
    text_mid(c, theme, "SHELF", db.x0 + 10.0, y + 6.0, mono(9.0), theme.text3());
    y += 16.0;
    let item = Rect::new(db.x0 + 8.0, y, db.x1 - 8.0, y + 22.0);
    draw::raised(c, theme, item, Some(theme.raised()), theme.line2());
    c.fill_rect(Rect::new(item.x0 + 8.0, item.y0 + 6.0, item.x0 + 18.0, item.y0 + 16.0), color(p.cyan));
    text_mid(c, theme, "wallpaper.png", item.x0 + 26.0, item.y0 + 11.0, ui(12.0, 400.0), theme.text());
    y += 28.0;
    let drop = Rect::new(db.x0 + 8.0, y, db.x1 - 8.0, (y + 22.0).min(db.y1 - 6.0));
    c.stroke(&drop.inset(-0.5), &StrokeStyle::new(1.0).with_dash(vec![3.0, 3.0], 0.0), theme.line2());
    let dw = width(theme, "Drop to park", ui(11.5, 400.0));
    text_mid(
        c,
        theme,
        "Drop to park",
        (drop.x0 + (drop.width() - dw) / 2.0).round(),
        drop.y0 + 11.0,
        ui(11.5, 400.0),
        theme.text3(),
    );
    // The Toolchest.
    if open {
        let rows = [
            sub("Applications", (900.0..2000.0).contains(&t)),
            sub("Places", false),
            sub("Desks", (2000.0..3000.0).contains(&t)),
            sub("Find", false),
            sub("System", false),
            sub("Session", false),
        ];
        let m = menu(c, theme, Point::new(button.x0, button.y1 + 1.0), 180.0, Some("Toolchest"), false, &rows);
        // The mark in its title strip.
        mark(c, theme, Point::new(m.x1 - 34.0, m.y0 + 4.0), 3.0, true, 3.0);
    } else {
        // Desks, by the keys.
        let lit = [3500.0, 5000.0, 6500.0].iter().any(|k| (k - 400.0..k + 500.0).contains(&t));
        chord(c, theme, Point::new(150.0, 110.0), &["⌃", "❖", "→"], 34.0, lit);
        let label = format!("Desk {}: {}", desk + 1, ["Mail", "Build", "Write"][desk]);
        text(c, theme, &label, Point::new(150.0, 156.0), ui(15.0, 600.0), theme.text2());
        text(c, theme, "⌃❖↑ shows them all", Point::new(150.0, 180.0), ui(12.0, 400.0), theme.text3());
    }
}

/// Super is the command key: ❖ with a letter does what ⌘ does on a Mac.
fn command_key(c: &mut Canvas, theme: &Theme, t: f64) {
    let chords = [("C", "Copy"), ("V", "Paste"), ("Z", "Undo"), ("Q", "Quit")];
    let k = ((t / 1500.0) as usize).min(3);
    let lit = (t % 1500.0) > 200.0 && (t % 1500.0) < 1100.0;
    let (letter, what) = chords[k];
    let key = 34.0;
    let gap = 4.0;
    let rows: [&[&str]; 3] =
        [&["Q", "W", "E", "R", "T", "Y"], &["A", "S", "D", "F", "G", "H"], &["Z", "X", "C", "V", "B", "N"]];
    let kb_w = 6.0 * (key + gap) - gap;
    let x0 = 70.0;
    let y0 = 30.0;
    for (ri, row) in rows.iter().enumerate() {
        let off = ri as f64 * 12.0;
        for (ci, l) in row.iter().enumerate() {
            let x = x0 + off + ci as f64 * (key + gap);
            let y = y0 + ri as f64 * (key + gap);
            keycap(c, theme, Rect::new(x, y, x + key, y + key), l, lit && *l == letter);
        }
    }
    // The bottom row: ⌃ ❖ ⌥ and the space bar.
    let y = y0 + 3.0 * (key + gap);
    let mods = [("⌃", 46.0), ("❖", 46.0), ("⌥", 46.0)];
    let mut x = x0;
    for (l, w) in mods {
        keycap(c, theme, Rect::new(x, y, x + w, y + key), l, lit && l == "❖");
        x += w + gap;
    }
    keycap(c, theme, Rect::new(x, y, x0 + kb_w + 24.0, y + key), "", false);
    // What the chord does.
    let cx = x0 + kb_w + 70.0;
    let r = chord(c, theme, Point::new(cx, 70.0), &["❖", letter], 44.0, lit);
    text(c, theme, what, Point::new(cx, r.y1 + 14.0), ui(26.0, 650.0), theme.text());
    text(c, theme, "in every app", Point::new(cx, r.y1 + 50.0), ui(13.0, 400.0), theme.text3());
}

/// Make it yours: Night and Day side by side, the focus colour changing
/// in both.
fn yours(c: &mut Canvas, theme: &Theme, t: f64) {
    let focuses = [FocusColor::Amber, FocusColor::Cyan, FocusColor::Rose, FocusColor::Lime];
    let fi = ((t / 1500.0) as usize) % 4;
    let mini = |c: &mut Canvas, r: Rect, name: ThemeName| {
        let p = Palette::new(name, focuses[fi]);
        shadow(c, theme, r);
        c.fill_rect(r, color(p.desktop));
        draw::border(c, r, theme.bevel_lo(), 1.0);
        // The bar, with the mark.
        let bar = Rect::new(r.x0 + 1.0, r.y0 + 1.0, r.x1 - 1.0, r.y0 + 13.0);
        c.fill_rect(bar, color(p.bar));
        let b = Brand::for_theme(name);
        for (rect, col) in b.mark_rects() {
            let [x, y, w, h] = rect.map(f64::from);
            c.fill_rect(
                Rect::new(
                    bar.x0 + 4.0 + x * 2.0,
                    bar.y0 + 2.0 + y * 2.0,
                    bar.x0 + 4.0 + (x + w) * 2.0,
                    bar.y0 + 2.0 + (y + h) * 2.0,
                ),
                color(col),
            );
        }
        for i in 0..4 {
            c.fill_rect(
                Rect::new(bar.x0 + 18.0 + i as f64 * 22.0, bar.y0 + 5.0, bar.x0 + 34.0 + i as f64 * 22.0, bar.y0 + 8.0),
                color(p.text2).with_alpha(0.7),
            );
        }
        // A window behind, and one in front with the focus.
        for (k, (wr, focused)) in [
            (Rect::new(r.x0 + 120.0, r.y0 + 40.0, r.x1 - 16.0, r.y1 - 26.0), false),
            (Rect::new(r.x0 + 20.0, r.y0 + 52.0, r.x0 + 170.0, r.y1 - 14.0), true),
        ]
        .into_iter()
        .enumerate()
        {
            let _ = k;
            c.fill_rect(Rect::new(wr.x0 + 2.0, wr.y1, wr.x1 + 2.0, wr.y1 + 2.0), color(p.slab));
            c.fill_rect(Rect::new(wr.x1, wr.y0 - 12.0 + 2.0, wr.x1 + 2.0, wr.y1), color(p.slab));
            c.fill_rect(wr, color(p.surface));
            draw::border(c, wr, color(p.bevel_lo), 1.0);
            let tabr = Rect::new(wr.x0, wr.y0 - 12.0, wr.x0 + 60.0, wr.y0);
            c.fill_rect(tabr, if focused { color(p.focus) } else { color(p.tab_idle) });
            c.fill_rect(
                Rect::new(tabr.x0 + 14.0, tabr.y0 + 5.0, tabr.x1 - 8.0, tabr.y0 + 7.0),
                if focused { color(p.on_focus) } else { color(p.tab_idle_text) },
            );
            if focused {
                c.fill_rect(Rect::new(wr.x0, wr.y0, wr.x1, wr.y0 + 1.5), color(p.focus));
            }
            for i in 0..4 {
                let w = [0.8, 0.6, 0.9, 0.5][i] * (wr.width() - 20.0);
                c.fill_rect(
                    Rect::new(
                        wr.x0 + 10.0,
                        wr.y0 + 10.0 + i as f64 * 10.0,
                        wr.x0 + 10.0 + w,
                        wr.y0 + 13.0 + i as f64 * 10.0,
                    ),
                    if i == 0 { color(p.blue) } else { color(p.faint).with_alpha(0.6) },
                );
            }
            // The selection, in ai.
            c.fill_rect(Rect::new(wr.x0 + 10.0, wr.y0 + 28.0, wr.x0 + 70.0, wr.y0 + 35.0), color(p.selection));
        }
    };
    mini(c, Rect::new(30.0, 8.0, 300.0, 168.0), ThemeName::Night);
    mini(c, Rect::new(340.0, 8.0, 610.0, 168.0), ThemeName::Day);
    text_mid(c, theme, "NIGHT", 30.0, 184.0, mono(10.0), theme.text3());
    text_mid(c, theme, "DAY", 340.0, 184.0, mono(10.0), theme.text3());
    // The focus colours, the one in use ringed.
    let names = ["Amber", "Cyan", "Rose", "Lime"];
    let total = 4.0 * 26.0 + 3.0 * 18.0;
    let mut x = ((SCENE.width - total) / 2.0).round();
    for (i, f) in focuses.iter().enumerate() {
        let col = color(aizome_theme::focus_color(pal(theme).name, *f));
        let sw = Rect::new(x, 206.0, x + 26.0, 232.0);
        if i == fi {
            draw::border(c, sw.inflate(4.0, 4.0), theme.text2(), 2.0);
        }
        c.fill_rect(sw, col);
        if i == fi {
            let w = width(theme, names[i], ui(11.0, 600.0));
            text(
                c,
                theme,
                names[i],
                Point::new((sw.x0 + 13.0 - w / 2.0).round(), 240.0),
                ui(11.0, 600.0),
                theme.text2(),
            );
        }
        x += 44.0;
    }
}
