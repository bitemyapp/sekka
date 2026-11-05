//! One authentication request's conversations with polkit's helper, as
//! typestate: a [`Conversation`] is in one of its states, each a type of its
//! own, and only the transitions here move it.
//!
//! ```text
//! Idle ─start→ Dialing ─dialed→ Starting ─Prompt→ Prompted ─answer→ Checking
//!                 │                 │                 │                │
//!                 └── connect error, EOF or FAILURE with no answer ────┤
//!                                  ↓                                   │
//!                     Cooling ─due→ Dialing     (FAILURE after an answer:
//!                     Unavailable ─start→ …      Refused, then the floor)
//! any state ─cancel or SUCCESS→ Over
//! ```
//!
//! What the types hold to:
//!
//! - **An event from the helper, the connection or the retry timer acts
//!   only on the conversation it belongs to.** Each carries its
//!   conversation's [`Ticket`]; a transition that applies one takes a
//!   [`Now`], which only [`ConvAny::check`] makes, from
//!   [`Generation::check_answer`]. A `Now` lives only as long as the step
//!   that made it (one `with_typed`, under the session's lock), so a check
//!   made under one lock can't be spent under another, where a newer
//!   conversation may have started; and the transition compares the
//!   proof's ticket with its own state's, so it can't be spent on another
//!   state in the same step.
//! - **Nothing starts the helper sooner than the floor allows.** Starting a
//!   helper takes a [`Dial`], and only the floor (`gate`) makes the proof a
//!   `Dial` needs; the code that spawns or connects to the helper is the
//!   floor's module's. Every attempt that ends without success (a refused
//!   answer, no answer, left for another identity) delays the next one
//!   ([`delay`]), for all the agent's requests, until a success or a quiet
//!   [`QUIET`]. After [`AUTOMATIC_ATTEMPTS`] in a row with no answer, no
//!   attempt starts until the UI asks again.
//! - **An answer goes only to the prompt it answers, of the request it
//!   came from**, once: [`Asked::answer`] sends it through the `Asked`'s
//!   own request, the answer carries the request, the conversation and the
//!   prompt, and only [`Prompted`] takes it, comparing all three. It is one
//!   line the helper reads whole: [`Reply`] can't hold a line break, a NUL
//!   or more than [`MAX_ANSWER`] bytes. It's written outside the session's
//!   lock.
//! - **polkitd gets one reply**: only the transitions into [`Over`] send it,
//!   and `Over` has none.
//!
//! What stays a run-time fact: which ticket a line, a connection or a timer
//! belongs to (they come from other threads), which request an answer is
//! for (the type can't name an instance), and the time. The floor reads the
//! clock itself; no caller passes it a time.

// Typestate code: every state and every case is named, so a new one
// added later can't fall into an arm written for others.
#![deny(clippy::wildcard_enum_match_arm)]

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use aizome_typestate::{Answer, Fresh, Generation, Ticket};

use super::{AuthEvent, AuthRequest, Ender, HelperLine};
use crate::error::Error;
use crate::oneshot;
use crate::service::lock;

pub(super) use gate::SharedFloor;
pub use gate::{AUTOMATIC_ATTEMPTS, Dial, QUIET, delay};
use gate::{Floor, Retry};

/// Names one request's conversations with the helper, for their
/// [`Ticket`]s.
pub enum Talk {}

/// Which authentication request an event is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

