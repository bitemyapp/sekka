//! `pointer` with statum 0.9.0: `#[state]` declares the six leaves (each
//! with its own payload type, sealed: private fields, or a token for the
//! states without data), `#[machine] Router` carries the drag threshold,
//! and each `#[transition]` block holds one state's legal edges. statum has
//! no superstates: every inside leaf carries the hover, and the `inside!`
//! macro stands in for `Inside`'s shared exits. `handle` matches the
//! generated `router::SomeState` per event.

use crate::{SEAL, Seal};
use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};
use statum::{Branch, machine, state, transition};

pub const IMPLEMENTED: bool = true;

/// The hover pair every inside state carries.
#[derive(Clone, Copy, Debug)]
pub struct Hover {
    hot: Option<Node>,
    under: Option<Node>,
}

impl Hover {
    pub fn hot(&self) -> Option<Node> {
        self.hot
    }
    fn track(&mut self, hit: Hit, out: &mut impl Sink<Effect>) {
        let under = if hit.drags_only { hit.under } else { None };
        if under != self.under {
            if let Some(o) = self.under {
                out.push(Effect::UnderOff(o));
            }
            if let Some(n) = under {
                out.push(Effect::UnderOn(n));
            }
            self.under = under;
        }
        if hit.node != self.hot {
            if let Some(o) = self.hot {
                out.push(Effect::HoverOff(o));
            }
            if let Some(n) = hit.node {
                out.push(Effect::HoverOn(n));
            }
            self.hot = hit.node;
        }
    }
    fn clear(self, out: &mut impl Sink<Effect>) {
        if let Some(u) = self.under {
            out.push(Effect::UnderOff(u));
        }
        if let Some(n) = self.hot {
            out.push(Effect::HoverOff(n));
        }
    }
    fn forget(&mut self, n: Node) {
        if self.hot == Some(n) {
            self.hot = None;
        }
        if self.under == Some(n) {
            self.under = None;
        }
    }
}

tokens!(OutsideTok, NativeDragTok);

#[derive(Debug)]
pub struct HoveringData {
    hover: Hover,
}

#[derive(Debug)]
pub struct PressedData {
    hover: Hover,
    owner: Node,
}

impl PressedData {
    pub fn owner(&self) -> Node {
        self.owner
    }
}

#[derive(Debug)]
pub struct EdgePressedData {
    hover: Hover,
    edge: Node,
    at: Point,
    clicks: u8,
    beneath: Option<Node>,
}

#[derive(Debug)]
pub struct EdgeDraggingData {
    hover: Hover,
    edge: Node,
}

#[state]
#[derive(Debug)]
pub enum PointerState {
    Outside(OutsideTok),
    Hovering(HoveringData),
    Pressed(PressedData),
    EdgePressed(EdgePressedData),
    EdgeDragging(EdgeDraggingData),
    NativeDrag(NativeDragTok),
}

#[machine]
#[derive(Debug)]
pub struct Router<PointerState> {
    threshold2: i64,
    seal: Seal,
}

fn target(button: Button, hit: Hit) -> Option<Node> {
    if button == Button::Left { hit.node } else { hit.beneath() }
}

fn down(t: Option<Node>, at: Point, button: Button, clicks: u8, out: &mut impl Sink<Effect>) {
    if let Some(node) = t {
        out.push(Effect::Down { node, at, button, clicks });
    }
}

fn up(node: Node, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) {
    out.push(Effect::Up { node, at, button, inside: hit.names(node) });
}

impl Router<Outside> {
    #[inline]
    pub fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        Router::<Outside>::builder().state_data(OutsideTok(())).threshold2(t * t).seal(SEAL).build()
    }
}

#[transition]
impl Router<Outside> {
    pub fn enter(self) -> Router<Hovering> {
        self.transition_with(HoveringData { hover: Hover { hot: None, under: None } })
    }
    pub fn start_drag(self) -> Router<NativeDrag> {
        self.transition_with(NativeDragTok(()))
    }
}

/// `Inside`'s exits, shared by its leaves (their only edges to `Outside`
/// and `NativeDrag`, so no other block declares them).
macro_rules! inside {
    ($($s:ident),*) => {$(
        #[transition]
        impl Router<$s> {
            pub fn leave(self, out: &mut impl Sink<Effect>) -> Router<Outside> {
                self.state_data.hover.clear(out);
                self.transition_with(OutsideTok(()))
            }
            pub fn start_drag(self, out: &mut impl Sink<Effect>) -> Router<NativeDrag> {
                self.state_data.hover.clear(out);
                self.transition_with(NativeDragTok(()))
            }
        }
    )*};
}
inside!(Hovering, Pressed, EdgePressed, EdgeDragging);

