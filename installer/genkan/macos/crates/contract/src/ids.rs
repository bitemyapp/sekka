// SPDX-License-Identifier: GPL-3.0-or-later
//! Validated scalar values. Each parses from a string and serializes back to
//! exactly the same string, so a document that parsed is already canonical in
//! every scalar.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::str::FromStr;

use crate::Error;

macro_rules! string_value {
    ($name:ident) => {
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(|e: Error| D::Error::custom(e.to_string()))
            }
        }
    };
}

/// A SHA-256 digest, written `sha256:` and 64 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(pub [u8; 32]);
impl Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    /// The same digest, without the `sha256:` prefix (as `shasum -a 256`
    /// prints it).
    pub fn hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
            s.push(char::from_digit((b & 15) as u32, 16).unwrap());
        }
        s
    }
    pub fn from_hex(hex: &str) -> Result<Self, Error> {
        let bytes = hex.as_bytes();
        if bytes.len() != 64 {
            return Err(Error::Invalid("a SHA-256 digest has 64 hex digits".into()));
        }
        let mut out = [0u8; 32];
        for (i, pair) in bytes.chunks(2).enumerate() {
            let digit = |c: u8| match c {
                b'0'..=b'9' => Ok(c - b'0'),
                b'a'..=b'f' => Ok(c - b'a' + 10),
                _ => Err(Error::Invalid("digests are lowercase hex".into())),
            };
            out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
        }
        Ok(Self(out))
    }
}
impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:{}", self.hex())
    }
}
impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl FromStr for Digest {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let hex = s
            .strip_prefix("sha256:")
            .ok_or_else(|| Error::Invalid("digests start with sha256:".into()))?;
        Self::from_hex(hex)
    }
}
string_value!(Digest);

/// A streaming SHA-256 for files and downloads.
#[derive(Clone, Default)]
pub struct Hasher(Sha256);
impl Hasher {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    pub fn finish(self) -> Digest {
        Digest(self.0.finalize().into())
    }
}

/// A UUID in its lowercase hyphenated form. macOS prints them in upper
/// case and Linux in lower; documents always hold lower case, and
/// [`Uuid::parse_any`] accepts either from a tool.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);
impl Uuid {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
    /// Either case, as printed by diskutil, blkid or lsblk.
    pub fn parse_any(s: &str) -> Result<Self, Error> {
        Self::parse(&s.to_ascii_lowercase())
    }
    fn parse(s: &str) -> Result<Self, Error> {
        let bytes = s.as_bytes();
        let shape = bytes.len() == 36
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| match i {
                    8 | 13 | 18 | 23 => *b == b'-',
                    _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
                });
        if !shape {
            return Err(Error::Invalid(format!(
                "not a lowercase UUID: {}",
                s.chars().take(40).collect::<String>()
            )));
        }
        let hex: String = s.chars().filter(|c| *c != '-').collect();
        let mut out = [0u8; 16];
        for i in 0..16 {
            out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        Ok(Self(out))
    }
    /// A random (version 4) UUID from the operating system's generator.
    pub fn random() -> Result<Self, Error> {
        let mut bytes = [0u8; 16];
        crate::random::fill(&mut bytes)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(Self(bytes))
    }
    /// Upper case, as macOS's tools print and accept it.
    pub fn to_upper(&self) -> String {
        self.to_string().to_ascii_uppercase()
    }
}
impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}
impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl FromStr for Uuid {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::parse(s)
    }
}
string_value!(Uuid);

/// A moment in UTC, to the second: `2026-11-06T20:39:38Z`. Nothing finer is
/// recorded, and no time zone but UTC is written.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Timestamp(i64);
impl Timestamp {
    pub const fn from_unix(seconds: i64) -> Self {
        Self(seconds)
    }
    pub fn unix(self) -> i64 {
        self.0
    }
    pub fn now() -> Self {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Self(d.as_secs() as i64)
    }
    pub fn plus(self, seconds: i64) -> Self {
        Self(self.0.saturating_add(seconds))
    }
}
impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.0.div_euclid(86_400);
        let secs = self.0.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        write!(
            f,
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        )
    }
}
impl FromStr for Timestamp {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::Invalid(format!("not a UTC timestamp: {s:.40}"));
        let b = s.as_bytes();
        if b.len() != 20
            || b[4] != b'-'
            || b[7] != b'-'
            || b[10] != b'T'
            || b[13] != b':'
            || b[16] != b':'
            || b[19] != b'Z'
        {
            return Err(bad());
        }
        let n = |r: std::ops::Range<usize>| -> Result<i64, Error> {
            let part = &s[r];
            if !part.bytes().all(|c| c.is_ascii_digit()) {
                return Err(bad());
            }
            part.parse().map_err(|_| bad())
        };
        let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
        if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || se > 59
        {
            return Err(bad());
        }
        Ok(Self(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se))
    }
}
string_value!(Timestamp);

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// Howard Hinnant's algorithms for the proleptic Gregorian calendar.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// A short identifier: lowercase letters, digits, `.`, `-` and `_`, starting
/// with a letter or digit, at most 96 bytes (`sekka-j514s-1c8b4e2c`,
/// `j514s`, `development`).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Slug(String);
impl Slug {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl FromStr for Slug {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let b = s.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= 96
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'-' | b'_')
            });
        if ok {
            Ok(Self(s.into()))
        } else {
            Err(Error::Invalid(format!("not an identifier: {s:.100}")))
        }
    }
}
string_value!(Slug);

