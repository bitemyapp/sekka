//! The pages: a widget of Shiori's own that draws the pages in view, on
//! `sunk`, and turns the pointer, wheels, pinches and keys into messages.
//!
//! It scrolls as the kit's scroll views do: its offset and its drawings
//! are left out of its hash, and from its layout it says how far it
//! scrolled (`LayoutCx::scrolled`), so the kit moves what was drawn and
//! draws only the strip that came into view; and which drawings and
//! highlights came, went or changed since it was last laid out, so only
//! those are drawn again. A frame of scrolling costs a strip, not the
//! window.

use std::cell::Cell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use aizome_kit::element::MsgFn;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::Constraints;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{BlendMode, Canvas, Cursor, ImageOptions, Key, NamedKey, PointerButton, StrokeStyle};
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::widgets::layout::Axis;
use aizome_kit::{MsgSender, column};

use crate::app::{Doc, Drag, Hover, Load, Msg, Nav, Seen, Shiori};
use crate::layout::Layout;
use crate::protocol::TileKey;
use crate::text;

/// Tells the program what the pages drew in.
#[derive(Clone)]
pub struct Reporter {
    pub tx: MsgSender<Msg>,
    pub last: Rc<Cell<u64>>,
}

/// What shows of a tile: its drawing, what stands in while that comes
/// (`standin`: for `part` of it, what was in view), or paper.
#[derive(Clone)]
pub enum Drawing {
    Exact(Image),
    StandIn { part: TileKey, image: Image },
    Blank,
}

/// A page that shows: its tiles in view, each exactly as large as it shows.
#[derive(Clone)]
pub struct PageDraw {
    pub page: usize,
    pub tiles: Vec<(TileKey, Drawing)>,
    pub broken: bool,
}

/// What the pages draw.
#[derive(Clone)]
pub struct Pages {
    pub window: WindowKey,
    pub layout: Rc<Layout>,
    pub scroll: (f64, f64),
    pub assumed: (Size, f64),
    pub shown: Rc<Vec<PageDraw>>,
    /// Found text (document points), and whether it's the current hit.
    pub hits: Rc<Vec<(Rect, bool)>>,
    pub selection: Rc<Vec<Rect>>,
    /// Where links (each with its page, and its place among the page's)
    /// and lines of text are (document points), for the pointer's shape
    /// and what the toolbar says.
    pub links: Rc<Vec<(Rect, u32, usize)>>,
    pub lines: Rc<Vec<Rect>>,
    pub hover: Hover,
    pub dragging: bool,
    pub bar: bool,
    pub report: Option<Reporter>,
}

