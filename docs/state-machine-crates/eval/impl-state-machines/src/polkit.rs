//! `polkit` with state-machines 0.34.0: `Active` is a superstate holding the
//! request, its exit hook closes the dialog; the running conversation is
//! the data of `Starting`, `Prompted` and `Checking`, produced by `data:`
//! factories (starting the helper is what yields a conversation number).
//! A `Begin` for another cookie is an external transition out of `Active`
//! and back in (`kind: external`), so the exit hook and the data reset run.

use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};
use state_machines::state_machine;

pub const IMPLEMENTED: bool = true;

pub use machine::{
    Active, Auth, AuthCheckingBeginOutcome, AuthIdleBeginOutcome, AuthPromptedBeginOutcome, AuthPromptedFailureOutcome,
    AuthRetryWaitBeginOutcome, AuthStartingBeginOutcome, AuthStartingFailureOutcome, AuthStartingUnreachableOutcome,
    AuthUnavailableBeginOutcome, Checking, Idle, Phase, Polkit, Prompted, RetryWait, Starting, Unavailable,
};

/// `Active`'s data. Superstate data is `Default`-initialized on entry (no
/// factory can supply it), then `take_request` fills it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    cookie: Cookie,
    identities: u8,
    identity: u8,
    unanswered: u8,
}

impl Default for Request {
    fn default() -> Self {
        Self { cookie: Cookie(0), identities: 0, identity: 0, unanswered: 0 }
    }
}

/// `Begin`'s payload.
#[derive(Clone, Copy, Debug)]
pub struct Begin {
    pub cookie: Cookie,
    pub identities: u8,
    pub preferred: u8,
    pub screen: bool,
}

/// The payloads that name a conversation.
pub trait Conversation {
    fn conv(&self) -> u32;
}

impl Conversation for u32 {
    fn conv(&self) -> u32 {
        *self
    }
}
impl Conversation for (u32, bool) {
    fn conv(&self) -> u32 {
        self.0
    }
}

/// The context: the config, the machine-wide conversation counter, the effects.
#[derive(Debug)]
pub struct Ctx {
    config: Config,
    last_conv: u32,
    out: Vec<Effect>,
}

impl Ctx {
    pub fn new(config: Config) -> Self {
        Self { config, last_conv: 0, out: Vec::with_capacity(crate::out::CAPACITY) }
    }
}

/// The generated machines, their callbacks and the facade. The module is
/// private so that the generated `Dynamic*` wrapper (with its
/// `new_init_state`), its event and its state enums aren't exported.
mod machine {
    use super::*;
    use crate::out::drain;

    state_machine! {
        name: Auth,
        context: Ctx,
        dynamic: true,
        initial: Idle,
        states: [
            Idle,
            superstate Active(Request) {
                state Starting(u32),
                state Prompted(u32),
                state Checking(u32),
                state RetryWait,
                state Unavailable,
            },
        ],
        lifecycle: {
            Active { exit: [close_dialog] }
        },
        events {
            begin {
                payload: Begin,
                branching: true,
                // polkitd's duplicate.
                transition: { from: Active, internal: true, guards: [is_open] }
                // Nobody may authenticate: refused before the shell sees it.
                transition: { from: Active, internal: true, guards: [nobody], before: [refuse] }
                transition: {
                    from: Active, to: Starting, kind: external, guards: [can_open],
                    before: [stop_for, deny_for], data: open_and_start, after: [take_request]
                }
                transition: { from: Active, to: Idle, fallback: true, before: [stop_for, deny_for], after: [refuse] }
                transition: { from: Idle, to: Starting, guards: [can_open], data: open_and_start, after: [take_request] }
                transition: { from: Idle, internal: true, fallback: true, before: [refuse] }
            }
            cancel_request {
                payload: Cookie,
                transition: { from: Active, to: Idle, guards: [is_request], before: [stop_for, deny_for] }
            }
            // A callback's arity follows its event's payload: these two have
            // none, so they take `stop`/`deny` rather than `stop_for`/`deny_for`.
            authority_changed {
                transition: { from: Active, to: Idle, before: [stop, deny] }
            }
            dismiss {
                transition: { from: Active, to: Idle, before: [stop, deny] }
            }
            prompt {
                payload: (u32, bool),
                transition: { from: [Starting, Checking], to: Prompted, guards: [current], data: prompted, before: [show_prompt] }
            }
            pam_error {
                payload: u32,
                transition: { from: [Starting, Prompted, Checking], internal: true, guards: [current], before: [note_pam_error] }
            }
            pam_info {
                payload: u32,
                transition: { from: [Starting, Prompted, Checking], internal: true, guards: [current], before: [note_pam_info] }
            }
            success {
                payload: u32,
                transition: { from: [Starting, Prompted, Checking], to: Idle, guards: [current], before: [end_helper, grant] }
            }
            failure {
                payload: u32,
                guards: [current],
                branching: true,
                // An answer was checked and refused: try again at once.
                transition: { from: Checking, to: Starting, fallback: true, before: [end_helper, try_again], data: restart }
                // No answer was given: back off.
                transition: { from: [Starting, Prompted], to: RetryWait, guards: [retries_left], before: [end_helper, retry_later] }
                transition: { from: [Starting, Prompted], to: Unavailable, fallback: true, before: [end_helper, give_up] }
            }
            unreachable {
                payload: u32,
                guards: [current],
                branching: true,
                transition: { from: Starting, to: RetryWait, guards: [retries_left], before: [retry_later] }
                transition: { from: Starting, to: Unavailable, fallback: true, before: [give_up] }
            }
            submit {
                transition: { from: Prompted, to: Checking, data: keep_conv, before: [write_answer] }
            }
            choose_identity {
                payload: u8,
                transition: { from: Active, to: Starting, guards: [known_identity], before: [stop_for, choose], data: restart }
            }
            retry_timer {
                transition: { from: RetryWait, to: Starting, data: start_helper }
            }
        }
    }

