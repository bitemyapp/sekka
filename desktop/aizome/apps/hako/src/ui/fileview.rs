//! The file views: a Miller column, a list with details, and a grid of
//! icons. One widget draws all three, building nothing per row: it
//! paints only the rows that show (and of those, only what's damaged),
//! scrolls itself, and keeps the keyboard's cursor, type-select and
//! gestures (presses, drags of the selection, rubber bands) in its state.
//!
//! Selection belongs to the program: the view shows `selected` and
//! reports what the person picks. Drags start here and look up targets in
//! the window's [`Spots`]; drops from other programs land here too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use aizome_kit::element::{Common, Element, MsgFn};
use aizome_kit::hash::PaintHasher;
use aizome_kit::sidestep_ui::{Cursor, Key, NamedKey, PointerButton};
use aizome_kit::text::TextSpec;
use aizome_kit::theme::FontSpec;
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Align, Constraints, Icon, Image, Layoutable, Point, Rect, Size, Vec2};

use super::dnd::{self, Dest, DragEvent, Spot, Spots, Target};
use super::look;
use crate::model::entry::{Entry, SortKey};
use crate::model::format;

/// How a view lays its items out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// A Miller column: names, a chevron on folders.
    #[default]
    Column,
    /// Rows with details under sortable headings.
    List,
    /// A grid of large icons.
    Icons,
}

/// A detail a list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detail {
    Name,
    Size,
    Kind,
    Modified,
    /// The folder it's in (search results) or came from (the Trash).
    Where,
    /// When it went to the Trash.
    Trashed,
}

impl Detail {
    fn title(self) -> &'static str {
        match self {
            Detail::Name => "Name",
            Detail::Size => "Size",
            Detail::Kind => "Kind",
            Detail::Modified => "Modified",
            Detail::Where => "Where",
            Detail::Trashed => "Date Trashed",
        }
    }

    fn width(self) -> f64 {
        match self {
            Detail::Name => 0.0,
            Detail::Size => 84.0,
            Detail::Kind => 150.0,
            Detail::Modified | Detail::Trashed => 156.0,
            Detail::Where => 200.0,
        }
    }

    fn sort_key(self) -> Option<SortKey> {
        match self {
            Detail::Name => Some(SortKey::Name),
            Detail::Size => Some(SortKey::Size),
            Detail::Kind => Some(SortKey::Kind),
            Detail::Modified => Some(SortKey::Modified),
            _ => None,
        }
    }
}

/// Keys a view hands to the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKey {
    /// → in a column: into the folder selected.
    Into,
    /// ← in a column: back to the column before.
    Out,
    /// Space.
    QuickLook,
    /// F2.
    Rename,
    /// Escape.
    Escape,
    /// A click on the view away from the name being edited.
    ClickAway,
}

/// The messages a view makes.
pub struct Handlers<Msg> {
    pub select: Box<dyn Fn(Vec<usize>) -> Msg>,
    pub activate: Box<dyn Fn(usize) -> Msg>,
    pub key: Box<dyn Fn(ViewKey) -> Msg>,
    /// A right click: on a row (it's selected first) or not, where (window
    /// points).
    pub menu: Box<dyn Fn(Option<usize>, Point) -> Msg>,
    pub drag: Box<dyn Fn(DragEvent) -> Msg>,
    /// Files dropped from another program: where they go, their URIs.
    pub drop: Box<dyn Fn(Dest, Vec<String>) -> Msg>,
    pub sort: Box<dyn Fn(SortKey) -> Msg>,
}

/// Thumbnails by path, and a count of changes to them.
#[derive(Default)]
pub struct Thumbs {
    pub images: HashMap<PathBuf, Image>,
    pub generation: u64,
}

/// A view of `rows`.
pub struct FileView<Msg> {
    common: Common,
    layout: Layout,
    spot: u64,
    rows: Rc<Vec<Entry>>,
    selected: Rc<Vec<usize>>,
    /// The drop target to show: a row, or the whole view.
    lit: Option<Option<usize>>,
    /// Rows drawn faded (cut to the clipboard).
    faded: Rc<Vec<bool>>,
    thumbs: Rc<Thumbs>,
    /// Shown when there are no rows (an empty folder, an error).
    message: Option<String>,
    /// Where drops on the view itself go.
    dest: Option<Dest>,
    details: Vec<Detail>,
    sort: (SortKey, bool),
    scroll_to: Option<(usize, u64)>,
    rename: Option<(usize, Element<Msg>)>,
    spots: Spots,
    handlers: Rc<Handlers<Msg>>,
    now: i64,
    size: Size,
    /// Paths under it show as `~/…`.
    home: Option<Rc<PathBuf>>,
}

impl<Msg> Layoutable for FileView<Msg> {
    fn common_mut(&mut self) -> &mut Common {
        &mut self.common
    }
}

/// A view: `file_view(Layout::Column, rows, handlers, spots, spot)`.
pub fn file_view<Msg>(
    layout: Layout,
    rows: Rc<Vec<Entry>>,
    handlers: Rc<Handlers<Msg>>,
    spots: Spots,
    spot: u64,
) -> FileView<Msg> {
    let n = rows.len();
    FileView {
        common: Common::default(),
        layout,
        spot,
        rows,
        selected: Rc::new(Vec::new()),
        lit: None,
        faded: Rc::new(vec![false; n]),
        thumbs: Rc::new(Thumbs::default()),
        message: None,
        dest: None,
        details: vec![Detail::Name, Detail::Size, Detail::Kind, Detail::Modified],
        sort: (SortKey::Name, true),
        scroll_to: None,
        rename: None,
        spots,
        handlers,
        now: format::now(),
        size: Size::ZERO,
        home: None,
    }
    .width(aizome_kit::Length::Fill)
    .height(aizome_kit::Length::Fill)
}

impl<Msg> FileView<Msg> {
    pub fn selected(mut self, rows: Rc<Vec<usize>>) -> Self {
        self.selected = rows;
        self
    }

    pub fn lit(mut self, lit: Option<Option<usize>>) -> Self {
        self.lit = lit;
        self
    }

