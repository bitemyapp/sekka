// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper's `session` protocol, both ends.
//!
//! Requests are newline-delimited JSON on the helper's stdin: first the
//! reviewed plan (without a confirmation), later at most one confirmation.
//! The helper prepares the installation as soon as it receives the plan and
//! reports `install::Event`s on stdout. Closing stdin before confirming
//! cancels preparation; nothing has been written to any disk at that point.
//!
//! The confirmation is accepted only for a finished preparation (GEN-06):
//! the GUI can send one only with the proof that the helper reported
//! `Prepared` ([`Ready`]), and the helper reads it only after reporting that
//! ([`Awaiting`]). A line before then is a protocol error that cancels
//! preparation, rather than a confirmation saved for when it finishes.
use crate::{
    ConfirmedInstall, InstallPlan, RawRequest,
    install::{self, Event, Prepared, Summary},
    process::{self, Latch},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirmation {
    pub confirmation: String,
}

/// Read one newline-terminated line of at most `limit` bytes. `None` at a
/// clean end of input; a partial final line or oversized line is an error.
pub(crate) fn read_line(
    reader: &mut impl BufRead,
    limit: usize,
) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let mut line = Zeroizing::new(Vec::new());
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            ensure!(line.is_empty(), "Truncated request");
            return Ok(None);
        }
        if let Some(end) = buffer.iter().position(|b| *b == b'\n') {
            line.extend_from_slice(&buffer[..end]);
            reader.consume(end + 1);
            ensure!(line.len() <= limit, "Request too large");
            return Ok(Some(line));
        }
        let length = buffer.len();
        line.extend_from_slice(buffer);
        reader.consume(length);
        ensure!(line.len() <= limit, "Request too large");
    }
}

// ---- The helper's end ------------------------------------------------------

/// The largest request line.
pub const REQUEST_LIMIT: usize = 1048576;
/// The largest confirmation line.
const CONFIRMATION_LIMIT: usize = 4096;

/// What the stdin watcher has seen, and so what a line from the GUI means.
const PREPARING: u8 = 0;
const AWAITING: u8 = 1;
/// The GUI closed its end, or sent something other than a line, while the
/// helper prepared.
const WITHDRAWN: u8 = 2;
/// The GUI sent a line before the helper reported `Prepared`.
const EARLY: u8 = 3;

/// A session whose helper is preparing. It watches stdin meanwhile: closing
/// it cancels preparation, and so does a line, since no confirmation is
/// accepted before the helper reports `Prepared`. Only
/// [`prepared`](Self::prepared) leads to the confirmation, so it can't be
/// taken while preparation runs:
///
/// ```compile_fail,E0599
/// use calamares_nixos::{InstallPlan, session::Preparing};
/// fn early(preparing: Preparing, plan: InstallPlan) -> anyhow::Result<()> {
///     let _confirmed = preparing.confirm(plan)?;
///     Ok(())
/// }
/// ```
///
/// ```
/// use calamares_nixos::{InstallPlan, install::Prepared, session::Preparing};
/// fn after(preparing: Preparing, prepared: &Prepared, plan: InstallPlan) -> anyhow::Result<()> {
///     if let Some(awaiting) = preparing.prepared(prepared)? {
///         let _confirmed = awaiting.confirm(plan)?;
///     }
///     Ok(())
/// }
/// ```
pub struct Preparing {
    phase: Arc<AtomicU8>,
    heard: Receiver<Option<Zeroizing<Vec<u8>>>>,
}

impl Preparing {
    /// Reads the request, then watches `input` for the GUI withdrawing it.
    pub fn begin(input: impl BufRead + Send + 'static) -> Result<(RawRequest, Self)> {
        Self::begin_with(input, &process::LATCH)
    }

    fn begin_with(
        mut input: impl BufRead + Send + 'static,
        latch: &'static Latch,
    ) -> Result<(RawRequest, Self)> {
        let line = read_line(&mut input, REQUEST_LIMIT)?.context("Missing installation request")?;
        let request: RawRequest = serde_json::from_slice(&line)
            .map_err(|_| anyhow::anyhow!("Invalid installation request"))?;
        drop(line);
        ensure!(
            request.confirmation.is_empty(),
            "A preparation request must not carry a confirmation"
        );
        let phase = Arc::new(AtomicU8::new(PREPARING));
        let (tell, heard) = mpsc::channel();
        let watching = phase.clone();
        thread::spawn(move || watch(input, &watching, latch, &tell));
        Ok((request, Self { phase, heard }))
    }

