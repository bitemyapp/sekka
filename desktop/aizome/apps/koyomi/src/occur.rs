//! Occurrences: the events of a stretch of time, recurring ones expanded,
//! their exceptions taken out and their changed instances put in, all
//! in the reader's wall-clock time.
//!
//! An [`Index`] keeps each event's first start and last end (none for a
//! rule that never ends) and the changed instances of each series, so a
//! month's occurrences cost a pass of comparisons plus the expansion of
//! the series that reach it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use crate::model::{Calendar, Event, Status, When, add, to_instant};

/// One showing of an event.
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    /// Indexes into the calendars and that calendar's events.
    pub cal: usize,
    pub event: usize,
    /// Wall-clock start and end where the reader is; the end is
    /// exclusive (midnight after the last day, for all-day events).
    pub start: DateTime,
    pub end: DateTime,
    pub all_day: bool,
    pub instant: Timestamp,
    pub end_instant: Timestamp,
    /// For an instance of a series: the start it was generated at, as
    /// its RECURRENCE-ID names it.
    pub rid: Option<When>,
    pub cancelled: bool,
}

impl Occurrence {
    /// The days it shows on (at least one).
    pub fn first_day(&self) -> Date {
        self.start.date()
    }

    /// The last day it shows on: an end at midnight belongs to the day
    /// before.
    pub fn last_day(&self) -> Date {
        let d = self.end.date();
        if self.end.time() == Time::midnight() && self.end > self.start { d.yesterday().unwrap_or(d) } else { d }
    }

    /// Whether it shows on more than one day (or is all-day): such events
    /// go in the all-day strip and month bars.
    pub fn spans_days(&self) -> bool {
        self.all_day || self.last_day() > self.first_day()
    }

    /// A stable identity: the calendar, the UID and which instance.
    pub fn key(&self, cals: &[Calendar]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        cals[self.cal].id.hash(&mut h);
        cals[self.cal].events[self.event].uid.hash(&mut h);
        match &self.rid {
            Some(r) => r.civil().hash(&mut h),
            None => cals[self.cal].events[self.event].recurrence_id.as_ref().map(|r| r.civil()).hash(&mut h),
        }
        h.finish()
    }
}

/// Where an event can show.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    first: Timestamp,
    /// None: it never ends.
    last: Option<Timestamp>,
}

/// What finding occurrences needs ready: each event's bounds, and each
/// series' changed instances.
#[derive(Clone, Debug, Default)]
pub struct Index {
    bounds: Vec<Vec<Bounds>>,
    /// Per calendar: UID to the events that change its instances.
    overrides: Vec<HashMap<String, Vec<usize>>>,
    /// Per calendar: UIDs that have a master.
    masters: Vec<HashMap<String, usize>>,
}

/// Slack for comparing wall-clock times in different zones: no two
/// offsets are more than 26 hours apart.
const SLACK: SignedDuration = SignedDuration::from_hours(27);

impl Index {
    pub fn new(cals: &[Calendar], local: &TimeZone) -> Index {
        let mut ix = Index::default();
        for c in cals {
            let mut b = Vec::with_capacity(c.events.len());
            let mut ov: HashMap<String, Vec<usize>> = HashMap::new();
            let mut ms: HashMap<String, usize> = HashMap::new();
            for (i, e) in c.events.iter().enumerate() {
                b.push(bounds(e, local));
                if e.recurrence_id.is_some() {
                    ov.entry(e.uid.clone()).or_default().push(i);
                } else {
                    ms.insert(e.uid.clone(), i);
                }
            }
            ix.bounds.push(b);
            ix.overrides.push(ov);
            ix.masters.push(ms);
        }
        ix
    }

    /// Calendar `ci` changed: index it again (the others stay).
    pub fn refresh(&mut self, ci: usize, cal: &Calendar, local: &TimeZone) {
        let one = Index::new(std::slice::from_ref(cal), local);
        if ci < self.bounds.len() {
            let Index { bounds, overrides, masters } = one;
            self.bounds[ci] = bounds.into_iter().next().unwrap_or_default();
            self.overrides[ci] = overrides.into_iter().next().unwrap_or_default();
            self.masters[ci] = masters.into_iter().next().unwrap_or_default();
        }
    }
}