impl RequestId {
    pub(super) fn next() -> RequestId {
        static LAST: AtomicU64 = AtomicU64::new(0);
        RequestId(LAST.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// What a request's conversation said, for the UI. Its payload is read
/// only from a proof that it is about the request's latest conversation:
/// [`AuthRequest::check`], then [`said`].
///
/// ```compile_fail,E0308
/// use aizome_sys::polkit::{Heard, Said, Talk, said};
/// use aizome_typestate::Generation;
/// // A proof from a generation of one's own, not the request's check.
/// fn forged(heard: Heard) -> Option<Said> {
///     let mut mine = Generation::<Talk>::new();
///     mine.issue();
///     let fresh = mine.check_answer(heard).ok()?;
///     Some(said(fresh))
/// }
/// ```
///
/// ```
/// use aizome_sys::polkit::{AuthRequest, Heard, Said, said};
/// fn read(request: &AuthRequest, heard: Heard) -> Option<Said> {
///     request.check(heard).ok().map(said)
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Heard {
    ticket: Ticket<Talk>,
    request: RequestId,
    said: Said,
}

impl Heard {
    /// The request it's about (not its conversation, which only the check
    /// can tell).
    pub fn request(&self) -> RequestId {
        self.request
    }
}

impl Answer<Talk> for Heard {
    fn ticket(&self) -> Ticket<Talk> {
        self.ticket
    }
}

/// What the dialog shows.
#[derive(Clone, Debug)]
pub enum Said {
    /// The helper asks for a line: answer it with [`Asked::answer`].
    Prompt(Asked),
    /// PAM's error ("Authentication failure").
    Error(String),
    /// PAM's message ("Touch the fingerprint reader").
    Info(String),
    /// The answer was refused; the next attempt has started.
    Refused,
    /// The next attempt starts after `after`: the last one ended with no
    /// answer (`failed` says how), or the floor holds a start the UI asked
    /// for (`failed` is `None`).
    Retrying { after: Duration, failed: Option<String> },
    /// Attempts keep ending with no answer, so none starts until the UI asks
    /// again ([`AuthRequest::authenticate`]).
    Unavailable { failed: String },
}

/// An event proven, by its request's [`AuthRequest::check`], to be about
/// that request's latest conversation. Only the check makes one.
#[derive(Debug)]
pub struct Checked {
    fresh: Fresh<Talk, Heard>,
}

impl Checked {
    /// The request it's about.
    pub fn request(&self) -> RequestId {
        self.fresh.answer().request
    }
}

/// The payload of an event proven to be about the latest conversation.
pub fn said(checked: Checked) -> Said {
    checked.fresh.into_answer().said
}

/// The longest answer polkit's helper reads as one line: it reads each into
/// a `PAM_MAX_RESP_SIZE` (512) buffer, newline and terminator included.
pub const MAX_ANSWER: usize = 510;

/// One prompt of one request's conversation, and the right to answer it
/// once. The answer goes to the request the prompt came from: it carries
/// the request, and nothing else names where the answer goes.
///
/// ```compile_fail,E0599
/// use aizome_sys::polkit::{Asked, AuthRequest};
/// // A's prompt answered through B: B's helper would take A's password.
/// fn through(b: &AuthRequest, asked_a: Asked) {
///     b.respond(asked_a, "secret");
/// }
/// ```
///
/// ```
/// use aizome_sys::polkit::{Asked, Unsent};
/// fn answer(asked_a: Asked) -> Result<(), Unsent> {
///     asked_a.answer("secret")
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Asked {
    ticket: Ticket<Talk>,
    request: RequestId,
    seq: u32,
    text: String,
    echo: bool,
    to: Weak<super::Session>,
}

impl Asked {
    /// PAM's prompt ("Password: ").
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether what's typed may show (a user name), or not (a password).
    pub fn echo(&self) -> bool {
        self.echo
    }

    /// Answers this prompt, of this request. The text is written to the
    /// helper only if this prompt is still the one waiting, and the agent's
    /// copy is overwritten. Text the helper can't read as one line (a line
    /// break, a NUL, more than [`MAX_ANSWER`] bytes) is refused here, and
    /// the prompt handed back.
    pub fn answer(self, text: &str) -> Result<(), Unsent> {
        let reply = match Reply::new(&self, text) {
            Ok(reply) => reply,
            Err(why) => return Err(Unsent { asked: self, why }),
        };
        // A request that's over has no helper to answer.
        if let Some(session) = self.to.upgrade() {
            session.answer(reply);
        }
        Ok(())
    }
}

/// Why an answer wasn't sent: the helper reads one line per prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsendable {
    /// Its tail would answer a later prompt.
    LineBreak,
    /// PAM would read only what's before it.
    Nul,
    /// Longer than [`MAX_ANSWER`]: the rest would answer a later prompt.
    TooLong,
}

/// An answer that wasn't sent, and the prompt to answer instead.
#[derive(Debug)]
pub struct Unsent {
    pub asked: Asked,
    pub why: Unsendable,
}

/// An answer to one prompt of one request: one line the helper reads
/// whole. Its text is wiped when it's dropped.
pub struct Reply {
    ticket: Ticket<Talk>,
    request: RequestId,
    seq: u32,
    text: Vec<u8>,
}

impl Reply {
    fn new(asked: &Asked, text: &str) -> Result<Reply, Unsendable> {
        let bytes = text.as_bytes();
        if bytes.contains(&b'\n') || bytes.contains(&b'\r') {
            return Err(Unsendable::LineBreak);
        }
        if bytes.contains(&0) {
            return Err(Unsendable::Nul);
        }
        if bytes.len() > MAX_ANSWER {
            return Err(Unsendable::TooLong);
        }
        Ok(Reply { ticket: asked.ticket, request: asked.request, seq: asked.seq, text: bytes.to_vec() })
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.text.fill(0);
    }
}

impl Answer<Talk> for Reply {
    fn ticket(&self) -> Ticket<Talk> {
        self.ticket
    }
}

/// A line the helper wrote, or the end of its output (`None`).
pub struct Line {
    ticket: Ticket<Talk>,
    said: Option<HelperLine>,
}

impl Answer<Talk> for Line {
    fn ticket(&self) -> Ticket<Talk> {
        self.ticket
    }
}

/// Stamps one conversation's lines with its ticket.
pub struct Lines {
    ticket: Ticket<Talk>,
}

impl Lines {
    pub(super) fn line(&self, text: &str) -> Line {
        Line { ticket: self.ticket, said: Some(super::parse_line(text)) }
    }

    pub(super) fn ended(&self) -> Line {
        Line { ticket: self.ticket, said: None }
    }
}

/// A helper's ends, as [`Dial::connect`] opens them.
pub(super) struct Pipe {
    pub(super) reader: Box<dyn BufRead + Send>,
    pub(super) writer: Box<dyn Write + Send>,
    pub(super) ender: Ender,
}

/// How starting a helper went.
pub struct Dialed {
    ticket: Ticket<Talk>,
    pipe: Result<Pipe, Error>,
}

impl Answer<Talk> for Dialed {
    fn ticket(&self) -> Ticket<Talk> {
        self.ticket
    }
}

impl Dialed {
    /// A helper started for a conversation that's no longer wanted: end it.
    pub(super) fn abandon(self) {
        if let Ok(Pipe { reader, writer, ender }) = self.pipe {
            drop((reader, writer));
            ender.end();
        }
    }
}

/// Read the helper of a started conversation, on a thread of its own.
pub struct Converse {
    lines: Lines,
    reader: Box<dyn BufRead + Send>,
    ender: Ender,
}

impl Converse {
    pub(super) fn into_parts(self) -> (Lines, Box<dyn BufRead + Send>, Ender) {
        (self.lines, self.reader, self.ender)
    }

    /// What its end will be, if its thread can't start.
    pub(super) fn ended(&self) -> Line {
        self.lines.ended()
    }
}

/// When to try again.
pub struct Wake {
    ticket: Ticket<Talk>,
    at: Instant,
}

impl Wake {
    pub(super) fn at(&self) -> Instant {
        self.at
    }

    /// The timer went off.
    pub(super) fn fire(self) -> Due {
        Due { ticket: self.ticket }
    }
}

/// A retry's timer went off.
pub struct Due {
    ticket: Ticket<Talk>,
}

impl Answer<Talk> for Due {
    fn ticket(&self) -> Ticket<Talk> {
        self.ticket
    }
}

/// The helper's input, shared by its running state and the answer being
/// written to it.
type Input = Arc<Mutex<Box<dyn Write + Send>>>;

/// An answer to write to the helper, outside the session's lock. It is one
/// line of at most [`MAX_ANSWER`] bytes, which a pipe's or a socket's
/// buffer takes without waiting. Its copy is wiped when it's dropped.
pub struct Answering {
    to: Input,
    line: Vec<u8>,
}

impl Answering {
    pub(super) fn send(self) {
        let mut to = lock(&self.to);
        if let Err(error) = to.write_all(&self.line).and_then(|()| to.flush()) {
            // The helper is gone: its output ends next, as a refusal.
            log::warn!("aizome-sys: polkit helper: {error}");
        }
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        self.line.fill(0);
    }
}

/// What the session does once its lock is released.
#[must_use = "a step's work is done outside the session's lock"]
pub enum Then {
    Nothing,
    /// Start the helper.
    Dial(Dial),
    /// Write an answer to the helper.
    Answer(Answering),
    /// Wait, then try again.
    Wake(Wake),
    /// Read the helper.
    Converse(Converse),
    /// End a helper no conversation wants.
    Abandon(Dialed),
}

/// An answer proven to be about the latest conversation, for this step only:
/// only [`ConvAny::check`] makes one, and it can't leave the step. The old
/// relay checked that its conversation was current under the session's
/// lock, then cleared the conversation under the lock again, by which time
/// a newer one could have started. A proof kept past its step doesn't
/// compile (a lifetime error, which has no code):
///
/// ```compile_fail
/// use aizome_sys::polkit::conversation::{ConvAny, Line, Now};
/// fn keep(state: &ConvAny<'_>, line: Line) -> Now<'static, Line> {
///     state.check(line).ok().unwrap()
/// }
/// ```
///
/// ```
/// use aizome_sys::polkit::conversation::{ConvAny, Line, Now};
/// fn keep<'s>(state: &ConvAny<'s>, line: Line) -> Option<Now<'s, Line>> {
///     state.check(line).ok()
/// }
/// ```
pub struct Now<'s, A> {
    fresh: Fresh<Talk, A>,
    step: PhantomData<fn(&'s ()) -> &'s ()>,
}

impl<'s, A> Now<'s, A> {
    pub(super) fn into_answer(self) -> A {
        self.fresh.into_answer()
    }

    /// Whether it names `ticket`: a state holds the ticket of the attempt
    /// it is, so a proof checked against one state can't drive another.
    fn names(&self, ticket: Ticket<Talk>) -> bool {
        self.fresh.current().ticket() == ticket
    }
}

/// Whether the conversation's answers have started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Spoken {
    /// No answer written yet.
    Silent,
    /// An earlier prompt was answered.
    Answered,
}

/// What the shared context holds.
pub(super) struct Ctx {
    request: RequestId,
    cookie: String,
    talks: Generation<Talk>,
    /// polkitd's reply, until it's sent.
    reply: Option<oneshot::Sender<bool>>,
    /// The agent's floor, which every request shares.
    floor: SharedFloor,
    prompts: u32,
    /// The session, for the prompts it hands out to answer through.
    session: Weak<super::Session>,
    /// What the UI is told, in order.
    outbox: VecDeque<AuthEvent>,
}

impl Ctx {
    pub(super) fn new(
        request: RequestId,
        cookie: String,
        reply: oneshot::Sender<bool>,
        floor: SharedFloor,
        session: Weak<super::Session>,
    ) -> Ctx {
        Ctx {
            request,
            cookie,
            talks: Generation::new(),
            reply: Some(reply),
            floor,
            prompts: 0,
            session,
            outbox: VecDeque::new(),
        }
    }

