//! Calendars and events as Koyomi keeps them, read from and written to
//! iCalendar components. What Koyomi doesn't use (attendees, organisers,
//! Apple's and Google's own properties, to-dos) is kept as it was read
//! and written back unchanged.

use std::sync::Arc;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use crate::ics::value::{self, Duration};
use crate::ics::{Component, Prop};
use crate::rrule::{RRule, Until};
use crate::tz::{Tz, Zones};

/// A DATE or DATE-TIME as written.
#[derive(Clone, Debug, PartialEq)]
pub enum When {
    /// A day (all-day events).
    Date(Date),
    /// Wall-clock time wherever the reader is.
    Floating(DateTime),
    /// UTC (written with a `Z`).
    Utc(DateTime),
    /// Wall-clock time in a zone (written with `TZID`).
    Zoned(DateTime, Tz),
}

impl When {
    pub fn is_date(&self) -> bool {
        matches!(self, When::Date(_))
    }

    /// Its civil date-time in its own terms (midnight for a date).
    pub fn civil(&self) -> DateTime {
        match self {
            When::Date(d) => d.to_datetime(Time::midnight()),
            When::Floating(dt) | When::Utc(dt) | When::Zoned(dt, _) => *dt,
        }
    }

    /// The zone its wall-clock time is in (`local` for floating times,
    /// dates and unknown zones).
    pub fn zone(&self, local: &TimeZone) -> TimeZone {
        match self {
            When::Utc(_) => TimeZone::UTC,
            When::Zoned(_, Tz { zone: Some(z), .. }) => z.clone(),
            _ => local.clone(),
        }
    }

    /// The instant it names, floating times and dates taken in `local`.
    pub fn instant(&self, local: &TimeZone) -> Timestamp {
        let z = self.zone(local);
        to_instant(self.civil(), &z)
    }

    /// The same kind of value at another wall-clock time.
    pub fn with_civil(&self, dt: DateTime) -> When {
        match self {
            When::Date(_) => When::Date(dt.date()),
            When::Floating(_) => When::Floating(dt),
            When::Utc(_) => When::Utc(dt),
            When::Zoned(_, tz) => When::Zoned(dt, tz.clone()),
        }
    }

    /// Read a DTSTART-like property, resolving its `TZID` in `zones`.
    pub fn from_prop(p: &Prop, zones: &mut Zones) -> Option<When> {
        let v = p.value.trim();
        // Some writers put several values in a single-valued property.
        let v = v.split(',').next().unwrap_or(v);
        Self::from_value(v, p.param("VALUE"), p.param("TZID"), zones)
    }

    /// Every value of a list property (EXDATE, RDATE).
    pub fn list_from_prop(p: &Prop, zones: &mut Zones) -> Vec<When> {
        value::split_list(&p.value)
            .into_iter()
            .filter_map(|v| Self::from_value(v.trim(), p.param("VALUE"), p.param("TZID"), zones))
            .collect()
    }

    fn from_value(v: &str, kind: Option<&str>, tzid: Option<&str>, zones: &mut Zones) -> Option<When> {
        let is_date = kind.is_some_and(|k| k.eq_ignore_ascii_case("DATE"));
        if (is_date || (v.len() == 8 && !v.contains('T')))
            && let Some(d) = value::parse_date(v)
        {
            return Some(When::Date(d));
        }
        let (dt, utc) = value::parse_datetime(v)
            .or_else(|| value::parse_date(v).map(|d| (d.to_datetime(Time::midnight()), false)))?;
        if utc {
            return Some(When::Utc(dt));
        }
        match tzid {
            Some(id) => {
                let tz = zones.resolve(id);
                if tz.zone.as_ref().is_some_and(|z| z == &TimeZone::UTC) && (&*tz.id == "UTC") {
                    return Some(When::Utc(dt));
                }
                Some(When::Zoned(dt, tz))
            }
            None => Some(When::Floating(dt)),
        }
    }

