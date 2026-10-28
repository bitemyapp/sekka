//! Scrolling: `scroll(el)`, and lists that build only the rows that show
//! (`virtual_list`, `list`).
//!
//! Scrolling moves the content without viewing the window again: the
//! scroll view lays itself out anew with its offset and the kit diffs the
//! result, so a frame of scrolling costs the rows that show. Scroll bars
//! are thin, square and drawn over the content; they show while it moves
//! (and while the pointer is on them), and can be dragged. Page Up and
//! Page Down, Home and End, and the arrows scroll too.

use std::marker::PhantomData;
use std::rc::Rc;
use std::time::{Duration, Instant};

use sidestep_ui::{DragSource, Key, NamedKey, PointerButton};

use crate::element::{Common, Element, MsgMap, RawElement};
use crate::hash::PaintHasher;
use crate::layout::{Constraints, Length, Point, Rect, Size, Vec2};
use crate::menu::{Menu, MenuSpec};
use crate::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticAction, SemanticRole, Semantics, Status, Widget,
};

/// How long scroll bars stay after scrolling stops.
const LINGER: Duration = Duration::from_millis(900);
const BAR: f64 = 4.0;
const BAR_WIDE: f64 = 8.0;
const MIN_THUMB: f64 = 24.0;

/// Which ways a view scrolls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Vertical,
    Horizontal,
    Both,
}

impl Direction {
    fn x(self) -> bool {
        self != Direction::Vertical
    }

    fn y(self) -> bool {
        self != Direction::Horizontal
    }
}

/// A scroll view's position, and its bars'.
#[derive(Default, Debug)]
pub(crate) struct ScrollState {
    pub offset: Vec2,
    pub content: Size,
    pub viewport: Size,
    shown_until: Option<Instant>,
    /// A bar being dragged (vertical?) and where in its thumb it was taken.
    drag: Option<(bool, f64)>,
    hover_bar: bool,
    /// What a program last asked to show (`.reveal(..)`).
    revealed: Option<u64>,
    /// The offset the content was last laid out at.
    laid: Option<Vec2>,
    /// The last `scroll_to_end` token the view went to the end for.
    end_token: Option<u64>,
}

impl ScrollState {
    /// How far the content moved since it was last laid out (the offset's
    /// change, the other way), noting this layout's offset.
    pub fn moved_since_laid(&mut self) -> Option<Vec2> {
        let moved = self.laid.map(|l| l - self.offset).filter(|d| *d != Vec2::ZERO);
        self.laid = Some(self.offset);
        moved
    }

    /// Where the bars draw (at their widest), to draw again when the
    /// content under them moves.
    pub fn bar_tracks(&self) -> Vec<Rect> {
        [self.v_bar(true), self.h_bar(true)].into_iter().flatten().map(|(t, _)| t.inflate(2.0, 2.0)).collect()
    }

    /// Scroll so `top..bottom` (down the content) shows, if `what` isn't
    /// what was shown so before.
    pub fn reveal_once(&mut self, what: u64, top: f64, bottom: f64) {
        if self.revealed == Some(what) {
            return;
        }
        self.revealed = Some(what);
        let v = self.viewport.height;
        if top < self.offset.y {
            self.offset.y = top;
        } else if bottom > self.offset.y + v {
            self.offset.y = bottom - v;
        }
        self.clamp();
    }

    fn max(&self) -> Vec2 {
        Vec2::new(
            (self.content.width - self.viewport.width).max(0.0),
            (self.content.height - self.viewport.height).max(0.0),
        )
    }

    pub fn clamp(&mut self) {
        let m = self.max();
        self.offset = Vec2::new(self.offset.x.clamp(0.0, m.x).round(), self.offset.y.clamp(0.0, m.y).round());
    }

    fn visible(&self, now: Instant) -> bool {
        self.drag.is_some() || self.hover_bar || self.shown_until.is_some_and(|t| now < t)
    }

    /// The vertical bar's track and thumb, in the view's coordinates.
    fn v_bar(&self, wide: bool) -> Option<(Rect, Rect)> {
        let (c, v) = (self.content.height, self.viewport.height);
        if c <= v + 0.5 || v <= 0.0 {
            return None;
        }
        let w = if wide { BAR_WIDE } else { BAR };
        let track = Rect::new(self.viewport.width - w - 2.0, 2.0, self.viewport.width - 2.0, v - 2.0);
        let len = (track.height() * v / c).max(MIN_THUMB).min(track.height());
        let y = track.y0 + (track.height() - len) * (self.offset.y / (c - v)).clamp(0.0, 1.0);
        Some((track, Rect::new(track.x0, y.round(), track.x1, (y + len).round())))
    }

