//! `pointer` as hand-written typestate: one type per state, transitions
//! that consume it, private fields, and the runtime wrapper `Any` that
//! routes events. Only `Pressed`, `EdgePressed` and `EdgeDragging` can emit
//! an `Up`; `NativeDrag` holds no owner.

use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

/// The hover pair every inside state carries.
#[derive(Clone, Copy, Debug, Default)]
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

#[derive(Debug)]
pub struct Outside(());
#[derive(Debug)]
pub struct Hovering {
    hover: Hover,
}
#[derive(Debug)]
pub struct Pressed {
    hover: Hover,
    owner: Node,
}
#[derive(Debug)]
pub struct EdgePressed {
    hover: Hover,
    edge: Node,
    at: Point,
    clicks: u8,
    beneath: Option<Node>,
}
#[derive(Debug)]
pub struct EdgeDragging {
    hover: Hover,
    edge: Node,
}
#[derive(Debug)]
pub struct NativeDrag(());

/// What a press from `Hovering` leads to.
pub enum Pressing {
    Nothing(Hovering),
    Pressed(Pressed),
    Edge(EdgePressed),
}

/// What a move during an edge press leads to.
pub enum EdgeMoved {
    Still(EdgePressed),
    Drag(EdgeDragging),
}

fn target(button: Button, hit: Hit) -> Option<Node> {
    if button == Button::Left { hit.node } else { hit.beneath() }
}

fn down(t: Option<Node>, at: Point, button: Button, clicks: u8, out: &mut impl Sink<Effect>) {
    if let Some(node) = t {
        out.push(Effect::Down { node, at, button, clicks });
    }
}

impl Outside {
    pub fn new() -> Self {
        Outside(())
    }
    pub fn enter(self) -> Hovering {
        Hovering { hover: Hover::default() }
    }
    pub fn start_drag(self) -> NativeDrag {
        NativeDrag(())
    }
}

impl Default for Outside {
    fn default() -> Self {
        Self::new()
    }
}

impl Hovering {
    pub fn hover(&self) -> &Hover {
        &self.hover
    }
    pub fn moved(mut self, at: Point, hit: Hit, out: &mut impl Sink<Effect>) -> Hovering {
        self.hover.track(hit, out);
        if let Some(node) = hit.node {
            out.push(Effect::Move { node, at });
        }
        self
    }
    pub fn press(mut self, at: Point, button: Button, clicks: u8, hit: Hit, out: &mut impl Sink<Effect>) -> Pressing {
        self.hover.track(hit, out);
        let t = target(button, hit);
        down(t, at, button, clicks, out);
        match t {
            None => Pressing::Nothing(self),
            Some(edge) if button == Button::Left && hit.drags_only => {
                Pressing::Edge(EdgePressed { hover: self.hover, edge, at, clicks, beneath: hit.under })
            }
            Some(owner) => Pressing::Pressed(Pressed { hover: self.hover, owner }),
        }
    }
    /// A release nobody pressed for: the hover follows, nothing is delivered.
    pub fn release(mut self, hit: Hit, out: &mut impl Sink<Effect>) -> Hovering {
        self.hover.track(hit, out);
        self
    }
    pub fn leave(self, out: &mut impl Sink<Effect>) -> Outside {
        self.hover.clear(out);
        Outside(())
    }
    pub fn capture(self, owner: Node) -> Pressed {
        Pressed { hover: self.hover, owner }
    }
    pub fn start_drag(self, out: &mut impl Sink<Effect>) -> NativeDrag {
        self.hover.clear(out);
        NativeDrag(())
    }
    pub fn removed(mut self, n: Node) -> Hovering {
        self.hover.forget(n);
        self
    }
}