    /// As a property called `name`.
    pub fn to_prop(&self, name: &str) -> Prop {
        match self {
            When::Date(d) => Prop::new(name, value::format_date(*d)).with_param("VALUE", "DATE"),
            When::Floating(dt) => Prop::new(name, value::format_datetime(*dt, false)),
            When::Utc(dt) => Prop::new(name, value::format_datetime(*dt, true)),
            When::Zoned(dt, tz) => {
                Prop::new(name, value::format_datetime(*dt, false)).with_param("TZID", tz.id.to_string())
            }
        }
    }
}

/// The instant of wall-clock `dt` in `zone` (a time skipped by a DST
/// change moves forward, as RFC 5545 says).
pub fn to_instant(dt: DateTime, zone: &TimeZone) -> Timestamp {
    zone.to_ambiguous_zoned(dt).compatible().map(|z| z.timestamp()).unwrap_or_else(|_| {
        TimeZone::UTC.to_ambiguous_zoned(dt).compatible().map(|z| z.timestamp()).unwrap_or(Timestamp::UNIX_EPOCH)
    })
}

/// How an event's end is given.
#[derive(Clone, Debug, PartialEq)]
pub enum End {
    /// Neither DTEND nor DURATION: a day for a date, an instant otherwise.
    None,
    At(When),
    Duration(Duration),
}

/// When an alarm goes off.
#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    /// Before (negative) or after the event's start, or its end.
    Relative {
        offset: Duration,
        from_end: bool,
    },
    Absolute(Timestamp),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alarm {
    pub trigger: Trigger,
    /// `DISPLAY`, `AUDIO`, `EMAIL`, or Apple's `NONE`.
    pub action: String,
    pub description: String,
    /// Properties besides those, as read.
    pub extra: Vec<Prop>,
    pub other: Vec<Component>,
}

impl Alarm {
    /// A display alarm `minutes` before the start (0: at the start).
    pub fn before(minutes: i64) -> Alarm {
        Alarm {
            trigger: Trigger::Relative { offset: Duration::minutes(-minutes), from_end: false },
            action: "DISPLAY".into(),
            description: "Reminder".into(),
            extra: Vec::new(),
            other: Vec::new(),
        }
    }

    /// Whether it does anything a person would notice.
    pub fn is_active(&self) -> bool {
        !self.action.eq_ignore_ascii_case("NONE")
            && !self.extra.iter().any(|p| p.name == "ACKNOWLEDGED" && self.is_absolute_past(p))
    }

    fn is_absolute_past(&self, _ack: &Prop) -> bool {
        // An acknowledged absolute alarm has fired elsewhere already.
        matches!(self.trigger, Trigger::Absolute(t) if t < Timestamp::now())
    }

    /// Minutes before the start, for the editor (none if it isn't one).
    pub fn minutes_before(&self) -> Option<i64> {
        match &self.trigger {
            Trigger::Relative { offset, from_end: false } if !offset.is_negative() || offset.total_seconds() <= 0 => {
                Some(-offset.total_seconds() / 60)
            }
            _ => None,
        }
    }

    fn from_component(c: &Component) -> Option<Alarm> {
        let t = c.prop("TRIGGER")?;
        let trigger = if t.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME"))
            || t.value.contains('T') && !t.value.contains('P')
        {
            let (dt, _) = value::parse_datetime(&t.value)?;
            Trigger::Absolute(to_instant(dt, &TimeZone::UTC))
        } else {
            let from_end = t.param("RELATED").is_some_and(|r| r.eq_ignore_ascii_case("END"));
            Trigger::Relative { offset: value::parse_duration(&t.value)?, from_end }
        };
        let action = c.prop("ACTION").map(|p| p.value.trim().to_ascii_uppercase()).unwrap_or_else(|| "DISPLAY".into());
        let description = c.text("DESCRIPTION").unwrap_or_default();
        let extra = c
            .props
            .iter()
            .filter(|p| !matches!(p.name.as_str(), "TRIGGER" | "ACTION" | "DESCRIPTION"))
            .cloned()
            .collect();
        Some(Alarm { trigger, action, description, extra, other: c.children.clone() })
    }

