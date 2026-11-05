//! One login, from choosing who logs in to the session greetd starts: the
//! screen's stage (SVC-05) and the conversation with greetd under it
//! (SVC-12). Pure: it turns what greetd says and what the person does into
//! what to ask greetd next and what the screen shows, so it's tested
//! without either.
//!
//! The stages are types, and so is who's logging in:
//!
//! - **Choosing:** [`People`] (the row, and who has the selection) or
//!   [`Name`] (the user name being typed).
//! - **Logging in:** a step of greetd's conversation ([`Waiting`],
//!   [`Asking`], [`Starting`], [`Failed`], [`Done`]) for someone:
//!   [`Chosen`] in the row, the [`Only`] person listed, or a [`Typed`] name.
//!   The login carries that person, so beginning again ([`Failed::retry`],
//!   or after a wrong password) is always for them: nothing outside picks
//!   whom.
//!
//! Each login has a way back to where its person was chosen: `back()`
//! returns the [`People`] row for someone chosen in it, and the [`Name`],
//! with what was typed, for a typed name. The only person listed has none:
//! there's nobody else to choose. Once greetd has the session to start
//! ([`Starting`]), there's no way back either: cancelling would drop a
//! session greetd has already scheduled.
//!
//! Return and Escape reach the states through [`Login::on_return`] and
//! [`Login::on_escape`], whose routes must hand back a [`Returned`] or a
//! [`Went`]. Only the transitions those keys may take make them, so a route
//! can't leave a failed login as it was, or a typed name with no way back.
//! greetd's answers reach their transitions bound to the proof that they
//! answer this login ([`Fresh`]): an answer meant for someone no longer
//! being logged in is let go. The screen reads the stage as a [`Screen`],
//! which is all the view has: it can't pick a stage of its own.
// Every stage and answer is matched by name: a new one forces a decision.
#![deny(clippy::wildcard_enum_match_arm)]

use std::collections::VecDeque;

use aizome_typestate::{Answer, Fresh, Generation, Ticket};

use crate::greetd::{AuthMessageType, ErrorType, Request, Response};
use crate::users::User;

use parts::{Ctx, Seal, StepState, Talk};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
}

/// The session to start once authenticated.
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub cmd: Vec<String>,
    pub env: Vec<String>,
}

/// PAM's prompt as a field's placeholder: `Password: ` is `Password`.
pub fn placeholder(prompt: &str) -> String {
    let p = prompt.trim().trim_end_matches(':').trim();
    if p.is_empty() { "Password".to_owned() } else { p.to_owned() }
}

/// Names the logins: each begins a conversation, whose requests carry its
/// ticket, and greetd's answers for an earlier one are let go.
pub enum Conversation {}

/// What a request still waiting for its answer was: one of a login's, or a
/// cancel (whose answer changes nothing: cancelling a session greetd
/// already ended is fine).
enum Pending {
    Cancel,
    Login(Ticket<Conversation>),
}

/// What a login is waiting for greetd to answer: the session's creation
/// (whose failure stops there), or an answer to one of PAM's questions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Create,
    Answer,
}

/// One of PAM's questions: hidden as it's typed (`secret`), or not.
struct Prompt {
    secret: bool,
    text: String,
}

/// greetd's answer to a login's request. Only [`Login::heard`] makes one,
/// and its transitions take it proven to answer this login.
pub struct Said {
    ticket: Ticket<Conversation>,
    response: Response,
}

impl Answer<Conversation> for Said {
    fn ticket(&self) -> Ticket<Conversation> {
        self.ticket
    }
}

/// Who's logging in: someone the screen lists or a typed name. Sealed: the
/// three below are all there are.
pub trait Who: Seal {}

/// Someone chosen in the row (or remembered from last time). Escape goes
/// back to the row, with them selected.
pub struct Chosen {
    index: usize,
}

/// The only person listed. There's nobody else to choose, so no way back.
pub struct Only(());

/// A user name typed (no list, or nobody on it). Escape goes back to the
/// name, as typed, to be corrected. It never goes to the people, who
/// aren't listed, as the `Msg::People` that the typed name's password
/// once had (SVC-05):
///
/// ```compile_fail,E0308
/// use aizome_greeter::login::{Asking, People, Typed};
/// fn escape<'s>(asking: Asking<'s, Typed>) -> People<'s> {
///     asking.back()
/// }
/// ```
///
/// ```
/// use aizome_greeter::login::{Asking, Name, Typed};
/// fn escape<'s>(asking: Asking<'s, Typed>) -> Name<'s> {
///     asking.back()
/// }
/// ```
///
/// and it has no way to stay, as it had no Escape (SVC-05):
///
/// ```compile_fail,E0599
/// use aizome_greeter::login::{Asking, Typed, Went};
/// fn escape<'s>(asking: Asking<'s, Typed>) -> Went<'s> {
///     asking.stay()
/// }
/// ```
///
/// ```
/// use aizome_greeter::login::{Asking, Typed, Went};
/// fn escape<'s>(asking: Asking<'s, Typed>) -> Went<'s> {
///     asking.back().into()
/// }
/// ```
pub struct Typed {
    name: String,
}

impl Who for Chosen {}
impl Who for Only {}
impl Who for Typed {}

/// The pieces the steps share, nominally public so the steps' methods can
/// be generic over [`Who`] (a private trait there trips `private_bounds`).
/// The module is private: nothing outside this one reaches them.
mod parts {
    use std::collections::VecDeque;

    use aizome_typestate::{Generation, Ticket};

    use super::{Asked, Conversation, LoginAny, Pending, Prompt, Tone};
    use crate::greetd::Request;
    use crate::users::User;

    /// What every stage shares: who can be picked, the requests greetd has
    /// yet to answer, what to say under the field, and the requests to send.
    pub struct Ctx {
        pub(super) users: Vec<User>,
        pub(super) logins: Generation<Conversation>,
        /// The requests waiting for their answers, oldest first.
        pub(super) asked: VecDeque<Pending>,
        /// greetd holds a session (created and not finished or cancelled).
        pub(super) open: bool,
        /// What PAM or greetd said, or why the last try failed.
        pub(super) message: Option<(String, Tone)>,
        /// Wrong answers so far: the field shakes, and its rail is red.
        pub(super) failed: u32,
        /// What to send greetd, in order.
        pub(super) out: Vec<Request>,
    }

    impl Ctx {
        pub(super) fn ask(&mut self, pending: Pending, request: Request) {
            self.asked.push_back(pending);
            self.out.push(request);
        }
    }

    /// A login, stored: who, and how far greetd has got.
    pub struct Talk<W> {
        pub(super) who: W,
        pub(super) step: StepState,
    }

