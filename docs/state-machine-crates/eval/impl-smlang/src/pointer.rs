//! `pointer` with smlang 0.8.0. smlang is flat, so the `Inside` superstate
//! is emulated: its hover pair rides in every substate's data (`Hovering`'s
//! `Hover`, `Owned` for `Pressed` and `DragsOnlyDragging`, `EdgePress` for
//! `DragsOnlyPressed`), and its rows are repeated once per substate data
//! type, grouped with `A | B` patterns where the data types match.
//! `Outside` and `NativeDrag` carry nothing. Effects go through smlang's
//! `temporary_context`, declared as `&mut impl Sink<Effect>`, so the sink
//! stays generic down into every guard and action.

use smlang::statemachine;
use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

/// `Move` event data.
pub type Motion = (Point, Hit);
/// `Press` event data: at, button, clicks, hit.
pub type Pr = (Point, Button, u8, Hit);
/// `Release` event data.
pub type Rl = (Point, Button, Hit);

/// The hover pair of the `Inside` superstate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    pub hot: Option<Node>,
    pub under: Option<Node>,
}

/// `Pressed` and `DragsOnlyDragging`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owned {
    pub hover: Hover,
    pub owner: Node,
}

/// `DragsOnlyPressed`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgePress {
    pub hover: Hover,
    pub edge: Node,
    pub at: Point,
    pub clicks: u8,
    pub beneath: Option<Node>,
}

/// The generated machine, kept private: its `new_with_state` and
/// `context_mut` would let callers forge a state.
mod fsm {
    use super::*;

    statemachine! {
        temporary_context: &mut impl Sink<Effect>,
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            // Entering `Inside`: as `Hovering` with an empty hover.
            *Outside + Move(Motion) / enter_move = Hovering(Hover),
            Outside + Press(Pr) [no_target] / enter_track_press = Hovering(Hover),
            Outside + Press(Pr) [edge_press] / enter_press_edge = DragsOnlyPressed(EdgePress),
            Outside + Press(Pr) / enter_press_owner = Pressed(Owned),
            Outside + Release(Rl) / enter_release = Hovering(Hover),
            Hovering(Hover) + Move(Motion) / hover_move = _,
            Hovering(Hover) + Press(Pr) [no_target_hovering] / track_press = _,
            Hovering(Hover) + Press(Pr) [edge_press_hovering] / press_edge = DragsOnlyPressed(EdgePress),
            Hovering(Hover) + Press(Pr) / press_owner = Pressed(Owned),
            Hovering(Hover) + Release(Rl) / track_release = _,
            Pressed(Owned) | DragsOnlyDragging(Owned) + Move(Motion) / owner_move = _,
            Pressed(Owned) | DragsOnlyDragging(Owned) + Press(Pr) / owner_press = _,
            Pressed(Owned) | DragsOnlyDragging(Owned) + Release(Rl) / owner_release = Hovering(Hover),
            DragsOnlyPressed(EdgePress) + Move(Motion) [past_threshold] / edge_drag = DragsOnlyDragging(Owned),
            DragsOnlyPressed(EdgePress) + Move(Motion) / edge_move = _,
            DragsOnlyPressed(EdgePress) + Press(Pr) / edge_press_more = DragsOnlyDragging(Owned),
            DragsOnlyPressed(EdgePress) + Release(Rl) / edge_release = Hovering(Hover),
            // `Inside`'s own rows, once per substate data type.
            Hovering(Hover) + Leave / exit_hovering = Outside,
            Pressed(Owned) | DragsOnlyDragging(Owned) + Leave / exit_owner = Outside,
            DragsOnlyPressed(EdgePress) + Leave / exit_edge = Outside,
            Hovering(Hover) + StartDrag / exit_hovering = NativeDrag,
            Pressed(Owned) | DragsOnlyDragging(Owned) + StartDrag / exit_owner = NativeDrag,
            DragsOnlyPressed(EdgePress) + StartDrag / exit_edge = NativeDrag,
            Hovering(Hover) + Capture(Node) / capture = Pressed(Owned),
            Pressed(Owned) | DragsOnlyDragging(Owned) + Capture(Node) / capture_owner = Pressed(Owned),
            DragsOnlyPressed(EdgePress) + Capture(Node) / capture_edge = Pressed(Owned),
            Pressed(Owned) | DragsOnlyDragging(Owned) + Uncapture(Node) [owned_by] / release_owner = Hovering(Hover),
            DragsOnlyPressed(EdgePress) + Uncapture(Node) [edge_is] / release_edge = Hovering(Hover),
            Hovering(Hover) + Removed(Node) / forget = _,
            Pressed(Owned) | DragsOnlyDragging(Owned) + Removed(Node) [owned_by] / forget_owner = Hovering(Hover),
            Pressed(Owned) | DragsOnlyDragging(Owned) + Removed(Node) / forget_other = _,
            DragsOnlyPressed(EdgePress) + Removed(Node) [edge_is] / forget_edge = Hovering(Hover),
            DragsOnlyPressed(EdgePress) + Removed(Node) / forget_beneath = _,
            Outside + StartDrag = NativeDrag,
            NativeDrag + DragEnded = Outside,
        }
    }
}

