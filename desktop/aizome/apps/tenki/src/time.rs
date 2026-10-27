//! Times at a place, as Open-Meteo gives them with `timezone=auto`
//! (`2026-10-17T14:15`, the place's own wall clock), and the words Tenki
//! says about them: "Now", "14:00" or "2 PM", "Sat", "12 min ago".
//!
//! Wall-clock times keep what the forecast says about daylight saving: an
//! hour after a change of clocks still reads as the place reads it.

use serde::{Deserialize, Serialize};

/// A wall-clock time at some place: no zone, as it reads there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LocalTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
pub fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01.
pub fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y as i32, m, d)
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl LocalTime {
    pub fn new(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> LocalTime {
        LocalTime { year, month, day, hour, minute }
    }

    /// `2026-10-17` or `2026-10-17T14:15` (seconds, if any, are dropped).
    pub fn parse(s: &str) -> Option<LocalTime> {
        let (date, time) = match s.split_once('T') {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        };
        let mut d = date.splitn(3, '-');
        let year = d.next()?.parse().ok()?;
        let month: u8 = d.next()?.parse().ok()?;
        let day: u8 = d.next()?.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let (hour, minute) = match time {
            Some(t) => {
                let mut p = t.split(':');
                let h: u8 = p.next()?.parse().ok()?;
                let m: u8 = p.next().unwrap_or("0").parse().ok()?;
                if h > 23 || m > 59 {
                    return None;
                }
                (h, m)
            }
            None => (0, 0),
        };
        Some(LocalTime { year, month, day, hour, minute })
    }

    /// The wall clock `secs` seconds after 1970-01-01T00:00 (pass a Unix
    /// time plus a place's UTC offset for that place's clock).
    pub fn from_seconds(secs: i64) -> LocalTime {
        let days = secs.div_euclid(86_400);
        let rest = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        LocalTime { year, month, day, hour: (rest / 3600) as u8, minute: (rest % 3600 / 60) as u8 }
    }

    /// Seconds since 1970-01-01T00:00 on this wall clock.
    pub fn seconds(self) -> i64 {
        self.days() * 86_400 + i64::from(self.hour) * 3600 + i64::from(self.minute) * 60
    }

    /// Days since 1970-01-01 of its date.
    pub fn days(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    /// The same day at midnight.
    pub fn date(self) -> LocalTime {
        LocalTime { hour: 0, minute: 0, ..self }
    }

    /// The top of its hour.
    pub fn hour_start(self) -> LocalTime {
        LocalTime { minute: 0, ..self }
    }

    /// Monday is 0.
    pub fn weekday(self) -> usize {
        // 1970-01-01 was a Thursday.
        (self.days() + 3).rem_euclid(7) as usize
    }

    /// "Mon".
    pub fn weekday_short(self) -> &'static str {
        WEEKDAYS[self.weekday()]
    }

    /// Minutes since midnight.
    pub fn minute_of_day(self) -> u32 {
        u32::from(self.hour) * 60 + u32::from(self.minute)
    }
}

/// How hours read: from the desktop's clock setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Clock {
    #[default]
    H24,
    H12,
}

impl Clock {
    /// The menu bar's clock setting, else 24-hour.
    pub fn from_desktop() -> Clock {
        let s = aizome_config::Settings::load_or_default();
        match s.menubar_settings().clock {
            aizome_config::ClockFormat::H12 => Clock::H12,
            _ => Clock::H24,
        }
    }

    /// An hour on the hourly strip: `14:00` or `2 PM`.
    pub fn hour(self, t: LocalTime) -> String {
        match self {
            Clock::H24 => format!("{:02}:00", t.hour),
            Clock::H12 => format!("{} {}", twelve(t.hour), if t.hour < 12 { "AM" } else { "PM" }),
        }
    }

    /// A time of day: `05:52` or `5:52 AM`.
    pub fn time(self, t: LocalTime) -> String {
        match self {
            Clock::H24 => format!("{:02}:{:02}", t.hour, t.minute),
            Clock::H12 => format!("{}:{:02} {}", twelve(t.hour), t.minute, if t.hour < 12 { "AM" } else { "PM" }),
        }
    }
}

fn twelve(h: u8) -> u8 {
    match h % 12 {
        0 => 12,
        h => h,
    }
}

/// How long ago `then` was at `now` (Unix seconds): "just now",
/// "12 min ago", "3 hr ago", "yesterday", "4 days ago".
pub fn ago(now: i64, then: i64) -> String {
    let s = (now - then).max(0);
    let min = s / 60;
    match min {
        0 => "just now".into(),
        1..=59 => format!("{min} min ago"),
        60..=1439 => format!("{} hr ago", min / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", min / 1440),
    }
}

/// A length of time as "11 h 16 min".
pub fn duration(minutes: i64) -> String {
    let m = minutes.max(0);
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The Unix time now.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_days_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for d in [-1000, -1, 0, 1, 59, 60, 11_017, 20_000, 20_743] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(civil_from_days(20_743), (2026, 10, 17));
    }

    #[test]
    fn parses_open_meteo_times() {
        let t = LocalTime::parse("2026-10-17T14:15").unwrap();
        assert_eq!(t, LocalTime::new(2026, 10, 17, 14, 15));
        assert_eq!(t.weekday_short(), "Sat");
        assert_eq!(LocalTime::parse("2026-10-17").unwrap().hour, 0);
        assert_eq!(LocalTime::parse("2026-10-17T14:15:30").unwrap().minute, 15);
        assert!(LocalTime::parse("2026-13-01").is_none());
        assert!(LocalTime::parse("yesterday").is_none());
        assert_eq!(LocalTime::from_seconds(t.seconds()), t);
    }

    #[test]
    fn words_for_times() {
        assert_eq!(Clock::H24.hour(LocalTime::new(2026, 1, 1, 7, 0)), "07:00");
        assert_eq!(Clock::H12.hour(LocalTime::new(2026, 1, 1, 0, 0)), "12 AM");
        assert_eq!(Clock::H12.hour(LocalTime::new(2026, 1, 1, 14, 0)), "2 PM");
        assert_eq!(Clock::H12.time(LocalTime::new(2026, 1, 1, 17, 8)), "5:08 PM");
        assert_eq!(ago(1000, 1000), "just now");
        assert_eq!(ago(1000 + 12 * 60, 1000), "12 min ago");
        assert_eq!(ago(1000 + 3 * 3600 + 120, 1000), "3 hr ago");
        assert_eq!(ago(1000 + 30 * 3600, 1000), "yesterday");
        assert_eq!(ago(1000 + 4 * 86_400, 1000), "4 days ago");
        assert_eq!(duration(676), "11 h 16 min");
        assert_eq!(duration(45), "45 min");
    }
}
