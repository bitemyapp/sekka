//! `pointer` with finny 0.3.0: the top machine `Router` has `Outside`,
//! `NativeDrag` and the sub-machine `Inside`, whose context holds the hover
//! pair and the effect buffer; its states are `Hovering`, `Pressed`,
//! `DragsOnlyPressed` and `DragsOnlyDragging`. A sub-machine only receives
//! its own event type and can't hand an event up, so `handle` addresses
//! each event to the level that handles it, and the first event entering
//! `Inside` is re-queued to it (run to completion) to be handled "as
//! `Hovering`".

use finny::decl::{BuiltFsm, FsmBuilder};
use finny::inspect::null::InspectNull;
use finny::{
    FsmCurrentState, FsmEventQueueArray, FsmEventQueueSender, FsmFactory, FsmResult, FsmTimersNull, finny_fsm,
};
use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

use crate::{Effects, Front};

pub const IMPLEMENTED: bool = true;

// Events of `Router`.
#[derive(Clone, Copy, Debug)]
pub struct Leave;
#[derive(Clone, Copy, Debug)]
pub struct StartDrag;
#[derive(Clone, Copy, Debug)]
pub struct DragEnded;
/// A pointer event arriving `Outside`: enter `Inside`, then handle it there.
#[derive(Clone)]
pub struct Enter(pub InsideEvents);

// Events of `Inside`.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub at: Point,
    pub hit: Hit,
}
#[derive(Clone, Copy, Debug)]
pub struct ButtonDown {
    pub at: Point,
    pub button: Button,
    pub clicks: u8,
    pub hit: Hit,
}
#[derive(Clone, Copy, Debug)]
pub struct ButtonUp {
    pub at: Point,
    pub button: Button,
    pub hit: Hit,
}
#[derive(Clone, Copy, Debug)]
pub struct Capture(pub Node);
#[derive(Clone, Copy, Debug)]
pub struct Uncapture(pub Node);
#[derive(Clone, Copy, Debug)]
pub struct Removed(pub Node);

impl ButtonDown {
    fn target(&self) -> Option<Node> {
        if self.button == Button::Left { self.hit.node } else { self.hit.beneath() }
    }
}

// States of `Router`.
#[derive(Default)]
pub struct Outside;
#[derive(Default)]
pub struct NativeDrag;

// States of `Inside`. finny keeps all of them at once, so each needs a value
// before it is entered: `Node(0)` stands for "not set yet".
#[derive(Default)]
pub struct Hovering;
pub struct Pressed {
    pub owner: Node,
}
pub struct DragsOnlyPressed {
    pub edge: Node,
    pub at: Point,
    pub clicks: u8,
    pub beneath: Option<Node>,
}
pub struct DragsOnlyDragging {
    pub edge: Node,
}

impl Default for Pressed {
    fn default() -> Self {
        Self { owner: Node(0) }
    }
}
impl Default for DragsOnlyPressed {
    fn default() -> Self {
        Self { edge: Node(0), at: Point::default(), clicks: 0, beneath: None }
    }
}
impl Default for DragsOnlyDragging {
    fn default() -> Self {
        Self { edge: Node(0) }
    }
}

/// `Inside`'s context: the superstate's hover pair, the threshold, and where
/// the effects go (every effect of this target is emitted inside `Inside`).
pub struct InsideCtx {
    hot: Option<Node>,
    under: Option<Node>,
    threshold2: i64,
    out: Effects<Effect, 8>,
}

/// `Router`'s context. A wrapper with a private field, so that no `Router`
/// can be built outside this module (finny's frontend would then hand out
/// its public fields).
pub struct RouterCtx {
    config: Config,
}

