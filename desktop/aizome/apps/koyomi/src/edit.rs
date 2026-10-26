//! Changing events: moving and resizing an occurrence (or its whole
//! series), changing one instance of a series (a `RECURRENCE-ID`
//! override), deleting one, the rest of a series or all of it, and the
//! repeat rules people pick.
//!
//! Times come in as wall-clock times where the person is; each event
//! keeps its own kind of time (a zone, UTC, floating, a date) and is
//! changed in its own terms, so a London meeting moved in Tokyo stays a
//! London meeting.

use jiff::civil::{Date, DateTime, Time, Weekday};
use jiff::tz::TimeZone;

use crate::ics::value::Duration;
use crate::model::{Calendar, End, Event, When, add, days_between, to_instant};
use crate::occur::Occurrence;
use crate::rrule::{Freq, RRule, Until, add_days};
use crate::tz::Tz;

/// Which events of a series a change is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// This occurrence only.
    This,
    /// This one and those after it.
    Future,
    All,
}

/// An occurrence, as the program keeps hold of it across views.
#[derive(Clone, Debug, PartialEq)]
pub struct OccRef {
    pub cal: String,
    pub uid: String,
    /// The instance's RECURRENCE-ID, for an occurrence of a series.
    pub rid: Option<When>,
    pub start: DateTime,
    pub end: DateTime,
    pub all_day: bool,
}

impl OccRef {
    pub fn of(o: &Occurrence, cals: &[Calendar]) -> OccRef {
        let cal = &cals[o.cal];
        let e = &cal.events[o.event];
        OccRef {
            cal: cal.id.clone(),
            uid: e.uid.clone(),
            rid: o.rid.clone().or_else(|| e.recurrence_id.clone()),
            start: o.start,
            end: o.end,
            all_day: o.all_day,
        }
    }

    /// Whether `o` is this occurrence.
    pub fn is(&self, o: &Occurrence, cals: &[Calendar]) -> bool {
        let cal = &cals[o.cal];
        let e = &cal.events[o.event];
        cal.id == self.cal
            && e.uid == self.uid
            && same_rid(o.rid.as_ref().or(e.recurrence_id.as_ref()), self.rid.as_ref())
    }
}

fn same_rid(a: Option<&When>, b: Option<&When>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.civil() == b.civil(),
        _ => false,
    }
}

/// The time zone new events are made in: the reader's, by IANA name
/// (floating if it has none).
pub fn local_when(dt: DateTime, local: &Tz) -> When {
    if local.zone.is_some() && &*local.id != "UTC" && local.is_iana() {
        When::Zoned(dt, local.clone())
    } else if &*local.id == "UTC" {
        When::Utc(dt)
    } else {
        When::Floating(dt)
    }
}

/// A wall-clock time here, as a value of `like`'s kind.
pub fn in_terms_of(like: &When, dt: DateTime, local: &TimeZone) -> When {
    match like {
        When::Date(_) => When::Date(dt.date()),
        When::Floating(_) => When::Floating(dt),
        When::Utc(_) => When::Utc(to_instant(dt, local).to_zoned(TimeZone::UTC).datetime()),
        When::Zoned(_, tz) => match &tz.zone {
            Some(z) => When::Zoned(to_instant(dt, local).to_zoned(z.clone()).datetime(), tz.clone()),
            None => When::Zoned(dt, tz.clone()),
        },
    }
}

/// The calendar's series master for `uid`, and the instance override
/// for `rid` if there is one.
fn parts(cal: &Calendar, uid: &str, rid: Option<&When>) -> (Option<usize>, Option<usize>) {
    let master = cal.master(uid);
    let over = rid.and_then(|r| {
        cal.events.iter().position(|e| e.uid == uid && e.recurrence_id.as_ref().is_some_and(|x| x.civil() == r.civil()))
    });
    (master, over)
}

/// Whether the occurrence belongs to a series (asks before changing).
pub fn in_series(cal: &Calendar, r: &OccRef) -> bool {
    match cal.master(&r.uid) {
        Some(m) => {
            cal.events[m].is_recurring()
                || r.rid.is_some() && cal.events.iter().any(|e| e.uid == r.uid && e.recurrence_id.is_some())
        }
        None => r.rid.is_some(),
    }
}