impl Pressed {
    pub fn owner(&self) -> Node {
        self.owner
    }
    pub fn moved(mut self, at: Point, hit: Hit, out: &mut impl Sink<Effect>) -> Pressed {
        self.hover.track(hit, out);
        out.push(Effect::Move { node: self.owner, at });
        self
    }
    pub fn press(mut self, at: Point, button: Button, clicks: u8, hit: Hit, out: &mut impl Sink<Effect>) -> Pressed {
        self.hover.track(hit, out);
        down(target(button, hit), at, button, clicks, out);
        self
    }
    pub fn release(mut self, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) -> Hovering {
        out.push(Effect::Up { node: self.owner, at, button, inside: hit.names(self.owner) });
        self.hover.track(hit, out);
        Hovering { hover: self.hover }
    }
    pub fn leave(self, out: &mut impl Sink<Effect>) -> Outside {
        self.hover.clear(out);
        Outside(())
    }
    pub fn capture(self, owner: Node) -> Pressed {
        Pressed { hover: self.hover, owner }
    }
    /// The owner letting go ends the press; another node's request is ignored.
    pub fn uncapture(self, n: Node) -> Result<Hovering, Pressed> {
        if n == self.owner { Ok(Hovering { hover: self.hover }) } else { Err(self) }
    }
    pub fn start_drag(self, out: &mut impl Sink<Effect>) -> NativeDrag {
        self.hover.clear(out);
        NativeDrag(())
    }
    pub fn removed(mut self, n: Node) -> Result<Hovering, Pressed> {
        self.hover.forget(n);
        if n == self.owner { Ok(Hovering { hover: self.hover }) } else { Err(self) }
    }
}

impl EdgePressed {
    pub fn edge(&self) -> Node {
        self.edge
    }
    pub fn press_point(&self) -> Point {
        self.at
    }
    pub fn beneath(&self) -> Option<Node> {
        self.beneath
    }
    pub fn moved(mut self, at: Point, hit: Hit, threshold2: i64, out: &mut impl Sink<Effect>) -> EdgeMoved {
        self.hover.track(hit, out);
        out.push(Effect::Move { node: self.edge, at });
        if at.dist2(self.at) >= threshold2 {
            EdgeMoved::Drag(EdgeDragging { hover: self.hover, edge: self.edge })
        } else {
            EdgeMoved::Still(self)
        }
    }
    /// Another button: the click under is cancelled.
    pub fn press(
        mut self,
        at: Point,
        button: Button,
        clicks: u8,
        hit: Hit,
        out: &mut impl Sink<Effect>,
    ) -> EdgeDragging {
        self.hover.track(hit, out);
        down(target(button, hit), at, button, clicks, out);
        EdgeDragging { hover: self.hover, edge: self.edge }
    }
    /// No drag came of it: a left release is a click on what's beneath.
    pub fn release(mut self, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) -> Hovering {
        out.push(Effect::Up { node: self.edge, at, button, inside: hit.names(self.edge) });
        if button == Button::Left
            && let Some(b) = self.beneath
        {
            out.push(Effect::Down { node: b, at: self.at, button: Button::Left, clicks: self.clicks });
            out.push(Effect::Up { node: b, at, button: Button::Left, inside: hit.names(b) });
        }
        self.hover.track(hit, out);
        Hovering { hover: self.hover }
    }
    pub fn leave(self, out: &mut impl Sink<Effect>) -> Outside {
        self.hover.clear(out);
        Outside(())
    }
    pub fn capture(self, owner: Node) -> Pressed {
        Pressed { hover: self.hover, owner }
    }
    pub fn uncapture(self, n: Node) -> Result<Hovering, EdgePressed> {
        if n == self.edge { Ok(Hovering { hover: self.hover }) } else { Err(self) }
    }
    pub fn start_drag(self, out: &mut impl Sink<Effect>) -> NativeDrag {
        self.hover.clear(out);
        NativeDrag(())
    }
    pub fn removed(mut self, n: Node) -> Result<Hovering, EdgePressed> {
        self.hover.forget(n);
        if n == self.edge {
            return Ok(Hovering { hover: self.hover });
        }
        if self.beneath == Some(n) {
            self.beneath = None;
        }
        Err(self)
    }
}

