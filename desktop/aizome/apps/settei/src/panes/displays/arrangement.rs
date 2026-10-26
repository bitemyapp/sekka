//! The arrangement: the displays that are on, drawn to scale where they are
//! in the layout, as in macOS's Displays settings, on a well with a faint
//! grid. Each is a dye-coloured, bevelled slab with a hard shadow, its name
//! in Archivo and its connector in mono; the main one carries a menu bar.
//! Dragging one moves it, showing where it will land (beside the others,
//! edges snapped, never overlapping) as a dashed outline; dropping it
//! reports the place. A click selects one; with the keyboard, the arrows
//! move the selected one to that side of the others.
//!
//! Ported from Tatami's Displays (`arrangement.rs`), drawn by hand rather
//! than with GTK widgets.

use aizome_kit::element::{Element, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Rect, Size};
use aizome_kit::sidestep_ui::kurbo::Line;
use aizome_kit::sidestep_ui::{Cursor, Key, NamedKey, PointerButton, StrokeStyle};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::{FontSpec, color};
use aizome_kit::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Align, Color, Layoutable};
use aizome_theme::Rgba;

use super::model::{self, Rect as LRect};

/// A display to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub name: String,
    /// Where it is in the layout, in points.
    pub rect: LRect,
    pub title: String,
    /// "1440 × 900".
    pub size: String,
    pub main: bool,
    /// Displays showing the same picture, by title.
    pub mirrors: Vec<String>,
    /// Which dye: 0 ai, 1 hanada, 2 kamenozoki, 3 violet, 4 cyan.
    pub dye: usize,
}

/// The dyes tiles take, in order.
pub const DYES: [u32; 5] = [0x3d59a1, 0x7aa2f7, 0xa9c4ff, 0x9d7cd8, 0x2ac3de];

/// The arrangement of `tiles`, `selected` one picked; `on_select` and
/// `on_move` (name, x, y in the layout) say what the pointer did.
pub fn arrangement<Msg: Send + 'static>(
    tiles: Vec<Tile>,
    selected: &str,
    on_select: impl Fn(String) -> Msg + 'static,
    on_move: impl Fn((String, i32, i32)) -> Msg + 'static,
    enabled: bool,
) -> Element<Msg> {
    let w = Arrangement {
        tiles,
        selected: selected.to_owned(),
        on_select: MsgMap::new(on_select),
        on_move: MsgMap::new(on_move),
        enabled,
        fit: Fit::default(),
    };
    Element::new(w, Vec::new()).width(aizome_kit::Length::Fill).label("Display arrangement")
}

/// The well's height.
const HEIGHT: f64 = 248.0;
/// Room around the displays, to drag one beside the others.
const PADDING: f64 = 28.0;

/// Layout to widget: widget = offset + (layout - origin) × factor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Fit {
    factor: f64,
    offset: (f64, f64),
    origin: (i32, i32),
}

impl Fit {
    fn of(tiles: &[Tile], size: Size) -> Fit {
        if tiles.is_empty() || size.width <= 0.0 {
            return Fit { factor: 0.1, offset: (size.width / 2.0, size.height / 2.0), origin: (0, 0) };
        }
        let left = tiles.iter().map(|t| t.rect.x).min().unwrap_or(0);
        let top = tiles.iter().map(|t| t.rect.y).min().unwrap_or(0);
        let right = tiles.iter().map(|t| t.rect.x + t.rect.w).max().unwrap_or(1);
        let bottom = tiles.iter().map(|t| t.rect.y + t.rect.h).max().unwrap_or(1);
        let (sx, sy) = ((right - left).max(1) as f64, (bottom - top).max(1) as f64);
        let factor = ((size.width - 2.0 * PADDING) / sx).min((size.height - 2.0 * PADDING) / sy).min(0.15) * 0.85;
        Fit {
            factor,
            origin: (left, top),
            offset: ((size.width - sx * factor) / 2.0, (size.height - sy * factor) / 2.0),
        }
    }

