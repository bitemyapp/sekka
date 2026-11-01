//! Home: shelves of tiles, one above the other (keep playing, watch,
//! games, films and shows, and the grown-ups' door), and grids of tiles
//! (every game, a show's episodes).
//!
//! Tiles are flat raised faces, as Aizome's are, headed by spaced
//! capitals. The tile the controller is on wears the focus ring; its name
//! shows under it when the tile is a picture. A shelf scrolls as the focus
//! nears its right edge, and every shelf remembers where it was.

use std::collections::HashMap;
use std::path::PathBuf;

use aizome_kit::Icon;
use aizome_kit::draw::border;
use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::sidestep_ui::{Canvas, Color, Image, ImageOptions};
use aizome_kit::theme::{Role, Theme};

use super::look::*;
use super::{Fit, Focus, H, HEAD_Y, MARGIN, TOP, W};

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

/// The icons on tiles that aren't a thing to play.
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

impl Symbol {
    fn icon(self) -> Icon {
        match self {
            Symbol::Grid => Icon::LayoutGrid,
            Symbol::Controller => Icon::Gamepad2,
            Symbol::Lock => Icon::Lock,
            Symbol::Gear => Icon::Settings,
            Symbol::Door => Icon::LogOut,
            Symbol::Plus => Icon::FolderPlus,
            Symbol::Film => Icon::Film,
        }
    }
}

/// What a tile shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TileLook {
    /// A service: its colour on a square with its initial, and its name.
    Brand {
        color: u32,
        ink: u32,
    },
    /// A picture (box art, a poster, where a game was left) on a mat;
    /// without one, the title, under the system's colour and name (`chip`)
    /// for a game.
    Picture {
        art: Option<PathBuf>,
        chip: Option<u32>,
        pixels: bool,
    },
    Symbol {
        symbol: Symbol,
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
/// From a shelf's caption to its tiles.
const TITLE_H: f64 = 52.0;
/// Below the tiles: room for the focused tile's name.
const BELOW: f64 = 80.0;

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
                    let ink = if focus.row == row { theme.text2() } else { theme.text3() };
                    caption(c, theme, &s.title, MARGIN, y + 14.0, ink);
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
        TOP + if self.tabs.is_empty() { 10.0 } else { 84.0 }
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
        // The title where Home has its name, the systems under it as tabs
        // (as the palette's scopes are).
        let head = Rect::new(0.0, 30.0, W, self.body_top() - 10.0);
        if c.needs(head) {
            let w = text_mid(c, theme, &self.title, MARGIN, HEAD_Y, font(HEADLINE - 4.0, BOLD), theme.text());
            if !self.subtitle.is_empty() {
                let spec = font(SMALL + 2.0, REGULAR);
                text_mid(c, theme, &self.subtitle, MARGIN + w + 24.0, HEAD_Y + 3.0, spec, theme.text3());
            }
            let mut x = MARGIN;
            for (i, t) in self.tabs.iter().enumerate() {
                let spec = font(SMALL + 2.0, MEDIUM);
                let w = (measure(theme, t, spec).width + 44.0).round();
                let r = Rect::new(x, TOP - 6.0, x + w, TOP + 46.0);
                let ink = if i == self.tab {
                    lit(c, theme, r);
                    theme.on_selection()
                } else {
                    face(c, theme, r, theme.raised(), theme.line());
                    theme.text2()
                };
                text_centred(c, theme, t, r.center(), spec, ink);
                x += w + 12.0;
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
            text_centred(
                c,
                theme,
                "Nothing here yet",
                Point::new(W / 2.0, H / 2.0),
                font(TITLE, MEDIUM),
                theme.text3(),
            );
        }
    }
}

fn rgb(c: u32) -> Color {
    Color::hex(c)
}

