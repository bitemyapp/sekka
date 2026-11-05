//! `install` with rust-fsm 0.8.0: the twenty stages are the table (`fsm`);
//! the sizes live beside it in [`Data`].
//!
//! What the DSL can't hold, and where it went:
//!
//! - `deferred` (data of `Prepared`, `Erase`, `Format`, `Files`) is a bool,
//!   so the table holds it as a second copy of those states (`…Deferred`),
//!   which `state()` folds back; this is the only state data the DSL holds;
//! - `root_mib` and `closure_mib` live beside the table; the room checks
//!   are guards, decided by [`Install::input`] (`EvaluatedNoRoom`, …);
//! - the `Preparing` and `Executing` superstates: their `Cancel`,
//!   `Confirm`, `StageFailed` rows are repeated per stage, and `Cancelling`
//!   lists every stage result (each guard-split input) one row at a time;
//! - entry and exit actions (`Run`, `StayAwake`, `AllowSleep`): each row's
//!   output does them.

use rust_fsm::state_machine;
use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

state_machine! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub fsm(Idle)

    Idle => {
        Begin => Checks [RunChecks],
        Cancel => Cancelled [Cancelled],
        Confirm => Failed [Failed],
        ConfirmWrong => Failed [Failed],
    },
    Checks => {
        Checked => Configuration [RunConfiguration],
        Cancel => Cancelling [Interrupt],
        Confirm => Failed [InterruptFailed],
        ConfirmWrong => Failed [InterruptFailed],
        StageFailed => Failed [Failed],
    },
    Configuration => {
        Configured => Evaluation [RunEvaluation],
        Cancel => Cancelling [Interrupt],
        Confirm => Failed [InterruptFailed],
        ConfirmWrong => Failed [InterruptFailed],
        StageFailed => Failed [Failed],
    },
    Evaluation => {
        EvaluatedFits => Build [RunBuild],
        EvaluatedDeferred => PreparedDeferred [PreparedDeferred],
        EvaluatedNoRoom => Refused [Refused],
        Cancel => Cancelling [Interrupt],
        Confirm => Failed [InterruptFailed],
        ConfirmWrong => Failed [InterruptFailed],
        StageFailed => Failed [Failed],
    },
    Build => {
        Built => Cache [RunCache],
        BuiltNoRoom => Refused [Refused],
        Cancel => Cancelling [Interrupt],
        Confirm => Failed [InterruptFailed],
        ConfirmWrong => Failed [InterruptFailed],
        StageFailed => Failed [Failed],
    },
    Cache => {
        Cached => Prepared [Prepared],
        Cancel => Cancelling [Interrupt],
        Confirm => Failed [InterruptFailed],
        ConfirmWrong => Failed [InterruptFailed],
        StageFailed => Failed [Failed],
    },
    Cancelling => {
        Checked => Cancelled [Cancelled],
        Configured => Cancelled [Cancelled],
        EvaluatedFits => Cancelled [Cancelled],
        EvaluatedDeferred => Cancelled [Cancelled],
        EvaluatedNoRoom => Cancelled [Cancelled],
        Built => Cancelled [Cancelled],
        BuiltNoRoom => Cancelled [Cancelled],
        Cached => Cancelled [Cancelled],
        StepDone => Cancelled [Cancelled],
        StageFailed => Cancelled [Cancelled],
    },
    Prepared => {
        Confirm => Erase [Erase],
        ConfirmWrong => Failed [Failed],
        Cancel => Cancelled [Cancelled],
    },
    PreparedDeferred => {
        Confirm => EraseDeferred [Erase],
        ConfirmWrong => Failed [Failed],
        Cancel => Cancelled [Cancelled],
    },
    Erase => {
        StepDone => Format [Format],
        StageFailed => Failed [FailedErased],
    },
    EraseDeferred => {
        StepDone => FormatDeferred [Format],
        StageFailed => Failed [FailedErased],
    },
    Format => {
        StepDone => Files [Files],
        StageFailed => Failed [FailedErased],
    },
    FormatDeferred => {
        StepDone => FilesDeferred [Files],
        StageFailed => Failed [FailedErased],
    },
    Files => {
        StepDone => Copy [Copy],
        StageFailed => Failed [FailedErased],
    },
    FilesDeferred => {
        StepDone => BuildOnTarget [BuildOnTarget],
        StageFailed => Failed [FailedErased],
    },
    Copy => {
        StepDone => Bootloader [Bootloader],
        StageFailed => Failed [FailedErased],
    },
    BuildOnTarget => {
        StepDone => Bootloader [Bootloader],
        StageFailed => Failed [FailedErased],
    },
    Bootloader => {
        StepDone => Record [Record],
        StageFailed => Failed [FailedErased],
    },
    Record => {
        StepDone => Flush [Flush],
        StageFailed => Flush [WarnFlush],
    },
    Flush => {
        StepDone => Complete [Complete],
        StageFailed => Failed [FailedErased],
    },
}

use fsm::{Input, Output, State};

/// The table and the data it can't hold, in one struct (so the table's byte
/// packs with the data): `root_mib` from `Checked` (meaningful in
/// `Configuration` to `Build`), `closure_mib` from `Built` (in `Cache`).
#[derive(Clone, Debug, Default)]
pub struct Data {
    fsm: fsm::StateMachine,
    pub root_mib: u64,
    pub closure_mib: u64,
}

pub struct Install {
    data: Data,
    reserve_mib: u64,
}

