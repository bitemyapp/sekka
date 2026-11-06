//! `pointer` with stateforward-sml 2.0.0: `Inside` is a child table
//! (`state<Inside>`) of the window's table. Its entry starts the hover
//! afresh and its exit clears it; the hover itself lives in the context
//! (a composite state carries no payload). The press owner and the edge
//! press are state payloads. Effects go through the per-call temporary
//! context, a `dyn Sink`.

use crate::OneDeferred;
use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

pub type Out<'a> = &'a mut dyn Sink<Effect>;

#[derive(Clone, Copy)]
pub struct Move {
    pub at: Point,
    pub hit: Hit,
}
#[derive(Clone, Copy)]
pub struct Press {
    pub at: Point,
    pub button: Button,
    pub clicks: u8,
    pub hit: Hit,
}
#[derive(Clone, Copy)]
pub struct Release {
    pub at: Point,
    pub button: Button,
    pub hit: Hit,
}
pub struct Leave;
pub struct Capture(pub Node);
pub struct Uncapture(pub Node);
pub struct StartDrag;
pub struct DragEnded;
pub struct Removed(pub Node);

/// A drags-only node's left press, before it became a drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgePress {
    pub edge: Node,
    pub at: Point,
    pub clicks: u8,
    pub beneath: Option<Node>,
}

