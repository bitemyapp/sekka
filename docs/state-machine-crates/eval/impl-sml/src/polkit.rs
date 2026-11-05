//! `polkit` with stateforward-sml 2.0.0: `Active` is a child table of the
//! request table. Entering `Active` opens the dialog, entering `Starting`
//! starts a conversation and entering `RetryWait` arms the timer (entry
//! actions); the exits the spec names depend on the target, so they are
//! transition actions. The request (Active's data) and the conversation
//! counter are the context. Callbacks can't see the active child state, so
//! the context also records whether a helper runs and whether the retry
//! timer is armed, for ending a request from the parent table.
//!
//! Events are declared with payload types (`Failure(Conv)`), not
//! `event<T>`: a callback's signature includes its event's payload type, so
//! events sharing a payload share their guards and actions.

use crate::OneDeferred;
use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

pub type Out<'a> = &'a mut dyn Sink<Effect>;

/// `Begin`'s payload.
#[derive(Clone, Copy)]
pub struct Begin {
    pub cookie: Cookie,
    pub identities: u8,
    pub preferred: u8,
    pub screen: bool,
}

/// The conversation a helper event names.
pub struct Conv(pub u32);

pub struct Prompt {
    pub conv: u32,
    pub echo: bool,
}

sml::sml! {
    Active[temporary_context: &mut dyn Sink<Effect>] {
        *Starting + on_entry<_> / start_helper,
         RetryWait + on_entry<_> / arm_retry,
         Starting | Checking + Prompt(Prompt) [prompt_current] / show_prompt = Prompted,
         Prompted + Submit / write_answer = Checking,
         Checking + Failure(Conv) [current] / (end_helper, try_again) = Starting,
         Starting | Prompted + Failure(Conv) [current && exhausted] / (end_helper, give_up) = Unavailable,
         Starting | Prompted + Failure(Conv) [current] / (end_helper, retrying) = RetryWait,
         Starting + Unreachable(Conv) [current && exhausted] / (drop_helper, give_up) = Unavailable,
         Starting + Unreachable(Conv) [current] / (drop_helper, retrying) = RetryWait,
         Starting | Prompted | Checking + ChooseIdentity(u8) [known] / (end_conversation, choose) = Starting,
         RetryWait + ChooseIdentity(u8) [known] / (cancel_timer, choose) = Starting,
         Unavailable + ChooseIdentity(u8) [known] / choose = Starting,
         RetryWait + RetryTimer / timer_fired = Starting,
    }

    Polkit[temporary_context: &mut dyn Sink<Effect>] {
        *Idle + Begin(Begin) [opens] / take_request = state<Active>,
         Idle + Begin(Begin) / refuse,
         state<Active> + on_entry<_> / open_dialog,
         state<Active> + Begin(Begin) [same_request],
         state<Active> + Begin(Begin) [no_identities] / refuse,
         state<Active> + Begin(Begin) / (end_for_another, defer) = Idle,
         state<Active> + CancelRequest(Cookie) [ours] / end_cancelled = Idle,
         state<Active> + AuthorityChanged / end_request = Idle,
         state<Active> + Dismiss / end_request = Idle,
         state<Active> + Success(Conv) [current] / authorize = Idle,
         state<Active> + PamError(Conv) [current] / note_pam_error,
         state<Active> + PamInfo(Conv) [current] / note_pam_info,
    }
}

/// `Active`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub cookie: Cookie,
    pub identities: u8,
    pub identity: u8,
    pub unanswered: u8,
}

/// The context.
pub struct Agent {
    config: Config,
    last_conv: u32,
    request: Request,
    /// The running conversation (in `Starting`, `Prompted`, `Checking`).
    helper: Option<u32>,
    /// The retry timer runs (in `RetryWait`).
    retry_armed: bool,
}

type R = Result<(), ()>;
type G = Result<bool, ()>;

impl Agent {
    fn end_helper(&mut self, out: Out<'_>) {
        if let Some(conv) = self.helper.take() {
            out.push(Effect::EndHelper { conv });
        }
    }

