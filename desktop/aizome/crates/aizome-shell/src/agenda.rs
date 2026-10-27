//! What the clock's panel shows under the month: the next events from
//! Koyomi's calendars (`aizome-koyomi --upcoming --json`, see
//! `docs/koyomi.md`) and the world clocks Tokei keeps
//! (`$XDG_STATE_HOME/aizome/tokei/state.json`). Read on a thread when the
//! panel opens and each minute while it shows; without Koyomi the events
//! are left out, without Tokei's state the world clocks.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aizome_config::ClockFormat;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::Deserialize;

/// An event coming up, as Koyomi reports it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Event {
    pub title: String,
    /// RFC 3339, with the local offset.
    pub start: String,
    pub end: String,
    /// The local day it starts on, `YYYY-MM-DD`.
    pub date: String,
    pub all_day: bool,
    pub in_progress: bool,
    pub location: String,
    /// Its calendar's colour, `#rrggbb`.
    pub color: String,
}

#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    events: Vec<Event>,
}

/// A world clock: a place's name and its time zone.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Place {
    pub name: String,
    pub zone: String,
}

#[derive(Deserialize)]
struct TokeiState {
    #[serde(default)]
    places: Vec<Place>,
}

/// The events and world clocks, as last read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Agenda {
    /// `None` where there's no Koyomi to ask.
    pub events: Option<Vec<Event>>,
    pub places: Vec<Place>,
}

/// Read both (on a thread: it runs Koyomi).
pub fn read() -> Agenda {
    Agenda { events: upcoming(), places: places(&tokei_state()) }
}

/// Today's and tomorrow's events, from Koyomi.
pub fn upcoming() -> Option<Vec<Event>> {
    let out = Command::new("aizome-koyomi")
        .args(["--upcoming", "--json", "--days", "2", "--limit", "8"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_upcoming(&out.stdout)
}

pub fn parse_upcoming(json: &[u8]) -> Option<Vec<Event>> {
    serde_json::from_slice::<Report>(json).ok().map(|r| r.events)
}

/// Where Tokei keeps its world clocks.
pub fn tokei_state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("aizome/tokei/state.json")
}

/// Tokei's world clocks (none if it has never saved any).
pub fn places(path: &Path) -> Vec<Place> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<TokeiState>(&b).ok())
        .map(|s| s.places)
        .unwrap_or_default()
}

/// An event's row in the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct EventRow {
    /// `Now`, `14:00`, `All day`, or `Sat 09:30` for another day.
    pub when: String,
    pub title: String,
    /// Its calendar's colour, `0xrrggbb`.
    pub color: Option<u32>,
    pub now: bool,
    /// The day, to open Koyomi at.
    pub date: String,
}

/// A world clock's row.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneRow {
    pub name: String,
    pub time: String,
    /// How it stands to here: `+9h`, `−5:30`, `same time`, and the day
    /// when it differs.
    pub note: String,
}

fn hm(h: i8, m: i8, format: ClockFormat) -> String {
    let (h, m) = (h as u32, m as u32);
    match format {
        ClockFormat::H24 => format!("{h:02}:{m:02}"),
        ClockFormat::H12 => {
            format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
        }
    }
}

/// The rows for the events still to come today and tomorrow, at most
/// `limit`; `today` is `YYYY-MM-DD`.
pub fn event_rows(events: &[Event], today: &str, format: ClockFormat, limit: usize) -> Vec<EventRow> {
    events
        .iter()
        .take(limit)
        .map(|e| {
            let day = if e.date == today {
                String::new()
            } else {
                e.date.parse::<jiff::civil::Date>().map(|d| format!("{} ", d.strftime("%a"))).unwrap_or_default()
            };
            let time = e
                .start
                .get(11..16)
                .and_then(|t| t.parse::<jiff::civil::Time>().ok())
                .map(|t| hm(t.hour(), t.minute(), format));
            let when = if e.in_progress {
                "Now".to_owned()
            } else if e.all_day {
                if day.is_empty() { "All day".to_owned() } else { day.trim_end().to_owned() }
            } else {
                format!("{day}{}", time.unwrap_or_default())
            };
            EventRow {
                when,
                title: e.title.clone(),
                color: e.color.strip_prefix('#').and_then(|c| u32::from_str_radix(c, 16).ok()),
                now: e.in_progress,
                date: e.date.clone(),
            }
        })
        .collect()
}