    /// What leaving each leaf of `Active` must stop: the conversation (its data
    /// says), or the retry timer.
    pub trait Phase {
        const TIMER: bool = false;
    }

    impl Phase for Starting {}
    impl Phase for Prompted {}
    impl Phase for Checking {}
    impl Phase for RetryWait {
        const TIMER: bool = true;
    }
    impl Phase for Unavailable {}

    impl<S: Phase> Auth<S> {
        /// Ends the conversation or the retry timer: the first part of ending the request.
        fn stop(&mut self) {
            if let Some(conv) = self.conv() {
                self.ctx.out.push(Effect::EndHelper { conv });
            }
            if S::TIMER {
                self.ctx.out.push(Effect::CancelRetry);
            }
        }

        fn stop_for<P>(&mut self, _: &P) {
            self.stop();
        }
    }

    impl<S> Auth<S> {
        /// `Active`'s data, set on entry (missing only if built in an `Active` state).
        fn request(&mut self) -> &mut Request {
            self.__state_data_active.get_or_insert_default()
        }

        /// The running conversation: the data of whichever of the three leaves this is.
        fn conv(&self) -> Option<u32> {
            self.state_data_starting().or(self.state_data_prompted()).or(self.state_data_checking()).copied()
        }

        fn start_with(&mut self, identity: u8) -> u32 {
            self.ctx.last_conv += 1;
            self.ctx.out.push(Effect::StartHelper { conv: self.ctx.last_conv, identity });
            self.ctx.last_conv
        }

        // Factories: the next leaf's conversation.

        fn open_and_start(&mut self, b: &mut Begin) -> u32 {
            self.ctx.out.push(Effect::OpenDialog { cookie: b.cookie });
            self.start_with(if b.preferred < b.identities { b.preferred } else { 0 })
        }

        fn start_helper(&mut self) -> u32 {
            let identity = self.request().identity;
            self.start_with(identity)
        }

        fn restart<P>(&mut self, _: &mut P) -> u32 {
            self.start_helper()
        }

        fn prompted(&mut self, p: &mut (u32, bool)) -> u32 {
            p.0
        }

        fn keep_conv(&mut self) -> u32 {
            self.conv().expect("Prompted is entered with its conversation")
        }

        // Guards.

        fn is_open(&self, _: &Ctx, b: &Begin) -> bool {
            self.state_data_active().is_some_and(|r| r.cookie == b.cookie)
        }

        fn nobody(&self, _: &Ctx, b: &Begin) -> bool {
            b.identities == 0
        }

        fn can_open(&self, _: &Ctx, b: &Begin) -> bool {
            b.screen && b.identities > 0
        }

        fn is_request(&self, _: &Ctx, cookie: &Cookie) -> bool {
            self.state_data_active().is_some_and(|r| r.cookie == *cookie)
        }

        fn current<P: Conversation>(&self, _: &Ctx, p: &P) -> bool {
            self.conv() == Some(p.conv())
        }

        fn retries_left(&self, ctx: &Ctx, _: &u32) -> bool {
            self.state_data_active().is_some_and(|r| r.unanswered.saturating_add(1) < ctx.config.max_unanswered)
        }

        fn known_identity(&self, _: &Ctx, &i: &u8) -> bool {
            self.state_data_active().is_some_and(|r| i < r.identities)
        }

        // Callbacks and hooks.

        fn take_request(&mut self, b: &Begin) {
            let identity = if b.preferred < b.identities { b.preferred } else { 0 };
            *self.request() = Request { cookie: b.cookie, identities: b.identities, identity, unanswered: 0 };
        }

        fn refuse(&mut self, b: &Begin) {
            self.ctx.out.push(Effect::Reply { cookie: b.cookie, authorized: false });
        }

        fn end_helper<P>(&mut self, _: &P) {
            if let Some(conv) = self.conv() {
                self.ctx.out.push(Effect::EndHelper { conv });
            }
        }