    pub fn faded(mut self, faded: Rc<Vec<bool>>) -> Self {
        self.faded = faded;
        self
    }

    pub fn thumbs(mut self, thumbs: Rc<Thumbs>) -> Self {
        self.thumbs = thumbs;
        self
    }

    pub fn message(mut self, m: Option<String>) -> Self {
        self.message = m;
        self
    }

    pub fn dest(mut self, d: Option<Dest>) -> Self {
        self.dest = d;
        self
    }

    pub fn details(mut self, d: Vec<Detail>) -> Self {
        self.details = d;
        self
    }

    pub fn sort(mut self, key: SortKey, ascending: bool) -> Self {
        self.sort = (key, ascending);
        self
    }

    /// The present, for "Today" and "Yesterday".
    pub fn now(mut self, now: i64) -> Self {
        self.now = now;
        self
    }

    /// Show paths under `home` as `~/…`.
    pub fn home(mut self, home: Rc<PathBuf>) -> Self {
        self.home = Some(home);
        self
    }

    /// Scroll so `row` shows, once per `generation`.
    pub fn scroll_to(mut self, row: usize, generation: u64) -> Self {
        self.scroll_to = Some((row, generation));
        self
    }

    /// A field over `row`'s name, renaming it.
    pub fn rename(mut self, row: usize, field: Element<Msg>) -> Self {
        self.rename = Some((row, field));
        self
    }
}

impl<Msg: Clone + Send + 'static> From<FileView<Msg>> for Element<Msg> {
    fn from(v: FileView<Msg>) -> Element<Msg> {
        let mut v = v;
        let common = std::mem::take(&mut v.common);
        let (rename_row, children) = match v.rename.take() {
            Some((row, el)) => (Some(row), vec![el]),
            None => (None, Vec::new()),
        };
        let mut el = Element::new(ViewWidget { v, rename_row }, children);
        *el.common_mut() = common;
        el
    }
}

struct ViewWidget<Msg> {
    v: FileView<Msg>,
    rename_row: Option<usize>,
}

const ICON: f64 = 16.0;
const CELL_W: f64 = 112.0;
const CELL_H: f64 = 112.0;
const BIG_ICON: f64 = 60.0;
const PAD: f64 = 8.0;
const LINGER: Duration = Duration::from_millis(900);

#[derive(Default)]
struct ViewState {
    offset: f64,
    content: f64,
    viewport: Size,
    scroll_gen: u64,
    anchor: Option<usize>,
    cursor: Option<usize>,
    typed: String,
    typed_at: Option<Instant>,
    press: Option<Press>,
    dragging: bool,
    over: Option<Target>,
    /// A rubber band, in content points, and the selection it started
    /// from.
    band: Option<(Point, Point, Vec<usize>)>,
    bar_until: Option<Instant>,
    /// The scroll bar's thumb being dragged, and where it was taken.
    thumb: Option<f64>,
}

#[derive(Clone, Debug)]
struct Press {
    at: Point,
    row: Option<usize>,
    /// A click on a row of a selection of several: it becomes the only
    /// one when the button comes up, unless a drag starts.
    deferred: Option<usize>,
}

