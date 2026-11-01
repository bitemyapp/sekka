//! Home: shelves of tiles, one above the other (keep playing, watch,
//! films and shows, games, and the grown-ups' door), and grids of tiles
//! (every game, a show's episodes).
//!
//! The focused tile is ringed in amber and a little larger; its name
//! shows under it when the tile is a picture. A shelf scrolls as the focus
//! nears its right edge, and every shelf remembers where it was.

use std::collections::HashMap;
use std::path::PathBuf;

use aizome_kit::layout::{Point, Rect, Size, Vec2};
use aizome_kit::sidestep_ui::kurbo::{BezPath, Circle, Line, RoundedRect};
use aizome_kit::sidestep_ui::{Canvas, Color, Image, ImageOptions, LineCap, StrokeStyle};
use aizome_kit::theme::Theme;

use super::look::*;
use super::{Fit, Focus, H, MARGIN, TOP, W};

/// A tile's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TileSize {
    /// 16:9, for services, games and where they were left.
    Wide,
    /// 2:3, for films' and shows' posters.
    Poster,
}

impl TileSize {
    pub fn size(self) -> Size {
        match self {
            TileSize::Wide => Size::new(352.0, 198.0),
            TileSize::Poster => Size::new(212.0, 318.0),
        }
    }
}

/// The pictures drawn for tiles without one of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Symbol {
    Grid,
    Controller,
    Lock,
    Gear,
    Door,
    Plus,
    Film,
}

/// What a tile shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TileLook {
    /// A service: its colour, its name set large.
    Brand {
        color: u32,
        ink: u32,
    },
    /// A picture (box art, a poster, where a game was left), on a card of
    /// `color`; without one the card wears the title.
    Picture {
        art: Option<PathBuf>,
        color: u32,
        pixels: bool,
    },
    Symbol {
        symbol: Symbol,
        color: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    pub label: String,
    /// Under the label: a system, an episode, "Grown-ups only".
    pub sub: String,
    pub look: TileLook,
    /// A corner flag ("New").
    pub badge: Option<String>,
    /// How far through, in thousandths.
    pub progress: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shelf {
    pub title: String,
    pub size: TileSize,
    pub tiles: Vec<Tile>,
    /// Where it's scrolled to: its first tile showing.
    pub start: usize,
}

/// Home's shelves.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Home {
    pub shelves: Vec<Shelf>,
    /// The first shelf showing.
    pub top: usize,
}

pub const GAP: f64 = 30.0;
/// From a shelf's title to its tiles.
const TITLE_H: f64 = 58.0;
/// Below the tiles: room for the focused tile's name.
const BELOW: f64 = 84.0;

/// Tiles of `size` that fit across.
pub fn across(size: TileSize) -> usize {
    let w = size.size().width;
    ((W - MARGIN * 2.0 + GAP) / (w + GAP)).floor().max(1.0) as usize
}

/// Where a shelf scrolls to keep tile `col` showing, the next one
/// peeking.
pub fn start_for(col: usize, len: usize, size: TileSize) -> usize {
    let n = across(size);
    if len <= n { 0 } else { col.saturating_sub(n.saturating_sub(2)).min(len - n + 1) }
}

/// The first shelf showing when the focus is on `row`: the one above it
/// stays in sight.
pub fn top_for(row: usize) -> usize {
    row.saturating_sub(1)
}

fn shelf_height(s: &Shelf) -> f64 {
    TITLE_H + s.size.size().height + BELOW
}

impl Home {
    /// Each shelf's top, scrolled.
    fn tops(&self) -> Vec<f64> {
        let mut y = TOP;
        let scroll: f64 = self.shelves.iter().take(self.top).map(shelf_height).sum();
        self.shelves
            .iter()
            .map(|s| {
                let t = y - scroll;
                y += shelf_height(s);
                t
            })
            .collect()
    }

    /// Tile `col` of shelf `row`'s rectangle.
    fn tile_rect(&self, tops: &[f64], row: usize, col: usize) -> Rect {
        let s = &self.shelves[row];
        let size = s.size.size();
        let x = MARGIN + (col as f64 - s.start as f64) * (size.width + GAP);
        Rect::from_origin_size(Point::new(x, tops[row] + TITLE_H), size)
    }

    fn showing(&self, tops: &[f64], row: usize, col: usize) -> bool {
        let r = self.tile_rect(tops, row, col);
        col >= self.shelves[row].start && r.x0 < W && r.y1 > TOP - 20.0 && r.y0 < H - 110.0
    }