impl Pages {
    /// What decides the whole drawing (besides the view's size): the
    /// layout. Not the scroll offset, nor the drawings, which `parts`
    /// follows.
    fn whole_hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.layout.zoom.to_bits().hash(&mut h);
        self.layout.len().hash(&mut h);
        self.layout.size.width.to_bits().hash(&mut h);
        self.layout.size.height.to_bits().hash(&mut h);
        self.assumed.0.width.to_bits().hash(&mut h);
        self.assumed.0.height.to_bits().hash(&mut h);
        self.assumed.1.to_bits().hash(&mut h);
        h.finish()
    }

    /// Each thing drawn over the pages that can come and go by itself, in
    /// document points, and what it is.
    fn parts(&self) -> Vec<(Rect, u64)> {
        let id = |what: &dyn Fn(&mut DefaultHasher)| {
            let mut h = DefaultHasher::new();
            what(&mut h);
            h.finish()
        };
        let rect_bits = |r: &Rect, h: &mut DefaultHasher| {
            for v in [r.x0, r.y0, r.x1, r.y1] {
                v.to_bits().hash(h);
            }
        };
        let mut out = Vec::new();
        for p in self.shown.iter() {
            let r = self.layout.page_rect(p.page);
            for (k, shown) in &p.tiles {
                let what = match shown {
                    Drawing::Exact(_) => 0u8,
                    Drawing::StandIn { .. } => 4,
                    Drawing::Blank => 5,
                };
                let part = match shown {
                    Drawing::StandIn { part, .. } => Some(*part),
                    _ => None,
                };
                out.push((tile_rect(r, k), id(&|h| (what, k, part).hash(h))));
            }
            if p.broken {
                out.push((r, id(&|h| (1u8, p.page).hash(h))));
            }
        }
        for (r, cur) in self.hits.iter() {
            out.push((
                r.inflate(4.0, 4.0),
                id(&|h| {
                    2u8.hash(h);
                    rect_bits(r, h);
                    cur.hash(h);
                }),
            ));
        }
        for r in self.selection.iter() {
            out.push((
                *r,
                id(&|h| {
                    3u8.hash(h);
                    rect_bits(r, h);
                }),
            ));
        }
        out
    }

    /// The scroll bars' thumbs (view points), when they show: the vertical
    /// one, and the horizontal one when the pages are wider than the view.
    fn bar_rects(&self, size: Size) -> [Option<Rect>; 2] {
        if !self.bar {
            return [None, None];
        }
        let (content, view) = (self.layout.size, size);
        let tall = content.height > view.height + 0.5;
        let wide = content.width > view.width + 0.5;
        let vertical = tall.then(|| {
            let track = Rect::new(size.width - 6.0, 2.0, size.width - 2.0, size.height - 2.0);
            let len = (track.height() * view.height / content.height).clamp(24.0, track.height());
            let t = (self.scroll.1 / (content.height - view.height)).clamp(0.0, 1.0);
            let y = track.y0 + (track.height() - len) * t;
            Rect::new(track.x0, y.round(), track.x1, (y + len).round())
        });
        let horizontal = wide.then(|| {
            let end = size.width - if tall { 8.0 } else { 2.0 };
            let track = Rect::new(2.0, size.height - 6.0, end, size.height - 2.0);
            let len = (track.width() * view.width / content.width).clamp(24.0, track.width());
            let t = (self.scroll.0 / (content.width - view.width)).clamp(0.0, 1.0);
            let x = track.x0 + (track.width() - len) * t;
            Rect::new(x.round(), track.y0, (x + len).round(), track.y1)
        });
        [vertical, horizontal]
    }
}

/// What the found text and the current hit are drawn in, multiplied over
/// the page: colours for paper, which pages are whatever the theme, so the
/// current hit is the stronger in Night and Day alike.
const HIT: Color = Color::rgb8(0xff, 0xe8, 0x80);
const CURRENT_HIT: Color = Color::rgb8(0xff, 0xa8, 0x3c);
const CURRENT_EDGE: Color = Color::rgb8(0xd9, 0x6a, 0x00);

/// Where tile `k` of a page at `r` (document points) is.
fn tile_rect(r: Rect, k: &TileKey) -> Rect {
    let f = r.width() / f64::from(k.size.0.max(1));
    Rect::new(
        r.x0 + f64::from(k.at.0) * f,
        r.y0 + f64::from(k.at.1) * f,
        r.x0 + f64::from(k.at.0 + k.w) * f,
        r.y0 + f64::from(k.at.1 + k.h) * f,
    )
}

