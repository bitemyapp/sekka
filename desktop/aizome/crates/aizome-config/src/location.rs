//! Where the computer is, and what time it is there: for sunrise and
//! sunset, which Night Light's schedule and Auto's theme follow.
//!
//! The place is, in order: `[night_light] latitude` and `longitude` when
//! they're set by hand; else `[location]`, where the shell writes what
//! GeoClue found (rounded to 0.01°, about a kilometre), used only while the
//! time zone is still the one it was found in, since a computer that
//! changed zones has moved; else the main city of the time zone, from
//! tzdata's `zone1970.tab` (or `zone.tab`). Every part of Aizome works it
//! out the same way from the same file, so the compositor's Night Light,
//! the shell's theme, the apps' theme and what Settings says agree.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::Settings;
use crate::compositor::{Sun, sun_times};

/// `[location]`: where GeoClue last said the computer is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Location {
    /// Degrees north (negative south).
    pub latitude: Option<f64>,
    /// Degrees east (negative west).
    pub longitude: Option<f64>,
    /// The time zone it was found in (`Europe/Paris`).
    pub zone: Option<String>,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Location {
    /// Nothing kept.
    pub fn is_empty(&self) -> bool {
        self.latitude.is_none() && self.longitude.is_none() && self.zone.is_none() && self.extra.is_empty()
    }

    /// A position as kept: to 0.01°.
    pub fn found(latitude: f64, longitude: f64, zone: Option<String>) -> Location {
        let round = |v: f64| (v * 100.0).round() / 100.0;
        Location { latitude: Some(round(latitude)), longitude: Some(round(longitude)), zone, extra: toml::Table::new() }
    }

    /// Its position, if it has one and it was found in `zone`.
    pub fn position_in(&self, zone: Option<&str>) -> Option<(f64, f64)> {
        let (lat, lon) = (self.latitude?, self.longitude?);
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return None;
        }
        match (self.zone.as_deref(), zone) {
            (Some(found), Some(now)) if found != now => None,
            _ => Some((lat, lon)),
        }
    }
}

/// Where a place came from, for saying so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceSource {
    /// `[night_light] latitude` and `longitude`, set by hand.
    Settings,
    /// Where GeoClue found the computer.
    Located,
    /// The time zone's main city (`Europe/Paris`).
    TimeZone(String),
}

/// A place sunrise and sunset are reckoned for.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub latitude: f64,
    pub longitude: f64,
    pub source: PlaceSource,
}

impl Place {
    /// Where it is, as a sentence ends: "in Paris", "where you are".
    pub fn words(&self) -> String {
        match &self.source {
            PlaceSource::Settings | PlaceSource::Located => "where you are".into(),
            PlaceSource::TimeZone(zone) => match zone_city(zone) {
                Some(city) => format!("in {city}"),
                None => "in your time zone".into(),
            },
        }
    }
}

/// The local time: minutes since midnight, day of the year (1–366), and
/// minutes east of UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalNow {
    pub minutes: u32,
    pub day_of_year: u32,
    pub utc_offset: i32,
}

impl LocalNow {
    /// Now, where the computer is. The time zone is looked at again each
    /// time (`tzset`), so a change in Date & Time is followed at once.
    pub fn now() -> LocalNow {
        // SAFETY: tzset only rereads TZ and /etc/localtime; localtime_r
        // writes only into `tm`.
        unsafe extern "C" {
            fn tzset();
        }
        unsafe {
            tzset();
            let t = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                return LocalNow { minutes: 12 * 60, day_of_year: 172, utc_offset: 0 };
            }
            LocalNow {
                minutes: (tm.tm_hour.clamp(0, 23) * 60 + tm.tm_min.clamp(0, 59)) as u32,
                day_of_year: (tm.tm_yday + 1).clamp(1, 366) as u32,
                utc_offset: (tm.tm_gmtoff / 60) as i32,
            }
        }
    }

    pub fn at(hour: u32, minute: u32, day_of_year: u32, utc_offset: i32) -> LocalNow {
        LocalNow { minutes: hour * 60 + minute, day_of_year, utc_offset }
    }
}

