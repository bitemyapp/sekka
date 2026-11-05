// SPDX-License-Identifier: GPL-3.0-or-later
use anyhow::{Result, ensure};
use calamares_nixos::{
    RawRequest, Settings, disk,
    install::{self, Event, InstallMode},
    session::{Preparing, REQUEST_LIMIT},
};
use std::io::{BufReader, Read};
use zeroize::Zeroizing;

fn parse_request(input: &[u8]) -> Result<RawRequest> {
    serde_json::from_slice(input).map_err(|_| anyhow::anyhow!("Invalid installation request"))
}

/// Prepare while the user reviews, then install only after a separate typed
/// confirmation line, which is read only once preparation has finished.
/// Closing stdin before confirming cancels without writes.
fn session() -> Result<()> {
    let (request, preparing) = Preparing::begin(BufReader::new(std::io::stdin()))?;
    let plan = request.parse(&Settings::load()?)?;
    let prepared = match install::prepare(&plan) {
        Ok(prepared) => prepared,
        Err(error) => return preparing.failed(error),
    };
    let Some(awaiting) = preparing.prepared(&prepared)? else {
        return Ok(());
    };
    let Some(confirmed) = awaiting.confirm(plan)? else {
        return Ok(());
    };
    install::execute(confirmed, prepared)
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