/// The world clocks' rows at `now`, against the time zone `here`.
pub fn zone_rows(places: &[Place], now: Timestamp, here: &TimeZone, format: ClockFormat) -> Vec<ZoneRow> {
    let local = now.to_zoned(here.clone());
    places
        .iter()
        .filter_map(|p| {
            let tz = TimeZone::get(&p.zone).ok()?;
            let there = now.to_zoned(tz);
            let diff = there.offset().seconds() - local.offset().seconds();
            let mut note = if diff == 0 {
                "same time".to_owned()
            } else {
                let (sign, a) = if diff < 0 { ('−', -diff) } else { ('+', diff) };
                if a % 3600 == 0 {
                    format!("{sign}{}h", a / 3600)
                } else {
                    format!("{sign}{}:{:02}", a / 3600, a % 3600 / 60)
                }
            };
            match there.date().cmp(&local.date()) {
                std::cmp::Ordering::Greater => note.push_str(", tomorrow"),
                std::cmp::Ordering::Less => note.push_str(", yesterday"),
                std::cmp::Ordering::Equal => {}
            }
            Some(ZoneRow { name: p.name.clone(), time: hm(there.hour(), there.minute(), format), note })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KOYOMI: &str = r##"{
      "version": 1, "now": "2026-10-17T21:42:00-05:00", "time_zone": "America/Chicago", "days": 2,
      "events": [
        {"title": "Standup", "start": "2026-10-17T21:30:00-05:00", "end": "2026-10-17T22:00:00-05:00", "date": "2026-10-17",
         "all_day": false, "in_progress": true, "location": "", "calendar": "work", "calendar_name": "Work",
         "dye": "hanada", "color": "#3a6ea5", "uid": "a", "recurring": true},
        {"title": "Dentist", "start": "2026-10-18T09:30:00-05:00", "end": "2026-10-18T10:00:00-05:00", "date": "2026-10-18",
         "all_day": false, "in_progress": false, "location": "Main St", "calendar": "home", "calendar_name": "Home",
         "dye": "kohaku", "color": "#d9a441", "uid": "b", "recurring": false},
        {"title": "Mum's birthday", "start": "2026-10-18T00:00:00-05:00", "end": "2026-10-19T00:00:00-05:00", "date": "2026-10-18",
         "all_day": true, "in_progress": false, "location": "", "calendar": "home", "calendar_name": "Home",
         "dye": "kohaku", "color": "#d9a441", "uid": "c", "recurring": true}
      ]}"##;

    #[test]
    fn koyomis_report_reads_as_rows() {
        let events = parse_upcoming(KOYOMI.as_bytes()).expect("read");
        let rows = event_rows(&events, "2026-10-17", ClockFormat::H24, 8);
        let when: Vec<&str> = rows.iter().map(|r| r.when.as_str()).collect();
        assert_eq!(when, ["Now", "Sun 09:30", "Sun"]);
        assert_eq!(rows[0].color, Some(0x3a6ea5));
        assert!(rows[0].now);
        assert_eq!(event_rows(&events, "2026-10-18", ClockFormat::H12, 8)[1].when, "9:30 AM");
        assert_eq!(event_rows(&events, "2026-10-17", ClockFormat::H24, 1).len(), 1);
        assert!(parse_upcoming(b"not json").is_none());
    }

    #[test]
    fn world_clocks_say_how_they_stand_to_here() {
        let now: Timestamp = "2026-10-18T02:42:00Z".parse().unwrap();
        let here = TimeZone::get("America/Chicago").unwrap();
        let places = vec![
            Place { name: "Tokyo".into(), zone: "Asia/Tokyo".into() },
            Place { name: "Mumbai".into(), zone: "Asia/Kolkata".into() },
            Place { name: "Chicago".into(), zone: "America/Chicago".into() },
            Place { name: "Nowhere".into(), zone: "Not/AZone".into() },
        ];
        let rows = zone_rows(&places, now, &here, ClockFormat::H24);
        assert_eq!(rows.len(), 3, "an unknown zone is left out");
        assert_eq!((rows[0].time.as_str(), rows[0].note.as_str()), ("11:42", "+14h, tomorrow"));
        assert_eq!((rows[1].time.as_str(), rows[1].note.as_str()), ("08:12", "+10:30, tomorrow"));
        assert_eq!((rows[2].time.as_str(), rows[2].note.as_str()), ("21:42", "same time"));
    }

    #[test]
    fn tokeis_places_come_from_its_state() {
        let dir = std::env::temp_dir().join(format!("aizome-agenda-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(
            &path,
            r#"{"page":"world","places":[{"name":"London","zone":"Europe/London"}],"timer_secs":300}"#,
        )
        .unwrap();
        assert_eq!(places(&path), [Place { name: "London".into(), zone: "Europe/London".into() }]);
        assert!(places(&dir.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