    pub enum StepState {
        Waiting(Ticket<Conversation>, Asked),
        Asking(Ticket<Conversation>, Prompt),
        Starting(Ticket<Conversation>),
        Failed,
        Done,
    }

    pub trait Seal: Sized {
        /// The user name greetd logs in.
        fn user<'c>(&'c self, ctx: &'c Ctx) -> &'c str;
        /// The stage for this kind of person's login.
        fn login<'s>(ctx: &'s mut Ctx, talk: Talk<Self>) -> LoginAny<'s>;
    }
}

aizome_typestate::machine! {
    /// The login screen's stage: choosing who logs in, then their login.
    /// Its state is private, and so is the conversation's. Code outside
    /// reads it as a [`Screen`], and moves it with Return, Escape and the
    /// events the methods below name. The view once picked between the
    /// typed name and its password from the conversation's fields; it
    /// can't read them now (SVC-05):
    ///
    /// ```compile_fail,E0624
    /// use aizome_greeter::login::Login;
    /// fn typing_a_name(login: &Login) -> bool {
    ///     let _ = login.stored();
    ///     true
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_greeter::login::{Login, Screen};
    /// fn typing_a_name(login: &Login) -> bool {
    ///     match login.screen() {
    ///         Screen::Name { .. } => true,
    ///         Screen::Unstarted | Screen::People { .. } | Screen::Login { .. } => false,
    ///     }
    /// }
    /// ```
    pub struct Login over Ctx {
        stored LoginState;
        /// The typed stage, for [`Login::on_return`] and [`Login::on_escape`].
        typed LoginAny;
        states {
            /// Before the greeter starts.
            Unstarted,
            /// The row of people, and who has the selection.
            People(usize),
            /// The user name, as typed so far.
            Name(String),
            /// The login of someone chosen in the row.
            ChosenLogin(Talk<Chosen>),
            /// The only person's login.
            OnlyLogin(Talk<Only>),
            /// A typed name's login.
            TypedLogin(Talk<Typed>),
        }
        access(private);
        new(private);
    }
}

impl Seal for Chosen {
    fn user<'c>(&'c self, ctx: &'c Ctx) -> &'c str {
        &ctx.users[self.index].name
    }
    fn login<'s>(ctx: &'s mut Ctx, talk: Talk<Self>) -> LoginAny<'s> {
        ChosenLogin::enter(ctx, talk).into()
    }
}

impl Seal for Only {
    fn user<'c>(&'c self, ctx: &'c Ctx) -> &'c str {
        &ctx.users[0].name
    }
    fn login<'s>(ctx: &'s mut Ctx, talk: Talk<Self>) -> LoginAny<'s> {
        OnlyLogin::enter(ctx, talk).into()
    }
}

impl Seal for Typed {
    fn user<'c>(&'c self, _ctx: &'c Ctx) -> &'c str {
        &self.name
    }
    fn login<'s>(ctx: &'s mut Ctx, talk: Talk<Self>) -> LoginAny<'s> {
        TypedLogin::enter(ctx, talk).into()
    }
}

/// What Return did. Only the transitions Return may take make one: in a
/// failed login, that's [`Failed::retry`], so a route can't leave it as it
/// was:
///
/// ```compile_fail,E0451
/// use aizome_greeter::login::{Failed, Returned, Typed};
/// fn on_return<'s>(failed: Failed<'s, Typed>) -> Returned<'s> {
///     Returned { next: failed.into() }
/// }
/// ```
///
/// ```
/// use aizome_greeter::login::{Failed, Returned, Typed};
/// fn on_return<'s>(failed: Failed<'s, Typed>) -> Returned<'s> {
///     failed.retry()
/// }
/// ```
#[must_use = "Login::on_return applies it"]
pub struct Returned<'s> {
    next: LoginAny<'s>,
}

/// Where Escape went: back where the person was chosen, or nowhere, from a
/// stage with no way back. Only those transitions make one.
#[must_use = "Login::on_escape applies it"]
pub struct Went<'s> {
    next: LoginAny<'s>,
}

impl<'s> From<People<'s>> for Went<'s> {
    fn from(people: People<'s>) -> Self {
        Went { next: people.into() }
    }
}

impl<'s> From<Name<'s>> for Went<'s> {
    fn from(name: Name<'s>) -> Self {
        Went { next: name.into() }
    }
}

/// Begin logging `who` in, giving up on whoever greetd held before.
fn begin<W: Who>(ctx: &mut Ctx, who: W) -> Waiting<'_, W> {
    if ctx.open {
        ctx.ask(Pending::Cancel, Request::CancelSession);
    }
    let ticket = ctx.logins.issue();
    let username = who.user(ctx).to_owned();
    ctx.ask(Pending::Login(ticket), Request::CreateSession { username });
    ctx.open = true;
    Waiting { ctx, who, ticket, asked: Asked::Create }
}

/// Give up on this login: greetd lets the session go, and its answers
/// still to come are let go.
fn cancel(ctx: &mut Ctx) {
    if ctx.open {
        ctx.ask(Pending::Cancel, Request::CancelSession);
    }
    ctx.logins.retire();
    ctx.open = false;
    ctx.message = None;
}

impl<'s> Unstarted<'s> {
    /// The first stage: the name to type (no list, or nobody on it), the
    /// only person's login, the login of whoever logged in last, or the row.
    fn start(self, user_list: bool, last: Option<&str>) -> LoginAny<'s> {
        let ctx = self.ctx;
        let n = ctx.users.len();
        if !user_list || n == 0 {
            return Name::enter(ctx, String::new()).into();
        }
        if n == 1 {
            return begin(ctx, Only(())).into();
        }
        match last.and_then(|u| ctx.users.iter().position(|x| x.name == u)) {
            Some(index) => begin(ctx, Chosen { index }).into(),
            None => People::enter(ctx, 0).into(),
        }
    }

    /// Return before the greeter starts: nothing.
    pub fn busy(self) -> Returned<'s> {
        Returned { next: self.into() }
    }

    /// Escape before the greeter starts: nothing.
    pub fn stay(self) -> Went<'s> {
        Went { next: self.into() }
    }
}

impl<'s> People<'s> {
    fn select(&mut self, i: usize) {
        self.data = i.min(self.ctx.users.len().saturating_sub(1));
    }

    fn next(&mut self) {
        if !self.ctx.users.is_empty() {
            self.data = (self.data + 1) % self.ctx.users.len();
        }
    }

    fn previous(&mut self) {
        let n = self.ctx.users.len();
        if n > 0 {
            self.data = (self.data + n - 1) % n;
        }
    }

