//! The authentication dialog's states, as typestate: a [`Dialog`] is in one
//! of its states, each a type of its own, and only the transitions here move
//! it.
//!
//! ```text
//! Starting ─Prompt→ Prompted ─submit→ Checking ─Prompt→ Prompted
//!    │ submit           ↑                 │ Refused → Starting
//!    ↓                  │ (a visible prompt)
//!  Eager ─Prompt→ Checking (the password typed early answers it)
//! any ─Retrying→ Waiting ─Prompt→ Prompted
//! any ─Unavailable→ Unavailable ─try_again→ Starting
//! any ─choose(identity)→ Starting
//! ```
//!
//! The dialog follows the agent's conversation (`aizome-sys`'s
//! `polkit::conversation`), which decides when an attempt starts. What the
//! types hold to:
//!
//! - **Only an event proven to be about the request's latest conversation
//!   moves the dialog.** [`DialogAny::heard`] takes a `Checked`, which only
//!   the agent's check ([`Dialog::check`], through `Generation::check_answer`)
//!   makes, and the event's payload is read only from it. An event of
//!   another request's is refused too.
//! - **Nothing here starts a conversation on an event.** An attempt that
//!   ended with no answer leaves the dialog [`Waiting`] or [`Unavailable`];
//!   the agent starts the next one when its floor allows. Only the person
//!   starts one: [`DialogAny::choose`] and [`Unavailable::try_again`].
//! - **An answer goes to a prompt the dialog shows**: only [`Prompted`]
//!   holds the prompt's `Asked`, which the answer spends. A password
//!   submitted before the helper asked ([`Eager`]) answers the
//!   conversation's first prompt, if it hides what's typed, and nothing
//!   else.

// Typestate code: every state and every case is named, so a new one
// added later can't fall into an arm written for others.
#![deny(clippy::wildcard_enum_match_arm)]

use aizome_kit::prelude::WindowKey;
use aizome_sys::polkit::{Asked, AuthRequest, Checked, Heard, NotOffered, RequestId, Said, Unsendable, Unsent, said};

use super::{AuthView, Submit};

/// The line under the field.
#[derive(Clone, Debug, PartialEq)]
enum Note {
    Error(String),
    Info(String),
}

/// What every state shares.
struct Ctx {
    request: AuthRequest,
    /// Its window, until the compositor takes it away.
    window: Option<WindowKey>,
    identity: usize,
    password: String,
    /// The helper's last prompt, as shown.
    prompt: Option<String>,
    note: Option<Note>,
}

/// A password submitted before the helper asked for it.
pub struct Held(String);

aizome_typestate::machine! {
    /// The authentication dialog for one polkit request.
    pub struct Dialog over Ctx {
        /// Where the dialog is.
        stored DialogState;
        /// The dialog's typed state, holding what the states share.
        typed DialogAny;
        states {
            /// An attempt is starting: nothing asked yet.
            Starting,
            /// The person submitted before the helper asked.
            Eager(Held),
            /// The helper asks; the person may answer.
            Prompted(Asked),
            /// The answer is being checked. A second submit has no prompt
            /// to go to:
            ///
            /// ```compile_fail,E0599
            /// use aizome_shell::auth::dialog::Checking;
            /// fn again(checking: Checking<'_>) {
            ///     checking.submit();
            /// }
            /// ```
            ///
            /// ```
            /// use aizome_shell::auth::dialog::{Checking, Prompted};
            /// fn answer(prompted: Prompted<'_>) -> Result<Checking<'_>, Prompted<'_>> {
            ///     prompted.submit()
            /// }
            /// ```
            Checking,
            /// The agent waits before its next attempt: an attempt ended
            /// with no answer. Nothing here starts another. The old Failed
            /// handler did, at once, as the request's identity:
            ///
            /// ```compile_fail,E0616
            /// use aizome_shell::auth::dialog::Waiting;
            /// fn failed(waiting: Waiting<'_>) {
            ///     let a = waiting.ctx;
            ///     if let Some(i) = a.request.identities().get(a.identity) {
            ///         a.request.authenticate(i);
            ///     }
            /// }
            /// ```
            ///
            /// and the person's Try Again isn't here either:
            ///
            /// ```compile_fail,E0599
            /// use aizome_shell::auth::dialog::{Starting, Waiting};
            /// fn failed(waiting: Waiting<'_>) -> Starting<'_> {
            ///     waiting.try_again()
            /// }
            /// ```
            ///
            /// The dialog waits for what the agent says next:
            ///
            /// ```
            /// use aizome_shell::auth::dialog::{DialogAny, Waiting};
            /// use aizome_sys::polkit::Checked;
            /// fn failed<'s>(waiting: Waiting<'s>, next: Option<Checked>) -> DialogAny<'s> {
            ///     let dialog = DialogAny::from(waiting);
            ///     match next {
            ///         Some(fresh) => dialog.heard(fresh),
            ///         None => dialog,
            ///     }
            /// }
            /// ```
            Waiting,
            /// The agent stopped trying until the person asks.
            Unavailable,
        }
        access(private);
        new(private);
        with_typed(pub(crate));
    }
}

