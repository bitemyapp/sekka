//! Tables and outlines: `table` (columns with sortable, resizable
//! headers, virtualised rows) and `tree` (an outline with disclosure
//! triangles, for Kura and Files). Both select as lists do: clicks (⇧ and
//! ❖ for several), arrows, Home/End, Page Up/Down, type-select, Return or
//! a double click to activate.

use std::collections::HashSet;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sidestep_ui::{Cursor, Key, NamedKey, PointerButton};

use crate::draw;
use crate::element::{Common, Element, MsgMap, RawElement};
use crate::hash::PaintHasher;
use crate::icons::Icon;
use crate::layout::{Align, Constraints, Length, Point, Rect, Size, Vec2};
use crate::menu::{Menu, MenuSpec};
use crate::text::{TextSpec, text};
use crate::theme::{FontSpec, Role};
use crate::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, StateView, Status, Widget,
};
use crate::widgets::scroll::{Direction, DragOutFn, ScrollState, Scroller, drag_moved, drag_pressed, drag_released};

/// Which rows are selected, and how the keyboard and pointer change it:
/// shared by tables and outlines. Rows are numbered as they show.
struct Picker<'a> {
    count: usize,
    selected: &'a [usize],
    multi: bool,
}

#[derive(Default)]
struct PickState {
    anchor: Option<usize>,
    cursor: Option<usize>,
    typed: String,
    typed_at: Option<Instant>,
}

impl Picker<'_> {
    /// The rows a click on `i` selects.
    fn click(&self, cx: &mut EventCx, i: usize) -> Vec<usize> {
        let mods = cx.modifiers();
        if self.multi && mods.super_key() {
            let mut s = self.selected.to_vec();
            match s.iter().position(|x| *x == i) {
                Some(p) => {
                    s.remove(p);
                }
                None => s.push(i),
            }
            s.sort_unstable();
            cx.state::<PickState>().anchor = Some(i);
            s
        } else if self.multi && mods.shift() {
            let a = cx.state::<PickState>().anchor.unwrap_or(i);
            (a.min(i)..=a.max(i)).collect()
        } else {
            cx.state::<PickState>().anchor = Some(i);
            vec![i]
        }
    }

    /// The row a key moves to (and whether it extends), if it's one.
    fn key(
        &self,
        cx: &mut EventCx,
        k: &sidestep_ui::KeyEvent,
        page: usize,
        names: Option<&dyn Fn(usize) -> String>,
    ) -> Option<(usize, bool)> {
        if self.count == 0 {
            return None;
        }
        let n = self.count;
        let cur = cx.state::<PickState>().cursor.or(self.selected.last().copied());
        let shift = k.modifiers.shift() && self.multi;
        Some(match &k.key {
            Key::Named(NamedKey::ArrowDown) => (cur.map_or(0, |c| (c + 1).min(n - 1)), shift),
            Key::Named(NamedKey::ArrowUp) => (cur.map_or(0, |c| c.saturating_sub(1)), shift),
            Key::Named(NamedKey::Home) => (0, shift),
            Key::Named(NamedKey::End) => (n - 1, shift),
            Key::Named(NamedKey::PageDown) => (cur.map_or(0, |c| (c + page).min(n - 1)), shift),
            Key::Named(NamedKey::PageUp) => (cur.map_or(0, |c| c.saturating_sub(page)), shift),
            Key::Character(_) if k.text.is_some() && names.is_some() => {
                let names = names?;
                let typed = k.text.clone().unwrap_or_default().to_lowercase();
                let now = cx.now();
                let st = cx.state::<PickState>();
                if st.typed_at.is_none_or(|t| now.duration_since(t) > Duration::from_millis(1000)) {
                    st.typed.clear();
                }
                st.typed.push_str(&typed);
                st.typed_at = Some(now);
                let prefix = st.typed.clone();
                let start = cur.unwrap_or(0);
                let from = if prefix.chars().count() == 1 { start + 1 } else { start };
                ((0..n).map(|k| (from + k) % n).find(|&i| names(i).to_lowercase().starts_with(&prefix))?, false)
            }
            _ => return None,
        })
    }

    /// The rows moving to `i` selects.
    fn moved(&self, cx: &mut EventCx, i: usize, extend: bool) -> Vec<usize> {
        cx.state::<PickState>().cursor = Some(i);
        if extend {
            let a = cx.state::<PickState>().anchor.unwrap_or(i);
            (a.min(i)..=a.max(i)).collect()
        } else {
            cx.state::<PickState>().anchor = Some(i);
            vec![i]
        }
    }
}

/// Scroll a row-list so row `i` (of height `rh`, under a header `top`)
/// shows.
fn show_row(cx: &mut EventCx, i: usize, rh: f64) {
    let st = cx.state::<ScrollState>();
    let (top, bottom) = (i as f64 * rh, (i + 1) as f64 * rh);
    let v = st.viewport.height;
    let to = if top < st.offset.y {
        Some(top)
    } else if bottom > st.offset.y + v {
        Some(bottom - v)
    } else {
        None
    };
    if let Some(y) = to {
        let x = st.offset.x;
        Scroller { dir: Direction::Both }.scroll_to(cx, Vec2::new(x, y));
    }
}

