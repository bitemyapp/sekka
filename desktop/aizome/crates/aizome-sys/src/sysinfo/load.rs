//! Load average from `/proc/loadavg`, uptime from `/proc/uptime`, and the
//! boot time from `btime` in `/proc/stat`.

use super::cpu::parse_proc_stat;
use super::read_text;
use crate::{Error, Result};
use std::time::{Duration, SystemTime};

const LOADAVG: &str = "/proc/loadavg";
const UPTIME: &str = "/proc/uptime";
const PROC_STAT: &str = "/proc/stat";

/// The run-queue averages over 1, 5 and 15 minutes, and the task counts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadAverage {
    /// Average number of runnable (and uninterruptible) tasks over the last
    /// minute.
    pub one: f32,
    /// Over the last 5 minutes.
    pub five: f32,
    /// Over the last 15 minutes.
    pub fifteen: f32,
    /// Tasks (threads) runnable right now.
    pub running: u32,
    /// Tasks (threads) in total.
    pub total: u32,
}

/// Read `/proc/loadavg`.
pub fn load_average() -> Result<LoadAverage> {
    let text = read_text(LOADAVG)?;
    parse_loadavg(&text).ok_or_else(|| Error::Failed(format!("{LOADAVG}: unexpected contents")))
}

/// Parse `/proc/loadavg`: `0.52 0.58 0.59 1/1750 544563`.
pub(crate) fn parse_loadavg(text: &str) -> Option<LoadAverage> {
    let mut fields = text.split_ascii_whitespace();
    let mut average = || fields.next()?.parse::<f32>().ok().filter(|n| n.is_finite() && *n >= 0.0);
    let (one, five, fifteen) = (average()?, average()?, average()?);
    let (running, total) = fields.next()?.split_once('/')?;
    Some(LoadAverage { one, five, fifteen, running: running.parse().ok()?, total: total.parse().ok()? })
}

/// How long the system has been up (`/proc/uptime`), including time spent
/// suspended.
pub fn uptime() -> Result<Duration> {
    let text = read_text(UPTIME)?;
    parse_uptime(&text).ok_or_else(|| Error::Failed(format!("{UPTIME}: unexpected contents")))
}

/// Parse `/proc/uptime`: `211244.41 472570.49` (up, and idle summed over
/// CPUs, in seconds).
pub(crate) fn parse_uptime(text: &str) -> Option<Duration> {
    let seconds: f64 = text.split_ascii_whitespace().next()?.parse().ok()?;
    Duration::try_from_secs_f64(seconds).ok()
}

/// When the system booted (`btime` in `/proc/stat`), to the second.
pub fn boot_time() -> Result<SystemTime> {
    let text = read_text(PROC_STAT)?;
    parse_proc_stat(&text)
        .boot_time
        .and_then(|seconds| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .ok_or_else(|| Error::Failed(format!("{PROC_STAT} has no btime")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loadavg_parses() {
        let load = parse_loadavg("1.19 3.15 2.25 1/1750 544563\n").unwrap();
        assert_eq!(load.one, 1.19);
        assert_eq!(load.five, 3.15);
        assert_eq!(load.fifteen, 2.25);
        assert_eq!(load.running, 1);
        assert_eq!(load.total, 1750);
    }

    #[test]
    fn odd_loadavg_is_none() {
        assert_eq!(parse_loadavg(""), None);
        assert_eq!(parse_loadavg("1 2 3"), None);
        assert_eq!(parse_loadavg("1 2 3 4 5"), None);
        assert_eq!(parse_loadavg("1 x 3 1/2 5"), None);
        assert_eq!(parse_loadavg("1 NaN 3 1/2 5"), None);
        assert_eq!(parse_loadavg("1 2 3 1/ 5"), None);
    }

    #[test]
    fn uptime_parses() {
        assert_eq!(parse_uptime("211244.41 472570.49\n"), Some(Duration::from_millis(211_244_410)));
        assert_eq!(parse_uptime(""), None);
        assert_eq!(parse_uptime("-5 3"), None);
        assert_eq!(parse_uptime("inf 3"), None);
        assert_eq!(parse_uptime("NaN"), None);
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_load_uptime_and_boot_time_are_sane() {
        let load = load_average().unwrap();
        let up = uptime().unwrap();
        let boot = boot_time().unwrap();
        println!("load {load:?}, up {up:?}, booted {boot:?}");
        assert!(load.total >= 1);
        assert!(up > Duration::ZERO);
        // Boot time plus uptime is now, give or take suspends and clock
        // changes (a suspended machine's uptime keeps counting, so the sum
        // stays close).
        let now = SystemTime::now();
        let drift = (boot + up).duration_since(now).unwrap_or_else(|e| e.duration());
        assert!(drift < Duration::from_secs(120), "drift {drift:?}");
    }
}
