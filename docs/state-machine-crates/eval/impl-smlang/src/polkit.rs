//! `polkit` with smlang 0.8.0. The `Active` superstate is emulated: its
//! request data rides in every substate's data (`Run` for the three states
//! with a conversation, `Request` for `RetryWait` and `Unavailable`), and
//! its rows (`Dismiss`, `CancelRequest`, `Begin`, `ChooseIdentity`) are
//! repeated once per substate data type and per exit effect. Entry and exit
//! hooks take no state data and no sink, so the dialog's and the
//! conversation's entry/exit effects are spelled out in the actions.

use smlang::statemachine;
use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

/// `Begin` event data: cookie, identities, preferred, screen.
pub type Bg = (Cookie, u8, u8, bool);

/// The request (`Active`'s data).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub cookie: Cookie,
    pub identities: u8,
    pub identity: u8,
    pub unanswered: u8,
}

/// A running conversation: `Starting`, `Prompted` and `Checking`'s data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub req: Request,
    pub conv: u32,
}

/// The generated machine, kept private: its `new_with_state` and
/// `context_mut` would let callers forge a state.
mod fsm {
    use super::*;

    statemachine! {
        temporary_context: &mut impl Sink<Effect>,
        derive_states: [Clone, Copy, Debug],
        derive_events: [Clone, Copy, Debug],
        transitions: {
            *Idle + Begin(Bg) [idle_refuses] / refuse,
            Idle + Begin(Bg) / open = Starting(Run),
            // `Begin` in `Active`: same cookie ignored (no guard passes).
            Starting(Run) | Prompted(Run) | Checking(Run) + Begin(Bg) [other_run && nobody_run] / refuse_run = _,
            Starting(Run) | Prompted(Run) | Checking(Run) + Begin(Bg) [other_run && no_screen_run] / end_run_refuse = Idle,
            Starting(Run) | Prompted(Run) | Checking(Run) + Begin(Bg) [other_run] / end_run_open = Starting(Run),
            RetryWait(Request) | Unavailable(Request) + Begin(Bg) [other_req && nobody_req] / refuse_req = _,
            RetryWait(Request) + Begin(Bg) [other_req && no_screen_req] / end_retry_refuse = Idle,
            RetryWait(Request) + Begin(Bg) [other_req] / end_retry_open = Starting(Run),
            Unavailable(Request) + Begin(Bg) [other_req && no_screen_req] / end_quiet_refuse = Idle,
            Unavailable(Request) + Begin(Bg) [other_req] / end_quiet_open = Starting(Run),
            // End the request.
            Starting(Run) | Prompted(Run) | Checking(Run) + Dismiss / end_run = Idle,
            RetryWait(Request) + Dismiss / end_retry = Idle,
            Unavailable(Request) + Dismiss / end_quiet = Idle,
            Starting(Run) | Prompted(Run) | Checking(Run) + AuthorityChanged / end_run = Idle,
            RetryWait(Request) + AuthorityChanged / end_retry = Idle,
            Unavailable(Request) + AuthorityChanged / end_quiet = Idle,
            Starting(Run) | Prompted(Run) | Checking(Run) + CancelRequest(Cookie) [cancels_run] / cancel_run = Idle,
            RetryWait(Request) + CancelRequest(Cookie) [cancels_req] / cancel_retry = Idle,
            Unavailable(Request) + CancelRequest(Cookie) [cancels_req] / cancel_quiet = Idle,
            // The conversation.
            Starting(Run) | Checking(Run) + Prompt((u32, bool)) [prompt_current] / show_prompt = Prompted(Run),
            Starting(Run) | Prompted(Run) | Checking(Run) + PamError(u32) [current] / pam_error = _,
            Starting(Run) | Prompted(Run) | Checking(Run) + PamInfo(u32) [current] / pam_info = _,
            Starting(Run) | Prompted(Run) | Checking(Run) + Success(u32) [current] / succeed = Idle,
            Checking(Run) + Failure(u32) [current] / try_again = Starting(Run),
            Starting(Run) | Prompted(Run) + Failure(u32) [current && retries_left] / end_back_off = RetryWait(Request),
            Starting(Run) | Prompted(Run) + Failure(u32) [current] / end_give_up = Unavailable(Request),
            Starting(Run) + Unreachable(u32) [current && retries_left] / back_off = RetryWait(Request),
            Starting(Run) + Unreachable(u32) [current] / give_up = Unavailable(Request),
            Prompted(Run) + Submit / write_answer = Checking(Run),
            Starting(Run) | Prompted(Run) | Checking(Run) + ChooseIdentity(u8) [identity_run] / restart_run = Starting(Run),
            RetryWait(Request) + ChooseIdentity(u8) [identity_req] / restart_retry = Starting(Run),
            Unavailable(Request) + ChooseIdentity(u8) [identity_req] / restart_quiet = Starting(Run),
            RetryWait(Request) + RetryTimer / retry = Starting(Run),
        }
    }
}

