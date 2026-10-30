//! A drawn element: the shell's surfaces are mostly things the kit has no
//! control for (the bar's titles, the Desks strip, meters, Deskbar rows),
//! each drawn by a closure from the state it was viewed with, repainted
//! only when its hash or rectangle changes, and answering the pointer with
//! messages.

use std::rc::Rc;

use aizome_kit::element::{Element, MsgFn, MsgMap};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Constraints, Point, Size, Vec2};
use aizome_kit::sidestep_ui::{Cursor, PointerButton};
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget};

/// What a drag through a drawn element says. Coordinates are those of the
/// window unless the handler was installed with [`Drawn::on_drag_local`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragPhase {
    /// The pointer moved far enough with the button down.
    Start(Point),
    Move(Point),
    /// Released, at.
    End(Point),
}

/// A pointer press, in the window's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Press {
    pub at: Point,
    /// The element's own rectangle in the window, for anchoring a menu.
    pub rect: aizome_kit::Rect,
}

type Painter = Rc<dyn Fn(&mut PaintCx)>;
type Fit = Rc<dyn Fn(&mut LayoutCx) -> f64>;

/// A drawn element, `width` × `height` points (0: take what's offered).
pub struct Drawn<Msg> {
    w: DrawnWidget,
    common: aizome_kit::element::Common,
    child: Option<Element<Msg>>,
    _m: std::marker::PhantomData<fn() -> Msg>,
}

pub fn drawn<Msg>(
    width: f64,
    height: f64,
    hash: impl std::hash::Hash,
    paint: impl Fn(&mut PaintCx) + 'static,
) -> Drawn<Msg> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash.hash(&mut h);
    Drawn {
        w: DrawnWidget {
            size: Size::new(width, height),
            hash: std::hash::Hasher::finish(&h),
            paint: Rc::new(paint),
            click: None,
            double: None,
            right: None,
            middle: None,
            wheel: None,
            press: None,
            drag: None,
            drag_local: false,
            drags_only: false,
            hover: false,
            cursor: None,
            label: None,
            role: SemanticRole::Generic,
            text: None,
            fit: None,
            hovered_msg: None,
            left_msg: None,
            pad: aizome_kit::Padding::default(),
            drop: None,
            drag_out: None,
            overflow: 0.0,
        },
        common: aizome_kit::element::Common::default(),
        child: None,
        _m: std::marker::PhantomData,
    }
}

impl<Msg: Clone + Send + 'static> Drawn<Msg> {
    /// A left click (press and release over it).
    pub fn on_click(mut self, msg: Msg) -> Self {
        self.w.click = Some(MsgFn::new(msg));
        self.w.hover = true;
        self
    }

    pub fn on_double_click(mut self, msg: Msg) -> Self {
        self.w.double = Some(MsgFn::new(msg));
        self.w.hover = true;
        self
    }

    pub fn on_middle_click(mut self, msg: Msg) -> Self {
        self.w.middle = Some(MsgFn::new(msg));
        self
    }

    /// The wheel (or two fingers) over it: how far, in points, positive
    /// away from the person and to the right.
    pub fn on_scroll(mut self, f: impl Fn(Vec2) -> Msg + 'static) -> Self {
        self.w.wheel = Some(MsgMap::new(f));
        self
    }

    /// A right press, with where.
    pub fn on_right_click(mut self, f: impl Fn(Press) -> Msg + 'static) -> Self {
        self.w.right = Some(MsgMap::new(f));
        self.w.hover = true;
        self
    }

    /// A left press, at once (menus open on press).
    pub fn on_press(mut self, f: impl Fn(Press) -> Msg + 'static) -> Self {
        self.w.press = Some(MsgMap::new(f));
        self.w.hover = true;
        self
    }

    /// Drags with the left button.
    pub fn on_drag(mut self, f: impl Fn(DragPhase) -> Msg + 'static) -> Self {
        self.w.drag = Some(MsgMap::new(f));
        self.w.drag_local = false;
        self
    }

    /// Drags in the element's current coordinates. Scrolling its container
    /// is then included in the displacement from the original press.
    pub fn on_drag_local(mut self, f: impl Fn(DragPhase) -> Msg + 'static) -> Self {
        self.w.drag = Some(MsgMap::new(f));
        self.w.drag_local = true;
        self
    }

    /// Takes only its drags (an edge to resize by, over rows): a press that
    /// doesn't become one is a click on what's under it, and the other
    /// buttons, the wheel and drops go there too. Use it with
    /// [`on_drag`](Self::on_drag) and no click handlers.
    pub fn drags_only(mut self) -> Self {
        self.w.drags_only = true;
        self
    }

    /// Takes drops of files, links and text.
    pub fn on_drop(mut self, f: impl Fn(Vec<String>, Option<String>) -> Msg + 'static) -> Self {
        self.w.drop = Some(MsgMap::new(move |(u, t): (Vec<String>, Option<String>)| f(u, t)));
        self
    }

    /// `on_hover`, when `on`.
    pub fn on_hover_if(self, on: bool, msg: Msg) -> Self {
        if on { self.on_hover(msg) } else { self }
    }

    /// Sent when the pointer comes over it (a menu bar switching menus).
    pub fn on_hover(mut self, msg: Msg) -> Self {
        self.w.hovered_msg = Some(MsgFn::new(msg));
        self.w.hover = true;
        self
    }

    /// Sent when the pointer leaves this element.
    pub fn on_leave(mut self, msg: Msg) -> Self {
        self.w.left_msg = Some(MsgFn::new(msg));
        self.w.hover = true;
        self
    }
}

