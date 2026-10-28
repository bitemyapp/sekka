//! Where the pages sit: one column, top to bottom, each centred, with a
//! gap between them; how large (the zoom); which show; and the tiles to
//! draw them in.
//!
//! The document's coordinates are points from the column's top left, the
//! view's are points from its own; a page's are points of the page as it
//! shows (`protocol`). A zoom of 1 shows a page point as a point of the
//! screen, which is how large the page was meant to be.

use std::ops::Range;

use aizome_kit::prelude::{Point, Rect, Size};

use crate::protocol::{PageRect, TileKey};

/// Between pages, and around the column.
pub const GAP: f64 = 12.0;
pub const MARGIN: f64 = 16.0;

pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 16.0;

/// The zooms ❖+ and ❖− step through.
pub const STEPS: [f64; 19] =
    [0.1, 0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 6.0, 8.0, 16.0];

/// The side of a tile, in device pixels.
pub const TILE: u32 = 1024;

/// How large pages show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// The widest page fills the view's width.
    FitWidth,
    /// The largest page shows whole.
    FitPage,
    /// Screen points per page point.
    Fixed(f64),
}

impl Zoom {
    pub fn resolve(self, pages: &[(f32, f32)], view: Size) -> f64 {
        let widest = pages.iter().map(|p| f64::from(p.0)).fold(0.0, f64::max);
        let tallest = pages.iter().map(|p| f64::from(p.1)).fold(0.0, f64::max);
        let room = |v: f64| (v - 2.0 * MARGIN).max(40.0);
        let z = match self {
            Zoom::Fixed(z) => z,
            _ if widest <= 0.0 || tallest <= 0.0 => 1.0,
            Zoom::FitWidth => room(view.width) / widest,
            Zoom::FitPage => (room(view.width) / widest).min(room(view.height) / tallest),
        };
        clamp_zoom(z)
    }
}

pub fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() { z.clamp(MIN_ZOOM, MAX_ZOOM) } else { 1.0 }
}

/// The next step in from `z`.
pub fn step_in(z: f64) -> f64 {
    STEPS.iter().copied().find(|s| *s > z * 1.001).unwrap_or(MAX_ZOOM)
}

/// The next step out from `z`.
pub fn step_out(z: f64) -> f64 {
    STEPS.iter().rev().copied().find(|s| *s < z / 1.001).unwrap_or(MIN_ZOOM)
}

/// The pages laid out at a zoom.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub zoom: f64,
    /// Each page's size in page points.
    pub pages: Vec<(f64, f64)>,
    /// Each page's top, down the document.
    pub tops: Vec<f64>,
    /// The document's size: the column, and at least the view's width.
    pub size: Size,
}