sml::sml! {
    Inside[temporary_context: &mut dyn Sink<Effect>] {
        *"hovering"_s + event<Move> / hover_move,
         "hovering"_s + event<Press> [takes_edge_press] / press_edge = DragsOnlyPressed(EdgePress),
         "hovering"_s + event<Press> [has_target] / press_owner = Pressed(Node),
         "hovering"_s + event<Press> / press_nothing,
         "hovering"_s + event<Release> / release_nothing,
         "hovering"_s + event<Capture> / capture = Pressed(Node),
         Pressed(Node) | DragsOnlyDragging(Node) + event<Move> / owner_move,
         Pressed(Node) | DragsOnlyDragging(Node) + event<Press> / owner_press,
         Pressed(Node) | DragsOnlyDragging(Node) + event<Release> / owner_release = "hovering"_s,
         Pressed(Node) | DragsOnlyDragging(Node) + event<Capture> / recapture = Pressed(Node),
         Pressed(Node) | DragsOnlyDragging(Node) + event<Uncapture> [uncaptures_owner] = "hovering"_s,
         Pressed(Node) | DragsOnlyDragging(Node) + event<Removed> [removes_owner] / forget_owner = "hovering"_s,
         DragsOnlyPressed(EdgePress) + event<Move> [past_threshold] / edge_move_drags = DragsOnlyDragging(Node),
         DragsOnlyPressed(EdgePress) + event<Move> / edge_move,
         DragsOnlyPressed(EdgePress) + event<Press> / edge_press_drags = DragsOnlyDragging(Node),
         DragsOnlyPressed(EdgePress) + event<Release> / edge_release = "hovering"_s,
         DragsOnlyPressed(EdgePress) + event<Capture> / capture_edge = Pressed(Node),
         DragsOnlyPressed(EdgePress) + event<Uncapture> [uncaptures_edge] = "hovering"_s,
         DragsOnlyPressed(EdgePress) + event<Removed> [removes_edge] / forget_edge = "hovering"_s,
         DragsOnlyPressed(EdgePress) + event<Removed> [removes_beneath] / forget_beneath = DragsOnlyPressed(EdgePress),
    }

    Pointer[temporary_context: &mut dyn Sink<Effect>] {
        *"outside"_s + event<Move> / defer = state<Inside>,
         "outside"_s + event<Press> / defer = state<Inside>,
         "outside"_s + event<Release> / defer = state<Inside>,
         "outside"_s + event<StartDrag> = "native drag"_s,
         state<Inside> + on_entry<_> / enter_inside,
         state<Inside> + on_exit<_> / exit_inside,
         state<Inside> + event<Leave> = "outside"_s,
         state<Inside> + event<StartDrag> = "native drag"_s,
         state<Inside> + event<Removed> / forget,
         "native drag"_s + event<DragEnded> = "outside"_s,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    hot: Option<Node>,
    under: Option<Node>,
}

/// The context: the drag threshold and `Inside`'s hover pair.
pub struct Router {
    threshold2: i64,
    hover: Hover,
}

fn target(button: Button, hit: Hit) -> Option<Node> {
    if button == Button::Left { hit.node } else { hit.beneath() }
}

fn down(out: Out<'_>, p: &Press) {
    if let Some(node) = target(p.button, p.hit) {
        out.push(Effect::Down { node, at: p.at, button: p.button, clicks: p.clicks });
    }
}

impl Router {
    fn track(&mut self, out: Out<'_>, hit: Hit) {
        let h = &mut self.hover;
        let under = if hit.drags_only { hit.under } else { None };
        if under != h.under {
            if let Some(o) = h.under {
                out.push(Effect::UnderOff(o));
            }
            if let Some(n) = under {
                out.push(Effect::UnderOn(n));
            }
            h.under = under;
        }
        if hit.node != h.hot {
            if let Some(o) = h.hot {
                out.push(Effect::HoverOff(o));
            }
            if let Some(n) = hit.node {
                out.push(Effect::HoverOn(n));
            }
            h.hot = hit.node;
        }
    }

    fn release(&mut self, out: Out<'_>, owner: Node, r: &Release) {
        out.push(Effect::Up { node: owner, at: r.at, button: r.button, inside: r.hit.names(owner) });
        self.track(out, r.hit);
    }

    fn forget_node(&mut self, n: Node) {
        if self.hover.hot == Some(n) {
            self.hover.hot = None;
        }
        if self.hover.under == Some(n) {
            self.hover.under = None;
        }
    }
}

impl PointerStateMachineContext for Router {
    fn enter_inside(&mut self, _: Out<'_>) -> Result<(), ()> {
        self.hover = Hover::default();
        Ok(())
    }

    fn exit_inside(&mut self, out: Out<'_>) -> Result<(), ()> {
        if let Some(u) = self.hover.under {
            out.push(Effect::UnderOff(u));
        }
        if let Some(n) = self.hover.hot {
            out.push(Effect::HoverOff(n));
        }
        Ok(())
    }

    fn forget(&mut self, _: Out<'_>, e: &Removed) -> Result<(), ()> {
        self.forget_node(e.0);
        Ok(())
    }

    fn hover_move(&mut self, out: Out<'_>, e: &Move) -> Result<(), ()> {
        self.track(out, e.hit);
        if let Some(node) = e.hit.node {
            out.push(Effect::Move { node, at: e.at });
        }
        Ok(())
    }

    fn takes_edge_press(&self, _: Out<'_>, e: &Press) -> Result<bool, ()> {
        Ok(e.button == Button::Left && e.hit.drags_only)
    }

    fn press_edge(&mut self, out: Out<'_>, e: &Press) -> Result<EdgePress, ()> {
        self.track(out, e.hit);
        down(out, e);
        let edge = e.hit.node.ok_or(())?;
        Ok(EdgePress { edge, at: e.at, clicks: e.clicks, beneath: e.hit.under })
    }

    fn has_target(&self, _: Out<'_>, e: &Press) -> Result<bool, ()> {
        Ok(target(e.button, e.hit).is_some())
    }

    fn press_owner(&mut self, out: Out<'_>, e: &Press) -> Result<Node, ()> {
        self.track(out, e.hit);
        down(out, e);
        target(e.button, e.hit).ok_or(())
    }

    fn press_nothing(&mut self, out: Out<'_>, e: &Press) -> Result<(), ()> {
        self.track(out, e.hit);
        Ok(())
    }

    fn release_nothing(&mut self, out: Out<'_>, e: &Release) -> Result<(), ()> {
        self.track(out, e.hit);
        Ok(())
    }

    fn capture(&mut self, _: Out<'_>, e: &Capture) -> Result<Node, ()> {
        Ok(e.0)
    }

    fn owner_move(&mut self, out: Out<'_>, owner: &Node, e: &Move) -> Result<(), ()> {
        self.track(out, e.hit);
        out.push(Effect::Move { node: *owner, at: e.at });
        Ok(())
    }

    fn owner_press(&mut self, out: Out<'_>, _: &Node, e: &Press) -> Result<(), ()> {
        self.track(out, e.hit);
        down(out, e);
        Ok(())
    }

    fn owner_release(&mut self, out: Out<'_>, owner: &Node, e: &Release) -> Result<(), ()> {
        self.release(out, *owner, e);
        Ok(())
    }

    fn recapture(&mut self, _: Out<'_>, _: &Node, e: &Capture) -> Result<Node, ()> {
        Ok(e.0)
    }

    fn uncaptures_owner(&self, _: Out<'_>, owner: &Node, e: &Uncapture) -> Result<bool, ()> {
        Ok(e.0 == *owner)
    }

    fn removes_owner(&self, _: Out<'_>, owner: &Node, e: &Removed) -> Result<bool, ()> {
        Ok(e.0 == *owner)
    }

    fn forget_owner(&mut self, _: Out<'_>, _: &Node, e: &Removed) -> Result<(), ()> {
        self.forget_node(e.0);
        Ok(())
    }

    fn past_threshold(&self, _: Out<'_>, p: &EdgePress, e: &Move) -> Result<bool, ()> {
        Ok(e.at.dist2(p.at) >= self.threshold2)
    }

    fn edge_move_drags(&mut self, out: Out<'_>, p: &EdgePress, e: &Move) -> Result<Node, ()> {
        self.track(out, e.hit);
        out.push(Effect::Move { node: p.edge, at: e.at });
        Ok(p.edge)
    }

    fn edge_move(&mut self, out: Out<'_>, p: &EdgePress, e: &Move) -> Result<(), ()> {
        self.track(out, e.hit);
        out.push(Effect::Move { node: p.edge, at: e.at });
        Ok(())
    }

    /// Another button: no click under any more.
    fn edge_press_drags(&mut self, out: Out<'_>, p: &EdgePress, e: &Press) -> Result<Node, ()> {
        self.track(out, e.hit);
        down(out, e);
        Ok(p.edge)
    }

    fn edge_release(&mut self, out: Out<'_>, p: &EdgePress, e: &Release) -> Result<(), ()> {
        out.push(Effect::Up { node: p.edge, at: e.at, button: e.button, inside: e.hit.names(p.edge) });
        if e.button == Button::Left
            && let Some(b) = p.beneath
        {
            out.push(Effect::Down { node: b, at: p.at, button: Button::Left, clicks: p.clicks });
            out.push(Effect::Up { node: b, at: e.at, button: Button::Left, inside: e.hit.names(b) });
        }
        self.track(out, e.hit);
        Ok(())
    }

    fn capture_edge(&mut self, _: Out<'_>, _: &EdgePress, e: &Capture) -> Result<Node, ()> {
        Ok(e.0)
    }

    fn uncaptures_edge(&self, _: Out<'_>, p: &EdgePress, e: &Uncapture) -> Result<bool, ()> {
        Ok(e.0 == p.edge)
    }

    fn removes_edge(&self, _: Out<'_>, p: &EdgePress, e: &Removed) -> Result<bool, ()> {
        Ok(e.0 == p.edge)
    }

    fn forget_edge(&mut self, _: Out<'_>, _: &EdgePress, e: &Removed) -> Result<(), ()> {
        self.forget_node(e.0);
        Ok(())
    }

    fn removes_beneath(&self, _: Out<'_>, p: &EdgePress, e: &Removed) -> Result<bool, ()> {
        Ok(p.beneath == Some(e.0))
    }

    fn forget_beneath(&mut self, _: Out<'_>, p: &EdgePress, e: &Removed) -> Result<EdgePress, ()> {
        self.forget_node(e.0);
        Ok(EdgePress { beneath: None, ..*p })
    }
}

pub struct Pointer {
    /// The generated machine: its `process_event`, `is` and
    /// `visit_current_states` are the typed API.
    pub machine: PointerStateMachine<Router, OneDeferred>,
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        Self {
            machine: PointerStateMachine::new_with_policy(
                Router { threshold2: t * t, hover: Hover::default() },
                OneDeferred::default(),
            ),
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let m = &mut self.machine;
        // An event no row takes is `Err(InvalidEvent)`: ignored, as the spec says.
        let _ = match event {
            Event::Move { at, hit } => m.process_event(out, Move { at, hit }),
            Event::Press { at, button, clicks, hit } => m.process_event(out, Press { at, button, clicks, hit }),
            Event::Release { at, button, hit } => m.process_event(out, Release { at, button, hit }),
            Event::Leave => m.process_event(out, Leave),
            Event::Capture(n) => m.process_event(out, Capture(n)),
            Event::Uncapture(n) => m.process_event(out, Uncapture(n)),
            Event::StartDrag(_) => m.process_event(out, StartDrag),
            Event::DragEnded => m.process_event(out, DragEnded),
            Event::Removed(n) => m.process_event(out, Removed(n)),
        };
    }

    fn state(&self) -> StateName {
        let m = &self.machine;
        if m.is(&PointerStates::Outside) {
            StateName::Outside
        } else if m.is(&PointerStates::NativeDrag) {
            StateName::NativeDrag
        } else if m.is(&PointerInsideStates::Hovering) {
            StateName::Hovering
        } else if m.is(&PointerInsideStates::DragsOnlyDragging(Node(0))) {
            StateName::DragsOnlyDragging
        } else if m.is(&PointerInsideStates::Pressed(Node(0))) {
            StateName::Pressed
        } else {
            StateName::DragsOnlyPressed
        }
    }
}
