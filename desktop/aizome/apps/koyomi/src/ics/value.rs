//! iCalendar value types: TEXT (escaping), DATE, DATE-TIME (floating, UTC
//! or with a TZID), DURATION and UTC-OFFSET.

use jiff::civil::{Date, DateTime, Time};

/// Escape text for a TEXT value: `\\`, `\;`, `\,` and `\n`.
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Read a TEXT value: `\n` and `\N` are line breaks, `\x` is `x`.
pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// Split a value list on commas that aren't escaped.
pub fn split_list(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b',' => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

fn digits(s: &str, at: usize, n: usize) -> Option<i32> {
    let part = s.get(at..at + n)?;
    if !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// `YYYYMMDD` (also `YYYY-MM-DD`, which some writers use).
pub fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    let compact: String;
    let s = if s.len() == 10 && s.as_bytes()[4] == b'-' {
        compact = s.replace('-', "");
        compact.as_str()
    } else {
        s
    };
    if s.len() != 8 {
        return None;
    }
    let y = digits(s, 0, 4)?;
    let m = digits(s, 4, 2)?;
    let d = digits(s, 6, 2)?;
    Date::new(y as i16, m as i8, d as i8).ok()
}

/// A DATE-TIME's parts: the civil time, and whether it ended in `Z`.
pub fn parse_datetime(s: &str) -> Option<(DateTime, bool)> {
    let s = s.trim();
    let (s, utc) = match s.strip_suffix(['Z', 'z']) {
        Some(rest) => (rest, true),
        None => (s, false),
    };
    let compact: String;
    let s = if s.contains('-') || s.contains(':') {
        compact = s.replace(['-', ':'], "");
        compact.as_str()
    } else {
        s
    };
    let (d, t) = s.split_once(['T', 't'])?;
    let date = parse_date(d)?;
    // Seconds may be missing, or come with a fraction.
    let t = t.split('.').next().unwrap_or(t);
    let h = digits(t, 0, 2)?;
    let mi = digits(t, 2, 2)?;
    let sec = if t.len() >= 6 { digits(t, 4, 2)? } else { 0 };
    // A leap second becomes the last whole one.
    let time = Time::new(h.min(23) as i8, mi.min(59) as i8, sec.min(59) as i8, 0).ok()?;
    Some((date.to_datetime(time), utc))
}

pub fn format_date(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

pub fn format_datetime(dt: DateTime, utc: bool) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}{}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        if utc { "Z" } else { "" }
    )
}

/// A DURATION: nominal days (which follow the wall clock across a DST
/// change) and exact seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Duration {
    pub days: i64,
    pub seconds: i64,
}

impl Duration {
    pub const ZERO: Duration = Duration { days: 0, seconds: 0 };

    pub fn days(days: i64) -> Duration {
        Duration { days, seconds: 0 }
    }

    pub fn seconds(seconds: i64) -> Duration {
        Duration { days: 0, seconds }
    }

    pub fn minutes(m: i64) -> Duration {
        Duration::seconds(m * 60)
    }

    pub fn is_negative(&self) -> bool {
        self.days < 0 || (self.days == 0 && self.seconds < 0)
    }

    /// Its length if days were 24 hours.
    pub fn total_seconds(&self) -> i64 {
        self.days * 86_400 + self.seconds
    }

    pub fn negated(self) -> Duration {
        Duration { days: -self.days, seconds: -self.seconds }
    }
}

/// `[+-]P[nW][nD][T[nH][nM][nS]]` (weeks and days nominal, the rest exact).
/// Writers mix weeks with days and leave off parts, so all of that is
/// taken.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (neg, s) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let s = s.strip_prefix(['P', 'p'])?;
    let mut days = 0i64;
    let mut secs = 0i64;
    let mut in_time = false;
    let mut num = String::new();
    let mut any = false;
    for c in s.chars() {
        match c.to_ascii_uppercase() {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            // Fractions (from careless writers) are dropped.
            '.' | ',' => num.push('.'),
            u => {
                let n: f64 = num.parse().ok()?;
                num.clear();
                any = true;
                match (u, in_time) {
                    ('W', _) => days += (n * 7.0) as i64,
                    ('D', _) => days += n as i64,
                    ('H', true) => secs += (n * 3600.0) as i64,
                    ('M', true) => secs += (n * 60.0) as i64,
                    ('S', true) => secs += n as i64,
                    // Months and years have no place in a DURATION; an
                    // `M` before `T` is most likely minutes.
                    ('M', false) => secs += (n * 60.0) as i64,
                    ('H', false) => secs += (n * 3600.0) as i64,
                    ('S', false) => secs += n as i64,
                    _ => return None,
                }
            }
        }
    }
    if !any {
        return None;
    }
    let d = Duration { days, seconds: secs };
    Some(if neg { d.negated() } else { d })
}