    fn h_bar(&self, wide: bool) -> Option<(Rect, Rect)> {
        let (c, v) = (self.content.width, self.viewport.width);
        if c <= v + 0.5 || v <= 0.0 {
            return None;
        }
        let w = if wide { BAR_WIDE } else { BAR };
        let track = Rect::new(2.0, self.viewport.height - w - 2.0, v - 2.0, self.viewport.height - 2.0);
        let len = (track.width() * v / c).max(MIN_THUMB).min(track.width());
        let x = track.x0 + (track.width() - len) * (self.offset.x / (c - v)).clamp(0.0, 1.0);
        Some((track, Rect::new(x.round(), track.y0, (x + len).round(), track.y1)))
    }
}

/// What every scrolling widget does with its [`ScrollState`].
pub(crate) struct Scroller {
    pub dir: Direction,
}

impl Scroller {
    fn state<'c>(cx: &'c mut EventCx) -> &'c mut ScrollState {
        cx.state::<ScrollState>()
    }

    /// Move by `delta`: whether it moved.
    pub fn scroll_by(&self, cx: &mut EventCx, delta: Vec2) -> bool {
        let dir = self.dir;
        let now = cx.now();
        let st = Scroller::state(cx);
        let before = st.offset;
        let d = Vec2::new(if dir.x() { delta.x } else { 0.0 }, if dir.y() { delta.y } else { 0.0 });
        st.offset += d;
        st.clamp();
        if st.offset == before {
            return false;
        }
        st.shown_until = Some(now + LINGER);
        cx.request_tick(LINGER + Duration::from_millis(20));
        // The content moves as pixels (the widget's layout says so); the
        // bars are drawn again.
        cx.relayout();
        self.invalidate_bars(cx);
        true
    }

    pub fn scroll_to(&self, cx: &mut EventCx, offset: Vec2) -> bool {
        let at = Scroller::state(cx).offset;
        self.scroll_by(cx, offset - at)
    }

    /// The scroll's share of an event: wheels, bars, keys. Ignored if it
    /// didn't move (so an outer scroll can).
    pub fn event(&self, cx: &mut EventCx, event: &Event) -> Status {
        let now = cx.now();
        match event {
            Event::Scroll { delta, mods, .. } => {
                // ⇧ turns a wheel sideways.
                let d = if mods.shift() && delta.x == 0.0 && self.dir.x() { Vec2::new(delta.y, 0.0) } else { *delta };
                if self.scroll_by(cx, d) { Status::Handled } else { Status::Ignored }
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                let st = Scroller::state(cx);
                if let Some((track, thumb)) = st.v_bar(true).filter(|(t, _)| t.inflate(2.0, 0.0).contains(*pos)) {
                    let grab = if thumb.contains(*pos) { pos.y - thumb.y0 } else { thumb.height() / 2.0 };
                    st.drag = Some((true, grab));
                    let _ = track;
                    self.drag_to(cx, *pos);
                    return Status::Handled;
                }
                if let Some((_, thumb)) = st.h_bar(true).filter(|(t, _)| t.inflate(0.0, 2.0).contains(*pos)) {
                    let grab = if thumb.contains(*pos) { pos.x - thumb.x0 } else { thumb.width() / 2.0 };
                    st.drag = Some((false, grab));
                    self.drag_to(cx, *pos);
                    return Status::Handled;
                }
                Status::Ignored
            }
            Event::PointerMove { pos, .. } => {
                if Scroller::state(cx).drag.is_some() {
                    self.drag_to(cx, *pos);
                    return Status::Handled;
                }
                let st = Scroller::state(cx);
                let near = st.v_bar(true).is_some_and(|(t, _)| t.inflate(4.0, 0.0).contains(*pos))
                    || st.h_bar(true).is_some_and(|(t, _)| t.inflate(0.0, 4.0).contains(*pos));
                if near != st.hover_bar {
                    st.hover_bar = near;
                    self.invalidate_bars(cx);
                }
                Status::Ignored
            }
            Event::PointerUp { .. } => {
                if Scroller::state(cx).drag.take().is_some() {
                    Scroller::state(cx).shown_until = Some(now + LINGER);
                    cx.request_tick(LINGER + Duration::from_millis(20));
                    self.invalidate_bars(cx);
                    return Status::Handled;
                }
                Status::Ignored
            }
            Event::PointerLeave => {
                let st = Scroller::state(cx);
                if st.hover_bar {
                    st.hover_bar = false;
                    self.invalidate_bars(cx);
                }
                Status::Ignored
            }
            Event::Tick => {
                // The bars go once scrolling has stopped a while.
                if !Scroller::state(cx).visible(now) {
                    self.invalidate_bars(cx);
                }
                Status::Ignored
            }
            // Keys scroll a view that has the focus (or anything under the
            // pointer while nothing has it), not one around a control.
            Event::Key(k)
                if k.pressed
                    && !k.modifiers.super_key()
                    && !k.modifiers.control()
                    && (cx.is_focused() || cx.nothing_focused()) =>
            {
                let v = Scroller::state(cx).viewport;
                let page = (v.height - 40.0).max(40.0);
                let d = match &k.key {
                    Key::Named(NamedKey::PageDown) => Vec2::new(0.0, page),
                    Key::Named(NamedKey::PageUp) => Vec2::new(0.0, -page),
                    Key::Character(s) if s == " " && k.modifiers.shift() => Vec2::new(0.0, -page),
                    Key::Character(s) if s == " " => Vec2::new(0.0, page),
                    Key::Named(NamedKey::Home) => Vec2::new(0.0, -1e9),
                    Key::Named(NamedKey::End) => Vec2::new(0.0, 1e9),
                    Key::Named(NamedKey::ArrowDown) => Vec2::new(0.0, 40.0),
                    Key::Named(NamedKey::ArrowUp) => Vec2::new(0.0, -40.0),
                    Key::Named(NamedKey::ArrowRight) if self.dir.x() => Vec2::new(40.0, 0.0),
                    Key::Named(NamedKey::ArrowLeft) if self.dir.x() => Vec2::new(-40.0, 0.0),
                    _ => return Status::Ignored,
                };
                if self.scroll_by(cx, d) { Status::Handled } else { Status::Ignored }
            }
            _ => Status::Ignored,
        }
    }

    fn drag_to(&self, cx: &mut EventCx, pos: Point) {
        let st = Scroller::state(cx);
        let Some((vertical, grab)) = st.drag else { return };
        let target = if vertical {
            let Some((track, thumb)) = st.v_bar(true) else { return };
            let room = (track.height() - thumb.height()).max(1.0);
            let t = ((pos.y - grab - track.y0) / room).clamp(0.0, 1.0);
            Vec2::new(st.offset.x, t * (st.content.height - st.viewport.height))
        } else {
            let Some((track, thumb)) = st.h_bar(true) else { return };
            let room = (track.width() - thumb.width()).max(1.0);
            let t = ((pos.x - grab - track.x0) / room).clamp(0.0, 1.0);
            Vec2::new(t * (st.content.width - st.viewport.width), st.offset.y)
        };
        self.scroll_to(cx, target);
    }

    fn invalidate_bars(&self, cx: &mut EventCx) {
        let st = Scroller::state(cx);
        let mut rects = Vec::new();
        if let Some((t, _)) = st.v_bar(true) {
            rects.push(t.inflate(2.0, 2.0));
        }
        if let Some((t, _)) = st.h_bar(true) {
            rects.push(t.inflate(2.0, 2.0));
        }
        for r in rects {
            cx.invalidate_rect(r);
        }
    }

    /// Show `rect` (window coordinates; the view is at `view`): whether it
    /// had to move.
    pub fn reveal(&self, cx: &mut EventCx, rect: Rect) -> bool {
        let view = cx.window_rect();
        let mut d = Vec2::ZERO;
        if self.dir.y() {
            if rect.y0 < view.y0 {
                d.y = rect.y0 - view.y0 - 8.0;
            } else if rect.y1 > view.y1 {
                d.y = (rect.y1 - view.y1 + 8.0).min(rect.y0 - view.y0);
            }
        }
        if self.dir.x() {
            if rect.x0 < view.x0 {
                d.x = rect.x0 - view.x0 - 8.0;
            } else if rect.x1 > view.x1 {
                d.x = (rect.x1 - view.x1 + 8.0).min(rect.x0 - view.x0);
            }
        }
        d != Vec2::ZERO && self.scroll_by(cx, d)
    }

    /// Draw the bars, if they're showing.
    pub fn paint(cx: &mut PaintCx) {
        let Some(st) = cx.state::<ScrollState>() else { return };
        if !st.visible(cx.now()) {
            return;
        }
        let t = cx.theme();
        let wide = st.hover_bar || st.drag.is_some();
        let thumb_color = if wide { t.text3() } else { t.line2().mix(t.text3(), 0.35) };
        for bar in [st.v_bar(wide), st.h_bar(wide)].into_iter().flatten() {
            let (track, thumb) = bar;
            if wide {
                cx.canvas.fill_rect(track, t.sunk().with_alpha(0.7));
            }
            cx.canvas.fill_rect(thumb, thumb_color);
        }
    }
}