    fn cancel_retry(&mut self, out: Out<'_>) {
        if std::mem::take(&mut self.retry_armed) {
            out.push(Effect::CancelRetry);
        }
    }

    fn reply(&mut self, out: Out<'_>, authorized: bool) {
        let cookie = self.request.cookie;
        out.push(Effect::Reply { cookie, authorized });
        out.push(Effect::CloseDialog { cookie });
    }

    /// Ends the request unauthorized.
    fn end(&mut self, out: Out<'_>) {
        self.end_helper(out);
        self.cancel_retry(out);
        self.reply(out, false);
    }
}

impl PolkitStateMachineContext for Agent {
    fn start_helper(&mut self, out: Out<'_>) -> R {
        self.last_conv += 1;
        self.helper = Some(self.last_conv);
        out.push(Effect::StartHelper { conv: self.last_conv, identity: self.request.identity });
        Ok(())
    }

    fn arm_retry(&mut self, out: Out<'_>) -> R {
        self.retry_armed = true;
        out.push(Effect::ArmRetry { ms: self.config.retry_delay(self.request.unanswered) });
        Ok(())
    }

    fn prompt_current(&self, _: Out<'_>, e: &Prompt) -> G {
        Ok(self.helper == Some(e.conv))
    }

    fn show_prompt(&mut self, out: Out<'_>, e: &Prompt) -> R {
        out.push(Effect::ShowPrompt { echo: e.echo });
        self.request.unanswered = 0;
        Ok(())
    }

    fn write_answer(&mut self, out: Out<'_>) -> R {
        out.push(Effect::WriteAnswer { conv: self.last_conv });
        Ok(())
    }

    fn current(&self, _: Out<'_>, e: &Conv) -> G {
        Ok(self.helper == Some(e.0))
    }

    fn exhausted(&self, _: Out<'_>, _: &Conv) -> G {
        Ok(self.request.unanswered.saturating_add(1) >= self.config.max_unanswered)
    }

    fn end_helper(&mut self, out: Out<'_>, _: &Conv) -> R {
        Agent::end_helper(self, out);
        Ok(())
    }

    /// The helper never ran: nothing to end.
    fn drop_helper(&mut self, _: Out<'_>, _: &Conv) -> R {
        self.helper = None;
        Ok(())
    }

    fn try_again(&mut self, out: Out<'_>, _: &Conv) -> R {
        out.push(Effect::ShowNote(Note::TryAgain));
        Ok(())
    }

    fn give_up(&mut self, out: Out<'_>, _: &Conv) -> R {
        self.request.unanswered = self.request.unanswered.saturating_add(1);
        out.push(Effect::ShowNote(Note::Unavailable));
        Ok(())
    }

    fn retrying(&mut self, out: Out<'_>, _: &Conv) -> R {
        self.request.unanswered = self.request.unanswered.saturating_add(1);
        out.push(Effect::ShowNote(Note::Retrying));
        Ok(())
    }

    fn known(&self, _: Out<'_>, i: &u8) -> G {
        Ok(*i < self.request.identities)
    }

    fn end_conversation(&mut self, out: Out<'_>, _: &u8) -> R {
        Agent::end_helper(self, out);
        Ok(())
    }

    fn cancel_timer(&mut self, out: Out<'_>, _: &u8) -> R {
        self.cancel_retry(out);
        Ok(())
    }

    fn choose(&mut self, _: Out<'_>, i: &u8) -> R {
        self.request.identity = *i;
        self.request.unanswered = 0;
        Ok(())
    }

    fn timer_fired(&mut self, _: Out<'_>) -> R {
        self.retry_armed = false;
        Ok(())
    }

    fn opens(&self, _: Out<'_>, e: &Begin) -> G {
        Ok(e.screen && e.identities > 0)
    }

    fn take_request(&mut self, _: Out<'_>, e: &Begin) -> R {
        let identity = if e.preferred < e.identities { e.preferred } else { 0 };
        self.request = Request { cookie: e.cookie, identities: e.identities, identity, unanswered: 0 };
        Ok(())
    }

