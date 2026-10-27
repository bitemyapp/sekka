//! The timer and the stopwatch, on the clock's monotonic time, and how
//! their times read.

use std::time::Duration;

/// A countdown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timer {
    /// What it was set to.
    pub length: Duration,
    pub state: TimerState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TimerState {
    /// Set but not started (or reset).
    #[default]
    Ready,
    /// Counting down to the monotonic time `ends`.
    Running { ends: Duration },
    /// Stopped with this much left.
    Paused { left: Duration },
    /// It reached zero.
    Done,
}

impl Timer {
    pub fn new(length: Duration) -> Timer {
        Timer { length, state: TimerState::Ready }
    }

    /// What's left at monotonic time `now`.
    pub fn left(&self, now: Duration) -> Duration {
        match self.state {
            TimerState::Ready => self.length,
            TimerState::Running { ends } => ends.saturating_sub(now),
            TimerState::Paused { left } => left,
            TimerState::Done => Duration::ZERO,
        }
    }

    /// How far through it is, 0 to 1.
    pub fn fraction(&self, now: Duration) -> f64 {
        if self.length.is_zero() {
            return 0.0;
        }
        1.0 - self.left(now).as_secs_f64() / self.length.as_secs_f64()
    }

    pub fn running(&self) -> bool {
        matches!(self.state, TimerState::Running { .. })
    }

    /// Start or resume: when it will end.
    pub fn start(&mut self, now: Duration) -> Option<Duration> {
        let left = match self.state {
            TimerState::Ready | TimerState::Done => self.length,
            TimerState::Paused { left } => left,
            TimerState::Running { ends } => return Some(ends),
        };
        if left.is_zero() {
            return None;
        }
        let ends = now + left;
        self.state = TimerState::Running { ends };
        Some(ends)
    }

    pub fn pause(&mut self, now: Duration) {
        if let TimerState::Running { ends } = self.state {
            self.state = TimerState::Paused { left: ends.saturating_sub(now) };
        }
    }

    pub fn reset(&mut self) {
        self.state = TimerState::Ready;
    }

    /// Whether it ran out by `now` (it's then Done).
    pub fn check(&mut self, now: Duration) -> bool {
        if let TimerState::Running { ends } = self.state
            && now >= ends
        {
            self.state = TimerState::Done;
            return true;
        }
        false
    }
}

/// A stopwatch with laps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stopwatch {
    /// Time run before the current run.
    banked: Duration,
    /// When the current run started (monotonic), if running.
    since: Option<Duration>,
    /// Each lap's total at its end.
    pub laps: Vec<Duration>,
}

