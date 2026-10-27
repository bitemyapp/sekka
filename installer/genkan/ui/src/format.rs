// SPDX-License-Identifier: GPL-3.0-or-later
//! Sizes, clocks and durations as the pages write them.

const KIB: f64 = 1024.0;

/// Human-readable binary size: "512 MiB", "31.4 GiB", "1.82 TiB".
pub fn size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes >= KIB.powi(4) {
        format!("{:.2} TiB", bytes / KIB.powi(4))
    } else if bytes >= KIB.powi(3) {
        format!("{:.1} GiB", bytes / KIB.powi(3))
    } else {
        format!("{:.0} MiB", bytes / KIB.powi(2))
    }
}

/// "0:07", "1:42", "1:02:05".
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "47 seconds", "1 minute 12 seconds".
pub fn duration(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let (m, s) = (total / 60, total % 60);
    let unit = |n: u64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    if m == 0 {
        unit(s, "second")
    } else if s == 0 {
        unit(m, "minute")
    } else {
        format!("{} {}", unit(m, "minute"), unit(s, "second"))
    }
}

/// "UTC", "UTC+05:30", "UTC−03:00" (with a minus sign).
pub fn utc_offset(seconds: i64) -> String {
    if seconds == 0 {
        return "UTC".into();
    }
    let sign = if seconds < 0 { '\u{2212}' } else { '+' };
    let minutes = seconds.abs() / 60;
    format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_and_durations_read_naturally() {
        assert_eq!(size(512 * 1024 * 1024), "512 MiB");
        assert_eq!(size(32 * 1024u64.pow(3)), "32.0 GiB");
        assert_eq!(size(2 * 1024u64.pow(4)), "2.00 TiB");
        assert_eq!(clock(7), "0:07");
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(duration(47.4), "47 seconds");
        assert_eq!(duration(60.0), "1 minute");
        assert_eq!(duration(72.0), "1 minute 12 seconds");
        assert_eq!(duration(1.0), "1 second");
    }
    #[test]
    fn offsets_use_a_minus_sign() {
        assert_eq!(utc_offset(0), "UTC");
        assert_eq!(utc_offset(19800), "UTC+05:30");
        assert_eq!(utc_offset(-10800), "UTC\u{2212}03:00");
    }
}