    pub fn parts(&self, focus: Focus, fit: &Fit) -> Vec<(Rect, u64)> {
        let tops = self.tops();
        let mut out = Vec::new();
        for (row, s) in self.shelves.iter().enumerate() {
            for col in 0..s.tiles.len() {
                if !self.showing(&tops, row, col) {
                    continue;
                }
                let r = self.tile_rect(&tops, row, col);
                let focused = focus.row == row && focus.col == col;
                let area = reach(r).union(Rect::new(r.x0 - 10.0, r.y1, r.x1 + 320.0, r.y1 + BELOW));
                out.push((fit.to_window(area), u64::from(focused)));
            }
        }
        out
    }

    pub fn hit(&self, _focus: Focus, p: Point) -> Option<(usize, usize)> {
        let tops = self.tops();
        for (row, s) in self.shelves.iter().enumerate() {
            for col in 0..s.tiles.len() {
                if self.showing(&tops, row, col) && self.tile_rect(&tops, row, col).contains(p) {
                    return Some((row, col));
                }
            }
        }
        None
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus, art: &HashMap<PathBuf, Image>) {
        let tops = self.tops();
        // Shelves go under the header as they scroll: clipped there.
        c.saved(|c| {
            c.clip_rect(Rect::new(0.0, TOP - 30.0, W, H - 110.0));
            for (row, s) in self.shelves.iter().enumerate() {
                let y = tops[row];
                if y + shelf_height(s) < TOP - 30.0 || y > H {
                    continue;
                }
                let title = Rect::new(MARGIN, y, W, y + TITLE_H);
                if c.needs(title) {
                    text(
                        c,
                        theme,
                        &s.title,
                        Point::new(MARGIN, y),
                        font(36.0, 700.0),
                        if focus.row == row { INK } else { INK_SOFT },
                    );
                }
                // The focused tile last, over its neighbours.
                let mut order: Vec<usize> = (0..s.tiles.len()).filter(|col| self.showing(&tops, row, *col)).collect();
                order.sort_by_key(|col| focus.row == row && focus.col == *col);
                for col in order {
                    let r = self.tile_rect(&tops, row, col);
                    if !c.needs(reach(r).union(Rect::new(r.x0, r.y1, r.x1 + 320.0, r.y1 + BELOW))) {
                        continue;
                    }
                    let focused = focus.row == row && focus.col == col;
                    paint_tile(c, theme, &s.tiles[col], r, focused, art);
                }
            }
        });
    }
}

/// A grid of tiles under a title, with tabs (systems) when it has them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Grid {
    pub title: String,
    pub subtitle: String,
    pub tabs: Vec<String>,
    pub tab: usize,
    pub size: TileSize,
    pub tiles: Vec<Tile>,
    /// The first line of tiles showing.
    pub first_line: usize,
}

impl Grid {
    pub fn columns(&self) -> usize {
        across(self.size)
    }

    fn body_top(&self) -> f64 {
        TOP + if self.tabs.is_empty() { 10.0 } else { 74.0 }
    }

    fn line_height(&self) -> f64 {
        self.size.size().height + 86.0
    }

    /// The first line to show with the focus on `index`.
    pub fn first_line_for(&self, index: usize) -> usize {
        let line = index / self.columns().max(1);
        let lines_showing = (((H - 130.0 - self.body_top()) / self.line_height()).floor() as usize).max(1);
        line.saturating_sub(lines_showing.saturating_sub(1))
    }

    fn tile_rect(&self, i: usize) -> Rect {
        let cols = self.columns().max(1);
        let size = self.size.size();
        let line = i / cols;
        let x = MARGIN + (i % cols) as f64 * (size.width + GAP);
        let y = self.body_top() + (line as f64 - self.first_line as f64) * self.line_height();
        Rect::from_origin_size(Point::new(x, y), size)
    }

    fn showing(&self, i: usize) -> bool {
        let r = self.tile_rect(i);
        i / self.columns().max(1) >= self.first_line && r.y0 < H - 120.0
    }

    pub fn parts(&self, focus: Focus, fit: &Fit) -> Vec<(Rect, u64)> {
        (0..self.tiles.len())
            .filter(|i| self.showing(*i))
            .map(|i| {
                let r = self.tile_rect(i);
                let area = reach(r).union(Rect::new(r.x0 - 10.0, r.y1, r.x1 + 320.0, r.y1 + 80.0));
                (fit.to_window(area), u64::from(focus.col == i))
            })
            .collect()
    }