fn paint_selection(cx: &mut PaintCx, rows: &[usize], shown: &std::ops::Range<usize>, rh: f64, top: f64, width: f64) {
    let t = cx.theme();
    let off = cx.state::<ScrollState>().map_or(0.0, |s| s.offset.y);
    let fill = if cx.is_focused() && cx.window_active() { t.selection() } else { t.line2() };
    for &i in rows {
        if shown.contains(&i) {
            let y = top + i as f64 * rh - off;
            cx.canvas.fill_rect(Rect::new(0.0, y, width, y + rh), fill);
        }
    }
}

// Tables.

/// A table's column.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub title: String,
    pub width: f64,
    pub min: f64,
    pub align: Align,
    pub sortable: bool,
}

impl Column {
    pub fn new(title: impl Into<String>) -> Column {
        Column { title: title.into(), width: 120.0, min: 40.0, align: Align::Start, sortable: false }
    }

    pub fn width(mut self, w: f64) -> Column {
        self.width = w;
        self
    }

    pub fn min_width(mut self, w: f64) -> Column {
        self.min = w;
        self
    }

    /// Numbers line up at the right.
    pub fn align_end(mut self) -> Column {
        self.align = Align::End;
        self
    }

    pub fn sortable(mut self) -> Column {
        self.sortable = true;
        self
    }
}

type CellFn = Rc<dyn Fn(usize, usize) -> RawElement>;

/// Rows of cells under column headers: `table(columns, rows.len(), move |r, c| text(cell(r, c)).into())`.
/// Only the rows that show are built.
pub struct Table<Msg> {
    pub(crate) common: Common,
    columns: Vec<Column>,
    count: usize,
    cell: CellFn,
    row_height: Option<f64>,
    selected: Vec<usize>,
    on_select: Option<MsgMap<Vec<usize>>>,
    on_activate: Option<MsgMap<usize>>,
    on_sort: Option<MsgMap<(usize, bool)>>,
    sort: Option<(usize, bool)>,
    multi: bool,
    type_select: Option<Rc<dyn Fn(usize) -> String>>,
    context: Option<Rc<dyn Fn(usize) -> MenuSpec>>,
    reveal: Option<usize>,
    drag_out: Option<DragOutFn>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Table);

