//! The time as the household lives it: the local day, its weekday and the
//! minute, for daily limits and bedtime.
//!
//! The program asks a [`Clock`] for the time, so tests can set it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A moment in local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Local {
    pub year: i32,
    /// 1–12.
    pub month: u8,
    /// 1–31.
    pub day: u8,
    /// Monday is 0.
    pub weekday: u8,
    /// Minutes since midnight.
    pub minute: u16,
    pub second: u8,
}

impl Local {
    /// The date as `YYYY-MM-DD`: which day usage counts toward.
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `7:30 pm`-style, as a clock on the wall says it.
    pub fn time_label(&self) -> String {
        minute_label(self.minute)
    }

    /// Converted from the system's local time zone.
    pub fn of(t: SystemTime) -> Local {
        let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as libc::time_t;
        unsafe extern "C" {
            fn tzset();
        }
        // SAFETY: localtime_r writes the zeroed struct it's given; tzset
        // reads TZ and /etc/localtime again (the zone may have changed).
        let tm = unsafe {
            tzset();
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&secs, &mut tm);
            tm
        };
        Local {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u8,
            day: tm.tm_mday as u8,
            weekday: ((tm.tm_wday + 6) % 7) as u8,
            minute: (tm.tm_hour * 60 + tm.tm_min) as u16,
            second: tm.tm_sec.clamp(0, 59) as u8,
        }
    }
}

/// A minute of the day as `7:30 pm`.
pub fn minute_label(minute: u16) -> String {
    let h = (minute / 60) % 24;
    let m = minute % 60;
    let (h12, half) = match h {
        0 => (12, "am"),
        1..=11 => (h, "am"),
        12 => (12, "pm"),
        _ => (h - 12, "pm"),
    };
    format!("{h12}:{m:02} {half}")
}

/// Where the program gets the time.
pub trait Clock {
    fn now(&self) -> SystemTime;
    fn local(&self) -> Local;
}

/// The system's clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn local(&self) -> Local {
        Local::of(SystemTime::now())
    }
}

/// A clock the test moves: local time is UTC.
pub struct ManualClock {
    pub at: std::cell::Cell<SystemTime>,
}

impl ManualClock {
    /// `y-m-d h:m` in UTC.
    pub fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> ManualClock {
        let days = days_from_civil(year, month, day);
        let secs = days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60;
        ManualClock { at: std::cell::Cell::new(UNIX_EPOCH + Duration::from_secs(secs as u64)) }
    }

    pub fn advance(&self, d: Duration) {
        self.at.set(self.at.get() + d);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        self.at.get()
    }

    fn local(&self) -> Local {
        let secs = self.at.get().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Local {
            year,
            month,
            day,
            // 1970-01-01 was a Thursday.
            weekday: ((days + 3).rem_euclid(7)) as u8,
            minute: (rem / 60) as u16,
            second: (rem % 60) as u8,
        }
    }
}

// Howard Hinnant's days-from-civil and its inverse.
fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manual_clock_knows_its_weekday() {
        // 2026-10-31 is a Saturday.
        let c = ManualClock::at(2026, 10, 31, 19, 45);
        let l = c.local();
        assert_eq!((l.year, l.month, l.day, l.weekday, l.minute), (2026, 10, 31, 5, 19 * 60 + 45));
        assert_eq!(l.date(), "2026-10-31");
        c.advance(Duration::from_secs(5 * 3600));
        assert_eq!(c.local().date(), "2026-11-01");
        assert_eq!(c.local().weekday, 6);
    }

    #[test]
    fn minutes_read_as_a_clock_says_them() {
        assert_eq!(minute_label(0), "12:00 am");
        assert_eq!(minute_label(7 * 60 + 5), "7:05 am");
        assert_eq!(minute_label(12 * 60), "12:00 pm");
        assert_eq!(minute_label(19 * 60 + 30), "7:30 pm");
    }
}