/// Runs a stage, after its step number (0: none).
fn run(n: u8, stage: Stage, out: &mut impl Sink<Effect>) {
    if n > 0 {
        out.push(Effect::Progress(n));
    }
    out.push(Effect::Run(stage));
}

impl Install {
    /// The table's state.
    pub fn table(&self) -> &fsm::StateMachine {
        &self.data.fsm
    }

    /// The data beside the table.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// The guards: the room checks and the phrase, decided on the data.
    fn input(&self, event: &Event) -> Input {
        let room = |mib: u64| self.data.root_mib >= mib.saturating_add(self.reserve_mib);
        match *event {
            Event::Begin => Input::Begin,
            Event::Checked { .. } => Input::Checked,
            Event::Configured => Input::Configured,
            Event::Evaluated { download_mib, budget_mib, .. } if download_mib <= budget_mib => Input::EvaluatedFits,
            Event::Evaluated { unpacked_mib, .. } if room(unpacked_mib) => Input::EvaluatedDeferred,
            Event::Evaluated { .. } => Input::EvaluatedNoRoom,
            Event::Built { closure_mib } if room(closure_mib) => Input::Built,
            Event::Built { .. } => Input::BuiltNoRoom,
            Event::Cached => Input::Cached,
            Event::StepDone => Input::StepDone,
            Event::StageFailed => Input::StageFailed,
            Event::Confirm { phrase_ok: true } => Input::Confirm,
            Event::Confirm { phrase_ok: false } => Input::ConfirmWrong,
            Event::Cancel => Input::Cancel,
        }
    }

    /// The actions the outputs name. Exhaustive on the output, so a new
    /// output is a compile error; arms that need a payload take it from
    /// their event (the table pairs every output with one kind of event).
    fn act(&mut self, action: Output, event: Event, out: &mut impl Sink<Effect>) {
        match action {
            Output::RunChecks => run(0, Stage::Checks, out),
            Output::RunConfiguration => {
                let Event::Checked { root_mib } = event else { unreachable!() };
                self.data.root_mib = root_mib;
                run(0, Stage::Configuration, out);
            }
            Output::RunEvaluation => run(0, Stage::Evaluation, out),
            Output::RunBuild => run(0, Stage::Build, out),
            Output::RunCache => {
                let Event::Built { closure_mib } = event else { unreachable!() };
                self.data.closure_mib = closure_mib;
                run(0, Stage::Cache, out);
            }
            Output::Prepared => out.push(Effect::Prepared { deferred: false, closure_mib: self.data.closure_mib }),
            Output::PreparedDeferred => out.push(Effect::Prepared { deferred: true, closure_mib: 0 }),
            Output::Refused => {
                let (Event::Evaluated { unpacked_mib: mib, .. } | Event::Built { closure_mib: mib }) = event else {
                    unreachable!()
                };
                let needed_mib = mib.saturating_add(self.reserve_mib);
                out.push(Effect::Refused { needed_mib, root_mib: self.data.root_mib });
            }
            Output::Interrupt => out.push(Effect::Interrupt),
            Output::InterruptFailed => {
                out.push(Effect::Interrupt);
                out.push(Effect::Failed { erased: false });
            }
            Output::Failed => out.push(Effect::Failed { erased: false }),
            Output::Cancelled => out.push(Effect::Cancelled),
            Output::Erase => {
                out.push(Effect::StayAwake);
                run(1, Stage::Erase, out);
            }
            Output::Format => run(2, Stage::Format, out),
            Output::Files => run(3, Stage::Files, out),
            Output::Copy => run(4, Stage::Copy, out),
            Output::BuildOnTarget => run(4, Stage::BuildOnTarget, out),
            Output::Bootloader => run(5, Stage::Bootloader, out),
            Output::Record => run(0, Stage::Record, out),
            Output::Flush => run(6, Stage::Flush, out),
            Output::WarnFlush => {
                out.push(Effect::Warn(Warning::NoRecord));
                run(6, Stage::Flush, out);
            }
            Output::Complete => {
                out.push(Effect::AllowSleep);
                out.push(Effect::Complete);
            }
            Output::FailedErased => {
                out.push(Effect::AllowSleep);
                out.push(Effect::Failed { erased: true });
            }
        }
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { data: Data::default(), reserve_mib: config.reserve_mib }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let input = self.input(&event);
        // `Err`: no row for this (state, input): the spec ignores the event.
        if let Ok(Some(action)) = self.data.fsm.consume(&input) {
            self.act(action, event, out);
        }
    }

    fn state(&self) -> StateName {
        match self.data.fsm.state() {
            State::Idle => StateName::Idle,
            State::Checks => StateName::Checks,
            State::Configuration => StateName::Configuration,
            State::Evaluation => StateName::Evaluation,
            State::Build => StateName::Build,
            State::Cache => StateName::Cache,
            State::Prepared | State::PreparedDeferred => StateName::Prepared,
            State::Cancelling => StateName::Cancelling,
            State::Erase | State::EraseDeferred => StateName::Erase,
            State::Format | State::FormatDeferred => StateName::Format,
            State::Files | State::FilesDeferred => StateName::Files,
            State::Copy => StateName::Copy,
            State::BuildOnTarget => StateName::BuildOnTarget,
            State::Bootloader => StateName::Bootloader,
            State::Record => StateName::Record,
            State::Flush => StateName::Flush,
            State::Complete => StateName::Complete,
            State::Failed => StateName::Failed,
            State::Refused => StateName::Refused,
            State::Cancelled => StateName::Cancelled,
        }
    }
}
