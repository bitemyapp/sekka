// SPDX-License-Identifier: GPL-3.0-or-later
//! What the login screen and the helper say to each other: one JSON object
//! a line over the helper's socket, each request answered before the next
//! is read. Requests are bounded and every field is parsed (unknown ones
//! refused), as Genkan's helper session reads its plan; the password
//! arrives only here, read into buffers that are zeroed once parsed. The
//! parser's own (serde_json unescapes a string with `"` or `\` into a
//! scratch buffer of its own) are zeroed as they're freed (memory.rs).
//!
//! | Request | Answer |
//! | --- | --- |
//! | `{"op":"status"}` | `{"status":…}`: whether setup is needed, what it offers, what it suggests |
//! | `{"op":"check","choices":…,"password":…}` | `{"checked":…}`: Genkan's verdict on each field, and suggestions |
//! | `{"op":"complete","choices":…,"password":…}` | `{"progress":…}` lines, then `{"done":…}` or `{"failed":…}` |
//! | `{"op":"retry"}` | the same, finishing a setup that was interrupted |
//!
//! A malformed request gets `{"error":…}` and the connection closes.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The protocol's version, in every status.
pub const VERSION: u32 = 1;
/// The longest request line: a check or completion with its password.
pub const MAX_REQUEST: usize = 16 * 1024;

/// A secret as it arrives: zeroed when dropped.
#[derive(Default)]
pub struct Secret(pub Zeroizing<String>);

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // The string is moved, not copied, into its zeroizing owner.
        Ok(Secret(Zeroizing::new(String::deserialize(d)?)))
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

/// A request, by its `op`. Each kind is a struct of its own, so that a
/// field it doesn't have is refused (serde's tagged enums let them by).
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Request {
    Status(Nothing),
    Check(CheckRequest),
    Complete(CompleteRequest),
    Retry(Nothing),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nothing {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    pub choices: Draft,
    #[serde(default)]
    pub password: Option<Secret>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteRequest {
    pub choices: Draft,
    pub password: Secret,
}

/// The choices as the person has them so far: untrusted text, parsed by
/// Genkan's rules before anything uses it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Draft {
    pub full_name: String,
    pub username: String,
    pub hostname: String,
    pub locale: String,
    pub keyboard: String,
    pub time_zone: String,
    pub desktop: String,
}

/// The fields a check judges, by their names on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    FullName,
    Username,
    Password,
    Hostname,
    Locale,
    Keyboard,
    TimeZone,
    Desktop,
}

