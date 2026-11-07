// SPDX-License-Identifier: GPL-3.0-or-later
//! What the Mac installer left on the EFI system partition for setup
//! (`sekka/install.json`, `sekka.mac.handoff/1`): macOS's choices, offered
//! as setup's starting point, and the installation's own partitions. Each
//! hint is used only if setup offers it (Genkan's lists and checks);
//! the rest are ignored, and setup works the same with no handoff at all.
//! The handoff's partitions aren't setup's: identity.nix names them
//! (nix/apple-silicon/identity.nix, written at the first start from the
//! same handoff), and setup.nix says nothing about the disk.

use std::path::Path;

use calamares_nixos::timezone::TimeZone;
use calamares_nixos::{KEYBOARDS, LOCALES};
use sekka_mac_contract::{Handoff, limits};

/// The handoff, as far as setup takes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hints {
    pub full_name: Option<String>,
    pub computer_name: Option<String>,
    pub locale: Option<String>,
    pub keyboard: Option<String>,
    pub time_zone: Option<String>,
}

/// Read the handoff at `path`. A missing, unreadable or invalid handoff
/// gives no hints.
pub fn read(path: &Path, zoneinfo: &Path) -> Hints {
    let bytes = match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() <= limits::HANDOFF as u64 => match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("the installer's handoff {}: {e}; setup starts from its defaults", path.display());
                return Hints::default();
            }
        },
        Ok(_) => {
            eprintln!("the installer's handoff {} isn't a small file; ignored", path.display());
            return Hints::default();
        }
        Err(_) => return Hints::default(),
    };
    match Handoff::parse(&bytes) {
        Ok(h) => hints(&h, zoneinfo),
        Err(e) => {
            eprintln!("the installer's handoff {}: {e}; ignored", path.display());
            Hints::default()
        }
    }
}

/// The hints setup can use from `h`.
pub fn hints(h: &Handoff, zoneinfo: &Path) -> Hints {
    let text = |t: &Option<sekka_mac_contract::Text>| t.as_ref().map(|t| t.as_str().trim().to_owned());
    Hints {
        full_name: text(&h.hints.full_name).filter(|n| !n.is_empty()),
        computer_name: text(&h.hints.computer_name).filter(|n| !n.is_empty()),
        locale: text(&h.hints.locale).filter(|l| LOCALES.contains(&l.as_str())),
        keyboard: text(&h.hints.keyboard).filter(|k| KEYBOARDS.contains(&k.as_str())),
        time_zone: text(&h.hints.timezone).filter(|z| TimeZone::parse(z, zoneinfo).is_ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handoff(locale: &str, keyboard: &str, zone: &str) -> Handoff {
        let json = format!(
            r#"{{"created":"2026-11-06T21:00:00Z","esp":"910de5ca-7390-4716-9a54-8028916e4d78","hints":{{"computer_name":"Chris's MacBook Pro","full_name":"Chris Allen","keyboard":"{keyboard}","locale":"{locale}","timezone":"{zone}"}},"installation":"01010101-0101-0101-0101-010101010101","release":"sekka-j514s-4882dc33","root":"63d89be1-72f7-4aff-9679-09cafa7e0c2d","schema":"sekka.mac.handoff/1","source_revision":"4882dc33652612e7475f467f6373d8b607e5407c"}}"#
        );
        Handoff::parse(json.as_bytes()).unwrap()
    }

    fn zoneinfo() -> std::path::PathBuf {
        std::env::var_os("TZDIR").map(Into::into).unwrap_or_else(|| "/etc/zoneinfo".into())
    }

    #[test]
    fn hints_setup_offers_are_kept_and_the_rest_dropped() {
        let h = hints(&handoff("de_DE.UTF-8", "de", "Europe/Berlin"), &zoneinfo());
        assert_eq!(h.full_name.as_deref(), Some("Chris Allen"));
        assert_eq!(h.computer_name.as_deref(), Some("Chris's MacBook Pro"));
        assert_eq!(h.locale.as_deref(), Some("de_DE.UTF-8"));
        assert_eq!(h.keyboard.as_deref(), Some("de"));
        if zoneinfo().join("Europe/Berlin").exists() {
            assert_eq!(h.time_zone.as_deref(), Some("Europe/Berlin"));
        }
        // A Dutch Mac: not offered, so not suggested.
        let h = hints(&handoff("nl_NL.UTF-8", "nl", "Mars/Olympus_Mons"), &zoneinfo());
        assert_eq!((h.locale, h.keyboard, h.time_zone), (None, None, None));
    }

    #[test]
    fn no_handoff_or_a_bad_one_gives_no_hints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("install.json");
        assert_eq!(read(&path, &zoneinfo()), Hints::default());
        std::fs::write(&path, b"{\"schema\":\"sekka.mac.handoff/9\"}").unwrap();
        assert_eq!(read(&path, &zoneinfo()), Hints::default());
        std::fs::write(&path, vec![b' '; limits::HANDOFF + 1]).unwrap();
        assert_eq!(read(&path, &zoneinfo()), Hints::default());
        let good = handoff("en_US.UTF-8", "us", "UTC").to_bytes().unwrap();
        std::fs::write(&path, good).unwrap();
        assert_eq!(read(&path, &zoneinfo()).keyboard.as_deref(), Some("us"));
    }
}