    fn to_widget(self, r: LRect) -> Rect {
        let x = self.offset.0 + (r.x - self.origin.0) as f64 * self.factor;
        let y = self.offset.1 + (r.y - self.origin.1) as f64 * self.factor;
        let w = (r.w as f64 * self.factor).max(24.0);
        let h = (r.h as f64 * self.factor).max(16.0);
        Rect::new(x.round(), y.round(), (x + w).round(), (y + h).round())
    }

    fn to_layout(self, p: Point) -> (i32, i32) {
        let f = self.factor.max(1e-6);
        (
            ((p.x - self.offset.0) / f).round() as i32 + self.origin.0,
            ((p.y - self.offset.1) / f).round() as i32 + self.origin.1,
        )
    }
}

struct Arrangement {
    tiles: Vec<Tile>,
    selected: String,
    on_select: MsgMap<String>,
    on_move: MsgMap<(String, i32, i32)>,
    enabled: bool,
    fit: Fit,
}

/// A drag under way.
#[derive(Clone, Debug, Default)]
struct Drag {
    name: String,
    /// The pointer's offset in the tile.
    grab: Point,
    /// The tile's top left now.
    at: Point,
    /// Where the press was.
    from: Point,
    moved: bool,
    /// The arrangement's fit when it started (kept while dragging).
    fit: Fit,
    /// What the last frame drew for the drag (the tile and its landing),
    /// to redraw only that and what replaces it.
    shown: Vec<Rect>,
}

#[derive(Default)]
struct State {
    drag: Option<Drag>,
}

impl Arrangement {
    fn fit_now(&self, state: Option<&State>) -> Fit {
        state.and_then(|s| s.drag.as_ref()).map(|d| d.fit).unwrap_or(self.fit)
    }

    fn hit(&self, fit: &Fit, p: Point) -> Option<&Tile> {
        // The last drawn is on top.
        self.tiles.iter().rev().find(|t| fit.to_widget(t.rect).contains(p))
    }

    /// Where a tile dragged to `at` (widget coordinates) would land.
    fn landing(&self, fit: &Fit, name: &str, at: Point) -> Option<LRect> {
        let tile = self.tiles.iter().find(|t| t.name == name)?;
        let (x, y) = fit.to_layout(at);
        let dropped = LRect { x, y, ..tile.rect };
        let others: Vec<LRect> = self.tiles.iter().filter(|t| t.name != name).map(|t| t.rect).collect();
        let tolerance = dropped.w.max(dropped.h) / 16;
        let (x, y) = model::snap(dropped, &others, tolerance);
        Some(LRect { x, y, ..dropped })
    }

