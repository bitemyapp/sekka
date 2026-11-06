//! Domain units, so that a byte count, a context length and a speed can't be
//! mixed up, and so that every number carries its meaning in its type.

use std::fmt;
use std::ops::{Add, AddAssign, Mul, Sub};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A quantity of memory or disk, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bytes(pub u64);

impl Bytes {
    pub const ZERO: Bytes = Bytes(0);
    pub const KIB: u64 = 1024;
    pub const MIB: u64 = 1024 * 1024;
    pub const GIB: u64 = 1024 * 1024 * 1024;

    pub const fn kib(n: u64) -> Bytes {
        Bytes(n * Self::KIB)
    }

    pub const fn mib(n: u64) -> Bytes {
        Bytes(n * Self::MIB)
    }

    pub const fn gib(n: u64) -> Bytes {
        Bytes(n * Self::GIB)
    }

    /// Fractional GiB, rounded down to a byte (for limits written as `1.5`).
    pub fn gib_f64(n: f64) -> Bytes {
        Bytes((n * Self::GIB as f64).max(0.0) as u64)
    }

    pub fn as_gib(self) -> f64 {
        self.0 as f64 / Self::GIB as f64
    }

    pub fn as_mib(self) -> f64 {
        self.0 as f64 / Self::MIB as f64
    }

    pub fn saturating_sub(self, other: Bytes) -> Bytes {
        Bytes(self.0.saturating_sub(other.0))
    }

    /// This amount scaled by a fraction (e.g. the 92% fit margin).
    pub fn scale(self, fraction: f64) -> Bytes {
        Bytes((self.0 as f64 * fraction).max(0.0) as u64)
    }
}

impl Add for Bytes {
    type Output = Bytes;
    fn add(self, rhs: Bytes) -> Bytes {
        Bytes(self.0.saturating_add(rhs.0))
    }
}

impl AddAssign for Bytes {
    fn add_assign(&mut self, rhs: Bytes) {
        *self = *self + rhs;
    }
}

impl Sub for Bytes {
    type Output = Bytes;
    /// Saturating: memory budgets never go negative.
    fn sub(self, rhs: Bytes) -> Bytes {
        self.saturating_sub(rhs)
    }
}

impl std::iter::Sum for Bytes {
    fn sum<I: Iterator<Item = Bytes>>(iter: I) -> Bytes {
        iter.fold(Bytes::ZERO, Add::add)
    }
}

/// Binary units, one decimal: `18.9 GiB`, `512 MiB`.
impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.0;
        if b >= Self::GIB {
            write!(f, "{:.1} GiB", self.as_gib())
        } else if b >= Self::MIB {
            write!(f, "{:.0} MiB", self.as_mib())
        } else if b >= Self::KIB {
            write!(f, "{:.0} KiB", b as f64 / Self::KIB as f64)
        } else {
            write!(f, "{b} B")
        }
    }
}

/// A transfer or memory rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BytesPerSecond(pub f64);

impl BytesPerSecond {
    /// How long `remaining` takes at this rate; `None` while the rate is unknown.
    pub fn time_for(self, remaining: Bytes) -> Option<Duration> {
        (self.0 > 0.0).then(|| Duration::from_secs_f64(remaining.0 as f64 / self.0))
    }

    pub fn gb_per_s(gb: f64) -> BytesPerSecond {
        BytesPerSecond(gb * 1e9)
    }
}

impl fmt::Display for BytesPerSecond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/s", Bytes(self.0.max(0.0) as u64))
    }
}

/// A number of tokens of model context (the KV cache's length).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContextTokens(pub u32);

impl ContextTokens {
    pub const K: u32 = 1024;

    pub const fn k(n: u32) -> ContextTokens {
        ContextTokens(n * Self::K)
    }
}

impl fmt::Display for ContextTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 >= Self::K && self.0.is_multiple_of(Self::K) {
            write!(f, "{}K", self.0 / Self::K)
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// KV-cache bytes for each token of context (the cost of context, per token).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BytesPerToken(pub u64);

impl Mul<ContextTokens> for BytesPerToken {
    type Output = Bytes;
    fn mul(self, ctx: ContextTokens) -> Bytes {
        Bytes(self.0.saturating_mul(u64::from(ctx.0)))
    }
}

/// Decode or prefill speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TokensPerSecond(pub f32);

impl fmt::Display for TokensPerSecond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0} tok/s", self.0)
    }
}

/// A count of model parameters, in billions as model cards give them.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BillionParams(pub f32);

impl fmt::Display for BillionParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.fract() == 0.0 { write!(f, "{:.0}B", self.0) } else { write!(f, "{}B", self.0) }
    }
}

/// A catalog model's identifier: `gemma-4-31b`. Also its llama-server preset
/// name, its download directory and the OpenAI `model` field.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn new(s: impl Into<String>) -> ModelId {
        ModelId(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Safe as one path component and one INI section name.
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
            && self.0 != "."
            && self.0 != ".."
            && self.0.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ModelId {
    fn from(s: &str) -> ModelId {
        ModelId(s.to_owned())
    }
}

/// A Hugging Face repository: `google/gemma-4-31B-it-qat-q4_0-gguf`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoId(pub String);

impl fmt::Display for RepoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A pinned git revision (commit hash) of a repository.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(pub String);

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A SHA-256 digest, as 64 lowercase hex digits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256(String);

impl Sha256 {
    pub fn from_digest(digest: &[u8]) -> Sha256 {
        use std::fmt::Write as _;
        let mut s = String::with_capacity(64);
        for b in digest {
            let _ = write!(s, "{b:02x}");
        }
        Sha256(s)
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256 {
    type Error = String;
    fn try_from(s: String) -> Result<Sha256, String> {
        let s = s.to_ascii_lowercase();
        if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Sha256(s))
        } else {
            Err(format!("not a sha256 (64 hex digits): {s:?}"))
        }
    }
}

impl From<Sha256> for String {
    fn from(h: Sha256) -> String {
        h.0
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A TCP port on localhost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Port(pub u16);

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_display_and_arithmetic() {
        assert_eq!(Bytes::gib(18).to_string(), "18.0 GiB");
        assert_eq!(Bytes::mib(512).to_string(), "512 MiB");
        assert_eq!(Bytes(10).to_string(), "10 B");
        assert_eq!(Bytes::gib(1) - Bytes::gib(2), Bytes::ZERO);
        assert_eq!(Bytes::gib(4).scale(0.5), Bytes::gib(2));
        assert_eq!(BytesPerToken(1000) * ContextTokens::k(32), Bytes(32_768_000));
    }

    #[test]
    fn context_display() {
        assert_eq!(ContextTokens::k(32).to_string(), "32K");
        assert_eq!(ContextTokens(1000).to_string(), "1000");
    }

    #[test]
    fn model_id_validity() {
        assert!(ModelId::from("qwen3.8-27b").is_valid());
        assert!(!ModelId::from("../etc").is_valid());
        assert!(!ModelId::from("a b").is_valid());
        assert!(!ModelId::from("").is_valid());
    }

    #[test]
    fn sha256_parses_hex_only() {
        assert!(Sha256::try_from("A".repeat(64)).is_ok());
        assert!(Sha256::try_from("g".repeat(64)).is_err());
        assert!(Sha256::try_from("a".repeat(63)).is_err());
    }

    #[test]
    fn eta() {
        assert_eq!(BytesPerSecond(100.0).time_for(Bytes(1000)), Some(Duration::from_secs(10)));
        assert_eq!(BytesPerSecond(0.0).time_for(Bytes(1000)), None);
    }
}
