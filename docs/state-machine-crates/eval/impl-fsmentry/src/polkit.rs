//! `polkit` with fsmentry 0.4.0: the `Agent` graph (`Idle`, `Active`) carries
//! in `Active` the request and the `Phase` graph (`Starting`, `Prompted`,
//! `Checking` with their conversation, `RetryWait`, `Unavailable`): the
//! README's nesting in place of a superstate. Leaving `Active` hands back the
//! session, so ending the request (`EndHelper`/`CancelRetry`, `Reply`,
//! `CloseDialog`) reads the phase it ended in from the returned data.

use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    cookie: Cookie,
    identities: u8,
    identity: u8,
    /// Failures in a row with no answer given.
    unanswered: u8,
}

/// `Active`'s data: the superstate's request and the phase sub-machine.
#[derive(Debug, PartialEq, Eq)]
pub struct Session {
    request: Request,
    phase: Phase,
}

pub mod phase {
    fsmentry::fsmentry! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Phase {
            // Prompt, Submit, Prompt again after a wrong answer.
            Starting(u32) -prompt-> Prompted(u32) -submit-> Checking(u32) -prompt-> Prompted,
            // A failure after an answer restarts at once (so does ChooseIdentity:
            // one edge per pair of states).
            Checking -restart-> Starting,
            // A failure before any answer backs off; the timer retries (ChooseIdentity too).
            Starting & Prompted -back_off-> RetryWait -retry-> Starting,
            Starting & Prompted -give_up-> Unavailable -choose-> Starting,
            Starting & Prompted -choose-> Starting,
        }
    }
}

pub mod agent {
    use super::*;
    fsmentry::fsmentry! {
        #[derive(Debug, PartialEq, Eq)]
        pub enum Agent {
            // Success, or the request ended (Dismiss, CancelRequest, ...).
            Idle -begin-> Active(Session) -end-> Idle,
        }
    }
}

pub use agent::{Agent, AgentEntry};
pub use phase::{Phase, PhaseEntry};

impl Phase {
    /// The running conversation, if one runs.
    pub fn conv(&self) -> Option<u32> {
        match *self {
            Phase::Starting(c) | Phase::Prompted(c) | Phase::Checking(c) => Some(c),
            Phase::RetryWait | Phase::Unavailable => None,
        }
    }
}

impl Session {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The phase graph's transition API (legal edges only, no effects).
    pub fn phase_entry(&mut self) -> PhaseEntry<'_> {
        self.phase.entry()
    }
}

/// Machine-wide data beside the graph.
struct Ctx {
    config: Config,
    last_conv: u32,
}

impl Ctx {
    fn start(&mut self, identity: u8, out: &mut impl Sink<Effect>) -> u32 {
        self.last_conv += 1;
        out.push(Effect::StartHelper { conv: self.last_conv, identity });
        self.last_conv
    }

    fn begin(&mut self, idle: agent::Idle<'_>, event: Event, out: &mut impl Sink<Effect>) {
        let Event::Begin { cookie, identities, preferred, screen } = event else { return };
        if !screen {
            out.push(Effect::Reply { cookie, authorized: false });
            return;
        }
        let identity = if preferred < identities { preferred } else { 0 };
        out.push(Effect::OpenDialog { cookie });
        let conv = self.start(identity, out);
        let request = Request { cookie, identities, identity, unanswered: 0 };
        idle.begin(Session { request, phase: Phase::Starting(conv) });
    }

    /// Back off after a failure before any answer: `true` when it gives up.
    fn back_off(&self, r: &mut Request, out: &mut impl Sink<Effect>) -> bool {
        r.unanswered = r.unanswered.saturating_add(1);
        if r.unanswered >= self.config.max_unanswered {
            out.push(Effect::ShowNote(Note::Unavailable));
            true
        } else {
            out.push(Effect::ShowNote(Note::Retrying));
            out.push(Effect::ArmRetry { ms: self.config.retry_delay(r.unanswered) });
            false
        }
    }