    pub fn hit(&self, _focus: Focus, p: Point) -> Option<usize> {
        (0..self.tiles.len()).find(|i| self.showing(*i) && self.tile_rect(*i).contains(p))
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus, art: &HashMap<PathBuf, Image>) {
        // The title where Home has its clock.
        let head = Rect::new(0.0, 40.0, W, self.body_top() - 10.0);
        if c.needs(head) {
            text(c, theme, &self.title, Point::new(MARGIN, 64.0), font(58.0, 800.0), INK);
            if !self.subtitle.is_empty() {
                let w = measure(theme, &self.title, font(58.0, 800.0)).width;
                text(c, theme, &self.subtitle, Point::new(MARGIN + w + 28.0, 84.0), font(32.0, 600.0), INK_SOFT);
            }
            let mut x = MARGIN;
            for (i, t) in self.tabs.iter().enumerate() {
                let spec = font(28.0, 700.0);
                let w = measure(theme, t, spec).width + 48.0;
                let r = Rect::new(x, TOP - 22.0, x + w, TOP + 32.0);
                let on = i == self.tab;
                c.fill(&RoundedRect::from_rect(r, 27.0), if on { INK } else { Color::hex(0x252c62) });
                text_centred(c, theme, t, r.center(), spec, if on { Color::hex(0x141a3a) } else { INK_SOFT });
                x += w + 14.0;
            }
        }
        c.saved(|c| {
            c.clip_rect(Rect::new(0.0, self.body_top() - 30.0, W, H - 110.0));
            let mut order: Vec<usize> = (0..self.tiles.len()).filter(|i| self.showing(*i)).collect();
            order.sort_by_key(|i| focus.col == *i);
            for i in order {
                let r = self.tile_rect(i);
                if c.needs(reach(r).union(Rect::new(r.x0, r.y1, r.x1 + 320.0, r.y1 + 80.0))) {
                    paint_tile(c, theme, &self.tiles[i], r, focus.col == i, art);
                }
            }
        });
        if self.tiles.is_empty() {
            text_centred(c, theme, "Nothing here yet", Point::new(W / 2.0, H / 2.0), font(40.0, 700.0), INK_SOFT);
        }
    }
}

fn rgb(c: u32) -> Color {
    Color::hex(c)
}

/// One tile at `r`.
pub fn paint_tile(c: &mut Canvas, theme: &Theme, t: &Tile, r: Rect, focused: bool, art: &HashMap<PathBuf, Image>) {
    let r = lifted(r, focused);
    let radius = 20.0;
    match &t.look {
        TileLook::Brand { color, ink } => {
            card(c, r, radius, rgb(*color));
            let size = fitted(theme, &t.label, r.width() - 48.0, 52.0, 900.0);
            text_centred(c, theme, &t.label, r.center(), font(size, 900.0), rgb(*ink));
        }
        TileLook::Picture { art: path, color, pixels } => {
            card(c, r, radius, rgb(*color));
            match path.as_ref().and_then(|p| art.get(p)) {
                Some(img) => c.saved(|c| {
                    c.clip(&RoundedRect::from_rect(r, radius));
                    let (iw, ih) = img.pixel_size();
                    // The whole picture, letterboxed on its card.
                    let s = (r.width() / f64::from(iw)).min(r.height() / f64::from(ih));
                    let size = Size::new(f64::from(iw) * s, f64::from(ih) * s);
                    let dst = Rect::from_center_size(r.center(), size);
                    c.draw_image_with(img, None, dst, ImageOptions { smooth: !pixels, ..ImageOptions::default() });
                }),
                None => {
                    // The title on the card, where there's no picture.
                    let lines = wrap(theme, &t.label, r.width() - 44.0, font(38.0, 800.0));
                    let spec = if lines.len() > 3 { font(28.0, 800.0) } else { font(38.0, 800.0) };
                    let lines = wrap(theme, &t.label, r.width() - 44.0, spec);
                    let lh = spec.size * 1.2;
                    let top = r.center().y - lh * lines.len().min(4) as f64 / 2.0;
                    for (i, l) in lines.iter().take(4).enumerate() {
                        text_centred(c, theme, l, Point::new(r.center().x, top + lh * (i as f64 + 0.5)), spec, INK);
                    }
                }
            }
        }
        TileLook::Symbol { symbol, color } => {
            card(c, r, radius, rgb(*color));
            let at = Point::new(r.center().x, r.y0 + r.height() * 0.42);
            draw_symbol(c, *symbol, at, r.height() * 0.34);
            let size = fitted(theme, &t.label, r.width() - 40.0, 32.0, 800.0);
            text_centred(
                c,
                theme,
                &t.label,
                Point::new(r.center().x, r.y1 - r.height() * 0.17),
                font(size, 800.0),
                INK,
            );
        }
    }
    if let Some(p) = t.progress {
        let bar = Rect::new(r.x0 + 18.0, r.y1 - 18.0, r.x1 - 18.0, r.y1 - 10.0);
        c.fill(&RoundedRect::from_rect(bar, 4.0), Color::rgba(0.0, 0.0, 0.0, 0.5));
        let done = Rect::new(bar.x0, bar.y0, bar.x0 + bar.width() * f64::from(p.min(1000)) / 1000.0, bar.y1);
        // Nothing to fill at the very start.
        if done.width() >= 1.0 {
            c.fill(&RoundedRect::from_rect(done, 4.0), FOCUS);
        }
    }
    if let Some(b) = &t.badge {
        let spec = font(22.0, 800.0);
        let w = measure(theme, b, spec).width + 26.0;
        let br = Rect::new(r.x1 - w - 10.0, r.y0 + 10.0, r.x1 - 10.0, r.y0 + 44.0);
        c.fill(&RoundedRect::from_rect(br, 17.0), FOCUS);
        text_centred(c, theme, b, br.center(), spec, Color::hex(0x2a1e05));
    }
    if focused {
        focus_ring(c, r, radius);
        // Its name on one line under it (a picture's; a service says its
        // own), and what it is after, softer.
        let named = !matches!(t.look, TileLook::Brand { .. } | TileLook::Symbol { .. });
        let label = if named { t.label.as_str() } else { "" };
        if !label.is_empty() || !t.sub.is_empty() {
            let y = r.y1 + 18.0;
            let room = r.width() + 300.0;
            let mut x = r.x0;
            if !label.is_empty() {
                let l = fit_text(theme, label, room, font(30.0, 700.0));
                text(c, theme, &l, Point::new(x, y), font(30.0, 700.0), INK);
                x += measure(theme, &l, font(30.0, 700.0)).width + 16.0;
            }
            if !t.sub.is_empty() && x < r.x0 + room {
                let s = fit_text(theme, &t.sub, r.x0 + room - x, font(26.0, 600.0));
                text(c, theme, &s, Point::new(x, y + 3.0), font(26.0, 600.0), INK_SOFT);
            }
        }
    }
}

