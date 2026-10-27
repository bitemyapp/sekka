//! Interaction wrappers: `popover`, `context_menu`, `tooltip`,
//! `mouse_area`, and `canvas` for drawing of one's own.

use std::hash::Hash;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use sidestep_ui::{Canvas, Cursor, PointerButton};

use crate::element::{Common, Element, MsgFn, MsgMap, RawElement};
use crate::hash::{PaintHasher, hash_of};
use crate::layout::{Constraints, Length, Point, Rect, Size, Vec2};
use crate::menu::{Menu, MenuSpec};
use crate::theme::Theme;
use crate::widget::{
    Event, EventCx, Focusable, LayoutCx, PaintCx, PopoverContent, SemanticRole, Semantics, Status, Widget,
};

/// Lay out a single child filling what it's given (or its own size).
fn wrap_layout(cx: &mut LayoutCx, bc: Constraints) -> Size {
    if cx.child_count() == 0 {
        return bc.min;
    }
    let s = cx.layout_child(0, bc);
    cx.place_child(0, Point::ZERO);
    bc.constrain(s)
}

// Popovers.

/// Content floating under an anchor while `open`, in a popup of its own
/// (raised, the hard shadow): `popover(button("Wi-Fi").on_press(Msg::Panel), panel(), self.panel_open)`.
pub struct Popover<Msg> {
    pub(crate) common: Common,
    anchor: RawElement,
    content: RawElement,
    open: bool,
    on_dismiss: Option<MsgFn>,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(Popover);

pub fn popover<Msg>(anchor: impl Into<Element<Msg>>, content: impl Into<Element<Msg>>, open: bool) -> Popover<Msg> {
    Popover {
        common: Common::default(),
        anchor: anchor.into().raw,
        content: content.into().raw,
        open,
        on_dismiss: None,
        _m: PhantomData,
    }
}

impl<Msg: Clone + Send + 'static> Popover<Msg> {
    /// Sent when it's dismissed (a click elsewhere, Escape): close it.
    pub fn on_dismiss(mut self, msg: Msg) -> Self {
        self.on_dismiss = Some(MsgFn::new(msg));
        self
    }
}

impl<Msg> From<Popover<Msg>> for Element<Msg> {
    fn from(p: Popover<Msg>) -> Element<Msg> {
        let w = PopoverWidget {
            content: Some(PopoverContent { content: p.content, open: p.open, on_dismiss: p.on_dismiss }),
        };
        Element::from_raw(RawElement { widget: Box::new(w), common: p.common, children: vec![p.anchor] })
    }
}

struct PopoverWidget {
    content: Option<PopoverContent>,
}

impl Widget for PopoverWidget {
    fn name(&self) -> &'static str {
        "popover"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        wrap_layout(cx, bc)
    }

    fn take_popover(&mut self) -> Option<PopoverContent> {
        self.content.take()
    }
}

// Context menus and tooltips.

/// `el`, with `menu` on a right click (or ⌃-click).
pub struct ContextMenu<Msg> {
    pub(crate) common: Common,
    child: RawElement,
    menu: MenuSpec,
    _m: PhantomData<fn() -> Msg>,
}

crate::builder!(ContextMenu);

pub fn context_menu<Msg>(child: impl Into<Element<Msg>>, menu: Menu<Msg>) -> ContextMenu<Msg> {
    ContextMenu { common: Common::default(), child: child.into().raw, menu: menu.spec, _m: PhantomData }
}

impl<Msg> From<ContextMenu<Msg>> for Element<Msg> {
    fn from(c: ContextMenu<Msg>) -> Element<Msg> {
        let mut common = c.common;
        // The wrapper takes its child's size.
        common.width = c.child.common.width;
        common.height = c.child.common.height;
        Element::from_raw(RawElement {
            widget: Box::new(ContextWidget { menu: c.menu }),
            common,
            children: vec![c.child],
        })
    }
}

struct ContextWidget {
    menu: MenuSpec,
}

impl Widget for ContextWidget {
    fn name(&self) -> &'static str {
        "context-menu"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        wrap_layout(cx, bc)
    }

    fn solid(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Right, .. } => {
                cx.open_context_menu(self.menu.clone(), *pos);
                Status::Handled
            }
            Event::PointerDown { pos, button: PointerButton::Left, mods, .. }
                if mods.control() && !mods.super_key() =>
            {
                cx.open_context_menu(self.menu.clone(), *pos);
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }
}

/// `el` with a tooltip: the same as `.tooltip(text)` on it.
pub fn tooltip<Msg>(child: impl Into<Element<Msg>>, text: impl Into<Arc<str>>) -> Element<Msg> {
    let mut el = child.into();
    el.raw.common.tooltip = Some(text.into());
    el
}

