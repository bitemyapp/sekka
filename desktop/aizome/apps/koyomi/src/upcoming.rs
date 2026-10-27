//! The next events, for the shell's clock panel and for scripts:
//! `aizome-koyomi --upcoming [--json] [--days N]`. The JSON is
//! documented in `docs/koyomi.md`.

use jiff::civil::{DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::Serialize;

use crate::model::{Calendar, Event};
use crate::occur::{self, Index, Occurrence};

/// One event coming up (or under way).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Upcoming {
    pub title: String,
    /// RFC 3339 with the local offset; an all-day event's is its first
    /// midnight.
    pub start: String,
    pub end: String,
    /// The local day it starts on, `YYYY-MM-DD`.
    pub date: String,
    pub all_day: bool,
    /// It has started and not yet ended.
    pub in_progress: bool,
    pub location: String,
    /// The calendar's file name (without `.ics`) and its name.
    pub calendar: String,
    pub calendar_name: String,
    /// The calendar's dye (`hanada`, `kohaku`, …) and its Night colour.
    pub dye: String,
    pub color: String,
    pub uid: String,
    pub recurring: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    /// Format version: bumped when fields change meaning.
    pub version: u32,
    pub now: String,
    pub time_zone: String,
    pub days: u32,
    pub events: Vec<Upcoming>,
}

fn rfc3339(dt: DateTime, tz: &TimeZone) -> String {
    match tz.to_ambiguous_zoned(dt).compatible() {
        Ok(z) => z.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        Err(_) => dt.to_string(),
    }
}

/// The events from `now` to the end of the day `days` days on (today
/// counts as day 1), of the calendars not `hidden`, at most `limit`.
pub fn upcoming(
    cals: &[Calendar],
    hidden: &[String],
    tz: &TimeZone,
    now: Timestamp,
    days: u32,
    limit: usize,
) -> Report {
    let shown: Vec<bool> = cals.iter().map(|c| !hidden.contains(&c.id)).collect();
    let ix = Index::new(cals, tz);
    let now_local = now.to_zoned(tz.clone()).datetime();
    let today = now_local.date();
    let end_day = today.checked_add((days.max(1) as i64).days()).unwrap_or(today);
    let to = end_day.to_datetime(Time::midnight());
    let v: Vec<Occurrence> = occur::occurrences(cals, &ix, &shown, tz, now_local, to);
    let mut events: Vec<Upcoming> = v
        .into_iter()
        .filter(|o| !o.cancelled && (o.end > now_local || (o.end == o.start && o.start >= now_local)))
        .map(|o| {
            let cal = &cals[o.cal];
            let e: &Event = &cal.events[o.event];
            Upcoming {
                title: if e.summary.trim().is_empty() { "New Event".into() } else { e.summary.clone() },
                start: rfc3339(o.start, tz),
                end: rfc3339(o.end, tz),
                date: o.start.date().to_string(),
                all_day: o.all_day,
                in_progress: o.start <= now_local && o.end > now_local,
                location: e.location.clone(),
                calendar: cal.id.clone(),
                calendar_name: cal.name.clone(),
                dye: cal.color.key().into(),
                color: format!("#{:06x}", cal.color.hex()),
                uid: e.uid.clone(),
                recurring: e.is_recurring() || e.recurrence_id.is_some(),
            }
        })
        .collect();
    // Under way first among the day's, then by start; all-day events lead
    // their day.
    events.sort_by(|a, b| a.date.cmp(&b.date).then(b.all_day.cmp(&a.all_day)).then(a.start.cmp(&b.start)));
    events.truncate(limit);
    Report {
        version: 1,
        now: rfc3339(now_local, tz),
        time_zone: crate::tz::named(tz.clone()).id.to_string(),
        days: days.max(1),
        events,
    }
}

/// The report as lines a person reads.
pub fn text(r: &Report, h24: bool) -> String {
    let mut out = String::new();
    let mut last_date = String::new();
    for e in &r.events {
        if e.date != last_date {
            if !last_date.is_empty() {
                out.push('\n');
            }
            let d: jiff::civil::Date = e.date.parse().unwrap_or_default();
            out.push_str(&format!("{}\n", d.strftime("%A %-d %B")));
            last_date.clone_from(&e.date);
        }
        let when = if e.all_day {
            "all day".to_owned()
        } else {
            let t = |s: &str| {
                s.get(11..16)
                    .and_then(|hm| hm.parse::<Time>().ok())
                    .map(|t| crate::format::time(t, h24))
                    .unwrap_or_default()
            };
            format!("{}–{}", t(&e.start), t(&e.end))
        };
        out.push_str(&format!("  {when:<13} {}  ({})", e.title, e.calendar_name));
        if !e.location.is_empty() {
            out.push_str(&format!(" · {}", e.location.lines().next().unwrap_or("")));
        }
        out.push('\n');
    }
    if r.events.is_empty() {
        out.push_str("Nothing coming up.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::datetime;

    #[test]
    fn upcoming_lists_what_is_next() {
        let (cal, _) = Calendar::parse(
            "work",
            "BEGIN:VCALENDAR\nX-WR-CALNAME:Work\nBEGIN:VEVENT\nUID:a\nDTSTART:20261017T090000Z\nDTEND:20261017T100000Z\nSUMMARY:Past\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nDTSTART:20261017T150000Z\nDTEND:20261017T170000Z\nSUMMARY:Now\nEND:VEVENT\nBEGIN:VEVENT\nUID:c\nDTSTART;VALUE=DATE:20261018\nSUMMARY:Tomorrow\nEND:VEVENT\nBEGIN:VEVENT\nUID:d\nDTSTART:20261025T090000Z\nSUMMARY:Too far\nEND:VEVENT\nEND:VCALENDAR\n",
        );
        let tz = TimeZone::get("Europe/London").unwrap();
        let now = datetime(2026, 10, 17, 17, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp();
        let r = upcoming(std::slice::from_ref(&cal), &[], &tz, now, 7, 50);
        let titles: Vec<&str> = r.events.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Now", "Tomorrow"]);
        assert!(r.events[0].in_progress);
        assert_eq!(r.events[0].start, "2026-10-17T16:00:00+01:00");
        assert_eq!(r.events[1].date, "2026-10-18");
        assert_eq!(r.time_zone, "Europe/London");
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"calendar_name\":\"Work\""));
        assert!(upcoming(&[cal], &["work".into()], &tz, now, 7, 50).events.is_empty());
        assert!(text(&r, true).contains("16:00–18:00"));
    }
}