    fn to_component(&self) -> Component {
        let mut c = Component::new("VALARM");
        c.push(Prop::new("ACTION", self.action.clone()));
        match &self.trigger {
            Trigger::Relative { offset, from_end } => {
                let mut p = Prop::new("TRIGGER", value::format_duration(*offset));
                if *from_end {
                    p = p.with_param("RELATED", "END");
                }
                c.push(p);
            }
            Trigger::Absolute(t) => {
                let dt = t.to_zoned(TimeZone::UTC).datetime();
                c.push(Prop::new("TRIGGER", value::format_datetime(dt, true)).with_param("VALUE", "DATE-TIME"));
            }
        }
        // DISPLAY alarms must have a description.
        if !self.description.is_empty() || self.action == "DISPLAY" {
            let d = if self.description.is_empty() { "Reminder" } else { &self.description };
            c.push(Prop::text("DESCRIPTION", d));
        }
        c.props.extend(self.extra.iter().cloned());
        c.children.extend(self.other.iter().cloned());
        c
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Status {
    #[default]
    None,
    Tentative,
    Confirmed,
    Cancelled,
}

/// A `VEVENT`.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: When,
    pub end: End,
    pub rrule: Option<RRule>,
    pub rdates: Vec<When>,
    pub exdates: Vec<When>,
    /// Set on an instance of a recurring event that was changed: which
    /// occurrence it replaces.
    pub recurrence_id: Option<When>,
    pub alarms: Vec<Alarm>,
    pub status: Status,
    pub sequence: i64,
    /// Other properties, in order, as read.
    pub extra: Vec<Prop>,
    /// Sub-components besides alarms.
    pub other: Vec<Component>,
}

impl Event {
    /// A new event with a fresh UID.
    pub fn new(summary: impl Into<String>, start: When, end: When) -> Event {
        let mut e = Event {
            uid: new_uid(),
            summary: summary.into(),
            location: String::new(),
            description: String::new(),
            start,
            end: End::At(end),
            rrule: None,
            rdates: Vec::new(),
            exdates: Vec::new(),
            recurrence_id: None,
            alarms: Vec::new(),
            status: Status::None,
            sequence: 0,
            extra: Vec::new(),
            other: Vec::new(),
        };
        let now = utc_stamp();
        e.set_extra("DTSTAMP", &now);
        e.set_extra("CREATED", &now);
        e
    }

    pub fn all_day(&self) -> bool {
        self.start.is_date()
    }

    pub fn is_recurring(&self) -> bool {
        self.rrule.is_some() || !self.rdates.is_empty()
    }

    /// How long it lasts: days nominal (for dates and wall-clock
    /// stretches across a DST change) and seconds exact.
    pub fn duration(&self, local: &TimeZone) -> Duration {
        match &self.end {
            End::Duration(d) => {
                if d.is_negative() {
                    Duration::ZERO
                } else {
                    *d
                }
            }
            End::At(end) => match (&self.start, end) {
                (When::Date(s), e) => {
                    let e = e.civil().date();
                    let days = days_between(*s, e);
                    Duration::days(days.max(1))
                }
                (s, e) => {
                    // Same zone: wall-clock days and the rest exact, so a
                    // repeat keeps its length across DST changes.
                    let (sc, ec) = (s.civil(), e.with_civil(e.civil()).civil());
                    if s.zone(local) == e.zone(local) {
                        let secs = ec.duration_since(sc).as_secs();
                        Duration::seconds(secs.max(0))
                    } else {
                        let secs = e.instant(local).duration_since(s.instant(local)).as_secs();
                        Duration::seconds(secs.max(0))
                    }
                }
            },
            End::None => {
                if self.all_day() {
                    Duration::days(1)
                } else {
                    Duration::ZERO
                }
            }
        }
    }

