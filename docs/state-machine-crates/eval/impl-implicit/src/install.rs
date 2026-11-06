//! `install` in `prepare`/`execute`'s shape (install.rs:325-841): the
//! stage running, a `deferred` bool choosing the route, the `Summary`'s
//! sizes, the `CANCEL` latch, a confirmation flag and the bare step
//! integer that `progress(n)` reports.

use crate::BUGS;
use spec::Sink;
use spec::install::{Config, Effect, Event, Machine, Stage, StateName, Warning};

pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Complete,
    Failed,
    Refused,
    Cancelled,
}

/// The machine's fields, in the current code's shape. Readable through
/// the machine (`Deref`); only `handle` changes them.
pub struct Fields {
    pub config: Config,
    /// What runs now.
    pub stage: Option<Stage>,
    pub root_bytes: u64,
    pub closure_bytes: u64,
    pub deferred: bool,
    /// `Prepared` exists and waits for the confirmation.
    pub prepared: bool,
    /// The `CANCEL` latch.
    pub cancel: bool,
    pub confirmed: bool,
    pub step: u8,
    pub end: Option<End>,
}

impl Fields {
    fn run(&mut self, stage: Stage, out: &mut impl Sink<Effect>) {
        self.stage = Some(stage);
        out.push(Effect::Run(stage));
    }
    fn progress(&mut self, n: u8, stage: Stage, out: &mut impl Sink<Effect>) {
        self.step = n;
        out.push(Effect::Progress(n));
        self.run(stage, out);
    }
    fn finish(&mut self, end: End, effect: Effect, out: &mut impl Sink<Effect>) {
        if self.confirmed && matches!(end, End::Complete | End::Failed) {
            out.push(Effect::AllowSleep);
        }
        out.push(effect);
        self.stage = None;
        self.prepared = false;
        self.end = Some(end);
    }
    fn preparing(&self) -> bool {
        matches!(
            self.stage,
            Some(Stage::Checks | Stage::Configuration | Stage::Evaluation | Stage::Build | Stage::Cache)
        )
    }
    fn refuse(&mut self, needed_mib: u64, out: &mut impl Sink<Effect>) {
        let root_mib = self.root_bytes;
        self.finish(End::Refused, Effect::Refused { needed_mib, root_mib }, out);
    }
}

