//! `polkit` with statum 0.9.0: one `#[state]` per phase, the request (and
//! the running `Conversation`, a token only this module makes) in sealed
//! per-state payloads (private fields; a token for `Idle`), `config` and
//! `last_conv` as the `#[machine] Session`'s fields.
//! An answer can only be written from `Prompted`, an unanswered failure only
//! leads to `RetryWait` or `Unavailable`, and every way out of a request
//! consumes it. statum has no superstate for `Active`: `conversing!` puts
//! the three conversing states' shared edges into each one's block.

use crate::{SEAL, Seal};
use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};
use statum::{Branch, CanTransitionWith, machine, state, transition};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug)]
pub struct Request {
    cookie: Cookie,
    identities: u8,
    identity: u8,
    unanswered: u8,
}

impl Request {
    pub fn cookie(&self) -> Cookie {
        self.cookie
    }
}

/// A running helper conversation.
#[derive(Debug)]
pub struct Conversation(u32);

impl Conversation {
    pub fn is(&self, conv: u32) -> bool {
        self.0 == conv
    }
}

tokens!(IdleTok);

/// The payloads of the states with a running conversation.
macro_rules! conversing_data {
    ($($d:ident),*) => {$(
        #[derive(Debug)]
        pub struct $d {
            req: Request,
            conv: Conversation,
        }

        impl $d {
            pub fn conversation(&self) -> &Conversation {
                &self.conv
            }
        }
    )*};
}
conversing_data!(StartingData, PromptedData, CheckingData);

#[derive(Debug)]
pub struct RetryWaitData {
    req: Request,
}

#[derive(Debug)]
pub struct UnavailableData {
    req: Request,
}

#[state]
#[derive(Debug)]
pub enum PolkitState {
    Idle(IdleTok),
    Starting(StartingData),
    Prompted(PromptedData),
    Checking(CheckingData),
    RetryWait(RetryWaitData),
    Unavailable(UnavailableData),
}

#[machine]
#[derive(Debug)]
pub struct Session<PolkitState> {
    config: Config,
    last_conv: u32,
    seal: Seal,
}

/// After an attempt failed before any answer.
pub type Backoff = ::statum::Branch<Session<RetryWait>, Session<Unavailable>>;

impl<S: PolkitStateTrait> Session<S> {
    /// Starts the next conversation (the caller moves to `Starting`).
    fn start(&mut self, req: &mut Request, identity: u8, out: &mut impl Sink<Effect>) -> Conversation {
        self.last_conv += 1;
        req.identity = identity;
        out.push(Effect::StartHelper { conv: self.last_conv, identity });
        Conversation(self.last_conv)
    }
}

fn reply(req: &Request, authorized: bool, out: &mut impl Sink<Effect>) {
    out.push(Effect::Reply { cookie: req.cookie, authorized });
    out.push(Effect::CloseDialog { cookie: req.cookie });
}

fn back_off<M>(m: M, config: &Config, mut req: Request, out: &mut impl Sink<Effect>) -> Backoff
where
    M: CanTransitionWith<RetryWaitData, Output = Session<RetryWait>>
        + CanTransitionWith<UnavailableData, Output = Session<Unavailable>>,
{
    req.unanswered = req.unanswered.saturating_add(1);
    if req.unanswered >= config.max_unanswered {
        out.push(Effect::ShowNote(Note::Unavailable));
        Branch::Second(m.transition_with_data(UnavailableData { req }))
    } else {
        out.push(Effect::ShowNote(Note::Retrying));
        out.push(Effect::ArmRetry { ms: config.retry_delay(req.unanswered) });
        Branch::First(m.transition_with_data(RetryWaitData { req }))
    }
}

impl Session<Idle> {
    #[inline]
    pub fn new(config: Config) -> Self {
        Session::<Idle>::builder().state_data(IdleTok(())).config(config).last_conv(0).seal(SEAL).build()
    }
}

