// SPDX-License-Identifier: GPL-3.0-or-later
//! The saved networks Genkan hands over at installation, on standard input
//! and never in a file: for each, its file name and its length in bytes on
//! lines of their own, then the profile itself.

use crate::skeleton::is_profile;

/// More than Genkan carries over: 32 profiles of 16 KiB.
pub const LIMIT: usize = 1 << 20;

pub struct Profile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The next line of `input`, without its newline.
fn line<'a>(input: &mut &'a [u8]) -> Option<&'a str> {
    let end = input.iter().position(|&b| b == b'\n')?;
    let line = &input[..end];
    *input = &input[end + 1..];
    std::str::from_utf8(line).ok()
}

pub fn parse(mut input: &[u8]) -> Result<Vec<Profile>, String> {
    let mut profiles: Vec<Profile> = Vec::new();
    while !input.is_empty() {
        let name = line(&mut input).ok_or("a profile without its name")?;
        if name.contains('/') || !name.ends_with(".nmconnection") || !is_profile(name) {
            return Err(format!("not a profile's name: {name:?}"));
        }
        if profiles.iter().any(|p| p.name == name) {
            return Err(format!("{name} twice"));
        }
        let length: usize = line(&mut input)
            .and_then(|l| l.parse().ok())
            .ok_or_else(|| format!("{name} without its length"))?;
        if length > input.len() {
            return Err(format!("{name} cut short"));
        }
        let (bytes, rest) = input.split_at(length);
        profiles.push(Profile {
            name: name.to_owned(),
            bytes: bytes.to_vec(),
        });
        input = rest;
    }
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_come_whole_and_named() {
        let input =
            b"installer-wifi-0.nmconnection\n13\n[connection]\ninstaller-wifi-1.nmconnection\n0\n";
        let profiles = parse(input).unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].name, "installer-wifi-0.nmconnection");
        assert_eq!(profiles[0].bytes, b"[connection]\n");
        assert!(profiles[1].bytes.is_empty());
        assert!(parse(b"").unwrap().is_empty());
    }

    #[test]
    fn anything_else_is_refused() {
        for input in [
            &b"../escape.nmconnection\n1\nx"[..],
            b".hidden.nmconnection\n1\nx",
            b"notes.txt\n1\nx",
            b"a.nmconnection\n5\nx",
            b"a.nmconnection\nfive\nxxxxx",
            b"a.nmconnection",
            b"a.nmconnection\n1\nxa.nmconnection\n1\ny",
        ] {
            assert!(parse(input).is_err(), "{}", String::from_utf8_lossy(input));
        }
    }
}
