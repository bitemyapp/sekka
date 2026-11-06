// SPDX-License-Identifier: GPL-3.0-or-later
//! Read an engine transcript on standard input and check it with
//! [`Transcript`], line by line, as the helper does. The engine's tests pipe
//! their transcripts through it.
//!
//! ```sh
//! cargo run -q -p sekka-mac-contract --example validate_transcript -- inspect < transcript
//! cargo run -q -p sekka-mac-contract --example validate_transcript -- execute sha256:… shrinks < transcript
//! ```
//!
//! Prints the outcome and exits 0 when the whole transcript is accepted;
//! prints the first refusal and exits 1 otherwise.
use std::io::BufRead;

use sekka_mac_contract::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut transcript = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["inspect"] => Transcript::inspect(),
        ["execute", plan, shrinks] => {
            let plan: Digest = plan.parse().expect("a plan digest");
            Transcript::execute(plan, *shrinks == "shrinks")
        }
        _ => {
            eprintln!("usage: validate_transcript inspect | execute PLAN_DIGEST shrinks|no-shrink");
            std::process::exit(2);
        }
    };
    let stdin = std::io::stdin();
    for (n, line) in stdin.lock().split(b'\n').enumerate() {
        let line = line.expect("standard input");
        if let Err(e) = transcript.accept(&line) {
            println!("refused line {}: {e}", n + 1);
            std::process::exit(1);
        }
    }
    match transcript.outcome() {
        Some(outcome) => println!("accepted: {outcome:?}"),
        None => {
            println!("refused: the transcript ended without a completion");
            std::process::exit(1);
        }
    }
}