impl Dialog {
    /// The dialog for polkitd's `request` in `window`, as the identity it
    /// suggests: the first attempt starts.
    pub fn begin(request: AuthRequest, window: WindowKey) -> Dialog {
        let identity =
            request.default_identity().and_then(|d| request.identities().iter().position(|i| i == d)).unwrap_or(0);
        let started = match request.identities().get(identity) {
            Some(i) => request.authenticate(i),
            None => Err(NotOffered),
        };
        let mut dialog = Dialog::new(Ctx {
            request,
            window: Some(window),
            identity,
            password: String::new(),
            prompt: None,
            note: None,
        });
        if let Err(NotOffered) = started {
            dialog.with_typed(|s| (s.not_offered(), ()));
        }
        dialog
    }

    /// The proof that `heard` is about this dialog's request and its latest
    /// conversation, for [`DialogAny::heard`].
    pub fn check(&self, heard: Heard) -> Result<Checked, Heard> {
        self.ctx.request.check(heard)
    }

    /// Which request it is.
    pub fn id(&self) -> RequestId {
        self.ctx.request.id()
    }

    pub fn window(&self) -> Option<WindowKey> {
        self.ctx.window
    }

    /// The compositor took its window away (its output went).
    pub fn window_gone(&mut self) {
        self.ctx.window = None;
    }

    /// It shows in `window` now.
    pub fn shown_in(&mut self, window: WindowKey) {
        self.ctx.window = Some(window);
    }

    pub fn type_password(&mut self, password: String) {
        self.ctx.password = password;
    }

    /// The person dismissed it: polkitd hears no. The window to close.
    pub fn cancel(self) -> Option<WindowKey> {
        self.ctx.request.cancel();
        self.ctx.window
    }

    /// polkitd has its answer. The window to close.
    pub fn close(self) -> Option<WindowKey> {
        self.ctx.window
    }

    /// What it shows.
    pub fn view(&self) -> AuthView {
        let c = &self.ctx;
        let shown = || c.prompt.clone().unwrap_or_else(|| "Password:".into());
        let (prompt, echo, busy, submit) = match &self.stored {
            DialogState::Prompted(asked) => (asked.text().to_owned(), asked.echo(), false, Submit::Answer),
            DialogState::Starting(()) => (shown(), false, false, Submit::Answer),
            DialogState::Eager(_) | DialogState::Checking(()) => (shown(), false, true, Submit::Wait),
            DialogState::Waiting(()) => (shown(), false, false, Submit::Answer),
            DialogState::Unavailable(()) => (shown(), false, false, Submit::TryAgain),
        };
        let note = c.note.as_ref().map(|n| match n {
            Note::Error(text) => (text.clone(), true),
            Note::Info(text) => (text.clone(), false),
        });
        AuthView {
            message: c.request.message.clone(),
            identities: c.request.identities().iter().map(|i| format!("{} ({})", i.display_name(), i.name())).collect(),
            identity: c.identity,
            prompt,
            echo,
            password: c.password.clone(),
            note,
            busy,
            submit,
        }
    }
}

