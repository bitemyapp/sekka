//! Recurrence rules (RFC 5545 §3.3.10): parsed, written back, and
//! expanded over wall-clock date-times.
//!
//! Expansion covers what calendars write: `FREQ` from `MINUTELY` to
//! `YEARLY` with `INTERVAL`, `COUNT` and `UNTIL`, and `BYDAY` (with
//! ordinals), `BYMONTHDAY` (negative too), `BYMONTH`, `BYYEARDAY`,
//! `BYHOUR`, `BYMINUTE` and `BYSETPOS`, `WKST`. `BYWEEKNO` and
//! `BYSECOND` are kept and written back but not expanded. The start
//! always counts as the first occurrence.

use jiff::civil::{Date, DateTime, Time, Weekday};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Freq {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Freq {
    pub fn name(self) -> &'static str {
        match self {
            Freq::Secondly => "SECONDLY",
            Freq::Minutely => "MINUTELY",
            Freq::Hourly => "HOURLY",
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
            Freq::Yearly => "YEARLY",
        }
    }

    fn parse(s: &str) -> Option<Freq> {
        Some(match s {
            "SECONDLY" => Freq::Secondly,
            "MINUTELY" => Freq::Minutely,
            "HOURLY" => Freq::Hourly,
            "DAILY" => Freq::Daily,
            "WEEKLY" => Freq::Weekly,
            "MONTHLY" => Freq::Monthly,
            "YEARLY" => Freq::Yearly,
            _ => return None,
        })
    }
}

/// When a rule ends, as written: a date, or a date-time (UTC when the
/// start has a zone, floating when it's floating).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    Date(Date),
    DateTime { at: DateTime, utc: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<Until>,
    /// Weekdays, each with an ordinal (0: every one).
    pub by_day: Vec<(i8, Weekday)>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<i8>,
    pub by_year_day: Vec<i16>,
    pub by_week_no: Vec<i8>,
    pub by_hour: Vec<i8>,
    pub by_minute: Vec<i8>,
    pub by_second: Vec<i8>,
    pub by_set_pos: Vec<i32>,
    pub wkst: Weekday,
    /// Parts this doesn't know (`X-NAME=…`, `RSCALE`), kept in order.
    pub other: Vec<(String, String)>,
}

impl RRule {
    pub fn new(freq: Freq) -> RRule {
        RRule {
            freq,
            interval: 1,
            count: None,
            until: None,
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_month: Vec::new(),
            by_year_day: Vec::new(),
            by_week_no: Vec::new(),
            by_hour: Vec::new(),
            by_minute: Vec::new(),
            by_second: Vec::new(),
            by_set_pos: Vec::new(),
            wkst: Weekday::Monday,
            other: Vec::new(),
        }
    }

