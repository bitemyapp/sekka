// SPDX-License-Identifier: GPL-3.0-or-later
//! Where an installation stands, read from its records, and what may safely
//! happen next.
//!
//! The table in `docs/apple-silicon-installer-plan.md` ("Journal and
//! recovery contract") is [`InstallState::permitted`]. Records are
//! evidence: they say what was intended and what was verified, and a state
//! derived from them is a starting point for looking at the real disk, never
//! a substitute for it.
use serde::{Deserialize, Serialize};

use crate::{DiskChanged, Error, Event, Record, Stage, invalid};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum InstallState {
    /// A plan was accepted; nothing has been intended yet.
    Planned,
    /// Downloads verified in private staging; still no disk change.
    AssetsVerified,
    /// A stage that changes the disk started and its result isn't recorded.
    MutationStarted { stage: Stage },
    /// A stage failed. `disk_changed` is what the engine could establish.
    Failed {
        stage: Stage,
        disk_changed: DiskChanged,
    },
    /// Everything in macOS succeeded but setting the startup disk; the
    /// installed bytes are verified.
    StartupDiskFailed,
    /// macOS's part is done; the owner continues in Recovery.
    WaitingForRecovery,
    /// Recovery started a policy change and its outcome isn't recorded.
    EnrollmentUncertain,
    /// Recovery finished; Sekka's first start creates the account.
    WaitingForSetup,
    Cancelled,
}

/// What may be offered next, from [`InstallState::permitted`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NextAction {
    /// Inspect again and plan afresh; a changed layout needs a new review.
    Reinspect,
    /// Reuse verified downloads.
    ReuseAssets,
    /// Stop without touching the disk.
    Cancel,
    /// Look at the actual disk; stop for reconciliation if the effect is
    /// ambiguous. Never repeat the stage blindly.
    InspectActualState,
    /// Verify the target and its contents again, then retry only the
    /// startup-disk authorization.
    RetryStartupDisk,
    /// Shut down and finish in Recovery.
    ContinueInRecovery,
    /// In Recovery: read the actual policy and selection, and keep the
    /// matching boot object.
    ReadPolicy,
    /// Start Sekka and create the account.
    StartSekka,
    /// Copy a redacted report for support.
    ExportDiagnostics,
}

impl InstallState {
    pub fn permitted(&self) -> &'static [NextAction] {
        use NextAction::*;
        match self {
            InstallState::Planned => &[Reinspect, Cancel, ExportDiagnostics],
            InstallState::AssetsVerified => &[ReuseAssets, Reinspect, Cancel, ExportDiagnostics],
            InstallState::MutationStarted { .. } => &[InspectActualState, ExportDiagnostics],
            InstallState::Failed {
                disk_changed: DiskChanged::No,
                ..
            } => &[Reinspect, ExportDiagnostics],
            InstallState::Failed { .. } => &[InspectActualState, ExportDiagnostics],
            InstallState::StartupDiskFailed => &[RetryStartupDisk, ExportDiagnostics],
            InstallState::WaitingForRecovery => &[ContinueInRecovery, ExportDiagnostics],
            InstallState::EnrollmentUncertain => &[ReadPolicy, ExportDiagnostics],
            InstallState::WaitingForSetup => &[StartSekka, ExportDiagnostics],
            InstallState::Cancelled => &[Reinspect],
        }
    }

    /// Whether another installation may start while this one stands. Only
    /// a finished, cancelled, or cleanly failed one (nothing changed) lets
    /// another begin; anything else needs reconciling first.
    pub fn allows_new_installation(&self) -> bool {
        matches!(
            self,
            InstallState::Cancelled
                | InstallState::WaitingForSetup
                | InstallState::Failed {
                    disk_changed: DiskChanged::No,
                    ..
                }
        )
    }

    /// The state a chain of records leaves an installation in. The chain
    /// must already be checked (sequence and digests); this reads its
    /// meaning and refuses records that contradict each other.
    pub fn observe(records: &[Record]) -> Result<InstallState, Error> {
        let Some(first) = records.first() else {
            return Err(invalid("an installation has at least its opening record"));
        };
        let Event::Opened { plan, .. } = &first.event else {
            return Err(invalid("the first record must open the installation"));
        };
        let shrinks = plan
            .actions
            .iter()
            .any(|a| matches!(a, crate::Action::ShrinkContainer { .. }));
        let mut state = InstallState::Planned;
        // The stage started and not yet finished, the last one completed,
        // and the last one that failed (which only it may follow, as a
        // retry, and only where retrying is allowed).
        let mut open: Option<Stage> = None;
        let mut last: Option<Stage> = None;
        let mut failed: Option<Stage> = None;
        for record in &records[1..] {
            let at = record.sequence;
            match &record.event {
                Event::Opened { .. } => return Err(invalid("an installation opens once")),
                Event::Cancelled => {
                    if open.is_some()
                        || last.is_some_and(Stage::mutates)
                        || failed.is_some_and(Stage::mutates)
                        || state == InstallState::Cancelled
                    {
                        return Err(invalid(format!(
                            "record {at}: only an installation that changed nothing can be cancelled"
                        )));
                    }
                    state = InstallState::Cancelled;
                }
                Event::Intent { stage } => {
                    if open.is_some() {
                        return Err(invalid(format!(
                            "record {at}: a stage started before the last one finished"
                        )));
                    }
                    let allowed = match (&state, failed) {
                        (InstallState::Cancelled | InstallState::WaitingForSetup, _) => false,
                        (_, Some(f)) => f == *stage && f.retry_allowed(),
                        _ => Some(*stage) == next_stage(last, shrinks),
                    };
                    if !allowed {
                        return Err(invalid(format!("record {at}: {stage:?} can't start here")));
                    }
                    open = Some(*stage);
                    failed = None;
                    if stage.in_recovery() {
                        state = InstallState::EnrollmentUncertain;
                    } else if stage.mutates() {
                        state = InstallState::MutationStarted { stage: *stage };
                    }
                }
                Event::Completed { stage, .. } => {
                    if open != Some(*stage) {
                        return Err(invalid(format!("record {at}: {stage:?} finished without starting")));
                    }
                    open = None;
                    last = Some(*stage);
                    state = match stage {
                        Stage::Import => InstallState::AssetsVerified,
                        Stage::StartupDisk => InstallState::WaitingForRecovery,
                        // The policy changed; the boot object isn't installed
                        // yet. Recovery reads the actual policy before going
                        // on.
                        Stage::Enrollment => InstallState::EnrollmentUncertain,
                        Stage::StartupSelection => InstallState::WaitingForSetup,
                        // Between stages: the disk has changed and the
                        // installation isn't finished.
                        s => InstallState::MutationStarted { stage: *s },
                    };
                }
                Event::Failed { stage, failure } => {
                    if open != Some(*stage) {
                        return Err(invalid(format!("record {at}: {stage:?} failed without starting")));
                    }
                    open = None;
                    failed = Some(*stage);
                    state = if *stage == Stage::StartupDisk {
                        InstallState::StartupDiskFailed
                    } else if stage.in_recovery() {
                        InstallState::EnrollmentUncertain
                    } else {
                        InstallState::Failed {
                            stage: *stage,
                            disk_changed: if stage.mutates() {
                                failure.disk_changed
                            } else {
                                DiskChanged::No
                            },
                        }
                    };
                }
            }
        }
        Ok(state)
    }
}

