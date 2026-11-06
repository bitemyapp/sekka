// SPDX-License-Identifier: GPL-3.0-or-later
//! Running the engine: read its transcript, journal each stage before it
//! starts, and only then let it start.
//!
//! The engine writes an intent and waits. The helper appends the intent to
//! the journal (durably, see `sekka-mac-journal`) and only then writes the
//! acknowledgement. So whatever the engine changes on disk, the journal
//! already says it was about to — the property the recovery table depends
//! on. If the journal can't be written, no acknowledgement is sent and the
//! engine stops before changing anything.
use sekka_mac_contract::{
    DiskChanged, EngineBody, EngineOutcome, Event, Failure, LogLevel, Progress, Stage, Timestamp,
    Transcript,
};
use sekka_mac_journal::{Journal, JournalError, Storage};

pub use sekka_mac_engine::{ChildEngine, EngineProcess};

/// What the person watching should see, as it happens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observation {
    Stage(Stage),
    Progress(Progress),
    Log(LogLevel, String),
}

#[derive(Debug, thiserror::Error)]
pub enum DriveError {
    /// The engine broke its protocol. Its message is kept for diagnostics.
    #[error("the installer's engine sent something unexpected: {0}")]
    Protocol(String),
    #[error("the installer's engine stopped unexpectedly")]
    Ended,
    /// Cancelled before a stage began; the engine was not acknowledged.
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("the installer's engine couldn't be reached: {0}")]
    Io(String),
}

/// Drive one `execute` run to its end.
///
/// `begin` is asked before each stage is journaled: it returns false if
/// the person cancelled, and otherwise records (under the same lock the
/// cancel request takes) that the stage is starting, so a cancel can't slip
/// in between the decision and the acknowledgement. `refusals` is the
/// stage a refusal before any intent is journaled under (the preparation
/// stage of a full run); none leaves the journal as it is.
pub fn drive<S: Storage>(
    process: &mut dyn EngineProcess,
    transcript: &mut Transcript,
    journal: &mut Journal<S>,
    now: &dyn Fn() -> Timestamp,
    begin: &mut dyn FnMut(Stage) -> bool,
    observe: &mut dyn FnMut(Observation),
    refusals: Option<Stage>,
) -> Result<EngineOutcome, DriveError> {
    let mut started = false;
    loop {
        let Some(line) = process.next_line().map_err(|e| DriveError::Io(e.to_string()))? else {
            return Err(DriveError::Ended);
        };
        let message = transcript
            .accept(&line)
            .map_err(|e| DriveError::Protocol(e.to_string()))?;
        match message.message {
            EngineBody::Hello { .. } | EngineBody::Inspection(_) => {}
            EngineBody::Intent { stage, .. } => {
                if !begin(stage) {
                    return Err(DriveError::Cancelled);
                }
                journal.append(now(), Event::Intent { stage })?;
                started = true;
                observe(Observation::Stage(stage));
                process
                    .ack(message.seq)
                    .map_err(|e| DriveError::Io(e.to_string()))?;
            }
            EngineBody::Checkpoint {
                stage,
                facts,
                verification,
                ..
            } => {
                journal.append(
                    now(),
                    Event::Completed {
                        stage,
                        facts,
                        verification,
                    },
                )?;
            }
            EngineBody::Progress { stage, done, total } => {
                observe(Observation::Progress(Progress { stage, done, total }));
            }
            EngineBody::Log { level, text } => observe(Observation::Log(level, text.to_string())),
            EngineBody::Failure { stage, failure } => {
                observe(Observation::Log(LogLevel::Error, failure.message.to_string()));
                match stage {
                    Some(stage) => {
                        journal.append(now(), Event::Failed { stage, failure })?;
                    }
                    // Refused before any stage began (the engine's own
                    // checks of the plan): nothing could have changed, so it
                    // is the preparation stage that failed.
                    None if !started && refusals.is_some() => {
                        let stage = refusals.expect("checked");
                        journal.append(now(), Event::Intent { stage })?;
                        journal.append(
                            now(),
                            Event::Failed {
                                stage,
                                failure: Failure {
                                    disk_changed: DiskChanged::No,
                                    ..failure
                                },
                            },
                        )?;
                    }
                    // Between stages, after the disk changed: the journal
                    // already shows the installation mid-way, which is the
                    // truth; the message goes to diagnostics.
                    None => {}
                }
            }
            EngineBody::Completion { outcome } => return Ok(outcome),
        }
    }
}