impl EdgeDragging {
    pub fn edge(&self) -> Node {
        self.edge
    }
    pub fn moved(mut self, at: Point, hit: Hit, out: &mut impl Sink<Effect>) -> EdgeDragging {
        self.hover.track(hit, out);
        out.push(Effect::Move { node: self.edge, at });
        self
    }
    pub fn press(
        mut self,
        at: Point,
        button: Button,
        clicks: u8,
        hit: Hit,
        out: &mut impl Sink<Effect>,
    ) -> EdgeDragging {
        self.hover.track(hit, out);
        down(target(button, hit), at, button, clicks, out);
        self
    }
    pub fn release(mut self, at: Point, button: Button, hit: Hit, out: &mut impl Sink<Effect>) -> Hovering {
        out.push(Effect::Up { node: self.edge, at, button, inside: hit.names(self.edge) });
        self.hover.track(hit, out);
        Hovering { hover: self.hover }
    }
    pub fn leave(self, out: &mut impl Sink<Effect>) -> Outside {
        self.hover.clear(out);
        Outside(())
    }
    pub fn capture(self, owner: Node) -> Pressed {
        Pressed { hover: self.hover, owner }
    }
    pub fn uncapture(self, n: Node) -> Result<Hovering, EdgeDragging> {
        if n == self.edge { Ok(Hovering { hover: self.hover }) } else { Err(self) }
    }
    pub fn start_drag(self, out: &mut impl Sink<Effect>) -> NativeDrag {
        self.hover.clear(out);
        NativeDrag(())
    }
    pub fn removed(mut self, n: Node) -> Result<Hovering, EdgeDragging> {
        self.hover.forget(n);
        if n == self.edge { Ok(Hovering { hover: self.hover }) } else { Err(self) }
    }
}

impl NativeDrag {
    pub fn drag_ended(self) -> Outside {
        Outside(())
    }
}

/// The runtime wrapper: one variant per state type.
#[derive(Debug)]
pub enum Any {
    Outside(Outside),
    Hovering(Hovering),
    Pressed(Pressed),
    EdgePressed(EdgePressed),
    EdgeDragging(EdgeDragging),
    NativeDrag(NativeDrag),
}

impl From<Pressing> for Any {
    fn from(p: Pressing) -> Any {
        match p {
            Pressing::Nothing(s) => Any::Hovering(s),
            Pressing::Pressed(s) => Any::Pressed(s),
            Pressing::Edge(s) => Any::EdgePressed(s),
        }
    }
}

fn either<A, B>(r: Result<A, B>, a: fn(A) -> Any, b: fn(B) -> Any) -> Any {
    match r {
        Ok(x) => a(x),
        Err(x) => b(x),
    }
}

pub struct Pointer {
    threshold2: i64,
    state: Any,
}

