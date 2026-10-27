//! Dates and times as Koyomi prints them.

use jiff::civil::{Date, DateTime, Time, Weekday};

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub fn month_name(m: i8) -> &'static str {
    MONTHS[(m.clamp(1, 12) - 1) as usize]
}

pub fn month_short(m: i8) -> &'static str {
    &month_name(m)[..3]
}

pub fn weekday_name(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "Monday",
        Weekday::Tuesday => "Tuesday",
        Weekday::Wednesday => "Wednesday",
        Weekday::Thursday => "Thursday",
        Weekday::Friday => "Friday",
        Weekday::Saturday => "Saturday",
        Weekday::Sunday => "Sunday",
    }
}

pub fn weekday_short(w: Weekday) -> &'static str {
    &weekday_name(w)[..3]
}

/// `09:30`, or `9:30 AM`.
pub fn time(t: Time, h24: bool) -> String {
    if h24 {
        format!("{:02}:{:02}", t.hour(), t.minute())
    } else {
        let h = t.hour();
        let (h12, ap) = match h {
            0 => (12, "AM"),
            1..=11 => (h, "AM"),
            12 => (12, "PM"),
            _ => (h - 12, "PM"),
        };
        format!("{h12}:{:02} {ap}", t.minute())
    }
}

/// An hour of the day's grid: `09:00`, or `9 AM`, `Noon`.
pub fn hour(h: i8, h24: bool) -> String {
    if h24 {
        format!("{h:02}:00")
    } else {
        match h {
            0 | 24 => "12 AM".into(),
            12 => "Noon".into(),
            1..=11 => format!("{h} AM"),
            h => format!("{} PM", h - 12),
        }
    }
}

/// `09:30–10:45` (or the 12-hour form, the first meridiem left off when
/// both share it).
pub fn time_range(start: Time, end: Time, h24: bool) -> String {
    if h24 {
        return format!("{}–{}", time(start, true), time(end, true));
    }
    let (a, b) = (time(start, false), time(end, false));
    if (start.hour() < 12) == (end.hour() < 12) {
        format!("{}–{b}", a.trim_end_matches(" AM").trim_end_matches(" PM"))
    } else {
        format!("{a}–{b}")
    }
}

/// `Saturday 17 October 2026`.
pub fn long_date(d: Date) -> String {
    format!("{} {} {} {}", weekday_name(d.weekday()), d.day(), month_name(d.month()), d.year())
}

/// `Sat 17 Oct`.
pub fn short_date(d: Date) -> String {
    format!("{} {} {}", weekday_short(d.weekday()), d.day(), month_short(d.month()))
}

/// A stretch of time, as an event's detail line: `Sat 17 Oct, 09:30–10:45`,
/// `Sat 17 Oct – Tue 20 Oct`, `Sat 17 Oct, all day`.
pub fn span(start: DateTime, end: DateTime, all_day: bool, h24: bool) -> String {
    if all_day {
        let last = if end.time() == Time::midnight() && end > start {
            end.date().yesterday().unwrap_or(end.date())
        } else {
            end.date()
        };
        if last <= start.date() {
            return format!("{}, all day", short_date(start.date()));
        }
        return format!("{} – {}", short_date(start.date()), short_date(last));
    }
    if start.date() == end.date()
        || (end.time() == Time::midnight() && end.date().yesterday().ok() == Some(start.date()))
    {
        format!("{}, {}", short_date(start.date()), time_range(start.time(), end.time(), h24))
    } else {
        format!(
            "{}, {} – {}, {}",
            short_date(start.date()),
            time(start.time(), h24),
            short_date(end.date()),
            time(end.time(), h24)
        )
    }
}

/// `45 min`, `1 h`, `1 h 30 min`, `2 days`.
pub fn minutes(m: i64) -> String {
    let m = m.abs();
    if m >= 1440 && m % 1440 == 0 {
        let d = m / 1440;
        return if d == 1 { "1 day".into() } else { format!("{d} days") };
    }
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Read a time typed by a person: `9`, `9:30`, `0930`, `9.30`, `9pm`,
/// `9:30 PM`, `21:30`, `noon`, `midnight`.
pub fn parse_time(s: &str) -> Option<Time> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "noon" | "midday" => return Time::new(12, 0, 0, 0).ok(),
        "midnight" => return Some(Time::midnight()),
        _ => {}
    }
    let (body, pm) = if let Some(b) =
        s.strip_suffix("pm").or_else(|| s.strip_suffix("p.m.")).or_else(|| s.strip_suffix('p'))
    {
        (b.trim(), Some(true))
    } else if let Some(b) = s.strip_suffix("am").or_else(|| s.strip_suffix("a.m.")).or_else(|| s.strip_suffix('a')) {
        (b.trim(), Some(false))
    } else {
        (s.as_str(), None)
    };
    let (h, m): (i8, i8) = if let Some((h, m)) = body.split_once([':', '.', 'h']) {
        (h.trim().parse().ok()?, if m.trim().is_empty() { 0 } else { m.trim().parse().ok()? })
    } else if body.len() >= 3 && body.chars().all(|c| c.is_ascii_digit()) {
        let n: i32 = body.parse().ok()?;
        ((n / 100) as i8, (n % 100) as i8)
    } else {
        (body.parse().ok()?, 0)
    };
    let h = match pm {
        Some(true) if (1..12).contains(&h) => h + 12,
        Some(false) if h == 12 => 0,
        Some(_) if !(1..=12).contains(&h) => return None,
        _ => h,
    };
    Time::new(h, m, 0, 0).ok()
}