impl<'s> DialogAny<'s> {
    fn ctx(&self) -> &Ctx {
        match self {
            DialogAny::Starting(s) => &*s.ctx,
            DialogAny::Eager(s) => &*s.ctx,
            DialogAny::Prompted(s) => &*s.ctx,
            DialogAny::Checking(s) => &*s.ctx,
            DialogAny::Waiting(s) => &*s.ctx,
            DialogAny::Unavailable(s) => &*s.ctx,
        }
    }

    fn ctx_mut(&mut self) -> &mut Ctx {
        match self {
            DialogAny::Starting(s) => &mut *s.ctx,
            DialogAny::Eager(s) => &mut *s.ctx,
            DialogAny::Prompted(s) => &mut *s.ctx,
            DialogAny::Checking(s) => &mut *s.ctx,
            DialogAny::Waiting(s) => &mut *s.ctx,
            DialogAny::Unavailable(s) => &mut *s.ctx,
        }
    }

    /// Leaves the state (a held password or a prompt is dropped).
    fn into_ctx(self) -> &'s mut Ctx {
        match self {
            DialogAny::Starting(s) => s.ctx,
            DialogAny::Eager(s) => s.ctx,
            DialogAny::Prompted(s) => s.ctx,
            DialogAny::Checking(s) => s.ctx,
            DialogAny::Waiting(s) => s.ctx,
            DialogAny::Unavailable(s) => s.ctx,
        }
    }

    /// What the request's conversation said, proven to be about its latest
    /// attempt.
    ///
    /// An event that wasn't checked doesn't do:
    ///
    /// ```compile_fail,E0308
    /// use aizome_shell::auth::dialog::DialogAny;
    /// use aizome_sys::polkit::Heard;
    /// // Whichever conversation it came from.
    /// fn hear<'s>(dialog: DialogAny<'s>, heard: Heard) -> DialogAny<'s> {
    ///     dialog.heard(heard)
    /// }
    /// ```
    ///
    /// ```
    /// use aizome_shell::auth::dialog::{Dialog, DialogAny};
    /// use aizome_sys::polkit::{Checked, Heard};
    /// fn check(dialog: &Dialog, heard: Heard) -> Option<Checked> {
    ///     dialog.check(heard).ok()
    /// }
    /// fn hear<'s>(dialog: DialogAny<'s>, checked: Checked) -> DialogAny<'s> {
    ///     dialog.heard(checked)
    /// }
    /// ```
    pub fn heard(self, checked: Checked) -> DialogAny<'s> {
        // Checked against another dialog's request: not this one's to apply.
        if checked.request() != self.ctx().request.id() {
            return self;
        }
        match said(checked) {
            Said::Prompt(asked) => match self {
                DialogAny::Eager(eager) => eager.asked(asked),
                s @ (DialogAny::Starting(_)
                | DialogAny::Prompted(_)
                | DialogAny::Checking(_)
                | DialogAny::Waiting(_)
                | DialogAny::Unavailable(_)) => {
                    let ctx = s.into_ctx();
                    ctx.prompt = Some(asked.text().to_owned());
                    Prompted::enter(ctx, asked).into()
                }
            },
            Said::Error(text) => {
                let mut s = self;
                s.ctx_mut().note = Some(Note::Error(text));
                s
            }
            Said::Info(text) => {
                let mut s = self;
                s.ctx_mut().note = Some(Note::Info(text));
                s
            }
            // As the lock screen does: PAM's own words when its modules said
            // why (a lockout, an expired password), else a sentence of ours.
            Said::Refused { pam } => {
                let ctx = self.into_ctx();
                ctx.password.clear();
                ctx.note = Some(Note::Error(pam.unwrap_or_else(|| "That didn't work. Try again.".into())));
                Starting::enter(ctx, ()).into()
            }
            Said::Retrying { failed, pam, .. } => {
                let ctx = self.into_ctx();
                match (failed, pam) {
                    (Some(_), Some(pam)) => ctx.note = Some(Note::Error(pam)),
                    (Some(_), None) => ctx.note = Some(Note::Info("Trying again…".into())),
                    // The floor holds a start: what's shown still stands.
                    (None, Some(_) | None) => {}
                }
                Waiting::enter(ctx, ()).into()
            }
            Said::Unavailable { pam, .. } => {
                let ctx = self.into_ctx();
                let why = pam.unwrap_or_else(|| "Authentication isn't available right now.".into());
                ctx.note = Some(Note::Error(why));
                Unavailable::enter(ctx, ()).into()
            }
        }
    }

    /// The agent refused the identity (polkitd didn't offer it for this
    /// request): nothing starts, and the dialog says so.
    fn not_offered(self) -> DialogAny<'s> {
        let ctx = self.into_ctx();
        ctx.note = Some(Note::Error("That account can't authenticate this.".into()));
        Unavailable::enter(ctx, ()).into()
    }

    /// The person picked identity `i`: an attempt as them starts.
    pub fn choose(self, i: usize) -> DialogAny<'s> {
        if i >= self.ctx().request.identities().len() {
            return self;
        }
        let ctx = self.into_ctx();
        ctx.identity = i;
        ctx.note = None;
        match ctx.request.authenticate(&ctx.request.identities()[i]) {
            Ok(()) => Starting::enter(ctx, ()).into(),
            Err(NotOffered) => DialogAny::from(Starting::enter(ctx, ())).not_offered(),
        }
    }
}