pub fn pages_of(app: &Shiori, d: &Doc) -> Pages {
    let layout = d.layout.clone();
    let s = d.scale;
    let view = Rect::new(d.scroll.0, d.scroll.1, d.scroll.0 + d.view.width, d.scroll.1 + d.view.height);
    let range = layout.visible(view.y0, view.y1);
    let mut shown = Vec::new();
    let mut links = Vec::new();
    let mut lines = Vec::new();
    let mut selection = Vec::new();
    let mut hits = Vec::new();
    for i in range {
        let p = i as u32;
        let tiles = layout
            .tiles(i, s, view, view.center())
            .into_iter()
            .map(|k| {
                let shown = match (d.tiles.get(&k), d.standins.get(&k)) {
                    (Some(img), _) => Drawing::Exact(img.clone()),
                    (None, Some((part, image))) => Drawing::StandIn { part: *part, image: image.clone() },
                    (None, None) => Drawing::Blank,
                };
                (k, shown)
            })
            .collect();
        shown.push(PageDraw { page: i, tiles, broken: d.broken_at(p, layout.device_size(i, s)) });
        if let Some(ls) = d.links.get(&p) {
            links.extend(ls.iter().enumerate().map(|(n, l)| (layout.rect_to_doc(i, &l.rect), p, n)));
        }
        if let Some(t) = d.texts.get(&p) {
            lines.extend(text::rects(t, 0, t.chars.len()).iter().map(|r| layout.rect_to_doc(i, r)));
            if let Some((a, b)) = d.selection.and_then(|sel| sel.on_page(p, t.chars.len())) {
                selection.extend(text::rects(t, a, b).iter().map(|r| layout.rect_to_doc(i, r)));
            }
        }
        if let Some(f) = &d.find
            && let Some(page_hits) = f.results.get(&p)
        {
            for (n, h) in page_hits.iter().enumerate() {
                let cur = f.current == Some((p, n));
                hits.extend(h.iter().map(|r| (layout.rect_to_doc(i, r), cur)));
            }
        }
    }
    Pages {
        window: d.window,
        layout,
        scroll: d.scroll,
        assumed: (d.view, d.scale),
        shown: Rc::new(shown),
        hits: Rc::new(hits),
        selection: Rc::new(selection),
        links: Rc::new(links),
        lines: Rc::new(lines),
        hover: d.hover,
        dragging: d.drag.is_some(),
        bar: d.bar_until > app.ticks || matches!(d.drag, Some(Drag::Bar(_))),
        report: app.stream.clone().map(|tx| Reporter { tx, last: d.reported.clone() }),
    }
}

fn middle(content: Element<Msg>) -> Element<Msg> {
    container(content).center().width(Fill).height(Fill).into()
}

/// The pages, and over them what's wrong if something is.
pub fn pages_area(app: &Shiori, d: &Doc) -> Element<Msg> {
    let st = pages_of(app, d);
    let cursor = match (d.drag, d.hover) {
        (Some(Drag::Pan { .. }), _) => Cursor::Grabbing,
        (_, Hover::Link { .. }) => Cursor::Pointer,
        (_, Hover::Text) => Cursor::Text,
        (_, Hover::None) => Cursor::Default,
    };
    let whole = st.whole_hash();
    let pages = Element::new(PagesWidget { st, whole }, Vec::new()).width(Fill).height(Fill).id("pages").label("Pages");
    let mut layers: Vec<Element<Msg>> = vec![pages];
    match &d.load {
        Load::Opening => layers.push(middle(text("Opening…").role(Role::Text3).into())),
        Load::Failed(why) => layers.push(middle(
            column![
                icon(Icon::CircleAlert).size(40.0).role(Role::Faint),
                text(format!("Shiori can't open “{}”", d.name())).size(15.0).semibold().role(Role::Text2),
                text(why.clone()).role(Role::Text3).wrap().center().max_width(420.0),
            ]
            .spacing(8.0)
            .align(Align::Center)
            .into(),
        )),
        Load::Locked { .. } => layers.push(middle(
            column![
                icon(Icon::Lock).size(40.0).role(Role::Faint),
                text(format!("“{}” is locked", d.name())).size(15.0).semibold().role(Role::Text2),
                button("Enter Password…").on_press(Msg::AskPassword(d.window)),
            ]
            .spacing(8.0)
            .align(Align::Center)
            .into(),
        )),
        Load::Open(_) => {}
    }
    mouse_area(aizome_kit::widgets::layout::stack(layers).width(Fill).height(Fill))
        .on_drop(|dr| Msg::Open(dr.urls.iter().filter_map(|u| crate::ui::file_path(u)).collect()))
        .cursor(cursor)
        .key("pages-area")
        .into()
}