    fn floor(&self) -> std::sync::MutexGuard<'_, Floor> {
        lock(&self.floor)
    }

    fn tell(&mut self, ticket: Ticket<Talk>, said: Said) {
        self.outbox.push_back(AuthEvent::Heard(Heard { ticket, request: self.request, said }));
    }
}

/// An attempt being started.
pub(super) struct Attempt {
    ticket: Ticket<Talk>,
    user: String,
}

/// A helper running. Leaving its state ends it.
pub(super) struct Running {
    ticket: Ticket<Talk>,
    user: String,
    writer: Input,
    ender: Ender,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.ender.stop();
    }
}

/// A helper's prompt, awaiting its answer.
pub(super) struct Prompting {
    running: Running,
    seq: u32,
    after: Spoken,
}

/// The next attempt, and when it may start.
pub(super) struct Planned {
    ticket: Ticket<Talk>,
    user: String,
    at: Instant,
}

aizome_typestate::machine! {
    /// One authentication request's conversations with polkit's helper.
    pub struct Conversation over Ctx {
        /// Where the conversation is.
        stored ConvState;
        /// The conversation's typed state, holding its context.
        typed ConvAny;
        states {
            /// polkitd asked; no attempt has started.
            Idle,
            /// The helper is being started, outside the session's lock.
            Dialing(Attempt),
            /// The helper runs and hasn't asked anything yet.
            Starting(Running),
            /// The helper asked; the answer is awaited.
            Prompted(Prompting),
            /// An answer was written; the helper is checking it.
            Checking(Running),
            /// An attempt ended with no answer: the next waits for the floor.
            Cooling(Planned),
            /// Attempts keep ending with no answer: none starts until the UI
            /// asks again.
            Unavailable,
            /// polkitd has its reply, and gets no other:
            ///
            /// ```compile_fail,E0599
            /// use aizome_sys::polkit::conversation::Over;
            /// fn again(over: Over<'_>) {
            ///     over.cancel();
            /// }
            /// ```
            ///
            /// ```
            /// use aizome_sys::polkit::conversation::{Over, Starting};
            /// fn dismissed(starting: Starting<'_>) -> Over<'_> {
            ///     starting.cancel()
            /// }
            /// ```
            Over,
        }
        access(private);
        new(pub(super));
        with_typed(pub(super));
    }
}

/// A step's outcome: the state it ends in, and the work it leaves.
pub type Step<'s> = (ConvAny<'s>, Then);

impl Conversation {
    /// Tells the UI first that polkitd asked.
    pub(super) fn begun(&mut self, request: AuthRequest) {
        self.ctx.outbox.push_front(AuthEvent::Begin(request));
    }

    /// The next thing to tell the UI.
    pub(super) fn next_event(&mut self) -> Option<AuthEvent> {
        self.ctx.outbox.pop_front()
    }

    /// The UI's check of an event: it's about this request's latest
    /// conversation.
    pub(super) fn check_heard(&self, heard: Heard) -> Result<Checked, Heard> {
        if heard.request != self.ctx.request {
            return Err(heard);
        }
        self.ctx.talks.check_answer(heard).map(|fresh| Checked { fresh })
    }
}