/// A DURATION as RFC 5545 writes it: `-PT15M`, `P1D`, `PT1H30M`, `P1W`.
pub fn format_duration(d: Duration) -> String {
    let neg = d.is_negative();
    let (days, mut secs) = if neg { (-d.days, -d.seconds) } else { (d.days, d.seconds) };
    let mut s = String::from(if neg { "-P" } else { "P" });
    if days > 0 && secs == 0 && days % 7 == 0 {
        s.push_str(&format!("{}W", days / 7));
        return s;
    }
    if days > 0 {
        s.push_str(&format!("{days}D"));
    }
    if secs > 0 || days == 0 {
        s.push('T');
        let h = secs / 3600;
        secs %= 3600;
        let m = secs / 60;
        secs %= 60;
        if h > 0 {
            s.push_str(&format!("{h}H"));
        }
        if m > 0 {
            s.push_str(&format!("{m}M"));
        }
        if secs > 0 || (h == 0 && m == 0) {
            s.push_str(&format!("{secs}S"));
        }
    }
    s
}

/// A UTC-OFFSET: `+0100`, `-0530`, `+013000`; seconds east of UTC.
pub fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    let (sign, rest) = match s.as_bytes().first()? {
        b'-' => (-1, &s[1..]),
        b'+' => (1, &s[1..]),
        _ => (1, s),
    };
    let rest = rest.replace(':', "");
    let h = digits(&rest, 0, 2)?;
    let m = digits(&rest, 2, 2).unwrap_or(0);
    let sec = digits(&rest, 4, 2).unwrap_or(0);
    Some(sign * (h * 3600 + m * 60 + sec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, datetime};

    #[test]
    fn text_round_trips() {
        let t = "Lunch; then, a walk\\\nback";
        assert_eq!(escape_text(t), "Lunch\\; then\\, a walk\\\\\\nback");
        assert_eq!(unescape_text(&escape_text(t)), t);
        assert_eq!(unescape_text("a\\Nb\\:c"), "a\nb:c");
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(parse_date("20261017"), Some(date(2026, 10, 17)));
        assert_eq!(parse_date("2026-10-17"), Some(date(2026, 10, 17)));
        assert_eq!(parse_date("20260230"), None);
        assert_eq!(parse_datetime("20261017T093000Z"), Some((datetime(2026, 10, 17, 9, 30, 0, 0), true)));
        assert_eq!(parse_datetime("20261017T0930"), Some((datetime(2026, 10, 17, 9, 30, 0, 0), false)));
        assert_eq!(parse_datetime("2026-10-17T09:30:00"), Some((datetime(2026, 10, 17, 9, 30, 0, 0), false)));
        assert_eq!(format_datetime(datetime(2026, 1, 2, 3, 4, 5, 0), true), "20260102T030405Z");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("PT1H30M"), Some(Duration::seconds(5400)));
        assert_eq!(parse_duration("-PT15M"), Some(Duration::seconds(-900)));
        assert_eq!(parse_duration("P1W"), Some(Duration::days(7)));
        assert_eq!(parse_duration("-P0DT0H30M0S"), Some(Duration::seconds(-1800)));
        assert_eq!(parse_duration("P1DT12H"), Some(Duration { days: 1, seconds: 43200 }));
        assert_eq!(parse_duration("P"), None);
        for d in ["-PT15M", "P1D", "PT1H30M", "P1W", "PT0S", "-P1DT2H"] {
            assert_eq!(format_duration(parse_duration(d).unwrap()), d);
        }
    }

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("+0100"), Some(3600));
        assert_eq!(parse_offset("-0530"), Some(-19800));
        assert_eq!(parse_offset("+013015"), Some(5415));
    }
}