pub use fsm::{Error, Events, States};
use fsm::{StateMachine, StateMachineContext};

/// The context: configuration and the machine-wide conversation counter.
pub struct Agent {
    config: Config,
    last_conv: u32,
}

impl Agent {
    fn new(config: Config) -> Self {
        Self { config, last_conv: 0 }
    }

    pub fn last_conv(&self) -> u32 {
        self.last_conv
    }

    /// start(identity).
    fn start(&mut self, req: Request, out: &mut impl Sink<Effect>) -> Run {
        self.last_conv += 1;
        out.push(Effect::StartHelper { conv: self.last_conv, identity: req.identity });
        Run { req, conv: self.last_conv }
    }

    /// Opens the dialog for a new request and starts its first conversation.
    fn open_request(&mut self, (cookie, identities, preferred, _): Bg, out: &mut impl Sink<Effect>) -> Run {
        let identity = if preferred < identities { preferred } else { 0 };
        out.push(Effect::OpenDialog { cookie });
        self.start(Request { cookie, identities, identity, unanswered: 0 }, out)
    }

    /// back off: `Retrying` and the timer (the row's guard said retries are left).
    fn back_off_retry(&self, mut req: Request, out: &mut impl Sink<Effect>) -> Request {
        req.unanswered = req.unanswered.saturating_add(1);
        out.push(Effect::ShowNote(Note::Retrying));
        out.push(Effect::ArmRetry { ms: self.config.retry_delay(req.unanswered) });
        req
    }

    fn back_off_give_up(&self, mut req: Request, out: &mut impl Sink<Effect>) -> Request {
        req.unanswered = req.unanswered.saturating_add(1);
        out.push(Effect::ShowNote(Note::Unavailable));
        req
    }

    fn restart(&mut self, mut req: Request, i: u8, out: &mut impl Sink<Effect>) -> Run {
        req.identity = i;
        req.unanswered = 0;
        self.start(req, out)
    }
}

fn refused(cookie: Cookie, out: &mut impl Sink<Effect>) {
    out.push(Effect::Reply { cookie, authorized: false });
}

/// The end of the request's exit effects (after its conversation or timer ended).
fn close(req: &Request, out: &mut impl Sink<Effect>) {
    refused(req.cookie, out);
    out.push(Effect::CloseDialog { cookie: req.cookie });
}

fn end_conv(run: &Run, out: &mut impl Sink<Effect>) {
    out.push(Effect::EndHelper { conv: run.conv });
}

type R<T = ()> = Result<T, ()>;

