//! `polkit` with rust-fsm 0.8.0: the six leaf states are the table (`fsm`);
//! the request (`Active`'s data) and the conversation counter live beside it
//! in [`Data`].
//!
//! What the DSL can't hold, and where it went:
//!
//! - the `Active` superstate: its rows (`Begin`, `Dismiss`,
//!   `ChooseIdentity`) are repeated in each of its five substates;
//! - guards: [`Polkit::input`] drops what the spec ignores (a stale
//!   conversation, a duplicate cookie, another request's cancel, an identity
//!   out of range) and splits the rest by outcome (`BeginNobody`,
//!   `BeginNoScreen`, `Failure`/`FailureLast` for the back-off limit);
//! - exit actions (end the conversation, cancel the retry timer, reply and
//!   close the dialog): the actions that leave a state get the state
//!   `consume` left (`from`) and emit its exit effects themselves;
//! - the table's states are `spec::polkit::StateName` (the crate's custom
//!   state type), so `state()` reads the table directly;
//! - `cookie` is `Some` exactly in `Active` (the actions keep it so), which
//!   is what lets the input drop a duplicate `Begin` without the state.

use rust_fsm::state_machine;
use spec::Sink;
use spec::polkit::{Config, Cookie, Effect, Event, Machine, Note, StateName};

pub const IMPLEMENTED: bool = true;

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[state_machine(state(spec::polkit::StateName))]
    pub fsm(Idle)

    Idle => {
        BeginNobody => Idle [Refuse],
        BeginNoScreen => Idle [Refuse],
        Begin => Starting [Open],
    },
    Starting => {
        BeginNobody => Starting [Refuse],
        BeginNoScreen => Idle [EndRefuse],
        Begin => Starting [EndOpen],
        Dismiss => Idle [End],
        Prompt => Prompted [ShowPrompt],
        PamNote => Starting [Note],
        Success => Idle [Authorized],
        Failure => RetryWait [EndRetrying],
        FailureLast => Unavailable [EndUnavailable],
        Unreachable => RetryWait [Retrying],
        UnreachableLast => Unavailable [Unavailable],
        ChooseIdentity => Starting [Choose],
    },
    Prompted => {
        BeginNobody => Prompted [Refuse],
        BeginNoScreen => Idle [EndRefuse],
        Begin => Starting [EndOpen],
        Dismiss => Idle [End],
        PamNote => Prompted [Note],
        Success => Idle [Authorized],
        Failure => RetryWait [EndRetrying],
        FailureLast => Unavailable [EndUnavailable],
        Submit => Checking [WriteAnswer],
        ChooseIdentity => Starting [Choose],
    },
    Checking => {
        BeginNobody => Checking [Refuse],
        BeginNoScreen => Idle [EndRefuse],
        Begin => Starting [EndOpen],
        Dismiss => Idle [End],
        Prompt => Prompted [ShowPrompt],
        PamNote => Checking [Note],
        Success => Idle [Authorized],
        Failure => Starting [TryAgain],
        FailureLast => Starting [TryAgain],
        ChooseIdentity => Starting [Choose],
    },
    RetryWait => {
        BeginNobody => RetryWait [Refuse],
        BeginNoScreen => Idle [EndRefuse],
        Begin => Starting [EndOpen],
        Dismiss => Idle [End],
        ChooseIdentity => Starting [Choose],
        RetryTimer => Starting [Start],
    },
    Unavailable => {
        BeginNobody => Unavailable [Refuse],
        BeginNoScreen => Idle [EndRefuse],
        Begin => Starting [EndOpen],
        Dismiss => Idle [End],
        ChooseIdentity => Starting [Choose],
    },
}

use fsm::{Input, Output};

/// The table and the data it can't hold, in one struct (so the table's
/// byte packs with the data). `cookie` is `Some` in `Active`; the
/// other request fields are stale outside it. `last_conv` is machine-wide,
/// and it is also the running conversation's number (a start always takes
/// the newest).
#[derive(Clone, Debug, Default)]
pub struct Data {
    fsm: fsm::StateMachine,
    pub cookie: Option<Cookie>,
    pub identities: u8,
    pub identity: u8,
    pub unanswered: u8,
    pub last_conv: u32,
}

pub struct Polkit {
    data: Data,
    config: Config,
}

impl Polkit {
    /// The table's state.
    pub fn table(&self) -> &fsm::StateMachine {
        &self.data.fsm
    }

    /// The data beside the table.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The guards: an event refined by what the data says about it; `None`
    /// for what the spec ignores whatever the state.
    fn input(&self, event: &Event) -> Option<Input> {
        let d = &self.data;
        let current = |conv: u32| conv == d.last_conv;
        let last = d.unanswered.saturating_add(1) >= self.config.max_unanswered;
        Some(match *event {
            Event::Begin { cookie, .. } if d.cookie == Some(cookie) => return None,
            Event::Begin { identities: 0, .. } => Input::BeginNobody,
            Event::Begin { screen: false, .. } => Input::BeginNoScreen,
            Event::Begin { .. } => Input::Begin,
            Event::CancelRequest { cookie } if d.cookie == Some(cookie) => Input::Dismiss,
            Event::CancelRequest { .. } => return None,
            Event::AuthorityChanged | Event::Dismiss => Input::Dismiss,
            Event::Prompt { conv, .. } if current(conv) => Input::Prompt,
            Event::PamError { conv } | Event::PamInfo { conv } if current(conv) => Input::PamNote,
            Event::Success { conv } if current(conv) => Input::Success,
            Event::Failure { conv } if current(conv) => {
                if last {
                    Input::FailureLast
                } else {
                    Input::Failure
                }
            }
            Event::Unreachable { conv } if current(conv) => {
                if last {
                    Input::UnreachableLast
                } else {
                    Input::Unreachable
                }
            }
            Event::Prompt { .. }
            | Event::PamError { .. }
            | Event::PamInfo { .. }
            | Event::Success { .. }
            | Event::Failure { .. }
            | Event::Unreachable { .. } => return None,
            Event::Submit => Input::Submit,
            Event::ChooseIdentity(i) if i < d.identities => Input::ChooseIdentity,
            Event::ChooseIdentity(_) => return None,
            Event::RetryTimer => Input::RetryTimer,
        })
    }