impl<Msg: Clone + Send + 'static> ViewWidget<Msg> {
    fn emit(&self, cx: &mut EventCx, msg: Msg) {
        cx.emit(&MsgFn::new(msg));
    }

    fn row_h(&self, t: &aizome_kit::Theme) -> f64 {
        t.metrics.row
    }

    fn header(&self, t: &aizome_kit::Theme) -> f64 {
        if self.v.layout == Layout::List { t.metrics.row } else { 0.0 }
    }

    fn n(&self) -> usize {
        self.v.rows.len()
    }

    fn grid_cols(&self) -> usize {
        ((self.v.size.width / CELL_W).floor() as usize).max(1)
    }

    fn cell_w(&self) -> f64 {
        (self.v.size.width / self.grid_cols() as f64).floor()
    }

    /// Row `i`'s rectangle, in the view's coordinates, scrolled by `off`.
    fn row_rect(&self, i: usize, off: f64, t: &aizome_kit::Theme) -> Rect {
        match self.v.layout {
            Layout::Icons => {
                let cols = self.grid_cols();
                let cw = self.cell_w();
                let (r, c) = (i / cols, i % cols);
                let x = c as f64 * cw;
                let y = r as f64 * CELL_H - off;
                Rect::new(x, y, x + cw, y + CELL_H)
            }
            _ => {
                let rh = self.row_h(t);
                let y = self.header(t) + i as f64 * rh - off;
                Rect::new(0.0, y, self.v.size.width, y + rh)
            }
        }
    }

    fn content_height(&self, t: &aizome_kit::Theme) -> f64 {
        match self.v.layout {
            Layout::Icons => (self.n().div_ceil(self.grid_cols())) as f64 * CELL_H + 8.0,
            _ => self.n() as f64 * self.row_h(t),
        }
    }

    fn viewport_h(&self, t: &aizome_kit::Theme) -> f64 {
        (self.v.size.height - self.header(t)).max(0.0)
    }

    fn hit(&self, p: Point, off: f64, t: &aizome_kit::Theme) -> Option<usize> {
        if p.y < self.header(t) || p.x < 0.0 || p.x >= self.v.size.width {
            return None;
        }
        let i = match self.v.layout {
            Layout::Icons => {
                let cols = self.grid_cols();
                let c = (p.x / self.cell_w()).floor() as usize;
                let r = ((p.y + off) / CELL_H).floor();
                if r < 0.0 || c >= cols {
                    return None;
                }
                r as usize * cols + c
            }
            _ => {
                let r = ((p.y - self.header(t) + off) / self.row_h(t)).floor();
                if r < 0.0 {
                    return None;
                }
                r as usize
            }
        };
        (i < self.n()).then_some(i)
    }

    fn is_selected(&self, i: usize) -> bool {
        self.v.selected.binary_search(&i).is_ok()
    }

    /// Where a drop at `p` (the view's points) goes: a folder row, or the
    /// view's own place.
    fn target_at(&self, p: Point, off: f64, t: &aizome_kit::Theme) -> Option<Target> {
        resolve(&self.v.rows, self.hit(p, off, t), self.v.dest.clone(), self.v.spot)
    }

    fn clamp(&self, st: &mut ViewState, t: &aizome_kit::Theme) {
        let max = (self.content_height(t) - self.viewport_h(t)).max(0.0);
        st.offset = st.offset.clamp(0.0, max).round();
    }

    /// Scroll so row `i` shows.
    fn show_row(&self, st: &mut ViewState, i: usize, t: &aizome_kit::Theme) {
        let r = self.row_rect(i, 0.0, t);
        let (top, bottom) = (r.y0 - self.header(t), r.y1 - self.header(t));
        let v = self.viewport_h(t);
        if top < st.offset {
            st.offset = top;
        } else if bottom > st.offset + v {
            st.offset = bottom - v;
        }
        self.clamp(st, t);
    }

    fn scrolled(&self, cx: &mut EventCx) {
        let now = cx.now();
        cx.state::<ViewState>().bar_until = Some(now + LINGER);
        cx.request_tick(LINGER + Duration::from_millis(20));
        cx.relayout();
        cx.invalidate();
    }

    fn select(&self, cx: &mut EventCx, rows: Vec<usize>, cursor: Option<usize>) {
        cx.state::<ViewState>().cursor = cursor;
        self.emit(cx, (self.v.handlers.select)(rows));
        cx.invalidate();
    }

    /// Move the cursor to `i` from the keyboard (extending with ⇧).
    fn move_to(&self, cx: &mut EventCx, i: usize, extend: bool) {
        let n = self.n();
        if n == 0 {
            return;
        }
        let i = i.min(n - 1);
        let rows = if extend {
            let a = cx.state::<ViewState>().anchor.unwrap_or(i);
            (a.min(i)..=a.max(i)).collect()
        } else {
            cx.state::<ViewState>().anchor = Some(i);
            vec![i]
        };
        let t = cx.theme().clone();
        let st = cx.state::<ViewState>();
        self.show_row(st, i, &t);
        self.select(cx, rows, Some(i));
        self.scrolled(cx);
    }

    fn scroll_bar(&self, st: &ViewState, t: &aizome_kit::Theme, wide: bool) -> Option<(Rect, Rect)> {
        let (c, v) = (st.content, st.viewport.height);
        if c <= v + 0.5 || v <= 0.0 {
            return None;
        }
        let w = if wide { 8.0 } else { 4.0 };
        let top = self.header(t) + 2.0;
        let track = Rect::new(self.v.size.width - w - 2.0, top, self.v.size.width - 2.0, top + v - 4.0);
        let len = (track.height() * v / c).max(24.0).min(track.height());
        let y = track.y0 + (track.height() - len) * (st.offset / (c - v)).clamp(0.0, 1.0);
        Some((track, Rect::new(track.x0, y.round(), track.x1, (y + len).round())))
    }

    fn key(&self, cx: &mut EventCx, k: &aizome_kit::sidestep_ui::KeyEvent) -> Status {
        let m = k.modifiers;
        let t = cx.theme().clone();
        if cx.state::<ViewState>().dragging {
            if k.key == Key::Named(NamedKey::Escape) {
                self.end_drag(cx);
                self.emit(cx, (self.v.handlers.drag)(DragEvent::Cancel));
                return Status::Handled;
            }
            return Status::Ignored;
        }
        if self.rename_row.is_some() {
            // The field has the keys; Escape that it doesn't take cancels.
            if k.key == Key::Named(NamedKey::Escape) {
                self.emit(cx, (self.v.handlers.key)(ViewKey::Escape));
                return Status::Handled;
            }
            return Status::Ignored;
        }
        if m.super_key() || m.control() {
            return Status::Ignored;
        }
        let n = self.n();
        let cur = cx.state::<ViewState>().cursor.filter(|c| *c < n).or(self.v.selected.last().copied());
        let shift = m.shift();
        let cols = if self.v.layout == Layout::Icons { self.grid_cols() } else { 1 };
        let page = ((self.viewport_h(&t) / if self.v.layout == Layout::Icons { CELL_H } else { self.row_h(&t) }).floor()
            as usize)
            .max(1) * cols;
        let named = |key: NamedKey| k.key == Key::Named(key);
        if named(NamedKey::ArrowDown) {
            self.move_to(cx, cur.map_or(0, |c| (c + cols).min(n.saturating_sub(1))), shift);
        } else if named(NamedKey::ArrowUp) {
            self.move_to(cx, cur.map_or(0, |c| c.saturating_sub(cols)), shift);
        } else if named(NamedKey::ArrowRight) && self.v.layout == Layout::Icons {
            self.move_to(cx, cur.map_or(0, |c| (c + 1).min(n.saturating_sub(1))), shift);
        } else if named(NamedKey::ArrowLeft) && self.v.layout == Layout::Icons {
            self.move_to(cx, cur.map_or(0, |c| c.saturating_sub(1)), shift);
        } else if named(NamedKey::ArrowRight) && self.v.layout == Layout::Column {
            self.emit(cx, (self.v.handlers.key)(ViewKey::Into));
        } else if named(NamedKey::ArrowLeft) && self.v.layout == Layout::Column {
            self.emit(cx, (self.v.handlers.key)(ViewKey::Out));
        } else if named(NamedKey::Home) {
            self.move_to(cx, 0, shift);
        } else if named(NamedKey::End) {
            self.move_to(cx, n.saturating_sub(1), shift);
        } else if named(NamedKey::PageDown) {
            self.move_to(cx, cur.map_or(0, |c| c + page), shift);
        } else if named(NamedKey::PageUp) {
            self.move_to(cx, cur.map_or(0, |c| c.saturating_sub(page)), shift);
        } else if named(NamedKey::Enter) {
            match cur {
                Some(c) if !self.v.selected.is_empty() => self.emit(cx, (self.v.handlers.activate)(c)),
                _ => return Status::Ignored,
            }
        } else if named(NamedKey::F(2)) {
            self.emit(cx, (self.v.handlers.key)(ViewKey::Rename));
        } else if named(NamedKey::Escape) {
            self.emit(cx, (self.v.handlers.key)(ViewKey::Escape));
        } else if matches!(&k.key, Key::Character(s) if s == " ") && !m.alt() {
            self.emit(cx, (self.v.handlers.key)(ViewKey::QuickLook));
        } else if let (Key::Character(_), Some(text)) = (&k.key, k.text.as_ref()) {
            if text.chars().any(char::is_control) || n == 0 {
                return Status::Ignored;
            }
            let now = cx.now();
            let st = cx.state::<ViewState>();
            if st.typed_at.is_none_or(|t| now.duration_since(t) > Duration::from_millis(1000)) {
                st.typed.clear();
            }
            st.typed.push_str(&text.to_lowercase());
            st.typed_at = Some(now);
            let prefix = st.typed.clone();
            let start = cur.unwrap_or(0);
            let from = if prefix.chars().count() == 1 { start + 1 } else { start };
            let rows = &self.v.rows;
            if let Some(i) =
                (0..n).map(|k| (from + k) % n).find(|&i| rows[i].display.to_lowercase().starts_with(&prefix))
            {
                self.move_to(cx, i, false);
            }
        } else {
            return Status::Ignored;
        }
        Status::Handled
    }

    fn end_drag(&self, cx: &mut EventCx) {
        let st = cx.state::<ViewState>();
        st.dragging = false;
        st.press = None;
        st.over = None;
        cx.release_pointer();
        cx.set_cursor(Cursor::Default);
    }

    fn band_rows(&self, a: Point, b: Point, t: &aizome_kit::Theme) -> Vec<usize> {
        let band = Rect::from_points(a, b);
        (0..self.n()).filter(|&i| self.row_rect(i, 0.0, t).intersect(band).area() > 0.0).collect()
    }
}

