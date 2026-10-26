//! Haiku tabs and frames, drawn by the compositor.
//!
//! Metrics come from the boards' CSS (`reference/html/C2-Desktop.html`):
//! a tab 22pt tall sitting on the frame's top edge, with a 1pt `bevel_lo`
//! border on three sides and a 1pt highlight inside it; an 11pt close box
//! 6pt in from the left, the title 8pt after it in Archivo 650 at 87.5%
//! width, and an 11pt zoom box 6pt in from the right. The frame is a 1pt
//! `bevel_lo` edge around the content with a 2pt rail along the top (the
//! focus colour when focused, a quiet band otherwise), and a hard `slab`
//! shadow 3pt right and down that takes in the tab.
//!
//! Layout and hit testing are plain geometry; [`DecorCache`] turns the
//! pieces into solid-colour render elements whose identities persist, so
//! the damage tracker redraws only what moved or changed colour.

use aizome_theme::{Palette, Rgba, metrics};

pub use aizome_comp_core::pieces::{DecorCache, Piece, to_physical};

use crate::wm::{Insets, Rect, WinId};

pub const BORDER: i32 = metrics::FRAME_EDGE as i32;
pub const RAIL: i32 = metrics::FOCUS_RAIL as i32;
pub const TAB_H: i32 = metrics::TAB_HEIGHT as i32;
pub const SHADOW: i32 = metrics::SHADOW as i32;
pub const REACH: i32 = metrics::RESIZE_REACH as i32;
/// How far around a window's content its decorations reach.
pub const OUTSET: i32 = TAB_H + RAIL + BORDER + SHADOW + REACH;
/// Between the tab's border and its boxes.
pub const TAB_EDGE: f64 = 6.0;
pub const BOX: f64 = 11.0;
pub const BOX_STROKE: f64 = 1.5;
/// Between a box and the title.
pub const GAP: f64 = 8.0;
/// A tab's width beyond its title: border, edge, box and gap on each side.
pub const TAB_CHROME: f64 = 2.0 * (1.0 + TAB_EDGE + BOX + GAP);
/// Corners: this far along an edge from a corner resizes both ways.
pub const CORNER: i32 = 16;
/// The idle app glyph in place of the close box.
pub const GLYPH: f64 = aizome_comp_core::icons::GLYPH;

/// The space decorations take around a window's content.
pub fn insets(ssd: bool) -> Insets {
    if ssd { Insets::new(TAB_H + BORDER + RAIL, BORDER, BORDER, BORDER) } else { Insets::new(TAB_H, 0, 0, 0) }
}

/// A tab's width for a title this wide (points).
pub fn tab_width(title_w: f64) -> i32 {
    ((title_w + TAB_CHROME).ceil() as i32).max(metrics::TAB_MIN_WIDTH as i32)
}

/// The widest a lone tab gets, whatever its title.
pub const TAB_MAX: i32 = 360;
/// A lone tab leaves at least this much of the top edge free (for sliding
/// and for the look).
pub const TAB_SPARE: i32 = 32;

/// The widths of a group's tabs (one or more) along a frame `frame_w`
/// wide: each its title's natural width, at most [`TAB_MAX`] (and, alone,
/// [`TAB_SPARE`] short of the frame); when they don't fit side by side,
/// all shrink by the same factor, down to [`metrics::TAB_MIN_WIDTH`].
pub fn tab_widths(frame_w: i32, titles: &[f64]) -> Vec<i32> {
    let min = metrics::TAB_MIN_WIDTH as i32;
    let natural: Vec<i32> = titles.iter().map(|t| tab_width(*t).min(TAB_MAX)).collect();
    if natural.len() <= 1 {
        let cap = (frame_w - TAB_SPARE).min(TAB_MAX).max(min);
        return natural.into_iter().map(|w| w.min(cap)).collect();
    }
    let total: i32 = natural.iter().sum();
    if total <= frame_w {
        return natural;
    }
    let k = frame_w as f64 / total as f64;
    natural.into_iter().map(|w| ((w as f64 * k).floor() as i32).max(min)).collect()
}

/// How a tab is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabLook {
    /// The focused window's tab: `focus`, close and zoom boxes.
    Focused,
    /// An unfocused window's tab: `tab_idle`, an app glyph.
    Idle,
    /// A stacked window that isn't showing: `tab_idle`, dimmer.
    Background,
}