// The `scroll` element.

/// Content taller (or wider) than its room: `scroll(column![...])`.
/// `.height(Shrink)` keeps short content compact and scrolls once it
/// reaches the height its parent allows.
pub struct Scroll<Msg> {
    pub(crate) common: Common,
    dir: Direction,
    child: RawElement,
    follow_end: bool,
    end_token: Option<u64>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Scroll);

pub fn scroll<Msg>(child: impl Into<Element<Msg>>) -> Scroll<Msg> {
    Scroll {
        common: Common::sized(Length::Fill, Length::Fill),
        dir: Direction::Vertical,
        child: child.into().raw,
        follow_end: false,
        end_token: None,
        _m: PhantomData,
    }
}

impl<Msg> Scroll<Msg> {
    pub fn direction(mut self, dir: Direction) -> Self {
        self.dir = dir;
        self
    }

    pub fn horizontal(self) -> Self {
        self.direction(Direction::Horizontal)
    }

    pub fn both(self) -> Self {
        self.direction(Direction::Both)
    }

    /// Keep the end in view as the content grows (a log, a conversation
    /// being written): it opens at its end, and while it shows its end it
    /// stays there. Scrolled away from the end, it stays where the reader
    /// left it until they come back to the end.
    pub fn follow_end(mut self) -> Self {
        self.follow_end = true;
        self
    }