    /// Parse `FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261231T235959Z`; parts
    /// that don't read are dropped, and a rule without a `FREQ` is none.
    pub fn parse(s: &str) -> Option<RRule> {
        let s = s.trim();
        let s = s.strip_prefix("RRULE:").unwrap_or(s);
        let mut rule = RRule::new(Freq::Daily);
        let mut freq = None;
        for part in s.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            let k = k.trim().to_ascii_uppercase();
            let v = v.trim();
            let up = v.to_ascii_uppercase();
            match k.as_str() {
                "FREQ" => freq = Freq::parse(&up),
                "INTERVAL" => rule.interval = v.parse::<u32>().ok().filter(|n| *n > 0).unwrap_or(1),
                "COUNT" => rule.count = v.parse().ok(),
                "UNTIL" => {
                    rule.until = if let Some((at, utc)) = crate::ics::value::parse_datetime(v) {
                        Some(Until::DateTime { at, utc })
                    } else {
                        crate::ics::value::parse_date(v).map(Until::Date)
                    }
                }
                "BYDAY" => rule.by_day = up.split(',').filter_map(parse_by_day).collect(),
                "BYMONTHDAY" => rule.by_month_day = nums(v, -31, 31),
                "BYMONTH" => rule.by_month = nums(v, 1, 12),
                "BYYEARDAY" => rule.by_year_day = nums(v, -366, 366),
                "BYWEEKNO" => rule.by_week_no = nums(v, -53, 53),
                "BYHOUR" => rule.by_hour = nums(v, 0, 23),
                "BYMINUTE" => rule.by_minute = nums(v, 0, 59),
                "BYSECOND" => rule.by_second = nums(v, 0, 60),
                "BYSETPOS" => rule.by_set_pos = nums(v, -366, 366),
                "WKST" => {
                    if let Some(w) = weekday(&up) {
                        rule.wkst = w;
                    }
                }
                _ => rule.other.push((k, v.to_owned())),
            }
        }
        rule.freq = freq?;
        Some(rule)
    }

    /// As RFC 5545 writes it.
    pub fn to_ical(&self) -> String {
        let mut parts = vec![format!("FREQ={}", self.freq.name())];
        if self.interval > 1 {
            parts.push(format!("INTERVAL={}", self.interval));
        }
        if let Some(c) = self.count {
            parts.push(format!("COUNT={c}"));
        }
        match self.until {
            Some(Until::Date(d)) => parts.push(format!("UNTIL={}", crate::ics::value::format_date(d))),
            Some(Until::DateTime { at, utc }) => {
                parts.push(format!("UNTIL={}", crate::ics::value::format_datetime(at, utc)))
            }
            None => {}
        }
        let list = |v: &[i8]| v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
        if !self.by_month.is_empty() {
            parts.push(format!("BYMONTH={}", list(&self.by_month)));
        }
        if !self.by_week_no.is_empty() {
            parts.push(format!("BYWEEKNO={}", list(&self.by_week_no)));
        }
        if !self.by_year_day.is_empty() {
            parts.push(format!(
                "BYYEARDAY={}",
                self.by_year_day.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")
            ));
        }
        if !self.by_month_day.is_empty() {
            parts.push(format!("BYMONTHDAY={}", list(&self.by_month_day)));
        }
        if !self.by_day.is_empty() {
            let days: Vec<String> = self
                .by_day
                .iter()
                .map(|(n, w)| if *n == 0 { weekday_code(*w).to_owned() } else { format!("{n}{}", weekday_code(*w)) })
                .collect();
            parts.push(format!("BYDAY={}", days.join(",")));
        }
        if !self.by_hour.is_empty() {
            parts.push(format!("BYHOUR={}", list(&self.by_hour)));
        }
        if !self.by_minute.is_empty() {
            parts.push(format!("BYMINUTE={}", list(&self.by_minute)));
        }
        if !self.by_second.is_empty() {
            parts.push(format!("BYSECOND={}", list(&self.by_second)));
        }
        if !self.by_set_pos.is_empty() {
            parts.push(format!(
                "BYSETPOS={}",
                self.by_set_pos.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")
            ));
        }
        if self.wkst != Weekday::Monday {
            parts.push(format!("WKST={}", weekday_code(self.wkst)));
        }
        for (k, v) in &self.other {
            parts.push(format!("{k}={v}"));
        }
        parts.join(";")
    }

    /// Whether expansion reads all of it (what it can't, it ignores).
    pub fn fully_supported(&self) -> bool {
        self.by_week_no.is_empty() && self.by_second.is_empty() && self.freq != Freq::Secondly
    }

    /// The wall-clock date-times of the rule from `start` (the first),
    /// in order, up to `until` (inclusive: the rule's own end, already in
    /// the start's terms) and while `f` says to go on; at most `limit`
    /// candidate periods are looked at, so a rule that matches nothing
    /// ends.
    pub fn expand(&self, start: DateTime, until: Option<DateTime>, mut f: impl FnMut(DateTime) -> bool) {
        let mut emitted: u32 = 0;
        if self.count == Some(0) {
            return;
        }
        if until.is_some_and(|u| start > u) {
            return;
        }
        if !f(start) {
            return;
        }
        emitted += 1;
        if self.count.is_some_and(|c| emitted >= c) {
            return;
        }
        let step = self.interval.max(1) as i64;
        let times = self.times_of_day(start.time());
        let mut periods = 0u32;
        let mut empty_run = 0u32;
        let mut k: i64 = 0;
        loop {
            periods += 1;
            if periods > 200_000 || empty_run > 5_000 {
                return;
            }
            let mut cands: Vec<DateTime> = match self.freq {
                Freq::Yearly => {
                    let y = start.year() as i64 + k * step;
                    if y > 9999 {
                        return;
                    }
                    with_times(self.yearly(y as i16, start.date()), &times)
                }
                Freq::Monthly => {
                    let m0 = start.year() as i64 * 12 + (start.month() as i64 - 1) + k * step;
                    let (y, m) = (m0.div_euclid(12), m0.rem_euclid(12) + 1);
                    if y > 9999 {
                        return;
                    }
                    with_times(self.monthly(y as i16, m as i8, start.date()), &times)
                }
                Freq::Weekly => {
                    let wstart = week_start(start.date(), self.wkst);
                    let Some(ws) = add_days(wstart, k * step * 7) else { return };
                    with_times(self.weekly(ws, start.date()), &times)
                }
                Freq::Daily => {
                    let Some(d) = add_days(start.date(), k * step) else { return };
                    if self.date_matches(d) { with_times(vec![d], &times) } else { Vec::new() }
                }
                Freq::Hourly | Freq::Minutely | Freq::Secondly => {
                    let secs = match self.freq {
                        Freq::Hourly => 3600,
                        Freq::Minutely => 60,
                        _ => 1,
                    } * k
                        * step;
                    let Ok(at) = start.checked_add(jiff::SignedDuration::from_secs(secs)) else { return };
                    let ok = self.date_matches(at.date())
                        && (self.by_hour.is_empty() || self.by_hour.contains(&at.hour()))
                        && (self.by_minute.is_empty() || self.by_minute.contains(&at.minute()));
                    if ok { vec![at] } else { Vec::new() }
                }
            };
            k += 1;
            cands.sort();
            cands.dedup();
            if !self.by_set_pos.is_empty() && !cands.is_empty() {
                let n = cands.len() as i32;
                let mut picked: Vec<DateTime> = self
                    .by_set_pos
                    .iter()
                    .filter_map(|&p| {
                        let i = if p > 0 { p - 1 } else { n + p };
                        usize::try_from(i).ok().and_then(|i| cands.get(i))
                    })
                    .copied()
                    .collect();
                picked.sort();
                picked.dedup();
                cands = picked;
            }
            let mut any = false;
            for c in cands {
                if c <= start {
                    continue;
                }
                if until.is_some_and(|u| c > u) {
                    return;
                }
                any = true;
                if !f(c) {
                    return;
                }
                emitted += 1;
                if self.count.is_some_and(|n| emitted >= n) {
                    return;
                }
            }
            empty_run = if any { 0 } else { empty_run + 1 };
        }
    }

    fn times_of_day(&self, t: Time) -> Vec<Time> {
        if !matches!(self.freq, Freq::Daily | Freq::Weekly | Freq::Monthly | Freq::Yearly)
            || (self.by_hour.is_empty() && self.by_minute.is_empty())
        {
            return vec![t];
        }
        let hours: Vec<i8> = if self.by_hour.is_empty() { vec![t.hour()] } else { self.by_hour.clone() };
        let minutes: Vec<i8> = if self.by_minute.is_empty() { vec![t.minute()] } else { self.by_minute.clone() };
        let mut out = Vec::new();
        for h in &hours {
            for m in &minutes {
                if let Ok(t) = Time::new(*h, *m, t.second(), 0) {
                    out.push(t);
                }
            }
        }
        out.sort();
        out
    }

    /// Whether `d` passes the rule's day filters (for DAILY and finer).
    fn date_matches(&self, d: Date) -> bool {
        (self.by_month.is_empty() || self.by_month.contains(&d.month()))
            && (self.by_month_day.is_empty()
                || self.by_month_day.iter().any(|&md| month_day(d.year(), d.month(), md) == Some(d.day())))
            && (self.by_day.is_empty() || self.by_day.iter().any(|(_, w)| *w == d.weekday()))
            && (self.by_year_day.is_empty() || self.by_year_day.iter().any(|&yd| year_day(d.year(), yd) == Some(d)))
    }

    fn weekly(&self, week: Date, start: Date) -> Vec<Date> {
        let days: Vec<Weekday> =
            if self.by_day.is_empty() { vec![start.weekday()] } else { self.by_day.iter().map(|(_, w)| *w).collect() };
        let mut out: Vec<Date> = days
            .iter()
            .filter_map(|w| {
                let off = (w.to_monday_zero_offset() - week.weekday().to_monday_zero_offset()).rem_euclid(7);
                add_days(week, off as i64)
            })
            .filter(|d| self.by_month.is_empty() || self.by_month.contains(&d.month()))
            .collect();
        out.sort();
        out
    }

    /// The days of month `m` of year `y` the rule picks.
    fn month_days(&self, y: i16, m: i8, start: Date, default_day: bool) -> Vec<Date> {
        let by_md: Vec<Date> = self
            .by_month_day
            .iter()
            .filter_map(|&md| month_day(y, m, md))
            .filter_map(|d| Date::new(y, m, d).ok())
            .collect();
        let by_wd: Vec<Date> = self.by_day.iter().flat_map(|&(n, w)| weekdays_in_month(y, m, n, w)).collect();
        let mut out = match (self.by_month_day.is_empty(), self.by_day.is_empty()) {
            (false, false) => by_md.into_iter().filter(|d| by_wd.contains(d)).collect(),
            (false, true) => by_md,
            (true, false) => by_wd,
            (true, true) if default_day => Date::new(y, m, start.day()).ok().into_iter().collect(),
            (true, true) => Vec::new(),
        };
        out.sort();
        out.dedup();
        out
    }

    fn monthly(&self, y: i16, m: i8, start: Date) -> Vec<Date> {
        if !self.by_month.is_empty() && !self.by_month.contains(&m) {
            return Vec::new();
        }
        let mut days = self.month_days(y, m, start, true);
        if !self.by_year_day.is_empty() {
            days.retain(|d| self.by_year_day.iter().any(|&yd| year_day(y, yd) == Some(*d)));
        }
        days
    }

    fn yearly(&self, y: i16, start: Date) -> Vec<Date> {
        let mut out: Vec<Date> = Vec::new();
        if !self.by_year_day.is_empty() {
            out = self.by_year_day.iter().filter_map(|&yd| year_day(y, yd)).collect();
            out.retain(|d| {
                (self.by_month.is_empty() || self.by_month.contains(&d.month()))
                    && (self.by_day.is_empty() || self.by_day.iter().any(|(_, w)| *w == d.weekday()))
            });
        } else if !self.by_month.is_empty() {
            for &m in &self.by_month {
                out.extend(self.month_days(y, m, start, true));
            }
        } else if !self.by_day.is_empty() && self.by_month_day.is_empty() {
            // Weekdays of the year: `BYDAY=20MO` is the twentieth Monday.
            for &(n, w) in &self.by_day {
                out.extend(weekdays_in_year(y, n, w));
            }
        } else if !self.by_month_day.is_empty() {
            for m in 1..=12 {
                out.extend(self.month_days(y, m, start, false));
            }
        } else if let Ok(d) = Date::new(y, start.month(), start.day()) {
            out.push(d);
        }
        out.sort();
        out.dedup();
        out
    }
}