/// Set an event's times: `[start, end)` here, all day or not.
pub fn set_times(e: &mut Event, start: DateTime, end: DateTime, all_day: bool, local: &TimeZone, here: &Tz) {
    if all_day {
        let s = start.date();
        let mut last = end.date();
        if end.time() != Time::midnight() {
            last = last.tomorrow().unwrap_or(last);
        }
        if last <= s {
            last = s.tomorrow().unwrap_or(s);
        }
        e.start = When::Date(s);
        e.end = match e.end {
            End::Duration(_) => End::Duration(Duration::days(days_between(s, last))),
            _ => End::At(When::Date(last)),
        };
        return;
    }
    let end = end.max(start);
    if e.start.is_date() {
        e.start = local_when(start, here);
        e.end = End::At(local_when(end, here));
        return;
    }
    let like = e.start.clone();
    e.start = in_terms_of(&like, start, local);
    e.end = match e.end {
        End::Duration(_) => End::Duration(Duration::seconds(end.duration_since(start).as_secs())),
        _ => End::At(in_terms_of(&like, end, local)),
    };
}

/// An override for instance `rid` of `master`, as the instance was.
fn instance_of(master: &Event, rid: &When, local: &TimeZone) -> Event {
    let mut o = master.clone();
    let dur = master.duration(local);
    o.start = rid.clone();
    o.end = End::At(o.start.with_civil(add(rid.civil(), dur)));
    o.recurrence_id = Some(rid.clone());
    o.rrule = None;
    o.rdates.clear();
    o.exdates.clear();
    o
}

/// Move or resize an occurrence to `[start, end)` here.
#[allow(clippy::too_many_arguments)]
pub fn reschedule(
    cal: &mut Calendar,
    r: &OccRef,
    start: DateTime,
    end: DateTime,
    all_day: bool,
    scope: Scope,
    local: &TimeZone,
    here: &Tz,
) -> bool {
    let (master, over) = parts(cal, &r.uid, r.rid.as_ref());
    match (scope, r.rid.as_ref()) {
        // A single event (or a lone override).
        (_, None) => {
            let Some(i) = master.or_else(|| cal.events.iter().position(|e| e.uid == r.uid)) else { return false };
            let e = &mut cal.events[i];
            set_times(e, start, end, all_day, local, here);
            e.touch();
            true
        }
        (Scope::This, Some(rid)) => {
            let i = match over {
                Some(i) => i,
                None => {
                    let Some(m) = master else { return false };
                    let inst = instance_of(&cal.events[m], rid, local);
                    cal.events.push(inst);
                    cal.events.len() - 1
                }
            };
            let e = &mut cal.events[i];
            set_times(e, start, end, all_day, local, here);
            e.touch();
            true
        }
        (Scope::All | Scope::Future, Some(rid)) => {
            let Some(m) = master else { return false };
            shift_series(cal, m, rid, start, end, all_day, local, here);
            true
        }
    }
}

/// Move a whole series by the change of one of its instances: from
/// `rid` to `[start, end)` here.
#[allow(clippy::too_many_arguments)]
fn shift_series(
    cal: &mut Calendar,
    m: usize,
    rid: &When,
    start: DateTime,
    end: DateTime,
    all_day: bool,
    local: &TimeZone,
    here: &Tz,
) {
    let uid = cal.events[m].uid.clone();
    let master = &cal.events[m];
    let was_all_day = master.all_day();
    // The instance's new start in the series' own terms.
    let new_start = if all_day {
        When::Date(start.date())
    } else if was_all_day {
        local_when(start, here)
    } else {
        in_terms_of(&master.start, start, local)
    };
    let old = rid.civil();
    let delta = new_start.civil().duration_since(old);
    let day_shift = days_between(old.date(), new_start.civil().date());
    let kind_change = all_day != was_all_day;
    let shift = |w: &When| -> When {
        if kind_change {
            let d = add_days(w.civil().date(), day_shift).unwrap_or(w.civil().date());
            if all_day { When::Date(d) } else { local_when(d.to_datetime(start.time()), here) }
        } else {
            w.with_civil(w.civil().checked_add(delta).unwrap_or(w.civil()))
        }
    };
    let e = &mut cal.events[m];
    let first = shift(&e.start);
    let len = if all_day {
        let last =
            if end.time() == Time::midnight() { end.date() } else { end.date().tomorrow().unwrap_or(end.date()) };
        Duration::days(days_between(start.date(), last).max(1))
    } else {
        Duration::seconds(end.duration_since(start).as_secs().max(0))
    };
    e.start = first;
    e.end = match e.end {
        End::Duration(_) => End::Duration(len),
        _ => End::At(e.start.with_civil(add(e.start.civil(), len))),
    };
    e.exdates = e.exdates.iter().map(shift).collect();
    e.rdates = e.rdates.iter().map(shift).collect();
    if let Some(rule) = e.rrule.as_mut() {
        shift_rule(rule, day_shift, delta);
    }
    e.touch();
    for o in cal.events.iter_mut().filter(|o| o.uid == uid && o.recurrence_id.is_some()) {
        o.recurrence_id = o.recurrence_id.as_ref().map(shift);
    }
}