impl<'s> ConvAny<'s> {
    fn ctx(&self) -> &Ctx {
        match self {
            ConvAny::Idle(s) => &*s.ctx,
            ConvAny::Dialing(s) => &*s.ctx,
            ConvAny::Starting(s) => &*s.ctx,
            ConvAny::Prompted(s) => &*s.ctx,
            ConvAny::Checking(s) => &*s.ctx,
            ConvAny::Cooling(s) => &*s.ctx,
            ConvAny::Unavailable(s) => &*s.ctx,
            ConvAny::Over(s) => &*s.ctx,
        }
    }

    /// The proof that `answer` is about the latest conversation, for this
    /// step, or the answer back when a newer conversation superseded its
    /// own (or the request is over).
    pub fn check<A: Answer<Talk>>(&self, answer: A) -> Result<Now<'s, A>, A> {
        self.ctx().talks.check_answer(answer).map(|fresh| Now { fresh, step: PhantomData })
    }
}

/// Starts an attempt as `user` under `ticket`: now, if the floor allows, or
/// when it does.
fn attempt(ctx: &mut Ctx, ticket: Ticket<Talk>, user: String) -> Step<'_> {
    let rested = ctx.floor().rested();
    match rested {
        Ok(rested) => {
            let dial = Dial::new(rested, ticket, user.clone());
            (Dialing::enter(ctx, Attempt { ticket, user }).into(), Then::Dial(dial))
        }
        Err(at) => {
            let after = at.saturating_duration_since(Instant::now());
            ctx.tell(ticket, Said::Retrying { after, failed: None });
            (Cooling::enter(ctx, Planned { ticket, user, at }).into(), Then::Wake(Wake { ticket, at }))
        }
    }
}

/// An attempt ended with no answer written: the next waits, or none starts.
fn unanswered(ctx: &mut Ctx, user: String, failed: String) -> Step<'_> {
    log::warn!("aizome-sys: polkit authentication: {failed}");
    let ticket = ctx.talks.issue();
    let retry = ctx.floor().unanswered();
    match retry {
        Retry::After(after) => {
            let at = Instant::now() + after;
            ctx.tell(ticket, Said::Retrying { after, failed: Some(failed) });
            (Cooling::enter(ctx, Planned { ticket, user, at }).into(), Then::Wake(Wake { ticket, at }))
        }
        Retry::Stop => {
            ctx.tell(ticket, Said::Unavailable { failed });
            (Unavailable::enter(ctx, ()).into(), Then::Nothing)
        }
    }
}

/// An attempt ended after an answer: polkit refused it. The next starts as
/// soon as the floor allows: a refusal counts as a failure too, so a UI
/// that answers every prompt by itself can't start helpers in a loop.
fn refused(ctx: &mut Ctx, user: String) -> Step<'_> {
    ctx.floor().failed();
    let ticket = ctx.talks.issue();
    ctx.tell(ticket, Said::Refused);
    attempt(ctx, ticket, user)
}

/// polkitd's reply, once, and the UI told.
fn over(ctx: &mut Ctx, authorized: bool) -> Over<'_> {
    if authorized {
        ctx.floor().succeeded();
    }
    ctx.talks.retire();
    if let Some(reply) = ctx.reply.take() {
        reply.send(authorized);
    }
    let done = AuthEvent::Done { cookie: ctx.cookie.clone(), authorized, request: ctx.request };
    ctx.outbox.push_back(done);
    Over::enter(ctx, ())
}

/// A conversation is left before it succeeded: it counts as a failure (one
/// helper after another, whoever asks, is what the floor is for), then the
/// next starts when the floor allows.
fn supersede(ctx: &mut Ctx, user: String) -> Step<'_> {
    ctx.floor().failed();
    let ticket = ctx.talks.issue();
    attempt(ctx, ticket, user)
}

/// The helper asked.
fn prompt(ctx: &mut Ctx, running: Running, text: String, echo: bool, after: Spoken) -> Step<'_> {
    ctx.prompts += 1;
    let seq = ctx.prompts;
    let asked = Asked { ticket: running.ticket, request: ctx.request, seq, text, echo, to: ctx.session.clone() };
    ctx.tell(running.ticket, Said::Prompt(asked));
    (Prompted::enter(ctx, Prompting { running, seq, after }).into(), Then::Nothing)
}

/// The helper's end without SUCCESS, or a line it shouldn't have written.
fn ended(line: &Option<HelperLine>) -> String {
    match line {
        None => "polkit's helper stopped".into(),
        Some(HelperLine::Failure) => "polkit's helper refused".into(),
        Some(HelperLine::Unknown(text)) => format!("unknown line from polkit's helper: {text:?}"),
        Some(HelperLine::Prompt { .. } | HelperLine::Error(_) | HelperLine::Info(_) | HelperLine::Success) => {
            "polkit's helper".into()
        }
    }
}

impl<'s> Idle<'s> {
    /// The first attempt, as `user`.
    pub fn start(self, user: String) -> Step<'s> {
        let ticket = self.ctx.talks.issue();
        attempt(self.ctx, ticket, user)
    }

    /// The person dismissed the dialog.
    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