    fn start(&mut self, identity: u8, out: &mut impl Sink<Effect>) {
        self.data.last_conv += 1;
        out.push(Effect::StartHelper { conv: self.data.last_conv, identity });
    }

    /// Leaving a conversation or the retry wait other than by its timer.
    fn exit_phase(&self, from: StateName, out: &mut impl Sink<Effect>) {
        match from {
            StateName::Starting | StateName::Prompted | StateName::Checking => {
                out.push(Effect::EndHelper { conv: self.data.last_conv })
            }
            StateName::RetryWait => out.push(Effect::CancelRetry),
            StateName::Idle | StateName::Unavailable => {}
        }
    }

    /// End the request unauthorized (exit `Active`).
    fn end(&mut self, from: StateName, out: &mut impl Sink<Effect>) {
        self.exit_phase(from, out);
        if let Some(cookie) = self.data.cookie.take() {
            out.push(Effect::Reply { cookie, authorized: false });
            out.push(Effect::CloseDialog { cookie });
        }
    }

    fn open(&mut self, cookie: Cookie, identities: u8, preferred: u8, out: &mut impl Sink<Effect>) {
        let identity = if preferred < identities { preferred } else { 0 };
        out.push(Effect::OpenDialog { cookie });
        let d = &mut self.data;
        (d.cookie, d.identities, d.identity, d.unanswered) = (Some(cookie), identities, identity, 0);
        self.start(identity, out);
    }

    fn back_off(&mut self, end: bool, out: &mut impl Sink<Effect>) {
        if end {
            out.push(Effect::EndHelper { conv: self.data.last_conv });
        }
        self.data.unanswered = self.data.unanswered.saturating_add(1);
        if self.data.unanswered >= self.config.max_unanswered {
            out.push(Effect::ShowNote(Note::Unavailable));
        } else {
            out.push(Effect::ShowNote(Note::Retrying));
            out.push(Effect::ArmRetry { ms: self.config.retry_delay(self.data.unanswered) });
        }
    }

    /// The actions the outputs name; `from` is the state before the step.
    /// Exhaustive on the output, so a new output is a compile error; arms
    /// that need a payload take it from their event (the table pairs every
    /// output with one kind of event).
    fn act(&mut self, action: Output, from: StateName, event: Event, out: &mut impl Sink<Effect>) {
        match action {
            Output::Refuse => {
                let Event::Begin { cookie, .. } = event else { unreachable!() };
                out.push(Effect::Reply { cookie, authorized: false });
            }
            Output::Open => {
                let Event::Begin { cookie, identities, preferred, .. } = event else { unreachable!() };
                self.open(cookie, identities, preferred, out);
            }
            Output::EndRefuse => {
                let Event::Begin { cookie, .. } = event else { unreachable!() };
                self.end(from, out);
                out.push(Effect::Reply { cookie, authorized: false });
            }
            Output::EndOpen => {
                let Event::Begin { cookie, identities, preferred, .. } = event else { unreachable!() };
                self.end(from, out);
                self.open(cookie, identities, preferred, out);
            }
            Output::End => self.end(from, out),
            Output::ShowPrompt => {
                let Event::Prompt { echo, .. } = event else { unreachable!() };
                out.push(Effect::ShowPrompt { echo });
                self.data.unanswered = 0;
            }
            Output::Note => out.push(Effect::ShowNote(match event {
                Event::PamError { .. } => Note::PamError,
                Event::PamInfo { .. } => Note::PamInfo,
                _ => unreachable!(),
            })),
            Output::Authorized => {
                out.push(Effect::EndHelper { conv: self.data.last_conv });
                if let Some(cookie) = self.data.cookie.take() {
                    out.push(Effect::Reply { cookie, authorized: true });
                    out.push(Effect::CloseDialog { cookie });
                }
            }
            Output::TryAgain => {
                out.push(Effect::EndHelper { conv: self.data.last_conv });
                out.push(Effect::ShowNote(Note::TryAgain));
                self.start(self.data.identity, out);
            }
            Output::EndRetrying | Output::EndUnavailable => self.back_off(true, out),
            Output::Retrying | Output::Unavailable => self.back_off(false, out),
            Output::WriteAnswer => out.push(Effect::WriteAnswer { conv: self.data.last_conv }),
            Output::Choose => {
                let Event::ChooseIdentity(i) = event else { unreachable!() };
                self.exit_phase(from, out);
                self.data.identity = i;
                self.data.unanswered = 0;
                self.start(i, out);
            }
            Output::Start => self.start(self.data.identity, out),
        }
    }
}

impl Machine for Polkit {
    fn new(config: Config) -> Self {
        Self { data: Data::default(), config }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let Some(input) = self.input(&event) else { return };
        let from = *self.data.fsm.state();
        // `Err`: no row for this (state, input): the spec ignores the event.
        if let Ok(Some(action)) = self.data.fsm.consume(&input) {
            self.act(action, from, event, out);
        }
    }

    fn state(&self) -> StateName {
        *self.data.fsm.state()
    }
}
