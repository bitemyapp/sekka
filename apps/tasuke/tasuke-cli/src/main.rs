// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tasuke`: the assistant's headless interface. See apps/tasuke/DESIGN.md
//! ("CLI").
//!
//! Subcommands live in their own modules: models.rs (hardware, recommend,
//! models, download, remove, use, server), ask.rs (ask) and checkup.rs
//! (checkup).

use std::process::ExitCode;

mod accounts;
mod args;
mod ask;
mod bench;
mod checkup;
mod models;
mod table;

const USAGE: &str = "\
usage: tasuke COMMAND [options]

  hardware [--json]                    what this machine has for running models
  recommend [--role ROLE] [--json]     the models to use here, best first
                                       (ROLE: assistant, desktop or fast)
  models [--json]                      the catalog, with fit and download state
  download ID                          download a model (progress on stderr)
  remove ID                            delete a downloaded model
  use ID                               make it Tasuke's model
  use anthropic/ID | openai/ID         use a cloud model through your own account
  account [list] [--json]              the Claude and OpenAI accounts connected
  account add anthropic|openai         connect one: paste an API key, checked and
                                       kept in the desktop's keyring
  account remove anthropic|openai      disconnect it
  bench [ID|anthropic/ID|openai/ID] [--json]
                                       time a model here (default: Tasuke's own)
  server start [--foreground]          start the model server (the user unit
                                       tasuke-llm.service, or here and now)
  server stop                          stop it
  server status [--json]               whether it runs, and what it serves
  server exec                          become the model server (the unit runs this)
  ask [--act] [--desktop] [--approve never|ask|all]
      [--base-url URL --model NAME] \"request\"
                                       ask Tasuke from the terminal
  checkup [--notify]                   look over the system's health, changing nothing
";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = argv.split_first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let code = match command.as_str() {
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            0
        }
        "ask" => ask::run(rest),
        "checkup" => checkup::run(rest),
        "hardware" | "recommend" | "models" | "download" | "remove" | "use" | "server" | "account" | "bench" => {
            let result = match command.as_str() {
                "account" => accounts::run(rest.to_vec()),
                "bench" => bench::run(rest.to_vec()),
                _ => models::run(command, rest.to_vec()),
            };
            match result {
                Ok(()) => 0,
                Err(models::Failure::Usage(e)) => {
                    eprintln!("tasuke {command}: {e}");
                    eprint!("{USAGE}");
                    2
                }
                Err(models::Failure::Failed(e)) => {
                    eprintln!("tasuke {command}: {e}");
                    1
                }
            }
        }
        other => {
            eprintln!("tasuke: unknown command {other:?}");
            eprint!("{USAGE}");
            2
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
