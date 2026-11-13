//! `pointer` with rust-fsm 0.8.0: the six leaf states are a flat Mealy table
//! (`fsm`); the DSL has no superstate, guard, state data or context, so:
//!
//! - `Inside` is spelled out: its `Leave`, `StartDrag`, `Capture` and
//!   `Removed` rows are repeated in each of its four substates;
//! - guards are folded into the input alphabet by [`Pointer::input`], which
//!   reads only the event and the data (never the table's state): a press is
//!   `PressNothing`/`PressEdge`/`PressNode`, a move past the threshold is
//!   `MoveFar`, an event naming the owner is `…Owner`. Rows where the guard
//!   doesn't matter are repeated for each outcome;
//! - the table's states are `spec::pointer::StateName` (the crate's custom
//!   state type), so `state()` reads the table directly;
//! - the data (hover, owner, the press) lives beside the table in [`Data`],
//!   valid only in the states the spec gives it, stale elsewhere;
//! - each output names an action, run on the data with the event's payload
//!   after `consume` (the payload doesn't pass through the table).

use rust_fsm::state_machine;
use spec::Sink;
use spec::pointer::{Button, Config, Effect, Event, Hit, Machine, Node, Point, StateName};

pub const IMPLEMENTED: bool = true;

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[state_machine(state(spec::pointer::StateName))]
    pub fsm(Outside)

    Outside => {
        Move => Hovering [Hover],
        MoveFar => Hovering [Hover],
        PressNothing => Hovering [Press],
        PressEdge => DragsOnlyPressed [Press],
        PressNode => Pressed [Press],
        Release => Hovering [Track],
        StartDrag => NativeDrag,
    },
    Hovering => {
        Move => Hovering [Hover],
        MoveFar => Hovering [Hover],
        PressNothing => Hovering [Press],
        PressEdge => DragsOnlyPressed [Press],
        PressNode => Pressed [Press],
        Release => Hovering [Track],
        Leave => Outside [Exit],
        StartDrag => NativeDrag [Exit],
        Capture => Pressed [Capture],
        Removed => Hovering [Forget],
        RemovedOwner => Hovering [Forget],
    },
    Pressed => {
        Move => Pressed [Drag],
        MoveFar => Pressed [Drag],
        PressNothing => Pressed [PressMore],
        PressEdge => Pressed [PressMore],
        PressNode => Pressed [PressMore],
        Release => Hovering [Up],
        Leave => Outside [Exit],
        StartDrag => NativeDrag [Exit],
        Capture => Pressed [Capture],
        UncaptureOwner => Hovering,
        Removed => Pressed [Forget],
        RemovedOwner => Hovering [Forget],
    },
    DragsOnlyPressed => {
        Move => DragsOnlyPressed [Drag],
        MoveFar => DragsOnlyDragging [Drag],
        PressNothing => DragsOnlyDragging [PressMore],
        PressEdge => DragsOnlyDragging [PressMore],
        PressNode => DragsOnlyDragging [PressMore],
        Release => Hovering [Click],
        Leave => Outside [Exit],
        StartDrag => NativeDrag [Exit],
        Capture => Pressed [Capture],
        UncaptureOwner => Hovering,
        Removed => DragsOnlyPressed [ForgetBeneath],
        RemovedOwner => Hovering [Forget],
    },
    DragsOnlyDragging => {
        Move => DragsOnlyDragging [Drag],
        MoveFar => DragsOnlyDragging [Drag],
        PressNothing => DragsOnlyDragging [PressMore],
        PressEdge => DragsOnlyDragging [PressMore],
        PressNode => DragsOnlyDragging [PressMore],
        Release => Hovering [Up],
        Leave => Outside [Exit],
        StartDrag => NativeDrag [Exit],
        Capture => Pressed [Capture],
        UncaptureOwner => Hovering,
        Removed => DragsOnlyDragging [Forget],
        RemovedOwner => Hovering [Forget],
    },
    NativeDrag(DragEnded) => Outside,
}

use fsm::{Input, Output};

/// The table and the data it can't hold, in one struct (so the table's
/// byte packs with the data). `hot`/`under` are the `Inside` hover (cleared
/// on leaving it); `owner` is meaningful in the owner states,
/// `at`/`clicks`/`beneath` in `DragsOnlyPressed`; elsewhere they are stale.
#[derive(Clone, Debug)]
pub struct Data {
    fsm: fsm::StateMachine,
    pub hot: Option<Node>,
    pub under: Option<Node>,
    pub owner: Node,
    pub at: Point,
    pub clicks: u8,
    pub beneath: Option<Node>,
}

pub struct Pointer {
    data: Data,
    threshold: i32,
}

fn target(button: Button, hit: Hit) -> Option<Node> {
    if button == Button::Left { hit.node } else { hit.beneath() }
}