#[derive(Clone, Debug)]
pub struct TabLayout {
    pub win: WinId,
    pub rect: Rect,
    pub look: TabLook,
    pub close: Rect,
    pub zoom: Rect,
    /// Where the title goes, and how wide it may be.
    pub title_x: f64,
    pub title_max_w: f64,
}

#[derive(Clone, Debug)]
pub struct Layout {
    /// The window (group) whose frame this is.
    pub win: WinId,
    pub content: Rect,
    /// The frame's outer rectangle (the content when there is no frame).
    pub frame: Rect,
    pub ssd: bool,
    pub focused: bool,
    pub tabs: Vec<TabLayout>,
}

impl Layout {
    /// Everything decorations cover, shadow and resize reach included.
    pub fn bounds(&self) -> Rect {
        let mut r = self.frame.grow(REACH);
        for t in &self.tabs {
            r = r.union(t.rect);
        }
        Rect::new(r.x, r.y, r.w + SHADOW, r.h + SHADOW)
    }
}

/// Lays out a window's frame and its group's tabs. `members` are the
/// group's windows in tab order with their titles' widths; `active` is the
/// one showing.
pub fn layout(
    content: Rect,
    ssd: bool,
    focused: bool,
    members: &[(WinId, f64)],
    active: WinId,
    strip_offset: i32,
) -> Layout {
    let frame = if ssd { content.outset(Insets::new(BORDER + RAIL, BORDER, BORDER, BORDER)) } else { content };
    let widths = tab_widths(frame.w, &members.iter().map(|(_, w)| *w).collect::<Vec<_>>());
    let total: i32 = widths.iter().sum();
    let offset = strip_offset.clamp(0, (frame.w - total).max(0));
    let mut x = frame.x + offset;
    let y = frame.y - TAB_H;
    let mut tabs = Vec::with_capacity(members.len());
    for ((win, _), w) in members.iter().zip(widths) {
        let look = if *win != active {
            TabLook::Background
        } else if focused {
            TabLook::Focused
        } else {
            TabLook::Idle
        };
        let rect = Rect::new(x, y, w, TAB_H);
        let by = y + 6; // centred in the 21pt under the top border
        let close = Rect::new(x + 1 + TAB_EDGE as i32, by, BOX as i32, BOX as i32);
        let zoom = Rect::new(x + w - 1 - TAB_EDGE as i32 - BOX as i32, by, BOX as i32, BOX as i32);
        let title_x = (x as f64) + 1.0 + TAB_EDGE + BOX + GAP;
        let title_max_w = (w as f64 - TAB_CHROME).max(0.0);
        tabs.push(TabLayout { win: *win, rect, look, close, zoom, title_x, title_max_w });
        x += w;
    }
    Layout { win: active, content, frame, ssd, focused, tabs }
}

/// What a point on the decorations does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Close(WinId),
    Zoom(WinId),
    /// The tab itself (drag to move, click to show).
    Tab(WinId),
    /// A frame edge or corner: resize.
    Edge(WinId, Edges),
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Edges: u8 {
        const TOP = 1;
        const BOTTOM = 2;
        const LEFT = 4;
        const RIGHT = 8;
    }
}

impl Edges {
    /// The cursor for resizing these edges.
    pub fn cursor(self) -> smithay::input::pointer::CursorIcon {
        use smithay::input::pointer::CursorIcon as C;
        match (
            self.contains(Edges::TOP),
            self.contains(Edges::BOTTOM),
            self.contains(Edges::LEFT),
            self.contains(Edges::RIGHT),
        ) {
            (true, _, true, _) => C::NwResize,
            (true, _, _, true) => C::NeResize,
            (_, true, true, _) => C::SwResize,
            (_, true, _, true) => C::SeResize,
            (true, _, _, _) => C::NResize,
            (_, true, _, _) => C::SResize,
            (_, _, true, _) => C::WResize,
            _ => C::EResize,
        }
    }