/// The target a drop on row `row` (or the view) of `rows` has.
fn resolve(rows: &[Entry], row: Option<usize>, dest: Option<Dest>, spot: u64) -> Option<Target> {
    if let Some(i) = row
        && let Some(e) = rows.get(i)
        && e.is_folder()
        && e.trashed.is_none()
    {
        return Some(Target { dest: Dest::Folder(e.path.clone()), spot, row: Some(i) });
    }
    dest.map(|dest| Target { dest, spot, row: None })
}

impl<Msg: Clone + Send + 'static> Widget for ViewWidget<Msg> {
    fn name(&self) -> &'static str {
        "hako-file-view"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 240.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 320.0 },
        ));
        self.v.size = size;
        let t = cx.theme().clone();
        let content = self.content_height(&t);
        let viewport = Size::new(size.width, self.viewport_h(&t));
        let scroll_to = self.v.scroll_to;
        let st = cx.state::<ViewState>();
        st.content = content;
        st.viewport = viewport;
        if let Some((row, generation)) = scroll_to
            && generation != st.scroll_gen
        {
            st.scroll_gen = generation;
            if row < self.v.rows.len() {
                // Show it, and keep the keyboard's cursor there.
                st.cursor = Some(row);
                st.anchor = Some(row);
                let r = self.row_rect(row, 0.0, &t);
                let (top, bottom) = (r.y0 - self.header(&t), r.y1 - self.header(&t));
                if top < st.offset || bottom > st.offset + viewport.height {
                    st.offset = (top - (viewport.height - (bottom - top)) / 2.0).max(0.0);
                }
            }
        }
        let max = (content - viewport.height).max(0.0);
        st.offset = st.offset.clamp(0.0, max).round();
        let off = st.offset;
        if let Some(row) = self.rename_row
            && cx.child_count() > 0
        {
            let r = self.row_rect(row, off, &t);
            let field = match self.v.layout {
                Layout::Icons => Rect::new(
                    r.x0 + 4.0,
                    r.y0 + 8.0 + BIG_ICON + 4.0,
                    r.x1 - 4.0,
                    r.y0 + 8.0 + BIG_ICON + 4.0 + t.metrics.control,
                ),
                Layout::List => {
                    let w = self.name_width();
                    Rect::new(PAD + ICON + PAD - 4.0, r.y0, PAD + w, r.y1)
                }
                Layout::Column => Rect::new(PAD + ICON + PAD - 4.0, r.y0, size.width - PAD, r.y1),
            };
            cx.layout_child(0, Constraints::tight(field.size()));
            cx.place_child(0, field.origin());
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        cx.canvas.fill_rect(b, t.surface());
        // Where it is in the window, for drags.
        let origin = cx.canvas.transform().translation();
        let window_rect = b + origin;
        let off = cx.state::<ViewState>().map_or(0.0, |s| s.offset);
        {
            let rows = self.v.rows.clone();
            let dest = self.v.dest.clone();
            let spot = self.v.spot;
            let geometry = Geometry {
                layout: self.v.layout,
                size: self.v.size,
                header: self.header(t),
                row_h: self.row_h(t),
                n: rows.len(),
            };
            let resolve_fn: dnd::Resolve = Rc::new(move |p: Point| {
                let local = Point::new(p.x - origin.x, p.y - origin.y);
                resolve(&rows, geometry.hit(local, off), dest.clone(), spot)
            });
            self.v.spots.set(spot, Spot { rect: window_rect, priority: 0, resolve: resolve_fn });
        }
        let active = cx.is_focused() && cx.window_active();
        if self.n() == 0
            && let Some(m) = &self.v.message
        {
            let spec = TextSpec {
                max_lines: 3,
                wrap: aizome_kit::sidestep_ui::Wrap::Words,
                align: aizome_kit::sidestep_ui::Alignment::Center,
                ..TextSpec::new(FontSpec::ui(t.metrics.small + 1.0), t.text3())
            };
            let w = (b.width() - 24.0).max(10.0);
            let l = cx.layout_text(m, &spec, Some(w));
            let s = l.size();
            cx.canvas.draw_text(
                &l,
                Point::new(((b.width() - s.width) / 2.0).round(), (b.height() * 0.38 - s.height / 2.0).round()),
            );
        }
        let header = self.header(t);
        let body = Rect::new(0.0, header, b.x1, b.y1);
        cx.canvas.save();
        cx.canvas.clip_rect(body);

        let (first, last) = match self.v.layout {
            Layout::Icons => {
                let cols = self.grid_cols();
                (
                    ((off / CELL_H).floor().max(0.0) as usize) * cols,
                    ((((off + body.height()) / CELL_H).ceil().max(0.0) as usize) * cols).min(self.n()),
                )
            }
            _ => {
                let rh = self.row_h(t);
                (
                    (off / rh).floor().max(0.0) as usize,
                    (((off + body.height()) / rh).ceil().max(0.0) as usize).min(self.n()),
                )
            }
        };
        for i in first.min(last)..last {
            let r = self.row_rect(i, off, t);
            if !cx.canvas.needs(r) {
                continue;
            }
            self.paint_row(cx, i, r, active);
        }
        // The drop target.
        if let Some(lit) = self.v.lit {
            let r = match lit {
                Some(i) if i < self.n() => self.row_rect(i, off, t),
                _ => body,
            };
            aizome_kit::draw::border(cx.canvas, r.inset(if lit.is_some() { 0.0 } else { -1.0 }), t.focus(), 2.0);
        }
        if let Some(st) = cx.state::<ViewState>()
            && let Some((a, z, _)) = &st.band
        {
            let r = Rect::from_points(*a, *z) + Vec2::new(0.0, -off);
            cx.canvas.fill_rect(r, t.selection().with_alpha(0.18));
            aizome_kit::draw::border(cx.canvas, r, t.selection(), 1.0);
        }
        cx.canvas.restore();
        if self.v.layout == Layout::List {
            self.paint_header(cx);
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let Some(st) = cx.state::<ViewState>() else { return };
        let visible = st.thumb.is_some() || st.bar_until.is_some_and(|u| cx.now() < u);
        if !visible {
            return;
        }
        let wide = st.thumb.is_some();
        if let Some((track, thumb)) = self.scroll_bar(st, t, wide) {
            if wide {
                cx.canvas.fill_rect(track, t.sunk().with_alpha(0.7));
            }
            cx.canvas.fill_rect(thumb, if wide { t.text3() } else { t.line2().mix(t.text3(), 0.35) });
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(Rc::as_ptr(&self.v.rows) as usize, self.v.rows.len(), self.v.layout, self.v.spot));
        h.add(&*self.v.selected).add(&self.v.lit).add(&*self.v.faded).add(&self.v.thumbs.generation);
        h.add(&self.v.message).add(&self.v.sort).add(&self.v.details).add(&self.rename_row).add(&self.v.dest);
        h.add(&(self.v.now / 60));
    }

    fn clips(&self) -> bool {
        true
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
        let t = cx.theme().clone();
        match event {
            Event::Focus(_) | Event::WindowFocus(_) => {
                cx.invalidate();
                Status::Ignored
            }
            Event::Tick => {
                cx.invalidate();
                Status::Ignored
            }
            Event::Scroll { delta, mods, .. } => {
                if mods.shift() || delta.y == 0.0 {
                    return Status::Ignored;
                }
                let before = cx.state::<ViewState>().offset;
                let st = cx.state::<ViewState>();
                st.offset += delta.y;
                self.clamp(st, &t);
                if st.offset == before {
                    return Status::Ignored;
                }
                self.scrolled(cx);
                Status::Handled
            }
            Event::Key(k) if k.pressed => self.key(cx, k),
            Event::PointerDown { pos, button, clicks, mods } => {
                // The strip's grips for resizing columns are at the edges'
                // feet.
                if self.v.layout == Layout::Column
                    && pos.y >= self.v.size.height - super::strip::GRIP
                    && (pos.x < 5.0 || pos.x > self.v.size.width - 5.0)
                {
                    return Status::Ignored;
                }
                let st = cx.state::<ViewState>();
                let off = st.offset;
                // The scroll bar.
                if *button == PointerButton::Left
                    && let Some((track, thumb)) = self.scroll_bar(st, &t, true)
                    && track.inflate(2.0, 0.0).contains(*pos)
                {
                    let grab = if thumb.contains(*pos) { pos.y - thumb.y0 } else { thumb.height() / 2.0 };
                    st.thumb = Some(grab);
                    self.drag_thumb(cx, *pos, &t);
                    return Status::Handled;
                }
                if self.v.layout == Layout::List && pos.y < self.header(&t) {
                    if let Some(key) = self.detail_at(pos.x).and_then(Detail::sort_key) {
                        self.emit(cx, (self.v.handlers.sort)(key));
                    }
                    return Status::Handled;
                }
                if self.rename_row.is_some() {
                    // A click away from the field ends the renaming.
                    self.emit(cx, (self.v.handlers.key)(ViewKey::ClickAway));
                }
                cx.request_focus();
                let row = self.hit(*pos, off, &t);
                match button {
                    PointerButton::Left => {
                        let Some(i) = row else {
                            // Empty space: nothing selected, and a rubber
                            // band where there's room for one.
                            let keep =
                                if mods.super_key() || mods.shift() { (*self.v.selected).clone() } else { Vec::new() };
                            if self.v.layout != Layout::Column {
                                let p = Point::new(pos.x, pos.y + off);
                                cx.state::<ViewState>().band = Some((p, p, keep.clone()));
                            }
                            if !self.v.selected.is_empty() && keep.is_empty() {
                                self.select(cx, Vec::new(), None);
                            }
                            return Status::Handled;
                        };
                        if *clicks >= 2 {
                            if !self.is_selected(i) {
                                self.select(cx, vec![i], Some(i));
                            }
                            self.emit(cx, (self.v.handlers.activate)(i));
                            return Status::Handled;
                        }
                        let mut deferred = None;
                        if mods.super_key() {
                            let mut s = (*self.v.selected).clone();
                            match s.binary_search(&i) {
                                Ok(p) => {
                                    s.remove(p);
                                }
                                Err(p) => s.insert(p, i),
                            }
                            cx.state::<ViewState>().anchor = Some(i);
                            self.select(cx, s, Some(i));
                        } else if mods.shift() {
                            let a = cx.state::<ViewState>().anchor.unwrap_or(i);
                            self.select(cx, (a.min(i)..=a.max(i)).collect(), Some(i));
                        } else if self.is_selected(i) && self.v.selected.len() > 1 {
                            deferred = Some(i);
                            cx.state::<ViewState>().anchor = Some(i);
                        } else {
                            cx.state::<ViewState>().anchor = Some(i);
                            if !(self.is_selected(i) && self.v.selected.len() == 1) {
                                self.select(cx, vec![i], Some(i));
                            } else {
                                cx.state::<ViewState>().cursor = Some(i);
                            }
                        }
                        cx.state::<ViewState>().press = Some(Press { at: *pos, row: Some(i), deferred });
                        Status::Handled
                    }
                    PointerButton::Right => {
                        if let Some(i) = row
                            && !self.is_selected(i)
                        {
                            cx.state::<ViewState>().anchor = Some(i);
                            self.select(cx, vec![i], Some(i));
                        } else if row.is_none() && !self.v.selected.is_empty() {
                            self.select(cx, Vec::new(), None);
                        }
                        let at = cx.window_rect().origin() + pos.to_vec2();
                        self.emit(cx, (self.v.handlers.menu)(row, at));
                        Status::Handled
                    }
                    _ => Status::Ignored,
                }
            }
            Event::PointerMove { pos, .. } => {
                let st = cx.state::<ViewState>();
                if st.thumb.is_some() {
                    self.drag_thumb(cx, *pos, &t);
                    return Status::Handled;
                }
                let off = st.offset;
                if let Some((a, _, keep)) = st.band.clone() {
                    let z = Point::new(pos.x.clamp(0.0, self.v.size.width), pos.y + off);
                    st.band = Some((a, z, keep.clone()));
                    let mut rows = self.band_rows(a, z, &t);
                    for k in keep {
                        if !rows.contains(&k) {
                            rows.push(k);
                        }
                    }
                    rows.sort_unstable();
                    if rows != *self.v.selected {
                        self.select(cx, rows, None);
                    }
                    cx.invalidate();
                    return Status::Handled;
                }
                let Some(press) = st.press.clone() else { return Status::Ignored };
                if !st.dragging {
                    if press.row.is_none() || (*pos - press.at).hypot() < dnd::SLOP {
                        return Status::Handled;
                    }
                    st.dragging = true;
                    if let Some(i) = press.deferred.or(press.row)
                        && !self.is_selected(i)
                    {
                        self.select(cx, vec![i], Some(i));
                    }
                    cx.capture_pointer();
                    cx.set_cursor(Cursor::Grabbing);
                    self.emit(cx, (self.v.handlers.drag)(DragEvent::Start));
                }
                let at = cx.window_rect().origin() + pos.to_vec2();
                if self.v.spots.window_bounds().is_some_and(|win| !win.inflate(-2.0, -2.0).contains(at)) {
                    // Out of the window: the desktop carries it on.
                    self.end_drag(cx);
                    self.emit(cx, (self.v.handlers.drag)(DragEvent::Leave(at)));
                    return Status::Handled;
                }
                let target = self.v.spots.at(at);
                let st = cx.state::<ViewState>();
                if target != st.over {
                    st.over = target.clone();
                    self.emit(cx, (self.v.handlers.drag)(DragEvent::Over(target)));
                }
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, mods } => {
                let st = cx.state::<ViewState>();
                if st.thumb.take().is_some() {
                    self.scrolled(cx);
                    return Status::Handled;
                }
                if st.band.take().is_some() {
                    cx.invalidate();
                    return Status::Handled;
                }
                let Some(press) = st.press.take() else { return Status::Ignored };
                if st.dragging {
                    let at = cx.window_rect().origin() + pos.to_vec2();
                    let target = self.v.spots.at(at);
                    self.end_drag(cx);
                    let copy = mods.alt() || mods.control();
                    self.emit(cx, (self.v.handlers.drag)(DragEvent::Drop { target, copy }));
                } else if let Some(i) = press.deferred {
                    self.select(cx, vec![i], Some(i));
                }
                Status::Handled
            }
            Event::DragOver { pos, urls, .. } => {
                let off = cx.state::<ViewState>().offset;
                if !urls.is_empty() && self.target_at(*pos, off, &t).is_some() {
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::Drop { pos, urls, .. } => {
                let off = cx.state::<ViewState>().offset;
                match self.target_at(*pos, off, &t) {
                    Some(target) if !urls.is_empty() => {
                        self.emit(cx, (self.v.handlers.drop)(target.dest, urls.clone()));
                        Status::Handled
                    }
                    _ => Status::Ignored,
                }
            }
            _ => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: if self.v.layout == Layout::List { SemanticRole::Table } else { SemanticRole::List },
            value: Some(format!("{} items, {} selected", self.n(), self.v.selected.len())),
            ..Default::default()
        })
    }

    fn selection(&self, _state: aizome_kit::widget::StateView) -> Option<String> {
        Some(
            self.v
                .selected
                .iter()
                .filter_map(|&i| self.v.rows.get(i))
                .map(|e| e.display.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

/// What a resolver needs to find rows: the view's shape at drawing time.
#[derive(Clone, Copy)]
struct Geometry {
    layout: Layout,
    size: Size,
    header: f64,
    row_h: f64,
    n: usize,
}

impl Geometry {
    fn hit(&self, p: Point, off: f64) -> Option<usize> {
        if p.y < self.header || p.x < 0.0 || p.x >= self.size.width || p.y >= self.size.height {
            return None;
        }
        let i = match self.layout {
            Layout::Icons => {
                let cols = ((self.size.width / CELL_W).floor() as usize).max(1);
                let cw = (self.size.width / cols as f64).floor();
                let c = (p.x / cw).floor() as usize;
                let r = ((p.y + off) / CELL_H).floor();
                if c >= cols || r < 0.0 {
                    return None;
                }
                r as usize * cols + c
            }
            _ => {
                let r = ((p.y - self.header + off) / self.row_h).floor();
                if r < 0.0 {
                    return None;
                }
                r as usize
            }
        };
        (i < self.n).then_some(i)
    }
}

impl<Msg: Clone + Send + 'static> ViewWidget<Msg> {
    fn drag_thumb(&self, cx: &mut EventCx, pos: Point, t: &aizome_kit::Theme) {
        let st = cx.state::<ViewState>();
        let Some(grab) = st.thumb else { return };
        let Some((track, thumb)) = self.scroll_bar(st, t, true) else { return };
        let room = (track.height() - thumb.height()).max(1.0);
        let f = ((pos.y - grab - track.y0) / room).clamp(0.0, 1.0);
        st.offset = f * (st.content - st.viewport.height);
        self.clamp(st, t);
        self.scrolled(cx);
    }

    /// The name column's width in a list.
    fn name_width(&self) -> f64 {
        let fixed: f64 = self.v.details.iter().filter(|d| **d != Detail::Name).map(|d| d.width()).sum();
        (self.v.size.width - fixed).max(160.0)
    }

    /// The details' x ranges in a list.
    fn detail_spans(&self) -> Vec<(Detail, f64, f64)> {
        let mut x = 0.0;
        let name = self.name_width();
        self.v
            .details
            .iter()
            .map(|d| {
                let w = if *d == Detail::Name { name } else { d.width() };
                let span = (*d, x, x + w);
                x += w;
                span
            })
            .collect()
    }

    fn detail_at(&self, x: f64) -> Option<Detail> {
        self.detail_spans().into_iter().find(|(_, a, b)| x >= *a && x < *b).map(|(d, _, _)| d)
    }

    fn paint_header(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let h = Rect::new(0.0, 0.0, self.v.size.width, self.header(t));
        cx.canvas.fill_rect(h, t.chrome());
        aizome_kit::draw::bottom(cx.canvas, h, t.line(), 1.0);
        let base = TextSpec::new(FontSpec { weight: 600.0, ..FontSpec::ui(t.metrics.small + 1.0) }, t.text3());
        let spans = self.detail_spans();
        for (k, (d, x0, x1)) in spans.iter().enumerate() {
            let sorted = d.sort_key().is_some_and(|key| key == self.v.sort.0);
            let fg = if sorted { t.text() } else { t.text3() };
            let align = if *d == Detail::Size { Align::End } else { Align::Start };
            let mut right = x1 - PAD;
            let mut left = x0 + PAD;
            if sorted {
                let icon = if self.v.sort.1 { Icon::ChevronUp } else { Icon::ChevronDown };
                if align == Align::End {
                    icon.draw(cx.canvas, Point::new(left, (h.height() - 10.0) / 2.0), 10.0, fg);
                    left += 14.0;
                } else {
                    icon.draw(cx.canvas, Point::new(right - 10.0, (h.height() - 10.0) / 2.0), 10.0, fg);
                    right -= 14.0;
                }
            }
            let left = if *d == Detail::Name { left + ICON + PAD } else { left };
            cx.draw_text_in(
                d.title(),
                &TextSpec { color: fg, ..base.clone() },
                Rect::new(left, 0.0, right, h.y1),
                align,
            );
            if k + 1 < spans.len() {
                cx.canvas.fill_rect(Rect::new(x1 - 1.0, 5.0, *x1, h.y1 - 5.0), t.line());
            }
        }
    }

    fn paint_row(&self, cx: &mut PaintCx, i: usize, r: Rect, active: bool) {
        let t = cx.theme();
        let e = &self.v.rows[i];
        let selected = self.is_selected(i);
        let on_sel = selected && active;
        let faded = self.v.faded.get(i).copied().unwrap_or(false);
        let text_color = if on_sel {
            t.on_selection()
        } else if e.hidden || faded {
            t.text3()
        } else {
            t.text()
        };
        let (icon, role) = look::entry_icon(e);
        let icon_color = {
            let c = look::tint(t, role, on_sel);
            if faded { c.with_alpha(0.5) } else { c }
        };
        let thumb = self.v.thumbs.images.get(&e.path);
        let body = TextSpec::new(FontSpec::ui(t.metrics.body), text_color);
        match self.v.layout {
            Layout::Icons => {
                let cell = r.inset(-4.0);
                let icon_rect = Rect::new(cell.x0 + (cell.width() - BIG_ICON) / 2.0, cell.y0 + 4.0, 0.0, 0.0);
                let icon_rect = Rect::new(
                    icon_rect.x0.round(),
                    icon_rect.y0,
                    icon_rect.x0.round() + BIG_ICON,
                    icon_rect.y0 + BIG_ICON,
                );
                if selected {
                    let fill = if active { t.selection() } else { t.line2() };
                    cx.canvas.fill_rect(icon_rect.inflate(4.0, 4.0), fill.with_alpha(if active { 0.35 } else { 0.6 }));
                }
                match thumb {
                    Some(img) => draw_fit(cx, img, icon_rect, faded),
                    None => icon.draw(cx.canvas, icon_rect.origin(), BIG_ICON, icon_color),
                }
                if e.is_link() {
                    link_badge(cx, Point::new(icon_rect.x0, icon_rect.y1 - 14.0), 14.0);
                }
                let spec = TextSpec {
                    max_lines: 2,
                    wrap: aizome_kit::sidestep_ui::Wrap::Words,
                    align: aizome_kit::sidestep_ui::Alignment::Center,
                    ..TextSpec::new(FontSpec::ui(t.metrics.small + 1.0), text_color)
                };
                let w = cell.width() - 4.0;
                let l = cx.layout_text(&e.display, &spec, Some(w));
                // Centred lines in a box as wide as the cell: the box's left edge
                // is where it's drawn, the widest line's extent what's filled.
                let s = l.size();
                let x = (cell.x0 + 2.0).round();
                let y = icon_rect.y1 + 6.0;
                if selected {
                    let fill = if active { t.selection() } else { t.line2() };
                    let mid = cell.x0 + cell.width() / 2.0;
                    let half = s.width.min(w) / 2.0;
                    cx.canvas.fill_rect(
                        Rect::new((mid - half - 3.0).round(), y - 1.0, (mid + half + 3.0).round(), y + s.height + 1.0),
                        fill,
                    );
                }
                cx.canvas.draw_text(&l, Point::new(x, y));
            }
            Layout::Column | Layout::List => {
                if selected {
                    cx.canvas.fill_rect(r, if active { t.selection() } else { t.line2() });
                }
                let ir = Rect::new(r.x0 + PAD, r.y0 + ((r.height() - ICON) / 2.0).round(), r.x0 + PAD + ICON, 0.0);
                let ir = Rect::new(ir.x0, ir.y0, ir.x1, ir.y0 + ICON);
                match thumb {
                    Some(img) => draw_fit(cx, img, ir, faded),
                    None => icon.draw(cx.canvas, ir.origin(), ICON, icon_color),
                }
                if e.is_link() {
                    link_badge(cx, Point::new(ir.x0 - 2.0, ir.y1 - 7.0), 9.0);
                }
                let x = ir.x1 + PAD;
                if self.v.layout == Layout::Column {
                    let mut right = r.x1 - PAD;
                    if e.is_folder() {
                        let c = if on_sel { t.on_selection() } else { t.text3() };
                        Icon::ChevronRight.draw(
                            cx.canvas,
                            Point::new(r.x1 - PAD - 12.0, r.y0 + ((r.height() - 12.0) / 2.0).round()),
                            12.0,
                            c,
                        );
                        right -= 16.0;
                    }
                    if self.rename_row != Some(i) {
                        cx.draw_text_in(&e.display, &body, Rect::new(x, r.y0, right, r.y1), Align::Start);
                    }
                } else {
                    let small = TextSpec::new(
                        FontSpec::ui(t.metrics.small + 1.0),
                        if on_sel { t.on_selection() } else { t.text2() },
                    );
                    for (d, x0, x1) in self.detail_spans() {
                        let cell = Rect::new(x0 + PAD, r.y0, x1 - PAD, r.y1);
                        match d {
                            Detail::Name => {
                                if self.rename_row != Some(i) {
                                    cx.draw_text_in(
                                        &e.display,
                                        &body,
                                        Rect::new(x, r.y0, x1 - PAD, r.y1),
                                        Align::Start,
                                    );
                                }
                            }
                            Detail::Size => {
                                let s = if e.is_folder() { "—".to_owned() } else { format::size(e.size) };
                                let spec = TextSpec::new(
                                    FontSpec { tabular: true, ..FontSpec::mono(t.metrics.small) },
                                    small.color,
                                );
                                cx.draw_text_in(&s, &spec, cell, Align::End);
                            }
                            Detail::Kind => {
                                cx.draw_text_in(&e.kind_name, &small, cell, Align::Start);
                            }
                            Detail::Modified => {
                                let s = e.modified.map(|m| format::date(m, self.v.now)).unwrap_or_else(|| "—".into());
                                cx.draw_text_in(&s, &small, cell, Align::Start);
                            }
                            Detail::Where => {
                                let dir = match &e.trashed {
                                    Some(tr) => tr.original.as_ref().and_then(|o| o.parent()),
                                    None => e.path.parent(),
                                };
                                let w = dir.map(|p| tilde(p, self.v.home.as_deref()));
                                cx.draw_text_in(&w.unwrap_or_else(|| "—".into()), &small, cell, Align::Start);
                            }
                            Detail::Trashed => {
                                let s = e
                                    .trashed
                                    .as_ref()
                                    .and_then(|tr| tr.deleted.as_deref())
                                    .and_then(crate::fsops::trash::parse_date)
                                    .map(|d| format::date(d - local_offset(d), self.v.now))
                                    .unwrap_or_else(|| "—".into());
                                cx.draw_text_in(&s, &small, cell, Align::Start);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// `p` with `home` written `~`.
pub fn tilde(p: &std::path::Path, home: Option<&PathBuf>) -> String {
    match home.and_then(|h| p.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

/// The local zone's offset from UTC at `secs`, in seconds.
fn local_offset(secs: i64) -> i64 {
    // SAFETY: `localtime_r` writes only into `tm`.
    unsafe {
        let t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() { 0 } else { tm.tm_gmtoff as i64 }
    }
}

/// An image fitted into `r`, centred, keeping its shape.
fn draw_fit(cx: &mut PaintCx, img: &Image, r: Rect, faded: bool) {
    let s = img.size();
    if s.width <= 0.0 || s.height <= 0.0 {
        return;
    }
    let k = (r.width() / s.width).min(r.height() / s.height);
    let (w, h) = (s.width * k, s.height * k);
    let dst = Rect::new(r.x0 + (r.width() - w) / 2.0, r.y0 + (r.height() - h) / 2.0, 0.0, 0.0);
    let dst = Rect::new(dst.x0, dst.y0, dst.x0 + w, dst.y0 + h);
    let opts = aizome_kit::sidestep_ui::ImageOptions { alpha: if faded { 0.5 } else { 1.0 }, ..Default::default() };
    cx.canvas.draw_image_with(img, None, dst, opts);
    aizome_kit::draw::border(cx.canvas, dst.inflate(1.0, 1.0), cx.theme().line(), 1.0);
}

/// The little arrow on a link's icon.
fn link_badge(cx: &mut PaintCx, at: Point, size: f64) {
    let t = cx.theme();
    cx.canvas.fill_rect(Rect::new(at.x, at.y, at.x + size, at.y + size), t.surface());
    aizome_kit::draw::border(cx.canvas, Rect::new(at.x, at.y, at.x + size, at.y + size), t.line2(), 1.0);
    Icon::ArrowUpRight.draw(cx.canvas, Point::new(at.x + 1.0, at.y + 1.0), size - 2.0, t.text2());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_folders_and_the_view() {
        let t = crate::fsops::testing::TempDir::new("resolve");
        t.dir("folder");
        t.file("file.txt", "x");
        let mut rows = crate::model::entry::read_folder(t.path()).unwrap();
        crate::model::entry::sort_entries(&mut rows, Default::default());

        let here = Dest::Folder(t.path().to_path_buf());
        assert_eq!(resolve(&rows, Some(0), Some(here.clone()), 3).unwrap().dest, Dest::Folder(t.join("folder")));
        assert_eq!(
            resolve(&rows, Some(1), Some(here.clone()), 3).unwrap(),
            Target { dest: here.clone(), spot: 3, row: None }
        );
        assert_eq!(resolve(&rows, None, None, 3), None);
    }
}
