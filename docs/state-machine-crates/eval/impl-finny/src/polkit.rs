//! `polkit` with finny 0.3.0: the top machine `Agent` has `Idle` and the
//! sub-machine `Active` (`Starting`, `Prompted`, `Checking`, `RetryWait`,
//! `Unavailable`), whose context holds the request and the machine-wide
//! `last_conv` (a sub-machine's context is built once and outlives its
//! runs). Entering `Starting` is starting a conversation (its entry action),
//! entering `RetryWait`/`Unavailable` is the back-off's note. finny exits a
//! state without saying why, so `EndHelper`/`CancelRetry` are in the
//! transitions' actions rather than exit actions (the spec's exits depend on
//! the event). Ending the request from `Agent` takes two steps through the
//! queue: an internal transition that ends what runs in `Active` (it can
//! still see `Active`'s state), then `End` leaves it (by then `Active` is
//! stopped). That is for concision: three guarded `Active -> Idle` rows per
//! ending event (conversation, `RetryWait`, neither) would do it in one
//! dispatch.

use finny::decl::{BuiltFsm, FsmBuilder};
use finny::inspect::null::InspectNull;
use finny::{
    FsmCurrentState, FsmEventQueueArray, FsmEventQueueSender, FsmFactory, FsmResult, FsmTimersNull, finny_fsm,
};
use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

use crate::{Effects, Front};

pub const IMPLEMENTED: bool = true;