    /// Preparation failed. A withdrawal it stopped for is reported as
    /// `Cancelled`; any other failure, even one after a withdrawal, as the
    /// failure it is.
    pub fn failed(self, error: anyhow::Error) -> Result<()> {
        match self.phase.load(Ordering::Acquire) {
            WITHDRAWN if process::is_cancellation(&error) => {
                install::event(Event::Cancelled);
                Ok(())
            }
            EARLY => Err(early()),
            _ => Err(error),
        }
    }

    /// Preparation finished: reports `Prepared` and accepts the confirmation
    /// from now on. `None` when the GUI withdrew meanwhile (reported as
    /// `Cancelled`).
    pub fn prepared(self, prepared: &Prepared) -> Result<Option<Awaiting>> {
        // Before the event: the GUI answers it, and the watcher must take
        // that answer as the confirmation.
        match self
            .phase
            .compare_exchange(PREPARING, AWAITING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                install::event(Event::Prepared {
                    summary: prepared.summary().clone(),
                });
                Ok(Some(Awaiting { heard: self.heard }))
            }
            Err(EARLY) => Err(early()),
            Err(_) => {
                install::event(Event::Cancelled);
                Ok(None)
            }
        }
    }
}

fn early() -> anyhow::Error {
    anyhow::anyhow!(
        "The installer sent a confirmation before preparation finished; no disk writes were made"
    )
}

/// The stdin watcher: one line or the end of input, and what it means in
/// the phase it comes in.
fn watch(
    mut input: impl BufRead,
    phase: &AtomicU8,
    latch: &Latch,
    tell: &Sender<Option<Zeroizing<Vec<u8>>>>,
) {
    let line = read_line(&mut input, CONFIRMATION_LIMIT).ok().flatten();
    let stopped = if line.is_some() { EARLY } else { WITHDRAWN };
    if phase
        .compare_exchange(PREPARING, stopped, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        latch.cancel();
    } else {
        // Awaiting: the confirmation, or the GUI withdrawing.
        let _ = tell.send(line);
    }
}

/// A prepared session, waiting for the person's confirmation.
pub struct Awaiting {
    heard: Receiver<Option<Zeroizing<Vec<u8>>>>,
}

impl Awaiting {
    /// Confirms `plan` with the phrase the GUI sends, or `None` when the GUI
    /// withdrew instead (reported as `Cancelled`). Stdin isn't read after
    /// it. A session confirms its plan only here, once the helper has
    /// reported `Prepared`: the helper's old session, which read the
    /// confirmation on a thread while preparing and confirmed the plan
    /// itself with whatever line came, so that an early confirmation was
    /// acted on the moment preparation ended (GEN-06), doesn't compile:
    ///
    /// ```compile_fail,E0624
    /// use calamares_nixos::{InstallPlan, install, session::Confirmation};
    /// use std::{io::BufRead, sync::mpsc, thread};
    /// fn session(plan: InstallPlan) -> anyhow::Result<()> {
    ///     let (send, confirmation) = mpsc::channel();
    ///     thread::spawn(move || {
    ///         let mut line = String::new();
    ///         let read = std::io::stdin().lock().read_line(&mut line).unwrap_or(0);
    ///         let _ = send.send((read > 0).then_some(line));
    ///     });
    ///     let prepared = install::prepare(&plan)?;
    ///     install::event(install::Event::Prepared { summary: prepared.summary().clone() });
    ///     let Ok(Some(line)) = confirmation.recv() else { return Ok(()) };
    ///     let confirmation: Confirmation = serde_json::from_str(&line)?;
    ///     install::execute(plan.confirm(&confirmation.confirmation)?, prepared)
    /// }
    /// ```
    ///
    /// ```
    /// use calamares_nixos::{InstallPlan, install, session::Preparing};
    /// fn session(preparing: Preparing, plan: InstallPlan) -> anyhow::Result<()> {
    ///     let prepared = install::prepare(&plan)?;
    ///     let Some(awaiting) = preparing.prepared(&prepared)? else { return Ok(()) };
    ///     let Some(confirmed) = awaiting.confirm(plan)? else { return Ok(()) };
    ///     install::execute(confirmed, prepared)
    /// }
    /// ```
    pub fn confirm(self, plan: InstallPlan) -> Result<Option<ConfirmedInstall>> {
        match self.heard.recv() {
            Ok(Some(line)) => {
                let confirmation: Confirmation =
                    serde_json::from_slice(&line).context("Invalid confirmation message")?;
                Ok(Some(plan.confirm(&confirmation.confirmation)?))
            }
            _ => {
                install::event(Event::Cancelled);
                Ok(None)
            }
        }
    }
}

