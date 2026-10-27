//! Local time for the bar's clock, the lock screen and the calendar panel,
//! from libc's `localtime_r` (the time zone follows `/etc/localtime` and
//! `$TZ`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aizome_config::ClockFormat;

/// A moment in local time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Local {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 is Sunday.
    pub weekday: u32,
}

pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
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

impl Local {
    pub fn now() -> Local {
        Local::at(SystemTime::now())
    }

    pub fn at(t: SystemTime) -> Local {
        let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
        // SAFETY: tzset rereads TZ and /etc/localtime; localtime_r writes
        // only into the supplied tm. Refresh before converting: localtime_r
        // alone may keep the time zone cached across a Settings change.
        unsafe extern "C" {
            fn tzset();
        }
        unsafe {
            tzset();
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&secs, &mut tm).is_null() {
                return Local::default();
            }
            Local {
                year: tm.tm_year + 1900,
                month: (tm.tm_mon + 1) as u32,
                day: tm.tm_mday as u32,
                hour: tm.tm_hour as u32,
                minute: tm.tm_min as u32,
                second: tm.tm_sec.min(59) as u32,
                weekday: tm.tm_wday as u32,
            }
        }
    }

    /// Minutes past midnight.
    pub fn minutes(&self) -> u32 {
        self.hour * 60 + self.minute
    }

    /// The bar's clock: `21:42`, `9:42 PM`, with seconds or the date if
    /// asked.
    pub fn bar_label(&self, format: ClockFormat, seconds: bool, date: bool) -> String {
        let mut s = String::new();
        if date {
            s.push_str(&format!("{} {} ", &WEEKDAYS[self.weekday as usize % 7][..3], self.day));
        }
        s.push_str(&self.time_label(format, seconds));
        s
    }

    pub fn time_label(&self, format: ClockFormat, seconds: bool) -> String {
        let (h, suffix) = match format {
            ClockFormat::H24 => (format!("{:02}", self.hour), ""),
            ClockFormat::H12 => {
                let h = if self.hour.is_multiple_of(12) { 12 } else { self.hour % 12 };
                (h.to_string(), if self.hour < 12 { " AM" } else { " PM" })
            }
        };
        if seconds {
            format!("{h}:{:02}:{:02}{suffix}", self.minute, self.second)
        } else {
            format!("{h}:{:02}{suffix}", self.minute)
        }
    }

    /// "Friday 17 October".
    pub fn date_label(&self) -> String {
        format!("{} {} {}", WEEKDAYS[self.weekday as usize % 7], self.day, MONTHS[(self.month as usize + 11) % 12])
    }
}

/// How long until the clock's label changes: the next minute (or second).
pub fn until_next_tick(seconds: bool) -> Duration {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let period: u64 = if seconds { 1_000 } else { 60_000 };
    let ms = now.as_millis() as u64;
    // A few milliseconds late, so the label has turned when it's read.
    Duration::from_millis(period - ms % period + 5)
}

/// Days in a month (1–12).
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// The weekday (0 Sunday) of a date, by Sakamoto's method.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    ((y + y / 4 - y / 100 + y / 400 + T[(month as usize + 11) % 12] + day as i32).rem_euclid(7)) as u32
}

/// A month as weeks of days (Monday first), `None` outside it.
pub fn month_grid(year: i32, month: u32) -> Vec<[Option<u32>; 7]> {
    let first = (weekday(year, month, 1) + 6) % 7; // Monday = 0
    let days = days_in_month(year, month);
    let mut weeks = Vec::new();
    let mut week = [None; 7];
    let mut col = first as usize;
    for d in 1..=days {
        week[col] = Some(d);
        col += 1;
        if col == 7 {
            weeks.push(week);
            week = [None; 7];
            col = 0;
        }
    }
    if col > 0 {
        weeks.push(week);
    }
    weeks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> Local {
        Local { year: 2026, month: 10, day: 17, hour: h, minute: m, second: 5, weekday: 6 }
    }

    #[test]
    fn labels_in_both_formats() {
        assert_eq!(at(21, 42).bar_label(ClockFormat::H24, false, false), "21:42");
        assert_eq!(at(9, 5).bar_label(ClockFormat::H24, false, false), "09:05");
        assert_eq!(at(21, 42).bar_label(ClockFormat::H12, false, false), "9:42 PM");
        assert_eq!(at(0, 1).bar_label(ClockFormat::H12, false, false), "12:01 AM");
        assert_eq!(at(21, 42).bar_label(ClockFormat::H24, true, true), "Sat 17 21:42:05");
        assert_eq!(at(21, 42).date_label(), "Saturday 17 October");
    }

    #[test]
    fn calendars() {
        assert_eq!(weekday(2026, 10, 17), 6);
        assert_eq!(weekday(2024, 2, 29), 4);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        let g = month_grid(2026, 10);
        // October 2026 starts on a Thursday.
        assert_eq!(g[0], [None, None, None, Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(g.last().unwrap().iter().flatten().max(), Some(&31));
    }

    #[test]
    fn ticks_land_on_the_minute() {
        let d = until_next_tick(false);
        assert!(d <= Duration::from_millis(60_005) && d > Duration::ZERO);
        assert!(until_next_tick(true) <= Duration::from_millis(1_005));
    }
}