    /// Return: log in whoever has the selection.
    pub fn pick(self) -> Returned<'s> {
        let index = self.data;
        self.ctx.message = None;
        Returned { next: begin(self.ctx, Chosen { index }).into() }
    }

    /// Escape in the row: it's where choosing starts.
    pub fn stay(self) -> Went<'s> {
        self.into()
    }
}

impl<'s> Name<'s> {
    /// Return: log in the name typed, if there's one.
    pub fn submit(self) -> Returned<'s> {
        let name = self.data.trim();
        if name.is_empty() {
            return Returned { next: self.into() };
        }
        let name = name.to_owned();
        self.ctx.message = None;
        Returned { next: begin(self.ctx, Typed { name }).into() }
    }

    /// Escape in the name: it's where choosing starts.
    pub fn stay(self) -> Went<'s> {
        self.into()
    }
}

impl<'s> ChosenLogin<'s> {
    /// The login's step.
    pub fn step(self) -> Step<'s, Chosen> {
        Step::open(self.ctx, self.data)
    }
}

impl<'s> OnlyLogin<'s> {
    /// The login's step.
    pub fn step(self) -> Step<'s, Only> {
        Step::open(self.ctx, self.data)
    }
}

impl<'s> TypedLogin<'s> {
    /// The login's step.
    pub fn step(self) -> Step<'s, Typed> {
        Step::open(self.ctx, self.data)
    }
}

/// How far greetd has got with `W`'s login.
pub enum Step<'s, W> {
    Waiting(Waiting<'s, W>),
    Asking(Asking<'s, W>),
    Starting(Starting<'s, W>),
    Failed(Failed<'s, W>),
    Done(Done<'s, W>),
}

impl<'s, W: Who> Step<'s, W> {
    fn open(ctx: &'s mut Ctx, talk: Talk<W>) -> Self {
        let who = talk.who;
        match talk.step {
            StepState::Waiting(ticket, asked) => Step::Waiting(Waiting { ctx, who, ticket, asked }),
            StepState::Asking(ticket, prompt) => Step::Asking(Asking { ctx, who, ticket, prompt }),
            StepState::Starting(ticket) => Step::Starting(Starting { ctx, who, ticket }),
            StepState::Failed => Step::Failed(Failed { ctx, who }),
            StepState::Done => Step::Done(Done { ctx, who }),
        }
    }

    /// greetd went away: nothing it was asked will be answered.
    fn lost(self) -> LoginAny<'s> {
        match self {
            Step::Waiting(Waiting { ctx, who, .. })
            | Step::Asking(Asking { ctx, who, .. })
            | Step::Starting(Starting { ctx, who, .. })
            | Step::Failed(Failed { ctx, who }) => Failed { ctx, who }.into(),
            Step::Done(done) => done.into(),
        }
    }
}

/// Waiting for greetd to answer. Its answer reaches [`Waiting::heard`]
/// only with the proof that it answers this login, bound to it: an answer
/// that wasn't checked doesn't do:
///
/// ```compile_fail,E0308
/// use aizome_greeter::login::{Chosen, Launch, Said, Step, Waiting};
/// fn heard<'s>(waiting: Waiting<'s, Chosen>, said: Said) -> Step<'s, Chosen> {
///     waiting.heard(said, |_| Launch { cmd: Vec::new(), env: Vec::new() })
/// }
/// ```
///
/// ```
/// use aizome_greeter::login::{Chosen, Conversation, Launch, Said, Step, Waiting};
/// use aizome_typestate::Fresh;
/// fn heard<'s>(waiting: Waiting<'s, Chosen>, said: Fresh<Conversation, Said>) -> Step<'s, Chosen> {
///     waiting.heard(said, |_| Launch { cmd: Vec::new(), env: Vec::new() })
/// }
/// ```
pub struct Waiting<'s, W> {
    ctx: &'s mut Ctx,
    who: W,
    ticket: Ticket<Conversation>,
    asked: Asked,
}

/// One of PAM's questions, to answer in the field.
pub struct Asking<'s, W> {
    ctx: &'s mut Ctx,
    who: W,
    ticket: Ticket<Conversation>,
    prompt: Prompt,
}

/// Authenticated: greetd has the session to start. There's no way back
/// now, as "Log in as someone else" once cancelled a session greetd had
/// scheduled (SVC-12):
///
/// ```compile_fail,E0599
/// use aizome_greeter::login::{Chosen, People, Starting};
/// fn escape<'s>(starting: Starting<'s, Chosen>) -> People<'s> {
///     starting.back()
/// }
/// ```
///
/// ```
/// use aizome_greeter::login::{Chosen, Starting, Went};
/// fn escape<'s>(starting: Starting<'s, Chosen>) -> Went<'s> {
///     starting.stay()
/// }
/// ```
pub struct Starting<'s, W> {
    ctx: &'s mut Ctx,
    who: W,
    ticket: Ticket<Conversation>,
}

/// greetd couldn't begin the login (an unknown user, PAM refusing it).
/// Return begins it again, for the same person: a route can't let Return
/// do nothing, as it once did for a typed name (SVC-05):
///
/// ```compile_fail,E0599
/// use aizome_greeter::login::{Failed, Returned, Typed};
/// fn on_return<'s>(failed: Failed<'s, Typed>) -> Returned<'s> {
///     failed.busy()
/// }
/// ```
///
/// ```
/// use aizome_greeter::login::{Failed, Returned, Typed};
/// fn on_return<'s>(failed: Failed<'s, Typed>) -> Returned<'s> {
///     failed.retry()
/// }
/// ```
///
/// and it begins again for the login's own person, not one named by the
/// stage, as `begin` once took from it (none, for a typed name):
///
/// ```compile_fail,E0061
/// use aizome_greeter::login::{Failed, Returned, Typed};
/// fn on_return<'s>(failed: Failed<'s, Typed>, user: &str) -> Returned<'s> {
///     failed.retry(user)
/// }
/// ```
pub struct Failed<'s, W> {
    ctx: &'s mut Ctx,
    who: W,
}

/// The session will start once the greeter has gone.
pub struct Done<'s, W> {
    ctx: &'s mut Ctx,
    who: W,
}

impl<'s, W: Who> From<Waiting<'s, W>> for LoginAny<'s> {
    fn from(s: Waiting<'s, W>) -> Self {
        W::login(s.ctx, Talk { who: s.who, step: StepState::Waiting(s.ticket, s.asked) })
    }
}

impl<'s, W: Who> From<Asking<'s, W>> for LoginAny<'s> {
    fn from(s: Asking<'s, W>) -> Self {
        W::login(s.ctx, Talk { who: s.who, step: StepState::Asking(s.ticket, s.prompt) })
    }
}