    /// Go to the end whenever `token` changes (a message just sent): the
    /// view follows its end again from there.
    pub fn scroll_to_end(mut self, token: u64) -> Self {
        self.end_token = Some(token);
        self
    }
}

impl<Msg> From<Scroll<Msg>> for Element<Msg> {
    fn from(s: Scroll<Msg>) -> Element<Msg> {
        Element::from_raw(RawElement {
            widget: Box::new(ScrollWidget {
                dir: s.dir,
                shrink_height: s.common.height == Length::Shrink,
                follow_end: s.follow_end,
                end_token: s.end_token,
            }),
            common: s.common,
            children: vec![s.child],
        })
    }
}

struct ScrollWidget {
    dir: Direction,
    shrink_height: bool,
    follow_end: bool,
    end_token: Option<u64>,
}

impl Widget for ScrollWidget {
    fn name(&self) -> &'static str {
        "scroll"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let dir = self.dir;
        let unbounded = |v: f64| if v.is_finite() { v } else { 0.0 };
        let view = Size::new(unbounded(bc.max.width), unbounded(bc.max.height));
        let child_bc = Constraints {
            // A shrinking view follows short content instead of stretching it.
            min: Size::new(view.width, if self.shrink_height { bc.min.height } else { view.height }),
            // Across an axis it doesn't scroll, the content has the room the
            // view has, or all it wants where that is unbounded (a sideways
            // scroll in a column that grows: code in a document).
            max: Size::new(
                if dir.x() || !bc.max.width.is_finite() { f64::INFINITY } else { view.width },
                if dir.y() || !bc.max.height.is_finite() { f64::INFINITY } else { view.height },
            ),
        };
        let content = if cx.child_count() > 0 { cx.layout_child(0, child_bc) } else { Size::ZERO };
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { view.width } else { content.width },
            if bc.max.height.is_finite() && !self.shrink_height { view.height } else { content.height },
        ));
        let (follow_end, end_token) = (self.follow_end, self.end_token);
        let st = cx.state::<ScrollState>();
        // At the end before this layout (or never laid out): stay there.
        let was_at_end = st.laid.is_none() || st.offset.y >= st.max().y - 1.0;
        let asked = end_token.is_some() && st.end_token != end_token;
        if asked {
            st.end_token = end_token;
        }
        st.viewport = size;
        st.content = content;
        if (follow_end && was_at_end) || asked {
            st.offset.y = st.max().y;
        }
        st.clamp();
        let offset = st.offset;
        let moved = st.moved_since_laid();
        let bars = st.bar_tracks();
        if cx.child_count() > 0 {
            cx.place_child(0, Point::new(-offset.x, -offset.y));
        }
        if let Some(d) = moved {
            cx.scrolled(Rect::from_origin_size(Point::ZERO, size), d, &bars);
        }
        size
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        Scroller::paint(cx);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&(self.dir, self.shrink_height));
    }

    fn clips(&self) -> bool {
        true
    }

    fn solid(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        Focusable::Pointer
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        Scroller { dir: self.dir }.event(cx, event)
    }

    fn reveal(&mut self, cx: &mut EventCx, rect: Rect) -> bool {
        Scroller { dir: self.dir }.reveal(cx, rect)
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics {
            role: SemanticRole::ScrollView,
            actions: vec![SemanticAction::ScrollIntoView],
            ..Default::default()
        })
    }
}

// Lists.

type Build = Rc<dyn Fn(usize) -> RawElement>;

enum Rows {
    /// Made as they show.
    Virtual { count: usize, build: Build },
    /// All there (mounted as children).
    Fixed(usize),
}

