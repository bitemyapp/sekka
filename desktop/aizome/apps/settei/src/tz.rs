//! Offsets from the system's tzdata, interpreted by Jiff. The caller
//! supplies the instant so previews and tests keep a fixed clock.

use std::path::Path;

use jiff::{Timestamp, tz::TimeZone};

/// The offset east of UTC, in seconds, of zone `id` (under `dir`) at Unix
/// time `now`. Read that database explicitly, including on NixOS, without
/// falling back to a different system or bundled database.
pub fn offset(dir: &Path, id: &str, now: i64) -> Option<i32> {
    if id.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return None;
    }
    let now = Timestamp::from_second(now).ok()?;
    let bytes = std::fs::read(dir.join(id)).ok()?;
    let zone = TimeZone::tzif(id, &bytes).ok()?;
    Some(zone.to_offset(now).seconds())
}

/// "UTC+9", "UTC−5:30", "UTC".
pub fn describe(offset: i32) -> String {
    if offset == 0 {
        return "UTC".into();
    }
    let sign = if offset < 0 { "−" } else { "+" };
    let a = offset.abs();
    let (h, m) = (a / 3600, (a % 3600) / 60);
    if m == 0 { format!("UTC{sign}{h}") } else { format!("UTC{sign}{h}:{m:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_follow_transitions_and_future_rules() {
        let Some(dir) = aizome_sys::datetime::zones::zoneinfo_dir() else {
            eprintln!("no tzdata here: set TZDIR");
            return;
        };
        for (id, instant, expected) in [
            ("America/Chicago", "2026-03-08T07:59:59Z", -21600),
            ("America/Chicago", "2026-03-08T08:00:00Z", -18000),
            ("America/Chicago", "2026-11-01T06:59:59Z", -18000),
            ("America/Chicago", "2026-11-01T07:00:00Z", -21600),
            // Beyond tzdata's explicit transition table: use its POSIX footer.
            ("America/Chicago", "2100-01-15T12:00:00Z", -21600),
            ("America/Chicago", "2100-07-15T12:00:00Z", -18000),
            ("Australia/Sydney", "2100-01-15T12:00:00Z", 39600),
            ("Australia/Sydney", "2100-07-15T12:00:00Z", 36000),
            ("Asia/Tokyo", "2026-10-17T12:00:00Z", 32400),
            ("Asia/Kathmandu", "2026-10-17T12:00:00Z", 20700),
        ] {
            let now: Timestamp = instant.parse().unwrap();
            assert_eq!(offset(&dir, id, now.as_second()), Some(expected), "{id} at {instant}");
        }
        assert_eq!(offset(&dir, "Asia/Tokyo", i64::MAX), None);
    }

    #[test]
    fn only_uses_the_requested_database() {
        let dir = std::env::temp_dir().join(format!("settei-timezones-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Asia")).unwrap();
        // These are valid IANA names: neither missing nor invalid files may
        // fall back to Jiff's own database and return a plausible offset.
        assert_eq!(offset(&dir, "America/Chicago", 0), None);
        std::fs::write(dir.join("Asia/Tokyo"), b"TZif").unwrap();
        assert_eq!(offset(&dir, "Asia/Tokyo", 0), None);
        for id in ["", "/Asia/Tokyo", "../Asia/Tokyo", "Asia/../Tokyo", "Asia/./Tokyo", "Asia//Tokyo"] {
            assert_eq!(offset(&dir, id, 0), None, "{id}");
        }
        if let Some(system) = aizome_sys::datetime::zones::zoneinfo_dir() {
            // A valid file with a different zone's name must still use the
            // requested file's rules, not the name in Jiff's database.
            std::fs::copy(system.join("Asia/Tokyo"), dir.join("Asia/Kathmandu")).unwrap();
            assert_eq!(offset(&dir, "Asia/Kathmandu", 0), Some(32400));
            for id in ["Asia/./Kathmandu", "Asia//Kathmandu", "Asia/../Asia/Kathmandu"] {
                assert_eq!(offset(&dir, id, 0), None, "{id}");
            }
            assert_eq!(offset(&dir, dir.join("Asia/Kathmandu").to_str().unwrap(), 0), None);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn offset_labels_keep_their_format() {
        assert_eq!(describe(0), "UTC");
        assert_eq!(describe(9 * 3600), "UTC+9");
        assert_eq!(describe(20700), "UTC+5:45");
        assert_eq!(describe(-19800), "UTC−5:30");
    }
}
