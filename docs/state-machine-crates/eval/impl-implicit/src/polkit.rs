//! `polkit` in the current shape: aizome-sys's `SessionInner { reply,
//! conversation, generation }` (polkit.rs:195-202) beside the shell's
//! `Option<AuthState>` with `prompt`, `busy` and `error` (auth.rs:58-68).

use crate::BUGS;
use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

/// aizome-sys's side of one `BeginAuthentication`.
#[derive(Clone, Copy, Debug)]
pub struct SessionInner {
    /// Some while polkitd still waits for the reply.
    pub reply: Option<Cookie>,
    /// The conversation running, if any.
    pub conversation: Option<u32>,
}

/// The shell's dialog.
#[derive(Clone, Copy, Debug)]
pub struct AuthState {
    pub cookie: Cookie,
    pub identities: u8,
    pub identity: u8,
    /// The prompt shown (its `echo`), if any.
    pub prompt: Option<bool>,
    pub busy: bool,
    pub error: Option<Note>,
    /// Failures in a row before any answer (the fix's counter).
    pub unanswered: u8,
    pub retry_armed: bool,
    pub unavailable: bool,
}

/// The machine's fields, in the current code's shape. Readable through
/// the machine (`Deref`); only `handle` changes them.
pub struct Fields {
    pub config: Config,
    pub session: Option<SessionInner>,
    pub auth: Option<AuthState>,
    /// SPEC-FIX: one counter for all sessions (aizome-sys counts per session).
    pub generation: u32,
}

impl Fields {
    /// `Session::start`: end the old conversation, count, connect.
    fn authenticate(&mut self, out: &mut impl Sink<Effect>) {
        let Some(s) = &mut self.session else { return };
        if s.reply.is_none() {
            return;
        }
        if let Some(old) = s.conversation.take() {
            out.push(Effect::EndHelper { conv: old });
        }
        self.generation += 1;
        s.conversation = Some(self.generation);
        let identity = self.auth.as_ref().map_or(0, |a| a.identity);
        out.push(Effect::StartHelper { conv: self.generation, identity });
        if let Some(a) = &mut self.auth {
            // SPEC-FIX: a restart shows no prompt until the helper asks.
            a.prompt = None;
        }
    }

    /// `Session::finish`, then the shell closing its dialog.
    fn finish(&mut self, authorized: bool, out: &mut impl Sink<Effect>) {
        let auth = self.auth.take();
        if let Some(s) = self.session.take() {
            if let Some(c) = s.conversation {
                out.push(Effect::EndHelper { conv: c });
            }
            if auth.is_some_and(|a| a.retry_armed) {
                out.push(Effect::CancelRetry);
            }
            if let Some(cookie) = s.reply {
                out.push(Effect::Reply { cookie, authorized });
            }
        }
        if let Some(a) = auth {
            out.push(Effect::CloseDialog { cookie: a.cookie });
        }
    }

    fn current(&self, conv: u32) -> bool {
        self.session.is_some_and(|s| s.reply.is_some() && s.conversation == Some(conv))
    }

    /// The shell's `Failed` handler.
    fn failed(&mut self, out: &mut impl Sink<Effect>) {
        let Some(a) = &mut self.auth else { return };
        let answered = a.busy;
        a.busy = false;
        if BUGS || answered {
            // BUG(SYB-09, SHB-03): restart at once, whatever the failure;
            // a helper that can't be reached spins.
            a.error = Some(Note::TryAgain);
            out.push(Effect::ShowNote(Note::TryAgain));
            self.authenticate(out);
            return;
        }
        a.unanswered = a.unanswered.saturating_add(1);
        if a.unanswered >= self.config.max_unanswered {
            a.unavailable = true;
            a.error = Some(Note::Unavailable);
            out.push(Effect::ShowNote(Note::Unavailable));
        } else {
            a.retry_armed = true;
            a.error = Some(Note::Retrying);
            out.push(Effect::ShowNote(Note::Retrying));
            out.push(Effect::ArmRetry { ms: self.config.retry_delay(a.unanswered) });
        }
    }
}