impl Layout {
    pub fn new(pages: &[(f32, f32)], zoom: f64, view_width: f64) -> Layout {
        let pages: Vec<(f64, f64)> = pages.iter().map(|p| (f64::from(p.0), f64::from(p.1))).collect();
        let mut tops = Vec::with_capacity(pages.len());
        let mut y = MARGIN;
        for p in &pages {
            tops.push(y);
            y += p.1 * zoom + GAP;
        }
        let height = if pages.is_empty() { 0.0 } else { y - GAP + MARGIN };
        let widest = pages.iter().map(|p| p.0).fold(0.0, f64::max) * zoom;
        let width = (widest + 2.0 * MARGIN).max(view_width);
        Layout { zoom, pages, tops, size: Size::new(width, height) }
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Page `i`'s rectangle in the document.
    pub fn page_rect(&self, i: usize) -> Rect {
        let (w, h) = (self.pages[i].0 * self.zoom, self.pages[i].1 * self.zoom);
        let x = ((self.size.width - w) / 2.0).round();
        Rect::new(x, self.tops[i], x + w, self.tops[i] + h)
    }

    /// The pages any part of which is between `top` and `bottom`.
    pub fn visible(&self, top: f64, bottom: f64) -> Range<usize> {
        let first = self.tops.partition_point(|t| *t <= top).saturating_sub(1);
        let mut first = first.min(self.len());
        while first < self.len() && self.page_rect(first).y1 < top {
            first += 1;
        }
        let end = self.tops.partition_point(|t| *t < bottom).max(first);
        first..end
    }

    /// The page at `y` down the document: the one it's on, or, between
    /// two, the nearer.
    pub fn page_at(&self, y: f64) -> usize {
        if self.is_empty() {
            return 0;
        }
        let i = self.tops.partition_point(|t| *t <= y).saturating_sub(1);
        let bottom = self.page_rect(i).y1;
        if y > bottom && i + 1 < self.len() && y - bottom > (self.tops[i + 1] - y) { i + 1 } else { i }
    }

    /// The page under `p` (document points) and where on it, in page
    /// points.
    pub fn hit(&self, p: Point) -> Option<(usize, (f64, f64))> {
        if self.is_empty() {
            return None;
        }
        let i = self.page_at(p.y);
        let r = self.page_rect(i);
        r.contains(p).then(|| (i, ((p.x - r.x0) / self.zoom, (p.y - r.y0) / self.zoom)))
    }

    /// Where `at` on page `i` (page points) is in the document.
    pub fn to_doc(&self, i: usize, at: (f64, f64)) -> Point {
        let r = self.page_rect(i);
        Point::new(r.x0 + at.0 * self.zoom, r.y0 + at.1 * self.zoom)
    }

    /// A page rectangle in the document.
    pub fn rect_to_doc(&self, i: usize, r: &PageRect) -> Rect {
        let a = self.to_doc(i, (f64::from(r[0]), f64::from(r[1])));
        let b = self.to_doc(i, (f64::from(r[2]), f64::from(r[3])));
        Rect::new(a.x, a.y, b.x, b.y)
    }

    /// Where `p` (document points) is on page `i`, in page points, whether
    /// on it or not.
    pub fn on_page(&self, i: usize, p: Point) -> (f64, f64) {
        let r = self.page_rect(i);
        ((p.x - r.x0) / self.zoom, (p.y - r.y0) / self.zoom)
    }

    /// Page `i`'s size in device pixels at `scale`.
    pub fn device_size(&self, i: usize, scale: f64) -> (u32, u32) {
        let (w, h) = self.pages[i];
        (((w * self.zoom * scale).round() as u32).max(1), ((h * self.zoom * scale).round() as u32).max(1))
    }

    /// The tiles of page `i` that cover `area` (document points) at
    /// `scale`, nearest `centre` first.
    pub fn tiles(&self, i: usize, scale: f64, area: Rect, centre: Point) -> Vec<TileKey> {
        let r = self.page_rect(i);
        let a = r.intersect(area);
        if a.width() <= 0.0 || a.height() <= 0.0 {
            return Vec::new();
        }
        let size = self.device_size(i, scale);
        let px = |v: f64, max: u32| ((v * scale).max(0.0) as u32).min(max);
        let (x0, x1) = (px(a.x0 - r.x0, size.0), px(a.x1 - r.x0, size.0).max(1));
        let (y0, y1) = (px(a.y0 - r.y0, size.1), px(a.y1 - r.y0, size.1).max(1));
        let mut out = Vec::new();
        for ty in y0 / TILE..=(y1 - 1) / TILE {
            for tx in x0 / TILE..=(x1 - 1) / TILE {
                let at = (tx * TILE, ty * TILE);
                if at.0 >= size.0 || at.1 >= size.1 {
                    continue;
                }
                out.push(TileKey { page: i as u32, size, at, w: TILE.min(size.0 - at.0), h: TILE.min(size.1 - at.1) });
            }
        }
        let dist = |k: &TileKey| {
            let cx = r.x0 + (f64::from(k.at.0) + f64::from(k.w) / 2.0) / scale;
            let cy = r.y0 + (f64::from(k.at.1) + f64::from(k.h) / 2.0) / scale;
            (cx - centre.x).powi(2) + (cy - centre.y).powi(2)
        };
        out.sort_by(|a, b| dist(a).total_cmp(&dist(b)));
        out
    }

    /// The small drawing of page `i` that stands in while its tiles come,
    /// and in the sidebar: `width` device pixels wide.
    pub fn preview(&self, i: usize, width: u32) -> TileKey {
        let (w, h) = self.pages[i];
        let size = (width.max(1), ((f64::from(width) * h / w.max(1.0)).round() as u32).clamp(1, width * 4));
        TileKey { page: i as u32, size, at: (0, 0), w: size.0, h: size.1 }
    }

    /// The scroll offset that puts `at` (document points) at `view_at`
    /// (view points), within what scrolls.
    pub fn clamp_scroll(&self, scroll: (f64, f64), view: Size) -> (f64, f64) {
        let max = ((self.size.width - view.width).max(0.0), (self.size.height - view.height).max(0.0));
        (scroll.0.clamp(0.0, max.0).round(), scroll.1.clamp(0.0, max.1).round())
    }
}

/// Where the view is, by what it shows rather than by offset: the page
/// at a point of the view and where on it. Kept across zooms and resizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub page: usize,
    /// Page points (may be off the page, in a gap).
    pub at: (f64, f64),
    /// The point of the view it's under.
    pub view_at: (f64, f64),
}

impl Anchor {
    /// What's under `view_at` with the document scrolled to `scroll`.
    pub fn of(layout: &Layout, scroll: (f64, f64), view_at: (f64, f64)) -> Anchor {
        let p = Point::new(scroll.0 + view_at.0, scroll.1 + view_at.1);
        let page = layout.page_at(p.y);
        Anchor { page, at: if layout.is_empty() { (0.0, 0.0) } else { layout.on_page(page, p) }, view_at }
    }