    fn paint_tile(&self, cx: &mut PaintCx, tile: &Tile, r: Rect, selected: bool, ghost: bool) {
        let t = cx.theme();
        let dye = Rgba::hex(DYES[tile.dye % DYES.len()]);
        let fill = color(dye);
        let dark = Rgba::hex(0x16161e);
        let light = Rgba::hex(0xf4f5fa);
        let ink = if dye.contrast(light) >= dye.contrast(dark) { light } else { dark };
        let ink = color(ink);
        // The hard shadow, never blurred.
        if t.shadows() && !ghost {
            let s = t.metrics.shadow;
            cx.canvas.fill_rect(Rect::new(r.x0 + s, r.y1, r.x1 + s, r.y1 + s), t.slab());
            cx.canvas.fill_rect(Rect::new(r.x1, r.y0 + s, r.x1 + s, r.y1), t.slab());
        }
        let fill = if ghost { fill.with_alpha(0.55) } else { fill };
        cx.canvas.fill_rect(r, fill);
        // The bevel: light along the top and left, dark along the bottom
        // and right, then the edge.
        let hi = Color::rgba8(255, 255, 255, 70);
        let lo = Color::rgba8(0, 0, 0, 60);
        cx.canvas.fill_rect(Rect::new(r.x0 + 1.0, r.y0 + 1.0, r.x1 - 1.0, r.y0 + 2.0), hi);
        cx.canvas.fill_rect(Rect::new(r.x0 + 1.0, r.y0 + 1.0, r.x0 + 2.0, r.y1 - 1.0), hi);
        cx.canvas.fill_rect(Rect::new(r.x0 + 1.0, r.y1 - 2.0, r.x1 - 1.0, r.y1 - 1.0), lo);
        cx.canvas.fill_rect(Rect::new(r.x1 - 2.0, r.y0 + 1.0, r.x1 - 1.0, r.y1 - 1.0), lo);
        aizome_kit::draw::border(cx.canvas, r, t.bevel_lo(), 1.0);
        let mut top = r.y0 + 2.0;
        if tile.main {
            // The menu bar, with the Toolchest's amber tab at its left.
            let bar_h = (r.height() / 12.0).clamp(4.0, 9.0).round();
            let bar = Rect::new(r.x0 + 1.0, r.y0 + 1.0, r.x1 - 1.0, r.y0 + 1.0 + bar_h);
            cx.canvas.fill_rect(bar, t.bar());
            cx.canvas.fill_rect(Rect::new(bar.x0, bar.y1, bar.x1, bar.y1 + 1.0), t.bevel_lo());
            let tab = (bar_h - 2.0).max(2.0);
            cx.canvas.fill_rect(
                Rect::new(bar.x0 + 3.0, bar.y0 + 1.0, bar.x0 + 3.0 + tab * 1.6, bar.y0 + 1.0 + tab),
                color(Rgba::hex(0xe0af68)),
            );
            top = bar.y1 + 1.0;
        }
        // The labels, as many as fit: name, connector, size.
        let body = Rect::new(r.x0 + 6.0, top, r.x1 - 6.0, r.y1 - 3.0);
        // Narrow tiles (a display turned on its side) take smaller type.
        let narrow = body.width() < 110.0;
        let title = TextSpec::new(FontSpec { weight: 600.0, ..FontSpec::ui(if narrow { 10.5 } else { 12.0 }) }, ink);
        let mono = TextSpec::new(FontSpec::mono(if narrow { 9.0 } else { 10.0 }), ink.with_alpha(0.85));
        let mut lines: Vec<(&str, &TextSpec, f64)> = Vec::new();
        let room = body.height();
        if room >= 44.0 {
            lines.push((&tile.title, &title, 15.0));
            lines.push((&tile.name, &mono, 13.0));
            lines.push((&tile.size, &mono, 13.0));
        } else if room >= 30.0 {
            lines.push((&tile.title, &title, 15.0));
            lines.push((&tile.name, &mono, 13.0));
        } else if room >= 13.0 {
            lines.push((&tile.name, &mono, 13.0));
        }
        let total: f64 = lines.iter().map(|l| l.2).sum();
        let mut y = (body.y0 + (body.height() - total) / 2.0).round();
        for (s, spec, h) in lines {
            cx.draw_text_in(s, spec, Rect::new(body.x0, y, body.x1, y + h), Align::Center);
            y += h;
        }
        if selected && !ghost {
            // Inside its edge, so it never covers a neighbour.
            aizome_kit::draw::border(cx.canvas, r, t.focus(), 2.0);
        }
    }
}

impl Widget for Arrangement {
    fn name(&self) -> &'static str {
        "arrangement"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        let w = if bc.max.width.is_finite() { bc.max.width } else { 560.0 };
        let size = bc.constrain(Size::new(w, HEIGHT));
        self.fit = Fit::of(&self.tiles, size);
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        // The well and its grid.
        cx.canvas.fill_rect(b, t.sunk());
        let grid = t.line().with_alpha(0.35);
        let mut x = b.x0 + 12.0;
        while x < b.x1 {
            cx.canvas.fill_rect(Rect::new(x.round(), b.y0, x.round() + 1.0, b.y1), grid);
            x += 24.0;
        }
        let mut y = b.y0 + 12.0;
        while y < b.y1 {
            cx.canvas.fill_rect(Rect::new(b.x0, y.round(), b.x1, y.round() + 1.0), grid);
            y += 24.0;
        }
        aizome_kit::draw::border(cx.canvas, b, t.line(), 1.0);
        aizome_kit::draw::top(cx.canvas, b.inset(-1.0), t.bevel_lo().with_alpha(0.5), 1.0);