/// What the pages were last laid out with.
#[derive(Default)]
struct Laid {
    size: Option<Size>,
    whole: u64,
    scroll: (f64, f64),
    parts: Vec<(Rect, u64)>,
    bar: [Option<Rect>; 2],
}

struct PagesWidget {
    st: Pages,
    whole: u64,
}

impl Widget for PagesWidget {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 400.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 300.0 },
        ));
        let parts = self.st.parts();
        let bar = self.st.bar_rects(size);
        let scroll = self.st.scroll;
        let laid = cx.state::<Laid>();
        let same = laid.size == Some(size) && laid.whole == self.whole;
        let before = std::mem::replace(&mut laid.parts, parts.clone());
        let old_scroll = std::mem::replace(&mut laid.scroll, scroll);
        let old_bar = std::mem::replace(&mut laid.bar, bar);
        laid.size = Some(size);
        laid.whole = self.whole;
        if !same {
            // The hash differs: it's all drawn again.
            return size;
        }
        let view = |r: &Rect| Rect::new(r.x0 - scroll.0, r.y0 - scroll.1, r.x1 - scroll.0, r.y1 - scroll.1);
        // The bars' tracks, where they can show, are drawn again rather
        // than moved.
        let content = self.st.layout.size;
        let tracks = [
            (content.height > size.height + 0.5).then(|| Rect::new(size.width - 10.0, 0.0, size.width, size.height)),
            (content.width > size.width + 0.5).then(|| Rect::new(0.0, size.height - 10.0, size.width, size.height)),
        ];
        if old_scroll != scroll {
            let delta = Vec2::new(old_scroll.0 - scroll.0, old_scroll.1 - scroll.1);
            let fixed: Vec<Rect> = tracks.iter().flatten().copied().collect();
            cx.scrolled(Rect::from_origin_size(Point::ZERO, size), delta, &fixed);
        } else {
            for (i, track) in tracks.iter().enumerate() {
                if let Some(t) = track
                    && old_bar[i] != bar[i]
                {
                    cx.invalidate_rect(*t);
                }
            }
        }
        let gone = before.iter().filter(|p| !parts.contains(p));
        let came = parts.iter().filter(|p| !before.contains(p));
        for (r, _) in gone.chain(came) {
            let v = view(r).intersect(Rect::from_origin_size(Point::ZERO, size));
            if v.area() > 0.0 {
                cx.invalidate_rect(v);
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let theme = cx.theme();
        cx.canvas.save();
        cx.canvas.clip_rect(b);
        paint_pages(cx.canvas, b, &self.st, theme);
        cx.canvas.restore();
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.whole);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        Focusable::Click
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match pages_event(event, cx.size(), &self.st) {
            Some(m) => {
                cx.emit(&MsgFn::new(m));
                Status::Handled
            }
            // Presses are its own even when they mean nothing yet.
            None if matches!(event, Event::PointerDown { .. }) => Status::Handled,
            None => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Canvas, label: Some("Pages".into()), ..Default::default() })
    }
}

/// Round a window: Escape that nothing in it took (the find field takes
/// the first, to empty itself) is `Msg::Escape`, when there's something
/// for it to do.
pub fn escapes(child: Element<Msg>, w: WindowKey, on: bool) -> Element<Msg> {
    Element::new(Escapes { w, on }, vec![child]).width(Fill).height(Fill)
}

struct Escapes {
    w: WindowKey,
    on: bool,
}

