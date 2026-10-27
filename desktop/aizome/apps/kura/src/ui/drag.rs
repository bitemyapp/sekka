//! Dragging items into the Collector, inside the window.
//!
//! The toolkit has no drag source of its own, so Kura drags as a gesture:
//! the widget that took the press (a sunburst segment, a list row) keeps
//! the pointer, and the Collector's [`DropZone`] says where it is through
//! a shared cell when it draws. Moving over it, and letting go there, are
//! messages; the program shows the Collector as a target meanwhile.

use std::cell::Cell;
use std::rc::Rc;

use aizome_kit::element::{MsgFn, MsgMap};
use aizome_kit::sidestep_ui::{Cursor, PointerButton};
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, Status, Widget};
use aizome_kit::{Constraints, Element, Point, Rect, Size};

use crate::tree::NodeId;

/// Where the Collector is, in window points, once it has been drawn.
pub type ZoneCell = Rc<Cell<Option<Rect>>>;

/// A drag's course.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragEvent {
    Start(NodeId),
    /// The pointer went over the Collector (true) or off it.
    Over(bool),
    /// Let go over the Collector.
    Drop(NodeId),
    /// Let go elsewhere.
    Cancel,
}

/// Points the pointer must move with the button down before it drags.
pub const SLOP: f64 = 4.0;

/// A press under way that may become a drag.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gesture {
    pub start: Option<(Point, NodeId)>,
    pub dragging: bool,
    pub over: bool,
}

impl Gesture {
    /// The pointer moved to `pos` (the widget's coordinates) with the button
    /// held: what to tell the program. `draggable` says whether the press's
    /// node may be dragged.
    pub fn moved(&mut self, cx: &mut EventCx, pos: Point, zone: &ZoneCell, draggable: bool) -> Option<DragEvent> {
        let (from, node) = self.start?;
        if !self.dragging {
            if !draggable || (pos - from).hypot() < SLOP {
                return None;
            }
            self.dragging = true;
            cx.capture_pointer();
            cx.set_cursor(Cursor::Grabbing);
            return Some(DragEvent::Start(node));
        }
        let at = cx.window_rect().origin() + pos.to_vec2();
        let over = zone.get().is_some_and(|r| r.contains(at));
        if over != self.over {
            self.over = over;
            cx.set_cursor(if over { Cursor::Copy } else { Cursor::Grabbing });
            return Some(DragEvent::Over(over));
        }
        None
    }

    /// The button came up: the drag's end, if it was one (else `None`: a
    /// click, for the widget to handle).
    pub fn released(&mut self, cx: &mut EventCx) -> Option<DragEvent> {
        let (_, node) = self.start.take()?;
        if !std::mem::take(&mut self.dragging) {
            return None;
        }
        cx.release_pointer();
        cx.set_cursor(Cursor::Default);
        Some(if std::mem::take(&mut self.over) { DragEvent::Drop(node) } else { DragEvent::Cancel })
    }
}

/// Send `ev` through `map`.
pub fn emit<T: Clone + 'static>(cx: &mut EventCx, map: &MsgMap<T>, ev: T) {
    let m = map.clone();
    cx.emit(&MsgFn::from_fn(move || m.make(ev.clone())));
}

/// The Collector's area: it says where it is when it draws.
pub struct DropZone {
    zone: ZoneCell,
}

/// `child`, telling `zone` where it is.
pub fn drop_zone<Msg: 'static>(zone: ZoneCell, child: impl Into<Element<Msg>>) -> Element<Msg> {
    Element::new(DropZone { zone }, vec![child.into()])
}

impl Widget for DropZone {
    fn name(&self) -> &'static str {
        "kura-drop-zone"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let at = cx.canvas.transform() * Point::ZERO;
        self.zone.set(Some(Rect::from_origin_size(at, cx.size())));
    }
}

/// A list row's content that can be clicked, double-clicked or dragged.
pub struct DragHandle {
    node: NodeId,
    draggable: bool,
    zone: ZoneCell,
    on_select: MsgMap<NodeId>,
    on_open: MsgMap<NodeId>,
    on_drag: MsgMap<DragEvent>,
}

/// What a row does with its clicks and drags.
#[derive(Clone)]
pub struct RowActions {
    pub zone: ZoneCell,
    pub on_select: MsgMap<NodeId>,
    pub on_open: MsgMap<NodeId>,
    pub on_drag: MsgMap<DragEvent>,
}

/// `child` as a row for `node`.
pub fn handle<Msg: 'static>(
    node: NodeId,
    draggable: bool,
    actions: &RowActions,
    child: impl Into<Element<Msg>>,
) -> Element<Msg> {
    let w = DragHandle {
        node,
        draggable,
        zone: actions.zone.clone(),
        on_select: actions.on_select.clone(),
        on_open: actions.on_open.clone(),
        on_drag: actions.on_drag.clone(),
    };
    Element::new(w, vec![child.into()])
}

impl Widget for DragHandle {
    fn name(&self) -> &'static str {
        "kura-row"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let s = cx.layout_child(0, bc);
        cx.place_child(0, Point::ZERO);
        bc.constrain(s)
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        false
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { pos, button: PointerButton::Left, clicks, .. } => {
                let g = cx.state::<Gesture>();
                *g = Gesture { start: Some((*pos, self.node)), dragging: false, over: false };
                emit(cx, &self.on_select, self.node);
                if *clicks >= 2 {
                    cx.state::<Gesture>().start = None;
                    emit(cx, &self.on_open, self.node);
                }
                Status::Handled
            }
            Event::PointerMove { pos, .. } => {
                let mut g = *cx.state::<Gesture>();
                let ev = g.moved(cx, *pos, &self.zone, self.draggable);
                *cx.state::<Gesture>() = g;
                if let Some(ev) = ev {
                    emit(cx, &self.on_drag, ev);
                    return Status::Handled;
                }
                if g.start.is_some() { Status::Handled } else { Status::Ignored }
            }
            Event::PointerUp { button: PointerButton::Left, .. } => {
                let mut g = *cx.state::<Gesture>();
                let ev = g.released(cx);
                *cx.state::<Gesture>() = g;
                if let Some(ev) = ev {
                    emit(cx, &self.on_drag, ev);
                }
                Status::Handled
            }
            _ => Status::Ignored,
        }
    }
}
