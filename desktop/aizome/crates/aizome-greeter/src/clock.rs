//! The time and date in the computer's time zone (libc's `localtime_r`,
//! which reads `/etc/localtime`), as the lock screen shows them.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Now {
    pub year: i32,
    /// 1–12.
    pub month: u32,
    pub day: u32,
    /// 0 is Sunday.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
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

impl Now {
    pub fn local() -> Now {
        // SAFETY: tzset rereads TZ and /etc/localtime; localtime_r writes
        // only into the supplied tm. Refresh before converting: localtime_r
        // alone may keep the time zone cached across a Settings change.
        unsafe extern "C" {
            fn tzset();
        }
        unsafe {
            tzset();
            let t = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&t, &mut tm).is_null() {
                return Now::default();
            }
            Now {
                year: tm.tm_year + 1900,
                month: (tm.tm_mon + 1) as u32,
                day: tm.tm_mday as u32,
                weekday: tm.tm_wday as u32,
                hour: tm.tm_hour as u32,
                minute: tm.tm_min as u32,
                second: tm.tm_sec as u32,
            }
        }
    }

    /// `21:42`.
    pub fn time(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// `Tuesday 20 October`.
    pub fn date(&self) -> String {
        let w = WEEKDAYS.get(self.weekday as usize).copied().unwrap_or("");
        let m = MONTHS.get(self.month.saturating_sub(1) as usize).copied().unwrap_or("");
        format!("{w} {} {m}", self.day)
    }

    /// Day, by the wallpaper's clock: 06:00 to 18:00.
    pub fn is_day(&self) -> bool {
        (6..18).contains(&self.hour)
    }

    /// How long until the minute turns.
    pub fn until_next_minute(&self) -> Duration {
        Duration::from_millis(u64::from(60 - self.second.min(59)) * 1000 + 50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_screens_words() {
        let n = Now { year: 2026, month: 10, day: 20, weekday: 2, hour: 7, minute: 5, second: 30 };
        assert_eq!(n.time(), "07:05");
        assert_eq!(n.date(), "Tuesday 20 October");
        assert!(n.is_day());
        assert!(!Now { hour: 21, ..n }.is_day());
        assert_eq!(n.until_next_minute(), Duration::from_millis(30_050));
        let real = Now::local();
        assert!((1..=12).contains(&real.month) && real.year >= 2026);
    }
}