fn with_times(days: Vec<Date>, times: &[Time]) -> Vec<DateTime> {
    let mut out = Vec::with_capacity(days.len() * times.len());
    for d in days {
        for t in times {
            out.push(d.to_datetime(*t));
        }
    }
    out
}

/// A list of numbers within `lo..=hi` (zero only where it means
/// something: hours, minutes, seconds).
fn nums<T: TryFrom<i32>>(s: &str, lo: i32, hi: i32) -> Vec<T> {
    s.split(',')
        .filter_map(|p| p.trim().trim_start_matches('+').parse::<i32>().ok())
        .filter(|n| *n >= lo && *n <= hi && (*n != 0 || lo == 0))
        .filter_map(|n| T::try_from(n).ok())
        .collect()
}

/// `MO` … `SU`.
pub fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

pub fn weekday_code(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "MO",
        Weekday::Tuesday => "TU",
        Weekday::Wednesday => "WE",
        Weekday::Thursday => "TH",
        Weekday::Friday => "FR",
        Weekday::Saturday => "SA",
        Weekday::Sunday => "SU",
    }
}

fn parse_by_day(s: &str) -> Option<(i8, Weekday)> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let (n, code) = s.split_at(s.len() - 2);
    let w = weekday(code)?;
    let n = if n.is_empty() { 0 } else { n.trim_start_matches('+').parse::<i8>().ok()? };
    Some((n, w))
}