pub use fsm::{Error, Events, States};
use fsm::{StateMachine, StateMachineContext};

impl Hover {
    fn track(mut self, hit: Hit, out: &mut impl Sink<Effect>) -> Hover {
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
        self
    }

    /// Exit `Inside`: clear the hover.
    fn exit(self, out: &mut impl Sink<Effect>) -> R {
        if let Some(u) = self.under {
            out.push(Effect::UnderOff(u));
        }
        if let Some(n) = self.hot {
            out.push(Effect::HoverOff(n));
        }
        Ok(())
    }

    fn forget(mut self, n: Node) -> Hover {
        if self.hot == Some(n) {
            self.hot = None;
        }
        if self.under == Some(n) {
            self.under = None;
        }
        self
    }

    /// Tracks, then delivers the press to its target (if any).
    fn press(self, (at, button, clicks, hit): Pr, out: &mut impl Sink<Effect>) -> (Hover, Option<Node>) {
        let hover = self.track(hit, out);
        let t = target(button, hit);
        if let Some(node) = t {
            out.push(Effect::Down { node, at, button, clicks });
        }
        (hover, t)
    }
}

fn target(button: Button, hit: Hit) -> Option<Node> {
    if button == Button::Left { hit.node } else { hit.beneath() }
}

fn no_target((_, button, _, hit): &Pr) -> bool {
    target(*button, *hit).is_none()
}

fn edge_press((_, button, _, hit): &Pr) -> bool {
    *button == Button::Left && hit.drags_only
}

fn up(node: Node, (at, button, hit): Rl, out: &mut impl Sink<Effect>) {
    out.push(Effect::Up { node, at, button, inside: hit.names(node) });
}

/// The context: the drag threshold, squared.
pub struct Router {
    threshold2: i64,
}

type R<T = ()> = Result<T, ()>;

impl StateMachineContext for Router {
    fn no_target(&self, _: &mut impl Sink<Effect>, p: &Pr) -> R<bool> {
        Ok(no_target(p))
    }
    fn edge_press(&self, _: &mut impl Sink<Effect>, p: &Pr) -> R<bool> {
        Ok(edge_press(p))
    }
    fn no_target_hovering(&self, _: &mut impl Sink<Effect>, _: &Hover, p: &Pr) -> R<bool> {
        Ok(no_target(p))
    }
    fn edge_press_hovering(&self, _: &mut impl Sink<Effect>, _: &Hover, p: &Pr) -> R<bool> {
        Ok(edge_press(p))
    }
    fn past_threshold(&self, _: &mut impl Sink<Effect>, p: &EdgePress, &(at, _): &Motion) -> R<bool> {
        Ok(at.dist2(p.at) >= self.threshold2)
    }
    fn owned_by(&self, _: &mut impl Sink<Effect>, o: &Owned, n: &Node) -> R<bool> {
        Ok(o.owner == *n)
    }
    fn edge_is(&self, _: &mut impl Sink<Effect>, p: &EdgePress, n: &Node) -> R<bool> {
        Ok(p.edge == *n)
    }

