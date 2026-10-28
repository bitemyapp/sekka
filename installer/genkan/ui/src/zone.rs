// SPDX-License-Identifier: GPL-3.0-or-later
//! The time zone being chosen, as the Location page shows it: its name,
//! local time and UTC offset, which change with the clock and daylight
//! saving time. Times come from the media's tzdata (the installed system's),
//! read by path, not from whatever this process's own zone database is.
use crate::format::utc_offset;
use calamares_nixos::{Settings, timezone::Source, zonemap::Place};
use jiff::{Timestamp, tz::TimeZone};
use std::path::Path;

/// tzdata used for the installed system; outside the live ISO (previews),
/// the host's.
pub fn zoneinfo_dir() -> String {
    Settings::load()
        .map(|s| s.zoneinfo)
        .ok()
        .or_else(|| std::env::var("TZDIR").ok().filter(|d| !d.is_empty()))
        .unwrap_or_else(|| "/usr/share/zoneinfo".into())
}

/// `zone` exactly as `zoneinfo` defines it.
fn zone(zoneinfo: &str, zone: &str) -> Option<TimeZone> {
    if zone
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    let data = std::fs::read(Path::new(zoneinfo).join(zone)).ok()?;
    TimeZone::tzif(zone, &data).ok()
}

/// Minutes east of UTC in `zone` now.
pub fn offset_minutes(zoneinfo: &str, name: &str) -> Option<i32> {
    let tz = zone(zoneinfo, name)?;
    Some(tz.to_offset(Timestamp::now()).seconds() / 60)
}

/// What the Location page says about the chosen zone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clock {
    /// "Central European Summer Time"; the zone's identifier without one.
    pub name: String,
    /// "Berlin, Germany".
    pub place: String,
    /// "3:04 PM"; empty when the zone couldn't be read.
    pub time: String,
    /// "CEST · UTC+02:00 · Europe/Berlin".
    pub detail: String,
}

impl Clock {
    pub fn of(zoneinfo: &str, place: &Place) -> Clock {
        Clock::at(zoneinfo, place, Timestamp::now())
    }

    pub fn at(zoneinfo: &str, place: &Place, now: Timestamp) -> Clock {
        let tz = zone(zoneinfo, &place.zone);
        let info = tz.as_ref().map(|tz| tz.to_offset_info(now));
        let daylight = info.as_ref().is_some_and(|i| i.dst().is_dst());
        let mut detail = Vec::new();
        let mut time = String::new();
        if let (Some(tz), Some(info)) = (&tz, &info) {
            let local = now.to_zoned(tz.clone());
            let hour = local.hour();
            time = format!(
                "{}:{:02} {}",
                if hour % 12 == 0 { 12 } else { hour % 12 },
                local.minute(),
                if hour < 12 { "AM" } else { "PM" }
            );
            let abbreviation = info.abbreviation();
            if abbreviation.starts_with(|c: char| c.is_ascii_alphabetic()) && abbreviation != "UTC"
            {
                detail.push(abbreviation.to_string());
            }
            detail.push(utc_offset(i64::from(info.offset().seconds())));
        }
        detail.push(place.zone.clone());
        Clock {
            name: place.name(daylight).unwrap_or(&place.zone).to_string(),
            place: if place.position.is_some() {
                place.label()
            } else {
                "The same everywhere, with no daylight saving time".into()
            },
            time,
            detail: detail.join(" · "),
        }
    }
}

/// How a detected zone was found, under the map.
pub fn detection_note(source: Source) -> String {
    match source {
        Source::Live => "From this live system's settings.".into(),
        Source::Internet(service) => {
            format!("Located through your internet connection by {service}; a VPN can mislead it.")
        }
        Source::Clock { offset_minutes } => format!(
            "Estimated from the computer's clock, which keeps local time ({}).",
            utc_offset(i64::from(offset_minutes) * 60)
        ),
        Source::Fallback => {
            "Your location couldn't be detected, so New York is selected. Click where you are."
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zone_names_cannot_leave_the_zoneinfo_directory() {
        assert!(zone("/usr/share/zoneinfo", "../../etc/passwd").is_none());
        assert!(zone("/usr/share/zoneinfo", "/etc/passwd").is_none());
        assert!(zone("/usr/share/zoneinfo", "Europe//Berlin").is_none());
    }
}