// Events of `Agent`.
#[derive(Clone, Copy, Debug)]
pub struct Begin {
    pub cookie: Cookie,
    pub identities: u8,
    pub preferred: u8,
    pub screen: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct CancelRequest {
    pub cookie: Cookie,
}
#[derive(Clone, Copy, Debug)]
pub struct AuthorityChanged;
#[derive(Clone, Copy, Debug)]
pub struct Dismiss;
#[derive(Clone, Copy, Debug)]
pub struct Success {
    pub conv: u32,
}
/// Queued by `Agent` once what ran in `Active` was ended: leave it unauthorized.
#[derive(Clone, Copy, Debug)]
pub struct End;

// Events of `Active`.
#[derive(Clone, Copy, Debug)]
pub struct Unreachable {
    pub conv: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Prompt {
    pub conv: u32,
    pub echo: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct PamError {
    pub conv: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct PamInfo {
    pub conv: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Failure {
    pub conv: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct ChooseIdentity(pub u8);
#[derive(Clone, Copy, Debug)]
pub struct Submit;
#[derive(Clone, Copy, Debug)]
pub struct RetryTimer;

// States. The running conversation is `Request::last_conv` (always the
// newest one started), so the conversation states carry no data.
#[derive(Default)]
pub struct Idle;
#[derive(Default)]
pub struct Starting;
#[derive(Default)]
pub struct Prompted;
#[derive(Default)]
pub struct Checking;
#[derive(Default)]
pub struct RetryWait;
#[derive(Default)]
pub struct Unavailable;

/// `Agent`'s context: its own effects (those of transitions outside `Active`).
pub struct AgentCtx {
    config: Config,
    out: Effects<Effect, 8>,
}

/// `Active`'s context: the request, and `last_conv`, which survives requests.
pub struct Request {
    config: Config,
    cookie: Cookie,
    identities: u8,
    identity: u8,
    unanswered: u8,
    last_conv: u32,
    out: Effects<Effect, 8>,
}

impl Request {
    /// The last conversation started (the running one, while one runs).
    pub fn last_conv(&self) -> u32 {
        self.last_conv
    }

    fn new(config: &Config) -> Self {
        Self {
            config: *config,
            cookie: Cookie(0),
            identities: 0,
            identity: 0,
            unanswered: 0,
            last_conv: 0,
            out: Effects::new(Effect::CancelRetry),
        }
    }

    fn current(&self, conv: u32) -> bool {
        conv == self.last_conv
    }

    fn end_helper(&mut self) {
        self.out.push(Effect::EndHelper { conv: self.last_conv });
    }

    /// The back-off's count; the target state's entry shows the note.
    fn backs_off_to_retry(&self) -> bool {
        self.unanswered.saturating_add(1) < self.config.max_unanswered
    }

    fn choose(&mut self, i: u8) {
        self.identity = i;
        self.unanswered = 0;
    }
}

impl Active {
    /// The running conversation, if `Active` is in one.
    pub fn conversation(&self) -> Option<u32> {
        use FsmCurrentState::State;
        match self.get_current_states()[0] {
            State(ActiveCurrentState::Starting | ActiveCurrentState::Prompted | ActiveCurrentState::Checking) => {
                Some(self.context.last_conv)
            }
            _ => None,
        }
    }

    /// Ends the conversation or the wait that runs (for ending the request).
    fn end_running(&self, out: &mut Effects<Effect, 8>) {
        if let Some(conv) = self.conversation() {
            out.push(Effect::EndHelper { conv });
        } else if self.get_current_states()[0] == FsmCurrentState::State(ActiveCurrentState::RetryWait) {
            out.push(Effect::CancelRetry);
        }
    }
}

#[finny_fsm]
fn active_fsm(mut fsm: FsmBuilder<Active, Request>) -> BuiltFsm {
    fsm.initial_state::<Starting>();

    // start(identity)
    fsm.state::<Starting>().on_entry(|_s, ctx| {
        ctx.last_conv += 1;
        let (conv, identity) = (ctx.last_conv, ctx.identity);
        ctx.out.push(Effect::StartHelper { conv, identity });
    });
    // back off: `unanswered` counted by the transition
    fsm.state::<RetryWait>().on_entry(|_s, ctx| {
        ctx.out.push(Effect::ShowNote(Note::Retrying));
        let ms = ctx.config.retry_delay(ctx.unanswered);
        ctx.out.push(Effect::ArmRetry { ms });
    });
    fsm.state::<Unavailable>().on_entry(|_s, ctx| ctx.out.push(Effect::ShowNote(Note::Unavailable)));

    fsm.state::<Starting>()
        .on_event::<Prompt>()
        .transition_to::<Prompted>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|ev, ctx, _from, _to| {
            ctx.out.push(Effect::ShowPrompt { echo: ev.echo });
            ctx.unanswered = 0;
        });
    fsm.state::<Starting>()
        .on_event::<PamError>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _s| ctx.out.push(Effect::ShowNote(Note::PamError)));
    fsm.state::<Starting>()
        .on_event::<PamInfo>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _s| ctx.out.push(Effect::ShowNote(Note::PamInfo)));
    fsm.state::<Starting>()
        .on_event::<Failure>()
        .transition_to::<RetryWait>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv) && ctx.backs_off_to_retry())
        .action(|_ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.unanswered += 1;
        });
    fsm.state::<Starting>()
        .on_event::<Failure>()
        .transition_to::<Unavailable>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.unanswered = ctx.unanswered.saturating_add(1);
        });
    // No helper runs after `Unreachable`: nothing to end.
    fsm.state::<Starting>()
        .on_event::<Unreachable>()
        .transition_to::<RetryWait>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv) && ctx.backs_off_to_retry())
        .action(|_ev, ctx, _from, _to| ctx.unanswered += 1);
    fsm.state::<Starting>()
        .on_event::<Unreachable>()
        .transition_to::<Unavailable>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _from, _to| ctx.unanswered = ctx.unanswered.saturating_add(1));
    fsm.state::<Starting>()
        .on_event::<ChooseIdentity>()
        .self_transition()
        .guard(|ev, ctx, _s| ev.0 < ctx.identities)
        .action(|ev, ctx, _s| {
            ctx.end_helper();
            ctx.choose(ev.0);
        });

    fsm.state::<Prompted>()
        .on_event::<PamError>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _s| ctx.out.push(Effect::ShowNote(Note::PamError)));
    fsm.state::<Prompted>()
        .on_event::<PamInfo>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _s| ctx.out.push(Effect::ShowNote(Note::PamInfo)));
    fsm.state::<Prompted>().on_event::<Submit>().transition_to::<Checking>().action(|_ev, ctx, _from, _to| {
        let conv = ctx.last_conv;
        ctx.out.push(Effect::WriteAnswer { conv });
    });
    fsm.state::<Prompted>()
        .on_event::<Failure>()
        .transition_to::<RetryWait>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv) && ctx.backs_off_to_retry())
        .action(|_ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.unanswered += 1;
        });
    fsm.state::<Prompted>()
        .on_event::<Failure>()
        .transition_to::<Unavailable>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.unanswered = ctx.unanswered.saturating_add(1);
        });
    fsm.state::<Prompted>()
        .on_event::<ChooseIdentity>()
        .transition_to::<Starting>()
        .guard(|ev, ctx, _s| ev.0 < ctx.identities)
        .action(|ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.choose(ev.0);
        });

    fsm.state::<Checking>()
        .on_event::<Prompt>()
        .transition_to::<Prompted>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|ev, ctx, _from, _to| {
            ctx.out.push(Effect::ShowPrompt { echo: ev.echo });
            ctx.unanswered = 0;
        });
    fsm.state::<Checking>()
        .on_event::<PamError>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _s| ctx.out.push(Effect::ShowNote(Note::PamError)));
    fsm.state::<Checking>()
        .on_event::<PamInfo>()
        .internal_transition()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _s| ctx.out.push(Effect::ShowNote(Note::PamInfo)));
    // A failure after an answer restarts at once.
    fsm.state::<Checking>()
        .on_event::<Failure>()
        .transition_to::<Starting>()
        .guard(|ev, ctx, _s| ctx.current(ev.conv))
        .action(|_ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.out.push(Effect::ShowNote(Note::TryAgain));
        });
    fsm.state::<Checking>()
        .on_event::<ChooseIdentity>()
        .transition_to::<Starting>()
        .guard(|ev, ctx, _s| ev.0 < ctx.identities)
        .action(|ev, ctx, _from, _to| {
            ctx.end_helper();
            ctx.choose(ev.0);
        });

    fsm.state::<RetryWait>().on_event::<RetryTimer>().transition_to::<Starting>();
    fsm.state::<RetryWait>()
        .on_event::<ChooseIdentity>()
        .transition_to::<Starting>()
        .guard(|ev, ctx, _s| ev.0 < ctx.identities)
        .action(|ev, ctx, _from, _to| {
            ctx.out.push(Effect::CancelRetry);
            ctx.choose(ev.0);
        });

    fsm.state::<Unavailable>()
        .on_event::<ChooseIdentity>()
        .transition_to::<Starting>()
        .guard(|ev, ctx, _s| ev.0 < ctx.identities)
        .action(|ev, ctx, _from, _to| ctx.choose(ev.0));

    fsm.build()
}

