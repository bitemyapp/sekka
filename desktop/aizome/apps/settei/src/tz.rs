//! A time zone's offset from UTC at a moment, from tzdata's TZif files and
//! their POSIX rule for times past the last transition (tzdata ships
//! "slim" files whose rule carries every future daylight-saving change).
//! Pure computation, so the sample's clocks stay still.

use std::path::Path;

/// The offset east of UTC, in seconds, of zone `id` (under `dir`) at Unix
/// time `now`.
pub fn offset(dir: &Path, id: &str, now: i64) -> Option<i32> {
    if id.contains("..") {
        return None;
    }
    let bytes = std::fs::read(dir.join(id)).ok()?;
    offset_in(&bytes, now)
}

/// The offset in a TZif file's bytes at `now`.
pub fn offset_in(b: &[u8], now: i64) -> Option<i32> {
    let h = header(b, 0)?;
    let v1_len = h.data_len(4);
    let (data_at, h, wide) = if h.version >= b'2' {
        let at = 44 + v1_len;
        (at + 44, header(b, at)?, true)
    } else {
        (44, h, false)
    };
    let tsize = if wide { 8 } else { 4 };
    let times_at = data_at;
    let idx_at = times_at + h.timecnt * tsize;
    let types_at = idx_at + h.timecnt;
    let footer_at = data_at + h.data_len(tsize);
    let time = |i: usize| -> Option<i64> {
        let s = b.get(times_at + i * tsize..times_at + (i + 1) * tsize)?;
        Some(if wide {
            i64::from_be_bytes(s.try_into().ok()?)
        } else {
            i64::from(i32::from_be_bytes(s.try_into().ok()?))
        })
    };
    let utoff = |t: usize| -> Option<i32> {
        let s = b.get(types_at + t * 6..types_at + t * 6 + 4)?;
        Some(i32::from_be_bytes(s.try_into().ok()?))
    };
    // The last transition at or before now.
    let mut found: Option<usize> = None;
    let (mut lo, mut hi) = (0usize, h.timecnt);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if time(mid)? <= now {
            found = Some(mid);
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let past_end = h.timecnt == 0 || found == Some(h.timecnt - 1);
    if wide && past_end {
        let rest = b.get(footer_at..)?;
        let text = std::str::from_utf8(rest).ok()?.trim_matches('\n');
        let rule = text.split('\n').next().unwrap_or("");
        if !rule.is_empty()
            && let Some(o) = posix_offset(rule, now)
        {
            return Some(o);
        }
    }
    match found {
        Some(i) => utoff(usize::from(*b.get(idx_at + i)?)),
        None => utoff(0),
    }
}

struct Header {
    version: u8,
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

impl Header {
    fn data_len(&self, tsize: usize) -> usize {
        self.timecnt * tsize
            + self.timecnt
            + self.typecnt * 6
            + self.charcnt
            + self.leapcnt * (tsize + 4)
            + self.isstdcnt
            + self.isutcnt
    }
}

fn header(b: &[u8], at: usize) -> Option<Header> {
    if b.get(at..at + 4)? != b"TZif" {
        return None;
    }
    let n = |i: usize| -> Option<usize> {
        Some(u32::from_be_bytes(b.get(at + 20 + i * 4..at + 24 + i * 4)?.try_into().ok()?) as usize)
    };
    Some(Header {
        version: *b.get(at + 4)?,
        isutcnt: n(0)?,
        isstdcnt: n(1)?,
        leapcnt: n(2)?,
        timecnt: n(3)?,
        typecnt: n(4)?,
        charcnt: n(5)?,
    })
}

/// A POSIX TZ rule's offset east of UTC at `now`: `CST6CDT,M3.2.0,M11.1.0`.
pub fn posix_offset(rule: &str, now: i64) -> Option<i32> {
    let mut p = Parser { s: rule.as_bytes(), i: 0 };
    p.name()?;
    let std = -p.offset()?;
    if p.done() {
        return Some(std);
    }
    p.name()?;
    let dst = if p.peek().is_none_or(|c| c == b',') { std + 3600 } else { -p.offset()? };
    if p.done() {
        return Some(std);
    }
    p.expect(b',')?;
    let (start, start_t) = p.rule()?;
    p.expect(b',')?;
    let (end, end_t) = p.rule()?;
    // The year of `now` in standard time.
    let (year, _, _) = civil_from_days((now + i64::from(std)).div_euclid(86400));
    let at = |r: &Rule, t: i32, off: i32, y: i64| days_from_civil_rule(r, y) * 86400 + i64::from(t) - i64::from(off);
    let s = at(&start, start_t, std, year);
    let e = at(&end, end_t, dst, year);
    let in_dst = if s < e { now >= s && now < e } else { !(now >= e && now < s) };
    Some(if in_dst { dst } else { std })
}

enum Rule {
    /// Month, week (5 = last), weekday (0 = Sunday).
    M(u32, u32, u32),
    /// Day 1–365, February 29 never counted.
    J(u32),
    /// Day 0–365, counting February 29.
    N(u32),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn done(&self) -> bool {
        self.i >= self.s.len()
    }

    fn expect(&mut self, c: u8) -> Option<()> {
        (self.peek()? == c).then(|| self.i += 1)
    }

    fn name(&mut self) -> Option<()> {
        if self.peek()? == b'<' {
            while self.peek()? != b'>' {
                self.i += 1;
            }
            self.i += 1;
            return Some(());
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        (self.i - start >= 3).then_some(())
    }

    fn number(&mut self) -> Option<i32> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }

    /// `[+-]hh[:mm[:ss]]` as seconds.
    fn offset(&mut self) -> Option<i32> {
        let sign = match self.peek()? {
            b'-' => {
                self.i += 1;
                -1
            }
            b'+' => {
                self.i += 1;
                1
            }
            _ => 1,
        };
        let h = self.number()?;
        let mut secs = h * 3600;
        if self.peek() == Some(b':') {
            self.i += 1;
            secs += self.number()? * 60;
            if self.peek() == Some(b':') {
                self.i += 1;
                secs += self.number()?;
            }
        }
        Some(sign * secs)
    }

    fn rule(&mut self) -> Option<(Rule, i32)> {
        let r = match self.peek()? {
            b'M' => {
                self.i += 1;
                let m = self.number()?;
                self.expect(b'.')?;
                let w = self.number()?;
                self.expect(b'.')?;
                let d = self.number()?;
                Rule::M(m as u32, w as u32, d as u32)
            }
            b'J' => {
                self.i += 1;
                Rule::J(self.number()? as u32)
            }
            _ => Rule::N(self.number()? as u32),
        };
        let t = if self.peek() == Some(b'/') {
            self.i += 1;
            self.offset()?
        } else {
            7200
        };
        Some((r, t))
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The civil date of a day since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Sunday.
pub fn weekday(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

fn days_from_civil_rule(r: &Rule, y: i64) -> i64 {
    match *r {
        Rule::J(n) => {
            let mut d = days_from_civil(y, 1, 1) + i64::from(n) - 1;
            if is_leap(y) && n >= 60 {
                d += 1;
            }
            d
        }
        Rule::N(n) => days_from_civil(y, 1, 1) + i64::from(n),
        Rule::M(m, w, wd) => {
            let first = days_from_civil(y, m, 1);
            let shift = (i64::from(wd) - i64::from(weekday(first))).rem_euclid(7);
            let mut d = first + shift + 7 * (i64::from(w) - 1);
            let next_month = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
            while d >= next_month {
                d -= 7;
            }
            d
        }
    }
}

/// "UTC+9", "UTC−5:30", "UTC".
pub fn describe(offset: i32) -> String {
    if offset == 0 {
        return "UTC".into();
    }
    let sign = if offset < 0 { "−" } else { "+" };
    let a = offset.abs();
    let (h, m) = (a / 3600, (a % 3600) / 60);
    if m == 0 { format!("UTC{sign}{h}") } else { format!("UTC{sign}{h}:{m:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trips() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(days_from_civil(2026, 10, 17)), (2026, 10, 17));
        assert_eq!(weekday(days_from_civil(2026, 10, 17)), 6, "a Saturday");
    }

    #[test]
    fn posix_rules() {
        // 2026-10-17 12:00 UTC: Chicago on daylight time, Sydney too.
        let oct = days_from_civil(2026, 10, 17) * 86400 + 12 * 3600;
        let jan = days_from_civil(2026, 1, 15) * 86400;
        assert_eq!(posix_offset("CST6CDT,M3.2.0,M11.1.0", oct), Some(-5 * 3600));
        assert_eq!(posix_offset("CST6CDT,M3.2.0,M11.1.0", jan), Some(-6 * 3600));
        assert_eq!(posix_offset("AEST-10AEDT,M10.1.0,M4.1.0/3", oct), Some(11 * 3600));
        assert_eq!(posix_offset("AEST-10AEDT,M10.1.0,M4.1.0/3", days_from_civil(2026, 6, 1) * 86400), Some(10 * 3600));
        assert_eq!(posix_offset("JST-9", oct), Some(9 * 3600));
        assert_eq!(posix_offset("<+0530>-5:30", oct), Some(5 * 3600 + 1800));
        assert_eq!(describe(5 * 3600 + 1800), "UTC+5:30");
        assert_eq!(describe(-5 * 3600), "UTC−5");
    }

    #[test]
    fn reads_tzdata_when_present() {
        let Some(dir) = aizome_sys::datetime::zones::zoneinfo_dir() else { return };
        let oct = days_from_civil(2026, 10, 17) * 86400 + 12 * 3600;
        assert_eq!(offset(&dir, "Asia/Tokyo", oct), Some(9 * 3600));
        assert_eq!(offset(&dir, "America/Chicago", oct), Some(-5 * 3600));
        assert_eq!(offset(&dir, "Australia/Sydney", oct), Some(11 * 3600));
        assert_eq!(offset(&dir, "../../etc/passwd", oct), None);
    }
}