        fn deny(&mut self) {
            let cookie = self.request().cookie;
            self.ctx.out.push(Effect::Reply { cookie, authorized: false });
        }

        fn deny_for<P>(&mut self, _: &P) {
            self.deny();
        }

        fn grant(&mut self, _: &u32) {
            let cookie = self.request().cookie;
            self.ctx.out.push(Effect::Reply { cookie, authorized: true });
        }

        /// `Active`'s exit hook.
        fn close_dialog(&mut self) {
            let cookie = self.request().cookie;
            self.ctx.out.push(Effect::CloseDialog { cookie });
        }

        fn show_prompt(&mut self, &(_, echo): &(u32, bool)) {
            self.ctx.out.push(Effect::ShowPrompt { echo });
            self.request().unanswered = 0;
        }

        fn note_pam_error(&mut self, _: &u32) {
            self.ctx.out.push(Effect::ShowNote(Note::PamError));
        }

        fn note_pam_info(&mut self, _: &u32) {
            self.ctx.out.push(Effect::ShowNote(Note::PamInfo));
        }

        fn try_again(&mut self, _: &u32) {
            self.ctx.out.push(Effect::ShowNote(Note::TryAgain));
        }

        fn retry_later(&mut self, _: &u32) {
            let request = self.request();
            request.unanswered = request.unanswered.saturating_add(1);
            let unanswered = request.unanswered;
            let ms = self.ctx.config.retry_delay(unanswered);
            self.ctx.out.push(Effect::ShowNote(Note::Retrying));
            self.ctx.out.push(Effect::ArmRetry { ms });
        }

        fn give_up(&mut self, _: &u32) {
            let request = self.request();
            request.unanswered = request.unanswered.saturating_add(1);
            self.ctx.out.push(Effect::ShowNote(Note::Unavailable));
        }

        fn write_answer(&mut self) {
            if let Some(conv) = self.conv() {
                self.ctx.out.push(Effect::WriteAnswer { conv });
            }
        }

        fn choose(&mut self, &i: &u8) {
            let request = self.request();
            request.identity = i;
            request.unanswered = 0;
        }
    }

    /// The `Machine` facade over the generated runtime wrapper.
    pub struct Polkit {
        machine: DynamicAuth,
    }

    /// The context inside the wrapper (`None` once a panicking callback
    /// poisoned it). `inner` and `AnyAuthState` are private generated
    /// items, visible here because the macro expanded in this module.
    impl DynamicAuth {
        fn ctx(&mut self) -> Option<&mut Ctx> {
            Some(match self.inner.as_mut()? {
                AnyAuthState::Idle(m) => &mut m.ctx,
                AnyAuthState::Starting(m) => &mut m.ctx,
                AnyAuthState::Prompted(m) => &mut m.ctx,
                AnyAuthState::Checking(m) => &mut m.ctx,
                AnyAuthState::RetryWait(m) => &mut m.ctx,
                AnyAuthState::Unavailable(m) => &mut m.ctx,
            })
        }
    }

    impl<S> Auth<S> {
        /// Takes the effects the callbacks buffered, for driving the typed
        /// machine directly (the compile-fail probes do).
        pub fn take_effects(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.ctx.out)
        }
    }

    impl Machine for Polkit {
        fn new(config: Config) -> Self {
            Self { machine: DynamicAuth::new(Ctx::new(config)) }
        }

        fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
            let event = match event {
                Event::Begin { cookie, identities, preferred, screen } => {
                    AuthEvent::Begin(Begin { cookie, identities, preferred, screen })
                }
                Event::CancelRequest { cookie } => AuthEvent::CancelRequest(cookie),
                Event::AuthorityChanged => AuthEvent::AuthorityChanged,
                Event::Unreachable { conv } => AuthEvent::Unreachable(conv),
                Event::Prompt { conv, echo } => AuthEvent::Prompt((conv, echo)),
                Event::PamError { conv } => AuthEvent::PamError(conv),
                Event::PamInfo { conv } => AuthEvent::PamInfo(conv),
                Event::Success { conv } => AuthEvent::Success(conv),
                Event::Failure { conv } => AuthEvent::Failure(conv),
                Event::ChooseIdentity(i) => AuthEvent::ChooseIdentity(i),
                Event::Submit => AuthEvent::Submit,
                Event::Dismiss => AuthEvent::Dismiss,
                Event::RetryTimer => AuthEvent::RetryTimer,
            };
            // `Err`: no transition from here, or its guards refused: ignored.
            let _ = self.machine.handle(event);
            if let Some(ctx) = self.machine.ctx() {
                drain(&mut ctx.out, out);
            }
        }

        fn state(&self) -> StateName {
            match self.machine.current_state() {
                AuthState::Idle => StateName::Idle,
                AuthState::Starting => StateName::Starting,
                AuthState::Prompted => StateName::Prompted,
                AuthState::Checking => StateName::Checking,
                AuthState::RetryWait => StateName::RetryWait,
                AuthState::Unavailable => StateName::Unavailable,
            }
        }
    }
}