impl<Msg> Drawn<Msg> {
    /// Dragging it (with the left button) drags `source` out to other
    /// programs: the Shelf's items.
    pub fn drag_out(mut self, source: impl Fn() -> aizome_kit::sidestep_ui::DragSource + 'static) -> Self {
        self.w.drag_out = Some(Rc::new(source));
        self
    }

    /// Repaints when hovered (it shows hover) even without a handler.
    pub fn hoverable(mut self) -> Self {
        self.w.hover = true;
        self
    }

    /// It paints up to `points` outside its rectangle (a label wider than
    /// its icon).
    pub fn overflow(mut self, points: f64) -> Self {
        self.w.overflow = points;
        self
    }

    /// Its width measured at layout (text and padding), not given.
    pub fn fit(mut self, f: impl Fn(&mut LayoutCx) -> f64 + 'static) -> Self {
        self.w.fit = Some(Rc::new(f));
        self
    }

    pub fn cursor(mut self, c: Cursor) -> Self {
        self.w.cursor = Some(c);
        self
    }

    /// What assistive technology and tests read: its text and role.
    pub fn text(mut self, t: impl Into<String>, role: SemanticRole) -> Self {
        let t = t.into();
        self.w.label = Some(t.clone());
        self.w.text = Some(t);
        self.w.role = role;
        self
    }
}

impl<Msg> aizome_kit::Layoutable for Drawn<Msg> {
    fn common_mut(&mut self) -> &mut aizome_kit::element::Common {
        &mut self.common
    }
}

impl<Msg> From<Drawn<Msg>> for Element<Msg> {
    fn from(d: Drawn<Msg>) -> Element<Msg> {
        let mut e = Element::new(d.w, d.child.into_iter().collect());
        *aizome_kit::Layoutable::common_mut(&mut e) = d.common;
        e
    }
}

struct DrawnWidget {
    size: Size,
    hash: u64,
    paint: Painter,
    click: Option<MsgFn>,
    double: Option<MsgFn>,
    right: Option<MsgMap<Press>>,
    middle: Option<MsgFn>,
    wheel: Option<MsgMap<Vec2>>,
    press: Option<MsgMap<Press>>,
    drag: Option<MsgMap<DragPhase>>,
    drag_local: bool,
    /// Takes only its drags: a click comes to what's under it.
    drags_only: bool,
    hover: bool,
    cursor: Option<Cursor>,
    label: Option<String>,
    role: SemanticRole,
    text: Option<String>,
    /// Its width from what it measures (text).
    fit: Option<Fit>,
    /// Sent when the pointer comes over it.
    hovered_msg: Option<MsgFn>,
    left_msg: Option<MsgFn>,
    /// Room round its child.
    pad: aizome_kit::Padding,
    /// How far outside its rectangle it paints.
    overflow: f64,
    /// Takes drops: files' URLs and text.
    drop: Option<MsgMap<(Vec<String>, Option<String>)>>,
    /// What a drag out of it carries.
    drag_out: Option<Rc<dyn Fn() -> aizome_kit::sidestep_ui::DragSource>>,
}

/// A press under way.
#[derive(Default)]
struct PressState {
    from: Option<Point>,
    dragging: bool,
    /// The pointer is over it (for `on_hover`).
    over: bool,
}

