// SPDX-License-Identifier: GPL-3.0-or-later
//! The GUI side of the helper's `session` protocol.
//!
//! Requests are newline-delimited JSON on the helper's stdin: first the
//! reviewed plan (without a confirmation), later at most one confirmation.
//! The helper prepares the installation as soon as it receives the plan and
//! reports `install::Event`s on stdout. Closing stdin before confirming
//! cancels preparation; nothing has been written to any disk at that point.
use crate::{InstallPlan, install::Event};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, Sender},
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
pub fn read_line(reader: &mut impl BufRead, limit: usize) -> Result<Option<Zeroizing<Vec<u8>>>> {
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

pub enum Update {
    Event(Event),
    /// The helper exited: `Ok` when its exit status was success.
    Finished(Result<(), String>),
}

/// A running helper. Dropping it without confirming cancels preparation.
pub struct Session {
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
                if let Ok(event) = serde_json::from_str(&line) {
                    on_update(Update::Event(event));
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
        Ok(Self { writer })
    }
    /// Send the typed phrase. The helper checks it against its own parsed
    /// plan; destructive work starts only if it matches.
    pub fn confirm(&self, phrase: &str) {
        let _ = self.writer.send(Some(phrase.to_owned()));
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
}