#[finny_fsm]
fn agent_fsm(mut fsm: FsmBuilder<Agent, AgentCtx>) -> BuiltFsm {
    fsm.initial_state::<Idle>();

    // The agent refuses a request with nobody to authenticate, and the shell
    // one it can't show.
    fsm.state::<Idle>()
        .on_event::<Begin>()
        .internal_transition()
        .guard(|ev, _ctx, _s| ev.identities == 0 || !ev.screen)
        .action(|ev, ctx, _s| ctx.out.push(Effect::Reply { cookie: ev.cookie, authorized: false }));
    fsm.state::<Idle>().on_event::<Begin>().transition_to::<Active>().action(|ev, _ctx, _from, active| {
        let r = &mut active.context;
        r.cookie = ev.cookie;
        r.identities = ev.identities;
        r.identity = if ev.preferred < ev.identities { ev.preferred } else { 0 };
        r.unanswered = 0;
    });

    // Entering `Active` opens the dialog (then `Active` starts: `Starting`).
    fsm.sub_machine::<Active>().with_context(|ctx| Request::new(&ctx.config)).on_entry(|active, ctx| {
        ctx.out.push(Effect::OpenDialog { cookie: active.context.cookie });
    });
    // polkitd's duplicate is refused by the agent.
    fsm.sub_machine::<Active>()
        .on_event::<Begin>()
        .internal_transition()
        .guard(|ev, _ctx, states| ev.cookie == states.active.context.cookie);
    fsm.sub_machine::<Active>()
        .on_event::<Begin>()
        .internal_transition()
        .guard(|ev, _ctx, _states| ev.identities == 0)
        .action(|ev, ctx, _active| ctx.out.push(Effect::Reply { cookie: ev.cookie, authorized: false }));
    // Another request: end this one, then the new one as in `Idle`.
    fsm.sub_machine::<Active>().on_event::<Begin>().internal_transition().action(|ev, ctx, active| {
        active.end_running(&mut ctx.out);
        let _ = ctx.queue.enqueue(End);
        let _ = ctx.queue.enqueue(*ev);
    });
    fsm.sub_machine::<Active>()
        .on_event::<CancelRequest>()
        .internal_transition()
        .guard(|ev, _ctx, states| ev.cookie == states.active.context.cookie)
        .action(|_ev, ctx, active| {
            active.end_running(&mut ctx.out);
            let _ = ctx.queue.enqueue(End);
        });
    fsm.sub_machine::<Active>().on_event::<AuthorityChanged>().internal_transition().action(|_ev, ctx, active| {
        active.end_running(&mut ctx.out);
        let _ = ctx.queue.enqueue(End);
    });
    fsm.sub_machine::<Active>().on_event::<Dismiss>().internal_transition().action(|_ev, ctx, active| {
        active.end_running(&mut ctx.out);
        let _ = ctx.queue.enqueue(End);
    });
    fsm.sub_machine::<Active>().on_event::<End>().transition_to::<Idle>().action(|_ev, ctx, active, _idle| {
        let cookie = active.context.cookie;
        ctx.out.push(Effect::Reply { cookie, authorized: false });
        ctx.out.push(Effect::CloseDialog { cookie });
    });
    fsm.sub_machine::<Active>()
        .on_event::<Success>()
        .transition_to::<Idle>()
        .guard(|ev, _ctx, states| states.active.conversation() == Some(ev.conv))
        .action(|ev, ctx, active, _idle| {
            let cookie = active.context.cookie;
            ctx.out.push(Effect::EndHelper { conv: ev.conv });
            ctx.out.push(Effect::Reply { cookie, authorized: true });
            ctx.out.push(Effect::CloseDialog { cookie });
        });

    fsm.build()
}