impl Fields {
    fn new(config: Config) -> Self {
        Self {
            config,
            stage: None,
            root_bytes: 0,
            closure_bytes: 0,
            deferred: false,
            prepared: false,
            cancel: false,
            confirmed: false,
            step: 0,
            end: None,
        }
    }

    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        if self.end.is_some() {
            return;
        }
        let begun = self.stage.is_some() || self.prepared;
        match event {
            Event::Begin => {
                if !begun {
                    self.run(Stage::Checks, out);
                }
            }
            Event::Cancel => {
                if !begun || self.prepared {
                    self.finish(End::Cancelled, Effect::Cancelled, out);
                } else if self.preparing() && !self.cancel {
                    self.cancel = true;
                    out.push(Effect::Interrupt);
                }
            }
            Event::Confirm { phrase_ok } => {
                if self.cancel {
                    return;
                }
                if !begun {
                    self.finish(End::Failed, Effect::Failed { erased: false }, out);
                } else if self.preparing() {
                    // SPEC-FIX: refused (the helper queues it and executes the
                    // moment preparation ends).
                    out.push(Effect::Interrupt);
                    self.finish(End::Failed, Effect::Failed { erased: false }, out);
                } else if self.prepared {
                    self.prepared = false;
                    if phrase_ok {
                        self.confirmed = true;
                        out.push(Effect::StayAwake);
                        self.progress(1, Stage::Erase, out);
                    } else {
                        self.finish(End::Failed, Effect::Failed { erased: false }, out);
                    }
                }
            }
            Event::StageFailed => {
                if self.cancel && self.preparing() {
                    // An error after CANCEL is reported as a cancellation.
                    self.finish(End::Cancelled, Effect::Cancelled, out);
                } else if self.preparing() {
                    self.finish(End::Failed, Effect::Failed { erased: false }, out);
                } else if self.stage == Some(Stage::Record) {
                    // SPEC-FIX: the system is installed; the record is extra.
                    out.push(Effect::Warn(Warning::NoRecord));
                    self.progress(6, Stage::Flush, out);
                } else if self.stage.is_some() {
                    self.finish(End::Failed, Effect::Failed { erased: true }, out);
                }
            }
            e => {
                if self.cancel {
                    // The interrupted stage's result, whatever it is.
                    self.finish(End::Cancelled, Effect::Cancelled, out);
                    return;
                }
                let expected = match (self.stage, e) {
                    (Some(Stage::Checks), Event::Checked { .. })
                    | (Some(Stage::Configuration), Event::Configured)
                    | (Some(Stage::Evaluation), Event::Evaluated { .. })
                    | (Some(Stage::Build), Event::Built { .. })
                    | (Some(Stage::Cache), Event::Cached) => true,
                    (Some(s), Event::StepDone) => !matches!(
                        s,
                        Stage::Checks | Stage::Configuration | Stage::Evaluation | Stage::Build | Stage::Cache
                    ),
                    _ => false,
                };
                if !expected {
                    return;
                }
                let reserve = self.config.reserve_mib;
                match e {
                    Event::Checked { root_mib } => {
                        self.root_bytes = root_mib;
                        self.run(Stage::Configuration, out);
                    }
                    Event::Configured => self.run(Stage::Evaluation, out),
                    Event::Evaluated { download_mib, unpacked_mib, budget_mib } => {
                        self.deferred = download_mib > budget_mib;
                        if self.deferred {
                            // BUG(GEN-07): the deferred route skips the root-space
                            // guard, which runs only after an in-memory build.
                            let needed = unpacked_mib.saturating_add(reserve);
                            if !BUGS && self.root_bytes < needed {
                                return self.refuse(needed, out);
                            }
                            self.stage = None;
                            self.prepared = true;
                            out.push(Effect::Prepared { deferred: true, closure_mib: self.closure_bytes });
                        } else {
                            self.run(Stage::Build, out);
                        }
                    }
                    Event::Built { closure_mib } => {
                        self.closure_bytes = closure_mib;
                        let needed = closure_mib.saturating_add(reserve);
                        if self.root_bytes < needed {
                            return self.refuse(needed, out);
                        }
                        self.run(Stage::Cache, out);
                    }
                    Event::Cached => {
                        self.stage = None;
                        self.prepared = true;
                        out.push(Effect::Prepared { deferred: false, closure_mib: self.closure_bytes });
                    }
                    Event::StepDone => match self.stage {
                        Some(Stage::Erase) => self.progress(2, Stage::Format, out),
                        Some(Stage::Format) => self.progress(3, Stage::Files, out),
                        Some(Stage::Files) => {
                            let s = if self.deferred { Stage::BuildOnTarget } else { Stage::Copy };
                            self.progress(4, s, out);
                        }
                        Some(Stage::Copy | Stage::BuildOnTarget) => self.progress(5, Stage::Bootloader, out),
                        Some(Stage::Bootloader) => self.run(Stage::Record, out),
                        // SPEC-FIX: `progress(6)` once (the current code emits it twice).
                        Some(Stage::Record) => self.progress(6, Stage::Flush, out),
                        Some(Stage::Flush) => self.finish(End::Complete, Effect::Complete, out),
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    fn state(&self) -> StateName {
        match self.end {
            Some(End::Complete) => return StateName::Complete,
            Some(End::Failed) => return StateName::Failed,
            Some(End::Refused) => return StateName::Refused,
            Some(End::Cancelled) => return StateName::Cancelled,
            None => {}
        }
        if self.prepared {
            return StateName::Prepared;
        }
        if self.cancel {
            return StateName::Cancelling;
        }
        match self.stage {
            None => StateName::Idle,
            Some(Stage::Checks) => StateName::Checks,
            Some(Stage::Configuration) => StateName::Configuration,
            Some(Stage::Evaluation) => StateName::Evaluation,
            Some(Stage::Build) => StateName::Build,
            Some(Stage::Cache) => StateName::Cache,
            Some(Stage::Erase) => StateName::Erase,
            Some(Stage::Format) => StateName::Format,
            Some(Stage::Files) => StateName::Files,
            Some(Stage::Copy) => StateName::Copy,
            Some(Stage::BuildOnTarget) => StateName::BuildOnTarget,
            Some(Stage::Bootloader) => StateName::Bootloader,
            Some(Stage::Record) => StateName::Record,
            Some(Stage::Flush) => StateName::Flush,
        }
    }
}

/// The machine: [`Fields`] read through `Deref`, never written from outside.
pub struct Install(Fields);

impl std::ops::Deref for Install {
    type Target = Fields;
    fn deref(&self) -> &Fields {
        &self.0
    }
}

impl Machine for Install {
    fn new(config: Config) -> Self {
        Install(Fields::new(config))
    }
    fn handle(&mut self, event: Event, out: &mut impl Sink<Effect>) {
        self.0.handle(event, out);
    }
    fn state(&self) -> StateName {
        self.0.state()
    }
}