// Mouse areas.

/// A drag under way in a mouse area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragEvent {
    /// The button went down here.
    Start(Point),
    /// The pointer moved from the start, by `delta` since the last move.
    Move { start: Point, at: Point, delta: Vec2 },
    /// The button came up here.
    End { start: Point, at: Point },
}

/// What was dropped on a mouse area.
#[derive(Clone, Debug, PartialEq)]
pub struct Dropped {
    pub at: Point,
    /// Files and links (`file://…`).
    pub urls: Vec<String>,
    pub text: Option<String>,
}

/// `el`, answering the pointer: presses, double and right clicks, hover,
/// drags, and things dropped from other programs.
pub struct MouseArea<Msg> {
    pub(crate) common: Common,
    child: RawElement,
    h: Handlers,
    _m: PhantomData<fn() -> Msg>,
}

#[derive(Default, Clone)]
struct Handlers {
    press: Option<MsgFn>,
    release: Option<MsgFn>,
    double: Option<MsgFn>,
    right: Option<MsgMap<Point>>,
    hover: Option<MsgMap<bool>>,
    moved: Option<MsgMap<Point>>,
    drag: Option<MsgMap<DragEvent>>,
    drop: Option<MsgMap<Dropped>>,
    cursor: Option<Cursor>,
    drag_out: Option<Rc<dyn Fn() -> sidestep_ui::DragSource>>,
}

crate::builder!(MouseArea);

pub fn mouse_area<Msg>(child: impl Into<Element<Msg>>) -> MouseArea<Msg> {
    MouseArea { common: Common::default(), child: child.into().raw, h: Handlers::default(), _m: PhantomData }
}

impl<Msg: Clone + Send + 'static> MouseArea<Msg> {
    pub fn on_press(mut self, msg: Msg) -> Self {
        self.h.press = Some(MsgFn::new(msg));
        self
    }

    pub fn on_release(mut self, msg: Msg) -> Self {
        self.h.release = Some(MsgFn::new(msg));
        self
    }

    pub fn on_double_click(mut self, msg: Msg) -> Self {
        self.h.double = Some(MsgFn::new(msg));
        self
    }

    /// A right click, where it was.
    pub fn on_right_click(mut self, f: impl Fn(Point) -> Msg + 'static) -> Self {
        self.h.right = Some(MsgMap::new(f));
        self
    }

    /// The pointer came over it (true) or left (false).
    pub fn on_hover(mut self, f: impl Fn(bool) -> Msg + 'static) -> Self {
        self.h.hover = Some(MsgMap::new(f));
        self
    }

    pub fn on_move(mut self, f: impl Fn(Point) -> Msg + 'static) -> Self {
        self.h.moved = Some(MsgMap::new(f));
        self
    }

    /// Let what's here be dragged out of the window: pressed here and
    /// moved a few points, it goes as `f` makes it
    /// (`DragSource::new().urls(..).icon(..)`), to other programs, the
    /// desktop, the Shelf or another window; drops on this program's
    /// windows come back through `on_drop`. `on_drag` hears no more of
    /// that press once it goes.
    pub fn drag_out(mut self, f: impl Fn() -> sidestep_ui::DragSource + 'static) -> Self {
        self.h.drag_out = Some(Rc::new(f));
        self
    }

    pub fn on_drag(mut self, f: impl Fn(DragEvent) -> Msg + 'static) -> Self {
        self.h.drag = Some(MsgMap::new(f));
        self
    }

    /// Files or text dropped on it (it takes drags that carry either).
    pub fn on_drop(mut self, f: impl Fn(Dropped) -> Msg + 'static) -> Self {
        self.h.drop = Some(MsgMap::new(f));
        self
    }
}

impl<Msg> MouseArea<Msg> {
    pub fn cursor(mut self, c: Cursor) -> Self {
        self.h.cursor = Some(c);
        self
    }
}

impl<Msg> From<MouseArea<Msg>> for Element<Msg> {
    fn from(m: MouseArea<Msg>) -> Element<Msg> {
        let mut common = m.common;
        if common.width == Length::Shrink {
            common.width = m.child.common.width;
        }
        if common.height == Length::Shrink {
            common.height = m.child.common.height;
        }
        Element::from_raw(RawElement { widget: Box::new(MouseWidget { h: m.h }), common, children: vec![m.child] })
    }
}

struct MouseWidget {
    h: Handlers,
}

#[derive(Default)]
struct MouseState {
    start: Option<Point>,
    last: Option<Point>,
    inside: bool,
}