impl InsideCtx {
    fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        Self { hot: None, under: None, threshold2: t * t, out: Effects::new(Effect::HoverOn(Node(0))) }
    }

    fn track(&mut self, hit: Hit) {
        let under = if hit.drags_only { hit.under } else { None };
        if under != self.under {
            if let Some(o) = self.under {
                self.out.push(Effect::UnderOff(o));
            }
            if let Some(n) = under {
                self.out.push(Effect::UnderOn(n));
            }
            self.under = under;
        }
        if hit.node != self.hot {
            if let Some(o) = self.hot {
                self.out.push(Effect::HoverOff(o));
            }
            if let Some(n) = hit.node {
                self.out.push(Effect::HoverOn(n));
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

    /// The press's hover update and `Down` (every state does this first).
    fn press(&mut self, ev: &ButtonDown) {
        self.track(ev.hit);
        if let Some(node) = ev.target() {
            self.out.push(Effect::Down { node, at: ev.at, button: ev.button, clicks: ev.clicks });
        }
    }

    fn up(&mut self, node: Node, ev: &ButtonUp) {
        self.out.push(Effect::Up { node, at: ev.at, button: ev.button, inside: ev.hit.names(node) });
    }

    /// Exit `Inside`.
    fn clear(&mut self) {
        if let Some(u) = self.under.take() {
            self.out.push(Effect::UnderOff(u));
        }
        if let Some(n) = self.hot.take() {
            self.out.push(Effect::HoverOff(n));
        }
    }
}

#[finny_fsm]
fn inside_fsm(mut fsm: FsmBuilder<Inside, InsideCtx>) -> BuiltFsm {
    fsm.initial_state::<Hovering>();

    fsm.state::<Hovering>().on_event::<Motion>().internal_transition().action(|ev, ctx, _s| {
        ctx.track(ev.hit);
        if let Some(node) = ev.hit.node {
            ctx.out.push(Effect::Move { node, at: ev.at });
        }
    });
    fsm.state::<Hovering>()
        .on_event::<ButtonDown>()
        .transition_to::<DragsOnlyPressed>()
        .guard(|ev, _ctx, _s| ev.button == Button::Left && ev.hit.drags_only)
        .action(|ev, ctx, _from, to| {
            ctx.press(ev);
            *to = DragsOnlyPressed { edge: ev.hit.node.unwrap(), at: ev.at, clicks: ev.clicks, beneath: ev.hit.under };
        });
    fsm.state::<Hovering>()
        .on_event::<ButtonDown>()
        .transition_to::<Pressed>()
        .guard(|ev, _ctx, _s| ev.target().is_some())
        .action(|ev, ctx, _from, to| {
            ctx.press(ev);
            to.owner = ev.target().unwrap();
        });
    fsm.state::<Hovering>().on_event::<ButtonDown>().internal_transition().action(|ev, ctx, _s| ctx.track(ev.hit));
    fsm.state::<Hovering>().on_event::<ButtonUp>().internal_transition().action(|ev, ctx, _s| ctx.track(ev.hit));
    fsm.state::<Hovering>().on_event::<Capture>().transition_to::<Pressed>().action(|ev, _ctx, _from, to| {
        to.owner = ev.0;
    });
    fsm.state::<Hovering>().on_event::<Removed>().internal_transition().action(|ev, ctx, _s| ctx.forget(ev.0));

    fsm.state::<Pressed>().on_event::<Motion>().internal_transition().action(|ev, ctx, s| {
        ctx.track(ev.hit);
        ctx.out.push(Effect::Move { node: s.owner, at: ev.at });
    });
    fsm.state::<Pressed>().on_event::<ButtonDown>().internal_transition().action(|ev, ctx, _s| ctx.press(ev));
    fsm.state::<Pressed>().on_event::<ButtonUp>().transition_to::<Hovering>().action(|ev, ctx, from, _to| {
        ctx.up(from.owner, ev);
        ctx.track(ev.hit);
    });
    fsm.state::<Pressed>().on_event::<Capture>().internal_transition().action(|ev, _ctx, s| s.owner = ev.0);
    fsm.state::<Pressed>()
        .on_event::<Uncapture>()
        .transition_to::<Hovering>()
        .guard(|ev, _ctx, states| states.pressed.owner == ev.0);
    fsm.state::<Pressed>()
        .on_event::<Removed>()
        .transition_to::<Hovering>()
        .guard(|ev, _ctx, states| states.pressed.owner == ev.0)
        .action(|ev, ctx, _from, _to| ctx.forget(ev.0));
    fsm.state::<Pressed>().on_event::<Removed>().internal_transition().action(|ev, ctx, _s| ctx.forget(ev.0));

    fsm.state::<DragsOnlyPressed>()
        .on_event::<Motion>()
        .transition_to::<DragsOnlyDragging>()
        .guard(|ev, ctx, states| ev.at.dist2(states.drags_only_pressed.at) >= ctx.threshold2)
        .action(|ev, ctx, from, to| {
            ctx.track(ev.hit);
            ctx.out.push(Effect::Move { node: from.edge, at: ev.at });
            to.edge = from.edge;
        });
    fsm.state::<DragsOnlyPressed>().on_event::<Motion>().internal_transition().action(|ev, ctx, s| {
        ctx.track(ev.hit);
        ctx.out.push(Effect::Move { node: s.edge, at: ev.at });
    });
    // Another button: the click under is cancelled.
    fsm.state::<DragsOnlyPressed>().on_event::<ButtonDown>().transition_to::<DragsOnlyDragging>().action(
        |ev, ctx, from, to| {
            ctx.press(ev);
            to.edge = from.edge;
        },
    );
    fsm.state::<DragsOnlyPressed>().on_event::<ButtonUp>().transition_to::<Hovering>().action(|ev, ctx, from, _to| {
        ctx.up(from.edge, ev);
        if ev.button == Button::Left
            && let Some(b) = from.beneath
        {
            ctx.out.push(Effect::Down { node: b, at: from.at, button: Button::Left, clicks: from.clicks });
            ctx.out.push(Effect::Up { node: b, at: ev.at, button: Button::Left, inside: ev.hit.names(b) });
        }
        ctx.track(ev.hit);
    });
    fsm.state::<DragsOnlyPressed>().on_event::<Capture>().transition_to::<Pressed>().action(|ev, _ctx, _from, to| {
        to.owner = ev.0;
    });
    fsm.state::<DragsOnlyPressed>()
        .on_event::<Uncapture>()
        .transition_to::<Hovering>()
        .guard(|ev, _ctx, states| states.drags_only_pressed.edge == ev.0);
    fsm.state::<DragsOnlyPressed>()
        .on_event::<Removed>()
        .transition_to::<Hovering>()
        .guard(|ev, _ctx, states| states.drags_only_pressed.edge == ev.0)
        .action(|ev, ctx, _from, _to| ctx.forget(ev.0));
    fsm.state::<DragsOnlyPressed>().on_event::<Removed>().internal_transition().action(|ev, ctx, s| {
        ctx.forget(ev.0);
        if s.beneath == Some(ev.0) {
            s.beneath = None;
        }
    });

    fsm.state::<DragsOnlyDragging>().on_event::<Motion>().internal_transition().action(|ev, ctx, s| {
        ctx.track(ev.hit);
        ctx.out.push(Effect::Move { node: s.edge, at: ev.at });
    });
    fsm.state::<DragsOnlyDragging>().on_event::<ButtonDown>().internal_transition().action(|ev, ctx, _s| ctx.press(ev));
    fsm.state::<DragsOnlyDragging>().on_event::<ButtonUp>().transition_to::<Hovering>().action(|ev, ctx, from, _to| {
        ctx.up(from.edge, ev);
        ctx.track(ev.hit);
    });
    fsm.state::<DragsOnlyDragging>().on_event::<Capture>().transition_to::<Pressed>().action(|ev, _ctx, _from, to| {
        to.owner = ev.0;
    });
    fsm.state::<DragsOnlyDragging>()
        .on_event::<Uncapture>()
        .transition_to::<Hovering>()
        .guard(|ev, _ctx, states| states.drags_only_dragging.edge == ev.0);
    fsm.state::<DragsOnlyDragging>()
        .on_event::<Removed>()
        .transition_to::<Hovering>()
        .guard(|ev, _ctx, states| states.drags_only_dragging.edge == ev.0)
        .action(|ev, ctx, _from, _to| ctx.forget(ev.0));
    fsm.state::<DragsOnlyDragging>().on_event::<Removed>().internal_transition().action(|ev, ctx, _s| ctx.forget(ev.0));

    fsm.build()
}

#[finny_fsm]
fn router_fsm(mut fsm: FsmBuilder<Router, RouterCtx>) -> BuiltFsm {
    fsm.initial_state::<Outside>();

    fsm.state::<Outside>().on_event::<Enter>().transition_to::<Inside>().action(|ev, ctx, _from, _to| {
        // Capacity 1, and nothing else is queued: it can't fail.
        let _ = ctx.queue.enqueue(ev.0.clone());
    });
    fsm.state::<Outside>().on_event::<StartDrag>().transition_to::<NativeDrag>();
    fsm.state::<NativeDrag>().on_event::<DragEnded>().transition_to::<Outside>();

    fsm.sub_machine::<Inside>().with_context(|ctx| InsideCtx::new(ctx.config)).on_exit(|inside, _ctx| {
        inside.context.clear();
    });
    fsm.sub_machine::<Inside>().on_event::<Leave>().transition_to::<Outside>();
    fsm.sub_machine::<Inside>().on_event::<StartDrag>().transition_to::<NativeDrag>();

    fsm.build()
}

pub struct Pointer {
    fsm: Front<Router, 1>,
}

impl Pointer {
    /// The top machine's frontend (its states, current states and context).
    pub fn fsm(&self) -> &Front<Router, 1> {
        &self.fsm
    }

    /// finny's `dispatch` of a typed event (either level's), then the effects.
    pub fn dispatch(&mut self, event: impl Into<RouterEvents>, out: &mut impl Sink<Effect>) -> FsmResult<()> {
        let r = self.fsm.dispatch(event);
        self.inside_mut().context.out.drain(out);
        r
    }

    /// The sub-machine `Inside`; it exists (stopped) in every state.
    pub fn inside(&self) -> &Inside {
        self.fsm.get_state()
    }

    fn inside_mut(&mut self) -> &mut Inside {
        self.fsm.states.as_mut()
    }

    pub fn is_inside(&self) -> bool {
        matches!(self.fsm.get_current_states()[0], FsmCurrentState::State(RouterCurrentState::Inside))
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        let ctx = RouterCtx { config };
        let mut fsm = Router::new_with(ctx, FsmEventQueueArray::new(), InspectNull::new(), FsmTimersNull)
            .expect("the states are built from defaults");
        fsm.start().expect("Outside has no entry action");
        Self { fsm }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let inside = self.is_inside();
        // Pointer events go to `Inside`; from `Outside` they enter it first.
        let to_inside = |e: InsideEvents| -> RouterEvents { if inside { e.into() } else { Enter(e).into() } };
        let ev: RouterEvents = match event {
            Event::Move { at, hit } => to_inside(Motion { at, hit }.into()),
            Event::Press { at, button, clicks, hit } => to_inside(ButtonDown { at, button, clicks, hit }.into()),
            Event::Release { at, button, hit } => to_inside(ButtonUp { at, button, hit }.into()),
            Event::Capture(n) => InsideEvents::from(Capture(n)).into(),
            Event::Uncapture(n) => InsideEvents::from(Uncapture(n)).into(),
            Event::Removed(n) => InsideEvents::from(Removed(n)).into(),
            Event::Leave => Leave.into(),
            Event::StartDrag(_) => StartDrag.into(),
            Event::DragEnded => DragEnded.into(),
        };
        // `Err(NoTransition)`: the event is ignored where it arrived.
        let _ = self.dispatch(ev, out);
    }

    fn state(&self) -> StateName {
        match self.fsm.get_current_states()[0] {
            FsmCurrentState::State(RouterCurrentState::Outside) | FsmCurrentState::Stopped => StateName::Outside,
            FsmCurrentState::State(RouterCurrentState::NativeDrag) => StateName::NativeDrag,
            FsmCurrentState::State(RouterCurrentState::Inside) => match self.inside().get_current_states()[0] {
                FsmCurrentState::State(InsideCurrentState::Hovering) | FsmCurrentState::Stopped => StateName::Hovering,
                FsmCurrentState::State(InsideCurrentState::Pressed) => StateName::Pressed,
                FsmCurrentState::State(InsideCurrentState::DragsOnlyPressed) => StateName::DragsOnlyPressed,
                FsmCurrentState::State(InsideCurrentState::DragsOnlyDragging) => StateName::DragsOnlyDragging,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    //! Evidence for NOTES.md §4: finny's generated transition types are public,
    //! and `execute_transition` runs a transition whatever the current state
    //! (the state match and the guard are only in the generated
    //! `dispatch_event`). It needs `&mut` to the backend, which only this
    //! module has.

    use super::*;
    use finny::{DispatchContext, FsmEventQueueNull, FsmTransitionAction};
    use spec::pointer::Point;

    #[test]
    fn the_pressed_release_transition_runs_from_hovering() {
        let mut m = Pointer::new(Config::default());
        let mut out = Vec::new();
        m.handle(Event::Move { at: Point::new(0, 0), hit: Hit::on(1) }, &mut out);
        assert_eq!(m.state(), StateName::Hovering);

        // `InsideTransition14` is the 14th declared transition of `Inside`
        // (counting the start): `Pressed` --ButtonUp--> `Hovering`.
        let inside = m.inside_mut();
        let (mut queue, mut inspect, mut timers) =
            (FsmEventQueueNull::<Inside>::new(), InspectNull::new(), FsmTimersNull);
        let mut ctx =
            DispatchContext { queue: &mut queue, inspect: &mut inspect, backend: &mut **inside, timers: &mut timers };
        let up = ButtonUp { at: Point::new(0, 0), button: Button::Left, hit: Hit::on(1) };
        <InsideTransition14 as FsmTransitionAction<Inside, ButtonUp, Pressed, Hovering>>::execute_transition(
            &mut ctx,
            &up,
            0,
            &InspectNull::new(),
        );

        out.clear();
        m.handle(Event::Leave, &mut out);
        // The `Up` of a press nobody owned (P1), to `Pressed`'s placeholder owner.
        assert_eq!(out[0], Effect::Up { node: Node(0), at: Point::new(0, 0), button: Button::Left, inside: false });
    }
}