impl<'s, W: Who> From<Starting<'s, W>> for LoginAny<'s> {
    fn from(s: Starting<'s, W>) -> Self {
        W::login(s.ctx, Talk { who: s.who, step: StepState::Starting(s.ticket) })
    }
}

impl<'s, W: Who> From<Failed<'s, W>> for LoginAny<'s> {
    fn from(s: Failed<'s, W>) -> Self {
        W::login(s.ctx, Talk { who: s.who, step: StepState::Failed })
    }
}

impl<'s, W: Who> From<Done<'s, W>> for LoginAny<'s> {
    fn from(s: Done<'s, W>) -> Self {
        W::login(s.ctx, Talk { who: s.who, step: StepState::Done })
    }
}

impl<'s, W: Who> From<Step<'s, W>> for LoginAny<'s> {
    fn from(step: Step<'s, W>) -> Self {
        match step {
            Step::Waiting(s) => s.into(),
            Step::Asking(s) => s.into(),
            Step::Starting(s) => s.into(),
            Step::Failed(s) => s.into(),
            Step::Done(s) => s.into(),
        }
    }
}

/// Show what PAM said, and acknowledge it, or ask its question.
fn prompted<'s, W: Who>(
    ctx: &'s mut Ctx,
    who: W,
    ticket: Ticket<Conversation>,
    kind: AuthMessageType,
    text: String,
) -> Step<'s, W> {
    match kind {
        AuthMessageType::Secret | AuthMessageType::Visible => {
            let prompt = Prompt { secret: kind == AuthMessageType::Secret, text: placeholder(&text) };
            Step::Asking(Asking { ctx, who, ticket, prompt })
        }
        AuthMessageType::Info | AuthMessageType::Error => {
            let tone = if kind == AuthMessageType::Error { Tone::Error } else { Tone::Info };
            let text = text.trim().to_owned();
            if !text.is_empty() {
                ctx.message = Some((text, tone));
            }
            ctx.ask(Pending::Login(ticket), Request::PostAuthMessageResponse { response: None });
            Step::Waiting(Waiting { ctx, who, ticket, asked: Asked::Answer })
        }
    }
}

/// greetd refused: say why (a wrong answer counts, and the field shakes).
/// greetd may still hold the session (it does after a wrong password, and
/// refuses a new one: "a session is already being configured"), so
/// beginning again cancels it first; cancelling one it already dropped is
/// harmless.
fn refused(ctx: &mut Ctx, kind: ErrorType, why: String) {
    let text = match kind {
        ErrorType::AuthError => {
            ctx.failed += 1;
            "That password didn't work. Try again.".to_owned()
        }
        ErrorType::Error => why,
    };
    ctx.message = Some((text, Tone::Error));
    ctx.open = true;
}

impl<'s, W: Who> Waiting<'s, W> {
    /// Return while greetd is asked: nothing until it answers.
    pub fn busy(self) -> Returned<'s> {
        Returned { next: self.into() }
    }

    /// greetd answered, with the proof that it's this login's answer.
    /// `launch` gives the session to start for the login's user, once
    /// authenticated.
    pub fn heard(self, said: Fresh<Conversation, Said>, launch: impl FnOnce(&str) -> Launch) -> Step<'s, W> {
        let Waiting { ctx, who, ticket, asked } = self;
        match said.into_answer().response {
            Response::AuthMessage { auth_message_type, auth_message } => {
                prompted(ctx, who, ticket, auth_message_type, auth_message)
            }
            // Authenticated (also with no questions, an account with no
            // password): start the session.
            Response::Success => {
                let Launch { cmd, env } = launch(who.user(ctx));
                ctx.ask(Pending::Login(ticket), Request::StartSession { cmd, env });
                Step::Starting(Starting { ctx, who, ticket })
            }
            Response::Error { error_type, description } => {
                refused(ctx, error_type, tidy(&description));
                match asked {
                    // Beginning is what failed: wait for Return, rather
                    // than loop.
                    Asked::Create => Step::Failed(Failed { ctx, who }),
                    // Begin again, for the same person.
                    Asked::Answer => Step::Waiting(begin(ctx, who)),
                }
            }
        }
    }
}

impl<'s, W: Who> Asking<'s, W> {
    /// Return: the field's text answers the question, and leaves the field.
    pub fn answer(self, field: &mut String) -> Returned<'s> {
        let Asking { ctx, who, ticket, .. } = self;
        let response = Some(std::mem::take(field));
        ctx.message = None;
        ctx.ask(Pending::Login(ticket), Request::PostAuthMessageResponse { response });
        Returned { next: Waiting { ctx, who, ticket, asked: Asked::Answer }.into() }
    }
}

impl<'s, W: Who> Starting<'s, W> {
    /// Return while the session starts: nothing.
    pub fn busy(self) -> Returned<'s> {
        Returned { next: self.into() }
    }

    /// Escape while the session starts: there's no way back.
    pub fn stay(self) -> Went<'s> {
        Went { next: self.into() }
    }

    /// greetd answered the request to start the session, with the proof
    /// that it's this login's answer.
    pub fn heard(self, said: Fresh<Conversation, Said>) -> Step<'s, W> {
        let Starting { ctx, who, ticket } = self;
        match said.into_answer().response {
            Response::Success => {
                ctx.open = false;
                Step::Done(Done { ctx, who })
            }
            Response::AuthMessage { auth_message_type, auth_message } => {
                prompted(ctx, who, ticket, auth_message_type, auth_message)
            }
            // It didn't start: say why, and begin again.
            Response::Error { error_type, description } => {
                refused(ctx, error_type, format!("The session didn't start: {}", tidy(&description)));
                Step::Waiting(begin(ctx, who))
            }
        }
    }
}

impl<'s, W: Who> Failed<'s, W> {
    /// Return: begin again, for the same person (cancelling whatever greetd
    /// kept first).
    pub fn retry(self) -> Returned<'s> {
        Returned { next: begin(self.ctx, self.who).into() }
    }
}

impl<'s, W: Who> Done<'s, W> {
    /// Return once logged in: nothing.
    pub fn busy(self) -> Returned<'s> {
        Returned { next: self.into() }
    }

    /// Escape once logged in: nothing.
    pub fn stay(self) -> Went<'s> {
        Went { next: self.into() }
    }

    /// Who logged in.
    pub fn user(&self) -> &str {
        self.who.user(self.ctx)
    }
}

