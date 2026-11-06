// SPDX-License-Identifier: GPL-3.0-or-later
//! The one channel between Sekka Installer and its helper: a launchd mach
//! service, `computer.sekka.installer.helper`, carrying one request and one
//! reply at a time.
//!
//! A message is an XPC dictionary with exactly these keys:
//!
//! - `request` (data): a canonical `sekka.mac.helper-request/1` document;
//! - `owner-password` (data, optional): the owner's password, only for the
//!   operations that need it — never inside the document;
//! - `response` (data, replies only): a `sekka.mac.helper-response/1`.
//!
//! Each side checks the other's code signature before reading anything:
//! the helper sets a peer code-signing requirement on every incoming
//! connection, and the app sets one on its connection to the helper, so
//! neither talks to an impostor. The requirement strings are built into
//! each binary; nothing at run time can widen them.
#![cfg_attr(not(target_os = "macos"), forbid(unsafe_code))]

pub const REQUEST_KEY: &str = "request";
pub const PASSWORD_KEY: &str = "owner-password";
pub const RESPONSE_KEY: &str = "response";
/// The largest request or response accepted.
pub const MESSAGE_LIMIT: usize = 1024 * 1024;

/// Who sent a request, as the kernel reports it for the connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub uid: u32,
    pub pid: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum XpcError {
    #[error("Sekka Installer's helper isn't running or isn't allowed yet")]
    Unavailable,
    #[error("the helper's signature isn't Sekka's")]
    Untrusted,
    #[error("the helper's reply couldn't be read")]
    BadReply,
}

/// The code-signing requirement for this build's peer. Release builds name
/// the team that signs Sekka; development builds (ad hoc signed) can only
/// name the identifier, which proves much less, so they say so.
pub fn requirement(identifier: &str, team: Option<&str>) -> String {
    match team {
        Some(team) => format!(
            "identifier \"{identifier}\" and anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
        ),
        None => format!("identifier \"{identifier}\""),
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{Client, listen};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_requirements_name_the_team() {
        assert_eq!(
            requirement("computer.sekka.installer", Some("ABCDE12345")),
            "identifier \"computer.sekka.installer\" and anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\""
        );
        assert_eq!(
            requirement("computer.sekka.installer.helper", None),
            "identifier \"computer.sekka.installer.helper\""
        );
    }
}
