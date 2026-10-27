//! Sizes, dates, counts and permissions in words.

/// Bytes as people read them, in powers of 1000 as macOS and GNOME show
/// them: "512 bytes", "4.2 MB", "1.1 TB".
pub fn size(bytes: u64) -> String {
    match bytes {
        1 => "1 byte".into(),
        b if b < 1000 => format!("{b} bytes"),
        b => {
            let units = ["KB", "MB", "GB", "TB", "PB", "EB"];
            let mut v = b as f64 / 1000.0;
            let mut i = 0;
            while v >= 999.95 && i + 1 < units.len() {
                v /= 1000.0;
                i += 1;
            }
            if v < 10.0 { format!("{v:.1} {}", units[i]) } else { format!("{v:.0} {}", units[i]) }
        }
    }
}

/// A size with its bytes counted out, where the short form rounds:
/// "4.2 MB (4,194,304 bytes)", "287 bytes".
pub fn size_long(bytes: u64) -> String {
    if bytes < 1000 { size(bytes) } else { format!("{} ({} bytes)", size(bytes), group_digits(bytes)) }
}

/// 1234567 → "1,234,567".
pub fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "1 item", "12 items".
pub fn items(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{n} items") }
}

/// A local time's parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// Days since the epoch, in local time.
    pub days: i64,
}

/// Seconds since the epoch in local time.
pub fn local(secs: i64) -> Option<Local> {
    // SAFETY: `localtime_r` writes only into `tm`.
    unsafe {
        let t: libc::time_t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return None;
        }
        let local_secs = secs + tm.tm_gmtoff as i64;
        Some(Local {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            days: local_secs.div_euclid(86400),
        })
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A moment as a list shows it: "Today, 21:42", "Yesterday, 09:05", "17
/// Oct 2026, 21:42". `now` is the present in seconds.
pub fn date(secs: i64, now: i64) -> String {
    let (Some(t), Some(n)) = (local(secs), local(now)) else { return "—".into() };
    let time = format!("{:02}:{:02}", t.hour, t.minute);
    match n.days - t.days {
        0 => format!("Today, {time}"),
        1 => format!("Yesterday, {time}"),
        _ => format!("{} {} {}, {time}", t.day, MONTHS[(t.month - 1) as usize], t.year),
    }
}

/// A moment in full, for Get Info: "Saturday 17 October 2026, 21:42".
pub fn date_long(secs: i64) -> String {
    const DAYS: [&str; 7] = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"];
    const LONG: [&str; 12] = [
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
    let Some(t) = local(secs) else { return "—".into() };
    let weekday = DAYS[t.days.rem_euclid(7) as usize];
    format!("{weekday} {} {} {}, {:02}:{:02}", t.day, LONG[(t.month - 1) as usize], t.year, t.hour, t.minute)
}

/// The present, in seconds since the epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// `rwxr-xr-x`, with the special bits as `ls` shows them.
pub fn permissions(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (i, (bit, c)) in bits.iter().enumerate() {
        let on = mode & bit != 0;
        let special = match i {
            2 => mode & 0o4000 != 0,
            5 => mode & 0o2000 != 0,
            8 => mode & 0o1000 != 0,
            _ => false,
        };
        s.push(match (special, on, i) {
            (true, true, 8) => 't',
            (true, false, 8) => 'T',
            (true, true, _) => 's',
            (true, false, _) => 'S',
            (false, true, _) => *c,
            (false, false, _) => '-',
        });
    }
    s
}

/// What a class of people may do, in words: "Read and write", "Read
/// only", "No access".
pub fn access(read: bool, write: bool) -> &'static str {
    match (read, write) {
        (true, true) => "Read and write",
        (true, false) => "Read only",
        (false, true) => "Write only",
        (false, false) => "No access",
    }
}

/// A user's name, by ID.
pub fn user_name(uid: u32) -> String {
    // SAFETY: getpwuid_r writes into our buffers only.
    unsafe {
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 4096];
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) == 0 && !out.is_null() {
            return std::ffi::CStr::from_ptr(pw.pw_name).to_string_lossy().into_owned();
        }
    }
    uid.to_string()
}

/// A group's name, by ID.
pub fn group_name(gid: u32) -> String {
    // SAFETY: getgrgid_r writes into our buffers only.
    unsafe {
        let mut gr: libc::group = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 8192];
        let mut out: *mut libc::group = std::ptr::null_mut();
        if libc::getgrgid_r(gid, &mut gr, buf.as_mut_ptr(), buf.len(), &mut out) == 0 && !out.is_null() {
            return std::ffi::CStr::from_ptr(gr.gr_name).to_string_lossy().into_owned();
        }
    }
    gid.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 bytes");
        assert_eq!(size(1), "1 byte");
        assert_eq!(size(999), "999 bytes");
        assert_eq!(size(1000), "1.0 KB");
        assert_eq!(size(4_200_000), "4.2 MB");
        assert_eq!(size(12_345_678), "12 MB");
        assert_eq!(size(999_999), "1.0 MB");
        assert_eq!(size(1_100_000_000_000), "1.1 TB");
        assert_eq!(items(1), "1 item");
        assert_eq!(items(3), "3 items");
    }

    #[test]
    fn dates() {
        let now = 1_792_271_000; // 2026-10-17 and a bit, UTC
        let today = date(now - 60, now);
        assert!(today.starts_with("Today, ") || today.starts_with("Yesterday, "), "{today}");
        let old = date(0, now);
        assert!(old.contains("1970") || old.contains("1969"), "{old}");
        assert!(date_long(now).contains("2026"));
    }

    #[test]
    fn permissions_read_as_ls() {
        assert_eq!(permissions(0o755), "rwxr-xr-x");
        assert_eq!(permissions(0o640), "rw-r-----");
        assert_eq!(permissions(0o4755), "rwsr-xr-x");
        assert_eq!(permissions(0o1777), "rwxrwxrwt");
        assert_eq!(permissions(0o1776), "rwxrwxrwT");
        assert_eq!(user_name(0), "root");
    }
}