/// What a press from `Hovering` leads to (`Branch` only takes two).
pub enum Pressing {
    Nothing(Router<Hovering>),
    Pressed(Router<Pressed>),
    Edge(Router<EdgePressed>),
}

#[transition]
impl Router<Hovering> {
    pub fn moved(mut self, at: Point, hit: Hit, out: &mut impl Sink<Effect>) -> Router<Hovering> {
        self.state_data.hover.track(hit, out);
        if let Some(node) = hit.node {
            out.push(Effect::Move { node, at });
        }
        self
    }
    /// A release nobody pressed for: the hover follows, nothing is delivered.
    pub fn release(mut self, hit: Hit, out: &mut impl Sink<Effect>) -> Router<Hovering> {
        self.state_data.hover.track(hit, out);
        self
    }
    pub fn removed(mut self, n: Node) -> Router<Hovering> {
        self.state_data.hover.forget(n);
        self
    }
    pub fn capture(self, owner: Node) -> Router<Pressed> {
        let hover = self.state_data.hover;
        self.transition_with(PressedData { hover, owner })
    }
    fn press_edge(self, edge: Node, at: Point, clicks: u8, beneath: Option<Node>) -> Router<EdgePressed> {
        let hover = self.state_data.hover;
        self.transition_with(EdgePressedData { hover, edge, at, clicks, beneath })
    }
}

impl Router<Hovering> {
    pub fn press(mut self, at: Point, button: Button, clicks: u8, hit: Hit, out: &mut impl Sink<Effect>) -> Pressing {
        self.state_data.hover.track(hit, out);
        let t = target(button, hit);
        down(t, at, button, clicks, out);
        match t {
            None => Pressing::Nothing(self),
            Some(edge) if button == Button::Left && hit.drags_only => {
                Pressing::Edge(self.press_edge(edge, at, clicks, hit.under))
            }
            Some(owner) => Pressing::Pressed(self.capture(owner)),
        }
    }
}

#[transition]
impl Router<Pressed> {
    pub fn moved(mut self, at: Point, hit: Hit, out: &mut impl Sink<Effect>) -> Router<Pressed> {
        self.state_data.hover.track(hit, out);
        out.push(Effect::Move { node: self.state_data.owner, at });
        self
    }
    pub fn press(
        mut self,
        at: Point,
        button: Button,
        clicks: u8,
        hit: Hit,
        out: &mut impl Sink<Effect>,
    ) -> Router<Pressed> {
        self.state_data.hover.track(hit, out);
        down(target(button, hit), at, button, clicks, out);
        self
    }
    pub fn capture(mut self, owner: Node) -> Router<Pressed> {
        self.state_data.owner = owner;
        self
    }
    pub fn release(self, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) -> Router<Hovering> {
        let PressedData { mut hover, owner } = self.state_data;
        up(owner, at, button, hit, out);
        hover.track(hit, out);
        self.transition_with(HoveringData { hover })
    }
    /// The owner letting go ends the press; another node's request is ignored.
    pub fn uncapture(self, n: Node) -> ::core::result::Result<Router<Hovering>, Router<Pressed>> {
        if n != self.state_data.owner {
            return Err(self);
        }
        let hover = self.state_data.hover;
        Ok(self.transition_with(HoveringData { hover }))
    }
    pub fn removed(mut self, n: Node) -> ::core::result::Result<Router<Hovering>, Router<Pressed>> {
        self.state_data.hover.forget(n);
        self.uncapture(n)
    }
}