    /// The end, as a value of the start's kind.
    pub fn end_when(&self, local: &TimeZone) -> When {
        let d = self.duration(local);
        let start = self.start.civil();
        self.start.with_civil(add(start, d))
    }

    /// Replace (or add) an extra property's value.
    pub fn set_extra(&mut self, name: &str, value: &str) {
        match self.extra.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.value = value.to_owned();
                p.params.clear();
            }
            None => self.extra.push(Prop::new(name, value)),
        }
    }

    /// Mark it changed: DTSTAMP and LAST-MODIFIED now, SEQUENCE up.
    pub fn touch(&mut self) {
        let now = utc_stamp();
        self.set_extra("DTSTAMP", &now);
        self.set_extra("LAST-MODIFIED", &now);
        self.sequence += 1;
    }

    pub fn from_component(c: &Component, zones: &mut Zones) -> Option<Event> {
        let start = When::from_prop(c.prop("DTSTART")?, zones)?;
        let end = match (c.prop("DTEND"), c.prop("DURATION")) {
            (Some(p), _) => match When::from_prop(p, zones) {
                Some(w) => End::At(w),
                None => End::None,
            },
            (None, Some(p)) => value::parse_duration(&p.value).map(End::Duration).unwrap_or(End::None),
            _ => End::None,
        };
        // An all-day start with a timed end (or the other way) is a
        // writer's slip: make the end the start's kind.
        let end = match (&start, end) {
            (When::Date(_), End::At(w)) if !w.is_date() => End::At(When::Date(w.civil().date())),
            (s, End::At(When::Date(d))) if !s.is_date() => End::At(s.with_civil(d.to_datetime(Time::midnight()))),
            (_, e) => e,
        };
        let status = match c.prop("STATUS").map(|p| p.value.trim().to_ascii_uppercase()) {
            Some(s) if s == "TENTATIVE" => Status::Tentative,
            Some(s) if s == "CONFIRMED" => Status::Confirmed,
            Some(s) if s == "CANCELLED" || s == "CANCELED" => Status::Cancelled,
            _ => Status::None,
        };
        let mut e = Event {
            uid: c.text("UID").map(|u| u.trim().to_owned()).filter(|u| !u.is_empty()).unwrap_or_else(new_uid),
            summary: c.text("SUMMARY").unwrap_or_default(),
            location: c.text("LOCATION").unwrap_or_default(),
            description: c.text("DESCRIPTION").unwrap_or_default(),
            start,
            end,
            rrule: c.prop("RRULE").and_then(|p| RRule::parse(&p.value)),
            rdates: c
                .props_named("RDATE")
                .filter(|p| !p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("PERIOD")))
                .flat_map(|p| When::list_from_prop(p, zones))
                .collect(),
            exdates: c.props_named("EXDATE").flat_map(|p| When::list_from_prop(p, zones)).collect(),
            recurrence_id: c.prop("RECURRENCE-ID").and_then(|p| When::from_prop(p, zones)),
            alarms: c.children_named("VALARM").filter_map(Alarm::from_component).collect(),
            status,
            sequence: c.prop("SEQUENCE").and_then(|p| p.value.trim().parse().ok()).unwrap_or(0),
            extra: Vec::new(),
            other: c.children.iter().filter(|k| k.name != "VALARM").cloned().collect(),
        };
        const KNOWN: &[&str] = &[
            "UID",
            "SUMMARY",
            "LOCATION",
            "DESCRIPTION",
            "DTSTART",
            "DTEND",
            "DURATION",
            "RRULE",
            "EXDATE",
            "RECURRENCE-ID",
            "STATUS",
            "SEQUENCE",
        ];
        e.extra = c
            .props
            .iter()
            .filter(|p| !KNOWN.contains(&p.name.as_str()))
            // RDATEs are kept as read when they're periods.
            .filter(|p| p.name != "RDATE" || p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("PERIOD")))
            .cloned()
            .collect();
        // DTSTAMP first (as it's written), and one there always.
        match e.extra.iter().position(|p| p.name == "DTSTAMP") {
            Some(i) => {
                let p = e.extra.remove(i);
                e.extra.insert(0, p);
            }
            None => e.extra.insert(0, Prop::new("DTSTAMP", utc_stamp())),
        }
        Some(e)
    }

    pub fn to_component(&self) -> Component {
        let mut c = Component::new("VEVENT");
        c.push(Prop::text("UID", &self.uid));
        if let Some(p) = self.extra.iter().find(|p| p.name == "DTSTAMP") {
            c.push(p.clone());
        } else {
            c.push(Prop::new("DTSTAMP", utc_stamp()));
        }
        c.push(self.start.to_prop("DTSTART"));
        match &self.end {
            End::At(w) => c.push(w.to_prop("DTEND")),
            End::Duration(d) => c.push(Prop::new("DURATION", value::format_duration(*d))),
            End::None => {}
        }
        if let Some(r) = &self.rrule {
            c.push(Prop::new("RRULE", r.to_ical()));
        }
        for d in &self.rdates {
            c.push(d.to_prop("RDATE"));
        }
        for d in &self.exdates {
            c.push(d.to_prop("EXDATE"));
        }
        if let Some(r) = &self.recurrence_id {
            c.push(r.to_prop("RECURRENCE-ID"));
        }
        c.push(Prop::text("SUMMARY", &self.summary));
        if !self.location.is_empty() {
            c.push(Prop::text("LOCATION", &self.location));
        }
        if !self.description.is_empty() {
            c.push(Prop::text("DESCRIPTION", &self.description));
        }
        match self.status {
            Status::None => {}
            Status::Tentative => c.push(Prop::new("STATUS", "TENTATIVE")),
            Status::Confirmed => c.push(Prop::new("STATUS", "CONFIRMED")),
            Status::Cancelled => c.push(Prop::new("STATUS", "CANCELLED")),
        }
        if self.sequence != 0 {
            c.push(Prop::new("SEQUENCE", self.sequence.to_string()));
        }
        c.props.extend(self.extra.iter().filter(|p| p.name != "DTSTAMP").cloned());
        for a in &self.alarms {
            c.children.push(a.to_component());
        }
        c.children.extend(self.other.iter().cloned());
        c
    }

    /// Which zones its times name (for the VTIMEZONEs a file must carry).
    pub fn zone_ids(&self) -> Vec<Arc<str>> {
        let mut out = Vec::new();
        let mut add = |w: &When| {
            if let When::Zoned(_, tz) = w
                && !out.contains(&tz.id)
            {
                out.push(tz.id.clone());
            }
        };
        add(&self.start);
        if let End::At(w) = &self.end {
            add(w);
        }
        for w in self.rdates.iter().chain(&self.exdates).chain(self.recurrence_id.iter()) {
            add(w);
        }
        out
    }

    /// The rule's end in the start's wall-clock terms.
    pub fn until_civil(&self, local: &TimeZone) -> Option<DateTime> {
        let r = self.rrule.as_ref()?;
        Some(match r.until? {
            Until::Date(d) => match self.start {
                When::Date(_) => d.to_datetime(Time::midnight()),
                _ => d.to_datetime(Time::constant(23, 59, 59, 0)),
            },
            Until::DateTime { at, utc } => match &self.start {
                When::Date(_) => {
                    let date =
                        if utc { to_instant(at, &TimeZone::UTC).to_zoned(local.clone()).date() } else { at.date() };
                    date.to_datetime(Time::midnight())
                }
                s if utc => to_instant(at, &TimeZone::UTC).to_zoned(s.zone(local)).datetime(),
                _ => at,
            },
        })
    }
}