impl<'s> Dialing<'s> {
    /// Another attempt, as `user`; this one's helper is ended when it's up.
    pub fn start(self, user: String) -> Step<'s> {
        supersede(self.ctx, user)
    }

    /// The helper is up, or couldn't be started.
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::polkit::conversation::{Dialed, Dialing, Step};
    /// // A connection's result applied without checking it's this attempt's.
    /// fn up<'s>(dialing: Dialing<'s>, dialed: Dialed) -> Step<'s> {
    ///     dialing.dialed(dialed)
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_sys::polkit::conversation::{ConvAny, Dialed, Dialing, Step, Then};
    /// fn up<'s>(dialing: Dialing<'s>, dialed: Dialed) -> Step<'s> {
    ///     let any = ConvAny::from(dialing);
    ///     match any.check(dialed) {
    ///         Ok(now) => match any {
    ///             ConvAny::Dialing(dialing) => dialing.dialed(now),
    ///             other => (other, Then::Nothing),
    ///         },
    ///         Err(stale) => (any, Then::Abandon(stale)),
    ///     }
    /// }
    /// ```
    pub fn dialed(self, now: Now<'s, Dialed>) -> Step<'s> {
        if !now.names(self.data.ticket) {
            return (self.into(), Then::Abandon(now.into_answer()));
        }
        let Dialing { ctx, data: Attempt { ticket, user } } = self;
        match now.into_answer().pipe {
            Ok(Pipe { reader, writer, ender }) => {
                let converse = Converse { lines: Lines { ticket }, reader, ender: ender.clone() };
                let writer = Arc::new(Mutex::new(writer));
                (Starting::enter(ctx, Running { ticket, user, writer, ender }).into(), Then::Converse(converse))
            }
            Err(error) => unanswered(ctx, user, format!("polkit's helper: {}", error.describe())),
        }
    }

    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

impl<'s> Starting<'s> {
    /// Another attempt, as `user`.
    pub fn start(self, user: String) -> Step<'s> {
        let Starting { ctx, data } = self;
        drop(data);
        supersede(ctx, user)
    }

    /// The helper wrote a line, or its output ended.
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::polkit::conversation::{Line, Starting, Step};
    /// // A line from whichever helper wrote it, applied to this attempt.
    /// fn hear<'s>(starting: Starting<'s>, line: Line) -> Step<'s> {
    ///     starting.heard(line)
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_sys::polkit::conversation::{ConvAny, Line, Starting, Step, Then};
    /// fn hear<'s>(starting: Starting<'s>, line: Line) -> Step<'s> {
    ///     let any = ConvAny::from(starting);
    ///     match any.check(line) {
    ///         Ok(now) => match any {
    ///             ConvAny::Starting(starting) => starting.heard(now),
    ///             other => (other, Then::Nothing),
    ///         },
    ///         Err(_stale) => (any, Then::Nothing),
    ///     }
    /// }
    /// ```
    pub fn heard(self, now: Now<'s, Line>) -> Step<'s> {
        if !now.names(self.data.ticket) {
            return (self.into(), Then::Nothing);
        }
        let Starting { ctx, data: mut running } = self;
        let line = now.into_answer().said;
        match line {
            Some(HelperLine::Prompt { text, echo }) => prompt(ctx, running, text, echo, Spoken::Silent),
            Some(HelperLine::Error(text)) => {
                ctx.tell(running.ticket, Said::Error(text));
                (Starting::enter(ctx, running).into(), Then::Nothing)
            }
            Some(HelperLine::Info(text)) => {
                ctx.tell(running.ticket, Said::Info(text));
                (Starting::enter(ctx, running).into(), Then::Nothing)
            }
            Some(HelperLine::Success) => {
                drop(running);
                (over(ctx, true).into(), Then::Nothing)
            }
            Some(HelperLine::Failure | HelperLine::Unknown(_)) | None => {
                let why = ended(&line);
                let user = std::mem::take(&mut running.user);
                drop(running);
                unanswered(ctx, user, format!("{why} before asking anything"))
            }
        }
    }

    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

impl<'s> Prompted<'s> {
    /// Another attempt, as `user`.
    pub fn start(self, user: String) -> Step<'s> {
        let Prompted { ctx, data } = self;
        drop(data);
        supersede(ctx, user)
    }

    /// The person's answer to the prompt. It's taken only if it answers this
    /// prompt of this request; any other is dropped. This is the only step
    /// that leads to writing an answer, and the write happens outside the
    /// session's lock ([`Answering`]).
    ///
    /// Only this state takes an answer, so one sent before the helper asked
    /// has nowhere to go:
    ///
    /// ```compile_fail,E0599
    /// use aizome_sys::polkit::conversation::{Answering, Checking, Now, Reply, Starting};
    /// fn early<'s>(starting: Starting<'s>, reply: Now<'s, Reply>) -> (Checking<'s>, Answering) {
    ///     starting.answer(reply).ok().unwrap()
    /// }
    /// ```
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::polkit::conversation::{Answering, Checking, Prompted, Reply};
    /// // An answer applied without checking it's this conversation's.
    /// fn answer<'s>(prompted: Prompted<'s>, reply: Reply) -> Result<(Checking<'s>, Answering), Prompted<'s>> {
    ///     prompted.answer(reply)
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_sys::polkit::conversation::{Answering, Checking, Now, Prompted, Reply};
    /// fn answer<'s>(prompted: Prompted<'s>, reply: Now<'s, Reply>) -> Result<(Checking<'s>, Answering), Prompted<'s>> {
    ///     prompted.answer(reply)
    /// }
    /// ```
    pub fn answer(self, now: Now<'s, Reply>) -> Result<(Checking<'s>, Answering), Prompted<'s>> {
        // Tickets and prompt numbers count per request: the request, the
        // conversation and the prompt must all be this one's.
        let reply = now.into_answer();
        if reply.request != self.ctx.request || reply.ticket != self.data.running.ticket || reply.seq != self.data.seq {
            return Err(self);
        }
        let Prompted { ctx, data: Prompting { running, .. } } = self;
        let mut line = Vec::with_capacity(reply.text.len() + 1);
        line.extend_from_slice(&reply.text);
        line.push(b'\n');
        let answering = Answering { to: running.writer.clone(), line };
        ctx.floor().answered();
        Ok((Checking::enter(ctx, running), answering))
    }

    /// The helper wrote a line, or its output ended.
    pub fn heard(self, now: Now<'s, Line>) -> Step<'s> {
        if !now.names(self.data.running.ticket) {
            return (self.into(), Then::Nothing);
        }
        let Prompted { ctx, data: Prompting { mut running, seq, after } } = self;
        let line = now.into_answer().said;
        match line {
            Some(HelperLine::Prompt { text, echo }) => prompt(ctx, running, text, echo, after),
            Some(HelperLine::Error(text)) => {
                ctx.tell(running.ticket, Said::Error(text));
                (Prompted::enter(ctx, Prompting { running, seq, after }).into(), Then::Nothing)
            }
            Some(HelperLine::Info(text)) => {
                ctx.tell(running.ticket, Said::Info(text));
                (Prompted::enter(ctx, Prompting { running, seq, after }).into(), Then::Nothing)
            }
            Some(HelperLine::Success) => {
                drop(running);
                (over(ctx, true).into(), Then::Nothing)
            }
            Some(HelperLine::Failure | HelperLine::Unknown(_)) | None => {
                let why = ended(&line);
                let user = std::mem::take(&mut running.user);
                drop(running);
                match after {
                    Spoken::Answered => refused(ctx, user),
                    Spoken::Silent => unanswered(ctx, user, format!("{why} before the prompt was answered")),
                }
            }
        }
    }

    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

impl<'s> Checking<'s> {
    /// Another attempt, as `user`.
    pub fn start(self, user: String) -> Step<'s> {
        let Checking { ctx, data } = self;
        drop(data);
        supersede(ctx, user)
    }

    /// The helper wrote a line, or its output ended.
    pub fn heard(self, now: Now<'s, Line>) -> Step<'s> {
        if !now.names(self.data.ticket) {
            return (self.into(), Then::Nothing);
        }
        let Checking { ctx, data: mut running } = self;
        let line = now.into_answer().said;
        match line {
            Some(HelperLine::Prompt { text, echo }) => prompt(ctx, running, text, echo, Spoken::Answered),
            Some(HelperLine::Error(text)) => {
                ctx.tell(running.ticket, Said::Error(text));
                (Checking::enter(ctx, running).into(), Then::Nothing)
            }
            Some(HelperLine::Info(text)) => {
                ctx.tell(running.ticket, Said::Info(text));
                (Checking::enter(ctx, running).into(), Then::Nothing)
            }
            Some(HelperLine::Success) => {
                drop(running);
                (over(ctx, true).into(), Then::Nothing)
            }
            Some(HelperLine::Failure | HelperLine::Unknown(_)) | None => {
                if let Some(HelperLine::Unknown(text)) = &line {
                    log::warn!("aizome-sys: unknown line from the polkit helper: {text:?}");
                }
                let user = std::mem::take(&mut running.user);
                drop(running);
                refused(ctx, user)
            }
        }
    }

    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

impl<'s> Cooling<'s> {
    /// The UI asks for an attempt as `user`: it's the planned one, at the
    /// planned time.
    pub fn start(mut self, user: String) -> Cooling<'s> {
        self.data.user = user;
        self
    }

    /// The retry's timer went off: the planned attempt starts, if the floor
    /// allows.
    ///
    /// The attempt that ended can't start the next by itself: only the
    /// timer's [`Due`] can, and a line or a connection result isn't one.
    ///
    /// ```compile_fail,E0308
    /// use aizome_sys::polkit::conversation::{Cooling, Line, Now, Step};
    /// // The helper's end, taken as the cue to start again.
    /// fn again<'s>(cooling: Cooling<'s>, ended: Now<'s, Line>) -> Step<'s> {
    ///     cooling.due(ended)
    /// }
    /// ```
    ///
    /// ```compile_fail,E0451
    /// use aizome_sys::polkit::conversation::Due;
    /// use aizome_typestate::Ticket;
    /// use aizome_sys::polkit::conversation::Talk;
    /// // A timer that went off at once.
    /// fn now(ticket: Ticket<Talk>) -> Due {
    ///     Due { ticket }
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_sys::polkit::conversation::{Cooling, Due, Now, Step};
    /// fn retry<'s>(cooling: Cooling<'s>, due: Now<'s, Due>) -> Step<'s> {
    ///     cooling.due(due)
    /// }
    /// ```
    pub fn due(self, timer: Now<'s, Due>) -> Step<'s> {
        if !timer.names(self.data.ticket) {
            return (self.into(), Then::Nothing);
        }
        let Cooling { ctx, data: Planned { ticket, user, at } } = self;
        let rested = ctx.floor().rested();
        match rested {
            Ok(rested) => {
                let dial = Dial::new(rested, ticket, user.clone());
                (Dialing::enter(ctx, Attempt { ticket, user }).into(), Then::Dial(dial))
            }
            // Woken early: wait again.
            Err(later) => {
                let at = later.max(at);
                (Cooling::enter(ctx, Planned { ticket, user, at }).into(), Then::Wake(Wake { ticket, at }))
            }
        }
    }

    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

impl<'s> Unavailable<'s> {
    /// The UI asks again, as `user`: an attempt starts when the floor
    /// allows.
    pub fn start(self, user: String) -> Step<'s> {
        let ticket = self.ctx.talks.issue();
        attempt(self.ctx, ticket, user)
    }

    pub fn cancel(self) -> Over<'s> {
        over(self.ctx, false)
    }
}

/// The floor: when the next helper may start. Only it makes the proof a
/// [`Dial`] needs, so nothing can start a helper without asking it.
mod gate {
    use super::super::{ChildSlot, Ender, Helper};
    use super::{Dialed, Pipe, Talk, Ticket};
    use crate::error::{Error, Result};
    use std::io::{BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Attempts in a row that end with no answer before automatic ones
    /// stop.
    pub const AUTOMATIC_ATTEMPTS: u8 = 3;

    /// How long without a failed attempt before the floor starts afresh.
    pub const QUIET: Duration = Duration::from_secs(60);

    /// How long the next attempt waits after `failed` attempts in a row
    /// ended without success: none after none, then half a second,
    /// doubling, at most four seconds.
    pub fn delay(failed: u8) -> Duration {
        match failed {
            0 => Duration::ZERO,
            n => Duration::from_millis(500 << u32::from(n - 1).min(3)),
        }
    }

    /// Proof the floor allows a helper to start now.
    pub struct Rested(());

    /// The agent's floor, shared by its requests: a new request doesn't
    /// start afresh.
    pub type SharedFloor = Arc<Mutex<Floor>>;

    /// Attempts in a row that ended without success, however they ended
    /// (no answer, a refused answer, left for another), and when the last
    /// did; of those, how many in a row ended with no answer written.
    #[derive(Debug, Default)]
    pub struct Floor {
        failed: u8,
        unanswered: u8,
        since: Option<Instant>,
    }

    /// After an unanswered attempt: when to try again, or not.
    pub enum Retry {
        After(Duration),
        Stop,
    }

    impl Floor {
        /// A quiet minute since the last failure: start afresh.
        fn settle(&mut self, now: Instant) {
            if self.since.is_some_and(|since| now.duration_since(since) >= QUIET) {
                *self = Floor::default();
            }
        }

        /// Whether a helper may start now, or when it may.
        pub fn rested(&mut self) -> Result<Rested, Instant> {
            let now = Instant::now();
            self.settle(now);
            match self.since {
                Some(since) if self.failed > 0 => {
                    let at = since + delay(self.failed);
                    if now >= at { Ok(Rested(())) } else { Err(at) }
                }
                Some(_) | None => Ok(Rested(())),
            }
        }

        /// An attempt ended without success: it delays the next.
        pub fn failed(&mut self) {
            let now = Instant::now();
            self.settle(now);
            self.failed = self.failed.saturating_add(1);
            self.since = Some(now);
        }

        /// An attempt ended with no answer written: it delays the next, and
        /// enough of them in a row stop automatic attempts.
        pub fn unanswered(&mut self) -> Retry {
            self.failed();
            self.unanswered = self.unanswered.saturating_add(1);
            if self.unanswered < AUTOMATIC_ATTEMPTS { Retry::After(delay(self.failed)) } else { Retry::Stop }
        }

        /// An answer was written: the helper works, so attempts that end
        /// with no answer count from none again. A refusal still counts as
        /// a failure.
        pub fn answered(&mut self) {
            self.unanswered = 0;
        }

        /// Moves the last failure `by` into the past.
        #[cfg(test)]
        pub fn age(&mut self, by: Duration) {
            self.since = self.since.map(|since| since - by);
        }

        /// An attempt succeeded.
        pub fn succeeded(&mut self) {
            *self = Floor::default();
        }
    }

    /// Leave to start the helper for one attempt, which only a [`Rested`]
    /// gives.
    ///
    /// ```compile_fail,E0451
    /// use aizome_sys::polkit::conversation::{Dial, Talk};
    /// use aizome_typestate::Ticket;
    /// // Start the helper again without asking the floor.
    /// fn at_once(ticket: Ticket<Talk>, user: String) -> Dial {
    ///     Dial { ticket, user, rested: todo!() }
    /// }
    /// ```
    pub struct Dial {
        ticket: Ticket<Talk>,
        user: String,
        rested: Rested,
    }

    impl Dial {
        pub(in super::super) fn new(rested: Rested, ticket: Ticket<Talk>, user: String) -> Dial {
            Dial { ticket, user, rested }
        }

        #[cfg(test)]
        pub(in super::super) fn ticket(&self) -> Ticket<Talk> {
            self.ticket
        }

        /// Who it authenticates.
        pub fn user(&self) -> &str {
            &self.user
        }
    }

    impl Dial {
        /// Starts the helper for this attempt. Nothing else in the crate can:
        /// what starts it is this module's.
        pub(in super::super) fn connect(self, helper: &Helper, cookie: &str) -> Dialed {
            let Dial { ticket, user, rested: Rested(()) } = self;
            Dialed { ticket, pipe: open(helper, &user, cookie) }
        }
    }

    /// Start the helper for `user` and hand it the cookie.
    fn open(helper: &Helper, user: &str, cookie: &str) -> Result<Pipe> {
        if user.contains('\n') || cookie.contains('\n') {
            return Err(Error::InvalidArgument("a line break in a user name or cookie".into()));
        }
        match helper {
            Helper::Socket(path) => {
                let stream = UnixStream::connect(path)?;
                let mut writer = stream.try_clone()?;
                writer.write_all(format!("{user}\n{cookie}\n").as_bytes())?;
                writer.flush()?;
                let reader = BufReader::new(stream.try_clone()?);
                Ok(Pipe { reader: Box::new(reader), writer: Box::new(writer), ender: Ender::Stream(Arc::new(stream)) })
            }
            Helper::Program(path) => {
                let mut child = Command::new(path)
                    .arg(user)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .env("LC_ALL", "C")
                    .spawn()
                    .map_err(|e| Error::Unavailable(format!("{}: {e}", path.display())))?;
                let stdin = child.stdin.take();
                let stdout = child.stdout.take();
                // From here a failure drops the slot, which ends and reaps it.
                let ender = Ender::Child(Arc::new(ChildSlot(Mutex::new(Some(child)))));
                let mut stdin = stdin.ok_or_else(|| Error::Failed("no helper stdin".into()))?;
                let stdout = stdout.ok_or_else(|| Error::Failed("no helper stdout".into()))?;
                stdin.write_all(format!("{cookie}\n").as_bytes())?;
                stdin.flush()?;
                Ok(Pipe { reader: Box::new(BufReader::new(stdout)), writer: Box::new(stdin), ender })
            }
        }
    }
}

#[cfg(test)]
// A test that reaches another state fails on what it asserts next.
#[allow(clippy::wildcard_enum_match_arm)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::os::unix::net::UnixStream;

    fn conversation() -> Conversation {
        let (reply, _) = oneshot::channel();
        Conversation::new(Ctx::new(RequestId::next(), "cookie".into(), reply, SharedFloor::default(), Weak::new()))
    }

    /// A helper that came up: its ends are one side of a socket pair.
    fn up(ticket: Ticket<Talk>) -> (Dialed, UnixStream) {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let pipe = Pipe {
            reader: Box::new(BufReader::new(ours.try_clone().unwrap())),
            writer: Box::new(ours.try_clone().unwrap()),
            ender: Ender::Stream(Arc::new(ours)),
        };
        (Dialed { ticket, pipe: Ok(pipe) }, theirs)
    }

    /// What the state is, by name.
    fn name(conversation: &Conversation) -> &'static str {
        match conversation.stored() {
            ConvState::Idle(()) => "idle",
            ConvState::Dialing(_) => "dialing",
            ConvState::Starting(_) => "starting",
            ConvState::Prompted(_) => "prompted",
            ConvState::Checking(_) => "checking",
            ConvState::Cooling(_) => "cooling",
            ConvState::Unavailable(()) => "unavailable",
            ConvState::Over(()) => "over",
        }
    }

    #[test]
    fn a_superseded_conversations_end_leaves_the_new_one_alone() {
        let mut c = conversation();
        let Then::Dial(dial) = c.with_typed(|s| match s {
            ConvAny::Idle(s) => s.start("me".into()),
            other => (other, Then::Nothing),
        }) else {
            panic!("the first attempt starts at once")
        };
        let (dialed, _helper) = up(dial.ticket());
        let Then::Converse(first) = c.with_typed(|s| {
            let now = s.check(dialed).ok().unwrap();
            match s {
                ConvAny::Dialing(s) => s.dialed(now),
                other => (other, Then::Nothing),
            }
        }) else {
            panic!("the helper is read")
        };
        assert_eq!(name(&c), "starting");
        // The person picks another identity before the first helper asks.
        let then = c.with_typed(|s| match s {
            ConvAny::Starting(s) => s.start("root".into()),
            other => (other, Then::Nothing),
        });
        assert!(matches!(then, Then::Wake(_)), "a second helper with nobody answering waits for the floor");
        let told = std::iter::from_fn(|| c.next_event()).count();
        // The first helper's thread reads its end only now.
        let ended = first.ended();
        let stale = c.with_typed(|s| {
            let stale = s.check(ended).is_err();
            (s, stale)
        });
        assert!(stale, "the superseded helper's end is refused");
        assert_eq!(name(&c), "cooling");
        assert_eq!(told, 1, "only the floor was told: {told}");
        assert!(c.next_event().is_none(), "the old end told nothing");
    }

    #[test]
    fn a_start_while_cooling_keeps_the_planned_time() {
        let mut c = conversation();
        let Then::Dial(dial) = c.with_typed(|s| match s {
            ConvAny::Idle(s) => s.start("me".into()),
            other => (other, Then::Nothing),
        }) else {
            panic!()
        };
        let failed = Dialed { ticket: dial.ticket(), pipe: Err(Error::NotFound("no socket".into())) };
        let then = c.with_typed(|s| {
            let now = s.check(failed).ok().unwrap();
            match s {
                ConvAny::Dialing(s) => s.dialed(now),
                other => (other, Then::Nothing),
            }
        });
        let Then::Wake(wake) = then else { panic!("an unreachable helper is retried later") };
        let at = wake.at();
        assert!(at > Instant::now());
        // The UI asks again at once, as the old dialog did: nothing starts.
        for _ in 0..100 {
            let then = c.with_typed(|s| match s {
                ConvAny::Cooling(s) => (s.start("me".into()).into(), Then::Nothing),
                other => (other, Then::Nothing),
            });
            assert!(matches!(then, Then::Nothing));
        }
        // An early timer waits again.
        let early = wake.fire();
        let then = c.with_typed(|s| {
            let now = s.check(early).ok().unwrap();
            match s {
                ConvAny::Cooling(s) => s.due(now),
                other => (other, Then::Nothing),
            }
        });
        assert!(matches!(then, Then::Wake(w) if w.at() >= at));
        assert_eq!(name(&c), "cooling");
    }

    #[test]
    fn the_floor_counts_every_failure_until_a_success_or_a_quiet_minute() {
        assert_eq!(delay(0), Duration::ZERO);
        assert_eq!(delay(1), Duration::from_millis(500));
        assert_eq!(delay(2), Duration::from_secs(1));
        assert_eq!(delay(3), Duration::from_secs(2));
        assert_eq!(delay(4), Duration::from_secs(4));
        assert_eq!(delay(200), Duration::from_secs(4));
        let mut floor = Floor::default();
        assert!(floor.rested().is_ok());
        assert!(matches!(floor.unanswered(), Retry::After(d) if d == Duration::from_millis(500)));
        assert!(floor.rested().is_err());
        assert!(matches!(floor.unanswered(), Retry::After(d) if d == Duration::from_secs(1)));
        // A written answer restarts the count toward stopping, not the delay.
        floor.answered();
        assert!(floor.rested().is_err());
        floor.failed();
        assert!(matches!(floor.unanswered(), Retry::After(d) if d == Duration::from_secs(4)));
        assert!(matches!(floor.unanswered(), Retry::After(_)));
        assert!(matches!(floor.unanswered(), Retry::Stop));
        floor.succeeded();
        assert!(floor.rested().is_ok());
        // A quiet minute since the last failure starts afresh.
        floor.failed();
        floor.age(QUIET);
        assert!(floor.rested().is_ok());
        assert!(matches!(floor.unanswered(), Retry::After(d) if d == Duration::from_millis(500)));
    }

    #[test]
    fn an_answer_for_another_requests_prompt_is_not_written() {
        let mut c = conversation();
        let Then::Dial(dial) = c.with_typed(|s| match s {
            ConvAny::Idle(s) => s.start("me".into()),
            other => (other, Then::Nothing),
        }) else {
            panic!()
        };
        let ticket = dial.ticket();
        let (dialed, mut helper) = up(ticket);
        let _converse = c.with_typed(|s| {
            let now = s.check(dialed).ok().unwrap();
            match s {
                ConvAny::Dialing(s) => s.dialed(now),
                other => (other, Then::Nothing),
            }
        });
        let asked = Lines { ticket }.line("PAM_PROMPT_ECHO_OFF Password: ");
        let _ = c.with_typed(|s| {
            let now = s.check(asked).ok().unwrap();
            match s {
                ConvAny::Starting(s) => s.heard(now),
                other => (other, Then::Nothing),
            }
        });
        assert_eq!(name(&c), "prompted");
        // Another request's first prompt has the same ticket and number.
        let other = Reply { ticket, request: RequestId::next(), seq: 1, text: b"secret".to_vec() };
        let then = c.with_typed(|s| {
            let now = s.check(other).ok().expect("the same ticket checks current");
            match s {
                ConvAny::Prompted(s) => match s.answer(now) {
                    Ok((checking, answering)) => (checking.into(), Then::Answer(answering)),
                    Err(prompted) => (prompted.into(), Then::Nothing),
                },
                other => (other, Then::Nothing),
            }
        });
        assert!(matches!(then, Then::Nothing), "nothing to write");
        assert_eq!(name(&c), "prompted");
        helper.set_nonblocking(true).unwrap();
        let mut got = [0u8; 64];
        assert!(std::io::Read::read(&mut helper, &mut got).is_err(), "the helper got nothing");
    }
}