/// The way back from a login, cancelling it: `$to` for `$who`.
macro_rules! way_back {
    ($who:ident => $to:ident, $doc:literal) => {
        impl<'s> Waiting<'s, $who> {
            #[doc = $doc]
            pub fn back(self) -> $to<'s> {
                cancel(self.ctx);
                self.who.back(self.ctx)
            }
        }
        impl<'s> Asking<'s, $who> {
            #[doc = $doc]
            pub fn back(self) -> $to<'s> {
                cancel(self.ctx);
                self.who.back(self.ctx)
            }
        }
        impl<'s> Failed<'s, $who> {
            #[doc = $doc]
            pub fn back(self) -> $to<'s> {
                cancel(self.ctx);
                self.who.back(self.ctx)
            }
        }
    };
}

way_back!(Chosen => People, "Escape: back to the row, with them selected.");
way_back!(Typed => Name, "Escape: back to the name, as typed, to correct it.");

impl Chosen {
    fn back(self, ctx: &mut Ctx) -> People<'_> {
        People::enter(ctx, self.index)
    }
}

impl Typed {
    fn back(self, ctx: &mut Ctx) -> Name<'_> {
        Name::enter(ctx, self.name)
    }
}

impl<'s> Waiting<'s, Only> {
    /// Escape: there's nobody else to choose.
    pub fn stay(self) -> Went<'s> {
        Went { next: self.into() }
    }
}

impl<'s> Asking<'s, Only> {
    /// Escape: there's nobody else to choose.
    pub fn stay(self) -> Went<'s> {
        Went { next: self.into() }
    }
}

impl<'s> Failed<'s, Only> {
    /// Escape: there's nobody else to choose.
    pub fn stay(self) -> Went<'s> {
        Went { next: self.into() }
    }
}

/// What greetd's answer came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to tell.
    Quiet,
    /// A wrong answer: the field shakes.
    Refused,
    /// `user` logged in: greetd starts their session once the greeter has
    /// gone.
    LoggedIn(String),
}

/// greetd's answer, applied: what to ask it next, and what it came to.
#[derive(Debug)]
pub struct Heard {
    pub requests: Vec<Request>,
    pub outcome: Outcome,
}

/// What the screen shows: the stage, as the view needs it, and nothing
/// the view could pick a stage from.
#[derive(Debug)]
pub enum Screen<'a> {
    /// Before the greeter starts.
    Unstarted,
    /// The row of people, and who has the selection.
    People { people: &'a [User], selected: usize },
    /// The user name, as typed so far.
    Name { typed: &'a str },
    /// Someone's login: who, how far, and where Escape goes.
    Login { person: Person<'a>, progress: Progress<'a>, escape: EscapeTo },
}

/// Who's logging in, as the screen shows them.
#[derive(Clone, Copy, Debug)]
pub enum Person<'a> {
    /// Someone listed: their picture and name.
    Listed(&'a User),
    /// A typed user name.
    Typed(&'a str),
}

/// How far a login has got, as the field shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress<'a> {
    /// greetd is asked.
    Checking,
    /// A question: hidden as it's typed (`secret`), or not.
    Asking { secret: bool, prompt: &'a str },
    /// The session is starting.
    Starting,
    /// greetd couldn't begin: Return tries again.
    Failed,
    /// Logged in.
    Done,
}

/// Where Escape (and "Log in as someone else") goes from a login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeTo {
    People,
    Name,
    /// The only person listed, or a session starting.
    Nowhere,
}

impl Login {
    /// The login screen for `users`, not yet started.
    pub fn with_users(users: Vec<User>) -> Login {
        Login::new(Ctx {
            users,
            logins: Generation::new(),
            asked: VecDeque::new(),
            open: false,
            message: None,
            failed: 0,
            out: Vec::new(),
        })
    }

    fn sent(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.ctx.out)
    }

    /// Start: the name to type (`user_list` off, or nobody listed), the
    /// only person's login, `last`'s login, or the row. What to ask
    /// greetd.
    pub fn start(&mut self, user_list: bool, last: Option<&str>) -> Vec<Request> {
        self.with_typed(|state| match state {
            LoginAny::Unstarted(unstarted) => (unstarted.start(user_list, last), ()),
            other @ (LoginAny::People(_)
            | LoginAny::Name(_)
            | LoginAny::ChosenLogin(_)
            | LoginAny::OnlyLogin(_)
            | LoginAny::TypedLogin(_)) => (other, ()),
        });
        self.sent()
    }

    fn in_row(&mut self, f: impl FnOnce(&mut People<'_>)) {
        self.with_typed(|state| match state {
            LoginAny::People(mut people) => {
                f(&mut people);
                (people.into(), ())
            }
            other @ (LoginAny::Unstarted(_)
            | LoginAny::Name(_)
            | LoginAny::ChosenLogin(_)
            | LoginAny::OnlyLogin(_)
            | LoginAny::TypedLogin(_)) => (other, ()),
        });
    }

    /// The selection moves to person `i` in the row.
    pub fn select(&mut self, i: usize) {
        self.in_row(|people| people.select(i));
    }

    /// → in the row.
    pub fn next(&mut self) {
        self.in_row(|people| people.next());
    }

    /// ← in the row.
    pub fn previous(&mut self) {
        self.in_row(|people| people.previous());
    }

    /// Person `i` clicked in the row: log them in. What to ask greetd.
    pub fn pick(&mut self, i: usize) -> Vec<Request> {
        if i < self.ctx.users.len() {
            self.with_typed(|state| match state {
                LoginAny::People(mut people) => {
                    people.select(i);
                    (people.pick().next, ())
                }
                other @ (LoginAny::Unstarted(_)
                | LoginAny::Name(_)
                | LoginAny::ChosenLogin(_)
                | LoginAny::OnlyLogin(_)
                | LoginAny::TypedLogin(_)) => (other, ()),
            });
        }
        self.sent()
    }

    /// The user name, as typed so far.
    pub fn type_name(&mut self, text: String) {
        self.with_typed(|state| match state {
            LoginAny::Name(mut name) => {
                name.data = text;
                (name.into(), ())
            }
            other @ (LoginAny::Unstarted(_)
            | LoginAny::People(_)
            | LoginAny::ChosenLogin(_)
            | LoginAny::OnlyLogin(_)
            | LoginAny::TypedLogin(_)) => (other, ()),
        });
    }

    /// Return, routed by `route`: it hands back what one of Return's
    /// transitions made. What to ask greetd.
    pub fn on_return(&mut self, route: impl for<'s> FnOnce(LoginAny<'s>) -> Returned<'s>) -> Vec<Request> {
        self.with_typed(|state| (route(state).next, ()));
        self.sent()
    }

    /// Escape, routed by `route`: it hands back where one of Escape's
    /// transitions went. What to ask greetd.
    pub fn on_escape(&mut self, route: impl for<'s> FnOnce(LoginAny<'s>) -> Went<'s>) -> Vec<Request> {
        self.with_typed(|state| (route(state).next, ()));
        self.sent()
    }

    /// greetd answered the oldest request waiting. `launch` gives the
    /// session to start for a user once authenticated.
    pub fn heard(&mut self, response: Response, launch: impl FnOnce(&str) -> Launch) -> Heard {
        let quiet = Heard { requests: Vec::new(), outcome: Outcome::Quiet };
        let ticket = match self.ctx.asked.pop_front() {
            Some(Pending::Login(ticket)) => ticket,
            // A cancel's answer, or one nothing asked for: nothing to do.
            Some(Pending::Cancel) | None => return quiet,
        };
        // An answer for an earlier login belongs to nobody now.
        let Ok(said) = self.ctx.logins.check_answer(Said { ticket, response }) else {
            return quiet;
        };
        let failed = self.ctx.failed;
        let logged_in = self.with_typed(|state| match state {
            LoginAny::ChosenLogin(login) => answered(login.step(), said, launch),
            LoginAny::OnlyLogin(login) => answered(login.step(), said, launch),
            LoginAny::TypedLogin(login) => answered(login.step(), said, launch),
            // Choosing retired the logins before: no answer is theirs.
            other @ (LoginAny::Unstarted(_) | LoginAny::People(_) | LoginAny::Name(_)) => (other, None),
        });
        let outcome = match logged_in {
            Some(user) => Outcome::LoggedIn(user),
            None if self.ctx.failed != failed => Outcome::Refused,
            None => Outcome::Quiet,
        };
        Heard { requests: self.sent(), outcome }
    }

    /// greetd went away (it restarts the greeter itself): nothing asked of
    /// it will be answered, and a login under way has failed.
    pub fn lost(&mut self, why: &str) {
        self.ctx.message = Some((format!("Lost the login service: {why}"), Tone::Error));
        self.ctx.asked.clear();
        self.ctx.logins.retire();
        self.ctx.open = false;
        self.with_typed(|state| match state {
            LoginAny::ChosenLogin(login) => (login.step().lost(), ()),
            LoginAny::OnlyLogin(login) => (login.step().lost(), ()),
            LoginAny::TypedLogin(login) => (login.step().lost(), ()),
            other @ (LoginAny::Unstarted(_) | LoginAny::People(_) | LoginAny::Name(_)) => (other, ()),
        });
    }

    /// What the screen shows.
    pub fn screen(&self) -> Screen<'_> {
        let users = &self.ctx.users;
        match self.stored() {
            LoginState::Unstarted(()) => Screen::Unstarted,
            LoginState::People(selected) => Screen::People { people: users, selected: *selected },
            LoginState::Name(typed) => Screen::Name { typed },
            LoginState::ChosenLogin(t) => login_screen(Person::Listed(&users[t.who.index]), &t.step, EscapeTo::People),
            LoginState::OnlyLogin(t) => login_screen(Person::Listed(&users[0]), &t.step, EscapeTo::Nowhere),
            LoginState::TypedLogin(t) => login_screen(Person::Typed(&t.who.name), &t.step, EscapeTo::Name),
        }
    }

    /// Who can be picked.
    pub fn users(&self) -> &[User] {
        &self.ctx.users
    }

    /// The user name being logged in, if a login is under way.
    pub fn user(&self) -> Option<&str> {
        match self.stored() {
            LoginState::ChosenLogin(t) => Some(t.who.user(&self.ctx)),
            LoginState::OnlyLogin(t) => Some(t.who.user(&self.ctx)),
            LoginState::TypedLogin(t) => Some(t.who.user(&self.ctx)),
            LoginState::Unstarted(()) | LoginState::People(_) | LoginState::Name(_) => None,
        }
    }

    /// What PAM or greetd said, or why the last try failed.
    pub fn message(&self) -> Option<(&str, Tone)> {
        self.ctx.message.as_ref().map(|(m, t)| (m.as_str(), *t))
    }

    /// Wrong answers so far.
    pub fn failed(&self) -> u32 {
        self.ctx.failed
    }
}