#[transition]
impl Router<EdgePressed> {
    pub fn moved(
        mut self,
        at: Point,
        hit: Hit,
        out: &mut impl Sink<Effect>,
    ) -> ::statum::Branch<Router<EdgePressed>, Router<EdgeDragging>> {
        self.state_data.hover.track(hit, out);
        out.push(Effect::Move { node: self.state_data.edge, at });
        if at.dist2(self.state_data.at) >= self.threshold2 { Branch::Second(self.drag()) } else { Branch::First(self) }
    }
    /// Another button: the click under is cancelled.
    pub fn press(
        mut self,
        at: Point,
        button: Button,
        clicks: u8,
        hit: Hit,
        out: &mut impl Sink<Effect>,
    ) -> Router<EdgeDragging> {
        self.state_data.hover.track(hit, out);
        down(target(button, hit), at, button, clicks, out);
        self.drag()
    }
    fn drag(self) -> Router<EdgeDragging> {
        let EdgePressedData { hover, edge, .. } = self.state_data;
        self.transition_with(EdgeDraggingData { hover, edge })
    }
    /// No drag came of it: a left release is a click on what's beneath.
    pub fn release(self, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) -> Router<Hovering> {
        let EdgePressedData { mut hover, edge, at: p0, clicks, beneath } = self.state_data;
        up(edge, at, button, hit, out);
        if button == Button::Left
            && let Some(b) = beneath
        {
            out.push(Effect::Down { node: b, at: p0, button: Button::Left, clicks });
            up(b, at, Button::Left, hit, out);
        }
        hover.track(hit, out);
        self.transition_with(HoveringData { hover })
    }
    pub fn capture(self, owner: Node) -> Router<Pressed> {
        let hover = self.state_data.hover;
        self.transition_with(PressedData { hover, owner })
    }
    pub fn uncapture(self, n: Node) -> ::core::result::Result<Router<Hovering>, Router<EdgePressed>> {
        if n != self.state_data.edge {
            return Err(self);
        }
        let hover = self.state_data.hover;
        Ok(self.transition_with(HoveringData { hover }))
    }
    pub fn removed(mut self, n: Node) -> ::core::result::Result<Router<Hovering>, Router<EdgePressed>> {
        self.state_data.hover.forget(n);
        if self.state_data.beneath == Some(n) {
            self.state_data.beneath = None;
        }
        self.uncapture(n)
    }
}

#[transition]
impl Router<EdgeDragging> {
    pub fn moved(mut self, at: Point, hit: Hit, out: &mut impl Sink<Effect>) -> Router<EdgeDragging> {
        self.state_data.hover.track(hit, out);
        out.push(Effect::Move { node: self.state_data.edge, at });
        self
    }
    pub fn press(
        mut self,
        at: Point,
        button: Button,
        clicks: u8,
        hit: Hit,
        out: &mut impl Sink<Effect>,
    ) -> Router<EdgeDragging> {
        self.state_data.hover.track(hit, out);
        down(target(button, hit), at, button, clicks, out);
        self
    }
    pub fn release(self, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) -> Router<Hovering> {
        let EdgeDraggingData { mut hover, edge } = self.state_data;
        up(edge, at, button, hit, out);
        hover.track(hit, out);
        self.transition_with(HoveringData { hover })
    }
    pub fn capture(self, owner: Node) -> Router<Pressed> {
        let hover = self.state_data.hover;
        self.transition_with(PressedData { hover, owner })
    }
    pub fn uncapture(self, n: Node) -> ::core::result::Result<Router<Hovering>, Router<EdgeDragging>> {
        if n != self.state_data.edge {
            return Err(self);
        }
        let hover = self.state_data.hover;
        Ok(self.transition_with(HoveringData { hover }))
    }
    pub fn removed(mut self, n: Node) -> ::core::result::Result<Router<Hovering>, Router<EdgeDragging>> {
        self.state_data.hover.forget(n);
        self.uncapture(n)
    }
}

#[transition]
impl Router<NativeDrag> {
    pub fn drag_ended(self) -> Router<Outside> {
        self.transition_with(OutsideTok(()))
    }
}

pub struct Pointer {
    /// `None` only while `handle` has moved the state out (statum's builder
    /// makes no cheap placeholder).
    state: Option<router::SomeState>,
}

impl Pointer {
    pub fn typed(&self) -> &router::SomeState {
        self.state.as_ref().expect("a state")
    }
}

