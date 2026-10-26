// SPDX-License-Identifier: MIT OR Apache-2.0
//! Store paths: `/nix/store/<hash>-<name>`, and what a person reads in them.
//!
//! The name follows Nix's own rule (`DrvName`): the package name runs up to
//! the first dash that is followed by something other than a letter, and the
//! rest is the version, so `python3.12-requests-2.32.3` is the package
//! `python3.12-requests` at `2.32.3`. Outputs other than `out` end the name
//! (`glibc-2.40-66-bin`), and [`StorePath::output`] takes them back off.
use std::cmp::Ordering;
use std::fmt;

use crate::STORE_DIR;

/// Length of the hash part of a store path.
pub const HASH_LEN: usize = 32;
/// Nix's base-32 alphabet, which store path hashes are written in.
const BASE32: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";
/// Output names Nixpkgs splits packages into, which end store path names.
const OUTPUTS: &[&str] = &[
    "bin", "data", "debug", "dev", "devdoc", "doc", "examples", "info", "lib", "lib32", "man", "modules", "py",
    "shared", "static", "terminfo", "test", "tests",
];

/// A path in the Nix store, parsed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StorePath {
    /// The full path, `/nix/store/<hash>-<name>`.
    path: String,
}

impl StorePath {
    /// Parse a path inside the store. Anything below a store path (such as
    /// `/nix/store/<hash>-firefox-142.0/bin/firefox`) names that store path.
    pub fn parse(path: &str) -> Option<StorePath> {
        let rest = path.strip_prefix(STORE_DIR)?.strip_prefix('/')?;
        let base = rest.split('/').next()?;
        let (hash, name) = base.split_at_checked(HASH_LEN)?;
        let name = name.strip_prefix('-')?;
        if !hash.bytes().all(|b| BASE32.contains(&b)) || name.is_empty() {
            return None;
        }
        Some(StorePath { path: format!("{STORE_DIR}/{base}") })
    }

    pub fn as_str(&self) -> &str {
        &self.path
    }

    /// The hash part, which makes the path unique.
    pub fn hash(&self) -> &str {
        &self.base()[..HASH_LEN]
    }

    /// Everything after the hash: `firefox-142.0.1`.
    pub fn full_name(&self) -> &str {
        &self.base()[HASH_LEN + 1..]
    }

    fn base(&self) -> &str {
        &self.path[STORE_DIR.len() + 1..]
    }

    /// Whether this is a derivation (`.drv`), a build recipe rather than a
    /// build result.
    pub fn is_derivation(&self) -> bool {
        self.full_name().ends_with(".drv")
    }

    /// The package name: `firefox` for `firefox-142.0.1`.
    pub fn name(&self) -> &str {
        split_name(self.full_name()).0
    }

    /// The version, without an output suffix: `142.0.1`. Empty for paths
    /// without one (sources, configuration files).
    pub fn version(&self) -> &str {
        let version = split_name(self.full_name()).1;
        match output_suffix(version) {
            Some(output) => &version[..version.len() - output.len() - 1],
            None => version,
        }
    }

    /// The output this path is, when it is not the default one: `dev` for
    /// `zlib-1.3.1-dev`.
    pub fn output(&self) -> Option<&str> {
        output_suffix(split_name(self.full_name()).1)
    }
}

impl fmt::Display for StorePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path)
    }
}

/// Split a store path name into package name and version, as Nix does.
pub fn split_name(name: &str) -> (&str, &str) {
    let bytes = name.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'-' && bytes.get(i + 1).is_some_and(|next| !next.is_ascii_alphabetic()) {
            return (&name[..i], &name[i + 1..]);
        }
    }
    (name, "")
}

fn output_suffix(version: &str) -> Option<&str> {
    let (_, last) = version.rsplit_once('-')?;
    OUTPUTS.contains(&last).then_some(last)
}