/// `MAJOR.MINOR.PATCH`, compared numerically. macOS versions with two parts
/// (`14.7`) are read with a zero patch and written as they were given.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Version {
    parts: [u32; 3],
    text: String,
}
impl Version {
    pub fn parts(&self) -> [u32; 3] {
        self.parts
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.parts.cmp(&other.parts)
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl FromStr for Version {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::Invalid(format!("not a version: {s:.40}"));
        let pieces: Vec<&str> = s.split('.').collect();
        if !(2..=3).contains(&pieces.len()) {
            return Err(bad());
        }
        let mut parts = [0u32; 3];
        for (i, p) in pieces.iter().enumerate() {
            if p.is_empty() || p.len() > 9 || !p.bytes().all(|c| c.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0')) {
                return Err(bad());
            }
            parts[i] = p.parse().map_err(|_| bad())?;
        }
        Ok(Self {
            parts,
            text: s.into(),
        })
    }
}
string_value!(Version);

/// A full Git commit: 40 lowercase hex digits.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct GitRevision(String);
impl GitRevision {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn short(&self) -> &str {
        &self.0[..8]
    }
}
impl fmt::Display for GitRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl FromStr for GitRevision {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        if s.len() == 40 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
            Ok(Self(s.into()))
        } else {
            Err(Error::Invalid("a revision is a full 40-digit commit".into()))
        }
    }
}
string_value!(GitRevision);

/// An `https://` address with a host, no credentials, no fragment, at most
/// 2048 bytes. Which hosts are trusted is the trust configuration's
/// business, not this type's.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct HttpsUrl(String);
impl HttpsUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn host(&self) -> &str {
        let rest = &self.0["https://".len()..];
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        &rest[..end]
    }
}
impl fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl FromStr for HttpsUrl {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = |why: &str| Error::Invalid(format!("{why}: {s:.80}"));
        let rest = s.strip_prefix("https://").ok_or_else(|| bad("only https:// addresses are used"))?;
        if s.len() > 2048 || s.bytes().any(|c| c <= b' ' || c >= 0x7f || c == b'#' || c == b'\\') {
            return Err(bad("not a plain address"));
        }
        let host = &rest[..rest.find(['/', '?']).unwrap_or(rest.len())];
        if host.is_empty() || host.contains('@') {
            return Err(bad("an address names a host and no credentials"));
        }
        let host_ok = host.split(':').next().is_some_and(|h| {
            !h.is_empty()
                && h.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-')
        });
        if !host_ok {
            return Err(bad("hosts are lowercase names"));
        }
        Ok(Self(s.into()))
    }
}
string_value!(HttpsUrl);

/// Text a person reads: at most 2048 bytes, no control characters but line
/// breaks.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Text(String);
impl Text {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl FromStr for Text {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        if s.len() <= 2048 && !s.chars().any(|c| c.is_control() && c != '\n') {
            Ok(Self(s.into()))
        } else {
            Err(Error::Invalid("text is at most 2 KiB with no control characters".into()))
        }
    }
}
string_value!(Text);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_round_trip() {
        let d = Digest::of(b"abc");
        assert_eq!(
            d.to_string(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(d.to_string().parse::<Digest>().unwrap(), d);
        assert!("sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
            .parse::<Digest>()
            .is_err());
        assert!("ba7816bf".parse::<Digest>().is_err());
    }

    #[test]
    fn uuids_are_lowercase_in_documents() {
        let u = Uuid::parse_any("910DE5CA-7390-4716-9A54-8028916E4D78").unwrap();
        assert_eq!(u.to_string(), "910de5ca-7390-4716-9a54-8028916e4d78");
        assert_eq!(u.to_upper(), "910DE5CA-7390-4716-9A54-8028916E4D78");
        assert!("910DE5CA-7390-4716-9A54-8028916E4D78".parse::<Uuid>().is_err());
        assert!("910de5ca73904716-9a54-8028916e4d78".parse::<Uuid>().is_err());
        let r = Uuid::random().unwrap();
        assert_eq!(r.as_bytes()[6] >> 4, 4);
        assert_ne!(r, Uuid::random().unwrap());
    }

    #[test]
    fn timestamps_round_trip() {
        for s in ["1970-01-01T00:00:00Z", "2026-11-06T20:39:38Z", "2028-02-29T23:59:59Z"] {
            assert_eq!(s.parse::<Timestamp>().unwrap().to_string(), s);
        }
        assert_eq!("1970-01-02T00:00:01Z".parse::<Timestamp>().unwrap().unix(), 86_401);
        for s in ["2026-02-29T00:00:00Z", "2026-11-06T24:00:00Z", "2026-11-06 20:39:38Z", "2026-11-06T20:39:38+00:00"] {
            assert!(s.parse::<Timestamp>().is_err(), "{s}");
        }
    }

    #[test]
    fn versions_compare_numerically() {
        let v = |s: &str| s.parse::<Version>().unwrap();
        assert!(v("14.10") > v("14.8.3"));
        assert_eq!(v("14.7").parts(), [14, 7, 0]);
        assert_eq!(v("14.7").to_string(), "14.7");
        for bad in ["14", "14.08", "a.b", "1.2.3.4", ""] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }

    #[test]
    fn urls_are_plain_https() {
        let u: HttpsUrl = "https://downloads.sekka.computer/mac/v1/catalog.json".parse().unwrap();
        assert_eq!(u.host(), "downloads.sekka.computer");
        for bad in [
            "http://downloads.sekka.computer/x",
            "https://user@host/x",
            "https://Host/x",
            "https://host/x#frag",
            "https:///x",
        ] {
            assert!(bad.parse::<HttpsUrl>().is_err(), "{bad}");
        }
    }
}