/// Rows to choose from, of one height, scrolled; only those that show are
/// built: `virtual_list(files.len(), 24.0, move |i| text(&files[i].name).into())`.
pub struct List<Msg> {
    pub(crate) common: Common,
    rows: Rows,
    children: Vec<RawElement>,
    row_height: Option<f64>,
    selected: Vec<usize>,
    on_select: Option<MsgMap<Vec<usize>>>,
    on_activate: Option<MsgMap<usize>>,
    multi: bool,
    type_select: Option<Rc<dyn Fn(usize) -> String>>,
    context: Option<Rc<dyn Fn(usize) -> MenuSpec>>,
    inset: f64,
    reveal: Option<usize>,
    drag_out: Option<DragOutFn>,
    _m: PhantomData<fn() -> Msg>,
}

/// What a list's or table's rows carry when dragged out of the window.
pub(crate) type DragOutFn = Rc<dyn Fn(&[usize]) -> DragSource>;

/// How far the pointer goes, pressed on a row, before it drags.
const DRAG_SLOP: f64 = 4.0;

/// A press on rows that may become a drag out.
#[derive(Default)]
struct DragPress {
    /// Where, and on which row.
    at: Option<(Point, usize)>,
    /// The row to select alone if the press ends without a drag: it fell
    /// on a selection of several, which a drag would take whole.
    collapse: Option<usize>,
}

/// A press on row `i` at `pos`, noted for a drag out: whether to hold off
/// selecting it (a plain press on a selection of several).
pub(crate) fn drag_pressed(cx: &mut EventCx, pos: Point, i: usize, selected: &[usize], plain: bool) -> bool {
    let defer = plain && selected.len() > 1 && selected.contains(&i);
    let st = cx.state::<DragPress>();
    st.at = Some((pos, i));
    st.collapse = defer.then_some(i);
    defer
}

/// The pointer moved with the button down: once it's gone far enough
/// from the press, the rows (the selection, if the press was on it) go
/// out as `f` says. Whether it started.
pub(crate) fn drag_moved(cx: &mut EventCx, pos: Point, selected: &[usize], f: &DragOutFn) -> bool {
    let st = cx.state::<DragPress>();
    let Some((at, i)) = st.at else { return false };
    if (pos - at).hypot() < DRAG_SLOP {
        return false;
    }
    st.at = None;
    st.collapse = None;
    let rows: Vec<usize> = if selected.contains(&i) { selected.to_vec() } else { vec![i] };
    cx.start_drag(f(&rows));
    true
}

/// The button came up: the row to select alone, if the press held off.
pub(crate) fn drag_released(cx: &mut EventCx) -> Option<usize> {
    let st = cx.state::<DragPress>();
    st.at = None;
    st.collapse.take()
}

crate::builder!(List);

/// A list of `count` rows `row_height` tall, built by `row` as they show.
pub fn virtual_list<Msg: 'static, E: Into<Element<Msg>>>(
    count: usize,
    row_height: f64,
    row: impl Fn(usize) -> E + 'static,
) -> List<Msg> {
    let build: Build = Rc::new(move |i| row(i).into().raw);
    List {
        common: Common::sized(Length::Fill, Length::Fill),
        rows: Rows::Virtual { count, build },
        children: Vec::new(),
        row_height: Some(row_height),
        selected: Vec::new(),
        on_select: None,
        on_activate: None,
        multi: false,
        type_select: None,
        context: None,
        inset: 0.0,
        reveal: None,
        drag_out: None,
        _m: PhantomData,
    }
}