/// A span of the day, `[start, end)` in minutes since midnight, which may
/// run past midnight; or all day, or none of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Between { start: u32, end: u32 },
    AllDay,
    Never,
}

impl Span {
    pub fn contains(self, minutes: u32) -> bool {
        match self {
            Span::Between { start, end } if start <= end => (start..end).contains(&minutes),
            Span::Between { start, end } => minutes >= start || minutes < end,
            Span::AllDay => true,
            Span::Never => false,
        }
    }

    /// Minutes from `minutes` until it next starts or ends (at least 1);
    /// none when it never changes.
    pub fn minutes_to_change(self, minutes: u32) -> Option<u32> {
        let Span::Between { start, end } = self else { return None };
        let until = |t: u32| (t + 1440 - minutes % 1440) % 1440;
        let next = [until(start), until(end)].into_iter().filter(|&m| m > 0).min().unwrap_or(1440);
        Some(next.max(1))
    }
}

/// `"07:05"` for minutes since midnight.
pub fn hm(minutes: u32) -> String {
    format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
}

/// The day, sunrise to sunset, at a place on a day (`None` without a
/// place).
pub fn daylight(place: Option<&Place>, day_of_year: u32, utc_offset: i32) -> Option<Span> {
    let p = place?;
    Some(match sun_times(p.latitude, p.longitude, day_of_year, utc_offset) {
        Sun::Rises { sunrise, sunset } => Span::Between { start: sunrise, end: sunset },
        Sun::AlwaysUp => Span::AllDay,
        Sun::AlwaysDown => Span::Never,
    })
}

impl Settings {
    /// Where sunrise and sunset are reckoned for (see the module's
    /// documentation), in time zone `zone`.
    pub fn place_in(&self, zone: Option<&str>) -> Option<Place> {
        let n = &self.night_light;
        if let (Some(latitude), Some(longitude)) = (n.latitude, n.longitude) {
            return Some(Place { latitude, longitude, source: PlaceSource::Settings });
        }
        if let Some((latitude, longitude)) = self.location.position_in(zone) {
            return Some(Place { latitude, longitude, source: PlaceSource::Located });
        }
        let zone = zone?;
        let (latitude, longitude) = zone_coordinates(zone)?;
        Some(Place { latitude, longitude, source: PlaceSource::TimeZone(zone.to_owned()) })
    }

    /// The place, in the computer's time zone now.
    pub fn place(&self) -> Option<Place> {
        self.place_in(system_timezone().as_deref())
    }

    /// Whether something follows the sun, so the shell should ask GeoClue
    /// where the computer is: Night Light on its sunset schedule (without a
    /// place set by hand), or Auto following sunset and sunrise.
    pub fn wants_location(&self) -> bool {
        let n = &self.night_light;
        let night =
            n.enabled && n.schedule == crate::NightSchedule::Sunset && (n.latitude.is_none() || n.longitude.is_none());
        let theme = self.appearance.theme == crate::ThemeChoice::Auto && self.appearance.follow_sunset;
        night || theme
    }
}

// Time zones.

/// The computer's time zone: `$TZ`, else what `/etc/localtime` names
/// (`…/zoneinfo/Europe/Paris`); none for UTC and zones named for no place.
/// Looked at again after a second, so views can ask as often as they draw.
pub fn system_timezone() -> Option<String> {
    static LAST: Mutex<Option<(std::time::Instant, Option<String>)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, zone)) = last.as_ref()
        && at.elapsed() < std::time::Duration::from_secs(1)
    {
        return zone.clone();
    }
    let zone = read_timezone();
    *last = Some((std::time::Instant::now(), zone.clone()));
    zone
}

fn read_timezone() -> Option<String> {
    if let Some(tz) = std::env::var("TZ").ok().map(|t| t.trim_start_matches(':').to_owned()).filter(|t| t.contains('/'))
    {
        return Some(zone_of_path(&tz).unwrap_or(tz));
    }
    // The link's own text first: tzdata links some zones to others
    // (US/Eastern to America/New_York), and the name chosen is the one meant.
    let link = std::fs::read_link("/etc/localtime").ok().map(|p| p.to_string_lossy().into_owned());
    if let Some(zone) = link.as_deref().and_then(zone_of_path) {
        return Some(zone);
    }
    let target = std::fs::canonicalize("/etc/localtime").ok()?;
    zone_of_path(&target.to_string_lossy())
}