fn bounds(e: &Event, local: &TimeZone) -> Bounds {
    let dur = e.duration(local);
    let start = e.start.instant(local);
    let end_of = |w: &When| -> Timestamp {
        let z = w.zone(local);
        to_instant(add(w.civil(), dur), &z)
    };
    let mut first = start;
    let mut last = Some(end_of(&e.start).max(start));
    if e.recurrence_id.is_none() {
        if let Some(r) = &e.rrule {
            last = if r.count.is_some() {
                // Walk it once to find its last instance.
                let mut at = e.start.civil();
                let n = std::cell::Cell::new(0u32);
                r.expand(e.start.civil(), e.until_civil(local), |dt| {
                    at = dt;
                    n.set(n.get() + 1);
                    n.get() < 100_000
                });
                Some(end_of(&e.start.with_civil(at)))
            } else {
                e.until_civil(local).map(|u| end_of(&e.start.with_civil(u)))
            };
        }
        for d in &e.rdates {
            let s = d.instant(local);
            first = first.min(s);
            if let Some(l) = last.as_mut() {
                *l = (*l).max(end_of(d));
            }
        }
    }
    Bounds { first, last }
}

/// Whether instance start `dt` (in `zone`) is the one `w` names.
fn same_instance(w: &When, dt: DateTime, all_day: bool, zone: &TimeZone, local: &TimeZone) -> bool {
    if all_day || w.is_date() {
        return w.civil().date() == dt.date();
    }
    w.instant(local) == to_instant(dt, zone)
}

/// The occurrences that overlap `[from, to)` (wall-clock time here), of
/// the calendars `shown`, in order of start (all-day first, longer
/// first). With `with_cancelled`, cancelled events are kept (marked).
pub fn occurrences(
    cals: &[Calendar],
    ix: &Index,
    shown: &[bool],
    local: &TimeZone,
    from: DateTime,
    to: DateTime,
) -> Vec<Occurrence> {
    let mut out = Vec::new();
    let from_i = to_instant(from, local);
    let to_i = to_instant(to, local);
    for (ci, cal) in cals.iter().enumerate() {
        if !shown.get(ci).copied().unwrap_or(true) {
            continue;
        }
        let Some(bounds) = ix.bounds.get(ci) else { continue };
        for (ei, e) in cal.events.iter().enumerate() {
            let Some(b) = bounds.get(ei) else { continue };
            // Quick refusal: entirely before or after.
            if b.first.checked_sub(SLACK).unwrap_or(b.first) >= to_i {
                continue;
            }
            if b.last.is_some_and(|l| l.checked_add(SLACK).unwrap_or(l) <= from_i) {
                continue;
            }
            if e.recurrence_id.is_some() {
                // A changed instance: shown on its own (unless it was
                // cancelled), whether or not its series is here.
                if e.status == Status::Cancelled && ix.masters[ci].contains_key(&e.uid) {
                    continue;
                }
                push_single(&mut out, ci, ei, e, e.recurrence_id.clone(), local, from, to);
                continue;
            }
            if !e.is_recurring() {
                push_single(&mut out, ci, ei, e, None, local, from, to);
                continue;
            }
            expand_series(
                &mut out,
                ci,
                ei,
                e,
                ix.overrides[ci].get(&e.uid).map(Vec::as_slice).unwrap_or(&[]),
                cal,
                local,
                from,
                to,
            );
        }
    }
    sort(&mut out);
    out
}

/// Order for showing: by day, all-day before timed, then by start, the
/// longer first.
pub fn sort(v: &mut [Occurrence]) {
    v.sort_by(|a, b| {
        a.start
            .date()
            .cmp(&b.start.date())
            .then(b.spans_days().cmp(&a.spans_days()))
            .then(a.start.cmp(&b.start))
            .then(b.end.cmp(&a.end))
            .then(a.cal.cmp(&b.cal))
            .then(a.event.cmp(&b.event))
    });
}

/// One occurrence at `start` (wall clock in `zone`) lasting like `e`.
fn make(ci: usize, ei: usize, e: &Event, start: &When, rid: Option<When>, local: &TimeZone) -> Occurrence {
    let dur = e.duration(local);
    if let When::Date(d) = start {
        let end = add(d.to_datetime(Time::midnight()), crate::ics::value::Duration::days(dur.days.max(1)));
        let s = d.to_datetime(Time::midnight());
        return Occurrence {
            cal: ci,
            event: ei,
            start: s,
            end,
            all_day: true,
            instant: to_instant(s, local),
            end_instant: to_instant(end, local),
            rid,
            cancelled: e.status == Status::Cancelled,
        };
    }
    let zone = start.zone(local);
    let s_i = to_instant(start.civil(), &zone);
    let e_i = to_instant(add(start.civil(), dur), &zone).max(s_i);
    Occurrence {
        cal: ci,
        event: ei,
        start: s_i.to_zoned(local.clone()).datetime(),
        end: e_i.to_zoned(local.clone()).datetime(),
        all_day: false,
        instant: s_i,
        end_instant: e_i,
        rid,
        cancelled: e.status == Status::Cancelled,
    }
}