impl Fields {
    fn new(config: Config) -> Self {
        Self { config, session: None, auth: None, generation: 0 }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        match event {
            Event::Begin { cookie, identities, preferred, screen } => {
                if self.auth.is_some_and(|a| a.cookie == cookie) {
                    return;
                }
                if identities == 0 {
                    out.push(Effect::Reply { cookie, authorized: false });
                    return;
                }
                if self.auth.is_some() {
                    self.finish(false, out);
                }
                if !screen {
                    out.push(Effect::Reply { cookie, authorized: false });
                    return;
                }
                out.push(Effect::OpenDialog { cookie });
                self.session = Some(SessionInner { reply: Some(cookie), conversation: None });
                self.auth = Some(AuthState {
                    cookie,
                    identities,
                    identity: if preferred < identities { preferred } else { 0 },
                    prompt: None,
                    busy: false,
                    error: None,
                    unanswered: 0,
                    retry_armed: false,
                    unavailable: false,
                });
                self.authenticate(out);
            }
            Event::CancelRequest { cookie } => {
                if self.auth.is_some_and(|a| a.cookie == cookie) {
                    self.finish(false, out);
                }
            }
            Event::AuthorityChanged | Event::Dismiss => self.finish(false, out),
            Event::Prompt { conv, echo } => {
                if !self.current(conv) {
                    return;
                }
                let Some(a) = &mut self.auth else { return };
                if a.prompt.is_some() && !a.busy {
                    return;
                }
                a.prompt = Some(echo);
                a.busy = false;
                a.unanswered = 0;
                out.push(Effect::ShowPrompt { echo });
            }
            Event::PamError { conv } => {
                if self.current(conv)
                    && let Some(a) = &mut self.auth
                {
                    // SPEC-FIX: the shell also cleared `busy` here.
                    a.error = Some(Note::PamError);
                    out.push(Effect::ShowNote(Note::PamError));
                }
            }
            Event::PamInfo { conv } => {
                if self.current(conv) {
                    out.push(Effect::ShowNote(Note::PamInfo));
                }
            }
            Event::Success { conv } => {
                if self.current(conv) {
                    self.finish(true, out);
                }
            }
            Event::Failure { conv } => {
                if self.current(conv) {
                    if let Some(s) = &mut self.session {
                        s.conversation = None;
                    }
                    out.push(Effect::EndHelper { conv });
                    self.failed(out);
                }
            }
            Event::Unreachable { conv } => {
                // The connect error comes before any line: only while starting.
                let starting = self.auth.is_some_and(|a| a.prompt.is_none() && !a.busy);
                if self.current(conv) && starting {
                    if let Some(s) = &mut self.session {
                        s.conversation = None;
                    }
                    self.failed(out);
                }
            }
            Event::ChooseIdentity(i) => {
                let Some(a) = &mut self.auth else { return };
                if i >= a.identities {
                    return;
                }
                a.identity = i;
                a.error = None;
                a.busy = false;
                a.unanswered = 0;
                a.unavailable = false;
                if std::mem::take(&mut a.retry_armed) {
                    out.push(Effect::CancelRetry);
                }
                self.authenticate(out);
            }
            Event::Submit => {
                let Some(a) = &mut self.auth else { return };
                // SPEC-FIX: only a shown prompt takes an answer.
                if a.busy || a.prompt.is_none() {
                    return;
                }
                let Some(conv) = self.session.and_then(|s| s.conversation) else { return };
                a.busy = true;
                a.error = None;
                out.push(Effect::WriteAnswer { conv });
            }
            Event::RetryTimer => {
                if let Some(a) = &mut self.auth
                    && std::mem::take(&mut a.retry_armed)
                {
                    self.authenticate(out);
                }
            }
        }
    }

    fn state(&self) -> StateName {
        let Some(a) = &self.auth else { return StateName::Idle };
        if a.retry_armed {
            StateName::RetryWait
        } else if a.unavailable {
            StateName::Unavailable
        } else if a.busy {
            StateName::Checking
        } else if a.prompt.is_some() {
            StateName::Prompted
        } else {
            StateName::Starting
        }
    }
}

/// The machine: [`Fields`] read through `Deref`, never written from outside.
pub struct Polkit(Fields);

impl std::ops::Deref for Polkit {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.0
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Polkit(Fields::new(config))
    }
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.0.handle(event, out);
    }
    fn state(&self) -> StateName {
        self.0.state()
    }
}