    fn refuse(&mut self, out: Out<'_>, e: &Begin) -> R {
        out.push(Effect::Reply { cookie: e.cookie, authorized: false });
        Ok(())
    }

    fn open_dialog(&mut self, out: Out<'_>) -> R {
        out.push(Effect::OpenDialog { cookie: self.request.cookie });
        Ok(())
    }

    fn same_request(&self, _: Out<'_>, e: &Begin) -> G {
        Ok(e.cookie == self.request.cookie)
    }

    fn no_identities(&self, _: Out<'_>, e: &Begin) -> G {
        Ok(e.identities == 0)
    }

    /// Another request: end this one; `defer` then takes it up in `Idle`.
    fn end_for_another(&mut self, out: Out<'_>, _: &Begin) -> R {
        self.end(out);
        Ok(())
    }

    fn ours(&self, _: Out<'_>, c: &Cookie) -> G {
        Ok(*c == self.request.cookie)
    }

    fn end_cancelled(&mut self, out: Out<'_>, _: &Cookie) -> R {
        self.end(out);
        Ok(())
    }

    fn end_request(&mut self, out: Out<'_>) -> R {
        self.end(out);
        Ok(())
    }

    fn authorize(&mut self, out: Out<'_>, _: &Conv) -> R {
        Agent::end_helper(self, out);
        self.reply(out, true);
        Ok(())
    }

    fn note_pam_error(&mut self, out: Out<'_>, _: &Conv) -> R {
        out.push(Effect::ShowNote(Note::PamError));
        Ok(())
    }

    fn note_pam_info(&mut self, out: Out<'_>, _: &Conv) -> R {
        out.push(Effect::ShowNote(Note::PamInfo));
        Ok(())
    }
}

pub struct Polkit {
    /// The generated machine: its `process_event`, `is` and
    /// `visit_current_states` are the typed API.
    pub machine: PolkitStateMachine<Agent, OneDeferred>,
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        let request = Request { cookie: Cookie(0), identities: 0, identity: 0, unanswered: 0 };
        let agent = Agent { config, last_conv: 0, request, helper: None, retry_armed: false };
        Self { machine: PolkitStateMachine::new_with_policy(agent, OneDeferred::default()) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        use PolkitEvents as E;
        let event = match event {
            Event::Begin { cookie, identities, preferred, screen } => {
                E::Begin(Begin { cookie, identities, preferred, screen })
            }
            Event::CancelRequest { cookie } => E::CancelRequest(cookie),
            Event::AuthorityChanged => E::AuthorityChanged,
            Event::Unreachable { conv } => E::Unreachable(Conv(conv)),
            Event::Prompt { conv, echo } => E::Prompt(Prompt { conv, echo }),
            Event::PamError { conv } => E::PamError(Conv(conv)),
            Event::PamInfo { conv } => E::PamInfo(Conv(conv)),
            Event::Success { conv } => E::Success(Conv(conv)),
            Event::Failure { conv } => E::Failure(Conv(conv)),
            Event::ChooseIdentity(i) => E::ChooseIdentity(i),
            Event::Submit => E::Submit,
            Event::Dismiss => E::Dismiss,
            Event::RetryTimer => E::RetryTimer,
        };
        // An event no row takes is `Err(InvalidEvent)`: ignored, as the spec says.
        let _ = self.machine.process_event(out, event);
    }

    fn state(&self) -> StateName {
        let m = &self.machine;
        if m.is(&PolkitStates::Idle) {
            StateName::Idle
        } else if m.is(&PolkitActiveStates::Starting) {
            StateName::Starting
        } else if m.is(&PolkitActiveStates::Prompted) {
            StateName::Prompted
        } else if m.is(&PolkitActiveStates::Checking) {
            StateName::Checking
        } else if m.is(&PolkitActiveStates::RetryWait) {
            StateName::RetryWait
        } else {
            StateName::Unavailable
        }
    }
}
