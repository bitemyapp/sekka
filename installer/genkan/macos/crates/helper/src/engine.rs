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
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout};

use sekka_mac_contract::{
    Ack, DiskChanged, EngineBody, EngineOutcome, Event, Failure, LogLevel, Progress, Stage,
    Timestamp, Transcript, canonical, limits,
};
use sekka_mac_journal::{Journal, JournalError, Storage};

/// A running engine, however it was started.
pub trait EngineProcess: Send {
    /// The next line of its transcript, without the newline; `None` once it
    /// has closed its output.
    fn next_line(&mut self) -> io::Result<Option<Vec<u8>>>;
    fn ack(&mut self, seq: u64) -> io::Result<()>;
    /// Something another thread can call to ask it to stop. Only used while
    /// no stage that changes the disk is running: disk tools are never
    /// killed mid-operation.
    fn terminator(&self) -> Box<dyn FnMut() + Send>;
    /// Wait for it to exit; whether it exited successfully.
    fn wait(&mut self) -> io::Result<bool>;
}

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
/// in between the decision and the acknowledgement.
pub fn drive<S: Storage>(
    process: &mut dyn EngineProcess,
    transcript: &mut Transcript,
    journal: &mut Journal<S>,
    now: &dyn Fn() -> Timestamp,
    begin: &mut dyn FnMut(Stage) -> bool,
    observe: &mut dyn FnMut(Observation),
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
                    None if !started => {
                        journal.append(now(), Event::Intent { stage: Stage::AppleInputs })?;
                        journal.append(
                            now(),
                            Event::Failed {
                                stage: Stage::AppleInputs,
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

/// An engine started as a child process: transcript on its stdout,
/// acknowledgements on its stdin.
pub struct ChildEngine {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl ChildEngine {
    pub fn new(mut child: Child) -> io::Result<Self> {
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("the engine has no output"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }
}

impl EngineProcess for ChildEngine {
    fn next_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let n = (&mut self.stdout)
            .take(limits::ENGINE_LINE as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line.last() != Some(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a transcript line is too long or unterminated",
            ));
        }
        line.pop();
        Ok(Some(line))
    }
    fn ack(&mut self, seq: u64) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("the engine's input is closed"))?;
        let mut bytes = canonical::to_vec(&Ack { ack: seq }).map_err(io::Error::other)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes)?;
        stdin.flush()
    }
    fn terminator(&self) -> Box<dyn FnMut() + Send> {
        let pid = self.child.id() as libc::pid_t;
        Box::new(move || {
            // SAFETY: signalling our own child by its pid. Only called
            // outside a disk-changing stage, while the child is alive (the
            // run thread clears this before it reaps the child).
            #[allow(unsafe_code)]
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        })
    }
    fn wait(&mut self) -> io::Result<bool> {
        self.stdin = None;
        Ok(self.child.wait()?.success())
    }
}
