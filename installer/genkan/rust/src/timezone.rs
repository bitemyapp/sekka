// SPDX-License-Identifier: GPL-3.0-or-later
//! Time zone validation and detection. Detection only proposes a zone: the
//! Location page shows it on a map for the user to check or change.
use anyhow::{Result, ensure};
use std::{fs, io::Read, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeZone(String);
impl TimeZone {
    pub fn parse(zone: &str, root: &Path) -> Result<Self> {
        ensure!(
            !zone.is_empty()
                && zone.len() <= 100
                && zone.split('/').all(|p| !p.is_empty()
                    && p != "."
                    && p != ".."
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_+-".contains(&b))),
            "Choose a valid IANA time zone, for example America/Chicago"
        );
        let root = root.canonicalize()?;
        let file = root.join(zone).canonicalize()?;
        ensure!(
            file.starts_with(root) && file.is_file(),
            "Unknown time zone"
        );
        let mut magic = [0; 4];
        fs::File::open(file)?.read_exact(&mut magic)?;
        ensure!(&magic == b"TZif", "Not a time-zone data file");
        Ok(Self(zone.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn usable_live_zone(zone: &str, root: &Path) -> Option<TimeZone> {
    let zone = zone.trim();
    // These are the untouched live ISO defaults, not a location guess.
    if matches!(
        zone,
        "" | "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "UCT" | "Etc/UCT"
    ) {
        return None;
    }
    TimeZone::parse(zone, root).ok()
}

/// Used when nothing indicates where the computer is.
pub const FALLBACK: &str = "America/New_York";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The live system was already set to a regional zone.
    Live,
    /// A geolocation service, from the public IP address.
    Internet(&'static str),
    /// The hardware clock keeps local time, as Windows sets it.
    Clock { offset_minutes: i32 },
    /// Nothing to go on.
    Fallback,
}

pub struct Detection {
    pub zone: TimeZone,
    pub source: Source,
}

/// Geolocation services in order; each returns the zone for the caller's
/// public IP address. geoip.kde.org is the service upstream Calamares uses.
const SERVICES: &[(&str, &str)] = &[
    ("geoip.kde.org", "https://geoip.kde.org/v1/calamares"),
    ("ipinfo.io", "https://ipinfo.io/timezone"),
];

fn lookup(url: &str) -> Option<String> {
    let mut args = vec![
        "--fail",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--connect-timeout",
        "3",
        "--max-time",
        "6",
        "--max-filesize",
        "4096",
    ];
    if let Some(ca) = option_env!("CALAMARES_CA_FILE") {
        args.extend(["--cacert", ca]);
    }
    args.extend(["--url", url]);
    let body = crate::process::output("curl", &args, 8).ok()?;
    let body = body.trim();
    // geoip.kde.org answers {"time_zone": "…"}; ipinfo.io answers plain text.
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(json) => json["time_zone"].as_str().map(str::to_owned),
        Err(_) => Some(body.to_owned()),
    }
}

/// The hardware clock's offset from UTC, when it keeps local time. Only
/// meaningful once NTP has corrected the system clock; until then the system
/// clock was itself read from the hardware clock.
fn clock_offset() -> Option<i32> {
    let synced = crate::process::output(
        "timedatectl",
        &["show", "--property=NTPSynchronized", "--value"],
        5,
    )
    .ok()?;
    if synced.trim() != "yes" {
        return None;
    }
    let rtc: i64 = fs::read_to_string("/sys/class/rtc/rtc0/since_epoch")
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    offset_from_drift(rtc - i64::try_from(now).ok()?)
}

/// Rounds a hardware clock lead in seconds to a UTC offset in whole quarter
/// hours, ignoring clocks that keep UTC or have simply drifted.
pub fn offset_from_drift(lead_seconds: i64) -> Option<i32> {
    let minutes = (lead_seconds as f64 / 900.0).round() as i64 * 15;
    ((lead_seconds - minutes * 60).abs() <= 120
        && minutes.abs() >= 30
        && (-12 * 60..=14 * 60).contains(&minutes))
    .then_some(minutes as i32)
}

/// The most-populated zones, in order of preference for a UTC offset.
const BY_OFFSET: &[&str] = &[
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Phoenix",
    "America/Los_Angeles",
    "America/Anchorage",
    "Pacific/Honolulu",
    "America/Mexico_City",
    "America/Bogota",
    "America/Halifax",
    "America/St_Johns",
    "America/Sao_Paulo",
    "America/Argentina/Buenos_Aires",
    "America/Noronha",
    "Atlantic/Azores",
    "Atlantic/Cape_Verde",
    "Europe/London",
    "Europe/Berlin",
    "Africa/Lagos",
    "Europe/Athens",
    "Africa/Cairo",
    "Africa/Johannesburg",
    "Europe/Moscow",
    "Africa/Nairobi",
    "Asia/Tehran",
    "Asia/Dubai",
    "Asia/Kabul",
    "Asia/Karachi",
    "Asia/Kolkata",
    "Asia/Kathmandu",
    "Asia/Dhaka",
    "Asia/Yangon",
    "Asia/Bangkok",
    "Asia/Jakarta",
    "Asia/Shanghai",
    "Australia/Eucla",
    "Asia/Tokyo",
    "Australia/Darwin",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Sydney",
    "Australia/Lord_Howe",
    "Pacific/Noumea",
    "Pacific/Auckland",
    "Pacific/Chatham",
    "Pacific/Tongatapu",
    "Pacific/Kiritimati",
    "Pacific/Pago_Pago",
    "Pacific/Marquesas",
];

/// The preferred zone currently `offset_minutes` from UTC.
pub fn zone_for_offset(
    offset_minutes: i32,
    root: &Path,
    offset_now: &dyn Fn(&str) -> Option<i32>,
) -> Option<TimeZone> {
    BY_OFFSET
        .iter()
        .filter(|zone| offset_now(zone) == Some(offset_minutes))
        .find_map(|zone| TimeZone::parse(zone, root).ok())
}

/// Best guess at the computer's zone, in order: the live system's setting,
/// IP geolocation (when allowed), a hardware clock kept in local time, then
/// New York. `offset_now` gives a zone's current UTC offset in minutes.
/// Run on a worker: the lookups can take several seconds.
pub fn detect(
    root: &Path,
    internet: bool,
    offset_now: &dyn Fn(&str) -> Option<i32>,
) -> Result<Detection> {
    let local = crate::process::output(
        "timedatectl",
        &["show", "--property=Timezone", "--value"],
        5,
    )
    .unwrap_or_default();
    if let Some(zone) = usable_live_zone(&local, root) {
        return Ok(Detection {
            zone,
            source: Source::Live,
        });
    }
    if internet {
        for (name, url) in SERVICES {
            if let Some(zone) = lookup(url).and_then(|z| TimeZone::parse(z.trim(), root).ok()) {
                return Ok(Detection {
                    zone,
                    source: Source::Internet(name),
                });
            }
        }
    }
    if let Some(offset_minutes) = clock_offset()
        && let Some(zone) = zone_for_offset(offset_minutes, root, offset_now)
    {
        return Ok(Detection {
            zone,
            source: Source::Clock { offset_minutes },
        });
    }
    Ok(Detection {
        zone: TimeZone::parse(FALLBACK, root)?,
        source: Source::Fallback,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn central_and_eastern_remain_distinct_and_utc_is_not_a_location() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("America")).unwrap();
        for zone in ["Chicago", "New_York"] {
            fs::write(tmp.path().join("America").join(zone), b"TZif").unwrap();
        }
        assert_eq!(
            usable_live_zone("America/Chicago\n", tmp.path())
                .as_ref()
                .map(TimeZone::as_str),
            Some("America/Chicago")
        );
        assert_eq!(
            usable_live_zone("America/New_York", tmp.path())
                .as_ref()
                .map(TimeZone::as_str),
            Some("America/New_York")
        );
        for zone in ["UTC", "Etc/UTC", "US", "-0500", "../etc/passwd", ""] {
            assert!(usable_live_zone(zone, tmp.path()).is_none());
        }
        fs::write(tmp.path().join("zone.tab"), "not TZif").unwrap();
        assert!(TimeZone::parse("zone.tab", tmp.path()).is_err());
    }

    #[test]
    fn aliases_stay_inside_tzdata_and_non_tzif_files_fail() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(root.path().join("Region"), b"TZif").unwrap();
        fs::write(root.path().join("NotZone"), b"text").unwrap();
        fs::write(outside.path().join("Zone"), b"TZif").unwrap();
        std::os::unix::fs::symlink("Region", root.path().join("Alias")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("Zone"), root.path().join("Escape"))
            .unwrap();
        assert_eq!(
            TimeZone::parse("Alias", root.path()).unwrap().as_str(),
            "Alias"
        );
        assert!(TimeZone::parse("NotZone", root.path()).is_err());
        assert!(TimeZone::parse("Escape", root.path()).is_err());
    }

    #[test]
    #[ignore = "contacts geoip.kde.org and ipinfo.io; set TZDIR to a tzdata directory"]
    fn geolocation_services_return_a_zone() {
        let root = std::env::var("TZDIR").unwrap_or("/usr/share/zoneinfo".into());
        for (name, url) in SERVICES {
            let zone = lookup(url).unwrap_or_else(|| panic!("{name} did not answer"));
            TimeZone::parse(zone.trim(), Path::new(&root))
                .unwrap_or_else(|e| panic!("{name} answered {zone:?}: {e}"));
        }
    }

    #[test]
    fn hardware_clock_offsets_round_to_quarter_hours_and_ignore_utc_clocks() {
        assert_eq!(offset_from_drift(-5 * 3600 + 40), Some(-300));
        assert_eq!(offset_from_drift(-5 * 3600 - 90), Some(-300));
        assert_eq!(offset_from_drift(5 * 3600 + 30 * 60), Some(330));
        assert_eq!(offset_from_drift(13 * 3600 + 45 * 60), Some(825));
        // UTC clocks, ordinary drift and clocks that are simply wrong.
        for lead in [0, 75, -110, 20 * 60, 7 * 60 + 3600, 15 * 3600, -13 * 3600] {
            assert_eq!(offset_from_drift(lead), None, "{lead}");
        }
    }

    #[test]
    fn offsets_map_to_the_preferred_zone_with_that_offset_now() {
        let root = tempfile::tempdir().unwrap();
        for zone in [
            "America/New_York",
            "America/Chicago",
            "Europe/London",
            "Europe/Berlin",
        ] {
            let file = root.path().join(zone);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"TZif").unwrap();
        }
        // Northern summer: Eastern is UTC-4 and Central UTC-5.
        let summer = |zone: &str| match zone {
            "America/New_York" => Some(-240),
            "America/Chicago" => Some(-300),
            "Europe/London" => Some(60),
            "Europe/Berlin" => Some(120),
            _ => None,
        };
        let pick =
            |offset| zone_for_offset(offset, root.path(), &summer).map(|z| z.as_str().to_owned());
        assert_eq!(pick(-240).as_deref(), Some("America/New_York"));
        assert_eq!(pick(-300).as_deref(), Some("America/Chicago"));
        assert_eq!(pick(60).as_deref(), Some("Europe/London"));
        assert_eq!(pick(120).as_deref(), Some("Europe/Berlin"));
        // Listed zones missing from tzdata are skipped.
        assert_eq!(pick(540), None);
        assert!(TimeZone::parse(FALLBACK, root.path()).is_ok());
    }
}
