//! How the window talks to its reader, `aizome-nisshi-helper`: a request
//! per line of JSON on the helper's standard input, and on its output a
//! line per answer, `ID KIND PAYLOAD`:
//!
//! | Kind | Payload |
//! |---|---|
//! | `r` | ready: [`Ready`] as JSON (id 0, once, first) |
//! | `e` | a journal entry, as journalctl's JSON line |
//! | `b` | the boots, as `journalctl --list-boots`'s JSON |
//! | `f` | a log file: [`FileInfo`] as JSON |
//! | `t` | a line of a log file |
//! | `d` | the request is done |
//! | `x` | it failed: why, in words |
//!
//! journalctl's JSON has no raw newlines and a file's lines are split at
//! them, so a line is always one answer.

use serde::{Deserialize, Serialize};

use crate::query::Query;

/// The version of this protocol: the window and the helper come from one
/// package, but a session can outlive an update.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub op: Op,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Entries, with the fields the table uses.
    Query(Query),
    /// One entry with all its fields.
    Entry {
        cursor: String,
    },
    Boots,
    /// The log files under /var/log.
    Files,
    /// The end of a log file under /var/log: at most `max` bytes.
    Read {
        path: String,
        max: u64,
    },
    /// Stop request `id` (a follow, or a slow read).
    Cancel,
}

/// What the helper says first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ready {
    pub version: u32,
    /// Whom it reads as.
    pub uid: u32,
    /// Whether it can read the system's journal (root, or a member of
    /// `wheel`, `adm` or `systemd-journal`), not just the user's own.
    pub system: bool,
    pub journalctl: String,
}

/// A log file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    /// µs since the epoch.
    pub modified: i64,
    /// Whether the reader can read it.
    pub readable: bool,
}

/// A boot, as `journalctl --list-boots --output=json` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BootInfo {
    pub index: i32,
    pub boot_id: String,
    pub first_entry: i64,
    pub last_entry: i64,
}

/// An answer, as the window reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame<'a> {
    Ready(&'a str),
    Entry(u64, &'a str),
    Boots(u64, &'a str),
    File(u64, &'a str),
    Text(u64, &'a str),
    Done(u64),
    Failed(u64, &'a str),
}

impl<'a> Frame<'a> {
    pub fn parse(line: &'a str) -> Option<Frame<'a>> {
        let (id, rest) = line.split_once(' ')?;
        let id: u64 = id.parse().ok()?;
        let (kind, payload) = rest.split_once(' ').unwrap_or((rest, ""));
        Some(match kind {
            "r" => Frame::Ready(payload),
            "e" => Frame::Entry(id, payload),
            "b" => Frame::Boots(id, payload),
            "f" => Frame::File(id, payload),
            "t" => Frame::Text(id, payload),
            "d" => Frame::Done(id),
            "x" => Frame::Failed(id, payload),
            _ => return None,
        })
    }
}

/// A line as the helper writes it.
pub fn line(id: u64, kind: char, payload: &str) -> String {
    // A payload is one line: a failure's words may not be.
    let payload = payload.replace(['\n', '\r'], " ");
    format!("{id} {kind} {payload}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        assert_eq!(Frame::parse(line(7, 'e', "{\"a\":1}").trim_end()), Some(Frame::Entry(7, "{\"a\":1}")));
        assert_eq!(Frame::parse("7 e {\"a\":1}"), Some(Frame::Entry(7, "{\"a\":1}")));
        assert_eq!(Frame::parse("3 d "), Some(Frame::Done(3)));
        assert_eq!(Frame::parse("3 d"), Some(Frame::Done(3)));
        assert_eq!(Frame::parse("4 x no such boot"), Some(Frame::Failed(4, "no such boot")));
        assert_eq!(Frame::parse("4 t "), Some(Frame::Text(4, "")));
        assert_eq!(Frame::parse("nonsense"), None);
        assert_eq!(line(1, 'x', "two\nlines"), "1 x two lines\n");
        let r = Request { id: 2, op: Op::Read { path: "/var/log/boot.log".into(), max: 10 } };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), r);
    }
}