// ---- The GUI's end ---------------------------------------------------------

pub enum Update {
    Event(Event),
    /// The helper prepared the installation and waits for the confirmation:
    /// the proof [`Session::confirm`] takes.
    Prepared(Ready),
    /// The helper exited: `Ok` when its exit status was success.
    Finished(Result<(), String>),
}

/// Proof that a session's helper reported `Prepared`. Only the session's
/// reader makes one, from the helper's own event, so the GUI can't send the
/// confirmation while the helper still prepares:
///
/// ```compile_fail,E0451
/// use calamares_nixos::{install::Summary, session::{Ready, Session}};
/// fn early(session: &Session) -> anyhow::Result<()> {
///     let ready = Ready { session: 0, summary: Summary::default() };
///     session.confirm(&ready, "ERASE /dev/sdb")
/// }
/// ```
///
/// ```
/// use calamares_nixos::session::{Ready, Session};
/// fn prepared(session: &Session, ready: &Ready) -> anyhow::Result<()> {
///     session.confirm(ready, "ERASE /dev/sdb")
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Ready {
    session: u64,
    summary: Summary,
}

impl Ready {
    /// What preparation established.
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
}

/// Numbers the sessions, so one's [`Ready`] can't confirm another.
static SESSIONS: AtomicU64 = AtomicU64::new(0);

