//! `pointer`: hover, press, drags-only presses, capture and compositor
//! drags in one Aizome kit window (catalog KIT-13, UIW-01). See `SPEC.md`.

pub mod workload;

#[cfg(feature = "testing")]
pub mod env;
#[cfg(feature = "testing")]
pub mod traces;

use crate::Sink;

/// A widget node in the window's tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node(pub u16);

/// Window coordinates in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
    /// Squared distance, for the drag threshold.
    pub const fn dist2(self, o: Point) -> i64 {
        let dx = (self.x - o.x) as i64;
        let dy = (self.y - o.y) as i64;
        dx * dx + dy * dy
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Middle,
    Right,
}

/// What the window's hit test found at the event's point. The engine
/// computes it; the machine only routes.
///
/// Valid hits (the environment guarantees): `drags_only` implies
/// `node.is_some()`; `under.is_some()` implies `drags_only`; `under != node`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Hit {
    /// The topmost node that reacts to the pointer here: it is hovered and
    /// takes a press. `None` over inert background.
    pub node: Option<Node>,
    /// `node` takes only left-button drags (a resize edge over rows).
    pub drags_only: bool,
    /// Beneath a drags-only `node`: what shows hover too, gets other
    /// buttons' presses, and gets the click when no drag came of a left press.
    pub under: Option<Node>,
}

impl Hit {
    pub const NOTHING: Hit = Hit { node: None, drags_only: false, under: None };

    /// Over an ordinary node.
    pub const fn on(n: u16) -> Hit {
        Hit { node: Some(Node(n)), drags_only: false, under: None }
    }

    /// Over drags-only `edge`, with `under` beneath it (0: nothing).
    pub const fn edge(edge: u16, under: u16) -> Hit {
        Hit { node: Some(Node(edge)), drags_only: true, under: if under == 0 { None } else { Some(Node(under)) } }
    }

    /// What a non-left press reaches: `under` for a drags-only node, else `node`.
    pub const fn beneath(&self) -> Option<Node> {
        if self.drags_only { self.under } else { self.node }
    }

    /// Whether the point is over `n` (as `node` or as what's `under` it):
    /// the release is "inside" the node that took the press.
    pub fn names(&self, n: Node) -> bool {
        self.node == Some(n) || self.under == Some(n)
    }
}

/// Who started a compositor drag. The spec treats both alike; the current
/// code doesn't (KIT-13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DragFrom {
    /// The widget that has the press asked, while handling a move (`EventCx::start_drag`).
    Widget,
    /// The program asked, from a message the press or a move sent (`Cx::start_drag`).
    Program,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    Move {
        at: Point,
        hit: Hit,
    },
    Press {
        at: Point,
        button: Button,
        clicks: u8,
        hit: Hit,
    },
    Release {
        at: Point,
        button: Button,
        hit: Hit,
    },
    /// The pointer left the window (or the compositor took it).
    Leave,
    /// A node asked to get the pointer's moves and next release (`capture_pointer`).
    Capture(Node),
    /// A node gave up the pointer (`release_pointer`).
    Uncapture(Node),
    /// A compositor drag-and-drop started: the compositor owns the pointer.
    StartDrag(DragFrom),
    /// That drag ended (dropped or cancelled), wherever the pointer is.
    DragEnded,
    /// A node was rebuilt away or covered by a modal layer.
    Removed(Node),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    /// The node is hovered now (paint, cursor).
    HoverOn(Node),
    /// The node isn't hovered any more (paint, `PointerLeave`).
    HoverOff(Node),
    /// Shows hover beneath a drags-only node.
    UnderOn(Node),
    UnderOff(Node),
    /// Deliver `PointerMove`.
    Move {
        node: Node,
        at: Point,
    },
    /// Deliver `PointerDown`.
    Down {
        node: Node,
        at: Point,
        button: Button,
        clicks: u8,
    },
    /// Deliver `PointerUp` to the node that has the press; `inside`: the
    /// release is over it (a control fires only then).
    Up {
        node: Node,
        at: Point,
        button: Button,
        inside: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateName {
    /// The pointer isn't over the window.
    Outside,
    /// Over the window, no press owner.
    Hovering,
    /// A node has the press (or a capture): it gets the moves and the release.
    Pressed,
    /// A drags-only node took a left press; no drag yet: a release is a click under it.
    DragsOnlyPressed,
    /// The drags-only press became a drag (or a click under was cancelled).
    DragsOnlyDragging,
    /// The compositor owns the pointer for a drag-and-drop.
    NativeDrag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    /// Distance from the press at which a drags-only press becomes a drag.
    pub drag_threshold: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self { drag_threshold: 4 }
    }
}

/// An implementation of the pointer machine.
pub trait Machine: Sized {
    fn new(config: Config) -> Self;
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>);
    fn state(&self) -> StateName;
}