impl Field {
    pub const ALL: [Field; 8] = [
        Field::FullName,
        Field::Username,
        Field::Password,
        Field::Hostname,
        Field::Locale,
        Field::Keyboard,
        Field::TimeZone,
        Field::Desktop,
    ];
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Response {
    Status(Status),
    Checked(Checked),
    Progress(Progress),
    Done(Done),
    Failed(Failure),
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Nobody has an account: show setup.
    Needed,
    /// Setup was begun and didn't finish: finish it (`retry`).
    Interrupted,
    /// Setup finished: the login screen as usual.
    Complete,
    /// People have accounts (an installation set up another way): the
    /// login screen as usual, and setup refuses to run.
    NotNeeded,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub version: u32,
    pub state: State,
    /// What to call the computer ("Mac").
    pub device: String,
    pub offer: Offer,
    /// Where to start: macOS's choices where the installer handed them over
    /// and they're offered here, else the defaults.
    pub suggested: Draft,
    /// The setup that was interrupted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<Interrupted>,
}

#[derive(Debug, Serialize)]
pub struct Interrupted {
    pub username: String,
    pub full_name: String,
    /// Its account was made already: the person can log in with it.
    pub account_exists: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct Offer {
    pub locales: Vec<Choice>,
    pub keyboards: Vec<Choice>,
    pub time_zones: Vec<Zone>,
    pub desktops: Vec<DesktopChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Zone {
    pub id: String,
    /// "New York", from the zone's name.
    pub city: String,
    /// "United States", from tzdata's tables.
    pub country: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DesktopChoice {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Checked {
    /// What's wrong with each field that is, in Genkan's words.
    pub problems: BTreeMap<Field, String>,
    /// A user name and a computer name for the full name given.
    pub suggested_username: String,
    pub suggested_hostname: String,
}

/// The steps of completing setup, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Check,
    Account,
    Settings,
    Configuration,
    Verify,
    Finish,
}

impl Step {
    pub const ALL: [Step; 6] = [Step::Check, Step::Account, Step::Settings, Step::Configuration, Step::Verify, Step::Finish];
}

#[derive(Debug, Serialize)]
pub struct Progress {
    pub step: Step,
    pub index: usize,
    pub count: usize,
}

impl Progress {
    pub fn of(step: Step) -> Progress {
        let index = Step::ALL.iter().position(|s| *s == step).unwrap_or(0);
        Progress { step, index, count: Step::ALL.len() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    pub username: String,
    pub full_name: String,
    pub uid: u32,
    pub desktop: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Failure {
    /// What happened and what to do, as a sentence or two.
    pub message: String,
    /// The account exists: the person can log in while setup is retried.
    pub account_exists: bool,
    /// Trying again may help (nothing about the choices was wrong).
    pub can_retry: bool,
}

impl Failure {
    pub fn choices(message: impl Into<String>) -> Failure {
        Failure { message: message.into(), account_exists: false, can_retry: false }
    }
}

/// Read one request line of at most `max` bytes (without its newline), into
/// a buffer zeroed when dropped. `None` at the end of the stream. It reads
/// a byte at a time, so no buffer between the socket and this one keeps a
/// copy of the password; requests are short and few.
pub fn read_line(r: &mut impl Read, max: usize) -> std::io::Result<Option<Zeroizing<Vec<u8>>>> {
    // Room for the longest line up front: growing would leave copies behind.
    let mut line = Zeroizing::new(Vec::with_capacity(max));
    let mut byte = [0u8; 1];
    loop {
        match r.read(&mut byte) {
            Ok(0) if line.is_empty() => return Ok(None),
            Ok(0) => {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the request ended mid-line"));
            }
            Ok(_) if byte[0] == b'\n' => return Ok(Some(line)),
            Ok(_) if line.len() >= max => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("a request longer than {max} bytes"),
                ));
            }
            Ok(_) => line.push(byte[0]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
        byte[0] = 0;
    }
}

/// Parse a request line.
pub fn parse(line: &[u8]) -> Result<Request, String> {
    // Only an object: serde also reads a tagged enum from an array.
    if line.trim_ascii_start().first() != Some(&b'{') {
        return Err("not a request: a request is a JSON object".into());
    }
    serde_json::from_slice(line).map_err(|e| format!("not a request: {e}"))
}

/// Write one answer line.
pub fn write(w: &mut impl Write, response: &Response) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_parsed_strictly() {
        assert!(matches!(parse(br#"{"op":"status"}"#), Ok(Request::Status(_))));
        assert!(matches!(parse(br#"{"op":"retry"}"#), Ok(Request::Retry(_))));
        let Ok(Request::Complete(CompleteRequest { choices, password })) = parse(
            br#"{"op":"complete","choices":{"full_name":"Ada Lovelace","username":"ada","hostname":"analytical","locale":"en_GB.UTF-8","keyboard":"gb","time_zone":"Europe/London","desktop":"aizome"},"password":"correct horse battery"}"#,
        ) else {
            panic!("a completion")
        };
        assert_eq!(choices.username, "ada");
        assert_eq!(password.0.as_str(), "correct horse battery");
        // A check without a password, with fields left out.
        let Ok(Request::Check(CheckRequest { choices, password })) = parse(br#"{"op":"check","choices":{"full_name":"Ada"}}"#)
        else {
            panic!("a check")
        };
        assert_eq!(choices.full_name, "Ada");
        assert!(password.is_none());
        // Unknown operations and fields, and anything not an object, are refused.
        for bad in [
            &br#"{"op":"run","command":"sh"}"#[..],
            br#"{"op":"status","path":"/etc/shadow"}"#,
            br#"{"op":"check","choices":{"home":"/root"}}"#,
            br#"{"op":"complete","choices":{}}"#,
            br#"{"op":"check","choices":{},"shell":"sh"}"#,
            br#"["status"]"#,
            b"status",
        ] {
            assert!(parse(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn lines_are_bounded() {
        let mut ok = &b"{\"op\":\"status\"}\n{\"op\":\"retry\"}\n"[..];
        assert_eq!(&**read_line(&mut ok, 64).unwrap().unwrap(), br#"{"op":"status"}"#);
        assert_eq!(&**read_line(&mut ok, 64).unwrap().unwrap(), br#"{"op":"retry"}"#);
        assert!(read_line(&mut ok, 64).unwrap().is_none());
        let long = vec![b'x'; MAX_REQUEST + 1];
        assert!(read_line(&mut &long[..], MAX_REQUEST).is_err());
        let mut torn = &b"{\"op\":"[..];
        assert!(read_line(&mut torn, 64).is_err());
        // Exactly the limit is accepted.
        let mut exact = vec![b'x'; 64];
        exact.push(b'\n');
        assert_eq!(read_line(&mut &exact[..], 64).unwrap().unwrap().len(), 64);
    }

    #[test]
    fn answers_are_one_line_each() {
        let mut out = Vec::new();
        write(&mut out, &Response::Progress(Progress::of(Step::Account))).unwrap();
        write(&mut out, &Response::Error("no".into())).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], r#"{"progress":{"step":"account","index":1,"count":6}}"#);
        assert_eq!(lines[1], r#"{"error":"no"}"#);
        let checked = Checked {
            problems: [(Field::Username, "Reserved username".to_owned())].into(),
            suggested_username: "ada".into(),
            suggested_hostname: "adas-mac".into(),
        };
        let mut out = Vec::new();
        write(&mut out, &Response::Checked(checked)).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap().trim(),
            r#"{"checked":{"problems":{"username":"Reserved username"},"suggested_username":"ada","suggested_hostname":"adas-mac"}}"#
        );
    }
}