/// A running helper. Dropping it without confirming cancels preparation.
pub struct Session {
    id: u64,
    writer: Sender<Option<String>>,
}
impl Session {
    /// Start the privileged helper through Polkit and send the plan. Events
    /// arrive on a worker thread; never call GTK from `on_update`.
    pub fn start(
        plan: InstallPlan,
        mut on_update: impl FnMut(Update) + Send + 'static,
    ) -> Result<Self> {
        let helper = std::env::current_exe()?
            .parent()
            .context("Installer location")?
            .join("calamares-nixos-helper");
        let mut child = Command::new(option_env!("CALAMARES_PKEXEC").unwrap_or("pkexec"))
            .arg(helper)
            .arg("session")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdin = child.stdin.take().context("Helper stdin")?;
        let stdout = child.stdout.take().context("Helper stdout")?;
        let mut request = Zeroizing::new(serde_json::to_vec(&plan.into_request())?);
        request.push(b'\n');
        let id = SESSIONS.fetch_add(1, Ordering::Relaxed) + 1;
        let (writer, confirmations) = mpsc::channel::<Option<String>>();
        // Writes can block until Polkit authorizes and the helper reads; keep
        // them off the caller's thread.
        thread::spawn(move || {
            if stdin.write_all(&request).is_err() {
                return;
            }
            drop(request);
            if let Ok(Some(phrase)) = confirmations.recv()
                && let Ok(mut line) = serde_json::to_vec(&Confirmation {
                    confirmation: phrase,
                })
            {
                line.push(b'\n');
                let _ = stdin.write_all(&line);
            }
            // Dropping stdin is the cancellation signal before confirmation
            // and is ignored by the helper afterwards.
        });
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                match serde_json::from_str(&line) {
                    Ok(Event::Prepared { summary }) => on_update(Update::Prepared(Ready {
                        session: id,
                        summary,
                    })),
                    Ok(event) => on_update(Update::Event(event)),
                    Err(_) => {}
                }
            }
            on_update(Update::Finished(match child.wait() {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!(
                    "The installation helper stopped (authorization canceled or helper failed, {status}). Do not assume the disk is unchanged if installation had started."
                )),
                Err(error) => Err(error.to_string()),
            }));
        });
        Ok(Self { id, writer })
    }
    /// Send the typed phrase, once the helper has prepared. The helper
    /// checks it against its own parsed plan; destructive work starts only
    /// if it matches.
    pub fn confirm(&self, ready: &Ready, phrase: &str) -> Result<()> {
        ensure!(
            ready.session == self.id,
            "That preparation belongs to another installation helper"
        );
        self.writer
            .send(Some(phrase.to_owned()))
            .map_err(|_| anyhow::anyhow!("The installation helper is no longer running"))
    }
    /// Withdraw an unconfirmed request. Has no effect after confirming.
    pub fn cancel(&self) {
        let _ = self.writer.send(None);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn lines_are_bounded_and_terminated() {
        let mut input = std::io::Cursor::new(b"{\"a\":1}\nsecond\n".to_vec());
        assert_eq!(
            read_line(&mut input, 64).unwrap().unwrap().as_slice(),
            b"{\"a\":1}"
        );
        assert_eq!(
            read_line(&mut input, 64).unwrap().unwrap().as_slice(),
            b"second"
        );
        assert!(read_line(&mut input, 64).unwrap().is_none());
        assert!(read_line(&mut std::io::Cursor::new(b"partial".to_vec()), 64).is_err());
        assert!(read_line(&mut std::io::Cursor::new(vec![b'x'; 100]), 64).is_err());
        assert!(
            read_line(
                &mut std::io::Cursor::new([vec![b'x'; 100], vec![b'\n']].concat()),
                64
            )
            .is_err()
        );
    }

    /// A request line as the GUI sends one, followed by `rest`.
    fn input(rest: &str) -> Cursor<Vec<u8>> {
        let (_dir, _settings, mut raw) = crate::plan_tests::fixture();
        raw.confirmation.clear();
        let mut line = serde_json::to_vec(&raw).unwrap();
        line.push(b'\n');
        line.extend_from_slice(rest.as_bytes());
        Cursor::new(line)
    }

    fn leaked() -> &'static Latch {
        Box::leak(Box::new(Latch::new()))
    }

    /// Waits for the watcher to have read what it was given.
    fn settled(preparing: &Preparing) -> u8 {
        for _ in 0..500 {
            let phase = preparing.phase.load(Ordering::Acquire);
            if phase != PREPARING {
                return phase;
            }
            thread::sleep(std::time::Duration::from_millis(2));
        }
        PREPARING
    }

    #[test]
    fn a_line_before_prepared_is_not_kept_as_the_confirmation() {
        let latch = leaked();
        let (_, preparing) =
            Preparing::begin_with(input("{\"confirmation\":\"ERASE /dev/vda\"}\n"), latch).unwrap();
        assert_eq!(settled(&preparing), EARLY);
        // It stopped preparation, and nothing can commit to installing now.
        assert!(latch.cancelled());
        assert!(latch.commit().is_err());
        let error = preparing.failed(process::Cancelled.into()).unwrap_err();
        assert!(format!("{error:#}").contains("before preparation finished"));
    }

    #[test]
    fn closing_stdin_while_preparing_cancels_and_reports_only_cancellations_so() {
        let latch = leaked();
        let (_, preparing) = Preparing::begin_with(input(""), latch).unwrap();
        assert_eq!(settled(&preparing), WITHDRAWN);
        assert!(latch.cancelled());
        // A failure that isn't the cancellation is still that failure.
        let error = anyhow::anyhow!("nix failed (exit status: 1)").context("Could not build");
        assert!(preparing.failed(error).is_err());
        let (_, preparing) = Preparing::begin_with(input(""), leaked()).unwrap();
        settled(&preparing);
        let cancelled = anyhow::Error::from(process::Cancelled).context("Could not build");
        assert!(preparing.failed(cancelled).is_ok());
    }

    #[test]
    fn a_committed_installation_cannot_be_cancelled() {
        let latch = Latch::new();
        latch.commit().unwrap();
        assert!(!latch.cancel());
        assert!(!latch.cancelled());
        latch.commit().unwrap();
    }

    #[test]
    fn a_request_carrying_a_confirmation_is_refused() {
        let (_dir, _settings, mut raw) = crate::plan_tests::fixture();
        raw.confirmation = "ERASE /dev/vda".into();
        let mut line = serde_json::to_vec(&raw).unwrap();
        line.push(b'\n');
        assert!(Preparing::begin_with(Cursor::new(line), leaked()).is_err());
    }
}
