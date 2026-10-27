//! Time: a clock the app reads (the system's, or one a test moves), and
//! what a world clock shows for a city: its time, whether it's day or
//! night there (from the sun's height at the city), its offset from here
//! and whether it's already tomorrow or still yesterday.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// What the app tells time by.
#[derive(Clone)]
pub enum Clock {
    System {
        start: Instant,
    },
    /// A clock a test sets and moves: the wall time, and a monotonic time.
    Fake(Rc<Cell<(Timestamp, Duration)>>),
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::System { start: Instant::now() }
    }
}

impl Clock {
    /// A test's clock at `at`, and the handle to move it with.
    pub fn fake(at: Timestamp) -> (Clock, Rc<Cell<(Timestamp, Duration)>>) {
        let cell = Rc::new(Cell::new((at, Duration::ZERO)));
        (Clock::Fake(cell.clone()), cell)
    }

    /// The wall time now.
    pub fn now(&self) -> Timestamp {
        match self {
            Clock::System { .. } => Timestamp::now(),
            Clock::Fake(c) => c.get().0,
        }
    }

    /// Monotonic time since the clock started: stopwatches and timers run
    /// on it, so changing the system's time doesn't move them.
    pub fn mono(&self) -> Duration {
        match self {
            Clock::System { start } => start.elapsed(),
            Clock::Fake(c) => c.get().1,
        }
    }
}

/// Move a fake clock on.
pub fn advance(cell: &Rc<Cell<(Timestamp, Duration)>>, d: Duration) {
    let (t, m) = cell.get();
    let span = jiff::SignedDuration::try_from(d).unwrap_or_default();
    cell.set((t.checked_add(span).unwrap_or(t), m + d));
}

/// The zone here, with its IANA name where it can be found: `$TZ`, then
/// the zone `/etc/localtime` links to (NixOS links it into the store,
/// where jiff doesn't know the name), then the system's as jiff reads it.
pub fn local_zone() -> TimeZone {
    let named = |name: &str| TimeZone::get(name.trim_start_matches(':')).ok();
    if let Some(tz) = std::env::var("TZ").ok().filter(|v| !v.is_empty()).and_then(|v| named(&v)) {
        return tz;
    }
    if let Ok(target) = std::fs::read_link("/etc/localtime").or_else(|_| std::fs::canonicalize("/etc/localtime")) {
        let t = target.to_string_lossy();
        if let Some(tz) = t.split_once("zoneinfo/").and_then(|(_, name)| named(name)) {
            return tz;
        }
    }
    TimeZone::system()
}

/// The city a zone is named for: `Berlin` for `Europe/Berlin`.
pub fn zone_city(tz: &TimeZone) -> Option<String> {
    let name = tz.iana_name()?;
    if name == "UTC" || name.starts_with("Etc/") {
        return Some(name.to_owned());
    }
    Some(name.rsplit('/').next().unwrap_or(name).replace('_', " "))
}

/// How light it is somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Daylight {
    Day,
    /// The sun just under the horizon (civil twilight): dawn or dusk.
    Twilight {
        rising: bool,
    },
    Night,
}

/// The sun's height above the horizon, in degrees, at `lat`, `lon` at
/// `t` (NOAA's low-precision formulas: well within a degree).
pub fn solar_elevation(t: Timestamp, lat: f64, lon: f64) -> f64 {
    let jd = t.as_second() as f64 / 86_400.0 + 2_440_587.5;
    let n = jd - 2_451_545.0;
    let l = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let g = (357.528 + 0.985_600_3 * n).rem_euclid(360.0).to_radians();
    let lambda = (l + 1.915 * g.sin() + 0.020 * (2.0 * g).sin()).to_radians();
    let eps = (23.439 - 0.000_000_4 * n).to_radians();
    let ra = (eps.cos() * lambda.sin()).atan2(lambda.cos()).to_degrees();
    let dec = (eps.sin() * lambda.sin()).asin();
    let gmst = (18.697_374_558 + 24.065_709_824_419_08 * n).rem_euclid(24.0) * 15.0;
    let ha = (gmst + lon - ra).to_radians();
    let lat = lat.to_radians();
    (lat.sin() * dec.sin() + lat.cos() * dec.cos() * ha.cos()).asin().to_degrees()
}

pub fn daylight(t: Timestamp, lat: f64, lon: f64) -> Daylight {
    let e = solar_elevation(t, lat, lon);
    if e > -0.833 {
        Daylight::Day
    } else if e > -6.0 {
        let later = solar_elevation(t.checked_add(jiff::SignedDuration::from_mins(10)).unwrap_or(t), lat, lon);
        Daylight::Twilight { rising: later > e }
    } else {
        Daylight::Night
    }
}

/// Which day it is there, from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DayRel {
    Yesterday,
    Today,
    Tomorrow,
}

impl DayRel {
    pub fn word(self) -> &'static str {
        match self {
            DayRel::Yesterday => "Yesterday",
            DayRel::Today => "Today",
            DayRel::Tomorrow => "Tomorrow",
        }
    }
}

