//! `polkit` with statig 0.4.1. `active` is a superstate whose state-local
//! `request` every leaf in it carries; its entry action opens the dialog and
//! its exit action replies to polkitd and closes it. `conversing` (inside
//! `active`) holds the running conversation: leaving it ends the helper.
//! `starting`'s entry action starts one, `retry_wait`'s arms the timer and
//! its exit cancels it.
//!
//! An action's signature has no event, so `handle` routes the event through
//! the context (`Cx::event`): the exit actions read it for the spec's
//! exceptions ("except after `Unreachable`", "other than by its timer") and
//! for the verdict (`Success`). A restart inside `conversing` doesn't exit
//! it, so those handlers end the helper themselves. `Begin` for another
//! cookie ends the request and is dispatched again in `Idle` (`Cx::again`):
//! a handler can't transition and re-dispatch.

use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};
use statig::prelude::*;

pub const IMPLEMENTED: bool = true;

/// statig's external context: where effects go, the event being handled
/// (`None` while initializing), and whether `handle` must dispatch it again
/// (now in the state it moved to).
pub struct Cx<'a> {
    out: &'a mut dyn Sink<Effect>,
    event: Option<Event>,
    again: bool,
}

impl Cx<'_> {
    #[inline]
    fn push(&mut self, effect: Effect) {
        self.out.push(effect);
    }
}

/// The request's data, carried by every state in `active`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    cookie: Cookie,
    identities: u8,
    identity: u8,
    /// Failures in a row with no answer given.
    unanswered: u8,
}

/// The shared storage.
pub struct Agent {
    config: Config,
    last_conv: u32,
}

impl Agent {
    /// back off: count an unanswered failure; wait, or give up.
    fn back_off(&self, request: &mut Request) -> Outcome<State> {
        request.unanswered = request.unanswered.saturating_add(1);
        if request.unanswered >= self.config.max_unanswered {
            Transition(State::unavailable(*request))
        } else {
            Transition(State::retry_wait(*request))
        }
    }
}