/// A symbol, `h` tall, centred on `at`.
pub fn draw_symbol(c: &mut Canvas, s: Symbol, at: Point, h: f64) {
    let stroke = StrokeStyle::new(h * 0.1).with_cap(LineCap::Round);
    match s {
        Symbol::Grid => {
            let k = h * 0.22;
            for dx in [-1.0, 0.0, 1.0] {
                for dy in [-1.0, 0.0, 1.0] {
                    let p = at + Vec2::new(dx * k * 1.5, dy * k * 1.5);
                    c.fill(&RoundedRect::from_rect(Rect::from_center_size(p, Size::new(k, k)), k * 0.25), INK);
                }
            }
        }
        Symbol::Controller => controller(c, at, h * 1.5, INK, Color::rgba(1.0, 1.0, 1.0, 0.1)),
        Symbol::Lock => padlock(c, at, h, INK),
        Symbol::Gear => {
            c.stroke(&Circle::new(at, h * 0.3), &stroke, INK);
            for i in 0..8 {
                let a = f64::from(i) * std::f64::consts::TAU / 8.0;
                let v = Vec2::new(a.cos(), a.sin());
                c.stroke(&Line::new(at + v * (h * 0.36), at + v * (h * 0.5)), &stroke, INK);
            }
        }
        Symbol::Door => {
            let r = Rect::from_center_size(at, Size::new(h * 0.6, h));
            c.stroke(&RoundedRect::from_rect(r, h * 0.06), &stroke, INK);
            c.fill(&Circle::new(Point::new(r.x1 - h * 0.14, at.y), h * 0.06), INK);
        }
        Symbol::Plus => {
            c.stroke(&Line::new((at.x - h * 0.4, at.y), (at.x + h * 0.4, at.y)), &stroke, INK);
            c.stroke(&Line::new((at.x, at.y - h * 0.4), (at.x, at.y + h * 0.4)), &stroke, INK);
        }
        Symbol::Film => {
            let r = Rect::from_center_size(at, Size::new(h * 1.1, h * 0.8));
            c.stroke(&RoundedRect::from_rect(r, h * 0.08), &stroke, INK);
            let mut tri = BezPath::new();
            tri.move_to((at.x - h * 0.14, at.y - h * 0.2));
            tri.line_to((at.x + h * 0.22, at.y));
            tri.line_to((at.x - h * 0.14, at.y + h * 0.2));
            tri.close_path();
            c.fill(&tri, INK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_scroll_to_keep_the_focus_with_a_neighbour_showing() {
        let n = across(TileSize::Wide);
        assert_eq!(n, 4);
        assert_eq!(start_for(0, 10, TileSize::Wide), 0);
        assert_eq!(start_for(2, 10, TileSize::Wide), 0);
        assert_eq!(start_for(3, 10, TileSize::Wide), 1);
        assert_eq!(start_for(9, 10, TileSize::Wide), 7);
        assert_eq!(start_for(3, 4, TileSize::Wide), 0, "all showing");
        assert_eq!(top_for(0), 0);
        assert_eq!(top_for(3), 2);
    }
}
