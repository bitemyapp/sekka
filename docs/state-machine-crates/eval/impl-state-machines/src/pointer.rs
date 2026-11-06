//! `pointer` with state-machines 0.34.0. One `state_machine!` generates the
//! typestate `Router<S>` (the compile-fail probes use it) and the runtime
//! `DynamicRouter` (`handle` uses it; it wraps the same typed machines and
//! calls their transition methods). `Inside` is a superstate whose data is
//! the hover pair; its exit hook clears the hover. Leaf data (the owner, the
//! edge press) comes from `data:` factories, so it needs no `Default`.

use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};
use state_machines::state_machine;

pub const IMPLEMENTED: bool = true;

pub use machine::{
    DragsOnlyDragging, DragsOnlyPressed, Hovering, Inside, NativeDrag, Outside, Pointer, Pressed, Router,
    RouterDragsOnlyDraggingRemovedOutcome, RouterDragsOnlyPressedMovedOutcome, RouterDragsOnlyPressedRemovedOutcome,
    RouterHoveringMovedOutcome, RouterHoveringPressOutcome, RouterOutsidePressOutcome, RouterPressedRemovedOutcome,
};

/// `Inside`'s data: what is hovered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    hot: Option<Node>,
    under: Option<Node>,
}

/// `DragsOnlyPressed`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgePress {
    edge: Node,
    at: Point,
    clicks: u8,
    beneath: Option<Node>,
}

/// Payloads: the generated event enum takes one type per event.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub at: Point,
    pub hit: Hit,
}

#[derive(Clone, Copy, Debug)]
pub struct Push {
    pub at: Point,
    pub button: Button,
    pub clicks: u8,
    pub hit: Hit,
}

#[derive(Clone, Copy, Debug)]
pub struct Lift {
    pub at: Point,
    pub button: Button,
    pub hit: Hit,
}

/// The payloads that carry a hit test (`track` takes any of them).
pub trait Pointing {
    fn hit(&self) -> Hit;
}

impl Pointing for Motion {
    fn hit(&self) -> Hit {
        self.hit
    }
}
impl Pointing for Push {
    fn hit(&self) -> Hit {
        self.hit
    }
}
impl Pointing for Lift {
    fn hit(&self) -> Hit {
        self.hit
    }
}

/// The machine's context: the squared drag threshold and the effect buffer.
#[derive(Debug)]
pub struct Ctx {
    threshold2: i64,
    out: Vec<Effect>,
}

impl Ctx {
    pub fn new(config: Config) -> Self {
        let t = i64::from(config.drag_threshold);
        Self { threshold2: t * t, out: Vec::with_capacity(crate::out::CAPACITY) }
    }
}

/// The generated machines, their callbacks and the facade. The module is
/// private so that the generated `Dynamic*` wrapper (with its
/// `new_init_state`), its event and its state enums aren't exported.
mod machine {
    use super::*;
    use crate::out::drain;

