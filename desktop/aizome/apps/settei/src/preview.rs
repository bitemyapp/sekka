//! A mini desktop in a palette: the menu bar with the mark, a window
//! without focus behind, the focused one in front with its amber tab and
//! rail, its hard shadow, and a sliver of the Deskbar. Appearance and
//! Accessibility show one per theme, drawn with the kit's canvas.

use aizome_kit::Image;
use aizome_kit::draw;
use aizome_kit::element::Element;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::sidestep_ui::kurbo::BezPath;
use aizome_kit::theme::color;
use aizome_kit::widget::{LayoutCx, PaintCx, SemanticRole, Semantics, Widget};
use aizome_theme::{Brand, Effects, FocusColor, Palette, Rgba, ThemeName};

/// What a preview shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    Theme(ThemeName),
    /// Night and Day split corner to corner.
    Auto,
}

/// A mini desktop `width` × `height` points.
pub fn desktop<Msg>(look: Look, focus: FocusColor, effects: Effects, width: f64, height: f64) -> Element<Msg> {
    with_wallpapers(look, focus, effects, width, height, [None, None])
}

/// The same over the wallpapers chosen for Night and Day (a thumbnail and
/// an id for it).
pub fn with_wallpapers<Msg>(
    look: Look,
    focus: FocusColor,
    effects: Effects,
    width: f64,
    height: f64,
    wallpapers: [Option<(Image, u64)>; 2],
) -> Element<Msg> {
    Element::new(Preview { look, focus, effects, size: Size::new(width, height), wallpapers }, Vec::new())
}

struct Preview {
    look: Look,
    focus: FocusColor,
    effects: Effects,
    size: Size,
    /// Night's and Day's.
    wallpapers: [Option<(Image, u64)>; 2],
}

impl Widget for Preview {
    fn name(&self) -> &'static str {
        "preview"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(self.size)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let r = Rect::from_origin_size(Point::ZERO, cx.size());
        match self.look {
            Look::Theme(t) => {
                let wall = if t == ThemeName::Day { &self.wallpapers[1] } else { &self.wallpapers[0] };
                paint_desktop(cx.canvas, r, &Palette::new(t, self.focus), self.effects, wall.as_ref().map(|w| &w.0));
            }
            Look::Auto => {
                let night = self.wallpapers[0].as_ref().map(|w| &w.0);
                paint_desktop(cx.canvas, r, &Palette::new(ThemeName::Night, self.focus), self.effects, night);
                let mut tri = BezPath::new();
                tri.move_to((r.x1, r.y0));
                tri.line_to((r.x1, r.y1));
                tri.line_to((r.x0, r.y1));
                tri.close_path();
                cx.canvas.save();
                cx.canvas.clip(&tri);
                let day = self.wallpapers[1].as_ref().map(|w| &w.0);
                paint_desktop(cx.canvas, r, &Palette::new(ThemeName::Day, self.focus), self.effects, day);
                cx.canvas.restore();
            }
        }
        let t = cx.theme();
        draw::border(cx.canvas, r, t.bevel_lo(), 1.0);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.look, self.focus, self.effects)).f64(self.size.width).f64(self.size.height);
        h.add(&self.wallpapers.each_ref().map(|w| w.as_ref().map(|(_, id)| *id)));
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Image, label: Some("Preview".into()), ..Default::default() })
    }
}

/// Snap to whole points.
fn snap(r: Rect) -> Rect {
    Rect::new(r.x0.round(), r.y0.round(), r.x1.round(), r.y1.round())
}