    /// Everything `Active` does without leaving it.
    fn converse(&mut self, s: &mut Session, event: Event, out: &mut impl Sink<Effect>) {
        let r = &mut s.request;
        let current = s.phase.conv();
        let now = |c: u32| current == Some(c);
        match (s.phase.entry(), event) {
            (PhaseEntry::Starting(p), Event::Prompt { conv, echo }) if now(conv) => {
                out.push(Effect::ShowPrompt { echo });
                r.unanswered = 0;
                p.prompt(conv);
            }
            (PhaseEntry::Checking(p), Event::Prompt { conv, echo }) if now(conv) => {
                out.push(Effect::ShowPrompt { echo });
                r.unanswered = 0;
                p.prompt(conv);
            }
            (_, Event::PamError { conv }) if now(conv) => out.push(Effect::ShowNote(Note::PamError)),
            (_, Event::PamInfo { conv }) if now(conv) => out.push(Effect::ShowNote(Note::PamInfo)),
            (PhaseEntry::Checking(p), Event::Failure { conv }) if now(conv) => {
                out.push(Effect::EndHelper { conv });
                out.push(Effect::ShowNote(Note::TryAgain));
                let next = self.start(r.identity, out);
                p.restart(next);
            }
            (PhaseEntry::Starting(p), Event::Failure { conv } | Event::Unreachable { conv }) if now(conv) => {
                if let Event::Failure { .. } = event {
                    out.push(Effect::EndHelper { conv });
                }
                _ = if self.back_off(r, out) { p.give_up() } else { p.back_off() };
            }
            (PhaseEntry::Prompted(p), Event::Failure { conv }) if now(conv) => {
                out.push(Effect::EndHelper { conv });
                _ = if self.back_off(r, out) { p.give_up() } else { p.back_off() };
            }
            (PhaseEntry::Prompted(p), Event::Submit) => {
                let conv = *p.as_ref();
                out.push(Effect::WriteAnswer { conv });
                p.submit(conv);
            }
            (PhaseEntry::RetryWait(p), Event::RetryTimer) => p.retry(self.start(r.identity, out)),
            (phase, Event::ChooseIdentity(i)) if i < r.identities => {
                match (current, &phase) {
                    (Some(conv), _) => out.push(Effect::EndHelper { conv }),
                    (None, PhaseEntry::RetryWait(_)) => out.push(Effect::CancelRetry),
                    (None, _) => {}
                }
                r.identity = i;
                r.unanswered = 0;
                let next = self.start(i, out);
                match phase {
                    PhaseEntry::Starting(p) => _ = p.choose(next),
                    PhaseEntry::Prompted(p) => _ = p.choose(next),
                    PhaseEntry::Checking(p) => _ = p.restart(next),
                    PhaseEntry::RetryWait(p) => p.retry(next),
                    PhaseEntry::Unavailable(p) => p.choose(next),
                }
            }
            _ => {}
        }
    }
}

/// End the request unauthorized (leave `Active` without success).
fn end(active: agent::Active<'_>, out: &mut impl Sink<Effect>) {
    let Session { request, phase } = active.end();
    match phase {
        Phase::Starting(conv) | Phase::Prompted(conv) | Phase::Checking(conv) => out.push(Effect::EndHelper { conv }),
        Phase::RetryWait => out.push(Effect::CancelRetry),
        Phase::Unavailable => {}
    }
    out.push(Effect::Reply { cookie: request.cookie, authorized: false });
    out.push(Effect::CloseDialog { cookie: request.cookie });
}

pub struct Polkit {
    agent: Agent,
    ctx: Ctx,
}

impl Polkit {
    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    /// fsmentry's transition API on the live machine: only the current
    /// state's legal edges (and none of `handle`'s effects).
    pub fn entry(&mut self) -> AgentEntry<'_> {
        self.agent.entry()
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Self { agent: Agent::Idle, ctx: Ctx { config, last_conv: 0 } }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let open = match &self.agent {
            Agent::Active(s) => Some(s.request.cookie),
            Agent::Idle => None,
        };
        // Refused before the shell sees it (an open request goes on), unless
        // it is the open request's duplicate.
        if let Event::Begin { cookie, identities: 0, .. } = event
            && open != Some(cookie)
        {
            out.push(Effect::Reply { cookie, authorized: false });
            return;
        }
        match (self.agent.entry(), event) {
            (AgentEntry::Idle(idle), Event::Begin { .. }) => self.ctx.begin(idle, event, out),
            (AgentEntry::Idle(_), _) => {}
            (AgentEntry::Active(mut a), event) => match event {
                Event::Begin { cookie, .. } if open == Some(cookie) => {}
                Event::Begin { .. } => {
                    end(a, out);
                    if let AgentEntry::Idle(idle) = self.agent.entry() {
                        self.ctx.begin(idle, event, out);
                    }
                }
                Event::CancelRequest { cookie } if open == Some(cookie) => end(a, out),
                Event::AuthorityChanged | Event::Dismiss => end(a, out),
                Event::Success { conv } if a.as_ref().phase.conv() == Some(conv) => {
                    let Session { request, .. } = a.end();
                    out.push(Effect::EndHelper { conv });
                    out.push(Effect::Reply { cookie: request.cookie, authorized: true });
                    out.push(Effect::CloseDialog { cookie: request.cookie });
                }
                _ => self.ctx.converse(a.as_mut(), event, out),
            },
        }
    }

    fn state(&self) -> StateName {
        match &self.agent {
            Agent::Idle => StateName::Idle,
            Agent::Active(Session { phase, .. }) => match phase {
                Phase::Starting(_) => StateName::Starting,
                Phase::Prompted(_) => StateName::Prompted,
                Phase::Checking(_) => StateName::Checking,
                Phase::RetryWait => StateName::RetryWait,
                Phase::Unavailable => StateName::Unavailable,
            },
        }
    }
}