    state_machine! {
        name: Router,
        context: Ctx,
        dynamic: true,
        initial: Outside,
        states: [
            Outside,
            NativeDrag,
            superstate Inside(Hover) {
                state Hovering,
                state Pressed(Node),
                state DragsOnlyPressed(EdgePress),
                state DragsOnlyDragging(Node),
            },
        ],
        lifecycle: {
            Inside { exit: [clear_hover] }
        },
        events {
            moved {
                payload: Motion,
                hierarchical: true,
                transition: { from: Outside, to: Hovering, after: [track, move_to_hit] }
                transition: { from: Hovering, internal: true, before: [track, move_to_hit] }
                transition: {
                    from: DragsOnlyPressed, to: DragsOnlyDragging,
                    guards: [past_threshold], data: keep_edge, before: [track, move_to_owner]
                }
                transition: { from: Inside, internal: true, before: [track, move_to_owner] }
            }
            press {
                payload: Push,
                branching: true,
                transition: {
                    from: [Outside, Hovering], to: DragsOnlyPressed,
                    guards: [is_edge_press], data: edge_press, after: [track, down]
                }
                transition: {
                    from: [Outside, Hovering], to: Pressed,
                    guards: [has_target], data: press_target, after: [track, down]
                }
                transition: { from: [Outside, Hovering], to: Hovering, fallback: true, after: [track] }
                // Another button: the click under is cancelled.
                transition: {
                    from: DragsOnlyPressed, to: DragsOnlyDragging,
                    fallback: true, data: keep_edge, before: [track, down]
                }
                transition: { from: [Pressed, DragsOnlyDragging], internal: true, fallback: true, before: [track, down] }
            }
            // Delivers before tracking, unlike moves and presses.
            release {
                payload: Lift,
                transition: { from: Outside, to: Hovering, after: [track] }
                transition: { from: Hovering, internal: true, before: [track] }
                transition: { from: [Pressed, DragsOnlyDragging], to: Hovering, before: [up, track] }
                transition: { from: DragsOnlyPressed, to: Hovering, before: [up, click_under, track] }
            }
            leave {
                transition: { from: Inside, to: Outside }
            }
            capture {
                payload: Node,
                transition: { from: Inside, to: Pressed, data: captured }
            }
            uncapture {
                payload: Node,
                transition: { from: [Pressed, DragsOnlyPressed, DragsOnlyDragging], to: Hovering, guards: [owned_by] }
            }
            start_drag {
                transition: { from: [Inside, Outside], to: NativeDrag }
            }
            drag_ended {
                transition: { from: NativeDrag, to: Outside }
            }
            removed {
                payload: Node,
                hierarchical: true,
                transition: {
                    from: [Pressed, DragsOnlyPressed, DragsOnlyDragging], to: Hovering,
                    guards: [owned_by], before: [forget]
                }
                transition: { from: Inside, internal: true, before: [forget] }
            }
        }
    }

    fn target(button: Button, hit: Hit) -> Option<Node> {
        if button == Button::Left { hit.node } else { hit.beneath() }
    }

    /// Guards, callbacks, hooks and factories: methods of the typed machine,
    /// generic over its state. Data accessors return `Option`s in every state.
    impl<S> Router<S> {
        /// The hover and the effect buffer. `Inside`'s data is set on entry;
        /// only a machine built in an `Inside` state (`new_init_state`) lacks it.
        fn hover(&mut self) -> (&mut Hover, &mut Vec<Effect>) {
            (self.__state_data_inside.get_or_insert_default(), &mut self.ctx.out)
        }

        /// The press owner of an owner state.
        fn owner(&self) -> Option<Node> {
            let edge = self.state_data_drags_only_pressed().map(|e| e.edge);
            self.state_data_pressed().or(self.state_data_drags_only_dragging()).copied().or(edge)
        }

        fn track<P: Pointing>(&mut self, p: &P) {
            let hit = p.hit();
            let (hover, out) = self.hover();
            let under = if hit.drags_only { hit.under } else { None };
            if under != hover.under {
                if let Some(o) = hover.under {
                    out.push(Effect::UnderOff(o));
                }
                if let Some(n) = under {
                    out.push(Effect::UnderOn(n));
                }
                hover.under = under;
            }
            if hit.node != hover.hot {
                if let Some(o) = hover.hot {
                    out.push(Effect::HoverOff(o));
                }
                if let Some(n) = hit.node {
                    out.push(Effect::HoverOn(n));
                }
                hover.hot = hit.node;
            }
        }

        /// `Inside`'s exit hook.
        fn clear_hover(&mut self) {
            let (hover, out) = self.hover();
            if let Some(u) = hover.under {
                out.push(Effect::UnderOff(u));
            }
            if let Some(n) = hover.hot {
                out.push(Effect::HoverOff(n));
            }
        }

        fn move_to_hit(&mut self, m: &Motion) {
            if let Some(node) = m.hit.node {
                self.ctx.out.push(Effect::Move { node, at: m.at });
            }
        }

        fn move_to_owner(&mut self, m: &Motion) {
            if let Some(node) = self.owner() {
                self.ctx.out.push(Effect::Move { node, at: m.at });
            }
        }

        fn down(&mut self, p: &Push) {
            if let Some(node) = target(p.button, p.hit) {
                self.ctx.out.push(Effect::Down { node, at: p.at, button: p.button, clicks: p.clicks });
            }
        }

        fn up(&mut self, r: &Lift) {
            if let Some(node) = self.owner() {
                self.ctx.out.push(Effect::Up { node, at: r.at, button: r.button, inside: r.hit.names(node) });
            }
        }

