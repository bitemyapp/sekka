//! Small types that say what a number or a string means.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A tool call's id: the one the model's API gave it, or one Tasuke made
/// for a call the model wrote as text.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallId(String);

impl CallId {
    pub fn new(id: impl Into<String>) -> CallId {
        CallId(id.into())
    }

    /// A fresh id, for calls written as text (and calls the API sent without one).
    pub(crate) fn generate() -> CallId {
        CallId(format!("call_{}", random_hex(4)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A conversation's id, which is also its file's stem: letters, digits and `-`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    /// An id read from outside (a command line, a file name), if it is one.
    pub fn parse(s: &str) -> Option<SessionId> {
        let ok = !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        ok.then(|| SessionId(s.to_owned()))
    }

    /// A new id: the creation time (sortable) and a random suffix.
    pub(crate) fn generate(now: Timestamp) -> SessionId {
        SessionId(format!("{}-{}", now.0, random_hex(3)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A memory note's id, short enough for a model to repeat (`m7`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteId(String);

impl NoteId {
    pub fn new(id: impl Into<String>) -> NoteId {
        NoteId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which user turn of a conversation this is, from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnNumber(pub u32);

impl TurnNumber {
    pub fn next(self) -> TurnNumber {
        TurnNumber(self.0 + 1)
    }
}

impl fmt::Display for TurnNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How many tool calls one user turn may make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepBudget(pub u32);

impl Default for StepBudget {
    fn default() -> StepBudget {
        StepBudget(40)
    }
}

/// A numbered box on a marked screenshot (set-of-marks), from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MarkId(pub u32);

impl fmt::Display for MarkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A number of tokens: a context size, a prompt, an answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tokens(pub u64);

impl std::ops::Add for Tokens {
    type Output = Tokens;
    fn add(self, o: Tokens) -> Tokens {
        Tokens(self.0 + o.0)
    }
}

impl std::ops::AddAssign for Tokens {
    fn add_assign(&mut self, o: Tokens) {
        self.0 += o.0;
    }
}

impl fmt::Display for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Seconds since the Unix epoch, UTC.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub fn now() -> Timestamp {
        Timestamp(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs())
    }
}

/// `n` random bytes as hex, from the kernel (or the clock if that fails).
pub(crate) fn random_hex(n: usize) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    let filled = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_ok();
    if !filled {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).subsec_nanos();
        let mixed = nanos ^ std::process::id().rotate_left(16);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (mixed >> ((i % 4) * 8)) as u8;
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_file_stems() {
        assert!(SessionId::parse("1730000000-ab12cd").is_some());
        assert!(SessionId::parse("../etc").is_none());
        assert!(SessionId::parse("a/b").is_none());
        assert!(SessionId::parse("").is_none());
        assert!(SessionId::parse(&"x".repeat(65)).is_none());
        let id = SessionId::generate(Timestamp(1_730_000_000));
        assert!(SessionId::parse(id.as_str()).is_some());
        assert!(id.as_str().starts_with("1730000000-"));
    }

    #[test]
    fn random_hex_has_the_length_asked() {
        assert_eq!(random_hex(4).len(), 8);
        assert_ne!(random_hex(8), random_hex(8));
    }
}
