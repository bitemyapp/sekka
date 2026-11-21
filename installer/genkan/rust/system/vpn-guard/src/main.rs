// SPDX-License-Identifier: GPL-3.0-or-later
//! sekka-vpn-guard: blocks the address family a full-tunnel WireGuard
//! connection does not carry, so that it cannot leave by the ordinary
//! network while the tunnel is up.
//!
//! NetworkManager's dispatcher runs it on every network change, and a
//! service runs it once when the system is switched to. Each run looks at
//! where both families' internet traffic goes and brings the guard to what
//! that calls for, so it keeps no state of its own and a run that changes
//! nothing costs a few reads.

#![forbid(unsafe_code)]

mod family;
mod guard;
mod iproute2;
mod system;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

use family::Family;
use iproute2::Iproute2;

const USAGE: &str = "\
Usage: sekka-vpn-guard status
       sekka-vpn-guard reconcile
       sekka-vpn-guard clear

status      say where IPv4 and IPv6 internet traffic goes, and why
reconcile   block or unblock each family as the connections now up call for
clear       unblock both families

NetworkManager's dispatcher runs it as `sekka-vpn-guard INTERFACE EVENT`,
which reconciles when the event can change where traffic goes.";

/// Held while routes are read and changed, so that two runs started by
/// events close together do not work from each other's half-made changes.
const LOCK: &str = "/run/sekka-vpn-guard.lock";

/// NetworkManager's dispatcher events after which traffic may go elsewhere.
const EVENTS: [&str; 8] = [
    "up",
    "down",
    "vpn-up",
    "vpn-down",
    "reapply",
    "dhcp4-change",
    "dhcp6-change",
    "connectivity-change",
];

/// What the program was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Status,
    Reconcile,
    Clear,
    /// A dispatcher event that changes no routes.
    Nothing,
}

impl Request {
    fn parse(arguments: &[String]) -> Option<Request> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["status"] => Some(Request::Status),
            ["reconcile"] => Some(Request::Reconcile),
            ["clear"] => Some(Request::Clear),
            // The dispatcher: an interface, empty for some events, then the
            // event. Events this does not know are left alone, so a newer
            // NetworkManager's are not errors.
            [_interface, event] if EVENTS.contains(event) => Some(Request::Reconcile),
            [_interface, _event] => Some(Request::Nothing),
            _ => None,
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let Some(request) = Request::parse(&arguments) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match run(request) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("sekka-vpn-guard: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Does what was asked, and says whether all of it worked.
fn run(request: Request) -> Result<bool, Box<dyn Error>> {
    let mut system = Iproute2;
    let outcomes = match request {
        Request::Nothing => return Ok(true),
        Request::Status => {
            for line in system::status(&mut system)? {
                say(&line);
            }
            return Ok(true);
        }
        Request::Reconcile => {
            let _held = lock()?;
            system::reconcile(&mut system)?
        }
        Request::Clear => {
            let _held = lock()?;
            system::clear(&mut system)
        }
    };
    let mut worked = true;
    for family in Family::BOTH {
        match outcomes.get(family) {
            Ok(outcome) => {
                if let Some(news) = outcome.news() {
                    say(&news);
                }
            }
            Err(error) => {
                worked = false;
                eprintln!("sekka-vpn-guard: {}: {error}", family.name());
            }
        }
    }
    Ok(worked)
}

/// Prints a line. Whoever was reading may have stopped (`status | head`),
/// which is no reason to fail, so a line that cannot be written is dropped.
fn say(line: &str) {
    let _ = writeln!(io::stdout().lock(), "{line}");
}

fn lock() -> Result<File, Box<dyn Error>> {
    let held = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(LOCK)
        .and_then(|file| file.lock().map(|()| file))
        .map_err(|error| format!("could not take {LOCK} (changing routes takes root): {error}"))?;
    Ok(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(words: &[&str]) -> Option<Request> {
        Request::parse(
            &words
                .iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn commands_are_taken_by_name() {
        assert_eq!(asked(&["status"]), Some(Request::Status));
        assert_eq!(asked(&["reconcile"]), Some(Request::Reconcile));
        assert_eq!(asked(&["clear"]), Some(Request::Clear));
        assert_eq!(asked(&[]), None);
        assert_eq!(asked(&["help"]), None);
        assert_eq!(asked(&["wlo1", "up", "extra"]), None);
    }

    #[test]
    fn dispatcher_events_reconcile_only_when_routes_can_change() {
        assert_eq!(asked(&["GLADOS", "up"]), Some(Request::Reconcile));
        assert_eq!(asked(&["GLADOS", "down"]), Some(Request::Reconcile));
        assert_eq!(asked(&["tun0", "vpn-up"]), Some(Request::Reconcile));
        assert_eq!(asked(&["wlo1", "dhcp6-change"]), Some(Request::Reconcile));
        // NetworkManager passes an empty interface for events about the
        // whole system.
        assert_eq!(
            asked(&["", "connectivity-change"]),
            Some(Request::Reconcile)
        );
        assert_eq!(asked(&["", "hostname"]), Some(Request::Nothing));
        assert_eq!(asked(&["wlo1", "pre-up"]), Some(Request::Nothing));
        assert_eq!(
            asked(&["wlo1", "an-event-from-the-future"]),
            Some(Request::Nothing)
        );
    }
}
