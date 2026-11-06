//! `polkit` as hand-written typestate. A `Conversation` token exists only
//! while a helper runs, an answer can only be written from `Prompted`, an
//! unanswered failure can only lead to `RetryWait` or `Unavailable`, and
//! every way out of a request consumes it (one reply).

use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

/// What the transitions need beyond the state.
pub struct Ctx {
    config: Config,
    last_conv: u32,
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct Idle(());
#[derive(Debug)]
pub struct Starting {
    req: Request,
    conv: Conversation,
}
#[derive(Debug)]
pub struct Prompted {
    req: Request,
    conv: Conversation,
}
#[derive(Debug)]
pub struct Checking {
    req: Request,
    conv: Conversation,
}
#[derive(Debug)]
pub struct RetryWait {
    req: Request,
}
#[derive(Debug)]
pub struct Unavailable {
    req: Request,
}

/// After an attempt failed before any answer.
pub enum Backoff {
    Wait(RetryWait),
    Unavailable(Unavailable),
}

fn start(ctx: &mut Ctx, mut req: Request, identity: u8, out: &mut impl Sink<Effect>) -> Starting {
    ctx.last_conv += 1;
    req.identity = identity;
    out.push(Effect::StartHelper { conv: ctx.last_conv, identity });
    Starting { req, conv: Conversation(ctx.last_conv) }
}

fn reply(req: Request, authorized: bool, out: &mut impl Sink<Effect>) -> Idle {
    out.push(Effect::Reply { cookie: req.cookie, authorized });
    out.push(Effect::CloseDialog { cookie: req.cookie });
    Idle(())
}

fn end_conv(conv: Conversation, out: &mut impl Sink<Effect>) {
    out.push(Effect::EndHelper { conv: conv.0 });
}

fn back_off(ctx: &Ctx, mut req: Request, out: &mut impl Sink<Effect>) -> Backoff {
    req.unanswered = req.unanswered.saturating_add(1);
    if req.unanswered >= ctx.config.max_unanswered {
        out.push(Effect::ShowNote(Note::Unavailable));
        Backoff::Unavailable(Unavailable { req })
    } else {
        out.push(Effect::ShowNote(Note::Retrying));
        out.push(Effect::ArmRetry { ms: ctx.config.retry_delay(req.unanswered) });
        Backoff::Wait(RetryWait { req })
    }
}

impl Idle {
    pub fn new() -> Self {
        Idle(())
    }
    /// Opens the dialog and starts a conversation, or refuses (no screen).
    pub fn begin(
        self,
        ctx: &mut Ctx,
        cookie: Cookie,
        identities: u8,
        preferred: u8,
        screen: bool,
        out: &mut impl Sink<Effect>,
    ) -> Result<Starting, Idle> {
        if !screen || identities == 0 {
            out.push(Effect::Reply { cookie, authorized: false });
            return Err(self);
        }
        out.push(Effect::OpenDialog { cookie });
        let identity = if preferred < identities { preferred } else { 0 };
        Ok(start(ctx, Request { cookie, identities, identity: 0, unanswered: 0 }, identity, out))
    }
}

impl Default for Idle {
    fn default() -> Self {
        Self::new()
    }
}

/// What every state with a running conversation can do.
macro_rules! conversing {
    ($t:ident) => {
        impl $t {
            pub fn request(&self) -> &Request {
                &self.req
            }
            pub fn conversation(&self) -> &Conversation {
                &self.conv
            }
            pub fn end(self, out: &mut impl Sink<Effect>) -> Idle {
                end_conv(self.conv, out);
                reply(self.req, false, out)
            }
            pub fn succeed(self, out: &mut impl Sink<Effect>) -> Idle {
                end_conv(self.conv, out);
                reply(self.req, true, out)
            }
            pub fn note(&self, note: Note, out: &mut impl Sink<Effect>) {
                out.push(Effect::ShowNote(note));
            }
            pub fn choose(mut self, ctx: &mut Ctx, i: u8, out: &mut impl Sink<Effect>) -> Result<Starting, $t> {
                if i >= self.req.identities {
                    return Err(self);
                }
                end_conv(self.conv, out);
                self.req.unanswered = 0;
                Ok(start(ctx, self.req, i, out))
            }
        }
    };
}
conversing!(Starting);
conversing!(Prompted);
conversing!(Checking);

impl Starting {
    pub fn prompt(mut self, echo: bool, out: &mut impl Sink<Effect>) -> Prompted {
        out.push(Effect::ShowPrompt { echo });
        self.req.unanswered = 0;
        Prompted { req: self.req, conv: self.conv }
    }
    pub fn failed(self, ctx: &Ctx, out: &mut impl Sink<Effect>) -> Backoff {
        end_conv(self.conv, out);
        back_off(ctx, self.req, out)
    }
    /// The helper couldn't be reached: nothing to end.
    pub fn unreachable(self, ctx: &Ctx, out: &mut impl Sink<Effect>) -> Backoff {
        back_off(ctx, self.req, out)
    }
}

impl Prompted {
    pub fn answer(self, out: &mut impl Sink<Effect>) -> Checking {
        out.push(Effect::WriteAnswer { conv: self.conv.0 });
        Checking { req: self.req, conv: self.conv }
    }
    pub fn failed(self, ctx: &Ctx, out: &mut impl Sink<Effect>) -> Backoff {
        end_conv(self.conv, out);
        back_off(ctx, self.req, out)
    }
}

impl Checking {
    pub fn prompt(mut self, echo: bool, out: &mut impl Sink<Effect>) -> Prompted {
        out.push(Effect::ShowPrompt { echo });
        self.req.unanswered = 0;
        Prompted { req: self.req, conv: self.conv }
    }
    /// The answer was refused: try again at once.
    pub fn refused(self, ctx: &mut Ctx, out: &mut impl Sink<Effect>) -> Starting {
        end_conv(self.conv, out);
        out.push(Effect::ShowNote(Note::TryAgain));
        let identity = self.req.identity;
        start(ctx, self.req, identity, out)
    }
}

impl RetryWait {
    pub fn retry(self, ctx: &mut Ctx, out: &mut impl Sink<Effect>) -> Starting {
        let identity = self.req.identity;
        start(ctx, self.req, identity, out)
    }
    pub fn end(self, out: &mut impl Sink<Effect>) -> Idle {
        out.push(Effect::CancelRetry);
        reply(self.req, false, out)
    }
    pub fn choose(mut self, ctx: &mut Ctx, i: u8, out: &mut impl Sink<Effect>) -> Result<Starting, RetryWait> {
        if i >= self.req.identities {
            return Err(self);
        }
        out.push(Effect::CancelRetry);
        self.req.unanswered = 0;
        Ok(start(ctx, self.req, i, out))
    }
}

impl Unavailable {
    pub fn end(self, out: &mut impl Sink<Effect>) -> Idle {
        reply(self.req, false, out)
    }
    pub fn choose(mut self, ctx: &mut Ctx, i: u8, out: &mut impl Sink<Effect>) -> Result<Starting, Unavailable> {
        if i >= self.req.identities {
            return Err(self);
        }
        self.req.unanswered = 0;
        Ok(start(ctx, self.req, i, out))
    }
}

/// The runtime wrapper.
#[derive(Debug)]
pub enum Any {
    Idle(Idle),
    Starting(Starting),
    Prompted(Prompted),
    Checking(Checking),
    RetryWait(RetryWait),
    Unavailable(Unavailable),
}

impl From<Backoff> for Any {
    fn from(b: Backoff) -> Any {
        match b {
            Backoff::Wait(s) => Any::RetryWait(s),
            Backoff::Unavailable(s) => Any::Unavailable(s),
        }
    }
}

impl Any {
    fn request(&self) -> Option<&Request> {
        match self {
            Any::Idle(_) => None,
            Any::Starting(s) => Some(&s.req),
            Any::Prompted(s) => Some(&s.req),
            Any::Checking(s) => Some(&s.req),
            Any::RetryWait(s) => Some(&s.req),
            Any::Unavailable(s) => Some(&s.req),
        }
    }
    fn conv(&self) -> Option<u32> {
        match self {
            Any::Starting(s) => Some(s.conv.0),
            Any::Prompted(s) => Some(s.conv.0),
            Any::Checking(s) => Some(s.conv.0),
            _ => None,
        }
    }
    /// Ends whatever request is open, unauthorized.
    fn end(self, out: &mut impl Sink<Effect>) -> Idle {
        match self {
            Any::Idle(s) => s,
            Any::Starting(s) => s.end(out),
            Any::Prompted(s) => s.end(out),
            Any::Checking(s) => s.end(out),
            Any::RetryWait(s) => s.end(out),
            Any::Unavailable(s) => s.end(out),
        }
    }
}

fn ok_or<A, B>(r: Result<A, B>, a: fn(A) -> Any, b: fn(B) -> Any) -> Any {
    match r {
        Ok(x) => a(x),
        Err(x) => b(x),
    }
}

pub struct Polkit {
    ctx: Ctx,
    state: Any,
}

impl Polkit {
    pub fn typed(&self) -> &Any {
        &self.state
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Self { ctx: Ctx { config, last_conv: 0 }, state: Any::Idle(Idle::new()) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use Any as A;
        let ctx = &mut self.ctx;
        let open = self.state.request().map(|r| r.cookie);
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
        if helper_conv.is_some() && helper_conv != self.state.conv() {
            return;
        }
        let state = std::mem::replace(&mut self.state, A::Idle(Idle(())));
        self.state = match (state, event) {
            (s, Event::Begin { cookie, .. }) if open == Some(cookie) => s,
            (s, Event::Begin { cookie, identities: 0, .. }) => {
                out.push(Effect::Reply { cookie, authorized: false });
                s
            }
            (s, Event::Begin { cookie, identities, preferred, screen }) => {
                let idle = s.end(out);
                ok_or(idle.begin(ctx, cookie, identities, preferred, screen, out), A::Starting, A::Idle)
            }
            (s, Event::CancelRequest { cookie }) if open == Some(cookie) => A::Idle(s.end(out)),
            (s, Event::AuthorityChanged | Event::Dismiss) => A::Idle(s.end(out)),
            (A::Starting(s), Event::Prompt { echo, .. }) => A::Prompted(s.prompt(echo, out)),
            (A::Checking(s), Event::Prompt { echo, .. }) => A::Prompted(s.prompt(echo, out)),
            (s @ (A::Starting(_) | A::Prompted(_) | A::Checking(_)), Event::PamError { .. }) => {
                out.push(Effect::ShowNote(Note::PamError));
                s
            }
            (s @ (A::Starting(_) | A::Prompted(_) | A::Checking(_)), Event::PamInfo { .. }) => {
                out.push(Effect::ShowNote(Note::PamInfo));
                s
            }
            (A::Starting(s), Event::Success { .. }) => A::Idle(s.succeed(out)),
            (A::Prompted(s), Event::Success { .. }) => A::Idle(s.succeed(out)),
            (A::Checking(s), Event::Success { .. }) => A::Idle(s.succeed(out)),
            (A::Checking(s), Event::Failure { .. }) => A::Starting(s.refused(ctx, out)),
            (A::Starting(s), Event::Failure { .. }) => s.failed(ctx, out).into(),
            (A::Prompted(s), Event::Failure { .. }) => s.failed(ctx, out).into(),
            (A::Starting(s), Event::Unreachable { .. }) => s.unreachable(ctx, out).into(),
            (A::Prompted(s), Event::Submit) => A::Checking(s.answer(out)),
            (A::Starting(s), Event::ChooseIdentity(i)) => ok_or(s.choose(ctx, i, out), A::Starting, A::Starting),
            (A::Prompted(s), Event::ChooseIdentity(i)) => ok_or(s.choose(ctx, i, out), A::Starting, A::Prompted),
            (A::Checking(s), Event::ChooseIdentity(i)) => ok_or(s.choose(ctx, i, out), A::Starting, A::Checking),
            (A::RetryWait(s), Event::ChooseIdentity(i)) => ok_or(s.choose(ctx, i, out), A::Starting, A::RetryWait),
            (A::Unavailable(s), Event::ChooseIdentity(i)) => ok_or(s.choose(ctx, i, out), A::Starting, A::Unavailable),
            (A::RetryWait(s), Event::RetryTimer) => A::Starting(s.retry(ctx, out)),
            (s, _) => s,
        };
    }

    fn state(&self) -> StateName {
        match self.state {
            Any::Idle(_) => StateName::Idle,
            Any::Starting(_) => StateName::Starting,
            Any::Prompted(_) => StateName::Prompted,
            Any::Checking(_) => StateName::Checking,
            Any::RetryWait(_) => StateName::RetryWait,
            Any::Unavailable(_) => StateName::Unavailable,
        }
    }
}