fn shift_rule(rule: &mut RRule, days: i64, delta: jiff::SignedDuration) {
    if days != 0 {
        for (_, w) in rule.by_day.iter_mut() {
            *w = w.wrapping_add(days);
        }
        for d in rule.by_month_day.iter_mut() {
            if *d > 0 {
                *d = (*d as i64 + days).clamp(1, 31) as i8;
            }
        }
    }
    rule.until = rule.until.map(|u| match u {
        Until::Date(d) => Until::Date(add_days(d, days).unwrap_or(d)),
        Until::DateTime { at, utc } => Until::DateTime { at: at.checked_add(delta).unwrap_or(at), utc },
    });
}

/// Delete an occurrence: it alone (an exception of its series), it and
/// those after, or everything with its UID.
pub fn delete(cal: &mut Calendar, r: &OccRef, scope: Scope) -> bool {
    let before = cal.events.len();
    let (master, over) = parts(cal, &r.uid, r.rid.as_ref());
    match (scope, r.rid.as_ref(), master) {
        (Scope::All, _, _) | (_, None, _) | (_, Some(_), None) => {
            cal.events.retain(|e| e.uid != r.uid);
        }
        (Scope::This, Some(rid), Some(m)) => {
            let e = &mut cal.events[m];
            if !e.exdates.iter().any(|x| x.civil() == rid.civil()) {
                e.exdates.push(e.start.with_civil(rid.civil()));
            }
            e.touch();
            if let Some(o) = over {
                cal.events.remove(o);
            }
            return true;
        }
        (Scope::Future, Some(rid), Some(m)) => {
            let e = &mut cal.events[m];
            let cut = rid.civil();
            if cut <= e.start.civil() {
                cal.events.retain(|e| e.uid != r.uid);
            } else {
                if let Some(rule) = e.rrule.as_mut() {
                    rule.count = None;
                    rule.until = Some(match &e.start {
                        When::Date(_) => Until::Date(cut.date().yesterday().unwrap_or(cut.date())),
                        When::Floating(_) => Until::DateTime {
                            at: cut.checked_sub(jiff::SignedDuration::from_secs(1)).unwrap_or(cut),
                            utc: false,
                        },
                        w => {
                            let z = w.zone(&TimeZone::UTC);
                            let ts = to_instant(cut, &z)
                                .checked_sub(jiff::SignedDuration::from_secs(1))
                                .unwrap_or_else(|_| to_instant(cut, &z));
                            Until::DateTime { at: ts.to_zoned(TimeZone::UTC).datetime(), utc: true }
                        }
                    });
                }
                e.rdates.retain(|d| d.civil() < cut);
                e.touch();
                cal.events.retain(|e| !(e.uid == r.uid && e.recurrence_id.as_ref().is_some_and(|x| x.civil() >= cut)));
            }
        }
    }
    cal.events.len() != before
}

/// The repeat rules the editor offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Repeat {
    pub freq: Option<Freq>,
    pub interval: u32,
    /// For weekly: the days (none: the start's).
    pub weekdays: Vec<Weekday>,
    /// For monthly: by weekday ("the third Saturday") rather than by day.
    pub by_weekday: bool,
    pub end: RepeatEnd,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RepeatEnd {
    Never,
    After(u32),
    On(Date),
}

