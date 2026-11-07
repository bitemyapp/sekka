// SPDX-License-Identifier: GPL-3.0-or-later
//! `sekka-setup-helper`: started by `sekka-setup.socket` (`serve`), or run
//! by root to see where setup stands (`status`), or by the build to list
//! what setup offers (`offered`).

use std::path::PathBuf;
use std::process::ExitCode;

use sekka_setup_helper::config::{self, Config};
use sekka_setup_helper::system::Host;
use sekka_setup_helper::{protocol, server};

const USAGE: &str = "\
usage: sekka-setup-helper [--config PATH] serve | status | offered

  serve     answer the login screen on the socket systemd passed
  status    print where setup stands (as root)
  offered   print the locales and keyboard layouts setup offers
";

fn main() -> ExitCode {
    let mut path = PathBuf::from(config::PATH);
    let mut command = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => match args.next() {
                Some(p) => path = p.into(),
                None => return usage(),
            },
            "serve" | "status" | "offered" if command.is_none() => command = Some(a),
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ => return usage(),
        }
    }
    let result = match command.as_deref() {
        Some("offered") => {
            let offered = serde_json::json!({
                "locales": calamares_nixos::LOCALES,
                "keyboards": calamares_nixos::KEYBOARDS,
            });
            println!("{offered}");
            Ok(())
        }
        Some("serve") => serve(&path),
        Some("status") => status(&path),
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sekka-setup-helper: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn serve(path: &std::path::Path) -> anyhow::Result<()> {
    // The password passes through this process: never a core of it
    // (sekka-setup.service also sets LimitCORE=0).
    // Safety: prctl with an integer option and argument.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) } != 0 {
        eprintln!("sekka-setup-helper: making the helper undumpable: {}", std::io::Error::last_os_error());
    }
    let listener = server::activated()?;
    let config = Config::load(path)?;
    let greeter = server::uid_of(&Host::new(&config), &config.greeter_user)?;
    server::serve(&listener, &config, || Host::new(&config), greeter, server::IDLE_SERVER)
}

fn status(path: &std::path::Path) -> anyhow::Result<()> {
    let config = Config::load(path)?;
    let host = Host::new(&config);
    let hints = sekka_setup_helper::handoff::read(
        &config.handoff,
        &std::fs::read_to_string("/proc/cmdline").unwrap_or_default(),
        &config.zoneinfo,
    );
    let status = server::status(&config, &host, &hints)?;
    let mut out = std::io::stdout();
    protocol::write(&mut out, &protocol::Response::Status(status))?;
    Ok(())
}