/// greetd's answer, proven this login's, applied to its step: and who
/// logged in, if that's what it came to.
fn answered<'s, W: Who>(
    step: Step<'s, W>,
    said: Fresh<Conversation, Said>,
    launch: impl FnOnce(&str) -> Launch,
) -> (LoginAny<'s>, Option<String>) {
    match step {
        Step::Waiting(waiting) => (waiting.heard(said, launch).into(), None),
        Step::Starting(starting) => match starting.heard(said) {
            Step::Done(done) => {
                let user = done.user().to_owned();
                (done.into(), Some(user))
            }
            other @ (Step::Waiting(_) | Step::Asking(_) | Step::Starting(_) | Step::Failed(_)) => (other.into(), None),
        },
        // Nothing of this login's waits for an answer in these: greetd
        // answers each request once.
        other @ (Step::Asking(_) | Step::Failed(_) | Step::Done(_)) => (other.into(), None),
    }
}

fn login_screen<'a>(person: Person<'a>, step: &'a StepState, back: EscapeTo) -> Screen<'a> {
    let (progress, escape) = match step {
        StepState::Waiting(..) => (Progress::Checking, back),
        StepState::Asking(_, p) => (Progress::Asking { secret: p.secret, prompt: &p.text }, back),
        StepState::Failed => (Progress::Failed, back),
        StepState::Starting(_) => (Progress::Starting, EscapeTo::Nowhere),
        StepState::Done => (Progress::Done, EscapeTo::Nowhere),
    };
    Screen::Login { person, progress, escape }
}