impl Repeat {
    pub const NEVER: Repeat =
        Repeat { freq: None, interval: 1, weekdays: Vec::new(), by_weekday: false, end: RepeatEnd::Never };

    /// What the editor makes of a rule; `None` if it can't show it (it
    /// keeps the rule as it is then).
    pub fn from_rule(rule: Option<&RRule>, start: Date) -> Option<Repeat> {
        let Some(r) = rule else { return Some(Repeat::NEVER) };
        if !matches!(r.freq, Freq::Daily | Freq::Weekly | Freq::Monthly | Freq::Yearly)
            || !r.by_month.is_empty() && r.freq != Freq::Yearly
            || !r.by_year_day.is_empty()
            || !r.by_week_no.is_empty()
            || !r.by_hour.is_empty()
            || !r.by_minute.is_empty()
            || !r.by_set_pos.is_empty()
            || !r.other.is_empty()
        {
            return None;
        }
        let end = match (r.count, r.until) {
            (Some(n), _) => RepeatEnd::After(n),
            (None, Some(Until::Date(d))) => RepeatEnd::On(d),
            (None, Some(Until::DateTime { at, .. })) => RepeatEnd::On(at.date()),
            (None, None) => RepeatEnd::Never,
        };
        let mut rep =
            Repeat { freq: Some(r.freq), interval: r.interval.max(1), weekdays: Vec::new(), by_weekday: false, end };
        match r.freq {
            Freq::Daily if r.by_day.is_empty() && r.by_month_day.is_empty() => {}
            Freq::Weekly if r.by_month_day.is_empty() && r.by_day.iter().all(|(n, _)| *n == 0) => {
                rep.weekdays = r.by_day.iter().map(|(_, w)| *w).collect();
            }
            Freq::Monthly if r.by_day.is_empty() && (r.by_month_day.is_empty() || r.by_month_day == [start.day()]) => {}
            Freq::Monthly if r.by_month_day.is_empty() && r.by_day.len() == 1 && r.by_day[0].0 != 0 => {
                rep.by_weekday = true;
            }
            Freq::Yearly
                if r.by_day.is_empty()
                    && r.by_month_day.is_empty()
                    && (r.by_month.is_empty() || r.by_month == [start.month()]) => {}
            _ => return None,
        }
        Some(rep)
    }

    /// The rule, for a series starting `start` (as the event's own kind
    /// of time, for UNTIL).
    pub fn to_rule(&self, start: &When, local: &TimeZone) -> Option<RRule> {
        let freq = self.freq?;
        let mut r = RRule::new(freq);
        r.interval = self.interval.max(1);
        let s = start.civil().date();
        match freq {
            Freq::Weekly if !(self.weekdays.is_empty() || self.weekdays == [s.weekday()]) => {
                let mut days = self.weekdays.clone();
                days.sort_by_key(|w| w.to_monday_zero_offset());
                days.dedup();
                r.by_day = days.into_iter().map(|w| (0, w)).collect();
            }
            Freq::Monthly if self.by_weekday => {
                // The fifth of a weekday is the month's last.
                let nth = (s.day() - 1) / 7 + 1;
                r.by_day = vec![(if nth >= 5 { -1 } else { nth }, s.weekday())];
            }
            _ => {}
        }
        match self.end {
            RepeatEnd::Never => {}
            RepeatEnd::After(n) => r.count = Some(n.max(1)),
            RepeatEnd::On(d) => {
                r.until = Some(match start {
                    When::Date(_) => Until::Date(d),
                    When::Floating(_) => {
                        Until::DateTime { at: d.to_datetime(Time::constant(23, 59, 59, 0)), utc: false }
                    }
                    w => {
                        let z = w.zone(local);
                        let ts = to_instant(d.to_datetime(Time::constant(23, 59, 59, 0)), &z);
                        Until::DateTime { at: ts.to_zoned(TimeZone::UTC).datetime(), utc: true }
                    }
                })
            }
        }
        Some(r)
    }