fn either<A, B>(r: Result<A, B>, a: fn(A) -> router::SomeState, b: fn(B) -> router::SomeState) -> router::SomeState {
    match r {
        Ok(x) => a(x),
        Err(x) => b(x),
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        Self { state: Some(router::SomeState::Outside(Router::<Outside>::new(config))) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use router::SomeState as S;
        let state = self.state.take().expect("a state");
        // Entering: a move, press or release from outside starts hovering.
        let state = match (state, event) {
            (S::Outside(s), Event::Move { .. } | Event::Press { .. } | Event::Release { .. }) => S::Hovering(s.enter()),
            (s, _) => s,
        };
        self.state = Some(match (state, event) {
            (S::Hovering(s), Event::Move { at, hit }) => S::Hovering(s.moved(at, hit, out)),
            (S::Pressed(s), Event::Move { at, hit }) => S::Pressed(s.moved(at, hit, out)),
            (S::EdgePressed(s), Event::Move { at, hit }) => match s.moved(at, hit, out) {
                Branch::First(s) => S::EdgePressed(s),
                Branch::Second(s) => S::EdgeDragging(s),
            },
            (S::EdgeDragging(s), Event::Move { at, hit }) => S::EdgeDragging(s.moved(at, hit, out)),
            (S::Hovering(s), Event::Press { at, button, clicks, hit }) => match s.press(at, button, clicks, hit, out) {
                Pressing::Nothing(s) => S::Hovering(s),
                Pressing::Pressed(s) => S::Pressed(s),
                Pressing::Edge(s) => S::EdgePressed(s),
            },
            (S::Pressed(s), Event::Press { at, button, clicks, hit }) => {
                S::Pressed(s.press(at, button, clicks, hit, out))
            }
            (S::EdgePressed(s), Event::Press { at, button, clicks, hit }) => {
                S::EdgeDragging(s.press(at, button, clicks, hit, out))
            }
            (S::EdgeDragging(s), Event::Press { at, button, clicks, hit }) => {
                S::EdgeDragging(s.press(at, button, clicks, hit, out))
            }
            (S::Hovering(s), Event::Release { hit, .. }) => S::Hovering(s.release(hit, out)),
            (S::Pressed(s), Event::Release { at, button, hit }) => S::Hovering(s.release(at, button, hit, out)),
            (S::EdgePressed(s), Event::Release { at, button, hit }) => S::Hovering(s.release(at, button, hit, out)),
            (S::EdgeDragging(s), Event::Release { at, button, hit }) => S::Hovering(s.release(at, button, hit, out)),
            (S::Hovering(s), Event::Leave) => S::Outside(s.leave(out)),
            (S::Pressed(s), Event::Leave) => S::Outside(s.leave(out)),
            (S::EdgePressed(s), Event::Leave) => S::Outside(s.leave(out)),
            (S::EdgeDragging(s), Event::Leave) => S::Outside(s.leave(out)),
            (S::Hovering(s), Event::Capture(n)) => S::Pressed(s.capture(n)),
            (S::Pressed(s), Event::Capture(n)) => S::Pressed(s.capture(n)),
            (S::EdgePressed(s), Event::Capture(n)) => S::Pressed(s.capture(n)),
            (S::EdgeDragging(s), Event::Capture(n)) => S::Pressed(s.capture(n)),
            (S::Pressed(s), Event::Uncapture(n)) => either(s.uncapture(n), S::Hovering, S::Pressed),
            (S::EdgePressed(s), Event::Uncapture(n)) => either(s.uncapture(n), S::Hovering, S::EdgePressed),
            (S::EdgeDragging(s), Event::Uncapture(n)) => either(s.uncapture(n), S::Hovering, S::EdgeDragging),
            (S::Outside(s), Event::StartDrag(_)) => S::NativeDrag(s.start_drag()),
            (S::Hovering(s), Event::StartDrag(_)) => S::NativeDrag(s.start_drag(out)),
            (S::Pressed(s), Event::StartDrag(_)) => S::NativeDrag(s.start_drag(out)),
            (S::EdgePressed(s), Event::StartDrag(_)) => S::NativeDrag(s.start_drag(out)),
            (S::EdgeDragging(s), Event::StartDrag(_)) => S::NativeDrag(s.start_drag(out)),
            (S::NativeDrag(s), Event::DragEnded) => S::Outside(s.drag_ended()),
            (S::Hovering(s), Event::Removed(n)) => S::Hovering(s.removed(n)),
            (S::Pressed(s), Event::Removed(n)) => either(s.removed(n), S::Hovering, S::Pressed),
            (S::EdgePressed(s), Event::Removed(n)) => either(s.removed(n), S::Hovering, S::EdgePressed),
            (S::EdgeDragging(s), Event::Removed(n)) => either(s.removed(n), S::Hovering, S::EdgeDragging),
            (s, _) => s,
        });
    }

    fn state(&self) -> StateName {
        use router::SomeState as S;
        match self.typed() {
            S::Outside(_) => StateName::Outside,
            S::Hovering(_) => StateName::Hovering,
            S::Pressed(_) => StateName::Pressed,
            S::EdgePressed(_) => StateName::DragsOnlyPressed,
            S::EdgeDragging(_) => StateName::DragsOnlyDragging,
            S::NativeDrag(_) => StateName::NativeDrag,
        }
    }
}