fn overlaps(o: &Occurrence, from: DateTime, to: DateTime) -> bool {
    // An instant event (no length) shows where it starts.
    o.start < to && (o.end > from || (o.end == o.start && o.start >= from))
}

#[allow(clippy::too_many_arguments)]
fn push_single(
    out: &mut Vec<Occurrence>,
    ci: usize,
    ei: usize,
    e: &Event,
    rid: Option<When>,
    local: &TimeZone,
    from: DateTime,
    to: DateTime,
) {
    let o = make(ci, ei, e, &e.start, rid, local);
    if overlaps(&o, from, to) {
        out.push(o);
    }
}

#[allow(clippy::too_many_arguments)]
fn expand_series(
    out: &mut Vec<Occurrence>,
    ci: usize,
    ei: usize,
    e: &Event,
    overrides: &[usize],
    cal: &Calendar,
    local: &TimeZone,
    from: DateTime,
    to: DateTime,
) {
    let zone = e.start.zone(local);
    let all_day = e.all_day();
    let dur = e.duration(local);
    // How far before `from` an instance may start and still reach it.
    let reach = SignedDuration::from_secs(dur.total_seconds().max(0)) + SLACK + SignedDuration::from_hours(24);
    let near_from = from.checked_sub(reach).unwrap_or(from);
    let near_to = to.checked_add(SLACK).unwrap_or(to);
    let take = |dt: DateTime, out: &mut Vec<Occurrence>| {
        if dt < near_from {
            return;
        }
        if e.exdates.iter().any(|x| same_instance(x, dt, all_day, &zone, local)) {
            return;
        }
        if overrides.iter().any(|&oi| {
            cal.events[oi].recurrence_id.as_ref().is_some_and(|r| same_instance(r, dt, all_day, &zone, local))
        }) {
            return;
        }
        let w = e.start.with_civil(dt);
        let o = make(ci, ei, e, &w, Some(w.clone()), local);
        if overlaps(&o, from, to) {
            out.push(o);
        }
    };
    let start = e.start.civil();
    match &e.rrule {
        Some(r) => r.expand(start, e.until_civil(local), |dt| {
            if dt > near_to {
                return false;
            }
            take(dt, out);
            true
        }),
        None => take(start, out),
    }
    for d in &e.rdates {
        let dt = if d.is_date() == all_day {
            d.civil()
        } else if all_day {
            d.civil().date().to_datetime(Time::midnight())
        } else {
            // A date RDATE on a timed series: the series' time that day.
            d.civil().date().to_datetime(start.time())
        };
        let dt = match d {
            When::Utc(_) | When::Zoned(..) if !all_day => d.instant(local).to_zoned(zone.clone()).datetime(),
            _ => dt,
        };
        if dt != start {
            take(dt, out);
        }
    }
}