    /// The xcursor name for resizing these edges.
    pub fn cursor_name(self) -> &'static str {
        match (
            self.contains(Edges::TOP),
            self.contains(Edges::BOTTOM),
            self.contains(Edges::LEFT),
            self.contains(Edges::RIGHT),
        ) {
            (true, _, true, _) => "nw-resize",
            (true, _, _, true) => "ne-resize",
            (_, true, true, _) => "sw-resize",
            (_, true, _, true) => "se-resize",
            (true, _, _, _) => "n-resize",
            (_, true, _, _) => "s-resize",
            (_, _, true, _) => "w-resize",
            _ => "e-resize",
        }
    }
}

/// What the decorations do at a point, if anything (the content is the
/// client's).
pub fn hit(l: &Layout, x: f64, y: f64) -> Option<Hit> {
    for t in &l.tabs {
        if t.rect.contains(x, y) {
            if t.look == TabLook::Focused && t.close.grow(3).contains(x, y) {
                return Some(Hit::Close(t.win));
            }
            if t.look == TabLook::Focused && t.zoom.grow(3).contains(x, y) {
                return Some(Hit::Zoom(t.win));
            }
            return Some(Hit::Tab(t.win));
        }
    }
    if !l.ssd || l.content.contains(x, y) {
        return None;
    }
    let reach = l.frame.grow(REACH);
    if !reach.contains(x, y) {
        return None;
    }
    let c = l.content;
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let mut e = Edges::empty();
    if xi < c.x {
        e |= Edges::LEFT;
    } else if xi >= c.right() {
        e |= Edges::RIGHT;
    }
    if yi < c.y {
        e |= Edges::TOP;
    } else if yi >= c.bottom() {
        e |= Edges::BOTTOM;
    }
    // Near a corner, both ways.
    if e == Edges::LEFT || e == Edges::RIGHT {
        if yi < c.y + CORNER {
            e |= Edges::TOP;
        } else if yi >= c.bottom() - CORNER {
            e |= Edges::BOTTOM;
        }
    } else if e == Edges::TOP || e == Edges::BOTTOM {
        if xi < c.x + CORNER {
            e |= Edges::LEFT;
        } else if xi >= c.right() - CORNER {
            e |= Edges::RIGHT;
        }
    }
    (!e.is_empty()).then_some(Hit::Edge(l.win, e))
}

/// How thick a box outline's stroke is at a scale, in points: 1.5pt
/// rounded to whole pixels.
pub fn stroke(scale: f64) -> f64 {
    // Halves round down: 1pt at 1x, 3px at 2x.
    (BOX_STROKE * scale - 0.01).round().max(1.0) / scale
}