    fn enter_move(&mut self, out: &mut impl Sink<Effect>, m: Motion) -> R<Hover> {
        self.hover_move(out, &Hover::default(), m)
    }
    fn enter_track_press(&mut self, out: &mut impl Sink<Effect>, p: Pr) -> R<Hover> {
        self.track_press(out, &Hover::default(), p)
    }
    fn enter_press_edge(&mut self, out: &mut impl Sink<Effect>, p: Pr) -> R<EdgePress> {
        self.press_edge(out, &Hover::default(), p)
    }
    fn enter_press_owner(&mut self, out: &mut impl Sink<Effect>, p: Pr) -> R<Owned> {
        self.press_owner(out, &Hover::default(), p)
    }
    fn enter_release(&mut self, out: &mut impl Sink<Effect>, r: Rl) -> R<Hover> {
        self.track_release(out, &Hover::default(), r)
    }
    fn hover_move(&mut self, out: &mut impl Sink<Effect>, h: &Hover, (at, hit): Motion) -> R<Hover> {
        let hover = h.track(hit, out);
        if let Some(node) = hit.node {
            out.push(Effect::Move { node, at });
        }
        Ok(hover)
    }
    fn track_press(&mut self, out: &mut impl Sink<Effect>, h: &Hover, p: Pr) -> R<Hover> {
        Ok(h.press(p, out).0)
    }
    fn press_edge(&mut self, out: &mut impl Sink<Effect>, h: &Hover, p: Pr) -> R<EdgePress> {
        let (hover, edge) = h.press(p, out);
        Ok(EdgePress { hover, edge: edge.ok_or(())?, at: p.0, clicks: p.2, beneath: p.3.under })
    }
    fn press_owner(&mut self, out: &mut impl Sink<Effect>, h: &Hover, p: Pr) -> R<Owned> {
        let (hover, owner) = h.press(p, out);
        Ok(Owned { hover, owner: owner.ok_or(())? })
    }
    fn track_release(&mut self, out: &mut impl Sink<Effect>, h: &Hover, (_, _, hit): Rl) -> R<Hover> {
        Ok(h.track(hit, out))
    }
    fn owner_move(&mut self, out: &mut impl Sink<Effect>, o: &Owned, (at, hit): Motion) -> R<Owned> {
        let hover = o.hover.track(hit, out);
        out.push(Effect::Move { node: o.owner, at });
        Ok(Owned { hover, ..*o })
    }
    fn owner_press(&mut self, out: &mut impl Sink<Effect>, o: &Owned, p: Pr) -> R<Owned> {
        Ok(Owned { hover: o.hover.press(p, out).0, ..*o })
    }
    fn owner_release(&mut self, out: &mut impl Sink<Effect>, o: &Owned, r: Rl) -> R<Hover> {
        up(o.owner, r, out);
        Ok(o.hover.track(r.2, out))
    }
    fn edge_drag(&mut self, out: &mut impl Sink<Effect>, p: &EdgePress, (at, hit): Motion) -> R<Owned> {
        let hover = p.hover.track(hit, out);
        out.push(Effect::Move { node: p.edge, at });
        Ok(Owned { hover, owner: p.edge })
    }
    fn edge_move(&mut self, out: &mut impl Sink<Effect>, p: &EdgePress, (at, hit): Motion) -> R<EdgePress> {
        let hover = p.hover.track(hit, out);
        out.push(Effect::Move { node: p.edge, at });
        Ok(EdgePress { hover, ..*p })
    }
    fn edge_press_more(&mut self, out: &mut impl Sink<Effect>, p: &EdgePress, e: Pr) -> R<Owned> {
        // Another button: no click under any more.
        Ok(Owned { hover: p.hover.press(e, out).0, owner: p.edge })
    }
    fn edge_release(&mut self, out: &mut impl Sink<Effect>, p: &EdgePress, r: Rl) -> R<Hover> {
        up(p.edge, r, out);
        if r.1 == Button::Left
            && let Some(b) = p.beneath
        {
            out.push(Effect::Down { node: b, at: p.at, button: Button::Left, clicks: p.clicks });
            up(b, (r.0, Button::Left, r.2), out);
        }
        Ok(p.hover.track(r.2, out))
    }
    fn exit_hovering(&mut self, out: &mut impl Sink<Effect>, h: &Hover) -> R {
        h.exit(out)
    }
    fn exit_owner(&mut self, out: &mut impl Sink<Effect>, o: &Owned) -> R {
        o.hover.exit(out)
    }
    fn exit_edge(&mut self, out: &mut impl Sink<Effect>, p: &EdgePress) -> R {
        p.hover.exit(out)
    }
    fn capture(&mut self, _: &mut impl Sink<Effect>, h: &Hover, owner: Node) -> R<Owned> {
        Ok(Owned { hover: *h, owner })
    }
    fn capture_owner(&mut self, _: &mut impl Sink<Effect>, o: &Owned, owner: Node) -> R<Owned> {
        Ok(Owned { hover: o.hover, owner })
    }
    fn capture_edge(&mut self, _: &mut impl Sink<Effect>, p: &EdgePress, owner: Node) -> R<Owned> {
        Ok(Owned { hover: p.hover, owner })
    }
    fn release_owner(&mut self, _: &mut impl Sink<Effect>, o: &Owned, _: Node) -> R<Hover> {
        Ok(o.hover)
    }
    fn release_edge(&mut self, _: &mut impl Sink<Effect>, p: &EdgePress, _: Node) -> R<Hover> {
        Ok(p.hover)
    }
    fn forget(&mut self, _: &mut impl Sink<Effect>, h: &Hover, n: Node) -> R<Hover> {
        Ok(h.forget(n))
    }
    fn forget_owner(&mut self, _: &mut impl Sink<Effect>, o: &Owned, n: Node) -> R<Hover> {
        Ok(o.hover.forget(n))
    }
    fn forget_other(&mut self, _: &mut impl Sink<Effect>, o: &Owned, n: Node) -> R<Owned> {
        Ok(Owned { hover: o.hover.forget(n), ..*o })
    }
    fn forget_edge(&mut self, _: &mut impl Sink<Effect>, p: &EdgePress, n: Node) -> R<Hover> {
        Ok(p.hover.forget(n))
    }
    fn forget_beneath(&mut self, _: &mut impl Sink<Effect>, p: &EdgePress, n: Node) -> R<EdgePress> {
        let beneath = if p.beneath == Some(n) { None } else { p.beneath };
        Ok(EdgePress { hover: p.hover.forget(n), beneath, ..*p })
    }
}