impl<'s> Starting<'s> {
    /// The person submitted before the helper asked: the password waits
    /// for the first prompt.
    pub fn submit(self) -> Eager<'s> {
        let password = std::mem::take(&mut self.ctx.password);
        self.ctx.note = None;
        Eager::enter(self.ctx, Held(password))
    }
}

impl<'s> Waiting<'s> {
    /// The person submitted while the agent waits to try again: the
    /// password waits for the next attempt's first prompt.
    pub fn submit(self) -> Eager<'s> {
        let password = std::mem::take(&mut self.ctx.password);
        Eager::enter(self.ctx, Held(password))
    }
}

/// What the line under the field says about an answer that wasn't sent.
fn unsent(why: Unsendable) -> Note {
    Note::Error(
        match why {
            Unsendable::LineBreak => "The answer can't contain a line break.",
            Unsendable::Nul => "The answer can't contain that character.",
            Unsendable::TooLong => "The answer is too long.",
        }
        .into(),
    )
}

impl<'s> Eager<'s> {
    /// The helper asked: a hidden prompt takes the held password; a visible
    /// one (a user name) is shown, and the password isn't put in its field.
    fn asked(self, asked: Asked) -> DialogAny<'s> {
        let Eager { ctx, data: Held(password) } = self;
        ctx.prompt = Some(asked.text().to_owned());
        if asked.echo() {
            return Prompted::enter(ctx, asked).into();
        }
        match asked.answer(&password) {
            Ok(()) => Checking::enter(ctx, ()).into(),
            Err(Unsent { asked, why }) => {
                ctx.note = Some(unsent(why));
                Prompted::enter(ctx, asked).into()
            }
        }
    }
}

impl<'s> Prompted<'s> {
    /// The person answered the prompt shown, or the answer can't be sent
    /// and the prompt stays, saying why.
    pub fn submit(self) -> Result<Checking<'s>, Prompted<'s>> {
        let Prompted { ctx, data: asked } = self;
        let password = std::mem::take(&mut ctx.password);
        match asked.answer(&password) {
            Ok(()) => {
                ctx.note = None;
                Ok(Checking::enter(ctx, ()))
            }
            Err(Unsent { asked, why }) => {
                ctx.password = password;
                ctx.note = Some(unsent(why));
                Err(Prompted::enter(ctx, asked))
            }
        }
    }
}

impl<'s> Unavailable<'s> {
    /// The person asks for another attempt.
    pub fn try_again(self) -> DialogAny<'s> {
        let ctx = self.ctx;
        ctx.note = None;
        let started = match ctx.request.identities().get(ctx.identity) {
            Some(identity) => ctx.request.authenticate(identity),
            None => Err(NotOffered),
        };
        match started {
            Ok(()) => Starting::enter(ctx, ()).into(),
            Err(NotOffered) => DialogAny::from(Unavailable::enter(ctx, ())).not_offered(),
        }
    }
}