impl Data {
    fn new() -> Self {
        let fsm = fsm::StateMachine::new();
        Data { fsm, hot: None, under: None, owner: Node(0), at: Point::new(0, 0), clicks: 0, beneath: None }
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

    fn forget(&mut self, n: Node) {
        if self.hot == Some(n) {
            self.hot = None;
        }
        if self.under == Some(n) {
            self.under = None;
        }
    }
}

impl Pointer {
    /// The table's state.
    pub fn table(&self) -> &fsm::StateMachine {
        &self.data.fsm
    }

    /// The data beside the table.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The guards: an event refined by what the data says about it.
    fn input(&self, event: &Event) -> Option<Input> {
        let t2 = i64::from(self.threshold) * i64::from(self.threshold);
        Some(match *event {
            Event::Move { at, .. } if at.dist2(self.data.at) >= t2 => Input::MoveFar,
            Event::Move { .. } => Input::Move,
            Event::Press { button, hit, .. } => match target(button, hit) {
                None => Input::PressNothing,
                Some(_) if button == Button::Left && hit.drags_only => Input::PressEdge,
                Some(_) => Input::PressNode,
            },
            Event::Release { .. } => Input::Release,
            Event::Leave => Input::Leave,
            Event::Capture(_) => Input::Capture,
            Event::Uncapture(n) if n == self.data.owner => Input::UncaptureOwner,
            Event::Uncapture(_) => return None,
            Event::StartDrag(_) => Input::StartDrag,
            Event::DragEnded => Input::DragEnded,
            Event::Removed(n) if n == self.data.owner => Input::RemovedOwner,
            Event::Removed(_) => Input::Removed,
        })
    }

    /// The actions the outputs name. Exhaustive on the output, so a new
    /// output is a compile error; each arm takes its event's payload (the
    /// table pairs every output with one kind of event).
    fn act(&mut self, action: Output, event: Event, out: &mut impl Sink<Effect>) {
        let d = &mut self.data;
        match action {
            Output::Hover => {
                let Event::Move { at, hit } = event else { unreachable!() };
                d.track(hit, out);
                if let Some(node) = hit.node {
                    out.push(Effect::Move { node, at });
                }
            }
            Output::Drag => {
                let Event::Move { at, hit } = event else { unreachable!() };
                d.track(hit, out);
                out.push(Effect::Move { node: d.owner, at });
            }
            Output::Press => {
                let Event::Press { at, button, clicks, hit } = event else { unreachable!() };
                d.track(hit, out);
                if let Some(node) = target(button, hit) {
                    out.push(Effect::Down { node, at, button, clicks });
                    (d.owner, d.at, d.clicks, d.beneath) = (node, at, clicks, hit.under);
                }
            }
            Output::PressMore => {
                let Event::Press { at, button, clicks, hit } = event else { unreachable!() };
                d.track(hit, out);
                if let Some(node) = target(button, hit) {
                    out.push(Effect::Down { node, at, button, clicks });
                }
            }
            Output::Track => {
                let Event::Release { hit, .. } = event else { unreachable!() };
                d.track(hit, out);
            }
            Output::Up => {
                let Event::Release { at, button, hit } = event else { unreachable!() };
                out.push(Effect::Up { node: d.owner, at, button, inside: hit.names(d.owner) });
                d.track(hit, out);
            }
            Output::Click => {
                let Event::Release { at, button, hit } = event else { unreachable!() };
                out.push(Effect::Up { node: d.owner, at, button, inside: hit.names(d.owner) });
                if button == Button::Left
                    && let Some(b) = d.beneath
                {
                    out.push(Effect::Down { node: b, at: d.at, button: Button::Left, clicks: d.clicks });
                    out.push(Effect::Up { node: b, at, button: Button::Left, inside: hit.names(b) });
                }
                d.track(hit, out);
            }
            Output::Exit => {
                if let Some(u) = d.under.take() {
                    out.push(Effect::UnderOff(u));
                }
                if let Some(n) = d.hot.take() {
                    out.push(Effect::HoverOff(n));
                }
            }
            Output::Capture => {
                let Event::Capture(n) = event else { unreachable!() };
                d.owner = n;
            }
            Output::Forget => {
                let Event::Removed(n) = event else { unreachable!() };
                d.forget(n);
            }
            Output::ForgetBeneath => {
                let Event::Removed(n) = event else { unreachable!() };
                d.forget(n);
                if d.beneath == Some(n) {
                    d.beneath = None;
                }
            }
        }
    }
}

impl Machine for Pointer {
    fn new(config: Config) -> Self {
        Self { data: Data::new(), threshold: config.drag_threshold }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let Some(input) = self.input(&event) else { return };
        // `Err`: no row for this (state, input): the spec ignores the event.
        if let Ok(Some(action)) = self.data.fsm.consume(&input) {
            self.act(action, event, out);
        }
    }

    fn state(&self) -> StateName {
        *self.data.fsm.state()
    }
}
