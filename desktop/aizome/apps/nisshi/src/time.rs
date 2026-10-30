//! Times: as the table and the details show them, as search's `since:` and
//! `until:` take them, and at the start of a log file's lines. All in µs
//! since the epoch, as journald keeps them, read in the computer's zone
//! (or a test's).

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

const MINUTE: i64 = 60_000_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// The zone and, in tests, the moment taken as now.
#[derive(Clone, Debug)]
pub struct Clock {
    tz: TimeZone,
    fixed: Option<i64>,
}

impl Clock {
    pub fn system() -> Clock {
        Clock { tz: TimeZone::system(), fixed: None }
    }

    /// A clock stopped at `now` in `zone` (an IANA name), for tests and
    /// pictures.
    pub fn fixed(zone: &str, now: i64) -> Clock {
        Clock { tz: TimeZone::get(zone).unwrap_or(TimeZone::UTC), fixed: Some(now) }
    }

    pub fn now(&self) -> i64 {
        self.fixed.unwrap_or_else(|| Timestamp::now().as_microsecond())
    }

    fn zoned(&self, us: i64) -> Zoned {
        Timestamp::from_microsecond(us).unwrap_or(Timestamp::UNIX_EPOCH).to_zoned(self.tz.clone())
    }

    fn day(&self, us: i64) -> Date {
        self.zoned(us).date()
    }

    /// 16:04:52.946
    pub fn time(&self, us: i64) -> String {
        let z = self.zoned(us);
        format!("{:02}:{:02}:{:02}.{:03}", z.hour(), z.minute(), z.second(), z.millisecond())
    }

    /// The table's time: the time alone today, the day before it otherwise
    /// ("Oct 28 16:04:52.946"), and the year when it isn't this one.
    pub fn stamp(&self, us: i64) -> String {
        if us == 0 {
            return String::new();
        }
        let (z, today) = (self.zoned(us), self.day(self.now()));
        if z.date() == today {
            self.time(us)
        } else if z.year() == today.year() {
            format!("{} {}", z.strftime("%b %e"), self.time(us))
        } else {
            format!("{} {}", z.strftime("%Y %b %e"), self.time(us))
        }
    }

    /// Hours and minutes, with the day when it isn't today ("Oct 28 15:16").
    pub fn short(&self, us: i64) -> String {
        let (z, today) = (self.zoned(us), self.day(self.now()));
        if z.date() == today {
            format!("{:02}:{:02}", z.hour(), z.minute())
        } else if z.year() == today.year() {
            format!("{} {:02}:{:02}", z.strftime("%b %e"), z.hour(), z.minute())
        } else {
            format!("{} {:02}:{:02}", z.strftime("%Y %b %e"), z.hour(), z.minute())
        }
    }

    /// Everything: "Thursday 29 October 2026, 16:04:52.946055 CDT".
    pub fn full(&self, us: i64) -> String {
        let z = self.zoned(us);
        format!(
            "{}, {:02}:{:02}:{:02}.{:06} {}",
            z.strftime("%A %e %B %Y"),
            z.hour(),
            z.minute(),
            z.second(),
            us.rem_euclid(1_000_000),
            z.strftime("%Z")
        )
    }

    /// "just now", "3 min ago", "2 h ago", "yesterday", "5 days ago".
    pub fn ago(&self, us: i64) -> String {
        let d = self.now() - us;
        if d < 0 {
            return "in the future".into();
        }
        if d < MINUTE {
            return "just now".into();
        }
        if d < HOUR {
            return format!("{} min ago", d / MINUTE);
        }
        if d < DAY && self.day(us) == self.day(self.now()) {
            return format!("{} h ago", d / HOUR);
        }
        let days = (self.day(self.now()) - self.day(us)).get_days();
        match days {
            0 => format!("{} h ago", d / HOUR),
            1 => "yesterday".into(),
            n => format!("{n} days ago"),
        }
    }

