// SPDX-License-Identifier: GPL-3.0-or-later
use anyhow::{Context, Result, ensure};
use calamares_nixos::{
    RawRequest, Settings, disk,
    install::{self, Event, InstallMode},
    process,
    session::{Confirmation, read_line},
};
use std::{
    io::{BufReader, Read},
    sync::{atomic::Ordering, mpsc},
    thread,
};
use zeroize::Zeroizing;

const REQUEST_LIMIT: usize = 1048576;

fn parse_request(input: &[u8]) -> Result<RawRequest> {
    serde_json::from_slice(input).map_err(|_| anyhow::anyhow!("Invalid installation request"))
}

/// Prepare while the user reviews, then install only after a separate typed
/// confirmation line. Closing stdin before confirming cancels without writes.
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
    let (send, confirmation) = mpsc::channel();
    thread::spawn(move || {
        let line = read_line(&mut stdin, 4096).ok().flatten();
        if line.is_none() {
            process::CANCEL.store(true, Ordering::Relaxed);
        }
        let _ = send.send(line);
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
    let Ok(Some(line)) = confirmation.recv() else {
        install::event(Event::Cancelled);
        return Ok(());
    };
    let confirmation: Confirmation =
        serde_json::from_slice(&line).context("Invalid confirmation message")?;
    install::execute(plan.confirm(&confirmation.confirmation)?, prepared)
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