pub fn add_days(d: Date, n: i64) -> Option<Date> {
    d.checked_add(jiff::Span::new().try_days(n).ok()?).ok()
}

/// The first day of `d`'s week, weeks starting on `wkst`.
pub fn week_start(d: Date, wkst: Weekday) -> Date {
    let back = (d.weekday().to_monday_zero_offset() - wkst.to_monday_zero_offset()).rem_euclid(7);
    add_days(d, -(back as i64)).unwrap_or(d)
}

fn days_in_month(y: i16, m: i8) -> i8 {
    Date::new(y, m, 1).map(|d| d.days_in_month()).unwrap_or(30)
}

/// Day `md` of the month (negative from its end), if the month has it.
fn month_day(y: i16, m: i8, md: i8) -> Option<i8> {
    let n = days_in_month(y, m);
    let d = if md > 0 { md } else { n + md + 1 };
    (1..=n).contains(&d).then_some(d)
}

/// Day `yd` of the year (negative from its end).
fn year_day(y: i16, yd: i16) -> Option<Date> {
    let jan1 = Date::new(y, 1, 1).ok()?;
    let len: i16 = if jan1.in_leap_year() { 366 } else { 365 };
    let n = if yd > 0 { yd } else { len + yd + 1 };
    if !(1..=len).contains(&n) {
        return None;
    }
    add_days(jan1, (n - 1) as i64)
}

