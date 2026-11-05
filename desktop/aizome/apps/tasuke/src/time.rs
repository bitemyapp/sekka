//! Times as people read them: "4 min", "yesterday", "Mon", "28 Oct" in
//! the sidebar; "today", "3 days ago" for notes; "Good evening" on the
//! first page. Days are local days, so a [`Clock`] carries the zone's
//! offset as well as the time, and fixtures fix both.

use crate::transcript::Millis;

/// Now, and how far the local zone is from UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub now: Millis,
    /// Seconds east of UTC.
    pub offset: i32,
}

const DAY: i64 = 86_400;
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

impl Clock {
    /// This computer's clock and zone.
    pub fn system() -> Clock {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        Clock { now, offset: local_offset(now / 1000) }
    }

    /// The local day `at` falls on, counted from 1 January 1970.
    fn day(&self, at: Millis) -> i64 {
        (at.div_euclid(1000) + i64::from(self.offset)).div_euclid(DAY)
    }

    /// Seconds into the local day.
    fn second_of_day(&self, at: Millis) -> i64 {
        (at.div_euclid(1000) + i64::from(self.offset)).rem_euclid(DAY)
    }

    /// For a list beside its titles: "now", "4 min", "2 h", "yesterday",
    /// a weekday within the week, then a date.
    pub fn brief(&self, at: Millis) -> String {
        let ago = (self.now - at).max(0) / 1000;
        let days = self.day(self.now) - self.day(at);
        match (ago, days) {
            (0..60, _) => "now".into(),
            (60..3600, 0) => format!("{} min", ago / 60),
            (_, 0) => format!("{} h", ago / 3600),
            (_, 1) => "yesterday".into(),
            (_, 2..7) => WEEKDAYS[weekday(self.day(at))].into(),
            _ => self.date(at),
        }
    }

    /// For when a note was made: "today", "yesterday", "3 days ago", a date.
    pub fn when(&self, at: Millis) -> String {
        match self.day(self.now) - self.day(at) {
            ..=0 => "today".into(),
            1 => "yesterday".into(),
            d @ 2..7 => format!("{d} days ago"),
            _ => self.date(at),
        }
    }

    /// "28 Oct", with the year when it isn't this year's.
    pub fn date(&self, at: Millis) -> String {
        let (y, m, d) = civil(self.day(at));
        let (this_year, _, _) = civil(self.day(self.now));
        if y == this_year { format!("{d} {}", MONTHS[m - 1]) } else { format!("{d} {} {y}", MONTHS[m - 1]) }
    }

    /// "Good morning" before noon, "Good afternoon" until six, then
    /// "Good evening" (and after midnight, until four, still evening).
    pub fn greeting(&self) -> &'static str {
        match self.second_of_day(self.now) / 3600 {
            4..12 => "Good morning",
            12..18 => "Good afternoon",
            _ => "Good evening",
        }
    }
}

/// Monday 0 … Sunday 6 of a day counted from 1970-01-01 (a Thursday).
fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

/// The proleptic Gregorian date of a day counted from 1970-01-01
/// (Howard Hinnant's `civil_from_days`).
fn civil(day: i64) -> (i64, usize, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The local zone's offset from UTC at `secs`, from the C library.
fn local_offset(secs: i64) -> i32 {
    let t: libc::time_t = secs as libc::time_t;
    // SAFETY: `tm` is plain data the call fills; `localtime_r` is the
    // reentrant form and keeps no pointer to either argument.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thursday 5 November 2026, 21:30 in Chicago (UTC−6).
    const NOW: Clock = Clock { now: 1_793_935_800_000, offset: -6 * 3600 };

    fn ago(secs: i64) -> Millis {
        NOW.now - secs * 1000
    }

    #[test]
    fn the_clock_is_where_it_says() {
        assert_eq!(civil(NOW.day(NOW.now)), (2026, 11, 5));
        assert_eq!(WEEKDAYS[weekday(NOW.day(NOW.now))], "Thu");
        assert_eq!(NOW.second_of_day(NOW.now) / 3600, 21);
        assert_eq!(NOW.greeting(), "Good evening");
    }

    #[test]
    fn brief_times_for_the_sidebar() {
        assert_eq!(NOW.brief(ago(20)), "now");
        assert_eq!(NOW.brief(ago(4 * 60)), "4 min");
        assert_eq!(NOW.brief(ago(3 * 3600)), "3 h");
        // 22 hours ago is yesterday's 23:30.
        assert_eq!(NOW.brief(ago(22 * 3600)), "yesterday");
        assert_eq!(NOW.brief(ago(3 * DAY)), "Mon");
        assert_eq!(NOW.brief(ago(9 * DAY)), "27 Oct");
        assert_eq!(NOW.brief(ago(400 * DAY)), "1 Oct 2025");
        // A clock a little behind a file's time isn't the future.
        assert_eq!(NOW.brief(NOW.now + 5_000), "now");
    }

    #[test]
    fn when_a_note_was_made() {
        assert_eq!(NOW.when(ago(3600)), "today");
        assert_eq!(NOW.when(ago(DAY)), "yesterday");
        assert_eq!(NOW.when(ago(4 * DAY)), "4 days ago");
        assert_eq!(NOW.when(ago(30 * DAY)), "6 Oct");
    }

    #[test]
    fn greetings_follow_the_local_hour() {
        let at = |h: i64| Clock { now: (NOW.day(NOW.now) * DAY + h * 3600 + 6 * 3600) * 1000, ..NOW };
        assert_eq!(at(9).greeting(), "Good morning");
        assert_eq!(at(13).greeting(), "Good afternoon");
        assert_eq!(at(2).greeting(), "Good evening");
    }
}
