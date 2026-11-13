//! `install` as an enum: each stage a variant, carrying what later stages need.

use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Checks,
    Configuration { root: u64 },
    Evaluation { root: u64 },
    Build { root: u64 },
    Cache { closure: u64 },
    Prepared { deferred: bool },
    Cancelling,
    Erase { deferred: bool },
    Format { deferred: bool },
    Files { deferred: bool },
    Copy,
    BuildOnTarget,
    Bootloader,
    Record,
    Flush,
    Complete,
    Failed,
    Refused,
    Cancelled,
}

impl State {
    fn preparing(self) -> bool {
        matches!(
            self,
            State::Checks
                | State::Configuration { .. }
                | State::Evaluation { .. }
                | State::Build { .. }
                | State::Cache { .. }
        )
    }
    fn executing(self) -> bool {
        matches!(
            self,
            State::Erase { .. }
                | State::Format { .. }
                | State::Files { .. }
                | State::Copy
                | State::BuildOnTarget
                | State::Bootloader
                | State::Record
                | State::Flush
        )
    }
}

fn is_result(e: Event) -> bool {
    matches!(
        e,
        Event::Checked { .. }
            | Event::Configured
            | Event::Evaluated { .. }
            | Event::Built { .. }
            | Event::Cached
            | Event::StepDone
            | Event::StageFailed
    )
}

/// The machine. Its fields are private: the state changes only through
/// `Machine::handle`; [`Install::typed`] reads it.
pub struct Install {
    config: Config,
    state: State,
}

impl Install {
    /// The current state, read-only.
    pub fn typed(&self) -> &State {
        &self.state
    }
}

impl Install {
    fn run(&mut self, stage: Stage, next: State, out: &mut impl Sink<Effect>) {
        out.push(Effect::Run(stage));
        self.state = next;
    }
    fn step(&mut self, n: u8, stage: Stage, next: State, out: &mut impl Sink<Effect>) {
        out.push(Effect::Progress(n));
        self.run(stage, next, out);
    }
    fn refuse(&mut self, needed_mib: u64, root_mib: u64, out: &mut impl Sink<Effect>) {
        out.push(Effect::Refused { needed_mib, root_mib });
        self.state = State::Refused;
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Self { config, state: State::Idle }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        let reserve = self.config.reserve_mib;
        let s = self.state;
        match (s, event) {
            (State::Idle, Event::Begin) => self.run(Stage::Checks, State::Checks, out),
            (State::Idle | State::Prepared { .. }, Event::Cancel) => {
                out.push(Effect::Cancelled);
                self.state = State::Cancelled;
            }
            (_, Event::Cancel) if s.preparing() => {
                out.push(Effect::Interrupt);
                self.state = State::Cancelling;
            }
            (State::Idle, Event::Confirm { .. }) => {
                out.push(Effect::Failed { erased: false });
                self.state = State::Failed;
            }
            (_, Event::Confirm { .. }) if s.preparing() => {
                out.push(Effect::Interrupt);
                out.push(Effect::Failed { erased: false });
                self.state = State::Failed;
            }
            (State::Prepared { deferred }, Event::Confirm { phrase_ok }) => {
                if phrase_ok {
                    out.push(Effect::StayAwake);
                    self.step(1, Stage::Erase, State::Erase { deferred }, out);
                } else {
                    out.push(Effect::Failed { erased: false });
                    self.state = State::Failed;
                }
            }
            (State::Cancelling, e) if is_result(e) => {
                out.push(Effect::Cancelled);
                self.state = State::Cancelled;
            }
            (_, Event::StageFailed) if s.preparing() => {
                out.push(Effect::Failed { erased: false });
                self.state = State::Failed;
            }
            (State::Checks, Event::Checked { root_mib }) => {
                self.run(Stage::Configuration, State::Configuration { root: root_mib }, out)
            }
            (State::Configuration { root }, Event::Configured) => {
                self.run(Stage::Evaluation, State::Evaluation { root }, out)
            }
            (State::Evaluation { root }, Event::Evaluated { download_mib, unpacked_mib, budget_mib }) => {
                if download_mib > budget_mib {
                    let needed = unpacked_mib.saturating_add(reserve);
                    if root < needed {
                        self.refuse(needed, root, out);
                    } else {
                        out.push(Effect::Prepared { deferred: true, closure_mib: 0 });
                        self.state = State::Prepared { deferred: true };
                    }
                } else {
                    self.run(Stage::Build, State::Build { root }, out);
                }
            }
            (State::Build { root }, Event::Built { closure_mib }) => {
                let needed = closure_mib.saturating_add(reserve);
                if root < needed {
                    self.refuse(needed, root, out);
                } else {
                    self.run(Stage::Cache, State::Cache { closure: closure_mib }, out);
                }
            }
            (State::Cache { closure }, Event::Cached) => {
                out.push(Effect::Prepared { deferred: false, closure_mib: closure });
                self.state = State::Prepared { deferred: false };
            }
            (State::Erase { deferred }, Event::StepDone) => {
                self.step(2, Stage::Format, State::Format { deferred }, out)
            }
            (State::Format { deferred }, Event::StepDone) => self.step(3, Stage::Files, State::Files { deferred }, out),
            (State::Files { deferred: true }, Event::StepDone) => {
                self.step(4, Stage::BuildOnTarget, State::BuildOnTarget, out)
            }
            (State::Files { deferred: false }, Event::StepDone) => self.step(4, Stage::Copy, State::Copy, out),
            (State::Copy | State::BuildOnTarget, Event::StepDone) => {
                self.step(5, Stage::Bootloader, State::Bootloader, out)
            }
            (State::Bootloader, Event::StepDone) => self.run(Stage::Record, State::Record, out),
            (State::Record, Event::StepDone) => self.step(6, Stage::Flush, State::Flush, out),
            (State::Record, Event::StageFailed) => {
                out.push(Effect::Warn(Warning::NoRecord));
                self.step(6, Stage::Flush, State::Flush, out);
            }
            (State::Flush, Event::StepDone) => {
                out.push(Effect::AllowSleep);
                out.push(Effect::Complete);
                self.state = State::Complete;
            }
            (_, Event::StageFailed) if s.executing() => {
                out.push(Effect::AllowSleep);
                out.push(Effect::Failed { erased: true });
                self.state = State::Failed;
            }
            _ => {}
        }
    }

    fn state(&self) -> StateName {
        match self.state {
            State::Idle => StateName::Idle,
            State::Checks => StateName::Checks,
            State::Configuration { .. } => StateName::Configuration,
            State::Evaluation { .. } => StateName::Evaluation,
            State::Build { .. } => StateName::Build,
            State::Cache { .. } => StateName::Cache,
            State::Prepared { .. } => StateName::Prepared,
            State::Cancelling => StateName::Cancelling,
            State::Erase { .. } => StateName::Erase,
            State::Format { .. } => StateName::Format,
            State::Files { .. } => StateName::Files,
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