/// Read a date typed by a person: `2026-10-17`, `17/10/2026`,
/// `17 Oct 2026`, `Oct 17 2026`, `17 Oct` (this year), `today`,
/// `tomorrow`.
pub fn parse_date(s: &str, today: Date) -> Option<Date> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "today" => return Some(today),
        "tomorrow" => return today.tomorrow().ok(),
        "yesterday" => return today.yesterday().ok(),
        _ => {}
    }
    if let Ok(d) = s.parse::<Date>() {
        return Some(d);
    }
    let parts: Vec<&str> = s
        .split(|c: char| c == '/' || c == '-' || c == '.' || c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let month_of = |p: &str| {
        MONTHS.iter().position(|m| p.len() >= 3 && m.to_ascii_lowercase().starts_with(p)).map(|i| i as i8 + 1)
    };
    let num = |p: &str| p.parse::<i32>().ok();
    let (y, m, d) = match parts.as_slice() {
        // 17 oct 2026 / oct 17 2026 / 17 oct / oct 17
        [a, b, rest @ ..] if month_of(b).is_some() || month_of(a).is_some() => {
            let (day, mon) = if let Some(m) = month_of(b) { (num(a)?, m) } else { (num(b)?, month_of(a)?) };
            let year = rest.first().and_then(|y| num(y)).unwrap_or(today.year() as i32);
            (year, mon as i32, day)
        }
        // 2026/10/17
        [a, b, c] if a.len() == 4 => (num(a)?, num(b)?, num(c)?),
        // 17/10/2026 (day first, as most of the world writes it)
        [a, b, c] => {
            let y = num(c)?;
            (if y < 100 { 2000 + y } else { y }, num(b)?, num(a)?)
        }
        [a, b] => (today.year() as i32, num(b)?, num(a)?),
        _ => return None,
    };
    Date::new(i16::try_from(y).ok()?, i8::try_from(m).ok()?, i8::try_from(d).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, datetime, time as t};

    #[test]
    fn prints() {
        assert_eq!(time(t(9, 5, 0, 0), true), "09:05");
        assert_eq!(time(t(21, 30, 0, 0), false), "9:30 PM");
        assert_eq!(time(t(0, 15, 0, 0), false), "12:15 AM");
        assert_eq!(time_range(t(9, 0, 0, 0), t(10, 30, 0, 0), false), "9:00–10:30 AM");
        assert_eq!(hour(12, false), "Noon");
        assert_eq!(
            span(datetime(2026, 10, 17, 0, 0, 0, 0), datetime(2026, 10, 18, 0, 0, 0, 0), true, true),
            "Sat 17 Oct, all day"
        );
        assert_eq!(
            span(datetime(2026, 10, 17, 0, 0, 0, 0), datetime(2026, 10, 21, 0, 0, 0, 0), true, true),
            "Sat 17 Oct – Tue 20 Oct"
        );
        assert_eq!(
            span(datetime(2026, 10, 17, 9, 30, 0, 0), datetime(2026, 10, 17, 10, 45, 0, 0), false, true),
            "Sat 17 Oct, 09:30–10:45"
        );
        assert_eq!(minutes(90), "1 h 30 min");
        assert_eq!(minutes(2880), "2 days");
    }

    #[test]
    fn reads_what_people_type() {
        assert_eq!(parse_time("9"), Some(t(9, 0, 0, 0)));
        assert_eq!(parse_time("9:30pm"), Some(t(21, 30, 0, 0)));
        assert_eq!(parse_time("12 am"), Some(t(0, 0, 0, 0)));
        assert_eq!(parse_time("0930"), Some(t(9, 30, 0, 0)));
        assert_eq!(parse_time("21.45"), Some(t(21, 45, 0, 0)));
        assert_eq!(parse_time("25:00"), None);
        let today = date(2026, 10, 17);
        assert_eq!(parse_date("2026-10-20", today), Some(date(2026, 10, 20)));
        assert_eq!(parse_date("20/10/2026", today), Some(date(2026, 10, 20)));
        assert_eq!(parse_date("20 Oct", today), Some(date(2026, 10, 20)));
        assert_eq!(parse_date("Oct 20, 2027", today), Some(date(2027, 10, 20)));
        assert_eq!(parse_date("tomorrow", today), Some(date(2026, 10, 18)));
        assert_eq!(parse_date("31/02/2026", today), None);
    }
}