impl StateMachineContext for Agent {
    fn idle_refuses(&self, _: &mut impl Sink<Effect>, b: &Bg) -> R<bool> {
        Ok(b.1 == 0 || !b.3)
    }
    fn other_run(&self, _: &mut impl Sink<Effect>, run: &Run, b: &Bg) -> R<bool> {
        Ok(b.0 != run.req.cookie)
    }
    fn nobody_run(&self, _: &mut impl Sink<Effect>, _: &Run, b: &Bg) -> R<bool> {
        Ok(b.1 == 0)
    }
    fn no_screen_run(&self, _: &mut impl Sink<Effect>, _: &Run, b: &Bg) -> R<bool> {
        Ok(!b.3)
    }
    fn other_req(&self, _: &mut impl Sink<Effect>, req: &Request, b: &Bg) -> R<bool> {
        Ok(b.0 != req.cookie)
    }
    fn nobody_req(&self, _: &mut impl Sink<Effect>, _: &Request, b: &Bg) -> R<bool> {
        Ok(b.1 == 0)
    }
    fn no_screen_req(&self, _: &mut impl Sink<Effect>, _: &Request, b: &Bg) -> R<bool> {
        Ok(!b.3)
    }
    fn cancels_run(&self, _: &mut impl Sink<Effect>, run: &Run, c: &Cookie) -> R<bool> {
        Ok(*c == run.req.cookie)
    }
    fn cancels_req(&self, _: &mut impl Sink<Effect>, req: &Request, c: &Cookie) -> R<bool> {
        Ok(*c == req.cookie)
    }
    fn prompt_current(&self, _: &mut impl Sink<Effect>, run: &Run, p: &(u32, bool)) -> R<bool> {
        Ok(p.0 == run.conv)
    }
    fn current(&self, _: &mut impl Sink<Effect>, run: &Run, conv: &u32) -> R<bool> {
        Ok(*conv == run.conv)
    }
    fn retries_left(&self, _: &mut impl Sink<Effect>, run: &Run, _: &u32) -> R<bool> {
        Ok(run.req.unanswered.saturating_add(1) < self.config.max_unanswered)
    }
    fn identity_run(&self, _: &mut impl Sink<Effect>, run: &Run, i: &u8) -> R<bool> {
        Ok(*i < run.req.identities)
    }
    fn identity_req(&self, _: &mut impl Sink<Effect>, req: &Request, i: &u8) -> R<bool> {
        Ok(*i < req.identities)
    }