/// What a world clock's row shows.
#[derive(Clone, Debug, PartialEq)]
pub struct CityTime {
    /// `04:42` (or `4:42` with `pm` in `suffix`).
    pub time: String,
    pub suffix: Option<&'static str>,
    pub day: DayRel,
    /// `+7 h`, `−3 h 30 min`, or `Same time`.
    pub offset: String,
    pub daylight: Daylight,
}

/// An offset from here in seconds, as a row says it.
pub fn offset_words(secs: i32) -> String {
    if secs == 0 {
        return "Same time".into();
    }
    let sign = if secs < 0 { "−" } else { "+" };
    let s = secs.unsigned_abs();
    let (h, m) = (s / 3600, (s % 3600) / 60);
    match (h, m) {
        (0, m) => format!("{sign}{m} min"),
        (h, 0) => format!("{sign}{h} h"),
        (h, m) => format!("{sign}{h} h {m} min"),
    }
}

/// A time of day as Tokei shows it: `07:05`, or `7:05` with `am`/`pm`.
pub fn hm(hour: i8, minute: i8, h24: bool) -> (String, Option<&'static str>) {
    if h24 {
        (format!("{hour:02}:{minute:02}"), None)
    } else {
        let h12 = match hour % 12 {
            0 => 12,
            h => h,
        };
        (format!("{h12}:{minute:02}"), Some(if hour < 12 { "AM" } else { "PM" }))
    }
}

/// A city's row, at `now`, seen from `local`.
pub fn city_time(now: Timestamp, local: &TimeZone, zone: &TimeZone, lat: f64, lon: f64, h24: bool) -> CityTime {
    let here = now.to_zoned(local.clone());
    let there = now.to_zoned(zone.clone());
    let (time, suffix) = hm(there.hour(), there.minute(), h24);
    let day = match there.date().cmp(&here.date()) {
        std::cmp::Ordering::Less => DayRel::Yesterday,
        std::cmp::Ordering::Equal => DayRel::Today,
        std::cmp::Ordering::Greater => DayRel::Tomorrow,
    };
    let diff = there.offset().seconds() - here.offset().seconds();
    CityTime { time, suffix, day, offset: offset_words(diff), daylight: daylight(now, lat, lon) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn the_sun_is_up_at_noon_and_down_at_midnight() {
        // Tokyo, 03:00 UTC is noon there.
        assert!(solar_elevation(ts("2026-06-21T03:00:00Z"), 35.65, 139.74) > 70.0);
        assert_eq!(daylight(ts("2026-06-21T15:00:00Z"), 35.65, 139.74), Daylight::Night);
        // Polar summer: the sun never sets at Longyearbyen in June.
        assert_eq!(daylight(ts("2026-06-21T23:00:00Z"), 78.22, 15.65), Daylight::Day);
        // Dawn in London on the equinox, a little before 06:00 UTC.
        assert_eq!(daylight(ts("2026-03-20T05:35:00Z"), 51.51, -0.13), Daylight::Twilight { rising: true });
        assert_eq!(daylight(ts("2026-03-20T18:30:00Z"), 51.51, -0.13), Daylight::Twilight { rising: false });
    }

    #[test]
    fn rows_say_time_day_and_offset() {
        let berlin = TimeZone::get("Europe/Berlin").unwrap();
        let tokyo = TimeZone::get("Asia/Tokyo").unwrap();
        let ny = TimeZone::get("America/New_York").unwrap();
        let kolkata = TimeZone::get("Asia/Kolkata").unwrap();
        // 22:30 in Berlin (summer time) on 17 October.
        let now = ts("2026-10-17T20:30:00Z");
        let t = city_time(now, &berlin, &tokyo, 35.65, 139.74, true);
        assert_eq!((t.time.as_str(), t.day, t.offset.as_str()), ("05:30", DayRel::Tomorrow, "+7 h"));
        let t = city_time(now, &berlin, &ny, 40.71, -74.01, true);
        assert_eq!((t.time.as_str(), t.day, t.offset.as_str()), ("16:30", DayRel::Today, "−6 h"));
        let t = city_time(now, &berlin, &kolkata, 22.57, 88.36, false);
        assert_eq!((t.time.as_str(), t.suffix, t.offset.as_str()), ("2:00", Some("AM"), "+3 h 30 min"));
        assert_eq!(city_time(now, &berlin, &berlin, 52.5, 13.4, true).offset, "Same time");
        let early = ts("2026-10-17T01:00:00Z");
        assert_eq!(city_time(early, &berlin, &ny, 40.71, -74.01, true).day, DayRel::Yesterday);
    }

    #[test]
    fn hours_read_both_ways() {
        assert_eq!(hm(0, 5, false), ("12:05".to_owned(), Some("AM")));
        assert_eq!(hm(12, 0, false), ("12:00".to_owned(), Some("PM")));
        assert_eq!(hm(23, 59, true), ("23:59".to_owned(), None));
        assert_eq!(offset_words(-1800), "−30 min");
    }
}