impl Widget for Escapes {
    fn name(&self) -> &'static str {
        "shiori-escape"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let mut size = Size::ZERO;
        for i in 0..cx.child_count() {
            let s = cx.layout_child(i, bc);
            cx.place_child(i, Point::ZERO);
            size = Size::new(size.width.max(s.width), size.height.max(s.height));
        }
        bc.constrain(size)
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        if let Event::Key(k) = event
            && k.pressed
            && self.on
            && k.key == Key::Named(NamedKey::Escape)
            && !(k.modifiers.super_key() || k.modifiers.control() || k.modifiers.alt() || k.modifiers.shift())
        {
            cx.emit(&MsgFn::new(Msg::Escape(self.w)));
            return Status::Handled;
        }
        Status::Ignored
    }
}

/// Which scroll bar's track `pos` (view points) is on: the vertical one
/// along the right, the horizontal one along the bottom.
fn on_bar(st: &Pages, size: Size, pos: Point) -> Option<Axis> {
    if st.layout.size.height > size.height + 0.5 && pos.x >= size.width - 12.0 {
        Some(Axis::Vertical)
    } else if st.layout.size.width > size.width + 0.5 && pos.y >= size.height - 12.0 {
        Some(Axis::Horizontal)
    } else {
        None
    }
}

fn hover_at(st: &Pages, pos: Point) -> Hover {
    let p = Point::new(st.scroll.0 + pos.x, st.scroll.1 + pos.y);
    if let Some((_, page, index)) = st.links.iter().find(|(r, _, _)| r.contains(p)) {
        Hover::Link { page: *page, index: *index }
    } else if st.lines.iter().any(|r| r.inflate(2.0, 2.0).contains(p)) {
        Hover::Text
    } else {
        Hover::None
    }
}

