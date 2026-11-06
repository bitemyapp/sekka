//! `pointer` with statig 0.4.1: `inside` is a superstate whose state-local
//! `hover` every leaf in it carries; its exit action clears the hover when
//! the pointer leaves or the compositor takes it. Effects go through the
//! external context (a `dyn Sink`, since statig's context type can't be
//! generic per call).

use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};
use statig::prelude::*;

use crate::{Nowhere, Out};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    hot: Option<Node>,
    under: Option<Node>,
}

impl Hover {
    fn track(&mut self, hit: Hit, out: &mut Out<'_, Effect>) {
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
    fn forget(&mut self, n: Node) {
        if self.hot == Some(n) {
            self.hot = None;
        }
        if self.under == Some(n) {
            self.under = None;
        }
    }
}

fn target(button: Button, hit: Hit) -> Option<Node> {
    if button == Button::Left { hit.node } else { hit.beneath() }
}

fn down(t: Option<Node>, at: Point, button: Button, clicks: u8, out: &mut Out<'_, Effect>) {
    if let Some(node) = t {
        out.push(Effect::Down { node, at, button, clicks });
    }
}

/// A press with nobody owning one (from `hovering`, or entering from `outside`).
fn first_press(
    mut hover: Hover,
    at: Point,
    button: Button,
    clicks: u8,
    hit: Hit,
    out: &mut Out<'_, Effect>,
) -> Outcome<State> {
    hover.track(hit, out);
    let t = target(button, hit);
    down(t, at, button, clicks, out);
    match t {
        None => Transition(State::hovering(hover)),
        Some(edge) if button == Button::Left && hit.drags_only => {
            Transition(State::edge_pressed(hover, edge, at, clicks, hit.under))
        }
        Some(owner) => Transition(State::pressed(hover, owner)),
    }
}

/// The shared storage: the drag threshold, squared.
pub struct Router {
    threshold2: i64,
}

#[state_machine(initial = "State::outside()", state(derive(Debug, Clone, Copy)))]
impl Router {
    #[state]
    fn outside(context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        let mut hover = Hover::default();
        match *event {
            Event::Move { at, hit } => {
                hover.track(hit, context);
                if let Some(node) = hit.node {
                    context.push(Effect::Move { node, at });
                }
                Transition(State::hovering(hover))
            }
            Event::Press { at, button, clicks, hit } => first_press(hover, at, button, clicks, hit, context),
            Event::Release { hit, .. } => {
                hover.track(hit, context);
                Transition(State::hovering(hover))
            }
            Event::StartDrag(_) => Transition(State::native_drag()),
            _ => Handled,
        }
    }

    #[state]
    fn native_drag(event: &Event) -> Outcome<State> {
        match event {
            Event::DragEnded => Transition(State::outside()),
            _ => Handled,
        }
    }

    /// Over the window. Leaving it (or a compositor drag) exits, clearing the hover.
    #[superstate(exit_action = "exit_inside")]
    fn inside(hover: &mut Hover, event: &Event) -> Outcome<State> {
        match *event {
            Event::Leave => Transition(State::outside()),
            Event::StartDrag(_) => Transition(State::native_drag()),
            Event::Capture(n) => Transition(State::pressed(*hover, n)),
            Event::Removed(n) => {
                hover.forget(n);
                Handled
            }
            _ => Handled,
        }
    }

    #[action]
    fn exit_inside(hover: &mut Hover, context: &mut Out<'_, Effect>) {
        if let Some(u) = hover.under {
            context.push(Effect::UnderOff(u));
        }
        if let Some(n) = hover.hot {
            context.push(Effect::HoverOff(n));
        }
    }

    #[state(superstate = "inside")]
    fn hovering(hover: &mut Hover, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Move { at, hit } => {
                hover.track(hit, context);
                if let Some(node) = hit.node {
                    context.push(Effect::Move { node, at });
                }
                Handled
            }
            Event::Press { at, button, clicks, hit } => first_press(*hover, at, button, clicks, hit, context),
            Event::Release { hit, .. } => {
                hover.track(hit, context);
                Handled
            }
            _ => Super,
        }
    }