/// Compare two versions the way Nix does (`builtins.compareVersions`):
/// numbers numerically, `pre` before anything, and `2.3a` before `2.3.1`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a, b);
    while !a.is_empty() || !b.is_empty() {
        let (ca, ra) = next_component(a);
        let (cb, rb) = next_component(b);
        if component_less(ca, cb) {
            return Ordering::Less;
        }
        if component_less(cb, ca) {
            return Ordering::Greater;
        }
        (a, b) = (ra, rb);
    }
    Ordering::Equal
}

fn next_component(s: &str) -> (&str, &str) {
    let s = s.trim_start_matches(['.', '-']);
    let end = if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.find(|c: char| !c.is_ascii_digit())
    } else {
        s.find(|c: char| c.is_ascii_digit() || c == '.' || c == '-')
    }
    .unwrap_or(s.len());
    s.split_at(end)
}

fn component_less(a: &str, b: &str) -> bool {
    let (na, nb) = (a.parse::<u64>().ok(), b.parse::<u64>().ok());
    match (na, nb) {
        (Some(x), Some(y)) => x < y,
        _ if a.is_empty() && nb.is_some() => true,
        _ if a == "pre" && b != "pre" => true,
        _ if b == "pre" => false,
        (_, Some(_)) => true,
        (Some(_), _) => false,
        _ => a < b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdfghijklmnpqrsvwxyz";

    fn p(name: &str) -> StorePath {
        StorePath::parse(&format!("/nix/store/{H}-{name}")).unwrap()
    }

    #[test]
    fn names_versions_and_outputs() {
        let firefox = p("firefox-142.0.1");
        assert_eq!(firefox.hash(), H);
        assert_eq!((firefox.name(), firefox.version(), firefox.output()), ("firefox", "142.0.1", None));
        let requests = p("python3.12-requests-2.32.3");
        assert_eq!((requests.name(), requests.version()), ("python3.12-requests", "2.32.3"));
        let bin = p("glibc-2.40-66-bin");
        assert_eq!((bin.name(), bin.version(), bin.output()), ("glibc", "2.40-66", Some("bin")));
        let source = p("source");
        assert_eq!((source.name(), source.version()), ("source", ""));
        let unit = p("unit-dbus.service");
        assert_eq!((unit.name(), unit.version()), ("unit-dbus.service", ""));
        assert!(p("hello-2.12.1.drv").is_derivation());
    }

    #[test]
    fn paths_below_a_store_path_name_it() {
        let path = StorePath::parse(&format!("/nix/store/{H}-firefox-142.0/bin/firefox")).unwrap();
        assert_eq!(path.as_str(), format!("/nix/store/{H}-firefox-142.0"));
        assert!(StorePath::parse("/nix/store/short-name").is_none());
        assert!(StorePath::parse(&format!("/nix/store/{}-x", "e".repeat(32))).is_none());
        assert!(StorePath::parse("/usr/bin/env").is_none());
    }

    #[test]
    fn versions_compare_like_nix() {
        use Ordering::*;
        let cases = [
            ("1.0", "2.3", Less),
            ("2.1", "2.3", Less),
            ("2.3", "2.3", Equal),
            ("2.5", "2.3", Greater),
            ("3.1", "2.3", Greater),
            ("2.3.1", "2.3", Greater),
            ("2.3.1", "2.3a", Greater),
            ("2.3pre1", "2.3", Less),
            ("2.3pre3", "2.3pre12", Less),
            ("2.3a", "2.3c", Less),
            ("2.3pre1", "2.3c", Less),
            ("2.3pre1", "2.3q", Less),
            ("142.0.1", "141.0", Greater),
            ("6.12.9", "6.12.10", Less),
        ];
        for (a, b, want) in cases {
            assert_eq!(compare_versions(a, b), want, "{a} vs {b}");
            assert_eq!(compare_versions(b, a), want.reverse(), "{b} vs {a}");
        }
    }
}