/// The instants alarms go off in `[from, to)`, with the occurrence they
/// belong to and which alarm.
pub fn alarms(
    cals: &[Calendar],
    ix: &Index,
    local: &TimeZone,
    from: Timestamp,
    to: Timestamp,
) -> Vec<(Timestamp, Occurrence, usize)> {
    // Alarms go off up to a week before their event (or after it):
    // look a little wider than asked, then keep what's in range.
    let widen = SignedDuration::from_hours(24 * 8);
    let f = from.checked_sub(widen).unwrap_or(from).to_zoned(local.clone()).datetime();
    let t = to.checked_add(widen).unwrap_or(to).to_zoned(local.clone()).datetime();
    let shown = vec![true; cals.len()];
    let mut out = Vec::new();
    for o in occurrences(cals, ix, &shown, local, f, t) {
        if o.cancelled {
            continue;
        }
        let e = &cals[o.cal].events[o.event];
        for (ai, a) in e.alarms.iter().enumerate() {
            if !a.is_active() {
                continue;
            }
            let at = match &a.trigger {
                crate::model::Trigger::Absolute(ts) => *ts,
                crate::model::Trigger::Relative { offset, from_end } => {
                    // Relative to the wall clock for all-day events.
                    let base = if *from_end { o.end } else { o.start };
                    if o.all_day {
                        to_instant(add(base, *offset), local)
                    } else {
                        let b = if *from_end { o.end_instant } else { o.instant };
                        b.checked_add(SignedDuration::from_secs(offset.total_seconds())).unwrap_or(b)
                    }
                }
            };
            if at >= from && at < to {
                out.push((at, o.clone(), ai));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.start.cmp(&b.1.start)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, datetime};

    fn cal(text: &str) -> Vec<Calendar> {
        vec![Calendar::parse("t", text).0]
    }

    fn days(v: &[Occurrence]) -> Vec<String> {
        v.iter().map(|o| o.start.to_string()).collect()
    }

    #[test]
    fn series_with_exceptions_and_moved_instances() {
        let cals = cal(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:s\nDTSTART;TZID=Europe/Berlin:20261005T100000\nDTEND;TZID=Europe/Berlin:20261005T110000\nRRULE:FREQ=WEEKLY;BYDAY=MO\nEXDATE;TZID=Europe/Berlin:20261012T100000\nSUMMARY:Weekly\nEND:VEVENT\nBEGIN:VEVENT\nUID:s\nRECURRENCE-ID;TZID=Europe/Berlin:20261019T100000\nDTSTART;TZID=Europe/Berlin:20261020T150000\nDTEND;TZID=Europe/Berlin:20261020T160000\nSUMMARY:Moved\nEND:VEVENT\nEND:VCALENDAR\n",
        );
        let berlin = TimeZone::get("Europe/Berlin").unwrap();
        let ix = Index::new(&cals, &berlin);
        let v = occurrences(
            &cals,
            &ix,
            &[true],
            &berlin,
            datetime(2026, 10, 1, 0, 0, 0, 0),
            datetime(2026, 11, 1, 0, 0, 0, 0),
        );
        assert_eq!(days(&v), ["2026-10-05T10:00:00", "2026-10-20T15:00:00", "2026-10-26T10:00:00"]);
        assert_eq!(cals[0].events[v[1].event].summary, "Moved");
        // Across the DST change (25 October) the wall-clock time holds.
        assert_eq!(v[2].end, datetime(2026, 10, 26, 11, 0, 0, 0));
        // Elsewhere it's the same instants.
        let ny = TimeZone::get("America/New_York").unwrap();
        let ix = Index::new(&cals, &ny);
        let v =
            occurrences(&cals, &ix, &[true], &ny, datetime(2026, 10, 1, 0, 0, 0, 0), datetime(2026, 11, 1, 0, 0, 0, 0));
        assert_eq!(v[0].start, datetime(2026, 10, 5, 4, 0, 0, 0));
    }

    #[test]
    fn all_day_and_long_events_reach_into_the_range() {
        let cals = cal(
            "BEGIN:VEVENT\nUID:a\nDTSTART;VALUE=DATE:20260928\nDTEND;VALUE=DATE:20261003\nSUMMARY:Trip\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nDTSTART;VALUE=DATE:20250101\nRRULE:FREQ=YEARLY\nSUMMARY:New Year\nEND:VEVENT\n",
        );
        let z = TimeZone::UTC;
        let ix = Index::new(&cals, &z);
        let v =
            occurrences(&cals, &ix, &[true], &z, datetime(2026, 10, 1, 0, 0, 0, 0), datetime(2026, 10, 2, 0, 0, 0, 0));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].last_day(), date(2026, 10, 2));
        let v =
            occurrences(&cals, &ix, &[true], &z, datetime(2027, 1, 1, 0, 0, 0, 0), datetime(2027, 1, 2, 0, 0, 0, 0));
        assert_eq!(days(&v), ["2027-01-01T00:00:00"]);
        assert!(v[0].all_day);
    }

    #[test]
    fn alarms_go_off_before_their_events() {
        let cals = cal(
            "BEGIN:VEVENT\nUID:a\nDTSTART:20261017T090000Z\nDTEND:20261017T100000Z\nSUMMARY:Call\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT15M\nEND:VALARM\nEND:VEVENT\nBEGIN:VEVENT\nUID:b\nDTSTART;VALUE=DATE:20261018\nSUMMARY:Birthday\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT15H\nEND:VALARM\nEND:VEVENT\n",
        );
        let z = TimeZone::get("Europe/London").unwrap();
        let ix = Index::new(&cals, &z);
        let from = datetime(2026, 10, 17, 0, 0, 0, 0).to_zoned(z.clone()).unwrap().timestamp();
        let to = datetime(2026, 10, 19, 0, 0, 0, 0).to_zoned(z.clone()).unwrap().timestamp();
        let a = alarms(&cals, &ix, &z, from, to);
        let times: Vec<String> = a.iter().map(|(t, _, _)| t.to_zoned(z.clone()).datetime().to_string()).collect();
        // 09:00Z is 10:00 in London (BST); the all-day one at 09:00 the day before.
        assert_eq!(times, ["2026-10-17T09:00:00", "2026-10-17T09:45:00"]);
    }
}