/// A list of these rows (each the theme's row height).
pub fn list<Msg>(items: Vec<Element<Msg>>) -> List<Msg> {
    let n = items.len();
    List {
        common: Common::sized(Length::Fill, Length::Fill),
        rows: Rows::Fixed(n),
        children: items.into_iter().map(|e| e.raw).collect(),
        row_height: None,
        selected: Vec::new(),
        on_select: None,
        on_activate: None,
        multi: false,
        type_select: None,
        context: None,
        inset: 0.0,
        reveal: None,
        drag_out: None,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static> List<Msg> {
    /// The rows selected (the program keeps them).
    pub fn selected(mut self, rows: impl IntoIterator<Item = usize>) -> Self {
        self.selected = rows.into_iter().collect();
        self.selected.sort_unstable();
        self
    }

    /// Sent with the new selection when the user changes it.
    pub fn on_select(mut self, f: impl Fn(Vec<usize>) -> Msg + 'static) -> Self {
        self.on_select = Some(MsgMap::new(f));
        self
    }

    /// Sent on a double click, or Return.
    pub fn on_activate(mut self, f: impl Fn(usize) -> Msg + 'static) -> Self {
        self.on_activate = Some(MsgMap::new(f));
        self
    }

    /// The menu a right click on a row opens.
    pub fn context_menu(mut self, f: impl Fn(usize) -> Menu<Msg> + 'static) -> Self {
        self.context = Some(Rc::new(move |i| f(i).spec));
        self
    }
}

impl<Msg> List<Msg> {
    /// Several rows can be selected, with ⇧ and ❖ clicks.
    pub fn multiple(mut self) -> Self {
        self.multi = true;
        self
    }

    /// Typing a row's start selects it: `|i| names[i].clone()`.
    pub fn type_select(mut self, f: impl Fn(usize) -> String + 'static) -> Self {
        self.type_select = Some(Rc::new(f));
        self
    }

    /// Room left and right of the rows' selection fills.
    /// Scroll to show row `row` whenever the row given here changes (a
    /// search's match, a file just made). The user's scrolling is theirs
    /// until then.
    pub fn reveal(mut self, row: usize) -> Self {
        self.reveal = Some(row);
        self
    }

    /// Let rows be dragged out of the window, to other programs, the
    /// desktop, the Shelf: pressed on a row and moved a few points, the
    /// selection (or the row, if it isn't selected) goes as `f` makes it,
    /// e.g. `DragSource::new().urls(..)` for files. A press on a
    /// selection of several selects its row alone on release, if it
    /// didn't drag.
    pub fn drag_out(mut self, f: impl Fn(&[usize]) -> DragSource + 'static) -> Self {
        self.drag_out = Some(Rc::new(f));
        self
    }

    pub fn inset(mut self, points: f64) -> Self {
        self.inset = points;
        self
    }

    pub fn row_height(mut self, h: f64) -> Self {
        self.row_height = Some(h);
        self
    }
}

impl<Msg> From<List<Msg>> for Element<Msg> {
    fn from(l: List<Msg>) -> Element<Msg> {
        let w = ListWidget {
            rows: l.rows,
            row_height: l.row_height.unwrap_or(0.0),
            selected: l.selected,
            on_select: l.on_select,
            on_activate: l.on_activate,
            multi: l.multi,
            type_select: l.type_select,
            context: l.context,
            inset: l.inset,
            reveal: l.reveal,
            drag_out: l.drag_out,
            shown: 0..0,
            width: 0.0,
        };
        Element::from_raw(RawElement { widget: Box::new(w), common: l.common, children: l.children })
    }
}

/// A list's own state, besides its scroll.
#[derive(Default)]
struct ListState {
    anchor: Option<usize>,
    cursor: Option<usize>,
    typed: String,
    typed_at: Option<Instant>,
}

struct ListWidget {
    rows: Rows,
    row_height: f64,
    selected: Vec<usize>,
    on_select: Option<MsgMap<Vec<usize>>>,
    on_activate: Option<MsgMap<usize>>,
    multi: bool,
    type_select: Option<Rc<dyn Fn(usize) -> String>>,
    context: Option<Rc<dyn Fn(usize) -> MenuSpec>>,
    inset: f64,
    reveal: Option<usize>,
    drag_out: Option<DragOutFn>,
    shown: std::ops::Range<usize>,
    width: f64,
}

impl ListWidget {
    fn count(&self) -> usize {
        match &self.rows {
            Rows::Virtual { count, .. } => *count,
            Rows::Fixed(n) => *n,
        }
    }

    fn is_selected(&self, i: usize) -> bool {
        self.selected.binary_search(&i).is_ok()
    }

    fn row_at(&self, cx: &mut EventCx, y: f64) -> Option<usize> {
        let off = cx.state::<ScrollState>().offset.y;
        let i = ((y + off) / self.row_height).floor();
        (i >= 0.0 && (i as usize) < self.count()).then_some(i as usize)
    }

    fn select(&self, cx: &mut EventCx, rows: Vec<usize>, cursor: usize) {
        let st = cx.state::<ListState>();
        st.cursor = Some(cursor);
        if let Some(f) = &self.on_select {
            cx.emit_any(f.make(rows));
        }
    }

    /// Move the cursor to `i` from the keyboard, extending with ⇧.
    fn move_to(&self, cx: &mut EventCx, i: usize, extend: bool) {
        let n = self.count();
        if n == 0 {
            return;
        }
        let i = i.min(n - 1);
        let rows = if extend && self.multi {
            let anchor = cx.state::<ListState>().anchor.unwrap_or(i);
            (anchor.min(i)..=anchor.max(i)).collect()
        } else {
            cx.state::<ListState>().anchor = Some(i);
            vec![i]
        };
        self.select(cx, rows, i);
        self.show_row(cx, i);
    }

    /// Scroll so row `i` shows.
    fn show_row(&self, cx: &mut EventCx, i: usize) {
        let st = cx.state::<ScrollState>();
        let (top, bottom) = (i as f64 * self.row_height, (i + 1) as f64 * self.row_height);
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
            Scroller { dir: Direction::Vertical }.scroll_to(cx, Vec2::new(x, y));
        }
    }

    fn page(&self, cx: &mut EventCx) -> usize {
        ((cx.state::<ScrollState>().viewport.height / self.row_height).floor() as usize).max(1)
    }
}

impl Widget for ListWidget {
    fn name(&self) -> &'static str {
        "list"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        if self.row_height <= 0.0 {
            self.row_height = cx.theme().metrics.row;
        }
        let rh = self.row_height;
        let n = self.count();
        let content_h = n as f64 * rh;
        let width = if bc.max.width.is_finite() { bc.max.width } else { 240.0 };
        let height = if bc.max.height.is_finite() { bc.max.height } else { content_h };
        let size = bc.constrain(Size::new(width, height));
        let st = cx.state::<ScrollState>();
        st.viewport = size;
        st.content = Size::new(width, content_h);
        st.clamp();
        if let Some(r) = self.reveal {
            st.reveal_once(r as u64, r as f64 * rh, (r + 1) as f64 * rh);
        }
        let off = st.offset.y;
        let (moved, bars) = (st.moved_since_laid(), st.bar_tracks());
        if let Some(d) = moved {
            cx.scrolled(Rect::from_origin_size(Point::ZERO, size), d, &bars);
        }
        let first = ((off / rh).floor().max(0.0) as usize).min(n);
        let last = (((off + size.height) / rh).ceil().max(0.0) as usize).min(n);
        self.shown = first..last;
        self.width = width;
        let selected = self.selected.clone();
        selection_damage(cx, &selected, &(first..last), rh, 0.0, off, width);
        let active = cx.selection_active();
        let row_bc = Constraints::tight(Size::new(width, rh));
        match &self.rows {
            Rows::Virtual { build, .. } => {
                let build = build.clone();
                cx.clear_children();
                for i in first..last {
                    let mut el = build(i);
                    el.common.key = Some(i as u64);
                    let c = cx.add_child(el);
                    let sel = active && self.selected.binary_search(&i).is_ok();
                    cx.layout_child_selected(c, row_bc, sel);
                    cx.place_child(c, Point::new(0.0, i as f64 * rh - off));
                }
            }
            Rows::Fixed(_) => {
                for i in 0..cx.child_count() {
                    let sel = active && self.selected.binary_search(&i).is_ok();
                    cx.layout_child_selected(i, row_bc, sel);
                    cx.place_child(i, Point::new(0.0, i as f64 * rh - off));
                }
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let off = cx.state::<ScrollState>().map_or(0.0, |s| s.offset.y);
        let active = cx.is_focused() && cx.window_active();
        let fill = if active { t.selection() } else { t.line2() };
        for &i in &self.selected {
            if !self.shown.contains(&i) {
                continue;
            }
            let y = i as f64 * self.row_height - off;
            cx.canvas.fill_rect(Rect::new(self.inset, y, self.width - self.inset, y + self.row_height), fill);
        }
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        Scroller::paint(cx);
    }

    fn hash(&self, h: &mut PaintHasher) {
        // Not where it's scrolled to: its rows move as pixels.
        h.add(&self.count()).f64(self.row_height).f64(self.inset);
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
        // The list moves its selection with the keys; the scroller has the
        // rest.
        let scroller = Scroller { dir: Direction::Vertical };
        if !matches!(event, Event::Key(_)) && scroller.event(cx, event) == Status::Handled {
            return Status::Handled;
        }
        match event {
            Event::PointerMove { pos, .. }
                if self.drag_out.as_ref().is_some_and(|f| drag_moved(cx, *pos, &self.selected, f)) =>
            {
                Status::Handled
            }
            Event::PointerUp { button: PointerButton::Left, .. } if self.drag_out.is_some() => {
                if let Some(i) = drag_released(cx) {
                    cx.state::<ListState>().anchor = Some(i);
                    self.select(cx, vec![i], i);
                }
                Status::Handled
            }
            Event::Focus(_) | Event::WindowFocus(_) => {
                cx.invalidate();
                Status::Ignored
            }
            Event::PointerDown { pos, button, clicks, mods } => {
                let Some(i) = self.row_at(cx, pos.y) else {
                    if *button == PointerButton::Left && !self.selected.is_empty() {
                        self.select(cx, Vec::new(), 0);
                    }
                    return Status::Handled;
                };
                match button {
                    PointerButton::Left => {
                        if *clicks >= 2 && self.is_selected(i) {
                            if let Some(f) = &self.on_activate {
                                cx.emit_any(f.make(i));
                            }
                            return Status::Handled;
                        }
                        if self.drag_out.is_some()
                            && drag_pressed(cx, *pos, i, &self.selected, !mods.super_key() && !mods.shift())
                        {
                            return Status::Handled;
                        }
                        let rows = if self.multi && mods.super_key() {
                            let mut s = self.selected.clone();
                            match s.binary_search(&i) {
                                Ok(p) => {
                                    s.remove(p);
                                }
                                Err(p) => s.insert(p, i),
                            }
                            cx.state::<ListState>().anchor = Some(i);
                            s
                        } else if self.multi && mods.shift() {
                            let a = cx.state::<ListState>().anchor.unwrap_or(i);
                            (a.min(i)..=a.max(i)).collect()
                        } else {
                            cx.state::<ListState>().anchor = Some(i);
                            vec![i]
                        };
                        self.select(cx, rows, i);
                        if *clicks >= 2
                            && let Some(f) = &self.on_activate
                        {
                            cx.emit_any(f.make(i));
                        }
                    }
                    PointerButton::Right => {
                        if !self.is_selected(i) {
                            cx.state::<ListState>().anchor = Some(i);
                            self.select(cx, vec![i], i);
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
                let n = self.count();
                if n == 0 {
                    return Status::Ignored;
                }
                let cur = cx.state::<ListState>().cursor.or(self.selected.last().copied());
                let shift = k.modifiers.shift();
                let page = self.page(cx);
                match &k.key {
                    Key::Named(NamedKey::ArrowDown) => self.move_to(cx, cur.map_or(0, |c| (c + 1).min(n - 1)), shift),
                    Key::Named(NamedKey::ArrowUp) => self.move_to(cx, cur.map_or(0, |c| c.saturating_sub(1)), shift),
                    Key::Named(NamedKey::Home) => self.move_to(cx, 0, shift),
                    Key::Named(NamedKey::End) => self.move_to(cx, n - 1, shift),
                    Key::Named(NamedKey::PageDown) => self.move_to(cx, cur.map_or(0, |c| c + page), shift),
                    Key::Named(NamedKey::PageUp) => self.move_to(cx, cur.map_or(0, |c| c.saturating_sub(page)), shift),
                    Key::Named(NamedKey::Enter) => match (&self.on_activate, cur) {
                        (Some(f), Some(c)) => cx.emit_any(f.make(c)),
                        _ => return Status::Ignored,
                    },
                    Key::Character(_) if k.text.is_some() && self.type_select.is_some() => {
                        let typed = k.text.clone().unwrap_or_default().to_lowercase();
                        let now = cx.now();
                        let st = cx.state::<ListState>();
                        if st.typed_at.is_none_or(|t| now.duration_since(t) > Duration::from_millis(1000)) {
                            st.typed.clear();
                        }
                        st.typed.push_str(&typed);
                        st.typed_at = Some(now);
                        let prefix = st.typed.clone();
                        let f = self.type_select.clone().expect("type-select");
                        let start = cur.unwrap_or(0);
                        // From the cursor on (a new first letter moves on).
                        let from = if prefix.chars().count() == 1 { start + 1 } else { start };
                        let found = (0..n).map(|k| (from + k) % n).find(|&i| f(i).to_lowercase().starts_with(&prefix));
                        if let Some(i) = found {
                            self.move_to(cx, i, false);
                        }
                    }
                    _ => return Status::Ignored,
                }
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
            role: SemanticRole::List,
            value: Some(format!("{} rows, {} selected", self.count(), self.selected.len())),
            ..Default::default()
        })
    }

    fn selection(&self, _state: crate::widget::StateView) -> Option<String> {
        let f = self.type_select.as_ref()?;
        Some(self.selected.iter().map(|&i| f(i)).collect::<Vec<_>>().join("\n"))
    }
}

/// The rows a row-list painted selected last, to redraw only the rows
/// whose selection changed (the list's own hash leaves its selection out).
#[derive(Default)]
pub(crate) struct DrawnSelection(pub Vec<usize>);

/// Redraw the rows (of height `rh`, from `top`, scrolled by `off`)
/// whose selection changed since the list last drew.
pub(crate) fn selection_damage(
    cx: &mut LayoutCx,
    now: &[usize],
    shown: &std::ops::Range<usize>,
    rh: f64,
    top: f64,
    off: f64,
    width: f64,
) {
    let before = std::mem::replace(&mut cx.state::<DrawnSelection>().0, now.to_vec());
    for &i in before.iter().filter(|i| !now.contains(i)).chain(now.iter().filter(|i| !before.contains(i))) {
        if shown.contains(&i) {
            let y = top + i as f64 * rh - off;
            cx.invalidate_rect(Rect::new(0.0, y, width, y + rh));
        }
    }
}