    #[state(superstate = "inside")]
    fn pressed(hover: &mut Hover, owner: &mut Node, context: &mut Out<'_, Effect>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Move { at, hit } => {
                hover.track(hit, context);
                context.push(Effect::Move { node: *owner, at });
                Handled
            }
            Event::Press { at, button, clicks, hit } => {
                hover.track(hit, context);
                down(target(button, hit), at, button, clicks, context);
                Handled
            }
            Event::Release { at, button, hit } => {
                context.push(Effect::Up { node: *owner, at, button, inside: hit.names(*owner) });
                hover.track(hit, context);
                Transition(State::hovering(*hover))
            }
            Event::Uncapture(n) if n == *owner => Transition(State::hovering(*hover)),
            Event::Removed(n) if n == *owner => {
                hover.forget(n);
                Transition(State::hovering(*hover))
            }
            _ => Super,
        }
    }

    #[state(superstate = "inside")]
    fn edge_pressed(
        &mut self,
        hover: &mut Hover,
        edge: &mut Node,
        at: &mut Point,
        clicks: &mut u8,
        beneath: &mut Option<Node>,
        context: &mut Out<'_, Effect>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Move { at: p, hit } => {
                hover.track(hit, context);
                context.push(Effect::Move { node: *edge, at: p });
                if p.dist2(*at) >= self.threshold2 { Transition(State::edge_dragging(*hover, *edge)) } else { Handled }
            }
            Event::Press { at: p, button, clicks: c, hit } => {
                // Another button: no click under any more.
                hover.track(hit, context);
                down(target(button, hit), p, button, c, context);
                Transition(State::edge_dragging(*hover, *edge))
            }
            Event::Release { at: p, button, hit } => {
                context.push(Effect::Up { node: *edge, at: p, button, inside: hit.names(*edge) });
                if button == Button::Left
                    && let Some(b) = *beneath
                {
                    context.push(Effect::Down { node: b, at: *at, button: Button::Left, clicks: *clicks });
                    context.push(Effect::Up { node: b, at: p, button: Button::Left, inside: hit.names(b) });
                }
                hover.track(hit, context);
                Transition(State::hovering(*hover))
            }
            Event::Uncapture(n) if n == *edge => Transition(State::hovering(*hover)),
            Event::Removed(n) if n == *edge => {
                hover.forget(n);
                Transition(State::hovering(*hover))
            }
            Event::Removed(n) => {
                if *beneath == Some(n) {
                    *beneath = None;
                }
                Super
            }
            _ => Super,
        }
    }

    #[state(superstate = "inside")]
    fn edge_dragging(
        hover: &mut Hover,
        edge: &mut Node,
        context: &mut Out<'_, Effect>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Move { at, hit } => {
                hover.track(hit, context);
                context.push(Effect::Move { node: *edge, at });
                Handled
            }
            Event::Press { at, button, clicks, hit } => {
                hover.track(hit, context);
                down(target(button, hit), at, button, clicks, context);
                Handled
            }
            Event::Release { at, button, hit } => {
                context.push(Effect::Up { node: *edge, at, button, inside: hit.names(*edge) });
                hover.track(hit, context);
                Transition(State::hovering(*hover))
            }
            Event::Uncapture(n) if n == *edge => Transition(State::hovering(*hover)),
            Event::Removed(n) if n == *edge => {
                hover.forget(n);
                Transition(State::hovering(*hover))
            }
            _ => Super,
        }
    }
}

pub struct Pointer {
    machine: statig::blocking::InitializedStateMachine<Router>,
}

impl Pointer {
    pub fn state_enum(&self) -> &State {
        self.machine.state()
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        let machine = Router { threshold2: t * t }.uninitialized_state_machine();
        // `outside` has no entry action: initializing emits nothing.
        let machine = machine.init_with_context(&mut Out(&mut Nowhere));
        Self { machine }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.machine.handle_with_context(&event, &mut Out(out));
    }

    fn state(&self) -> StateName {
        match self.machine.state() {
            State::Outside {} => StateName::Outside,
            State::NativeDrag {} => StateName::NativeDrag,
            State::Hovering { .. } => StateName::Hovering,
            State::Pressed { .. } => StateName::Pressed,
            State::EdgePressed { .. } => StateName::DragsOnlyPressed,
            State::EdgeDragging { .. } => StateName::DragsOnlyDragging,
        }
    }
}