/// greetd's or PAM's words, made a sentence.
fn tidy(s: &str) -> String {
    let s = s.trim();
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Something went wrong.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{on_escape, on_return};

    fn launch() -> Launch {
        Launch { cmd: vec!["aizome-session".into()], env: vec!["XDG_SESSION_TYPE=wayland".into()] }
    }
    fn secret(p: &str) -> Response {
        Response::AuthMessage { auth_message_type: AuthMessageType::Secret, auth_message: p.into() }
    }
    fn error(kind: ErrorType, description: &str) -> Response {
        Response::Error { error_type: kind, description: description.into() }
    }
    fn user(name: &str) -> User {
        User { name: name.into(), real_name: name.into(), uid: 1000, icon: None }
    }
    fn create(name: &str) -> Request {
        Request::CreateSession { username: name.into() }
    }
    fn answer(text: &str) -> Request {
        Request::PostAuthMessageResponse { response: Some(text.into()) }
    }

    /// A login screen listing `people`, started.
    fn listing(people: &[&str], last: Option<&str>) -> (Login, Vec<Request>) {
        let mut l = Login::with_users(people.iter().map(|n| user(n)).collect());
        let sent = l.start(true, last);
        (l, sent)
    }

    fn typing() -> Login {
        let mut l = Login::with_users(Vec::new());
        assert!(l.start(true, None).is_empty());
        l
    }

    fn heard(l: &mut Login, r: Response) -> Heard {
        l.heard(r, |_| launch())
    }

    fn enter(l: &mut Login, field: &str) -> Vec<Request> {
        let mut field = field.to_owned();
        l.on_return(|s| on_return(s, &mut field))
    }

    fn escape(l: &mut Login) -> Vec<Request> {
        l.on_escape(on_escape)
    }

    fn progress(l: &Login) -> Option<Progress<'_>> {
        match l.screen() {
            Screen::Login { progress, .. } => Some(progress),
            Screen::Unstarted | Screen::People { .. } | Screen::Name { .. } => None,
        }
    }

    fn escape_to(l: &Login) -> Option<EscapeTo> {
        match l.screen() {
            Screen::Login { escape, .. } => Some(escape),
            Screen::Unstarted | Screen::People { .. } | Screen::Name { .. } => None,
        }
    }

    fn typed(l: &Login) -> Option<&str> {
        match l.screen() {
            Screen::Name { typed } => Some(typed),
            Screen::Unstarted | Screen::People { .. } | Screen::Login { .. } => None,
        }
    }

    fn selected(l: &Login) -> Option<usize> {
        match l.screen() {
            Screen::People { selected, .. } => Some(selected),
            Screen::Unstarted | Screen::Name { .. } | Screen::Login { .. } => None,
        }
    }

    const ASKED: Option<Progress<'static>> = Some(Progress::Asking { secret: true, prompt: "Password" });

    #[test]
    fn a_password_logs_in_and_starts_the_session() {
        let (mut l, sent) = listing(&["alice"], None);
        assert_eq!(sent, [create("alice")]);
        assert_eq!(heard(&mut l, secret("Password: ")).requests, []);
        assert_eq!(progress(&l), ASKED);
        assert_eq!(enter(&mut l, "hunter2"), [answer("hunter2")]);
        assert_eq!(progress(&l), Some(Progress::Checking));
        let h = heard(&mut l, Response::Success);
        assert_eq!(
            h.requests,
            [Request::StartSession {
                cmd: vec!["aizome-session".into()],
                env: vec!["XDG_SESSION_TYPE=wayland".into()]
            }]
        );
        assert_eq!(progress(&l), Some(Progress::Starting));
        let h = heard(&mut l, Response::Success);
        assert_eq!(h.outcome, Outcome::LoggedIn("alice".into()));
        assert_eq!(progress(&l), Some(Progress::Done));
    }

    #[test]
    fn a_wrong_password_says_so_and_asks_again() {
        let (mut l, _) = listing(&["alice"], None);
        heard(&mut l, secret("Password: "));
        enter(&mut l, "wrong");
        let h = heard(&mut l, error(ErrorType::AuthError, "pam_authenticate: AUTH_ERR"));
        assert_eq!(h.requests, [Request::CancelSession, create("alice")]);
        assert_eq!(h.outcome, Outcome::Refused);
        assert_eq!(l.failed(), 1);
        assert_eq!(l.message(), Some(("That password didn't work. Try again.", Tone::Error)));
        // greetd answers the cancel, then asks again.
        assert_eq!(heard(&mut l, Response::Success).requests, []);
        heard(&mut l, secret("Password: "));
        assert_eq!(progress(&l), ASKED);
        // The message stays until they answer again.
        assert!(l.message().is_some());
        enter(&mut l, "right");
        assert!(l.message().is_none());
    }

    #[test]
    fn info_is_shown_and_acknowledged_and_more_questions_are_asked() {
        let (mut l, _) = listing(&["alice"], None);
        let h = heard(
            &mut l,
            Response::AuthMessage { auth_message_type: AuthMessageType::Info, auth_message: "Touch your key".into() },
        );
        assert_eq!(h.requests, [Request::PostAuthMessageResponse { response: None }]);
        assert_eq!(l.message(), Some(("Touch your key", Tone::Info)));
        heard(
            &mut l,
            Response::AuthMessage { auth_message_type: AuthMessageType::Visible, auth_message: "Code: ".into() },
        );
        assert_eq!(progress(&l), Some(Progress::Asking { secret: false, prompt: "Code" }));
    }

    #[test]
    fn picking_someone_else_cancels_first_and_ignores_the_cancels_answer() {
        let (mut l, sent) = listing(&["alice", "bob"], Some("alice"));
        assert_eq!(sent, [create("alice")]);
        heard(&mut l, secret("Password: "));
        assert_eq!(escape(&mut l), [Request::CancelSession]);
        assert_eq!(selected(&l), Some(0));
        l.next();
        assert_eq!(enter(&mut l, ""), [create("bob")]);
        // The cancel's answer (an error if greetd had no session) changes nothing.
        assert_eq!(heard(&mut l, error(ErrorType::Error, "no session")).requests, []);
        assert!(l.message().is_none());
        heard(&mut l, secret("Password: "));
        assert_eq!(l.user(), Some("bob"));
        assert_eq!(progress(&l), ASKED);
    }

    #[test]
    fn an_account_without_a_password_starts_at_once_and_a_failed_start_says_why() {
        let (mut l, _) = listing(&["kiosk"], None);
        let h = heard(&mut l, Response::Success);
        assert!(matches!(h.requests[..], [Request::StartSession { .. }]));
        let h = heard(&mut l, error(ErrorType::Error, "exec failed"));
        assert_eq!(l.message(), Some(("The session didn't start: Exec failed", Tone::Error)));
        assert_eq!(h.requests, [Request::CancelSession, create("kiosk")]);
    }

    #[test]
    fn a_failure_to_begin_waits_rather_than_looping() {
        let (mut l, _) = listing(&["ghost"], None);
        let h = heard(&mut l, error(ErrorType::Error, "unknown user"));
        assert_eq!(h.requests, []);
        assert_eq!(progress(&l), Some(Progress::Failed));
        assert_eq!(l.message(), Some(("Unknown user", Tone::Error)));
        // Return tries again, cancelling whatever greetd kept first.
        assert_eq!(enter(&mut l, "x"), [Request::CancelSession, create("ghost")]);
        assert_eq!(progress(&l), Some(Progress::Checking));
    }

    #[test]
    fn a_wrong_password_at_once_still_waits_and_counts() {
        // PAM refusing at the very start (no question asked) stops there,
        // as any failure to begin does, and the field shakes.
        let (mut l, _) = listing(&["alice"], None);
        let h = heard(&mut l, error(ErrorType::AuthError, "pam_authenticate: AUTH_ERR"));
        assert_eq!(h.requests, []);
        assert_eq!(h.outcome, Outcome::Refused);
        assert_eq!(progress(&l), Some(Progress::Failed));
    }

    #[test]
    fn a_late_answer_for_someone_no_longer_picked_starts_nothing() {
        let (mut l, _) = listing(&["alice", "bob"], Some("alice"));
        heard(&mut l, secret("Password: "));
        enter(&mut l, "alices");
        // Bob is picked while greetd checks Alice's password.
        assert_eq!(escape(&mut l), [Request::CancelSession]);
        assert_eq!(l.pick(1), [create("bob")]);
        // Alice's success, then the cancel's answer: neither starts anything.
        assert_eq!(heard(&mut l, Response::Success).requests, []);
        assert_eq!(heard(&mut l, Response::Success).requests, []);
        assert_eq!(progress(&l), Some(Progress::Checking));
        heard(&mut l, secret("Password: "));
        assert_eq!(l.user(), Some("bob"));
        assert_eq!(progress(&l), ASKED);
        // Back to the people: cancelled, and its answer let go.
        assert_eq!(escape(&mut l), [Request::CancelSession]);
        assert_eq!(selected(&l), Some(1));
        assert_eq!(heard(&mut l, Response::Success).requests, []);
        assert_eq!(escape(&mut l), []);
    }

    #[test]
    fn answering_only_when_asked() {
        let (mut l, _) = listing(&["alice"], None);
        // greetd hasn't asked anything yet.
        assert_eq!(enter(&mut l, "x"), []);
        assert_eq!(progress(&l), Some(Progress::Checking));
    }

    #[test]
    fn a_mistyped_name_goes_back_to_be_corrected() {
        let mut l = typing();
        l.type_name("alcie".into());
        assert_eq!(enter(&mut l, ""), [create("alcie")]);
        assert_eq!(escape_to(&l), Some(EscapeTo::Name));
        heard(&mut l, secret("Password: "));
        enter(&mut l, "hunter2");
        // PAM doesn't say the name was wrong: the same name is asked again.
        let h = heard(&mut l, error(ErrorType::AuthError, "pam_authenticate: AUTH_ERR"));
        assert_eq!(h.requests, [Request::CancelSession, create("alcie")]);
        heard(&mut l, Response::Success);
        heard(&mut l, secret("Password: "));
        // Escape: greetd lets it go, and the name is as typed.
        assert_eq!(escape(&mut l), [Request::CancelSession]);
        assert_eq!(typed(&l), Some("alcie"));
        assert!(l.message().is_none());
        l.type_name("alice".into());
        assert_eq!(enter(&mut l, ""), [create("alice")]);
        // The first name's late answers belong to nobody.
        assert_eq!(heard(&mut l, Response::Success).requests, []);
        heard(&mut l, secret("Password: "));
        assert_eq!(l.user(), Some("alice"));
    }

    #[test]
    fn a_typed_name_that_cant_begin_tries_again_or_goes_back() {
        let mut l = typing();
        l.type_name("alcie".into());
        enter(&mut l, "");
        heard(&mut l, error(ErrorType::Error, "pam_start: user unknown"));
        assert_eq!(progress(&l), Some(Progress::Failed));
        // Return begins again, for the name typed.
        assert_eq!(enter(&mut l, "x"), [Request::CancelSession, create("alcie")]);
        // greetd answers the cancel, then fails the name again.
        heard(&mut l, Response::Success);
        heard(&mut l, error(ErrorType::Error, "pam_start: user unknown"));
        assert_eq!(progress(&l), Some(Progress::Failed));
        // Escape goes back to the name.
        assert_eq!(escape(&mut l), [Request::CancelSession]);
        assert_eq!(typed(&l), Some("alcie"));
    }

    #[test]
    fn nothing_typed_logs_nobody_in() {
        let mut l = typing();
        l.type_name("   ".into());
        assert_eq!(enter(&mut l, ""), []);
        assert_eq!(typed(&l), Some("   "));
    }

    #[test]
    fn the_only_person_has_no_way_back_but_can_try_again() {
        let (mut l, _) = listing(&["solo"], None);
        assert_eq!(escape_to(&l), Some(EscapeTo::Nowhere));
        assert_eq!(escape(&mut l), []);
        heard(&mut l, error(ErrorType::Error, "account expired"));
        assert_eq!(escape(&mut l), []);
        assert_eq!(enter(&mut l, ""), [Request::CancelSession, create("solo")]);
    }

    #[test]
    fn a_session_starting_is_not_cancelled() {
        let (mut l, _) = listing(&["alice", "bob"], Some("alice"));
        heard(&mut l, Response::Success);
        assert_eq!(progress(&l), Some(Progress::Starting));
        assert_eq!(escape_to(&l), Some(EscapeTo::Nowhere));
        assert_eq!(escape(&mut l), []);
        assert_eq!(progress(&l), Some(Progress::Starting));
    }

    #[test]
    fn a_lost_service_fails_the_login_and_lets_its_answers_go() {
        let (mut l, _) = listing(&["alice", "bob"], Some("alice"));
        l.lost("broken pipe");
        assert_eq!(progress(&l), Some(Progress::Failed));
        assert_eq!(l.message(), Some(("Lost the login service: broken pipe", Tone::Error)));
        assert_eq!(heard(&mut l, secret("Password: ")).requests, []);
        assert_eq!(progress(&l), Some(Progress::Failed));
        // Return tries again (greetd itself is gone: it restarts the greeter).
        assert_eq!(enter(&mut l, ""), [create("alice")]);
    }

    #[test]
    fn the_first_stage_follows_the_list() {
        let (l, sent) = listing(&["alice", "bob"], None);
        assert_eq!((selected(&l), sent), (Some(0), Vec::new()));
        let (l, sent) = listing(&["alice", "bob"], Some("bob"));
        assert_eq!((l.user(), sent), (Some("bob"), vec![create("bob")]));
        let (l, _) = listing(&["alice", "bob"], Some("carol"));
        assert_eq!(selected(&l), Some(0));
        let mut l = Login::with_users(vec![user("alice"), user("bob")]);
        assert!(l.start(false, Some("alice")).is_empty());
        assert_eq!(typed(&l), Some(""));
    }
}