impl Pointer {
    pub fn typed(&self) -> &Any {
        &self.state
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        Self { threshold2: t * t, state: Any::Outside(Outside::new()) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use Any as A;
        let state = std::mem::replace(&mut self.state, A::NativeDrag(NativeDrag(())));
        // Entering: a move, press or release from outside starts hovering.
        let state = match (state, event) {
            (A::Outside(s), Event::Move { .. } | Event::Press { .. } | Event::Release { .. }) => A::Hovering(s.enter()),
            (s, _) => s,
        };
        self.state = match (state, event) {
            (A::Hovering(s), Event::Move { at, hit }) => A::Hovering(s.moved(at, hit, out)),
            (A::Pressed(s), Event::Move { at, hit }) => A::Pressed(s.moved(at, hit, out)),
            (A::EdgePressed(s), Event::Move { at, hit }) => match s.moved(at, hit, self.threshold2, out) {
                EdgeMoved::Still(s) => A::EdgePressed(s),
                EdgeMoved::Drag(s) => A::EdgeDragging(s),
            },
            (A::EdgeDragging(s), Event::Move { at, hit }) => A::EdgeDragging(s.moved(at, hit, out)),
            (A::Hovering(s), Event::Press { at, button, clicks, hit }) => s.press(at, button, clicks, hit, out).into(),
            (A::Pressed(s), Event::Press { at, button, clicks, hit }) => {
                A::Pressed(s.press(at, button, clicks, hit, out))
            }
            (A::EdgePressed(s), Event::Press { at, button, clicks, hit }) => {
                A::EdgeDragging(s.press(at, button, clicks, hit, out))
            }
            (A::EdgeDragging(s), Event::Press { at, button, clicks, hit }) => {
                A::EdgeDragging(s.press(at, button, clicks, hit, out))
            }
            (A::Hovering(s), Event::Release { hit, .. }) => A::Hovering(s.release(hit, out)),
            (A::Pressed(s), Event::Release { at, button, hit }) => A::Hovering(s.release(at, button, hit, out)),
            (A::EdgePressed(s), Event::Release { at, button, hit }) => A::Hovering(s.release(at, button, hit, out)),
            (A::EdgeDragging(s), Event::Release { at, button, hit }) => A::Hovering(s.release(at, button, hit, out)),
            (A::Hovering(s), Event::Leave) => A::Outside(s.leave(out)),
            (A::Pressed(s), Event::Leave) => A::Outside(s.leave(out)),
            (A::EdgePressed(s), Event::Leave) => A::Outside(s.leave(out)),
            (A::EdgeDragging(s), Event::Leave) => A::Outside(s.leave(out)),
            (A::Hovering(s), Event::Capture(n)) => A::Pressed(s.capture(n)),
            (A::Pressed(s), Event::Capture(n)) => A::Pressed(s.capture(n)),
            (A::EdgePressed(s), Event::Capture(n)) => A::Pressed(s.capture(n)),
            (A::EdgeDragging(s), Event::Capture(n)) => A::Pressed(s.capture(n)),
            (A::Pressed(s), Event::Uncapture(n)) => either(s.uncapture(n), A::Hovering, A::Pressed),
            (A::EdgePressed(s), Event::Uncapture(n)) => either(s.uncapture(n), A::Hovering, A::EdgePressed),
            (A::EdgeDragging(s), Event::Uncapture(n)) => either(s.uncapture(n), A::Hovering, A::EdgeDragging),
            (A::Outside(s), Event::StartDrag(_)) => A::NativeDrag(s.start_drag()),
            (A::Hovering(s), Event::StartDrag(_)) => A::NativeDrag(s.start_drag(out)),
            (A::Pressed(s), Event::StartDrag(_)) => A::NativeDrag(s.start_drag(out)),
            (A::EdgePressed(s), Event::StartDrag(_)) => A::NativeDrag(s.start_drag(out)),
            (A::EdgeDragging(s), Event::StartDrag(_)) => A::NativeDrag(s.start_drag(out)),
            (A::NativeDrag(s), Event::DragEnded) => A::Outside(s.drag_ended()),
            (A::Hovering(s), Event::Removed(n)) => A::Hovering(s.removed(n)),
            (A::Pressed(s), Event::Removed(n)) => either(s.removed(n), A::Hovering, A::Pressed),
            (A::EdgePressed(s), Event::Removed(n)) => either(s.removed(n), A::Hovering, A::EdgePressed),
            (A::EdgeDragging(s), Event::Removed(n)) => either(s.removed(n), A::Hovering, A::EdgeDragging),
            (s, _) => s,
        };
    }

    fn state(&self) -> StateName {
        match self.state {
            Any::Outside(_) => StateName::Outside,
            Any::Hovering(_) => StateName::Hovering,
            Any::Pressed(_) => StateName::Pressed,
            Any::EdgePressed(_) => StateName::DragsOnlyPressed,
            Any::EdgeDragging(_) => StateName::DragsOnlyDragging,
            Any::NativeDrag(_) => StateName::NativeDrag,
        }
    }
}