/// The `w`s of a month: every one (`n` 0), the `n`th, or the `-n`th from
/// the end.
fn weekdays_in_month(y: i16, m: i8, n: i8, w: Weekday) -> Vec<Date> {
    let Ok(first) = Date::new(y, m, 1) else { return Vec::new() };
    let off = (w.to_monday_zero_offset() - first.weekday().to_monday_zero_offset()).rem_euclid(7);
    let all: Vec<Date> =
        (0..5i64).filter_map(|i| add_days(first, off as i64 + 7 * i)).filter(|d| d.month() == m).collect();
    pick(all, n)
}

fn weekdays_in_year(y: i16, n: i8, w: Weekday) -> Vec<Date> {
    let Ok(first) = Date::new(y, 1, 1) else { return Vec::new() };
    let off = (w.to_monday_zero_offset() - first.weekday().to_monday_zero_offset()).rem_euclid(7);
    let all: Vec<Date> =
        (0..54i64).filter_map(|i| add_days(first, off as i64 + 7 * i)).filter(|d| d.year() == y).collect();
    pick(all, n)
}

fn pick(all: Vec<Date>, n: i8) -> Vec<Date> {
    match n {
        0 => all,
        n if n > 0 => all.get(n as usize - 1).copied().into_iter().collect(),
        n => {
            let i = all.len() as i64 + n as i64;
            if i >= 0 { vec![all[i as usize]] } else { Vec::new() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, datetime};

    fn take(rule: &str, start: DateTime, n: usize) -> Vec<DateTime> {
        let r = RRule::parse(rule).expect(rule);
        let mut out = Vec::new();
        r.expand(start, None, |d| {
            out.push(d);
            out.len() < n
        });
        out
    }

    fn dates(v: &[DateTime]) -> Vec<String> {
        v.iter().map(|d| d.date().to_string()).collect()
    }

    #[test]
    fn parses_and_writes() {
        let r = RRule::parse("FREQ=MONTHLY;INTERVAL=2;BYDAY=-1FR,2MO;UNTIL=20261231T235959Z;X-FOO=bar").unwrap();
        assert_eq!(r.freq, Freq::Monthly);
        assert_eq!(r.interval, 2);
        assert_eq!(r.by_day, vec![(-1, Weekday::Friday), (2, Weekday::Monday)]);
        assert_eq!(r.to_ical(), "FREQ=MONTHLY;INTERVAL=2;UNTIL=20261231T235959Z;BYDAY=-1FR,2MO;X-FOO=bar");
        assert!(RRule::parse("INTERVAL=2").is_none());
    }

    #[test]
    fn daily_with_count() {
        let v = take("FREQ=DAILY;COUNT=3", datetime(2026, 10, 30, 9, 0, 0, 0), 10);
        assert_eq!(dates(&v), ["2026-10-30", "2026-10-31", "2026-11-01"]);
    }

    #[test]
    fn weekly_on_days_with_interval() {
        // Every other week on Monday and Thursday, from a Monday.
        let v = take("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH", datetime(2026, 10, 5, 10, 0, 0, 0), 5);
        assert_eq!(dates(&v), ["2026-10-05", "2026-10-08", "2026-10-19", "2026-10-22", "2026-11-02"]);
        // WKST changes which weeks count (RFC 5545's own example).
        let a = take("FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=MO", datetime(1997, 8, 5, 9, 0, 0, 0), 9);
        assert_eq!(dates(&a), ["1997-08-05", "1997-08-10", "1997-08-19", "1997-08-24"]);
        let b = take("FREQ=WEEKLY;INTERVAL=2;COUNT=4;BYDAY=TU,SU;WKST=SU", datetime(1997, 8, 5, 9, 0, 0, 0), 9);
        assert_eq!(dates(&b), ["1997-08-05", "1997-08-17", "1997-08-19", "1997-08-31"]);
    }

    #[test]
    fn monthly_by_day_and_month_day() {
        let last_fri = take("FREQ=MONTHLY;BYDAY=-1FR", datetime(2026, 10, 30, 9, 0, 0, 0), 3);
        assert_eq!(dates(&last_fri), ["2026-10-30", "2026-11-27", "2026-12-25"]);
        // The 31st skips months without one.
        let d31 = take("FREQ=MONTHLY;BYMONTHDAY=31", datetime(2026, 1, 31, 9, 0, 0, 0), 4);
        assert_eq!(dates(&d31), ["2026-01-31", "2026-03-31", "2026-05-31", "2026-07-31"]);
        let last = take("FREQ=MONTHLY;BYMONTHDAY=-1", datetime(2026, 1, 31, 9, 0, 0, 0), 3);
        assert_eq!(dates(&last), ["2026-01-31", "2026-02-28", "2026-03-31"]);
        // Friday the 13th.
        let f13 = take("FREQ=MONTHLY;BYDAY=FR;BYMONTHDAY=13", datetime(2026, 2, 13, 0, 0, 0, 0), 3);
        assert_eq!(dates(&f13), ["2026-02-13", "2026-03-13", "2026-11-13"]);
        // The last weekday of the month.
        let lw = take("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", datetime(2026, 10, 30, 9, 0, 0, 0), 3);
        assert_eq!(dates(&lw), ["2026-10-30", "2026-11-30", "2026-12-31"]);
    }

    #[test]
    fn yearly_rules() {
        let bday = take("FREQ=YEARLY", datetime(2024, 2, 29, 0, 0, 0, 0), 3);
        assert_eq!(dates(&bday), ["2024-02-29", "2028-02-29", "2032-02-29"]);
        let thanks = take("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH", datetime(2026, 11, 26, 0, 0, 0, 0), 3);
        assert_eq!(dates(&thanks), ["2026-11-26", "2027-11-25", "2028-11-23"]);
        let mondays20 = take("FREQ=YEARLY;BYDAY=20MO", datetime(1997, 5, 19, 9, 0, 0, 0), 3);
        assert_eq!(dates(&mondays20), ["1997-05-19", "1998-05-18", "1999-05-17"]);
        let yd = take("FREQ=YEARLY;BYYEARDAY=1,100", datetime(2026, 1, 1, 9, 0, 0, 0), 3);
        assert_eq!(dates(&yd), ["2026-01-01", "2026-04-10", "2027-01-01"]);
    }

    #[test]
    fn until_is_inclusive_and_stops() {
        let r = RRule::parse("FREQ=DAILY;UNTIL=20261020T090000").unwrap();
        let mut v = Vec::new();
        r.expand(datetime(2026, 10, 17, 9, 0, 0, 0), Some(datetime(2026, 10, 20, 9, 0, 0, 0)), |d| {
            v.push(d);
            true
        });
        assert_eq!(dates(&v), ["2026-10-17", "2026-10-18", "2026-10-19", "2026-10-20"]);
    }

    #[test]
    fn hours_within_days() {
        let v = take("FREQ=DAILY;BYHOUR=9,17;COUNT=4", datetime(2026, 10, 17, 9, 0, 0, 0), 9);
        let s: Vec<String> = v.iter().map(|d| d.to_string()).collect();
        assert_eq!(s, ["2026-10-17T09:00:00", "2026-10-17T17:00:00", "2026-10-18T09:00:00", "2026-10-18T17:00:00"]);
    }

    #[test]
    fn rules_that_match_nothing_end() {
        let v = take("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30", datetime(2026, 1, 1, 0, 0, 0, 0), 5);
        assert_eq!(v.len(), 1);
        assert_eq!(week_start(date(2026, 10, 17), Weekday::Monday), date(2026, 10, 12));
        assert_eq!(week_start(date(2026, 10, 17), Weekday::Sunday), date(2026, 10, 11));
    }
}