/// An outlined square, as four pieces.
pub fn outline(out: &mut Vec<Piece>, r: Rect, s: f64, color: Rgba) {
    let (x, y, w, h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    out.push(Piece::new(x, y, w, s, color));
    out.push(Piece::new(x, y + h - s, w, s, color));
    out.push(Piece::new(x, y + s, s, h - 2.0 * s, color));
    out.push(Piece::new(x + w - s, y + s, s, h - 2.0 * s, color));
}

/// The pieces of a window's decorations, back to front: shadow, ground, frame,
/// rail, then each tab (border, fill, highlight, boxes). Titles and
/// glyphs are separate textures placed by the caller.
pub fn pieces(l: &Layout, p: &Palette, shadows: bool, scale: f64) -> Vec<Piece> {
    let mut out = Vec::with_capacity(32);
    let f = l.frame;
    if shadows {
        let s = SHADOW;
        if l.ssd {
            // Right strip from 3pt below the top, bottom strip from 3pt in.
            out.push(Piece::rect(Rect::new(f.right(), f.y + s, s, f.h), p.slab));
            out.push(Piece::rect(Rect::new(f.x + s, f.bottom(), f.w - s, s), p.slab));
        }
        if let Some(last) = l.tabs.last() {
            // Beside the strip of tabs, down to the frame.
            let t = last.rect;
            if t.right() < f.right() || !l.ssd {
                out.push(Piece::rect(Rect::new(t.right(), t.y + s, s, t.h - s), p.slab));
            }
        }
    }
    if l.ssd {
        let c = l.content;
        // The window's ground, under the client: a client smaller than its
        // frame (a fixed-size one stacked with a larger window, or one
        // still drawing at its old size) shows it rather than a hole onto
        // the windows behind. Where the client is opaque it covers this,
        // and the damage tracker skips it.
        out.push(Piece::rect(c, p.surface));
        // The frame's edge.
        out.push(Piece::rect(Rect::new(f.x, f.y, f.w, BORDER), p.bevel_lo));
        out.push(Piece::rect(Rect::new(f.x, f.bottom() - BORDER, f.w, BORDER), p.bevel_lo));
        out.push(Piece::rect(Rect::new(f.x, f.y + BORDER, BORDER, f.h - 2 * BORDER), p.bevel_lo));
        out.push(Piece::rect(Rect::new(f.right() - BORDER, f.y + BORDER, BORDER, f.h - 2 * BORDER), p.bevel_lo));
        // The rail.
        let rail = Rect::new(c.x, c.y - RAIL, c.w, RAIL);
        if l.focused {
            out.push(Piece::rect(rail, p.focus));
        } else {
            out.push(Piece::rect(rail, p.tab_idle));
            out.push(Piece::rect(Rect::new(rail.x, rail.y, rail.w, 1), p.bevel_hi));
        }
    }
    let sw = stroke(scale);
    for t in &l.tabs {
        let r = t.rect;
        let (fill, ink, hi) = tab_colors(t.look, p);
        // Border on three sides; the frame's top edge runs beneath.
        out.push(Piece::rect(Rect::new(r.x, r.y, r.w, BORDER), p.bevel_lo));
        out.push(Piece::rect(Rect::new(r.x, r.y + BORDER, BORDER, r.h - BORDER), p.bevel_lo));
        out.push(Piece::rect(Rect::new(r.right() - BORDER, r.y + BORDER, BORDER, r.h - BORDER), p.bevel_lo));
        out.push(Piece::rect(Rect::new(r.x + BORDER, r.y + BORDER, r.w - 2 * BORDER, r.h - BORDER), fill));
        out.push(Piece::rect(Rect::new(r.x + BORDER, r.y + BORDER, r.w - 2 * BORDER, 1), hi));
        if t.look == TabLook::Focused {
            outline(&mut out, t.close, sw, ink);
            outline(&mut out, t.zoom, sw, ink);
            // The zoom box's small square in its top-left corner.
            outline(&mut out, Rect::new(t.zoom.x, t.zoom.y, 6, 6), sw, ink);
        }
    }
    out
}

/// A tab's fill, ink (text and boxes) and top highlight.
pub fn tab_colors(look: TabLook, p: &Palette) -> (Rgba, Rgba, Rgba) {
    match look {
        TabLook::Focused => (p.focus, p.on_focus, p.focus.mix(Rgba::hex(0xffffff), 0.35)),
        TabLook::Idle => (p.tab_idle, p.tab_idle_text, blend(p.tab_idle, p.bevel_hi)),
        TabLook::Background => {
            (p.tab_idle.mix(p.chrome, 0.35), p.tab_idle_text.mix(p.tab_idle, 0.3), blend(p.tab_idle, p.bevel_hi))
        }
    }
}

/// `top` over `bottom`, opaque.
pub fn blend(bottom: Rgba, top: Rgba) -> Rgba {
    let a = top.a as f32 / 255.0;
    let m = |b: u8, t: u8| (b as f32 * (1.0 - a) + t as f32 * a).round() as u8;
    Rgba { r: m(bottom.r, top.r), g: m(bottom.g, top.g), b: m(bottom.b, top.b), a: 255 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single(ssd: bool, focused: bool) -> Layout {
        layout(Rect::new(100, 100, 400, 300), ssd, focused, &[(1, 100.0)], 1, 0)
    }

    #[test]
    fn frame_and_tab_sit_outside_the_content() {
        let l = single(true, true);
        assert_eq!(l.frame, Rect::new(99, 97, 402, 304));
        let t = &l.tabs[0];
        assert_eq!(t.rect, Rect::new(99, 75, 152, 22));
        assert_eq!(t.close, Rect::new(106, 81, 11, 11));
        assert_eq!(t.zoom, Rect::new(99 + 152 - 18, 81, 11, 11));
        assert_eq!(t.title_x, 99.0 + 26.0);
        // The insets agree with the layout.
        let i = insets(true);
        assert_eq!(l.content.y - i.top, t.rect.y);
        assert_eq!(l.content.x - i.left, l.frame.x);
    }

    #[test]
    fn tabs_are_as_wide_as_their_titles_within_bounds() {
        assert_eq!(tab_width(0.0), 64);
        assert_eq!(tab_width(100.0), 152);
        let l = layout(Rect::new(0, 50, 120, 100), true, true, &[(1, 500.0)], 1, 0);
        assert_eq!(l.tabs[0].rect.w, 122 - TAB_SPARE, "leaves some of the top edge");
        let l = layout(Rect::new(0, 50, 1200, 100), true, true, &[(1, 900.0)], 1, 0);
        assert_eq!(l.tabs[0].rect.w, TAB_MAX, "never wider than the cap");
        assert_eq!(l.tabs[0].title_max_w, TAB_MAX as f64 - TAB_CHROME);
    }

    #[test]
    fn stacked_tabs_shrink_evenly_when_they_dont_fit() {
        // Natural widths 152 and 252 in a 300pt frame: both scale by 300/404.
        let w = tab_widths(300, &[100.0, 200.0]);
        assert_eq!(w, vec![112, 187]);
        assert!(w.iter().sum::<i32>() <= 300);
        // They fit: natural widths.
        assert_eq!(tab_widths(800, &[100.0, 200.0]), vec![152, 252]);
        // Never under the minimum.
        assert!(tab_widths(100, &[100.0, 200.0, 300.0]).iter().all(|w| *w == 64));
    }

    #[test]
    fn stacked_tabs_sit_side_by_side() {
        let l = layout(Rect::new(0, 50, 800, 400), true, false, &[(1, 60.0), (2, 80.0), (3, 40.0)], 2, 0);
        assert_eq!(l.tabs.len(), 3);
        assert_eq!(l.tabs[1].rect.x, l.tabs[0].rect.right());
        assert_eq!(l.tabs[2].rect.x, l.tabs[1].rect.right());
        assert_eq!(l.tabs[1].look, TabLook::Idle);
        assert_eq!(l.tabs[0].look, TabLook::Background);
    }

    #[test]
    fn tab_offset_slides_but_stays_on_the_frame() {
        let l = layout(Rect::new(0, 50, 800, 400), true, true, &[(1, 100.0)], 1, 300);
        assert_eq!(l.tabs[0].rect.x, -1 + 300);
        let l = layout(Rect::new(0, 50, 800, 400), true, true, &[(1, 100.0)], 1, 5000);
        assert_eq!(l.tabs[0].rect.right(), l.frame.right());
    }

    #[test]
    fn hits() {
        let l = single(true, true);
        assert_eq!(hit(&l, 110.0, 85.0), Some(Hit::Close(1)));
        assert_eq!(hit(&l, 237.0, 85.0), Some(Hit::Zoom(1)));
        assert_eq!(hit(&l, 170.0, 85.0), Some(Hit::Tab(1)));
        assert_eq!(hit(&l, 300.0, 300.0), None, "content is the client's");
        assert_eq!(hit(&l, 96.0, 300.0), Some(Hit::Edge(1, Edges::LEFT)));
        assert_eq!(hit(&l, 504.0, 300.0), Some(Hit::Edge(1, Edges::RIGHT)));
        assert_eq!(hit(&l, 300.0, 403.0), Some(Hit::Edge(1, Edges::BOTTOM)));
        assert_eq!(hit(&l, 300.0, 96.0), Some(Hit::Edge(1, Edges::TOP)));
        assert_eq!(hit(&l, 503.0, 402.0), Some(Hit::Edge(1, Edges::BOTTOM | Edges::RIGHT)));
        assert_eq!(hit(&l, 96.0, 105.0), Some(Hit::Edge(1, Edges::LEFT | Edges::TOP)));
        assert_eq!(hit(&l, 90.0, 300.0), None, "beyond the reach");
        // Without a frame, only the tab.
        let l = single(false, true);
        assert_eq!(hit(&l, 98.0, 300.0), None);
        assert!(matches!(hit(&l, 120.0, 90.0), Some(Hit::Close(1)) | Some(Hit::Tab(1))));
    }

    #[test]
    fn pieces_snap_to_pixels() {
        let r = to_physical(10.0, 10.0, 1.5, 1.5, (0.0, 0.0), 2.0);
        assert_eq!((r.loc.x, r.loc.y, r.size.w, r.size.h), (20, 20, 3, 3));
        assert_eq!(stroke(1.0), 1.0);
        assert_eq!(stroke(2.0), 1.5);
    }
}