    /// The scroll that puts it back under its point in `layout`.
    pub fn scroll(&self, layout: &Layout, view: Size) -> (f64, f64) {
        if layout.is_empty() {
            return (0.0, 0.0);
        }
        let page = self.page.min(layout.len() - 1);
        let p = layout.to_doc(page, self.at);
        layout.clamp_scroll((p.x - self.view_at.0, p.y - self.view_at.1), view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: (f32, f32) = (612.0, 792.0);

    #[test]
    fn pages_stack_centred() {
        let l = Layout::new(&[LETTER, (792.0, 612.0), LETTER], 1.0, 1000.0);
        assert_eq!(l.tops, vec![MARGIN, MARGIN + 792.0 + GAP, MARGIN + 792.0 + 612.0 + 2.0 * GAP]);
        assert_eq!(l.size, Size::new(1000.0, MARGIN * 2.0 + 792.0 * 2.0 + 612.0 + 2.0 * GAP));
        assert_eq!(l.page_rect(0).x0, 194.0);
        assert_eq!(l.page_rect(1).x0, 104.0);
        // Wider than the view: the column sets the width.
        let wide = Layout::new(&[LETTER], 2.0, 800.0);
        assert_eq!(wide.size.width, 1224.0 + 2.0 * MARGIN);
        assert_eq!(wide.page_rect(0).x0, MARGIN);
    }

    #[test]
    fn fits() {
        let pages = [LETTER, LETTER];
        let view = Size::new(644.0, 400.0);
        assert!((Zoom::FitWidth.resolve(&pages, view) - 1.0).abs() < 1e-9);
        assert!((Zoom::FitPage.resolve(&pages, view) - (400.0 - 32.0) / 792.0).abs() < 1e-9);
        assert_eq!(Zoom::Fixed(100.0).resolve(&pages, view), MAX_ZOOM);
        assert_eq!(Zoom::FitWidth.resolve(&[], view), 1.0);
    }

    #[test]
    fn steps() {
        assert_eq!(step_in(1.0), 1.1);
        assert_eq!(step_out(1.0), 0.9);
        assert_eq!(step_in(1.05), 1.1);
        assert_eq!(step_out(0.1), MIN_ZOOM);
        assert_eq!(step_in(16.0), MAX_ZOOM);
    }

    #[test]
    fn what_shows_and_what_is_where() {
        let l = Layout::new(&[LETTER; 10], 1.0, 800.0);
        assert_eq!(l.visible(0.0, 500.0), 0..1);
        // The gap between pages one and two.
        let gap = MARGIN + 792.0 + 5.0;
        assert_eq!(l.visible(gap, gap + 3.0), 1..1);
        assert_eq!(l.visible(gap, gap + 900.0), 1..3);
        assert_eq!(l.visible(1e9, 1e9 + 10.0), 10..10);
        assert_eq!(l.page_at(MARGIN + 792.0 + 8.0), 1);
        assert_eq!(l.page_at(MARGIN + 792.0 + 2.0), 0);
        let (i, at) = l.hit(Point::new(94.0 + 72.0, MARGIN + 804.0 + 100.0)).unwrap();
        assert_eq!(i, 1);
        assert!((at.0 - 72.0).abs() < 1e-9 && (at.1 - 100.0).abs() < 1e-9);
        assert!(l.hit(Point::new(10.0, 100.0)).is_none());
    }

    #[test]
    fn tiles_cover_what_shows_and_no_more() {
        // At 2× a letter page is 1224 × 1584: two tiles across, two down.
        let l = Layout::new(&[LETTER], 1.0, 644.0);
        let r = l.page_rect(0);
        let all = l.tiles(0, 2.0, r, r.center());
        assert_eq!(all.len(), 4);
        assert_eq!(all.iter().map(|t| t.w as u64 * t.h as u64).sum::<u64>(), 1224 * 1584);
        assert!(all.iter().all(|t| t.size == (1224, 1584)));
        // The top left corner alone: one tile.
        let corner = Rect::new(r.x0, r.y0, r.x0 + 100.0, r.y0 + 100.0);
        let one = l.tiles(0, 2.0, corner, corner.center());
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].at, (0, 0));
        // Off the page: none. Nearest the centre first.
        assert!(l.tiles(0, 2.0, Rect::new(0.0, 0.0, 10.0, 10.0), Point::ZERO).is_empty());
        let bottom = Point::new(r.x1, r.y1);
        assert_eq!(l.tiles(0, 2.0, r, bottom)[0].at, (1024, 1024));
    }

    #[test]
    fn anchors_keep_the_place_across_zooms() {
        let pages = [LETTER; 5];
        let view = Size::new(800.0, 600.0);
        let l1 = Layout::new(&pages, 1.0, view.width);
        let scroll = (0.0, 2000.0);
        let a = Anchor::of(&l1, scroll, (400.0, 300.0));
        let l2 = Layout::new(&pages, 2.0, view.width);
        let s2 = a.scroll(&l2, view);
        let b = Anchor::of(&l2, s2, (400.0, 300.0));
        assert_eq!(a.page, b.page);
        assert!((a.at.0 - b.at.0).abs() < 1.0 && (a.at.1 - b.at.1).abs() < 1.0, "{a:?} {b:?}");
    }

    #[test]
    fn previews_keep_the_page_shape() {
        let l = Layout::new(&[LETTER, (792.0, 612.0)], 1.0, 800.0);
        assert_eq!(l.preview(0, 200).size, (200, 259));
        assert_eq!(l.preview(1, 200).size, (200, 155));
    }
}
