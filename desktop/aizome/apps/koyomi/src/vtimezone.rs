//! `VTIMEZONE`s for the IANA zones Koyomi's own events use, so other
//! programs that don't know the name can still place the times: the
//! zone's present rules as yearly STANDARD and DAYLIGHT observances
//! (or one fixed STANDARD), with `X-LIC-LOCATION` naming the zone.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};

use crate::ics::{Component, Prop, value};
use crate::rrule::weekday_code;

fn offset_text(secs: i32) -> String {
    let sign = if secs < 0 { '-' } else { '+' };
    let a = secs.abs();
    if a % 60 != 0 {
        format!("{sign}{:02}{:02}{:02}", a / 3600, (a % 3600) / 60, a % 60)
    } else {
        format!("{sign}{:02}{:02}", a / 3600, (a % 3600) / 60)
    }
}

/// A `VTIMEZONE` for IANA zone `id` (none if there's no such zone).
pub fn make(id: &str) -> Option<Component> {
    let zone = TimeZone::get(id).ok()?;
    let mut c = Component::new("VTIMEZONE");
    c.push(Prop::new("TZID", id));
    c.push(Prop::new("X-LIC-LOCATION", id));
    let now = Timestamp::now();
    let year = now.to_zoned(TimeZone::UTC).year();
    let from = Date::new(year, 1, 1).ok()?.to_zoned(TimeZone::UTC).ok()?.timestamp();
    let to = Date::new(year + 1, 1, 1).ok()?.to_zoned(TimeZone::UTC).ok()?.timestamp();
    let transitions: Vec<_> = zone.following(from).take_while(|t| t.timestamp() < to).take(4).collect();
    if transitions.len() != 2 {
        // No daylight time (or something odd): the offset now, fixed.
        let info = zone.to_offset_info(now);
        let mut s = Component::new("STANDARD");
        s.push(Prop::new("DTSTART", "19700101T000000"));
        let off = offset_text(info.offset().seconds());
        s.push(Prop::new("TZOFFSETFROM", off.clone()));
        s.push(Prop::new("TZOFFSETTO", off));
        s.push(Prop::text("TZNAME", info.abbreviation()));
        c.children.push(s);
        return Some(c);
    }
    for t in &transitions {
        let before = zone.to_offset(t.timestamp().checked_sub(1.second()).ok()?);
        let after = t.offset();
        // The wall-clock time of the change, in the offset before it.
        let local = before.to_datetime(t.timestamp());
        let d = local.date();
        let week: i8 = if d.day() > d.days_in_month() - 7 { -1 } else { (d.day() - 1) / 7 + 1 };
        let wd = d.weekday();
        // The same rule's day in 1970, as DTSTART.
        let first = Date::new(1970, d.month(), 1).ok()?;
        let start = if week < 0 {
            first.last_of_month().nth_weekday_of_month(-1, wd).ok()?
        } else {
            first.nth_weekday_of_month(week, wd).ok()?
        };
        let mut o = Component::new(if t.dst().is_dst() { "DAYLIGHT" } else { "STANDARD" });
        o.push(Prop::new("DTSTART", value::format_datetime(start.to_datetime(local.time()), false)));
        o.push(Prop::new("TZOFFSETFROM", offset_text(before.seconds())));
        o.push(Prop::new("TZOFFSETTO", offset_text(after.seconds())));
        o.push(Prop::new("RRULE", format!("FREQ=YEARLY;BYMONTH={};BYDAY={}{}", d.month(), week, weekday_code(wd))));
        o.push(Prop::text("TZNAME", t.abbreviation()));
        c.children.push(o);
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tz::from_vtimezone;
    use jiff::civil::datetime;

    #[test]
    fn made_zones_read_back_as_the_same_rules() {
        for id in ["Europe/London", "America/New_York", "Australia/Sydney", "Asia/Tokyo", "Asia/Kolkata"] {
            let c = make(id).unwrap();
            let back = from_vtimezone(&c).unwrap();
            let real = TimeZone::get(id).unwrap();
            for (m, d) in [(1, 15), (4, 15), (7, 15), (10, 15), (12, 15)] {
                let dt = datetime(2026, m, d, 12, 0, 0, 0);
                let a = dt.to_zoned(back.clone()).unwrap().offset();
                let b = dt.to_zoned(real.clone()).unwrap().offset();
                assert_eq!(a, b, "{id} {dt}");
            }
        }
    }
}