pub fn table<Msg: 'static, E: Into<Element<Msg>>>(
    columns: Vec<Column>,
    rows: usize,
    cell: impl Fn(usize, usize) -> E + 'static,
) -> Table<Msg> {
    Table {
        common: Common::sized(Length::Fill, Length::Fill),
        columns,
        count: rows,
        cell: Rc::new(move |r, c| cell(r, c).into().raw),
        row_height: None,
        selected: Vec::new(),
        on_select: None,
        on_activate: None,
        on_sort: None,
        sort: None,
        multi: false,
        type_select: None,
        context: None,
        reveal: None,
        drag_out: None,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> Table<Msg> {
    /// Let rows be dragged out of the window, as a list's
    /// [`drag_out`](crate::widgets::scroll::List::drag_out).
    pub fn drag_out(mut self, f: impl Fn(&[usize]) -> sidestep_ui::DragSource + 'static) -> Self {
        self.drag_out = Some(Rc::new(f));
        self
    }

    /// Scroll to show row `row` whenever the row given here changes.
    pub fn reveal(mut self, row: usize) -> Self {
        self.reveal = Some(row);
        self
    }

    pub fn selected(mut self, rows: impl IntoIterator<Item = usize>) -> Self {
        self.selected = rows.into_iter().collect();
        self.selected.sort_unstable();
        self
    }

    pub fn on_select(mut self, f: impl Fn(Vec<usize>) -> Msg + 'static) -> Self {
        self.on_select = Some(MsgMap::new(f));
        self
    }

    pub fn on_activate(mut self, f: impl Fn(usize) -> Msg + 'static) -> Self {
        self.on_activate = Some(MsgMap::new(f));
        self
    }

    /// Sent with a column and whether ascending when a sortable header is
    /// clicked: sort in `update` and say so with [`sorted`](Table::sorted).
    pub fn on_sort(mut self, f: impl Fn(usize, bool) -> Msg + 'static) -> Self {
        self.on_sort = Some(MsgMap::new(move |(c, a)| f(c, a)));
        self
    }

    pub fn context_menu(mut self, f: impl Fn(usize) -> Menu<Msg> + 'static) -> Self {
        self.context = Some(Rc::new(move |i| f(i).spec));
        self
    }
}

impl<Msg> Table<Msg> {
    /// The column the rows are sorted by, and whether ascending.
    pub fn sorted(mut self, column: usize, ascending: bool) -> Self {
        self.sort = Some((column, ascending));
        self
    }

    pub fn multiple(mut self) -> Self {
        self.multi = true;
        self
    }

    pub fn row_height(mut self, h: f64) -> Self {
        self.row_height = Some(h);
        self
    }

    pub fn type_select(mut self, f: impl Fn(usize) -> String + 'static) -> Self {
        self.type_select = Some(Rc::new(f));
        self
    }
}

impl<Msg> From<Table<Msg>> for Element<Msg> {
    fn from(t: Table<Msg>) -> Element<Msg> {
        let w = TableWidget {
            columns: t.columns,
            count: t.count,
            cell: t.cell,
            row_height: t.row_height.unwrap_or(0.0),
            selected: t.selected,
            on_select: t.on_select,
            on_activate: t.on_activate,
            on_sort: t.on_sort,
            sort: t.sort,
            multi: t.multi,
            type_select: t.type_select,
            context: t.context,
            reveal: t.reveal,
            drag_out: t.drag_out,
            widths: Vec::new(),
            shown: 0..0,
            size: Size::ZERO,
        };
        Element::from_raw(RawElement::new(w, t.common, Vec::new()))
    }
}

#[derive(Default)]
struct TableState {
    /// Widths the user gave the columns.
    widths: Vec<Option<f64>>,
    /// A column edge being dragged: its column and where it started.
    resizing: Option<(usize, f64, f64)>,
    near_edge: Option<usize>,
}

struct TableWidget {
    columns: Vec<Column>,
    count: usize,
    cell: CellFn,
    row_height: f64,
    selected: Vec<usize>,
    on_select: Option<MsgMap<Vec<usize>>>,
    on_activate: Option<MsgMap<usize>>,
    on_sort: Option<MsgMap<(usize, bool)>>,
    sort: Option<(usize, bool)>,
    multi: bool,
    type_select: Option<Rc<dyn Fn(usize) -> String>>,
    context: Option<Rc<dyn Fn(usize) -> MenuSpec>>,
    drag_out: Option<DragOutFn>,
    widths: Vec<f64>,
    shown: std::ops::Range<usize>,
    size: Size,
    reveal: Option<usize>,
}

const CELL_PAD: f64 = 8.0;

impl TableWidget {
    fn header(&self) -> f64 {
        self.row_height
    }

    fn picker(&self) -> Picker<'_> {
        Picker { count: self.count, selected: &self.selected, multi: self.multi }
    }

    /// The x where column `c` starts, scrolled.
    fn col_x(&self, c: usize, scroll_x: f64) -> f64 {
        self.widths[..c].iter().sum::<f64>() - scroll_x
    }

    fn edge_at(&self, x: f64, scroll_x: f64) -> Option<usize> {
        (0..self.widths.len()).find(|&c| (self.col_x(c + 1, scroll_x) - x).abs() <= 3.0)
    }

    fn column_at(&self, x: f64, scroll_x: f64) -> Option<usize> {
        (0..self.widths.len()).find(|&c| x >= self.col_x(c, scroll_x) && x < self.col_x(c + 1, scroll_x))
    }

    fn row_at(&self, cx: &mut EventCx, y: f64) -> Option<usize> {
        let off = cx.state::<ScrollState>().offset.y;
        let i = ((y - self.header() + off) / self.row_height).floor();
        (y >= self.header() && i >= 0.0 && (i as usize) < self.count).then_some(i as usize)
    }

    fn select(&self, cx: &mut EventCx, rows: Vec<usize>) {
        if let Some(f) = &self.on_select {
            cx.emit_any(f.make(rows));
        }
    }
}

impl Widget for TableWidget {
    fn name(&self) -> &'static str {
        "table"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        if self.row_height <= 0.0 {
            self.row_height = cx.theme().metrics.row;
        }
        let rh = self.row_height;
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 480.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 320.0 },
        ));
        self.size = size;
        let user = cx.state::<TableState>().widths.clone();
        self.widths = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| user.get(i).copied().flatten().unwrap_or(c.width).max(c.min))
            .collect();
        // The last column takes what's left.
        let total: f64 = self.widths.iter().sum();
        if total < size.width
            && let Some(last) = self.widths.last_mut()
        {
            *last += size.width - total;
        }
        let content_w: f64 = self.widths.iter().sum();
        let body_h = size.height - self.header();
        let st = cx.state::<ScrollState>();
        st.viewport = Size::new(size.width, body_h);
        st.content = Size::new(content_w, self.count as f64 * rh);
        st.clamp();
        if let Some(r) = self.reveal {
            st.reveal_once(r as u64, r as f64 * rh, (r + 1) as f64 * rh);
        }
        let (offx, offy) = (st.offset.x, st.offset.y);
        let (moved, bars) = (st.moved_since_laid(), st.bar_tracks());
        if let Some(d) = moved {
            // The header goes sideways with the rows; the rows under it.
            let header = self.header();
            if d.x != 0.0 {
                cx.scrolled(Rect::new(0.0, 0.0, size.width, header), Vec2::new(d.x, 0.0), &bars);
            }
            cx.scrolled(Rect::new(0.0, header, size.width, size.height), d, &bars);
        }
        let first = ((offy / rh).floor().max(0.0) as usize).min(self.count);
        let last = (((offy + body_h) / rh).ceil().max(0.0) as usize).min(self.count);
        self.shown = first..last;
        let selected = self.selected.clone();
        crate::widgets::scroll::selection_damage(cx, &selected, &(first..last), rh, self.header(), offy, size.width);
        let active = cx.selection_active();
        cx.clear_children();
        for r in first..last {
            let sel = active && self.selected.binary_search(&r).is_ok();
            for c in 0..self.columns.len() {
                let mut el = (self.cell)(r, c);
                el.common.key = Some((r as u64) << 16 | c as u64);
                let i = cx.add_child(el);
                let w = (self.widths[c] - 2.0 * CELL_PAD).max(0.0);
                let s = cx.layout_child_selected(i, Constraints::loose(Size::new(w, rh)), sel);
                let x = self.widths[..c].iter().sum::<f64>() - offx + CELL_PAD;
                let x = match self.columns[c].align {
                    Align::End => x + w - s.width,
                    Align::Center => x + ((w - s.width) / 2.0).round(),
                    Align::Start => x,
                };
                cx.place_child(
                    i,
                    Point::new(x, self.header() + r as f64 * rh - offy + ((rh - s.height) / 2.0).round()),
                );
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let w = self.size.width;
        paint_selection(cx, &self.selected, &self.shown, self.row_height, self.header(), w);
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let scroll_x = cx.state::<ScrollState>().map_or(0.0, |s| s.offset.x);
        let near = cx.state::<TableState>().and_then(|s| s.near_edge.or(s.resizing.map(|r| r.0)));
        // The header over the rows.
        let h = Rect::new(0.0, 0.0, self.size.width, self.header());
        cx.canvas.fill_rect(h, t.chrome());
        draw::bottom(cx.canvas, h, t.line(), 1.0);
        let spec = TextSpec::new(FontSpec { weight: 600.0, ..FontSpec::ui(t.metrics.small + 1.0) }, t.text3());
        for (c, col) in self.columns.iter().enumerate() {
            let x0 = self.col_x(c, scroll_x);
            let x1 = self.col_x(c + 1, scroll_x);
            let cell = Rect::new(x0 + CELL_PAD, 0.0, x1 - CELL_PAD, self.header());
            let sorted = self.sort.filter(|(s, _)| *s == c);
            let fg = if sorted.is_some() { t.text() } else { t.text3() };
            let mut right = cell.x1;
            if let Some((_, asc)) = sorted {
                let icon = if asc { Icon::ChevronUp } else { Icon::ChevronDown };
                icon.draw(cx.canvas, Point::new(right - 10.0, (self.header() - 10.0) / 2.0), 10.0, fg);
                right -= 14.0;
            }
            let spec = TextSpec { color: fg, ..spec.clone() };
            cx.draw_text_in(&col.title, &spec, Rect::new(cell.x0, 0.0, right, self.header()), col.align);
            if c + 1 < self.columns.len() {
                let edge = if near == Some(c) { t.focus() } else { t.line() };
                cx.canvas.fill_rect(Rect::new(x1 - 1.0, 4.0, x1, self.header() - 4.0), edge);
            }
        }
        Scroller::paint(cx);
    }

    fn hash(&self, h: &mut PaintHasher) {
        for c in &self.columns {
            h.str(&c.title);
        }
        for w in &self.widths {
            h.f64(*w);
        }
        // Not where it's scrolled to: its rows move as pixels.
        h.add(&(self.sort, self.count));
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
        if !matches!(event, Event::Key(_)) && (Scroller { dir: Direction::Both }).event(cx, event) == Status::Handled {
            return Status::Handled;
        }
        let scroll_x = cx.state::<ScrollState>().offset.x;
        match event {
            Event::Focus(_) | Event::WindowFocus(_) => {
                cx.invalidate();
                Status::Ignored
            }
            Event::PointerMove { pos, .. }
                if self.drag_out.as_ref().is_some_and(|f| drag_moved(cx, *pos, &self.selected, f)) =>
            {
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. }
                if self.drag_out.is_some() && cx.state::<TableState>().resizing.is_none() =>
            {
                if let Some(i) = drag_released(cx) {
                    cx.state::<PickState>().anchor = Some(i);
                    cx.state::<PickState>().cursor = Some(i);
                    self.select(cx, vec![i]);
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                if let Some((c, from, w0)) = cx.state::<TableState>().resizing {
                    let w = (w0 + pos.x - from).max(self.columns[c].min);
                    let st = cx.state::<TableState>();
                    st.widths.resize(self.columns.len(), None);
                    st.widths[c] = Some(w);
                    cx.relayout();
                    cx.invalidate();
                    return Status::Handled;
                }
                let near = (pos.y < self.header())
                    .then(|| self.edge_at(pos.x, scroll_x))
                    .flatten()
                    .filter(|c| c + 1 < self.columns.len());
                if near != cx.state::<TableState>().near_edge {
                    cx.state::<TableState>().near_edge = near;
                    cx.set_cursor(if near.is_some() { Cursor::ColResize } else { Cursor::Default });
                    cx.invalidate_rect(Rect::new(0.0, 0.0, self.size.width, self.header()));
                }
                Status::Ignored
            }
            Event::PointerLeave => {
                if cx.state::<TableState>().near_edge.take().is_some() {
                    cx.set_cursor(Cursor::Default);
                    cx.invalidate_rect(Rect::new(0.0, 0.0, self.size.width, self.header()));
                }
                Status::Ignored
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } if pos.y < self.header() => {
                if let Some(c) = self.edge_at(pos.x, scroll_x).filter(|c| c + 1 < self.columns.len()) {
                    cx.state::<TableState>().resizing = Some((c, pos.x, self.widths[c]));
                    return Status::Handled;
                }
                if let Some(c) = self.column_at(pos.x, scroll_x)
                    && self.columns[c].sortable
                    && let Some(f) = &self.on_sort
                {
                    let asc = match self.sort {
                        Some((s, a)) if s == c => !a,
                        _ => true,
                    };
                    cx.emit_any(f.make((c, asc)));
                }
                Status::Handled
            }
            Event::PointerUp { .. } if cx.state::<TableState>().resizing.is_some() => {
                cx.state::<TableState>().resizing = None;
                cx.invalidate();
                Status::Handled
            }
            Event::PointerDown { pos, button, clicks, .. } => {
                let Some(i) = self.row_at(cx, pos.y) else {
                    if *button == PointerButton::Left && !self.selected.is_empty() {
                        self.select(cx, Vec::new());
                    }
                    return Status::Handled;
                };
                match button {
                    PointerButton::Left => {
                        let mods = cx.modifiers();
                        if *clicks < 2
                            && self.drag_out.is_some()
                            && drag_pressed(cx, *pos, i, &self.selected, !mods.super_key() && !mods.shift())
                        {
                            cx.state::<PickState>().cursor = Some(i);
                            return Status::Handled;
                        }
                        let rows = self.picker().click(cx, i);
                        cx.state::<PickState>().cursor = Some(i);
                        self.select(cx, rows);
                        if *clicks >= 2
                            && let Some(f) = &self.on_activate
                        {
                            cx.emit_any(f.make(i));
                        }
                    }
                    PointerButton::Right => {
                        if !self.selected.contains(&i) {
                            cx.state::<PickState>().anchor = Some(i);
                            self.select(cx, vec![i]);
                        }
                        if let Some(f) = &self.context {
                            let menu = f(i);
                            cx.open_context_menu(menu, *pos);
                        }
                    }
                    _ => return Status::Ignored,
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed && !k.modifiers.super_key() && !k.modifiers.control() => {
                if k.key == Key::Named(NamedKey::Enter) {
                    let cur = cx.state::<PickState>().cursor.or(self.selected.last().copied());
                    if let (Some(f), Some(c)) = (&self.on_activate, cur) {
                        cx.emit_any(f.make(c));
                        return Status::Handled;
                    }
                    return Status::Ignored;
                }
                let page = ((self.size.height - self.header()) / self.row_height).floor().max(1.0) as usize;
                let names = self.type_select.clone();
                let picked = self.picker().key(cx, k, page, names.as_deref());
                let Some((i, extend)) = picked else { return Status::Ignored };
                let rows = self.picker().moved(cx, i, extend);
                self.select(cx, rows);
                show_row(cx, i, self.row_height);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn reveal(&mut self, cx: &mut EventCx, rect: Rect) -> bool {
        Scroller { dir: Direction::Both }.reveal(cx, rect)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Table,
            value: Some(format!("{} rows, {} columns", self.count, self.columns.len())),
            ..Default::default()
        })
    }

    fn selection(&self, _state: StateView) -> Option<String> {
        let f = self.type_select.as_ref()?;
        Some(self.selected.iter().map(|&i| f(i)).collect::<Vec<_>>().join("\n"))
    }
}

// Outlines.

/// An item of an outline: what it shows, and its children (`None` for a
/// leaf; `Some(vec![])` for one whose children aren't loaded yet).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeItem {
    pub id: u64,
    pub label: String,
    pub icon: Option<Icon>,
    /// Quiet text at the right (a size, a count).
    pub detail: Option<String>,
    pub children: Option<Vec<TreeItem>>,
}

impl TreeItem {
    pub fn leaf(id: u64, label: impl Into<String>) -> TreeItem {
        TreeItem { id, label: label.into(), icon: None, detail: None, children: None }
    }

    pub fn branch(id: u64, label: impl Into<String>, children: Vec<TreeItem>) -> TreeItem {
        TreeItem { children: Some(children), ..TreeItem::leaf(id, label) }
    }

    pub fn icon(mut self, i: Icon) -> TreeItem {
        self.icon = Some(i);
        self
    }

    pub fn detail(mut self, d: impl Into<String>) -> TreeItem {
        self.detail = Some(d.into());
        self
    }
}

/// An outline: `tree(self.items.clone()).expanded(&self.open).on_toggle(Msg::Toggle).selected(..).on_select(..)`.
pub struct TreeView<Msg> {
    pub(crate) common: Common,
    roots: Arc<Vec<TreeItem>>,
    expanded: Option<Arc<HashSet<u64>>>,
    on_toggle: Option<MsgMap<(u64, bool)>>,
    selected: Vec<u64>,
    on_select: Option<MsgMap<Vec<u64>>>,
    on_activate: Option<MsgMap<u64>>,
    multi: bool,
    row_height: Option<f64>,
    context: Option<Rc<dyn Fn(u64) -> MenuSpec>>,
    reveal: Option<u64>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(TreeView);

pub fn tree<Msg>(roots: impl Into<Arc<Vec<TreeItem>>>) -> TreeView<Msg> {
    TreeView {
        common: Common::sized(Length::Fill, Length::Fill),
        roots: roots.into(),
        expanded: None,
        on_toggle: None,
        selected: Vec::new(),
        on_select: None,
        on_activate: None,
        multi: false,
        row_height: None,
        context: None,
        reveal: None,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> TreeView<Msg> {
    /// Sent when an item opens (true) or closes: load its children then.
    /// Without [`expanded`](TreeView::expanded), the outline keeps what's
    /// open itself.
    pub fn on_toggle(mut self, f: impl Fn(u64, bool) -> Msg + 'static) -> Self {
        self.on_toggle = Some(MsgMap::new(move |(id, on)| f(id, on)));
        self
    }

    pub fn on_select(mut self, f: impl Fn(Vec<u64>) -> Msg + 'static) -> Self {
        self.on_select = Some(MsgMap::new(f));
        self
    }

    pub fn on_activate(mut self, f: impl Fn(u64) -> Msg + 'static) -> Self {
        self.on_activate = Some(MsgMap::new(f));
        self
    }

    pub fn context_menu(mut self, f: impl Fn(u64) -> Menu<Msg> + 'static) -> Self {
        self.context = Some(Rc::new(move |i| f(i).spec));
        self
    }
}

impl<Msg> TreeView<Msg> {
    /// The items that are open (the program keeps them).
    pub fn expanded(mut self, ids: impl Into<Arc<HashSet<u64>>>) -> Self {
        self.expanded = Some(ids.into());
        self
    }

    pub fn selected(mut self, ids: impl IntoIterator<Item = u64>) -> Self {
        self.selected = ids.into_iter().collect();
        self
    }

    pub fn multiple(mut self) -> Self {
        self.multi = true;
        self
    }

    pub fn row_height(mut self, h: f64) -> Self {
        self.row_height = Some(h);
        self
    }

    /// Scroll to show the item `id` (if its branches are open) whenever
    /// the id given here changes.
    pub fn reveal(mut self, id: u64) -> Self {
        self.reveal = Some(id);
        self
    }
}

impl<Msg> From<TreeView<Msg>> for Element<Msg> {
    fn from(t: TreeView<Msg>) -> Element<Msg> {
        let w = TreeWidget {
            roots: t.roots,
            expanded: t.expanded,
            on_toggle: t.on_toggle,
            selected: t.selected,
            on_select: t.on_select,
            on_activate: t.on_activate,
            multi: t.multi,
            row_height: t.row_height.unwrap_or(0.0),
            context: t.context,
            reveal: t.reveal,
            rows: Vec::new(),
            shape: 0,
            selected_rows: Vec::new(),
            shown: 0..0,
            width: 0.0,
        };
        Element::from_raw(RawElement::new(w, t.common, Vec::new()))
    }
}

/// An outline's own memory of what's open, when the program keeps none.
#[derive(Default)]
struct OpenState {
    open: HashSet<u64>,
}

/// A row as it shows: the item's path in the roots, its depth.
#[derive(Clone)]
struct Row {
    path: Vec<usize>,
    depth: usize,
    id: u64,
    branch: bool,
    open: bool,
}

struct TreeWidget {
    roots: Arc<Vec<TreeItem>>,
    expanded: Option<Arc<HashSet<u64>>>,
    on_toggle: Option<MsgMap<(u64, bool)>>,
    selected: Vec<u64>,
    on_select: Option<MsgMap<Vec<u64>>>,
    on_activate: Option<MsgMap<u64>>,
    multi: bool,
    row_height: f64,
    context: Option<Rc<dyn Fn(u64) -> MenuSpec>>,
    reveal: Option<u64>,
    rows: Vec<Row>,
    /// The rows' ids, depths and openness, hashed.
    shape: u64,
    selected_rows: Vec<usize>,
    shown: std::ops::Range<usize>,
    width: f64,
}

const INDENT: f64 = 16.0;

impl TreeWidget {
    fn item(&self, path: &[usize]) -> &TreeItem {
        let mut items = &*self.roots;
        let mut it = &items[path[0]];
        for &i in &path[1..] {
            items = it.children.as_ref().expect("a branch");
            it = &items[i];
        }
        it
    }

    fn is_open(&self, id: u64, own: &HashSet<u64>) -> bool {
        match &self.expanded {
            Some(e) => e.contains(&id),
            None => own.contains(&id),
        }
    }

    fn flatten(&mut self, own: &HashSet<u64>) {
        let mut rows = Vec::new();
        fn walk(
            w: &TreeWidget,
            items: &[TreeItem],
            path: &mut Vec<usize>,
            depth: usize,
            own: &HashSet<u64>,
            rows: &mut Vec<Row>,
        ) {
            for (i, it) in items.iter().enumerate() {
                path.push(i);
                let open = it.children.is_some() && w.is_open(it.id, own);
                rows.push(Row { path: path.clone(), depth, id: it.id, branch: it.children.is_some(), open });
                if open && let Some(kids) = &it.children {
                    walk(w, kids, path, depth + 1, own, rows);
                }
                path.pop();
            }
        }
        walk(self, &self.roots, &mut Vec::new(), 0, own, &mut rows);
        let mut fx = crate::hash::Fx::default();
        for r in &rows {
            std::hash::Hash::hash(&(r.id, r.open, r.depth), &mut fx);
        }
        self.shape = std::hash::Hasher::finish(&fx);
        self.selected_rows =
            rows.iter().enumerate().filter(|(_, r)| self.selected.contains(&r.id)).map(|(i, _)| i).collect();
        self.rows = rows;
    }

    fn row_at(&self, cx: &mut EventCx, y: f64) -> Option<usize> {
        let off = cx.state::<ScrollState>().offset.y;
        let i = ((y + off) / self.row_height).floor();
        (i >= 0.0 && (i as usize) < self.rows.len()).then_some(i as usize)
    }

    fn toggle(&self, cx: &mut EventCx, row: usize, open: bool) {
        let r = &self.rows[row];
        if !r.branch || r.open == open {
            return;
        }
        if self.expanded.is_none() {
            let st = cx.state::<OpenState>();
            if open {
                st.open.insert(r.id);
            } else {
                st.open.remove(&r.id);
            }
            cx.relayout();
            cx.invalidate();
        }
        if let Some(f) = &self.on_toggle {
            cx.emit_any(f.make((r.id, open)));
        }
    }

    fn select_rows(&self, cx: &mut EventCx, rows: Vec<usize>) {
        let ids: Vec<u64> = rows.iter().filter_map(|&i| self.rows.get(i).map(|r| r.id)).collect();
        if let Some(f) = &self.on_select {
            cx.emit_any(f.make(ids));
        }
    }

    fn picker(&self) -> Picker<'_> {
        Picker { count: self.rows.len(), selected: &self.selected_rows, multi: self.multi }
    }
}

/// An outline row's look: its icon, label and detail.
fn tree_row<Msg>(it: &TreeItem) -> Element<Msg> {
    let mut items: Vec<Element<Msg>> = Vec::new();
    if let Some(i) = it.icon {
        items.push(crate::icons::icon(i).size(16.0).into());
    }
    items.push(text(it.label.clone()).into());
    items.push(crate::widgets::layout::spacer().into());
    if let Some(d) = &it.detail {
        items.push(text(d.clone()).mono().tabular().size(11.0).role(Role::Text3).into());
    }
    crate::widgets::layout::row(items).spacing(8.0).padding([0.0, 10.0, 0.0, 0.0]).into()
}

impl Widget for TreeWidget {
    fn name(&self) -> &'static str {
        "tree"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        if self.row_height <= 0.0 {
            self.row_height = cx.theme().metrics.row;
        }
        let own = cx.state::<OpenState>().open.clone();
        self.flatten(&own);
        let rh = self.row_height;
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 280.0 },
            if bc.max.height.is_finite() { bc.max.height } else { self.rows.len() as f64 * rh },
        ));
        self.width = size.width;
        let st = cx.state::<ScrollState>();
        st.viewport = size;
        st.content = Size::new(size.width, self.rows.len() as f64 * rh);
        st.clamp();
        if let Some(id) = self.reveal
            && let Some(r) = self.rows.iter().position(|r| r.id == id)
        {
            st.reveal_once(id, r as f64 * rh, (r + 1) as f64 * rh);
        }
        let off = st.offset.y;
        let (moved, bars) = (st.moved_since_laid(), st.bar_tracks());
        if let Some(d) = moved {
            cx.scrolled(Rect::from_origin_size(Point::ZERO, size), d, &bars);
        }
        let first = ((off / rh).floor().max(0.0) as usize).min(self.rows.len());
        let last = (((off + size.height) / rh).ceil().max(0.0) as usize).min(self.rows.len());
        self.shown = first..last;
        let selected = self.selected_rows.clone();
        crate::widgets::scroll::selection_damage(cx, &selected, &(first..last), rh, 0.0, off, size.width);
        let active = cx.selection_active();
        cx.clear_children();
        for i in first..last {
            let r = self.rows[i].clone();
            let x = 4.0 + r.depth as f64 * INDENT + 16.0;
            let mut el = tree_row::<()>(self.item(&r.path)).into_raw();
            el.common.key = Some(r.id);
            let c = cx.add_child(el);
            let sel = active && self.selected_rows.contains(&i);
            cx.layout_child_selected(c, Constraints::tight(Size::new((size.width - x).max(0.0), rh)), sel);
            cx.place_child(c, Point::new(x, i as f64 * rh - off));
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        paint_selection(cx, &self.selected_rows, &self.shown, self.row_height, 0.0, self.width);
        let t = cx.theme();
        let off = cx.state::<ScrollState>().map_or(0.0, |s| s.offset.y);
        let focused = cx.is_focused() && cx.window_active();
        for i in self.shown.clone() {
            let r = &self.rows[i];
            if !r.branch {
                continue;
            }
            let y = i as f64 * self.row_height - off;
            let x = 4.0 + r.depth as f64 * INDENT;
            let sel = self.selected_rows.contains(&i);
            let c = if sel && focused { t.on_selection() } else { t.text3() };
            let icon = if r.open { Icon::ChevronDown } else { Icon::ChevronRight };
            icon.draw(cx.canvas, Point::new(x + 2.0, y + (self.row_height - 12.0) / 2.0), 12.0, c);
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        Scroller::paint(cx);
    }

    fn hash(&self, h: &mut PaintHasher) {
        // Its rows' disclosure triangles, all of them (not where it's
        // scrolled to: its rows move as pixels).
        h.add(&self.shape);
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
        if !matches!(event, Event::Key(_))
            && (Scroller { dir: Direction::Vertical }).event(cx, event) == Status::Handled
        {
            return Status::Handled;
        }
        match event {
            Event::Focus(_) | Event::WindowFocus(_) => {
                cx.invalidate();
                Status::Ignored
            }
            Event::PointerDown { pos, button, clicks, .. } => {
                let Some(i) = self.row_at(cx, pos.y) else { return Status::Handled };
                let r = self.rows[i].clone();
                let tri = 4.0 + r.depth as f64 * INDENT;
                if *button == PointerButton::Left && r.branch && pos.x >= tri && pos.x < tri + 16.0 {
                    self.toggle(cx, i, !r.open);
                    return Status::Handled;
                }
                match button {
                    PointerButton::Left => {
                        let rows = self.picker().click(cx, i);
                        cx.state::<PickState>().cursor = Some(i);
                        self.select_rows(cx, rows);
                        if *clicks >= 2 {
                            if let Some(f) = &self.on_activate {
                                cx.emit_any(f.make(r.id));
                            } else if r.branch {
                                self.toggle(cx, i, !r.open);
                            }
                        }
                    }
                    PointerButton::Right => {
                        if !self.selected_rows.contains(&i) {
                            self.select_rows(cx, vec![i]);
                        }
                        if let Some(f) = &self.context {
                            cx.open_context_menu(f(r.id), *pos);
                        }
                    }
                    _ => return Status::Ignored,
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed && !k.modifiers.super_key() && !k.modifiers.control() => {
                let cur = cx.state::<PickState>().cursor.or(self.selected_rows.last().copied());
                match (&k.key, cur) {
                    (Key::Named(NamedKey::ArrowRight), Some(c)) if self.rows[c].branch && !self.rows[c].open => {
                        self.toggle(cx, c, true);
                        return Status::Handled;
                    }
                    (Key::Named(NamedKey::ArrowLeft), Some(c)) => {
                        if self.rows[c].branch && self.rows[c].open {
                            self.toggle(cx, c, false);
                        } else if self.rows[c].depth > 0 {
                            // To the parent.
                            let depth = self.rows[c].depth;
                            if let Some(p) = (0..c).rev().find(|&i| self.rows[i].depth < depth) {
                                let rows = self.picker().moved(cx, p, false);
                                self.select_rows(cx, rows);
                                show_row(cx, p, self.row_height);
                            }
                        }
                        return Status::Handled;
                    }
                    (Key::Named(NamedKey::Enter), Some(c)) => {
                        if let Some(f) = &self.on_activate {
                            cx.emit_any(f.make(self.rows[c].id));
                        } else {
                            let open = !self.rows[c].open;
                            self.toggle(cx, c, open);
                        }
                        return Status::Handled;
                    }
                    _ => {}
                }
                let page = (cx.size().height / self.row_height).floor().max(1.0) as usize;
                let labels: Vec<String> = self.rows.iter().map(|r| self.item(&r.path).label.clone()).collect();
                let names = move |i: usize| labels[i].clone();
                let picked = self.picker().key(cx, k, page, Some(&names));
                let Some((i, extend)) = picked else { return Status::Ignored };
                let rows = self.picker().moved(cx, i, extend);
                self.select_rows(cx, rows);
                show_row(cx, i, self.row_height);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }

    fn reveal(&mut self, cx: &mut EventCx, rect: Rect) -> bool {
        Scroller { dir: Direction::Vertical }.reveal(cx, rect)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::Tree,
            value: Some(format!("{} rows", self.rows.len())),
            ..Default::default()
        })
    }
}