fn pages_event(ev: &Event, size: Size, st: &Pages) -> Option<Msg> {
    let w = st.window;
    match ev {
        Event::PointerDown { pos, button: PointerButton::Left, clicks, mods } => match on_bar(st, size, *pos) {
            Some(Axis::Vertical) => Some(Msg::BarPress(w, *pos)),
            Some(Axis::Horizontal) => Some(Msg::HBarPress(w, *pos)),
            None => Some(Msg::Press { window: w, at: *pos, clicks: *clicks, shift: mods.shift() }),
        },
        // The middle button takes hold of the pages, for moving about
        // them zoomed in without a sideways wheel.
        Event::PointerDown { pos, button: PointerButton::Middle, .. } if !st.dragging => Some(Msg::PanStart(w, *pos)),
        Event::PointerMove { pos, .. } if st.dragging => Some(Msg::DragTo(w, *pos)),
        Event::PointerMove { pos, .. } => {
            let h = if on_bar(st, size, *pos).is_some() { Hover::None } else { hover_at(st, *pos) };
            (h != st.hover).then_some(Msg::Hover(w, h))
        }
        Event::PointerLeave if st.hover != Hover::None && !st.dragging => Some(Msg::Hover(w, Hover::None)),
        Event::PointerUp { button: PointerButton::Left | PointerButton::Middle, .. } if st.dragging => {
            Some(Msg::Release(w))
        }
        Event::Scroll { pos, delta, mods, .. } => {
            if mods.super_key() || mods.control() {
                // A wheel's notch zooms by a tenth.
                Some(Msg::ZoomBy { window: w, factor: 1.1f64.powf(-delta.y / aizome_kit::LINE), at: *pos })
            } else if mods.shift() && delta.x == 0.0 {
                Some(Msg::ScrollBy(w, Vec2::new(delta.y, 0.0)))
            } else {
                Some(Msg::ScrollBy(w, *delta))
            }
        }
        Event::Pinch { pos, scale, .. } => Some(Msg::ZoomBy { window: w, factor: *scale, at: *pos }),
        Event::Key(k) if k.pressed => {
            let m = k.modifiers;
            match &k.key {
                Key::Character(s) if m.super_key() && s == "=" => Some(Msg::ZoomIn),
                // The menus take keys with ❖ or ⌃.
                _ if m.super_key() || m.control() || m.alt() => None,
                Key::Named(NamedKey::ArrowDown) => Some(Msg::Nav(w, Nav::Down)),
                Key::Named(NamedKey::ArrowUp) => Some(Msg::Nav(w, Nav::Up)),
                Key::Named(NamedKey::ArrowLeft) => Some(Msg::Nav(w, Nav::Left)),
                Key::Named(NamedKey::ArrowRight) => Some(Msg::Nav(w, Nav::Right)),
                Key::Named(NamedKey::PageDown) => Some(Msg::Nav(w, Nav::PageDown)),
                Key::Named(NamedKey::PageUp) => Some(Msg::Nav(w, Nav::PageUp)),
                Key::Named(NamedKey::Home) => Some(Msg::Nav(w, Nav::Home)),
                Key::Named(NamedKey::End) => Some(Msg::Nav(w, Nav::End)),
                Key::Character(s) if s == " " => Some(Msg::Nav(w, if m.shift() { Nav::PageUp } else { Nav::PageDown })),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Draw what the damage needs of the pages: each on its shadow, its tiles
/// copied one to one, drawn or standing in; then the found text and the
/// selection; then the scroll bar. Only the gutters are filled with
/// `sunk`, and only the edges drawn under a page: its drawings are
/// opaque, so each pixel is written about once.
pub fn paint_pages(c: &mut Canvas, bounds: Rect, st: &Pages, theme: &Theme) {
    let s = c.scale();
    // Whole device pixels, from the window's corner.
    let px = |v: f64| (v * s).round() / s;
    let to_view = |r: Rect| {
        Rect::new(
            px(bounds.x0 + r.x0 - st.scroll.0),
            px(bounds.y0 + r.y0 - st.scroll.1),
            px(bounds.x0 + r.x1 - st.scroll.0),
            px(bounds.y0 + r.y1 - st.scroll.1),
        )
    };
    let fill = |c: &mut Canvas, r: Rect, col: Color| {
        if r.width() > 0.0 && r.height() > 0.0 && c.needs(r) {
            c.fill_rect(r, col);
        }
    };
    // Each page from a corner on a whole device pixel, exactly as large as
    // its drawings at this zoom, so they're copied one to one.
    let pages: Vec<Rect> = st
        .shown
        .iter()
        .map(|p| {
            let o = to_view(st.layout.page_rect(p.page)).origin();
            let (w, h) = st.layout.device_size(p.page, s);
            Rect::from_origin_size(o, Size::new(f64::from(w) / s, f64::from(h) / s))
        })
        .collect();
    // The ground: around and between the pages.
    let mut y = bounds.y0;
    for v in &pages {
        fill(c, Rect::new(bounds.x0, y, bounds.x1, v.y0.max(y)), theme.sunk());
        fill(c, Rect::new(bounds.x0, v.y0, v.x0, v.y1), theme.sunk());
        fill(c, Rect::new(v.x1, v.y0, bounds.x1, v.y1), theme.sunk());
        y = y.max(v.y1);
    }
    fill(c, Rect::new(bounds.x0, y, bounds.x1, bounds.y1), theme.sunk());
    for (p, v) in st.shown.iter().zip(&pages) {
        if !c.needs(v.inflate(4.0, 4.0)) {
            continue;
        }
        // The page edged, on a hard shadow down and right.
        let (line, shadow) = (theme.line(), theme.bevel_lo());
        fill(c, Rect::new(v.x0 - 1.0, v.y0 - 1.0, v.x1 + 1.0, v.y0), line);
        fill(c, Rect::new(v.x0 - 1.0, v.y1, v.x1 + 1.0, v.y1 + 1.0), line);
        fill(c, Rect::new(v.x0 - 1.0, v.y0, v.x0, v.y1), line);
        fill(c, Rect::new(v.x1, v.y0, v.x1 + 1.0, v.y1), line);
        fill(c, Rect::new(v.x1 + 1.0, v.y0 + 3.0, v.x1 + 3.0, v.y1 + 3.0), shadow);
        fill(c, Rect::new(v.x0 + 3.0, v.y1 + 1.0, v.x1 + 1.0, v.y1 + 3.0), shadow);
        // Each tile one to one, opaque, in place of what's under it: its
        // drawing, what stands in for it, or paper.
        let place = |k: &TileKey| {
            Rect::from_origin_size(
                Point::new(v.x0 + f64::from(k.at.0) / s, v.y0 + f64::from(k.at.1) / s),
                Size::new(f64::from(k.w) / s, f64::from(k.h) / s),
            )
        };
        let copy = |c: &mut Canvas, img: &Image, dst: Rect| {
            c.save();
            c.set_blend_mode(BlendMode::Copy);
            c.draw_image_with(img, None, dst, ImageOptions { smooth: false, ..ImageOptions::default() });
            c.restore();
        };
        for (k, shown) in &p.tiles {
            let dst = place(k);
            if !c.needs(dst) {
                continue;
            }
            match shown {
                Drawing::Exact(img) => copy(c, img, dst),
                Drawing::StandIn { part, image } => {
                    // Paper where the part made is short of what shows.
                    let made = place(part);
                    let shows = dst.intersect(bounds);
                    if made.intersect(shows) != shows {
                        fill(c, dst, Color::WHITE);
                    }
                    copy(c, image, made);
                }
                Drawing::Blank => fill(c, dst, Color::WHITE),
            }
        }
        if p.broken {
            let at = Point::new(v.center().x - 12.0, v.center().y - 12.0);
            Icon::CircleAlert.draw(c, at, 24.0, theme.text3());
            let style = theme.style(FontSpec::ui(13.0), theme.text3());
            let label = "This page couldn't be drawn.";
            let width = aizome_kit::sidestep_ui::measure(label, &style).width;
            c.draw_label(label, &style, Point::new(v.center().x - width / 2.0, v.center().y + 18.0));
        }
    }
    // Found text in a highlighter's yellow, the current hit in orange (see
    // `HIT`), the selection in the selection's colour: multiplied over the
    // page, so the text stays legible.
    c.set_blend_mode(BlendMode::Multiply);
    for (r, cur) in st.hits.iter() {
        fill(c, to_view(r.inflate(1.0, 1.0)), if *cur { CURRENT_HIT } else { HIT });
    }
    let sel = theme.selection().with_alpha(0.45);
    for r in st.selection.iter() {
        fill(c, to_view(*r), sel);
    }
    c.set_blend_mode(BlendMode::Normal);
    // The current hit is edged too, to find it among the others.
    let edge = StrokeStyle::new(1.5);
    for (r, _) in st.hits.iter().filter(|(_, cur)| *cur) {
        let v = to_view(r.inflate(2.0, 2.0));
        if c.needs(v.inflate(1.0, 1.0)) {
            c.stroke(&v, &edge, CURRENT_EDGE);
        }
    }
    // The scroll bars, while scrolling.
    for thumb in st.bar_rects(bounds.size()).into_iter().flatten() {
        let t = Rect::new(bounds.x0 + thumb.x0, bounds.y0 + thumb.y0, bounds.x0 + thumb.x1, bounds.y0 + thumb.y1);
        fill(c, t, theme.line2().mix(theme.text3(), 0.35));
    }
    if let Some(r) = &st.report {
        let seen = Seen { view: bounds.size(), scale: s };
        if (seen.view, seen.scale) != st.assumed {
            let mut h = DefaultHasher::new();
            (seen.view.width.to_bits(), seen.view.height.to_bits(), s.to_bits()).hash(&mut h);
            let key = h.finish();
            if r.last.get() != key {
                r.last.set(key);
                r.tx.send(Msg::Seen(st.window, seen));
            }
        }
    }
}