    /// How the editor says it: `Every 2 weeks on Mon, Thu`.
    pub fn describe(&self, start: Date) -> String {
        let Some(f) = self.freq else { return "Never".into() };
        let n = self.interval.max(1);
        let unit = match f {
            Freq::Daily => "day",
            Freq::Weekly => "week",
            Freq::Monthly => "month",
            Freq::Yearly => "year",
            _ => "time",
        };
        let mut s = if n == 1 { format!("Every {unit}") } else { format!("Every {n} {unit}s") };
        match f {
            Freq::Weekly if !self.weekdays.is_empty() && self.weekdays != [start.weekday()] => {
                let days: Vec<&str> = self.weekdays.iter().map(|w| crate::format::weekday_short(*w)).collect();
                s.push_str(&format!(" on {}", days.join(", ")));
            }
            Freq::Monthly if self.by_weekday => {
                let nth = (start.day() - 1) / 7 + 1;
                let ord = ["first", "second", "third", "fourth", "last"][(nth as usize - 1).min(4)];
                s.push_str(&format!(" on the {ord} {}", crate::format::weekday_name(start.weekday())));
            }
            Freq::Monthly => s.push_str(&format!(" on the {}", ordinal(start.day()))),
            _ => {}
        }
        match self.end {
            RepeatEnd::Never => {}
            RepeatEnd::After(c) => s.push_str(&format!(", {c} times")),
            RepeatEnd::On(d) => s.push_str(&format!(", until {}", crate::format::short_date(d))),
        }
        s
    }
}