    fn refuse(&mut self, out: &mut impl Sink<Effect>, b: Bg) -> R {
        refused(b.0, out);
        Ok(())
    }
    fn open(&mut self, out: &mut impl Sink<Effect>, b: Bg) -> R<Run> {
        Ok(self.open_request(b, out))
    }
    fn refuse_run(&mut self, out: &mut impl Sink<Effect>, run: &Run, b: Bg) -> R<Run> {
        refused(b.0, out);
        Ok(*run)
    }
    fn end_run_refuse(&mut self, out: &mut impl Sink<Effect>, run: &Run, b: Bg) -> R {
        end_conv(run, out);
        close(&run.req, out);
        refused(b.0, out);
        Ok(())
    }
    fn end_run_open(&mut self, out: &mut impl Sink<Effect>, run: &Run, b: Bg) -> R<Run> {
        end_conv(run, out);
        close(&run.req, out);
        Ok(self.open_request(b, out))
    }
    fn refuse_req(&mut self, out: &mut impl Sink<Effect>, req: &Request, b: Bg) -> R<Request> {
        refused(b.0, out);
        Ok(*req)
    }
    fn end_retry_refuse(&mut self, out: &mut impl Sink<Effect>, req: &Request, b: Bg) -> R {
        out.push(Effect::CancelRetry);
        close(req, out);
        refused(b.0, out);
        Ok(())
    }
    fn end_retry_open(&mut self, out: &mut impl Sink<Effect>, req: &Request, b: Bg) -> R<Run> {
        out.push(Effect::CancelRetry);
        close(req, out);
        Ok(self.open_request(b, out))
    }
    fn end_quiet_refuse(&mut self, out: &mut impl Sink<Effect>, req: &Request, b: Bg) -> R {
        close(req, out);
        refused(b.0, out);
        Ok(())
    }
    fn end_quiet_open(&mut self, out: &mut impl Sink<Effect>, req: &Request, b: Bg) -> R<Run> {
        close(req, out);
        Ok(self.open_request(b, out))
    }
    fn end_run(&mut self, out: &mut impl Sink<Effect>, run: &Run) -> R {
        end_conv(run, out);
        close(&run.req, out);
        Ok(())
    }
    fn end_retry(&mut self, out: &mut impl Sink<Effect>, req: &Request) -> R {
        out.push(Effect::CancelRetry);
        close(req, out);
        Ok(())
    }
    fn end_quiet(&mut self, out: &mut impl Sink<Effect>, req: &Request) -> R {
        close(req, out);
        Ok(())
    }
    fn cancel_run(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: Cookie) -> R {
        self.end_run(out, run)
    }
    fn cancel_retry(&mut self, out: &mut impl Sink<Effect>, req: &Request, _: Cookie) -> R {
        self.end_retry(out, req)
    }
    fn cancel_quiet(&mut self, out: &mut impl Sink<Effect>, req: &Request, _: Cookie) -> R {
        self.end_quiet(out, req)
    }
    fn show_prompt(&mut self, out: &mut impl Sink<Effect>, run: &Run, (_, echo): (u32, bool)) -> R<Run> {
        out.push(Effect::ShowPrompt { echo });
        Ok(Run { req: Request { unanswered: 0, ..run.req }, ..*run })
    }
    fn pam_error(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Run> {
        out.push(Effect::ShowNote(Note::PamError));
        Ok(*run)
    }
    fn pam_info(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Run> {
        out.push(Effect::ShowNote(Note::PamInfo));
        Ok(*run)
    }
    fn succeed(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R {
        end_conv(run, out);
        out.push(Effect::Reply { cookie: run.req.cookie, authorized: true });
        out.push(Effect::CloseDialog { cookie: run.req.cookie });
        Ok(())
    }
    fn try_again(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Run> {
        end_conv(run, out);
        out.push(Effect::ShowNote(Note::TryAgain));
        Ok(self.start(run.req, out))
    }
    fn end_back_off(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Request> {
        end_conv(run, out);
        Ok(self.back_off_retry(run.req, out))
    }
    fn end_give_up(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Request> {
        end_conv(run, out);
        Ok(self.back_off_give_up(run.req, out))
    }
    fn back_off(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Request> {
        Ok(self.back_off_retry(run.req, out))
    }
    fn give_up(&mut self, out: &mut impl Sink<Effect>, run: &Run, _: u32) -> R<Request> {
        Ok(self.back_off_give_up(run.req, out))
    }
    fn write_answer(&mut self, out: &mut impl Sink<Effect>, run: &Run) -> R<Run> {
        out.push(Effect::WriteAnswer { conv: run.conv });
        Ok(*run)
    }
    fn restart_run(&mut self, out: &mut impl Sink<Effect>, run: &Run, i: u8) -> R<Run> {
        end_conv(run, out);
        Ok(self.restart(run.req, i, out))
    }
    fn restart_retry(&mut self, out: &mut impl Sink<Effect>, req: &Request, i: u8) -> R<Run> {
        out.push(Effect::CancelRetry);
        Ok(self.restart(*req, i, out))
    }
    fn restart_quiet(&mut self, out: &mut impl Sink<Effect>, req: &Request, i: u8) -> R<Run> {
        Ok(self.restart(*req, i, out))
    }
    fn retry(&mut self, out: &mut impl Sink<Effect>, req: &Request) -> R<Run> {
        Ok(self.start(*req, out))
    }
}

pub struct Polkit {
    sm: StateMachine<Agent>,
}

impl Polkit {
    /// The generated `process_event`: the typed transition API.
    pub fn process_event(&mut self, out: &mut impl Sink<Effect>, event: Events) -> Result<&States, Error> {
        self.sm.process_event(out, event)
    }

    /// The generated state.
    pub fn state_enum(&self) -> &States {
        self.sm.state()
    }

    /// The context, read-only.
    pub fn context(&self) -> &Agent {
        self.sm.context()
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Self { sm: StateMachine::new(Agent::new(config)) }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let event = match event {
            Event::Begin { cookie, identities, preferred, screen } => {
                Events::Begin((cookie, identities, preferred, screen))
            }
            Event::CancelRequest { cookie } => Events::CancelRequest(cookie),
            Event::AuthorityChanged => Events::AuthorityChanged,
            Event::Unreachable { conv } => Events::Unreachable(conv),
            Event::Prompt { conv, echo } => Events::Prompt((conv, echo)),
            Event::PamError { conv } => Events::PamError(conv),
            Event::PamInfo { conv } => Events::PamInfo(conv),
            Event::Success { conv } => Events::Success(conv),
            Event::Failure { conv } => Events::Failure(conv),
            Event::ChooseIdentity(i) => Events::ChooseIdentity(i),
            Event::Submit => Events::Submit,
            Event::Dismiss => Events::Dismiss,
            Event::RetryTimer => Events::RetryTimer,
        };
        // Undeclared pairs, stale conversations and failed guards are `Err`: ignored.
        let _ = self.sm.process_event(out, event);
    }

    fn state(&self) -> StateName {
        match self.sm.state() {
            States::Idle => StateName::Idle,
            States::Starting(_) => StateName::Starting,
            States::Prompted(_) => StateName::Prompted,
            States::Checking(_) => StateName::Checking,
            States::RetryWait(_) => StateName::RetryWait,
            States::Unavailable(_) => StateName::Unavailable,
        }
    }
}