        let state = cx.state::<State>();
        let drag = state.and_then(|s| s.drag.clone());
        let fit = self.fit_now(state);
        // Mirrored displays stack behind their source.
        for tile in &self.tiles {
            for (i, _) in tile.mirrors.iter().enumerate() {
                let r = fit.to_widget(tile.rect);
                let off = 6.0 * (i + 1) as f64;
                let behind = Rect::new(r.x0 + off, r.y0 + off, r.x1 + off, r.y1 + off);
                cx.canvas.fill_rect(behind, t.chrome());
                aizome_kit::draw::border(cx.canvas, behind, t.line2(), 1.0);
            }
        }
        for tile in &self.tiles {
            if drag.as_ref().is_some_and(|d| d.moved && d.name == tile.name) {
                continue;
            }
            let r = fit.to_widget(tile.rect);
            self.paint_tile(cx, tile, r, tile.name == self.selected, false);
        }
        if let Some(d) = drag.filter(|d| d.moved)
            && let Some(tile) = self.tiles.iter().find(|t| t.name == d.name)
        {
            // Where it will land, dashed; then the tile under the pointer.
            if let Some(land) = self.landing(&fit, &d.name, d.at) {
                let r = fit.to_widget(land);
                let style = StrokeStyle::new(2.0).with_dash(vec![6.0, 4.0], 0.0);
                let c = t.focus();
                for (a, b2) in [
                    ((r.x0, r.y0), (r.x1, r.y0)),
                    ((r.x1, r.y0), (r.x1, r.y1)),
                    ((r.x1, r.y1), (r.x0, r.y1)),
                    ((r.x0, r.y1), (r.x0, r.y0)),
                ] {
                    cx.canvas.stroke(&Line::new(a, b2), &style, c);
                }
            }
            let base = fit.to_widget(tile.rect);
            let r = Rect::from_origin_size(d.at, base.size());
            self.paint_tile(cx, tile, r, true, true);
        }
        if cx.focus_visible() {
            aizome_kit::draw::border(cx.canvas, b, t.focus(), 2.0);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for tile in &self.tiles {
            h.str(&tile.name).str(&tile.title).str(&tile.size).add(&(
                tile.rect,
                tile.main,
                tile.dye,
                tile.mirrors.len(),
            ));
        }
        h.str(&self.selected).add(&self.enabled);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        if self.enabled { Focusable::Click } else { Focusable::No }
    }