/// `Europe/Paris` from `/etc/zoneinfo/Europe/Paris` (or a relative link
/// like `../etc/zoneinfo/…`, or tzdata's `posix/` and `right/` copies).
pub fn zone_of_path(path: &str) -> Option<String> {
    let (_, zone) = path.rsplit_once("zoneinfo/")?;
    let zone = zone.trim_start_matches("posix/").trim_start_matches("right/");
    zone.contains('/').then(|| zone.to_owned())
}

/// The city a zone is named for: `America/Argentina/Buenos_Aires` is
/// "Buenos Aires"; `Etc/UTC` is none.
pub fn zone_city(zone: &str) -> Option<String> {
    let (area, rest) = zone.split_once('/')?;
    if matches!(area, "Etc" | "SystemV" | "US" | "Canada" | "Brazil" | "Chile" | "Mexico") {
        return None;
    }
    let city = rest.rsplit('/').next()?.replace('_', " ");
    (!city.is_empty()).then_some(city)
}

/// Where tzdata is: `$TZDIR`, NixOS's `/etc/zoneinfo`, the usual places.
fn zoneinfo_dirs() -> Vec<PathBuf> {
    std::env::var_os("TZDIR")
        .map(PathBuf::from)
        .into_iter()
        .chain(
            ["/etc/zoneinfo", "/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo"].map(PathBuf::from),
        )
        .collect()
}

/// The coordinates of a zone's main city, from tzdata's tables (read once
/// per zone).
pub fn zone_coordinates(zone: &str) -> Option<(f64, f64)> {
    type Found = HashMap<String, Option<(f64, f64)>>;
    static CACHE: OnceLock<Mutex<Found>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(zone) {
        return *found;
    }
    let found = zoneinfo_dirs().into_iter().find_map(|dir| {
        ["zone1970.tab", "zone.tab"]
            .iter()
            .find_map(|t| std::fs::read_to_string(dir.join(t)).ok().and_then(|text| coordinates_in_table(&text, zone)))
    });
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(zone.to_owned(), found);
    found
}

/// A zone's coordinates in a `zone1970.tab` or `zone.tab`:
/// `codes<TAB>±DDMM±DDDMM<TAB>Zone[<TAB>comment]`.
pub fn coordinates_in_table(text: &str, zone: &str) -> Option<(f64, f64)> {
    text.lines().filter(|l| !l.starts_with('#')).find_map(|line| {
        let mut fields = line.split('\t');
        let _codes = fields.next()?;
        let coordinates = fields.next()?;
        (fields.next()?.trim() == zone).then(|| parse_coordinates(coordinates)).flatten()
    })
}

/// ISO 6709 as tzdata writes it: `+4852+00220` or `+353916+1394441`.
pub fn parse_coordinates(text: &str) -> Option<(f64, f64)> {
    let text = text.trim();
    let split = text.char_indices().skip(1).find(|(_, c)| *c == '+' || *c == '-')?.0;
    let (lat, lon) = text.split_at(split);
    Some((angle(lat, 2)?, angle(lon, 3)?))
}