impl Stopwatch {
    pub fn elapsed(&self, now: Duration) -> Duration {
        self.banked + self.since.map_or(Duration::ZERO, |s| now.saturating_sub(s))
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    pub fn start(&mut self, now: Duration) {
        if self.since.is_none() {
            self.since = Some(now);
        }
    }

    pub fn stop(&mut self, now: Duration) {
        if let Some(s) = self.since.take() {
            self.banked += now.saturating_sub(s);
        }
    }

    pub fn lap(&mut self, now: Duration) {
        if self.running() {
            self.laps.push(self.elapsed(now));
        }
    }

    pub fn reset(&mut self) {
        *self = Stopwatch::default();
    }

    /// Whether it has run at all (Reset has something to do).
    pub fn started(&self) -> bool {
        self.running() || !self.banked.is_zero()
    }

    /// Each finished lap's own length.
    pub fn splits(&self) -> Vec<Duration> {
        let mut out = Vec::new();
        let mut before = Duration::ZERO;
        for l in &self.laps {
            out.push(l.saturating_sub(before));
            before = *l;
        }
        out
    }

    /// The fastest and slowest of the finished laps (with two or more).
    pub fn extremes(&self) -> Option<(usize, usize)> {
        let s = self.splits();
        if s.len() < 2 {
            return None;
        }
        let fastest = (0..s.len()).min_by_key(|i| s[*i])?;
        let slowest = (0..s.len()).max_by_key(|i| s[*i])?;
        (fastest != slowest).then_some((fastest, slowest))
    }
}

/// A stopwatch's time: `00:12.34`, `1:02:03.45` past an hour.
pub fn stopwatch_text(d: Duration) -> String {
    let cs = d.as_millis() / 10;
    let (h, m, s, c) = (cs / 360_000, (cs / 6000) % 60, (cs / 100) % 60, cs % 100);
    if h > 0 { format!("{h}:{m:02}:{s:02}.{c:02}") } else { format!("{m:02}:{s:02}.{c:02}") }
}

/// A timer's time, rounded up to the second (it shows 00:01 until it's
/// done): `04:59`, `1:00:00`.
pub fn timer_text(d: Duration) -> String {
    let s = d.as_millis().div_ceil(1000);
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// A length typed: `5` (minutes), `90s`, `1:30` (minutes and seconds),
/// `1:00:00`, `1h 30m`, `25 min`.
pub fn parse_length(s: &str) -> Option<Duration> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    if s.contains(':') {
        let parts: Vec<u64> = s.split(':').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
        let secs = match parts.as_slice() {
            [m, s] if *s < 60 => m * 60 + s,
            [h, m, s] if *m < 60 && *s < 60 => h * 3600 + m * 60 + s,
            _ => return None,
        };
        return (secs > 0).then(|| Duration::from_secs(secs));
    }
    let mut total = 0u64;
    let mut num = String::new();
    let mut any = false;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            num.push(c);
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let mut unit = String::new();
        while i < chars.len() && chars[i].is_alphabetic() {
            unit.push(chars[i]);
            i += 1;
        }
        let n: u64 = num.parse().ok()?;
        num.clear();
        total += match unit.as_str() {
            "h" | "hr" | "hrs" | "hour" | "hours" => n * 3600,
            "m" | "min" | "mins" | "minute" | "minutes" => n * 60,
            "s" | "sec" | "secs" | "second" | "seconds" => n,
            _ => return None,
        };
        any = true;
    }
    if !num.is_empty() {
        // A bare number is minutes.
        total += num.parse::<u64>().ok()? * if any { 1 } else { 60 };
    }
    (total > 0 && total < 100 * 3600).then(|| Duration::from_secs(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn timers_count_down_pause_and_finish() {
        let mut t = Timer::new(5 * S);
        assert_eq!(t.left(Duration::ZERO), 5 * S);
        assert_eq!(t.start(10 * S), Some(15 * S));
        assert_eq!(timer_text(t.left(10 * S + Duration::from_millis(1))), "00:05");
        t.pause(12 * S);
        assert_eq!(t.left(100 * S), 3 * S);
        t.start(100 * S);
        assert!(!t.check(102 * S));
        assert!(t.check(103 * S));
        assert_eq!(t.state, TimerState::Done);
        assert_eq!(t.fraction(103 * S), 1.0);
        assert_eq!(timer_text(Duration::from_millis(400)), "00:01");
        assert_eq!(timer_text(3600 * S), "1:00:00");
    }

    #[test]
    fn stopwatches_bank_time_and_take_laps() {
        let mut w = Stopwatch::default();
        w.start(S);
        w.lap(4 * S);
        w.stop(6 * S);
        assert_eq!(w.elapsed(100 * S), 5 * S);
        w.start(100 * S);
        w.lap(101 * S);
        w.lap(105 * S);
        assert_eq!(w.splits(), [3 * S, 3 * S, 4 * S]);
        assert_eq!(w.extremes(), Some((0, 2)));
        assert_eq!(stopwatch_text(Duration::from_millis(12_345)), "00:12.34");
        assert_eq!(stopwatch_text(Duration::from_millis(3_723_450)), "1:02:03.45");
        w.reset();
        assert!(!w.started() && w.laps.is_empty());
    }

    #[test]
    fn lengths_read_many_ways() {
        assert_eq!(parse_length("5"), Some(300 * S));
        assert_eq!(parse_length("1:30"), Some(90 * S));
        assert_eq!(parse_length("1:00:00"), Some(3600 * S));
        assert_eq!(parse_length("90s"), Some(90 * S));
        assert_eq!(parse_length("1h 30m"), Some(5400 * S));
        assert_eq!(parse_length("25 min"), Some(1500 * S));
        assert_eq!(parse_length("2m30"), Some(150 * S));
        assert_eq!(parse_length("0"), None);
        assert_eq!(parse_length("1:75"), None);
        assert_eq!(parse_length("soon"), None);
    }
}