    fn cursor(&self) -> Option<Cursor> {
        self.enabled.then_some(Cursor::Grab)
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if !self.enabled {
            return Status::Ignored;
        }
        let fit = self.fit;
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                let Some(tile) = self.hit(&fit, *pos).cloned() else { return Status::Handled };
                let r = fit.to_widget(tile.rect);
                cx.state::<State>().drag = Some(Drag {
                    name: tile.name.clone(),
                    grab: Point::new(pos.x - r.x0, pos.y - r.y0),
                    at: r.origin(),
                    from: *pos,
                    moved: false,
                    fit,
                    shown: Vec::new(),
                });
                cx.request_focus();
                cx.capture_pointer();
                cx.set_cursor(Cursor::Grabbing);
                if tile.name != self.selected {
                    let m = self.on_select.make(tile.name.clone());
                    emit(cx, m);
                }
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let size = cx.size();
                let Some(d) = cx.state::<State>().drag.as_mut() else { return Status::Ignored };
                if !d.moved && (pos.x - d.from.x).hypot(pos.y - d.from.y) < 4.0 {
                    return Status::Handled;
                }
                let home =
                    self.tiles.iter().find(|t| t.name == d.name).map(|t| d.fit.to_widget(t.rect)).unwrap_or_default();
                let mut old = std::mem::take(&mut d.shown);
                if !d.moved {
                    // The tile leaves its place.
                    old.push(home);
                }
                d.moved = true;
                let x = (pos.x - d.grab.x).clamp(0.0, (size.width - home.width()).max(0.0));
                let y = (pos.y - d.grab.y).clamp(0.0, (size.height - home.height()).max(0.0));
                d.at = Point::new(x.round(), y.round());
                let (name, at, fit) = (d.name.clone(), d.at, d.fit);
                let mut new = vec![Rect::from_origin_size(at, home.size())];
                if let Some(land) = self.landing(&fit, &name, at) {
                    new.push(fit.to_widget(land));
                }
                cx.state::<State>().drag.as_mut().expect("dragging").shown = new.clone();
                // Only what moved redraws: the old and new tile and outline,
                // with room for the shadow, the ring and the dashes.
                for r in old.into_iter().chain(new) {
                    cx.invalidate_rect(r.inflate(5.0, 5.0));
                }
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                let Some(d) = cx.state::<State>().drag.take() else { return Status::Ignored };
                cx.release_pointer();
                cx.set_cursor(Cursor::Grab);
                cx.invalidate();
                if d.moved {
                    let (x, y) = d.fit.to_layout(d.at);
                    let m = self.on_move.make((d.name, x, y));
                    emit(cx, m);
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => {
                let Some(tile) = self.tiles.iter().find(|t| t.name == self.selected) else { return Status::Ignored };
                // Half its size that way: snapping puts it on that side.
                let (dx, dy) = match &k.key {
                    Key::Named(NamedKey::ArrowLeft) => (-(tile.rect.w / 2 + tile.rect.w), 0),
                    Key::Named(NamedKey::ArrowRight) => (tile.rect.w / 2 + tile.rect.w, 0),
                    Key::Named(NamedKey::ArrowUp) => (0, -(tile.rect.h / 2 + tile.rect.h)),
                    Key::Named(NamedKey::ArrowDown) => (0, tile.rect.h / 2 + tile.rect.h),
                    _ => return Status::Ignored,
                };
                if self.tiles.len() < 2 {
                    return Status::Handled;
                }
                let m = self.on_move.make((tile.name.clone(), tile.rect.x + dx, tile.rect.y + dy));
                emit(cx, m);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    /// Each display's tile, by its connector, where it's drawn.
    fn parts(&self) -> Vec<(String, Rect)> {
        self.tiles.iter().map(|t| (t.name.clone(), self.fit.to_widget(t.rect))).collect()
    }

    fn semantics(&self) -> Option<Semantics> {
        let value = self
            .tiles
            .iter()
            .map(|t| format!("{} at {}, {}{}", t.title, t.rect.x, t.rect.y, if t.main { " (main)" } else { "" }))
            .collect::<Vec<_>>()
            .join("; ");
        Some(Semantics {
            role: SemanticRole::Canvas,
            label: Some("Display arrangement".into()),
            value: Some(value),
            actions: vec![SemanticAction::Focus],
            ..Default::default()
        })
    }
}

fn emit(cx: &mut EventCx, m: aizome_kit::element::AnyMsg) {
    let f = aizome_kit::element::MsgFn::from_fn({
        let cell = std::cell::RefCell::new(Some(m));
        move || cell.borrow_mut().take().unwrap_or_else(|| Box::new(()))
    });
    cx.emit(&f);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(name: &str, x: i32, w: i32) -> Tile {
        Tile {
            name: name.into(),
            rect: LRect::new(x, 0, w, 900),
            title: name.into(),
            size: String::new(),
            main: false,
            mirrors: vec![],
            dye: 0,
        }
    }

    #[test]
    fn the_fit_round_trips_and_leaves_room_to_drag() {
        let tiles = vec![tile("eDP-1", 0, 1440), tile("DP-1", 1440, 1920)];
        let fit = Fit::of(&tiles, Size::new(600.0, HEIGHT));
        let r = fit.to_widget(tiles[1].rect);
        let (x, y) = fit.to_layout(r.origin());
        assert!((x - 1440).abs() <= 8 && y.abs() <= 8, "{x},{y}");
        let left = fit.to_widget(tiles[0].rect);
        assert!(left.x0 >= PADDING && r.x1 <= 600.0 - PADDING);
    }
}