impl Widget for MouseWidget {
    fn name(&self) -> &'static str {
        "mouse-area"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        wrap_layout(cx, bc)
    }

    fn hit(&self) -> bool {
        let h = &self.h;
        h.press.is_some()
            || h.release.is_some()
            || h.double.is_some()
            || h.right.is_some()
            || h.hover.is_some()
            || h.moved.is_some()
            || h.drag.is_some()
    }

    fn solid(&self) -> bool {
        self.hit() || self.h.drop.is_some()
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn cursor(&self) -> Option<Cursor> {
        self.h.cursor
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let h = self.h.clone();
        match event {
            Event::PointerMove { pos, .. } => {
                let st = cx.state::<MouseState>();
                let entered = !st.inside;
                st.inside = true;
                if entered && let Some(f) = &h.hover {
                    cx.emit_any(f.make(true));
                }
                if let Some(f) = &h.moved {
                    cx.emit_any(f.make(*pos));
                }
                if let (Some(out), Some(start)) = (&h.drag_out, cx.state::<MouseState>().start)
                    && (*pos - start).hypot() >= 4.0
                {
                    let st = cx.state::<MouseState>();
                    st.start = None;
                    st.last = None;
                    cx.start_drag(out());
                    return Status::Handled;
                }
                if let (Some(f), Some(start)) = (&h.drag, cx.state::<MouseState>().start) {
                    let last = cx.state::<MouseState>().last.replace(*pos).unwrap_or(start);
                    cx.emit_any(f.make(DragEvent::Move { start, at: *pos, delta: *pos - last }));
                }
                if h.moved.is_some() || h.drag.is_some() || h.hover.is_some() || h.drag_out.is_some() {
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::PointerLeave => {
                let st = cx.state::<MouseState>();
                if std::mem::replace(&mut st.inside, false)
                    && let Some(f) = &h.hover
                {
                    cx.emit_any(f.make(false));
                }
                Status::Ignored
            }
            Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => {
                if *clicks >= 2
                    && let Some(m) = &h.double
                {
                    cx.emit(m);
                    return Status::Handled;
                }
                let mut used = false;
                if let Some(m) = &h.press {
                    cx.emit(m);
                    used = true;
                }
                if let Some(f) = &h.drag {
                    let st = cx.state::<MouseState>();
                    st.start = Some(*pos);
                    st.last = Some(*pos);
                    cx.emit_any(f.make(DragEvent::Start(*pos)));
                    used = true;
                }
                if h.drag_out.is_some() {
                    let st = cx.state::<MouseState>();
                    st.start = Some(*pos);
                    st.last = Some(*pos);
                    used = true;
                }
                if used || h.release.is_some() { Status::Handled } else { Status::Ignored }
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                let start = cx.state::<MouseState>().start.take();
                cx.state::<MouseState>().last = None;
                if let (Some(f), Some(start)) = (&h.drag, start) {
                    cx.emit_any(f.make(DragEvent::End { start, at: *pos }));
                }
                if let Some(m) = &h.release {
                    cx.emit(m);
                }
                if h.release.is_some() || h.drag.is_some() || h.drag_out.is_some() {
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::PointerDown { pos, button: PointerButton::Right, .. } => match &h.right {
                Some(f) => {
                    cx.emit_any(f.make(*pos));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::DragOver { urls, has_text, .. } if h.drop.is_some() && (!urls.is_empty() || *has_text) => {
                Status::Handled
            }
            Event::Drop { pos, urls, text } => match &h.drop {
                Some(f) => {
                    cx.emit_any(f.make(Dropped { at: *pos, urls: urls.clone(), text: text.clone() }));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            _ => Status::Ignored,
        }
    }
}

// Canvases.

type DrawFn<S> = Rc<dyn Fn(&mut Canvas, Rect, &S, &Theme)>;
type EventFn<S> = Rc<dyn Fn(&Event, Size, &S) -> Option<crate::element::AnyMsg>>;
type PartsFn<S> = Rc<dyn Fn(&S, Size) -> Vec<(Rect, u64)>>;

/// Drawing of one's own, from `state` (whose hash decides when it's
/// drawn again): `canvas(self.samples.clone(), |c, bounds, samples, theme| …)`.
pub struct CanvasEl<Msg, S> {
    pub(crate) common: Common,
    state: S,
    draw: DrawFn<S>,
    on_event: Option<EventFn<S>>,
    parts: Option<PartsFn<S>>,
    focusable: bool,
    ring: crate::widget::FocusRing,
    _m: PhantomData<fn() -> Msg>,
}

impl<Msg, S> crate::element::Layoutable for CanvasEl<Msg, S> {
    fn common_mut(&mut self) -> &mut Common {
        &mut self.common
    }
}

pub fn canvas<Msg, S: Hash + 'static>(
    state: S,
    draw: impl Fn(&mut Canvas, Rect, &S, &Theme) + 'static,
) -> CanvasEl<Msg, S> {
    CanvasEl {
        common: Common::sized(Length::Fill, Length::Fill),
        state,
        draw: Rc::new(draw),
        on_event: None,
        parts: None,
        focusable: false,
        ring: crate::widget::FocusRing::Keyboard,
        _m: PhantomData,
    }
}

impl<Msg: Send + 'static, S: Hash + 'static> CanvasEl<Msg, S> {
    /// Input over it (positions in its own coordinates), with its size
    /// and state: a message, if it means one.
    pub fn on_event(mut self, f: impl Fn(&Event, Size, &S) -> Option<Msg> + 'static) -> Self {
        self.on_event = Some(Rc::new(move |e, s, st| f(e, s, st).map(|m| Box::new(m) as crate::element::AnyMsg)));
        self
    }

    /// It takes the keyboard focus on a click (and by Tab), for keys.
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }

    /// When its focus ring shows: from the keyboard (the default), or
    /// never, for a canvas that is the window's whole content (a picture,
    /// a page).
    pub fn focus_ring(mut self, ring: crate::widget::FocusRing) -> Self {
        self.ring = ring;
        self
    }

    /// Parts of the drawing that change by themselves (a hovered bar, a
    /// cursor, a selection): `f` gives each its rectangle (the canvas's
    /// coordinates) and a hash of how it looks. When parts change and
    /// `state`'s hash doesn't, only the rectangles of the parts that came,
    /// went or changed are drawn again, so leave what they show out of
    /// `state`'s `Hash`.
    pub fn parts(mut self, f: impl Fn(&S, Size) -> Vec<(Rect, u64)> + 'static) -> Self {
        self.parts = Some(Rc::new(f));
        self
    }
}

impl<Msg, S: Hash + 'static> From<CanvasEl<Msg, S>> for Element<Msg> {
    fn from(c: CanvasEl<Msg, S>) -> Element<Msg> {
        let hash = hash_of(&c.state);
        let w = CanvasWidget {
            state: c.state,
            hash,
            draw: c.draw,
            on_event: c.on_event,
            parts: c.parts,
            focusable: c.focusable,
            ring: c.ring,
        };
        Element::from_raw(RawElement::new(w, c.common, Vec::new()))
    }
}

struct CanvasWidget<S> {
    state: S,
    hash: u64,
    draw: DrawFn<S>,
    on_event: Option<EventFn<S>>,
    parts: Option<PartsFn<S>>,
    focusable: bool,
    ring: crate::widget::FocusRing,
}

impl<S: 'static> Widget for CanvasWidget<S> {
    fn name(&self) -> &'static str {
        "canvas"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let size = bc.constrain(Size::new(
            if bc.max.width.is_finite() { bc.max.width } else { 200.0 },
            if bc.max.height.is_finite() { bc.max.height } else { 120.0 },
        ));
        if let Some(f) = &self.parts {
            let now = f(&self.state, size);
            let st = cx.state::<CanvasParts>();
            let before = std::mem::replace(&mut st.parts, now.clone());
            // A new size redraws it all anyway.
            if st.size.replace(size) == Some(size) {
                let gone = before.iter().filter(|p| !now.contains(p));
                let came = now.iter().filter(|p| !before.contains(p));
                for (r, _) in gone.chain(came) {
                    cx.invalidate_rect(*r);
                }
            }
        }
        size
    }

    fn paint(&self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let theme = cx.theme();
        cx.canvas.save();
        cx.canvas.clip_rect(b);
        (self.draw)(cx.canvas, b, &self.state, theme);
        cx.canvas.restore();
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.hash);
    }

    fn hit(&self) -> bool {
        self.on_event.is_some()
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn focusable(&self) -> Focusable {
        if self.focusable { Focusable::Click } else { Focusable::No }
    }

    fn focus_ring(&self) -> crate::widget::FocusRing {
        self.ring
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let Some(f) = &self.on_event else { return Status::Ignored };
        match f(event, cx.size(), &self.state) {
            Some(m) => {
                cx.emit_any(m);
                Status::Handled
            }
            // Presses are its own even when they mean nothing yet (a drag
            // starts with one).
            None if matches!(event, Event::PointerDown { .. }) => Status::Handled,
            None => Status::Ignored,
        }
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: SemanticRole::Canvas, ..Default::default() })
    }
}

/// A canvas's parts as last laid out, to see which changed.
#[derive(Default)]
struct CanvasParts {
    parts: Vec<(Rect, u64)>,
    size: Option<Size>,
}
