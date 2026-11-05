//! `polkit` as an enum: the request's data in `Active`, the conversation
//! (when one runs) in the phase.

use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting { conv: u32 },
    Prompted { conv: u32 },
    Checking { conv: u32 },
    RetryWait,
    Unavailable,
}

impl Phase {
    fn conv(self) -> Option<u32> {
        match self {
            Phase::Starting { conv } | Phase::Prompted { conv } | Phase::Checking { conv } => Some(conv),
            Phase::RetryWait | Phase::Unavailable => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub cookie: Cookie,
    pub identities: u8,
    pub identity: u8,
    pub unanswered: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Active { request: Request, phase: Phase },
}

/// The machine. Its fields are private: the state changes only through
/// `Machine::handle`; [`Polkit::typed`] reads it.
pub struct Polkit {
    config: Config,
    last_conv: u32,
    state: State,
}

impl Polkit {
    /// The current state, read-only.
    pub fn typed(&self) -> &State {
        &self.state
    }
}

impl Polkit {
    fn start(&mut self, identity: u8, out: &mut impl Sink<Effect>) -> Phase {
        self.last_conv += 1;
        out.push(Effect::StartHelper { conv: self.last_conv, identity });
        Phase::Starting { conv: self.last_conv }
    }

    /// Ends the request unauthorized.
    fn end(&mut self, out: &mut impl Sink<Effect>) {
        if let State::Active { request, phase } = std::mem::replace(&mut self.state, State::Idle) {
            match phase {
                Phase::RetryWait => out.push(Effect::CancelRetry),
                p => {
                    if let Some(conv) = p.conv() {
                        out.push(Effect::EndHelper { conv });
                    }
                }
            }
            out.push(Effect::Reply { cookie: request.cookie, authorized: false });
            out.push(Effect::CloseDialog { cookie: request.cookie });
        }
    }

    fn back_off(&self, request: &mut Request, out: &mut impl Sink<Effect>) -> Phase {
        request.unanswered = request.unanswered.saturating_add(1);
        if request.unanswered >= self.config.max_unanswered {
            out.push(Effect::ShowNote(Note::Unavailable));
            Phase::Unavailable
        } else {
            out.push(Effect::ShowNote(Note::Retrying));
            out.push(Effect::ArmRetry { ms: self.config.retry_delay(request.unanswered) });
            Phase::RetryWait
        }
    }

    fn begin(&mut self, cookie: Cookie, identities: u8, preferred: u8, screen: bool, out: &mut impl Sink<Effect>) {
        if !screen {
            out.push(Effect::Reply { cookie, authorized: false });
            return;
        }
        let identity = if preferred < identities { preferred } else { 0 };
        out.push(Effect::OpenDialog { cookie });
        let phase = self.start(identity, out);
        self.state = State::Active { request: Request { cookie, identities, identity, unanswered: 0 }, phase };
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Self { config, last_conv: 0, state: State::Idle }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        if let Event::Begin { cookie, identities: 0, .. } = event
            && !matches!(self.state, State::Active { request, .. } if request.cookie == cookie)
        {
            out.push(Effect::Reply { cookie, authorized: false });
            return;
        }
        let State::Active { mut request, phase } = self.state else {
            if let Event::Begin { cookie, identities, preferred, screen } = event {
                self.begin(cookie, identities, preferred, screen, out);
            }
            return;
        };
        let current = |c: u32| phase.conv() == Some(c);
        let next = match (event, phase) {
            (Event::Begin { cookie, identities, preferred, screen }, _) => {
                if cookie != request.cookie {
                    self.end(out);
                    self.begin(cookie, identities, preferred, screen, out);
                }
                return;
            }
            (Event::CancelRequest { cookie }, _) if cookie == request.cookie => return self.end(out),
            (Event::AuthorityChanged | Event::Dismiss, _) => return self.end(out),
            (Event::Prompt { conv, echo }, Phase::Starting { .. } | Phase::Checking { .. }) if current(conv) => {
                out.push(Effect::ShowPrompt { echo });
                request.unanswered = 0;
                Phase::Prompted { conv }
            }
            (Event::PamError { conv }, _) if current(conv) => {
                out.push(Effect::ShowNote(Note::PamError));
                phase
            }
            (Event::PamInfo { conv }, _) if current(conv) => {
                out.push(Effect::ShowNote(Note::PamInfo));
                phase
            }
            (Event::Success { conv }, _) if current(conv) => {
                out.push(Effect::EndHelper { conv });
                out.push(Effect::Reply { cookie: request.cookie, authorized: true });
                out.push(Effect::CloseDialog { cookie: request.cookie });
                self.state = State::Idle;
                return;
            }
            (Event::Failure { conv }, Phase::Checking { .. }) if current(conv) => {
                out.push(Effect::EndHelper { conv });
                out.push(Effect::ShowNote(Note::TryAgain));
                self.start(request.identity, out)
            }
            (Event::Failure { conv }, Phase::Starting { .. } | Phase::Prompted { .. }) if current(conv) => {
                out.push(Effect::EndHelper { conv });
                self.back_off(&mut request, out)
            }
            (Event::Unreachable { conv }, Phase::Starting { .. }) if current(conv) => self.back_off(&mut request, out),
            (Event::Submit, Phase::Prompted { conv }) => {
                out.push(Effect::WriteAnswer { conv });
                Phase::Checking { conv }
            }
            (Event::ChooseIdentity(i), _) if i < request.identities => {
                match phase {
                    Phase::RetryWait => out.push(Effect::CancelRetry),
                    p => {
                        if let Some(conv) = p.conv() {
                            out.push(Effect::EndHelper { conv });
                        }
                    }
                }
                request.identity = i;
                request.unanswered = 0;
                self.start(i, out)
            }
            (Event::RetryTimer, Phase::RetryWait) => self.start(request.identity, out),
            _ => return,
        };
        self.state = State::Active { request, phase: next };
    }

    fn state(&self) -> StateName {
        match self.state {
            State::Idle => StateName::Idle,
            State::Active { phase, .. } => match phase {
                Phase::Starting { .. } => StateName::Starting,
                Phase::Prompted { .. } => StateName::Prompted,
                Phase::Checking { .. } => StateName::Checking,
                Phase::RetryWait => StateName::RetryWait,
                Phase::Unavailable => StateName::Unavailable,
            },
        }
    }
}