        fn click_under(&mut self, r: &Lift) {
            if r.button == Button::Left
                && let Some(&EdgePress { at, clicks, beneath: Some(b), .. }) = self.state_data_drags_only_pressed()
            {
                self.ctx.out.push(Effect::Down { node: b, at, button: Button::Left, clicks });
                self.ctx.out.push(Effect::Up { node: b, at: r.at, button: Button::Left, inside: r.hit.names(b) });
            }
        }

        fn forget(&mut self, &n: &Node) {
            let (hover, _) = self.hover();
            if hover.hot == Some(n) {
                hover.hot = None;
            }
            if hover.under == Some(n) {
                hover.under = None;
            }
            if let Some(press) = self.__state_data_drags_only_pressed.as_mut()
                && press.beneath == Some(n)
            {
                press.beneath = None;
            }
        }

        fn past_threshold(&self, ctx: &Ctx, m: &Motion) -> bool {
            self.state_data_drags_only_pressed().is_some_and(|e| m.at.dist2(e.at) >= ctx.threshold2)
        }

        fn is_edge_press(&self, _: &Ctx, p: &Push) -> bool {
            p.button == Button::Left && p.hit.drags_only
        }

        fn has_target(&self, _: &Ctx, p: &Push) -> bool {
            target(p.button, p.hit).is_some()
        }

        fn owned_by(&self, _: &Ctx, n: &Node) -> bool {
            self.owner() == Some(*n)
        }

        fn edge_press(&mut self, p: &mut Push) -> EdgePress {
            let edge = p.hit.node.expect("a drags-only hit names its node");
            EdgePress { edge, at: p.at, clicks: p.clicks, beneath: p.hit.under }
        }

        fn press_target(&mut self, p: &mut Push) -> Node {
            target(p.button, p.hit).expect("guarded by has_target")
        }

        fn captured(&mut self, n: &mut Node) -> Node {
            *n
        }

        fn keep_edge<P>(&mut self, _: &mut P) -> Node {
            self.owner().expect("DragsOnlyPressed is entered with its data")
        }
    }

    /// The `Machine` facade over the generated runtime wrapper.
    pub struct Pointer {
        machine: DynamicRouter,
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyRouterState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicRouter {
        fn ctx(&mut self) -> Option<&mut Ctx> {
            Some(match self.inner.as_mut()? {
                AnyRouterState::Outside(m) => &mut m.ctx,
                AnyRouterState::NativeDrag(m) => &mut m.ctx,
                AnyRouterState::Hovering(m) => &mut m.ctx,
                AnyRouterState::Pressed(m) => &mut m.ctx,
                AnyRouterState::DragsOnlyPressed(m) => &mut m.ctx,
                AnyRouterState::DragsOnlyDragging(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Router<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    impl Machine for Pointer {
        fn new(config: Config) -> Self {
            Self { machine: DynamicRouter::new(Ctx::new(config)) }
        }

        fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
            let event = match event {
                Event::Move { at, hit } => RouterEvent::Moved(Motion { at, hit }),
                Event::Press { at, button, clicks, hit } => RouterEvent::Press(Push { at, button, clicks, hit }),
                Event::Release { at, button, hit } => RouterEvent::Release(Lift { at, button, hit }),
                Event::Leave => RouterEvent::Leave,
                Event::Capture(n) => RouterEvent::Capture(n),
                Event::Uncapture(n) => RouterEvent::Uncapture(n),
                Event::StartDrag(_) => RouterEvent::StartDrag,
                Event::DragEnded => RouterEvent::DragEnded,
                Event::Removed(n) => RouterEvent::Removed(n),
            };
            // `Err`: no transition from here, or its guards refused: ignored.
            let _ = self.machine.handle(event);
            if let Some(ctx) = self.machine.ctx() {
                drain(&mut ctx.out, out);
            }
        }

        fn state(&self) -> StateName {
            match self.machine.current_state() {
                RouterState::Outside => StateName::Outside,
                RouterState::NativeDrag => StateName::NativeDrag,
                RouterState::Hovering => StateName::Hovering,
                RouterState::Pressed => StateName::Pressed,
                RouterState::DragsOnlyPressed => StateName::DragsOnlyPressed,
                RouterState::DragsOnlyDragging => StateName::DragsOnlyDragging,
            }
        }
    }
}