fn next_stage(last: Option<Stage>, shrinks: bool) -> Option<Stage> {
    let order = Stage::ORDER;
    let next = match last {
        None => Some(Stage::Import),
        Some(s) => order
            .iter()
            .position(|o| *o == s)
            .and_then(|i| order.get(i + 1).copied()),
    };
    match next {
        Some(Stage::Shrink) if !shrinks => Some(Stage::StubPartition),
        n => n,
    }
}

/// Whether a completed stage was the last one macOS's part runs.
pub fn macos_part_complete(state: &InstallState) -> bool {
    matches!(
        state,
        InstallState::WaitingForRecovery
            | InstallState::EnrollmentUncertain
            | InstallState::WaitingForSetup
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Failure, Fact, Plan, RecordSchema, Timestamp, Uuid, Verification};

    fn plan() -> Plan {
        crate::derive_plan(
            &crate::samples::inspection(),
            &crate::samples::catalog(),
            &crate::PlanRequest {
                release: "sekka-j514s-4882dc33".parse().unwrap(),
                place: crate::Place::ShrinkContainer {
                    container: "c54f285b-fe6e-4d0c-9bc6-567c62bd7c20".parse().unwrap(),
                },
                sekka: 100 * crate::GIB,
            },
            0,
        )
        .unwrap()
    }

    struct Chain(Vec<Record>);
    impl Chain {
        fn new() -> Self {
            let p = plan();
            let d = p.digest().unwrap();
            let mut c = Chain(Vec::new());
            c.push(Event::Opened {
                plan: Box::new(p),
                plan_digest: d,
                app: "0.1.0".parse().unwrap(),
                helper: "0.1.0".parse().unwrap(),
            });
            c
        }
        fn push(&mut self, event: Event) -> &mut Self {
            let previous = self.0.last().map(|r| r.digest().unwrap());
            self.0.push(Record {
                schema: RecordSchema,
                installation: Uuid::from_bytes([7; 16]),
                sequence: self.0.len() as u64 + 1,
                previous,
                at: Timestamp::from_unix(1_800_000_000),
                event,
            });
            self
        }
        fn stage(&mut self, stage: Stage) -> &mut Self {
            self.push(Event::Intent { stage });
            self.push(Event::Completed {
                stage,
                facts: vec![Fact::new("ok", "yes").unwrap()],
                verification: Verification::Inspected,
            })
        }
        fn fail(&mut self, stage: Stage, changed: DiskChanged) -> &mut Self {
            self.push(Event::Intent { stage });
            self.push(Event::Failed {
                stage,
                failure: Failure {
                    code: "resize-failed".parse().unwrap(),
                    message: "Apple's resize failed.".parse().unwrap(),
                    apple_diagnostic: Some("-69606".parse().unwrap()),
                    disk_changed: changed,
                },
            })
        }
        fn state(&self) -> Result<InstallState, Error> {
            for pair in self.0.windows(2) {
                pair[0].check_successor(&pair[1])?;
            }
            InstallState::observe(&self.0)
        }
    }

    const MACOS: [Stage; 11] = [
        Stage::Import,
        Stage::AppleInputs,
        Stage::Shrink,
        Stage::StubPartition,
        Stage::StubSystem,
        Stage::Partitions,
        Stage::Firmware,
        Stage::Esp,
        Stage::Root,
        Stage::Recovery,
        Stage::StartupDisk,
    ];

    #[test]
    fn a_full_installation_reaches_setup() {
        let mut c = Chain::new();
        assert_eq!(c.state().unwrap(), InstallState::Planned);
        c.stage(Stage::Import);
        assert_eq!(c.state().unwrap(), InstallState::AssetsVerified);
        for s in &MACOS[1..] {
            c.stage(*s);
        }
        assert_eq!(c.state().unwrap(), InstallState::WaitingForRecovery);
        c.stage(Stage::Enrollment);
        assert_eq!(c.state().unwrap(), InstallState::EnrollmentUncertain);
        c.stage(Stage::StartupSelection);
        assert_eq!(c.state().unwrap(), InstallState::WaitingForSetup);
        assert!(c.state().unwrap().allows_new_installation());
    }

    #[test]
    fn an_interrupted_stage_is_never_replayed_blindly() {
        let mut c = Chain::new();
        c.stage(Stage::Import)
            .stage(Stage::AppleInputs)
            .push(Event::Intent { stage: Stage::Shrink });
        let s = c.state().unwrap();
        assert_eq!(s, InstallState::MutationStarted { stage: Stage::Shrink });
        assert_eq!(s.permitted(), &[NextAction::InspectActualState, NextAction::ExportDiagnostics]);
        assert!(!s.allows_new_installation());
    }

    #[test]
    fn a_resize_that_changed_nothing_allows_a_new_plan() {
        let mut c = Chain::new();
        c.stage(Stage::Import).stage(Stage::AppleInputs).fail(Stage::Shrink, DiskChanged::No);
        let s = c.state().unwrap();
        assert!(s.permitted().contains(&NextAction::Reinspect));
        assert!(s.allows_new_installation());
        let mut c = Chain::new();
        c.stage(Stage::Import).stage(Stage::AppleInputs).fail(Stage::Shrink, DiskChanged::Unknown);
        assert!(!c.state().unwrap().allows_new_installation());
        // Shrinking is never retried within the same installation.
        c.push(Event::Intent { stage: Stage::Shrink });
        assert!(c.state().is_err());
    }

    #[test]
    fn only_the_startup_disk_is_retried_after_the_payload_is_verified() {
        let mut c = Chain::new();
        for s in &MACOS[..10] {
            c.stage(*s);
        }
        c.fail(Stage::StartupDisk, DiskChanged::No);
        assert_eq!(c.state().unwrap(), InstallState::StartupDiskFailed);
        assert_eq!(c.state().unwrap().permitted()[0], NextAction::RetryStartupDisk);
        c.stage(Stage::StartupDisk);
        assert_eq!(c.state().unwrap(), InstallState::WaitingForRecovery);
        let mut bad = Chain::new();
        for s in &MACOS[..10] {
            bad.stage(*s);
        }
        bad.fail(Stage::StartupDisk, DiskChanged::No).push(Event::Intent { stage: Stage::Root });
        assert!(bad.state().is_err());
    }

    #[test]
    fn stages_out_of_order_or_after_cancelling_are_refused() {
        let mut c = Chain::new();
        c.stage(Stage::Import).push(Event::Intent { stage: Stage::Root });
        assert!(c.state().is_err());
        let mut c = Chain::new();
        c.stage(Stage::Import).push(Event::Cancelled);
        assert_eq!(c.state().unwrap(), InstallState::Cancelled);
        c.push(Event::Intent { stage: Stage::Shrink });
        assert!(c.state().is_err());
        let mut c = Chain::new();
        c.stage(Stage::Import)
            .stage(Stage::AppleInputs)
            .stage(Stage::Shrink)
            .push(Event::Cancelled);
        assert!(c.state().is_err(), "a changed disk can't be cancelled away");
    }

    #[test]
    fn a_tampered_record_breaks_the_chain() {
        let mut c = Chain::new();
        c.stage(Stage::Import).stage(Stage::AppleInputs);
        c.0[2].at = Timestamp::from_unix(1);
        assert!(c.state().is_err());
    }
}