pub fn ordinal(n: i8) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::occur::{Index, occurrences};
    use jiff::civil::{date, datetime};

    fn london() -> (TimeZone, Tz) {
        (TimeZone::get("Europe/London").unwrap(), Tz::iana("Europe/London"))
    }

    fn weekly() -> Calendar {
        Calendar::parse(
            "w",
            "BEGIN:VEVENT\nUID:s\nDTSTART;TZID=Europe/London:20261005T100000\nDTEND;TZID=Europe/London:20261005T110000\nRRULE:FREQ=WEEKLY;BYDAY=MO\nSUMMARY:Weekly\nEND:VEVENT\n",
        )
        .0
    }

    fn starts(cal: &Calendar, tz: &TimeZone) -> Vec<String> {
        let cals = vec![cal.clone()];
        let ix = Index::new(&cals, tz);
        occurrences(&cals, &ix, &[true], tz, datetime(2026, 10, 1, 0, 0, 0, 0), datetime(2026, 11, 1, 0, 0, 0, 0))
            .iter()
            .map(|o| o.start.to_string())
            .collect()
    }

    fn occ(cal: &Calendar, tz: &TimeZone, n: usize) -> OccRef {
        let cals = vec![cal.clone()];
        let ix = Index::new(&cals, tz);
        let v =
            occurrences(&cals, &ix, &[true], tz, datetime(2026, 10, 1, 0, 0, 0, 0), datetime(2026, 11, 1, 0, 0, 0, 0));
        OccRef::of(&v[n], &cals)
    }

    #[test]
    fn moving_one_instance_makes_an_override() {
        let (tz, here) = london();
        let mut cal = weekly();
        let r = occ(&cal, &tz, 1);
        assert!(in_series(&cal, &r));
        assert!(reschedule(
            &mut cal,
            &r,
            datetime(2026, 10, 13, 14, 0, 0, 0),
            datetime(2026, 10, 13, 15, 30, 0, 0),
            false,
            Scope::This,
            &tz,
            &here
        ));
        assert_eq!(
            starts(&cal, &tz),
            ["2026-10-05T10:00:00", "2026-10-13T14:00:00", "2026-10-19T10:00:00", "2026-10-26T10:00:00"]
        );
        // Moving it again changes the same override.
        let r = occ(&cal, &tz, 1);
        reschedule(
            &mut cal,
            &r,
            datetime(2026, 10, 14, 9, 0, 0, 0),
            datetime(2026, 10, 14, 10, 0, 0, 0),
            false,
            Scope::This,
            &tz,
            &here,
        );
        assert_eq!(cal.events.len(), 2);
        assert_eq!(starts(&cal, &tz)[1], "2026-10-14T09:00:00");
    }

    #[test]
    fn moving_the_series_moves_its_rule_and_exceptions() {
        let (tz, here) = london();
        let mut cal = weekly();
        let third = occ(&cal, &tz, 2);
        delete(&mut cal, &third, Scope::This);
        assert_eq!(starts(&cal, &tz).len(), 3);
        let r = occ(&cal, &tz, 1);
        // Monday 10:00 to Tuesday 11:00, an hour long still.
        reschedule(
            &mut cal,
            &r,
            datetime(2026, 10, 13, 11, 0, 0, 0),
            datetime(2026, 10, 13, 12, 0, 0, 0),
            false,
            Scope::All,
            &tz,
            &here,
        );
        assert_eq!(starts(&cal, &tz), ["2026-10-06T11:00:00", "2026-10-13T11:00:00", "2026-10-27T11:00:00"]);
    }

    #[test]
    fn deleting_this_future_or_all() {
        let (tz, _) = london();
        let mut cal = weekly();
        let r = occ(&cal, &tz, 2);
        delete(&mut cal, &r, Scope::Future);
        assert_eq!(starts(&cal, &tz), ["2026-10-05T10:00:00", "2026-10-12T10:00:00"]);
        let r = occ(&cal, &tz, 0);
        delete(&mut cal, &r, Scope::All);
        assert!(cal.events.is_empty());
    }

    #[test]
    fn all_day_and_back() {
        let (tz, here) = london();
        let mut cal = Calendar::parse(
            "c",
            "BEGIN:VEVENT\nUID:x\nDTSTART:20261017T090000Z\nDTEND:20261017T100000Z\nSUMMARY:x\nEND:VEVENT\n",
        )
        .0;
        let r = occ(&cal, &tz, 0);
        reschedule(
            &mut cal,
            &r,
            datetime(2026, 10, 18, 0, 0, 0, 0),
            datetime(2026, 10, 20, 0, 0, 0, 0),
            true,
            Scope::This,
            &tz,
            &here,
        );
        assert_eq!(cal.events[0].start, When::Date(date(2026, 10, 18)));
        assert_eq!(cal.events[0].end_when(&tz), When::Date(date(2026, 10, 20)));
        let r = occ(&cal, &tz, 0);
        reschedule(
            &mut cal,
            &r,
            datetime(2026, 10, 18, 15, 0, 0, 0),
            datetime(2026, 10, 18, 16, 0, 0, 0),
            false,
            Scope::This,
            &tz,
            &here,
        );
        assert_eq!(cal.events[0].start, When::Zoned(datetime(2026, 10, 18, 15, 0, 0, 0), here));
    }

    #[test]
    fn repeats_round_trip_through_rules() {
        let (tz, here) = london();
        let start = When::Zoned(datetime(2026, 10, 17, 9, 0, 0, 0), here);
        let rep = Repeat {
            freq: Some(Freq::Weekly),
            interval: 2,
            weekdays: vec![Weekday::Saturday, Weekday::Monday],
            by_weekday: false,
            end: RepeatEnd::After(5),
        };
        let rule = rep.to_rule(&start, &tz).unwrap();
        assert_eq!(rule.to_ical(), "FREQ=WEEKLY;INTERVAL=2;COUNT=5;BYDAY=MO,SA");
        let back = Repeat::from_rule(Some(&rule), date(2026, 10, 17)).unwrap();
        assert_eq!(back.weekdays, vec![Weekday::Monday, Weekday::Saturday]);
        assert_eq!(back.describe(date(2026, 10, 17)), "Every 2 weeks on Mon, Sat, 5 times");
        let third_sat = Repeat {
            freq: Some(Freq::Monthly),
            interval: 1,
            weekdays: vec![],
            by_weekday: true,
            end: RepeatEnd::On(date(2027, 1, 31)),
        };
        let rule = third_sat.to_rule(&start, &tz).unwrap();
        assert_eq!(rule.to_ical(), "FREQ=MONTHLY;UNTIL=20270131T235959Z;BYDAY=3SA");
        assert_eq!(third_sat.describe(date(2026, 10, 17)), "Every month on the third Saturday, until Sun 31 Jan");
        assert!(
            Repeat::from_rule(RRule::parse("FREQ=MONTHLY;BYDAY=MO,TU;BYSETPOS=-1").as_ref(), date(2026, 10, 17))
                .is_none()
        );
    }
}
