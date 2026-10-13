//! Upload times as the window shows them.

/// "2026-10-13T02:51:44Z", as the publisher writes it, in Unix seconds.
pub fn parse_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, min, sec) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86400 + h * 3600 + min * 60 + sec)
}

/// Days since 1970-01-01 of a Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "13 Oct 2026, 02:51 UTC".
pub fn day_and_time(t: i64) -> String {
    let (y, m, d) = civil_from_days(t.div_euclid(86400));
    let s = t.rem_euclid(86400);
    format!("{d} {} {y}, {:02}:{:02} UTC", MONTHS[(m - 1) as usize], s / 3600, s % 3600 / 60)
}

/// "3 hours ago".
pub fn age(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    let ago = |n: i64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match minutes {
        0 => "just now".into(),
        1..60 => ago(minutes, "minute"),
        60..1440 => ago(minutes / 60, "hour"),
        _ => ago(minutes / 1440, "day"),
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "2:05" for a duration in seconds.
pub fn duration(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}:{:02}:{:02}", seconds / 3600, seconds % 3600 / 60, seconds % 60)
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times() {
        let t = parse_utc("2026-10-13T08:42:05Z").unwrap();
        assert_eq!(t, 1_791_880_925);
        assert_eq!(day_and_time(t), "13 Oct 2026, 08:42 UTC");
        assert_eq!(age(3 * 3600 + 5), "3 hours ago");
        assert_eq!(age(86400), "1 day ago");
        assert_eq!(duration(125), "2:05");
        assert_eq!(duration(3725), "1:02:05");
        assert_eq!(parse_utc("2026-10-13 08:42:05Z"), None);
    }
}
