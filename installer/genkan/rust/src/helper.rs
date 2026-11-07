// SPDX-License-Identifier: GPL-3.0-or-later
use anyhow::{Context, Result, ensure};
use calamares_nixos::{
    RawRequest, Settings, disk,
    install::{self, Event, InstallMode},
    process, retry,
    session::{Confirmation, read_line},
};
use std::{
    io::{BufReader, Read},
    sync::{
        atomic::Ordering,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
};
use zeroize::Zeroizing;

const REQUEST_LIMIT: usize = 1048576;

fn parse_request(input: &[u8]) -> Result<RawRequest> {
    serde_json::from_slice(input).map_err(|_| anyhow::anyhow!("Invalid installation request"))
}

/// The window's lines after the plan; `None` once it closed stdin.
type Lines = Receiver<Option<Zeroizing<Vec<u8>>>>;

/// The window, answering a retry offer with its next line.
struct Window(Lines);
impl retry::Answers for Window {
    fn discard_stale(&mut self) -> bool {
        loop {
            match self.0.try_recv() {
                // Sent before the offer existed: not an answer to it.
                Ok(Some(_)) => continue,
                Ok(None) | Err(TryRecvError::Disconnected) => return false,
                Err(TryRecvError::Empty) => return true,
            }
        }
    }
    fn next(&mut self) -> Option<Vec<u8>> {
        self.0.recv().ok().flatten().map(|line| line.to_vec())
    }
}

/// Prepare while the user reviews, then install only after a separate typed
/// confirmation line. Closing stdin before confirming cancels without writes;
/// afterwards it declines any offer to try the bootloader again.
fn session() -> Result<()> {
    let mut stdin = BufReader::new(std::io::stdin());
    let line = read_line(&mut stdin, REQUEST_LIMIT)?.context("Missing installation request")?;
    let request = parse_request(&line)?;
    drop(line);
    ensure!(
        request.confirmation.is_empty(),
        "A preparation request must not carry a confirmation"
    );
    let plan = request.parse(&Settings::load()?)?;
    let (send, lines) = mpsc::channel();
    thread::spawn(move || {
        let line = read_line(&mut stdin, 4096).ok().flatten();
        let confirmed = line.is_some();
        if !confirmed {
            process::CANCEL.store(true, Ordering::Relaxed);
        }
        if send.send(line).is_err() || !confirmed {
            return;
        }
        // Answers to retry offers. The end of input never cancels: by now
        // the disk may be changing.
        loop {
            let line = read_line(&mut stdin, 4096).ok().flatten();
            let end = line.is_none();
            if send.send(line).is_err() || end {
                return;
            }
        }
    });
    let prepared = match install::prepare(&plan) {
        Ok(prepared) => prepared,
        Err(_) if process::cancelled() => {
            install::event(Event::Cancelled);
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    install::event(Event::Prepared {
        summary: prepared.summary().clone(),
    });
    let Ok(Some(line)) = lines.recv() else {
        prepared.cancel();
        install::event(Event::Cancelled);
        return Ok(());
    };
    let confirmed = serde_json::from_slice::<Confirmation>(&line)
        .context("Invalid confirmation message")
        .and_then(|confirmation| plan.confirm(&confirmation.confirmation));
    match confirmed {
        Ok(confirmed) => install::execute(confirmed, prepared, &mut Window(lines)),
        Err(error) => Err(prepared.refuse(error)),
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["discover"] {
        println!("{}", serde_json::to_string(&disk::discover()?)?);
    } else if args == ["session"] {
        session()?;
    } else if args == ["install"] || args == ["preflight"] {
        let mut input = Zeroizing::new(Vec::new());
        std::io::stdin()
            .take(REQUEST_LIMIT as u64 + 1)
            .read_to_end(&mut input)?;
        ensure!(input.len() <= REQUEST_LIMIT, "Request too large");
        let request = parse_request(&input)?;
        drop(input);
        let confirmed = request.parse_confirmed(&Settings::load()?)?;
        let mode = if args == ["preflight"] {
            InstallMode::Preflight
        } else {
            InstallMode::Execute
        };
        install::install(confirmed, mode)?;
    } else {
        anyhow::bail!(
            "Usage: calamares-nixos-helper discover|session|preflight|install; requests are JSON on stdin"
        );
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        install::event(Event::Failed {
            message: format!("{error:#}"),
        });
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamares_nixos::retry::Answers;
    fn line(text: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
        Some(Zeroizing::new(text.to_vec()))
    }
    #[test]
    fn only_lines_sent_after_an_offer_answer_it() {
        let (send, lines) = mpsc::channel();
        let mut window = Window(lines);
        send.send(line(b"sent while installing")).unwrap();
        assert!(window.discard_stale());
        send.send(line(b"the answer")).unwrap();
        assert_eq!(window.next().unwrap(), b"the answer");
        // The end of input declines, whether or not an offer was made.
        send.send(None).unwrap();
        assert!(!window.discard_stale());
        let (send, lines) = mpsc::channel();
        let mut window = Window(lines);
        send.send(None).unwrap();
        assert!(window.next().is_none());
        drop(send);
        assert!(window.next().is_none());
        assert!(!window.discard_stale());
    }
}