/// The mini desktop in `p`, filling `r`.
pub fn paint_desktop(c: &mut Canvas, r: Rect, p: &Palette, effects: Effects, wallpaper: Option<&Image>) {
    let (w, h) = (r.width(), r.height());
    let at = |x: f64, y: f64, ww: f64, hh: f64| {
        snap(Rect::new(r.x0 + x * w, r.y0 + y * h, r.x0 + (x + ww) * w, r.y0 + (y + hh) * h))
    };
    match wallpaper {
        // The picture, covering the desktop.
        Some(img) => {
            let s = img.size();
            let k = (w / s.width).max(h / s.height);
            let (iw, ih) = (s.width * k, s.height * k);
            let dst =
                Rect::new(r.x0 + (w - iw) / 2.0, r.y0 + (h - ih) / 2.0, r.x0 + (w + iw) / 2.0, r.y0 + (h + ih) / 2.0);
            c.save();
            c.clip_rect(r);
            c.draw_image(img, dst);
            c.restore();
        }
        // The desktop's colour, with a dusk of the dye ramp low down, as
        // the boards' wallpapers glow.
        None => {
            c.fill_rect(r, color(p.desktop));
            let glow = if p.name == ThemeName::Day { Rgba::hex(0xa9c4ff) } else { Rgba::hex(0x3d59a1) };
            for i in 0..6 {
                let y0 = 0.62 + f64::from(i) * 0.065;
                let a = 0.10 + f64::from(i) * 0.06;
                c.fill_rect(at(0.0, y0, 1.0, 0.066), color(glow.with_alpha(a as f32)));
            }
        }
    }

    // The menu bar.
    let bar_h = (h * 0.075).max(6.0).round();
    let bar = snap(Rect::new(r.x0, r.y0, r.x1, r.y0 + bar_h));
    c.fill_rect(bar, color(p.bar));
    draw::bottom(c, bar, color(p.bevel_lo), 1.0);
    let m = ((bar_h - 2.0) / 4.0).floor().max(1.0);
    let brand = Brand::for_theme(p.name);
    crate::widgets::draw_mark(c, &brand, Point::new(r.x0 + 4.0, r.y0 + ((bar_h - 4.0 * m) / 2.0).round()), m);
    let line_y = r.y0 + (bar_h / 2.0).round() - 1.0;
    let mut x = r.x0 + 6.0 + 4.0 * m;
    for (i, len) in [14.0, 9.0, 9.0, 11.0].iter().enumerate() {
        let col = if i == 0 { p.text } else { p.text3 };
        c.fill_rect(Rect::new(x, line_y, x + len, line_y + 2.0), color(col));
        x += len + 5.0;
    }
    // The clock, and the current desk on `selection`.
    c.fill_rect(Rect::new(r.x1 - 18.0, line_y, r.x1 - 5.0, line_y + 2.0), color(p.text));
    let desk = Rect::new(r.x0 + w * 0.56, r.y0 + 1.0, r.x0 + w * 0.56 + 14.0, r.y0 + bar_h - 1.0);
    c.fill_rect(snap(desk), color(p.selection));

    let shadow = |c: &mut Canvas, rr: Rect| {
        if effects.shadows() {
            c.fill_rect(Rect::new(rr.x0 + 2.0, rr.y1, rr.x1 + 2.0, rr.y1 + 2.0), color(p.slab));
            c.fill_rect(Rect::new(rr.x1, rr.y0 + 2.0, rr.x1 + 2.0, rr.y1), color(p.slab));
        }
    };

    // The Deskbar, top right.
    let deskbar = at(0.80, 0.0, 0.20, 0.42);
    let deskbar = Rect::new(deskbar.x0, bar.y1 + 3.0, r.x1 - 3.0, deskbar.y1);
    shadow(c, deskbar);
    c.fill_rect(deskbar, color(p.chrome));
    draw::border(c, deskbar, color(p.bevel_lo), 1.0);
    let mut y = deskbar.y0 + 4.0;
    for i in 0..4 {
        let (ww, col) = if i == 1 { (0.7, p.text) } else { (0.5, p.text3) };
        if i == 1 {
            c.fill_rect(Rect::new(deskbar.x0 + 2.0, y - 1.0, deskbar.x0 + 4.0, y + 3.0), color(p.focus));
        }
        c.fill_rect(
            Rect::new(deskbar.x0 + 6.0, y, deskbar.x0 + 6.0 + (deskbar.width() - 10.0) * ww, y + 2.0),
            color(col),
        );
        y += 6.0;
    }

    // The window behind, without focus.
    let back = at(0.07, 0.24, 0.42, 0.40);
    let tab_h = (h * 0.07).max(5.0).round();
    let back_tab = Rect::new(back.x0, back.y0 - tab_h, back.x0 + (back.width() * 0.5).round(), back.y0);
    shadow(c, back.union(back_tab));
    c.fill_rect(back_tab, color(p.tab_idle));
    c.fill_rect(
        Rect::new(
            back_tab.x0 + 3.0,
            back_tab.y0 + (tab_h / 2.0).round() - 1.0,
            back_tab.x1 - 4.0,
            back_tab.y0 + (tab_h / 2.0).round() + 1.0,
        ),
        color(p.tab_idle_text.with_alpha(0.7)),
    );
    c.fill_rect(back, color(p.surface));
    draw::border(c, back, color(p.bevel_lo), 1.0);
    let mut y = back.y0 + 5.0;
    while y < back.y1 - 4.0 {
        c.fill_rect(Rect::new(back.x0 + 5.0, y, back.x0 + back.width() * 0.7, y + 1.0), color(p.faint));
        y += 5.0;
    }

    // The focused window: its amber tab, the rail, a sidebar and a
    // selected row.
    let front = at(0.27, 0.42, 0.46, 0.44);
    let tab = Rect::new(front.x0, front.y0 - tab_h, front.x0 + (front.width() * 0.55).round(), front.y0);
    shadow(c, front.union(tab));
    c.fill_rect(tab, color(p.focus));
    let box_s = (tab_h - 3.0).max(2.0);
    draw::border(
        c,
        Rect::new(tab.x0 + 2.0, tab.y0 + 1.5, tab.x0 + 2.0 + box_s, tab.y0 + 1.5 + box_s),
        color(p.on_focus),
        1.0,
    );
    c.fill_rect(
        Rect::new(
            tab.x0 + box_s + 5.0,
            tab.y0 + (tab_h / 2.0).round() - 1.0,
            tab.x1 - 4.0,
            tab.y0 + (tab_h / 2.0).round() + 1.0,
        ),
        color(p.on_focus),
    );
    c.fill_rect(front, color(p.surface));
    let side = Rect::new(front.x0, front.y0, front.x0 + (front.width() * 0.3).round(), front.y1);
    c.fill_rect(side, color(p.chrome));
    c.fill_rect(Rect::new(side.x1 - 1.0, side.y0, side.x1, side.y1), color(p.line));
    let mut y = side.y0 + 5.0;
    for i in 0..4 {
        if i == 1 {
            c.fill_rect(Rect::new(side.x0 + 1.0, y - 2.0, side.x1 - 1.0, y + 3.0), color(p.selection));
            c.fill_rect(Rect::new(side.x0 + 4.0, y, side.x1 - 6.0, y + 1.0), color(p.on_selection));
        } else {
            c.fill_rect(Rect::new(side.x0 + 4.0, y, side.x1 - 8.0, y + 1.0), color(p.text3));
        }
        y += 6.0;
    }
    let mut y = front.y0 + 6.0;
    let mut i = 0;
    while y < front.y1 - 5.0 {
        let (len, col) = match i % 4 {
            0 => (0.55, p.blue),
            3 => (0.30, p.text2),
            _ => (0.62, p.text2),
        };
        let x0 = side.x1 + 5.0;
        c.fill_rect(Rect::new(x0, y, x0 + (front.x1 - x0 - 6.0) * len, y + 1.0), color(col));
        y += 5.0;
        i += 1;
    }
    draw::border(c, front, color(p.bevel_lo), 1.0);
    c.fill_rect(Rect::new(front.x0, front.y0, front.x1, front.y0 + 1.0), color(p.focus));
}