impl Widget for DrawnWidget {
    fn name(&self) -> &'static str {
        "drawn"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        if cx.child_count() > 0 {
            // Around a child: its size and the padding, or the width given.
            let p = self.pad;
            let mut inner = Constraints::loose(Size::new(
                (bc.max.width - p.horizontal()).max(0.0),
                (bc.max.height - p.vertical()).max(0.0),
            ));
            if self.size.width > 0.0 {
                inner = inner.with_width((self.size.width - p.horizontal()).max(0.0));
            }
            if self.size.height > 0.0 {
                inner = inner.with_height((self.size.height - p.vertical()).max(0.0));
            }
            let s = cx.layout_child(0, inner);
            cx.place_child(0, Point::new(p.left, p.top));
            return bc.constrain(Size::new(s.width + p.horizontal(), s.height + p.vertical()));
        }
        if let Some(f) = &self.fit {
            self.size.width = f(cx).ceil();
        }
        let w = if self.size.width > 0.0 { self.size.width } else { bc.max.width.min(1e6) };
        let h = if self.size.height > 0.0 { self.size.height } else { bc.max.height.min(1e6) };
        bc.constrain(Size::new(w, h))
    }

    fn paint(&self, cx: &mut PaintCx) {
        (self.paint)(cx);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.hash);
    }

    fn hit(&self) -> bool {
        self.hover
            || self.drag.is_some()
            || self.middle.is_some()
            || self.wheel.is_some()
            || self.drop.is_some()
            || self.drag_out.is_some()
    }

    fn hover_paints(&self) -> bool {
        self.hover
    }

    fn drags_only(&self) -> bool {
        self.drags_only
    }

    fn overflow(&self) -> f64 {
        self.overflow
    }

    fn cursor(&self) -> Option<Cursor> {
        self.cursor
    }

    fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    fn semantics(&self) -> Option<Semantics> {
        self.label.as_ref().map(|l| Semantics { role: self.role, label: Some(l.clone()), ..Default::default() })
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        let origin = cx.window_rect().origin().to_vec2();
        let drag_origin = if self.drag_local { aizome_kit::Vec2::ZERO } else { origin };
        let rect = cx.window_rect();
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => {
                if *clicks >= 2
                    && let Some(m) = &self.double
                {
                    cx.emit(m);
                    return Status::Handled;
                }
                if let Some(m) = &self.press {
                    cx.emit_map(m, Press { at: *pos + origin, rect });
                }
                let s = cx.state::<PressState>();
                s.from = Some(*pos);
                s.dragging = false;
                if self.drag.is_some() {
                    cx.capture_pointer();
                }
                cx.invalidate();
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                if !cx.state::<PressState>().over {
                    cx.state::<PressState>().over = true;
                    if let Some(m) = &self.hovered_msg {
                        cx.emit(m);
                    }
                }
                // Out to other programs: the system takes the drag over.
                if let Some(out) = &self.drag_out {
                    let s = cx.state::<PressState>();
                    if let Some(from) = s.from
                        && !s.dragging
                        && (pos.x - from.x).hypot(pos.y - from.y) >= 4.0
                    {
                        s.dragging = true;
                        let source = out();
                        cx.start_drag(source);
                    }
                    return Status::Handled;
                }
                let Some(drag) = &self.drag else { return Status::Ignored };
                let s = cx.state::<PressState>();
                let Some(from) = s.from else { return Status::Ignored };
                if !s.dragging && (pos.x - from.x).hypot(pos.y - from.y) >= 4.0 {
                    s.dragging = true;
                    cx.emit_map(drag, DragPhase::Start(from + drag_origin));
                }
                if cx.state::<PressState>().dragging {
                    cx.emit_map(drag, DragPhase::Move(*pos + drag_origin));
                }
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                let over = cx.state::<PressState>().over;
                let s = std::mem::take(cx.state::<PressState>());
                cx.state::<PressState>().over = over;
                if self.drag.is_some() {
                    cx.release_pointer();
                }
                cx.invalidate();
                if !s.dragging && self.drags_only {
                    // No drag came of it: the press was a click on what's
                    // under it, which gets it now.
                    return Status::Ignored;
                }
                if s.dragging {
                    if let Some(d) = &self.drag {
                        cx.emit_map(d, DragPhase::End(*pos + drag_origin));
                    }
                } else if s.from.is_some()
                    && cx.bounds().contains(*pos)
                    && let Some(m) = &self.click
                {
                    cx.emit(m);
                }
                Status::Handled
            }
            Event::PointerDown { pos, button: PointerButton::Right, .. } => match &self.right {
                Some(m) => {
                    cx.emit_map(m, Press { at: *pos + origin, rect });
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::PointerDown { button: PointerButton::Middle, .. } => match &self.middle {
                Some(m) => {
                    cx.emit(m);
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::Scroll { delta, .. } => match &self.wheel {
                Some(m) => {
                    cx.emit_map(m, *delta);
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::DragOver { .. } => {
                if self.drop.is_some() {
                    Status::Handled
                } else {
                    Status::Ignored
                }
            }
            Event::Drop { urls, text, .. } => match &self.drop {
                Some(m) => {
                    cx.emit_map(m, (urls.clone(), text.clone()));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            Event::PointerLeave => {
                cx.state::<PressState>().over = false;
                if let Some(m) = &self.left_msg {
                    cx.emit(m);
                }
                Status::Ignored
            }
            _ => Status::Ignored,
        }
    }
}

/// `emit` for a value: [`EventCx`] takes made messages.
trait EmitMap {
    fn emit_map<T: 'static>(&mut self, m: &MsgMap<T>, v: T);
}

impl EmitMap for EventCx<'_, '_> {
    fn emit_map<T: 'static>(&mut self, m: &MsgMap<T>, v: T) {
        let msg = m.make(v);
        self.emit(&MsgFn::from_fn(make_once(msg)));
    }
}

/// A message made once: `MsgFn` asks for a maker, and this one has made it.
fn make_once(msg: aizome_kit::element::AnyMsg) -> impl Fn() -> aizome_kit::element::AnyMsg {
    let cell = std::cell::RefCell::new(Some(msg));
    move || cell.borrow_mut().take().expect("made once")
}

/// A drawn element around `child`, `pad` points from its edges: it paints
/// under the child (a frame, a fill) and is the child's size plus the
/// padding, or the size given with `.width()`-like `size`.
pub fn around<Msg>(
    child: impl Into<Element<Msg>>,
    pad: impl Into<aizome_kit::Padding>,
    size: Size,
    hash: impl std::hash::Hash,
    paint: impl Fn(&mut PaintCx) + 'static,
) -> Drawn<Msg> {
    let mut d = drawn(size.width, size.height, hash, paint);
    d.w.pad = pad.into();
    d.child = Some(child.into());
    d
}

/// Two children at the ends of a row at least `min_width` wide: the first
/// from the left, the second from the right, `gap` apart at least; the row
/// is as wide as both need when that's more than `min_width` (a header
/// whose widgets outgrow it).
pub fn split<Msg>(left: Element<Msg>, right: Element<Msg>, min_width: f64, gap: f64, pad: (f64, f64)) -> Element<Msg> {
    Element::new(SplitWidget { min_width, gap, pad }, vec![left, right])
}

struct SplitWidget {
    min_width: f64,
    gap: f64,
    pad: (f64, f64),
}

impl Widget for SplitWidget {
    fn name(&self) -> &'static str {
        "split"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let room = Constraints::loose(Size::new(f64::INFINITY, bc.max.height));
        let l = cx.layout_child(0, room);
        let r = cx.layout_child(1, room);
        let h = l.height.max(r.height);
        let w = (self.pad.0 + l.width + self.gap + r.width + self.pad.1).max(self.min_width).ceil();
        cx.place_child(0, Point::new(self.pad.0, ((h - l.height) / 2.0).round()));
        cx.place_child(1, Point::new(w - self.pad.1 - r.width, ((h - r.height) / 2.0).round()));
        bc.constrain(Size::new(w, h))
    }
}

/// Children at points of their own (the desktop's icons), over the room
/// given.
pub fn free<Msg>(children: Vec<(Point, Element<Msg>)>) -> Element<Msg> {
    let (points, els): (Vec<Point>, Vec<Element<Msg>>) = children.into_iter().unzip();
    Element::new(FreeWidget { points }, els)
}

struct FreeWidget {
    points: Vec<Point>,
}

impl Widget for FreeWidget {
    fn name(&self) -> &'static str {
        "free"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        for i in 0..cx.child_count() {
            cx.layout_child(i, Constraints::loose(bc.max));
            cx.place_child(i, self.points.get(i).copied().unwrap_or(Point::ZERO));
        }
        bc.constrain(bc.max)
    }
}