pub struct Polkit {
    fsm: Front<Agent, 2>,
}

impl Polkit {
    pub fn fsm(&self) -> &Front<Agent, 2> {
        &self.fsm
    }

    /// finny's `dispatch` of a typed event (either level's), then the
    /// effects: `Agent`'s of a step come before `Active`'s (entering `Active`
    /// opens the dialog, then `Starting` starts the helper).
    pub fn dispatch(&mut self, event: impl Into<AgentEvents>, out: &mut impl Sink<Effect>) -> FsmResult<()> {
        let r = self.fsm.dispatch(event);
        self.fsm.context.out.drain(out);
        let active: &mut Active = self.fsm.states.as_mut();
        active.context.out.drain(out);
        r
    }

    /// The sub-machine `Active`; it exists (stopped) while `Idle`.
    pub fn active(&self) -> &Active {
        self.fsm.get_state()
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        let ctx = AgentCtx { config, out: Effects::new(Effect::CancelRetry) };
        let mut fsm = Agent::new_with(ctx, FsmEventQueueArray::new(), InspectNull::new(), FsmTimersNull)
            .expect("the states are built from defaults");
        fsm.start().expect("Idle has no entry action");
        Self { fsm }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let ev: AgentEvents = match event {
            Event::Begin { cookie, identities, preferred, screen } => {
                Begin { cookie, identities, preferred, screen }.into()
            }
            Event::CancelRequest { cookie } => CancelRequest { cookie }.into(),
            Event::AuthorityChanged => AuthorityChanged.into(),
            Event::Dismiss => Dismiss.into(),
            Event::Success { conv } => Success { conv }.into(),
            Event::Unreachable { conv } => ActiveEvents::from(Unreachable { conv }).into(),
            Event::Prompt { conv, echo } => ActiveEvents::from(Prompt { conv, echo }).into(),
            Event::PamError { conv } => ActiveEvents::from(PamError { conv }).into(),
            Event::PamInfo { conv } => ActiveEvents::from(PamInfo { conv }).into(),
            Event::Failure { conv } => ActiveEvents::from(Failure { conv }).into(),
            Event::ChooseIdentity(i) => ActiveEvents::from(ChooseIdentity(i)).into(),
            Event::Submit => ActiveEvents::from(Submit).into(),
            Event::RetryTimer => ActiveEvents::from(RetryTimer).into(),
        };
        // `Err(NoTransition)`: ignored where it arrived (`Active`'s events
        // while `Idle`, stale conversations, ...).
        let _ = self.dispatch(ev, out);
    }

    fn state(&self) -> StateName {
        use FsmCurrentState::State;
        match self.fsm.get_current_states()[0] {
            State(AgentCurrentState::Idle) | FsmCurrentState::Stopped => StateName::Idle,
            State(AgentCurrentState::Active) => match self.active().get_current_states()[0] {
                State(ActiveCurrentState::Starting) | FsmCurrentState::Stopped => StateName::Starting,
                State(ActiveCurrentState::Prompted) => StateName::Prompted,
                State(ActiveCurrentState::Checking) => StateName::Checking,
                State(ActiveCurrentState::RetryWait) => StateName::RetryWait,
                State(ActiveCurrentState::Unavailable) => StateName::Unavailable,
            },
        }
    }
}