fn angle(text: &str, degree_digits: usize) -> Option<f64> {
    let sign = match text.chars().next()? {
        '+' => 1.0,
        '-' => -1.0,
        _ => return None,
    };
    let digits = &text[1..];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |r: std::ops::Range<usize>| -> Option<f64> { digits.get(r)?.parse::<f64>().ok() };
    let degrees = part(0..degree_digits)?;
    let minutes = part(degree_digits..degree_digits + 2)?;
    let seconds = match digits.len().checked_sub(degree_digits)? {
        2 => 0.0,
        4 => part(degree_digits + 2..degree_digits + 4)?,
        _ => return None,
    };
    Some(sign * (degrees + minutes / 60.0 + seconds / 3600.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# tzdata\nFR,MC\t+4852+00220\tEurope/Paris\nJP\t+353916+1394441\tAsia/Tokyo\tmost of Japan\n";

    #[test]
    fn reads_tzdata_coordinates() {
        let (lat, lon) = coordinates_in_table(TABLE, "Europe/Paris").unwrap();
        assert!((lat - 48.8667).abs() < 1e-3 && (lon - 2.3333).abs() < 1e-3, "{lat} {lon}");
        let (lat, lon) = coordinates_in_table(TABLE, "Asia/Tokyo").unwrap();
        assert!((lat - 35.6544).abs() < 1e-3 && (lon - 139.7447).abs() < 1e-3, "{lat} {lon}");
        assert_eq!(coordinates_in_table(TABLE, "Europe/Monaco"), None);
        assert_eq!(parse_coordinates("-3352+15113"), Some((-(33.0 + 52.0 / 60.0), 151.0 + 13.0 / 60.0)));
    }

    #[test]
    fn zones_from_links() {
        assert_eq!(zone_of_path("/etc/zoneinfo/Europe/Paris").as_deref(), Some("Europe/Paris"));
        assert_eq!(zone_of_path("../etc/zoneinfo/Asia/Tokyo").as_deref(), Some("Asia/Tokyo"));
        assert_eq!(
            zone_of_path("/nix/store/x-tzdata/share/zoneinfo/posix/America/Chicago").as_deref(),
            Some("America/Chicago")
        );
        assert_eq!(zone_of_path("/etc/zoneinfo/UTC"), None);
        assert_eq!(zone_city("America/Argentina/Buenos_Aires").as_deref(), Some("Buenos Aires"));
        assert_eq!(zone_city("Etc/UTC"), None);
    }

    #[test]
    fn a_located_place_holds_only_in_its_zone() {
        let l = Location::found(48.85341, 2.3488, Some("Europe/Paris".into()));
        assert_eq!((l.latitude, l.longitude), (Some(48.85), Some(2.35)));
        assert_eq!(l.position_in(Some("Europe/Paris")), Some((48.85, 2.35)));
        assert_eq!(l.position_in(Some("Asia/Tokyo")), None);
        // Nothing kept: left out of the file.
        assert!(!Settings::default().to_toml().unwrap().contains("[location]"));
        let mut s = Settings { location: l, ..Settings::default() };
        assert_eq!(s.place_in(Some("Europe/Paris")).unwrap().source, PlaceSource::Located);
        // Moved to Tokyo: Tokyo's coordinates, if tzdata is here, else none.
        assert!(s.place_in(Some("Asia/Tokyo")).is_none_or(|p| p.source == PlaceSource::TimeZone("Asia/Tokyo".into())));
        // Set by hand wins.
        s.night_light.latitude = Some(60.0);
        s.night_light.longitude = Some(10.0);
        assert_eq!(s.place_in(Some("Asia/Tokyo")).unwrap().source, PlaceSource::Settings);
    }

    #[test]
    fn spans_and_their_next_change() {
        let day = Span::Between { start: 7 * 60 + 30, end: 18 * 60 + 45 };
        assert!(day.contains(12 * 60) && !day.contains(19 * 60) && !day.contains(7 * 60));
        assert_eq!(day.minutes_to_change(7 * 60), Some(30));
        assert_eq!(day.minutes_to_change(19 * 60), Some(12 * 60 + 30));
        let night = Span::Between { start: 18 * 60 + 45, end: 7 * 60 + 30 };
        assert!(night.contains(23 * 60) && night.contains(3 * 60) && !night.contains(12 * 60));
        assert_eq!(Span::AllDay.minutes_to_change(5), None);
        assert_eq!(hm(7 * 60 + 5), "07:05");
    }

    #[test]
    fn daylight_in_paris_in_october() {
        let paris =
            Place { latitude: 48.8667, longitude: 2.3333, source: PlaceSource::TimeZone("Europe/Paris".into()) };
        // 18 October (day 291), CEST: about 08:15 and 18:57.
        let Some(Span::Between { start, end }) = daylight(Some(&paris), 291, 120) else { panic!() };
        assert!((8 * 60 + 5..8 * 60 + 25).contains(&start), "{}", hm(start));
        assert!((18 * 60 + 45..19 * 60 + 5).contains(&end), "{}", hm(end));
        assert_eq!(paris.words(), "in Paris");
        assert_eq!(daylight(None, 291, 120), None);
    }
}