pub struct Pointer {
    sm: StateMachine<Router>,
}

impl Pointer {
    /// The generated `process_event`: the typed transition API.
    pub fn process_event(&mut self, out: &mut impl Sink<Effect>, event: Events) -> Result<&States, Error> {
        self.sm.process_event(out, event)
    }

    /// The generated state.
    pub fn state_enum(&self) -> &States {
        self.sm.state()
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        Self { sm: StateMachine::new(Router { threshold2: t * t }) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let event = match event {
            Event::Move { at, hit } => Events::Move((at, hit)),
            Event::Press { at, button, clicks, hit } => Events::Press((at, button, clicks, hit)),
            Event::Release { at, button, hit } => Events::Release((at, button, hit)),
            Event::Leave => Events::Leave,
            Event::Capture(n) => Events::Capture(n),
            Event::Uncapture(n) => Events::Uncapture(n),
            Event::StartDrag(_) => Events::StartDrag,
            Event::DragEnded => Events::DragEnded,
            Event::Removed(n) => Events::Removed(n),
        };
        // Undeclared (state, event) pairs and failed guards are `Err`: ignored.
        let _ = self.sm.process_event(out, event);
    }

    fn state(&self) -> StateName {
        match self.sm.state() {
            States::Outside => StateName::Outside,
            States::Hovering(_) => StateName::Hovering,
            States::Pressed(_) => StateName::Pressed,
            States::DragsOnlyPressed(_) => StateName::DragsOnlyPressed,
            States::DragsOnlyDragging(_) => StateName::DragsOnlyDragging,
            States::NativeDrag => StateName::NativeDrag,
        }
    }
}