/// One tile at `r`: a flat face, its picture or its name, and while the
/// controller is on it the focus ring, with its name under it when the
/// tile is a picture.
pub fn paint_tile(c: &mut Canvas, theme: &Theme, t: &Tile, r: Rect, focused: bool, art: &HashMap<PathBuf, Image>) {
    const PAD: f64 = 24.0;
    match &t.look {
        TileLook::Brand { color, ink } => {
            face(c, theme, r, theme.raised(), theme.line());
            // The service's colour on a square with its initial, as an
            // account's picture is drawn; never its logo.
            let sq = Rect::new(r.x0 + PAD, r.y0 + PAD, r.x0 + PAD + 64.0, r.y0 + PAD + 64.0);
            c.fill_rect(sq, rgb(*color));
            border(c, sq, theme.bevel_lo().with_alpha(0.4), EDGE);
            let initial: String = t.label.chars().find(|c| c.is_alphanumeric()).into_iter().collect();
            text_centred(c, theme, &initial, sq.center(), font(36.0, BOLD), rgb(*ink));
            let size = fitted(theme, &t.label, r.width() - PAD * 2.0, TITLE, SEMIBOLD);
            let spec = font(size, SEMIBOLD);
            let h = measure(theme, &t.label, spec).height;
            text(c, theme, &t.label, Point::new(r.x0 + PAD, r.y1 - PAD - h), spec, theme.text());
        }
        TileLook::Picture { art: path, chip, pixels } => match path.as_ref().and_then(|p| art.get(p)) {
            Some(img) => {
                well(c, theme, r);
                c.saved(|c| {
                    let inner = r.inset(-EDGE);
                    c.clip_rect(inner);
                    let (iw, ih) = img.pixel_size();
                    // The whole picture, on its mat.
                    let s = (inner.width() / f64::from(iw)).min(inner.height() / f64::from(ih));
                    let size = Size::new(f64::from(iw) * s, f64::from(ih) * s);
                    let dst = Rect::from_center_size(inner.center(), size);
                    c.draw_image_with(img, None, dst, ImageOptions { smooth: !pixels, ..ImageOptions::default() });
                });
            }
            None => {
                face(c, theme, r, theme.raised(), theme.line());
                // What it is along the top (a game's system, by its
                // colour and name; a film's icon), its title at the foot.
                let y = r.y0 + PAD + 12.0;
                match chip {
                    Some(color) => {
                        c.fill_rect(Rect::new(r.x0 + PAD, y - 8.0, r.x0 + PAD + 16.0, y + 8.0), rgb(*color));
                        let room = r.width() - PAD * 2.0 - 28.0;
                        let sub = if caption_width(theme, &t.sub, CAPTION) <= room { t.sub.as_str() } else { "" };
                        caption(c, theme, sub, r.x0 + PAD + 28.0, y, theme.text3());
                    }
                    None => icon(c, Icon::Film, Point::new(r.x0 + PAD + 16.0, y + 4.0), 32.0, theme.text3()),
                }
                let spec = font(30.0, SEMIBOLD);
                let lines = wrap(theme, &t.label, r.width() - PAD * 2.0, spec);
                let room = ((r.height() - PAD * 2.0 - 40.0) / (spec.size * 1.2)).floor().max(1.0) as usize;
                let lh = spec.size * 1.2;
                let shown = lines.len().min(room);
                let mut top = r.y1 - PAD - lh * shown as f64;
                for (i, l) in lines.iter().take(shown).enumerate() {
                    let l = if i + 1 == shown && lines.len() > shown { format!("{l}…") } else { l.clone() };
                    let l = fit_text(theme, &l, r.width() - PAD * 2.0, spec);
                    text(c, theme, &l, Point::new(r.x0 + PAD, top), spec, theme.text());
                    top += lh;
                }
            }
        },
        TileLook::Symbol { symbol } => {
            face(c, theme, r, theme.raised(), theme.line());
            icon(c, symbol.icon(), Point::new(r.x0 + PAD + 24.0, r.y0 + PAD + 24.0), 48.0, theme.text2());
            let size = fitted(theme, &t.label, r.width() - PAD * 2.0, 30.0, SEMIBOLD);
            let spec = font(size, SEMIBOLD);
            let h = measure(theme, &t.label, spec).height;
            text(c, theme, &t.label, Point::new(r.x0 + PAD, r.y1 - PAD - h), spec, theme.text());
        }
    }
    if let Some(p) = t.progress {
        // How far through, along the foot, as Utsushi's timeline fills.
        let track = Rect::new(r.x0 + EDGE, r.y1 - EDGE - 8.0, r.x1 - EDGE, r.y1 - EDGE);
        c.fill_rect(track, theme.sunk());
        let done = Rect::new(track.x0, track.y0, track.x0 + track.width() * f64::from(p.min(1000)) / 1000.0, track.y1);
        // Nothing to fill at the very start.
        if done.width() >= 1.0 {
            c.fill_rect(done, theme.role(Role::Accent));
        }
    }
    if let Some(b) = &t.badge {
        chip(c, theme, b, r.x1 - 12.0, r.y0 + 28.0, 16.0);
    }
    if focused {
        ring(c, theme, r);
        // Under it, on one line, what the tile doesn't already say: a
        // picture's name, and what it is after, quieter (a game without a
        // picture has both on it).
        let pictured = matches!(&t.look, TileLook::Picture { art: Some(p), .. } if art.contains_key(p));
        let lettered = matches!(&t.look, TileLook::Picture { chip: Some(_), .. }) && !pictured;
        let label = if pictured { t.label.as_str() } else { "" };
        let sub = if lettered { "" } else { t.sub.as_str() };
        if !label.is_empty() || !sub.is_empty() {
            let y = r.y1 + RING + 30.0;
            let room = r.width() + 300.0;
            let mut x = r.x0;
            if !label.is_empty() {
                let spec = font(BODY, SEMIBOLD);
                let l = fit_text(theme, label, room, spec);
                x += text_mid(c, theme, &l, x, y, spec, theme.text()) + 16.0;
            }
            if !sub.is_empty() && x < r.x0 + room {
                let spec = mono(SMALL - 2.0, REGULAR);
                let s = fit_text(theme, sub, r.x0 + room - x, spec);
                text_mid(c, theme, &s, x, y + 1.0, spec, theme.text3());
            }
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