#[state_machine(initial = "State::idle()", state(derive(Debug, Clone, Copy, PartialEq, Eq)))]
impl Agent {
    #[state]
    fn idle(context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Begin { cookie, identities, preferred, screen } => {
                if identities == 0 || !screen {
                    context.push(Effect::Reply { cookie, authorized: false });
                    return Handled;
                }
                let identity = if preferred < identities { preferred } else { 0 };
                let request = Request { cookie, identities, identity, unanswered: 0 };
                Transition(State::starting(request))
            }
            _ => Handled,
        }
    }

    /// A request is open: the dialog shows.
    #[superstate(entry_action = "open_dialog", exit_action = "close_dialog")]
    fn active(request: &mut Request, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Begin { cookie, .. } if cookie == request.cookie => Handled,
            Event::Begin { cookie, identities: 0, .. } => {
                context.push(Effect::Reply { cookie, authorized: false });
                Handled
            }
            Event::Begin { .. } => {
                context.again = true;
                Transition(State::idle())
            }
            Event::CancelRequest { cookie } if cookie == request.cookie => Transition(State::idle()),
            Event::AuthorityChanged | Event::Dismiss => Transition(State::idle()),
            Event::ChooseIdentity(i) if i < request.identities => {
                request.identity = i;
                request.unanswered = 0;
                Transition(State::starting(*request))
            }
            _ => Handled,
        }
    }

    #[action]
    fn open_dialog(request: &mut Request, context: &mut Cx<'_>) {
        context.push(Effect::OpenDialog { cookie: request.cookie });
    }

    /// Replies `true` only when the request ends with the helper's `Success`.
    #[action]
    fn close_dialog(request: &mut Request, context: &mut Cx<'_>) {
        let authorized = matches!(context.event, Some(Event::Success { .. }));
        context.push(Effect::Reply { cookie: request.cookie, authorized });
        context.push(Effect::CloseDialog { cookie: request.cookie });
    }

    /// A conversation runs; only its events count.
    #[superstate(superstate = "active", exit_action = "end_helper")]
    fn conversing(request: &mut Request, conv: &mut u32, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::PamError { conv: c } if c == *conv => {
                context.push(Effect::ShowNote(Note::PamError));
                Handled
            }
            Event::PamInfo { conv: c } if c == *conv => {
                context.push(Effect::ShowNote(Note::PamInfo));
                Handled
            }
            Event::Success { conv: c } if c == *conv => Transition(State::idle()),
            // `active` restarts; `starting` is inside `conversing`, so ending
            // this helper is up to us.
            Event::ChooseIdentity(i) if i < request.identities => {
                context.push(Effect::EndHelper { conv: *conv });
                Super
            }
            _ => Super,
        }
    }

    /// Ends the helper, unless it was never reached.
    #[action]
    fn end_helper(conv: &mut u32, context: &mut Cx<'_>) {
        if !matches!(context.event, Some(Event::Unreachable { .. })) {
            context.push(Effect::EndHelper { conv: *conv });
        }
    }

    #[state(superstate = "conversing", entry_action = "start")]
    fn starting(
        &self,
        request: &mut Request,
        #[default] conv: &mut u32,
        context: &mut Cx<'_>,
        event: &Event,
    ) -> Outcome<State> {
        match *event {
            Event::Prompt { conv: c, echo } if c == *conv => {
                context.push(Effect::ShowPrompt { echo });
                request.unanswered = 0;
                Transition(State::prompted(*request, *conv))
            }
            Event::Failure { conv: c } | Event::Unreachable { conv: c } if c == *conv => self.back_off(request),
            _ => Super,
        }
    }

    /// start(identity): the next conversation number.
    #[action]
    fn start(&mut self, request: &mut Request, conv: &mut u32, context: &mut Cx<'_>) {
        self.last_conv += 1;
        *conv = self.last_conv;
        context.push(Effect::StartHelper { conv: *conv, identity: request.identity });
    }

    #[state(superstate = "conversing")]
    fn prompted(&self, request: &mut Request, conv: &mut u32, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Submit => {
                context.push(Effect::WriteAnswer { conv: *conv });
                Transition(State::checking(*request, *conv))
            }
            Event::Failure { conv: c } if c == *conv => self.back_off(request),
            _ => Super,
        }
    }

    #[state(superstate = "conversing")]
    fn checking(request: &mut Request, conv: &mut u32, context: &mut Cx<'_>, event: &Event) -> Outcome<State> {
        match *event {
            Event::Prompt { conv: c, echo } if c == *conv => {
                context.push(Effect::ShowPrompt { echo });
                request.unanswered = 0;
                Transition(State::prompted(*request, *conv))
            }
            // An answer was refused: restart at once (inside `conversing`).
            Event::Failure { conv: c } if c == *conv => {
                context.push(Effect::EndHelper { conv: c });
                context.push(Effect::ShowNote(Note::TryAgain));
                Transition(State::starting(*request))
            }
            _ => Super,
        }
    }

    #[state(superstate = "active", entry_action = "arm_retry", exit_action = "cancel_retry")]
    fn retry_wait(request: &mut Request, event: &Event) -> Outcome<State> {
        match *event {
            Event::RetryTimer => Transition(State::starting(*request)),
            _ => Super,
        }
    }

    #[action]
    fn arm_retry(&self, request: &mut Request, context: &mut Cx<'_>) {
        context.push(Effect::ShowNote(Note::Retrying));
        context.push(Effect::ArmRetry { ms: self.config.retry_delay(request.unanswered) });
    }

    /// Cancels the timer, unless it is the timer that fired.
    #[action]
    fn cancel_retry(context: &mut Cx<'_>) {
        if !matches!(context.event, Some(Event::RetryTimer)) {
            context.push(Effect::CancelRetry);
        }
    }

    /// No automatic starts: only `ChooseIdentity` (in `active`) leaves.
    #[state(superstate = "active", entry_action = "give_up", local_storage("request: Request"))]
    fn unavailable() -> Outcome<State> {
        Super
    }

    #[action]
    fn give_up(context: &mut Cx<'_>) {
        context.push(Effect::ShowNote(Note::Unavailable));
    }
}

pub struct Polkit {
    machine: statig::blocking::InitializedStateMachine<Agent>,
}

impl Polkit {
    pub fn state_enum(&self) -> &State {
        self.machine.state()
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        let machine = Agent { config, last_conv: 0 }.uninitialized_state_machine();
        let machine = machine.init_with_context(&mut Cx { out: &mut crate::Nowhere, event: None, again: false });
        Self { machine }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let mut context = Cx { out, event: Some(event), again: false };
        self.machine.handle_with_context(&event, &mut context);
        if context.again {
            context.again = false;
            self.machine.handle_with_context(&event, &mut context);
        }
    }

    fn state(&self) -> StateName {
        match self.machine.state() {
            State::Idle {} => StateName::Idle,
            State::Starting { .. } => StateName::Starting,
            State::Prompted { .. } => StateName::Prompted,
            State::Checking { .. } => StateName::Checking,
            State::RetryWait { .. } => StateName::RetryWait,
            State::Unavailable { .. } => StateName::Unavailable,
        }
    }
}