    /// A span for a status line: "5 h 12 min", "3 days".
    pub fn span(us: i64) -> String {
        let us = us.max(0);
        if us < MINUTE {
            format!("{} s", us / 1_000_000)
        } else if us < HOUR {
            format!("{} min", us / MINUTE)
        } else if us < DAY {
            let (h, m) = (us / HOUR, (us % HOUR) / MINUTE);
            if m == 0 { format!("{h} h") } else { format!("{h} h {m} min") }
        } else {
            let d = us / DAY;
            if d == 1 { "1 day".into() } else { format!("{d} days") }
        }
    }

    fn at(&self, date: Date, time: Time) -> Option<i64> {
        DateTime::from_parts(date, time).to_zoned(self.tz.clone()).ok().map(|z| z.timestamp().as_microsecond())
    }

    /// A moment as search takes it: `10m`, `2h`, `3d` (that long ago),
    /// `now`, `today`, `yesterday`, `14:30` (today), `2026-10-28`,
    /// `2026-10-28T14:30`, or `@SECONDS`.
    pub fn parse(&self, s: &str) -> Option<i64> {
        let s = s.trim().to_ascii_lowercase();
        let now = self.now();
        let today = self.day(now);
        match s.as_str() {
            "now" => return Some(now),
            "today" => return self.at(today, Time::midnight()),
            "yesterday" => return self.at(today.yesterday().ok()?, Time::midnight()),
            _ => {}
        }
        if let Some(secs) = s.strip_prefix('@') {
            let f: f64 = secs.parse().ok()?;
            return Some((f * 1e6) as i64);
        }
        let units =
            [("ms", 1_000), ("s", 1_000_000), ("min", MINUTE), ("m", MINUTE), ("h", HOUR), ("d", DAY), ("w", 7 * DAY)];
        for (suffix, scale) in units {
            if let Some(n) = s.strip_suffix(suffix).and_then(|n| n.trim().parse::<f64>().ok()) {
                return Some(now - (n * scale as f64) as i64);
            }
        }
        if let Some(t) = parse_clock(&s) {
            return self.at(today, t);
        }
        let (date, time) = match s.split_once(['t', ' ']) {
            Some((d, t)) => (d, Some(t)),
            None => (s.as_str(), None),
        };
        let date: Date = date.parse().ok()?;
        let time = match time {
            Some(t) => parse_clock(t)?,
            None => Time::midnight(),
        };
        self.at(date, time)
    }

    /// The timestamp a log file's line starts with, and the line after it:
    /// ISO 8601 (`2026-10-29T16:04:52-05:00`, `2026-10-29 16:04:52,123`,
    /// `…Z`) or syslog's (`Oct 29 16:04:52`, this year).
    pub fn leading<'a>(&self, line: &'a str) -> Option<(i64, &'a str)> {
        let b = line.as_bytes();
        let digits = |r: std::ops::Range<usize>| b.get(r.clone()).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
        // ISO 8601.
        if b.len() >= 19
            && digits(0..4)
            && b[4] == b'-'
            && digits(5..7)
            && b[7] == b'-'
            && digits(8..10)
            && (b[10] == b'T' || b[10] == b' ')
            && digits(11..13)
            && b[13] == b':'
            && digits(14..16)
            && b[16] == b':'
            && digits(17..19)
        {
            let mut end = 19;
            if b.get(end).is_some_and(|c| *c == b'.' || *c == b',') {
                end += 1;
                while b.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
            }
            let offset_start = end;
            if b.get(end) == Some(&b'Z') {
                end += 1;
            } else if b.get(end).is_some_and(|c| *c == b'+' || *c == b'-') && digits(end + 1..end + 3) {
                end += 3;
                if b.get(end) == Some(&b':') {
                    end += 1;
                }
                if digits(end..end + 2) {
                    end += 2;
                }
            }
            let text = line[..end].replace(',', ".").replace(' ', "T");
            let us = if end > offset_start {
                text.parse::<Timestamp>().ok()?.as_microsecond()
            } else {
                let dt: DateTime = text.parse().ok()?;
                dt.to_zoned(self.tz.clone()).ok()?.timestamp().as_microsecond()
            };
            return Some((us, line[end..].trim_start()));
        }
        // syslog: "Oct 29 16:04:52".
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        if b.len() >= 15 && b[3] == b' ' && b[6] == b' ' && b[9] == b':' && b[12] == b':' {
            let month = MONTHS.iter().position(|m| line.get(..3) == Some(m))? as i8 + 1;
            let day: i8 = line.get(4..6)?.trim().parse().ok()?;
            let t = parse_clock(line.get(7..15)?)?;
            let year = self.day(self.now()).year();
            let date = Date::new(year, month, day).ok()?;
            let mut us = self.at(date, t)?;
            // Last December's lines read in January.
            if us > self.now() + DAY {
                us = self.at(Date::new(year - 1, month, day).ok()?, t)?;
            }
            return Some((us, line[15..].trim_start()));
        }
        None
    }
}