/// `dt` plus a duration: days on the wall clock, then seconds.
pub fn add(dt: DateTime, d: Duration) -> DateTime {
    let with_days = dt.checked_add(jiff::Span::new().days(d.days.clamp(-100_000, 100_000))).unwrap_or(dt);
    with_days.checked_add(SignedDuration::from_secs(d.seconds)).unwrap_or(with_days)
}

/// Whole days from `a` to `b`.
pub fn days_between(a: Date, b: Date) -> i64 {
    b.duration_since(a).as_secs().div_euclid(86_400)
}

/// Now, as DTSTAMP writes it.
pub fn utc_stamp() -> String {
    let dt = Timestamp::now().to_zoned(TimeZone::UTC).datetime();
    value::format_datetime(dt, true)
}

/// A UID nobody else has: the time, the process and a counter.
pub fn new_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let t = Timestamp::now();
    let mut seed =
        t.as_nanosecond() as u64 ^ (u64::from(std::process::id()) << 32) ^ n.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    // A splitmix step, so UIDs made in one nanosecond still differ.
    seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    format!("{:016x}-{:x}@koyomi.aizome", z, t.as_second())
}

/// A calendar's colour: one of the theme's dyes, so it reads in Night
/// and Day alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dye {
    /// 縹, the theme's blue.
    #[default]
    Hanada,
    /// 藍, indigo.
    Ai,
    /// 甕覗, the palest indigo.
    Kamenozoki,
    Cyan,
    Green,
    Red,
    Violet,
    /// 琥珀.
    Kohaku,
}