#[transition]
impl Session<Idle> {
    /// Opens the dialog and starts a conversation, or refuses (no screen).
    pub fn begin(
        mut self,
        cookie: Cookie,
        identities: u8,
        preferred: u8,
        screen: bool,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Session<Starting>, Session<Idle>> {
        if !screen || identities == 0 {
            out.push(Effect::Reply { cookie, authorized: false });
            return Err(self);
        }
        out.push(Effect::OpenDialog { cookie });
        let mut req = Request { cookie, identities, identity: 0, unanswered: 0 };
        let conv = self.start(&mut req, if preferred < identities { preferred } else { 0 }, out);
        Ok(self.transition_with(StartingData { req, conv }))
    }
}

/// The edges every state with a running conversation has, in the same
/// `#[transition]` block as its own (statum wants each edge declared once).
macro_rules! conversing {
    ($s:ident { $($own:tt)* }) => {
        #[transition]
        impl Session<$s> {
            pub fn end(self, out: &mut impl Sink<Effect>) -> Session<Idle> {
                out.push(Effect::EndHelper { conv: self.state_data.conv.0 });
                reply(&self.state_data.req, false, out);
                self.transition_with(IdleTok(()))
            }
            pub fn succeed(self, out: &mut impl Sink<Effect>) -> Session<Idle> {
                out.push(Effect::EndHelper { conv: self.state_data.conv.0 });
                reply(&self.state_data.req, true, out);
                self.transition_with(IdleTok(()))
            }
            pub fn choose(
                mut self,
                i: u8,
                out: &mut impl Sink<Effect>,
            ) -> ::core::result::Result<Session<Starting>, Session<$s>> {
                if i >= self.state_data.req.identities {
                    return Err(self);
                }
                out.push(Effect::EndHelper { conv: self.state_data.conv.0 });
                let mut req = self.state_data.req;
                req.unanswered = 0;
                let conv = self.start(&mut req, i, out);
                Ok(self.transition_with(StartingData { req, conv }))
            }
            $($own)*
        }
    };
}

conversing!(Starting {
    pub fn prompt(self, echo: bool, out: &mut impl Sink<Effect>) -> Session<Prompted> {
        out.push(Effect::ShowPrompt { echo });
        self.transition_map(|StartingData { mut req, conv }| {
            req.unanswered = 0;
            PromptedData { req, conv }
        })
    }
    pub fn failed(self, out: &mut impl Sink<Effect>) -> Backoff {
        out.push(Effect::EndHelper { conv: self.state_data.conv.0 });
        self.unreachable(out)
    }
    /// The helper couldn't be reached: nothing to end.
    pub fn unreachable(self, out: &mut impl Sink<Effect>) -> Backoff {
        let (config, req) = (self.config, self.state_data.req);
        back_off(self, &config, req, out)
    }
});

conversing!(Prompted {
    pub fn answer(self, out: &mut impl Sink<Effect>) -> Session<Checking> {
        out.push(Effect::WriteAnswer { conv: self.state_data.conv.0 });
        self.transition_map(|PromptedData { req, conv }| CheckingData { req, conv })
    }
    pub fn failed(self, out: &mut impl Sink<Effect>) -> Backoff {
        out.push(Effect::EndHelper { conv: self.state_data.conv.0 });
        let (config, req) = (self.config, self.state_data.req);
        back_off(self, &config, req, out)
    }
});

conversing!(Checking {
    pub fn prompt(self, echo: bool, out: &mut impl Sink<Effect>) -> Session<Prompted> {
        out.push(Effect::ShowPrompt { echo });
        self.transition_map(|CheckingData { mut req, conv }| {
            req.unanswered = 0;
            PromptedData { req, conv }
        })
    }
    /// The answer was refused: try again at once.
    pub fn refused(mut self, out: &mut impl Sink<Effect>) -> Session<Starting> {
        out.push(Effect::EndHelper { conv: self.state_data.conv.0 });
        out.push(Effect::ShowNote(Note::TryAgain));
        let mut req = self.state_data.req;
        let identity = req.identity;
        let conv = self.start(&mut req, identity, out);
        self.transition_with(StartingData { req, conv })
    }
});

#[transition]
impl Session<RetryWait> {
    pub fn retry(mut self, out: &mut impl Sink<Effect>) -> Session<Starting> {
        let mut req = self.state_data.req;
        let identity = req.identity;
        let conv = self.start(&mut req, identity, out);
        self.transition_with(StartingData { req, conv })
    }
    pub fn end(self, out: &mut impl Sink<Effect>) -> Session<Idle> {
        out.push(Effect::CancelRetry);
        reply(&self.state_data.req, false, out);
        self.transition_with(IdleTok(()))
    }
    pub fn choose(
        mut self,
        i: u8,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Session<Starting>, Session<RetryWait>> {
        if i >= self.state_data.req.identities {
            return Err(self);
        }
        out.push(Effect::CancelRetry);
        let mut req = self.state_data.req;
        req.unanswered = 0;
        let conv = self.start(&mut req, i, out);
        Ok(self.transition_with(StartingData { req, conv }))
    }
}

#[transition]
impl Session<Unavailable> {
    pub fn end(self, out: &mut impl Sink<Effect>) -> Session<Idle> {
        reply(&self.state_data.req, false, out);
        self.transition_with(IdleTok(()))
    }
    pub fn choose(
        mut self,
        i: u8,
        out: &mut impl Sink<Effect>,
    ) -> ::core::result::Result<Session<Starting>, Session<Unavailable>> {
        if i >= self.state_data.req.identities {
            return Err(self);
        }
        let mut req = self.state_data.req;
        req.unanswered = 0;
        let conv = self.start(&mut req, i, out);
        Ok(self.transition_with(StartingData { req, conv }))
    }
}

#[inline]
fn request(s: &session::SomeState) -> Option<&Request> {
    use session::SomeState as S;
    match s {
        S::Idle(_) => None,
        S::Starting(s) => Some(&s.state_data.req),
        S::Prompted(s) => Some(&s.state_data.req),
        S::Checking(s) => Some(&s.state_data.req),
        S::RetryWait(s) => Some(&s.state_data.req),
        S::Unavailable(s) => Some(&s.state_data.req),
    }
}

#[inline]
fn conv(s: &session::SomeState) -> Option<u32> {
    use session::SomeState as S;
    match s {
        S::Starting(s) => Some(s.state_data.conv.0),
        S::Prompted(s) => Some(s.state_data.conv.0),
        S::Checking(s) => Some(s.state_data.conv.0),
        _ => None,
    }
}

/// Ends whatever request is open, unauthorized.
fn end(s: session::SomeState, out: &mut impl Sink<Effect>) -> Session<Idle> {
    use session::SomeState as S;
    match s {
        S::Idle(s) => s,
        S::Starting(s) => s.end(out),
        S::Prompted(s) => s.end(out),
        S::Checking(s) => s.end(out),
        S::RetryWait(s) => s.end(out),
        S::Unavailable(s) => s.end(out),
    }
}

fn ok_or<A, B>(r: Result<A, B>, a: fn(A) -> session::SomeState, b: fn(B) -> session::SomeState) -> session::SomeState {
    match r {
        Ok(x) => a(x),
        Err(x) => b(x),
    }
}

#[inline]
fn backoff(b: Backoff) -> session::SomeState {
    match b {
        Branch::First(s) => session::SomeState::RetryWait(s),
        Branch::Second(s) => session::SomeState::Unavailable(s),
    }
}

pub struct Polkit {
    /// `None` only while `handle` has moved the state out (statum's builder
    /// makes no cheap placeholder).
    state: Option<session::SomeState>,
}

impl Polkit {
    pub fn typed(&self) -> &session::SomeState {
        self.state.as_ref().expect("a state")
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Self { state: Some(session::SomeState::Idle(Session::<Idle>::new(config))) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use session::SomeState as S;
        let open = request(self.typed()).map(|r| r.cookie);
        // Helper events of another conversation, or with none running, do nothing.
        let helper_conv = match event {
            Event::Unreachable { conv }
            | Event::Prompt { conv, .. }
            | Event::PamError { conv }
            | Event::PamInfo { conv }
            | Event::Success { conv }
            | Event::Failure { conv } => Some(conv),
            _ => None,
        };
        if helper_conv.is_some() && helper_conv != conv(self.typed()) {
            return;
        }
        let state = self.state.take().expect("a state");
        self.state = Some(match (state, event) {
            (s, Event::Begin { cookie, .. }) if open == Some(cookie) => s,
            (s, Event::Begin { cookie, identities: 0, .. }) => {
                out.push(Effect::Reply { cookie, authorized: false });
                s
            }
            (s, Event::Begin { cookie, identities, preferred, screen }) => {
                ok_or(end(s, out).begin(cookie, identities, preferred, screen, out), S::Starting, S::Idle)
            }
            (s, Event::CancelRequest { cookie }) if open == Some(cookie) => S::Idle(end(s, out)),
            (s, Event::AuthorityChanged | Event::Dismiss) => S::Idle(end(s, out)),
            (S::Starting(s), Event::Prompt { echo, .. }) => S::Prompted(s.prompt(echo, out)),
            (S::Checking(s), Event::Prompt { echo, .. }) => S::Prompted(s.prompt(echo, out)),
            (s @ (S::Starting(_) | S::Prompted(_) | S::Checking(_)), Event::PamError { .. }) => {
                out.push(Effect::ShowNote(Note::PamError));
                s
            }
            (s @ (S::Starting(_) | S::Prompted(_) | S::Checking(_)), Event::PamInfo { .. }) => {
                out.push(Effect::ShowNote(Note::PamInfo));
                s
            }
            (S::Starting(s), Event::Success { .. }) => S::Idle(s.succeed(out)),
            (S::Prompted(s), Event::Success { .. }) => S::Idle(s.succeed(out)),
            (S::Checking(s), Event::Success { .. }) => S::Idle(s.succeed(out)),
            (S::Checking(s), Event::Failure { .. }) => S::Starting(s.refused(out)),
            (S::Starting(s), Event::Failure { .. }) => backoff(s.failed(out)),
            (S::Prompted(s), Event::Failure { .. }) => backoff(s.failed(out)),
            (S::Starting(s), Event::Unreachable { .. }) => backoff(s.unreachable(out)),
            (S::Prompted(s), Event::Submit) => S::Checking(s.answer(out)),
            (S::Starting(s), Event::ChooseIdentity(i)) => ok_or(s.choose(i, out), S::Starting, S::Starting),
            (S::Prompted(s), Event::ChooseIdentity(i)) => ok_or(s.choose(i, out), S::Starting, S::Prompted),
            (S::Checking(s), Event::ChooseIdentity(i)) => ok_or(s.choose(i, out), S::Starting, S::Checking),
            (S::RetryWait(s), Event::ChooseIdentity(i)) => ok_or(s.choose(i, out), S::Starting, S::RetryWait),
            (S::Unavailable(s), Event::ChooseIdentity(i)) => ok_or(s.choose(i, out), S::Starting, S::Unavailable),
            (S::RetryWait(s), Event::RetryTimer) => S::Starting(s.retry(out)),
            (s, _) => s,
        });
    }

    fn state(&self) -> StateName {
        use session::SomeState as S;
        match self.typed() {
            S::Idle(_) => StateName::Idle,
            S::Starting(_) => StateName::Starting,
            S::Prompted(_) => StateName::Prompted,
            S::Checking(_) => StateName::Checking,
            S::RetryWait(_) => StateName::RetryWait,
            S::Unavailable(_) => StateName::Unavailable,
        }
    }
}