/// `14:30`, `14:30:05`, `14:30:05.250`.
fn parse_clock(s: &str) -> Option<Time> {
    let mut parts = s.split(':');
    let h: i8 = parts.next()?.parse().ok()?;
    let m: i8 = parts.next()?.parse().ok()?;
    let (sec, sub) = match parts.next() {
        Some(sec) => match sec.split_once('.') {
            Some((s, f)) => {
                let f = format!("{f:0<9}");
                (s.parse().ok()?, f.get(..9)?.parse().ok()?)
            }
            None => (sec.parse().ok()?, 0),
        },
        None => (0, 0),
    };
    if parts.next().is_some() {
        return None;
    }
    Time::new(h, m, sec, sub).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Thursday 29 October 2026, 16:05:00 in Chicago (CDT, UTC−5).
    const NOW: i64 = 1_793_307_900_000_000;

    fn clock() -> Clock {
        Clock::fixed("America/Chicago", NOW)
    }

    #[test]
    fn as_the_table_shows_them() {
        let c = clock();
        assert_eq!(c.stamp(NOW - 8_054_000), "16:04:51.946");
        assert_eq!(c.stamp(NOW - DAY), "Oct 28 16:05:00.000");
        assert_eq!(c.stamp(NOW - 400 * DAY), "2025 Sep 24 16:05:00.000");
        assert_eq!(c.ago(NOW - 3 * MINUTE), "3 min ago");
        assert_eq!(c.ago(NOW - DAY), "yesterday");
        assert_eq!(Clock::span(5 * HOUR + 12 * MINUTE), "5 h 12 min");
        assert!(c.full(NOW).starts_with("Thursday 29 October 2026, 16:05:00.000000"));
    }

    #[test]
    fn as_search_takes_them() {
        let c = clock();
        assert_eq!(c.parse("10m"), Some(NOW - 10 * MINUTE));
        assert_eq!(c.parse("2h"), Some(NOW - 2 * HOUR));
        assert_eq!(c.parse("now"), Some(NOW));
        assert_eq!(c.parse("16:00"), Some(NOW - 5 * MINUTE));
        assert_eq!(c.parse("today"), Some(NOW - (16 * HOUR + 5 * MINUTE)));
        assert_eq!(c.parse("yesterday"), Some(NOW - (40 * HOUR + 5 * MINUTE)));
        assert_eq!(c.parse("2026-10-29T16:05"), Some(NOW));
        assert_eq!(c.parse("2026-10-29 16:05"), Some(NOW));
        assert_eq!(c.parse("@1793307900"), Some(NOW));
        assert_eq!(c.parse("soon"), None);
    }

    #[test]
    fn at_the_start_of_lines() {
        let c = clock();
        assert_eq!(c.leading("2026-10-29T16:05:00-05:00 host x: hi"), Some((NOW, "host x: hi")));
        assert_eq!(c.leading("2026-10-29T21:05:00.5Z hi"), Some((NOW + 500_000, "hi")));
        assert_eq!(c.leading("2026-10-29 16:05:00,250 INFO up"), Some((NOW + 250_000, "INFO up")));
        assert_eq!(c.leading("Oct 29 16:05:00 runnymede sshd[1]: up"), Some((NOW, "runnymede sshd[1]: up")));
        assert_eq!(c.leading("[    38.211] (II) Loading"), None);
        assert_eq!(c.leading("plain"), None);
    }
}