impl Dye {
    pub const ALL: [Dye; 8] =
        [Dye::Hanada, Dye::Ai, Dye::Kamenozoki, Dye::Cyan, Dye::Green, Dye::Red, Dye::Violet, Dye::Kohaku];

    pub fn key(self) -> &'static str {
        match self {
            Dye::Hanada => "hanada",
            Dye::Ai => "ai",
            Dye::Kamenozoki => "kamenozoki",
            Dye::Cyan => "cyan",
            Dye::Green => "green",
            Dye::Red => "red",
            Dye::Violet => "violet",
            Dye::Kohaku => "kohaku",
        }
    }

    /// Its name in menus.
    pub fn name(self) -> &'static str {
        match self {
            Dye::Hanada => "Hanada Blue",
            Dye::Ai => "Ai Indigo",
            Dye::Kamenozoki => "Kamenozoki",
            Dye::Cyan => "Cyan",
            Dye::Green => "Green",
            Dye::Red => "Red",
            Dye::Violet => "Violet",
            Dye::Kohaku => "Kohaku Amber",
        }
    }

    pub fn from_key(s: &str) -> Option<Dye> {
        Dye::ALL.iter().copied().find(|d| d.key().eq_ignore_ascii_case(s.trim()))
    }

    /// Its colour in Night, as other apps are told it.
    pub fn hex(self) -> u32 {
        match self {
            Dye::Hanada => 0x7aa2f7,
            Dye::Ai => 0x3d59a1,
            Dye::Kamenozoki => 0xa9c4ff,
            Dye::Cyan => 0x7dcfff,
            Dye::Green => 0x9ece6a,
            Dye::Red => 0xf7768e,
            Dye::Violet => 0xbb9af7,
            Dye::Kohaku => 0xe0af68,
        }
    }

    /// The dye nearest a colour from another app, by hue (greys and
    /// browns go to Ai and Kohaku).
    pub fn nearest(rgb: u32) -> Dye {
        let (r, g, b) =
            (((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta < 0.12 {
            return Dye::Ai;
        }
        let mut h = if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        if h < 0.0 {
            h += 360.0;
        }
        match h {
            h if !(15.0..330.0).contains(&h) => Dye::Red,
            h if h < 70.0 => Dye::Kohaku,
            h if h < 165.0 => Dye::Green,
            h if h < 200.0 => Dye::Cyan,
            h if h < 235.0 => {
                if max > 0.85 && min > 0.55 {
                    Dye::Kamenozoki
                } else {
                    Dye::Hanada
                }
            }
            h if h < 255.0 => Dye::Ai,
            _ => Dye::Violet,
        }
    }
}

/// One calendar: one `.ics` file.
#[derive(Clone, Debug, PartialEq)]
pub struct Calendar {
    /// The file's name without `.ics`.
    pub id: String,
    pub name: String,
    pub color: Dye,
    pub events: Vec<Event>,
    /// `VTIMEZONE`s, as read (written back for the zones still used).
    pub timezones: Vec<Component>,
    /// To-dos, journal entries and the rest, kept.
    pub other: Vec<Component>,
    /// The calendar's other properties (`X-WR-TIMEZONE`, `METHOD`, …).
    pub props: Vec<Prop>,
}

impl Calendar {
    pub fn new(id: impl Into<String>, name: impl Into<String>, color: Dye) -> Calendar {
        Calendar {
            id: id.into(),
            name: name.into(),
            color,
            events: Vec::new(),
            timezones: Vec::new(),
            other: Vec::new(),
            props: Vec::new(),
        }
    }

    /// Read iCalendar text: every VCALENDAR in it (and bare events) as one
    /// calendar. Events without a start are dropped (and counted).
    pub fn parse(id: &str, text: &str) -> (Calendar, usize) {
        let top = crate::ics::parse(text);
        let mut cal = Calendar::new(id, id, Dye::Hanada);
        let mut colour: Option<Dye> = None;
        let mut name: Option<String> = None;
        let mut vevents: Vec<&Component> = Vec::new();
        for c in &top {
            match c.name.as_str() {
                "VCALENDAR" => {
                    for p in &c.props {
                        match p.name.as_str() {
                            "X-WR-CALNAME" | "NAME" => {
                                name = name.or_else(|| Some(p.text_value()).filter(|n| !n.trim().is_empty()))
                            }
                            "X-AIZOME-COLOR" => colour = Dye::from_key(&p.value).or(colour),
                            "X-APPLE-CALENDAR-COLOR" | "COLOR" | "X-OUTLOOK-COLOR" => {
                                if colour.is_none() {
                                    colour = parse_hex(&p.value).map(Dye::nearest);
                                }
                            }
                            "VERSION" | "PRODID" => {}
                            _ => cal.props.push(p.clone()),
                        }
                    }
                    for k in &c.children {
                        match k.name.as_str() {
                            "VEVENT" => vevents.push(k),
                            "VTIMEZONE" => cal.timezones.push(k.clone()),
                            _ => cal.other.push(k.clone()),
                        }
                    }
                }
                "VEVENT" => vevents.push(c),
                "VTIMEZONE" => cal.timezones.push(c.clone()),
                _ => {}
            }
        }
        let mut zones = Zones::new(cal.timezones.iter());
        let mut dropped = 0;
        for v in vevents {
            match Event::from_component(v, &mut zones) {
                Some(e) => cal.events.push(e),
                None => dropped += 1,
            }
        }
        if let Some(n) = name {
            cal.name = n.trim().to_owned();
        }
        if let Some(c) = colour {
            cal.color = c;
        }
        (cal, dropped)
    }

    /// The calendar as iCalendar text.
    pub fn to_ics(&self) -> String {
        let mut c = Component::new("VCALENDAR");
        c.push(Prop::new("VERSION", "2.0"));
        c.push(Prop::new("PRODID", "-//Aizome//Koyomi 0.1//EN"));
        c.push(Prop::text("X-WR-CALNAME", &self.name));
        c.push(Prop::new("X-AIZOME-COLOR", self.color.key()));
        c.push(Prop::new("X-APPLE-CALENDAR-COLOR", format!("#{:06X}", self.color.hex())));
        c.props.extend(
            self.props
                .iter()
                .filter(|p| !matches!(p.name.as_str(), "X-WR-CALNAME" | "X-AIZOME-COLOR" | "X-APPLE-CALENDAR-COLOR"))
                .cloned(),
        );
        // The zones the events use: their definitions as read, or made
        // for IANA zones.
        let mut used: Vec<Arc<str>> = Vec::new();
        for e in &self.events {
            for id in e.zone_ids() {
                if !used.contains(&id) {
                    used.push(id);
                }
            }
        }
        for id in &used {
            match self.timezones.iter().find(|t| t.text("TZID").is_some_and(|t| t.trim() == &**id)) {
                Some(t) => c.children.push(t.clone()),
                None => {
                    if let Some(t) = crate::vtimezone::make(id) {
                        c.children.push(t);
                    }
                }
            }
        }
        for e in &self.events {
            c.children.push(e.to_component());
        }
        c.children.extend(self.other.iter().cloned());
        crate::ics::to_string(&c)
    }

    /// The event with `uid` that isn't an instance override.
    pub fn master(&self, uid: &str) -> Option<usize> {
        self.events.iter().position(|e| e.uid == uid && e.recurrence_id.is_none())
    }
}

fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    match s.len() {
        6 => u32::from_str_radix(s, 16).ok(),
        8 => u32::from_str_radix(&s[..6], 16).ok(),
        3 => {
            let v = u32::from_str_radix(s, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 15, (v >> 4) & 15, v & 15);
            Some(((r * 17) << 16) | ((g * 17) << 8) | (b * 17))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, datetime};

    #[test]
    fn events_round_trip() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Work\r\nX-APPLE-CALENDAR-COLOR:#FF9500\r\nBEGIN:VEVENT\r\nUID:abc\r\nDTSTAMP:20260101T000000Z\r\nDTSTART;TZID=Europe/London:20261017T090000\r\nDTEND;TZID=Europe/London:20261017T103000\r\nRRULE:FREQ=WEEKLY;BYDAY=SA\r\nEXDATE;TZID=Europe/London:20261024T090000\r\nSUMMARY:Stand-up\\, weekly\r\nORGANIZER;CN=Jo:mailto:jo@example.com\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT10M\r\nDESCRIPTION:Soon\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (cal, dropped) = Calendar::parse("work", text);
        assert_eq!(dropped, 0);
        assert_eq!(cal.name, "Work");
        assert_eq!(cal.color, Dye::Kohaku);
        let e = &cal.events[0];
        assert_eq!(e.summary, "Stand-up, weekly");
        assert_eq!(e.start, When::Zoned(datetime(2026, 10, 17, 9, 0, 0, 0), Tz::iana("Europe/London")));
        assert_eq!(e.duration(&TimeZone::UTC), Duration::seconds(5400));
        assert_eq!(e.alarms[0].minutes_before(), Some(10));
        assert_eq!(e.extra.iter().find(|p| p.name == "ORGANIZER").unwrap().param("CN"), Some("Jo"));
        let again = Calendar::parse("work", &cal.to_ics()).0;
        assert_eq!(again.events, cal.events);
        assert_eq!(again.name, "Work");
        // The zone it uses gets a definition.
        assert!(cal.to_ics().contains("BEGIN:VTIMEZONE"));
    }

    #[test]
    fn all_day_ends_default_to_a_day() {
        let text = "BEGIN:VEVENT\nUID:x\nDTSTART;VALUE=DATE:20261017\nSUMMARY:Day\nEND:VEVENT\n";
        let (cal, _) = Calendar::parse("c", text);
        let e = &cal.events[0];
        assert!(e.all_day());
        assert_eq!(e.end_when(&TimeZone::UTC), When::Date(date(2026, 10, 18)));
    }

    #[test]
    fn colours_map_to_dyes() {
        assert_eq!(Dye::nearest(0xFF2968), Dye::Red);
        assert_eq!(Dye::nearest(0x1BADF8), Dye::Hanada);
        assert_eq!(Dye::nearest(0x63DA38), Dye::Green);
        assert_eq!(Dye::nearest(0xCC73E1), Dye::Violet);
        assert_eq!(Dye::nearest(0xFFCC00), Dye::Kohaku);
        assert_eq!(Dye::nearest(0x888888), Dye::Ai);
        assert_eq!(parse_hex("#0B8043FF"), Some(0x0B8043));
    }
}
