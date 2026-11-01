//! The grown-ups' rules: the PIN, what kids may open, and for how long.
//!
//! The PIN is kept as a salted PBKDF2-HMAC-SHA-256 hash. Wrong PINs cost
//! time: three are free, then each locks the pad for longer (half a
//! minute, doubling, up to a quarter of an hour), and the count survives
//! restarts. A four-digit PIN can't resist someone who can read the
//! settings file and run code; it keeps out kids with a controller, which
//! is what it's for. On a television set up for kids there is no
//! terminal to run code from.
//!
//! Time: each day of the week may have a limit in minutes, and bedtime is
//! a stretch of the clock (it may cross midnight) when nothing plays. Only
//! time spent watching or playing counts. A grown-up can give more time
//! (it also lifts bedtime for that long), or unlock grown-up mode, where
//! nothing is limited until it locks again.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::clock::Local;

/// Rounds of PBKDF2 for a new PIN: about a tenth of a second.
pub const ROUNDS: u32 = 150_000;
/// Wrong PINs before the pad locks.
pub const FREE_TRIES: u32 = 3;
/// The longest the pad locks for.
pub const MAX_LOCK_SECS: u64 = 15 * 60;

/// A PIN as it's kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinHash {
    /// Hex.
    pub salt: String,
    /// Hex.
    pub hash: String,
    pub rounds: u32,
}

impl PinHash {
    /// Hash `pin` with a new salt.
    pub fn new(pin: &str, rounds: u32) -> PinHash {
        let salt = random_bytes::<16>();
        let hash = pbkdf2_sha256(pin.as_bytes(), &salt, rounds);
        PinHash { salt: hex(&salt), hash: hex(&hash), rounds }
    }

    pub fn verify(&self, pin: &str) -> bool {
        let Some(salt) = unhex(&self.salt) else { return false };
        let Some(want) = unhex(&self.hash) else { return false };
        let got = pbkdf2_sha256(pin.as_bytes(), &salt, self.rounds.max(1));
        // Compared in full whatever differs first.
        want.len() == got.len() && want.iter().zip(got.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// Whether `pin` is one a grown-up may choose: four to eight digits.
pub fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    // SAFETY: getrandom fills the buffer it's given, up to its length.
    let n = unsafe { libc::getrandom(out.as_mut_ptr().cast(), N, 0) };
    if n != N as isize {
        // No entropy call (an old kernel): the time and address are a
        // poor salt, but a salt still.
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let mix = Sha256::digest(format!("{t}{:p}", &out).as_bytes());
        out.copy_from_slice(&mix[..N]);
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// PBKDF2 (RFC 8018) with HMAC-SHA-256, one 32-byte block.
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    // HMAC's keyed inner and outer states, made once and cloned each round.
    let mut key = [0u8; 64];
    if password.len() > 64 {
        key[..32].copy_from_slice(&Sha256::digest(password));
    } else {
        key[..password.len()].copy_from_slice(password);
    }
    let pad = |byte: u8| {
        let mut h = Sha256::new();
        h.update(key.map(|k| k ^ byte));
        h
    };
    let (inner, outer) = (pad(0x36), pad(0x5c));
    let mac = |parts: &[&[u8]]| -> [u8; 32] {
        let mut i = inner.clone();
        for p in parts {
            i.update(p);
        }
        let mut o = outer.clone();
        o.update(i.finalize());
        o.finalize().into()
    };
    let mut u = mac(&[salt, &1u32.to_be_bytes()]);
    let mut t = u;
    for _ in 1..rounds {
        u = mac(&[&u]);
        for (t, u) in t.iter_mut().zip(u.iter()) {
            *t ^= u;
        }
    }
    t
}

/// Wrong PINs, and how long the pad stays locked.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockout {
    pub failures: u32,
    /// Unix seconds the pad is locked until.
    pub until: u64,
}

impl Lockout {
    /// Seconds the pad stays locked from `now`, if it's locked.
    pub fn wait(&self, now: u64) -> Option<u64> {
        (self.until > now).then(|| self.until - now)
    }

    pub fn failed(&mut self, now: u64) {
        self.failures += 1;
        if self.failures >= FREE_TRIES {
            let doublings = (self.failures - FREE_TRIES).min(16);
            self.until = now + (30u64 << doublings).min(MAX_LOCK_SECS);
        }
    }

    pub fn succeeded(&mut self) {
        *self = Lockout::default();
    }
}

/// When the television sleeps: minutes of the day, crossing midnight when
/// `from` is later than `until`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Bedtime {
    pub from: u16,
    pub until: u16,
}

impl Bedtime {
    pub fn contains(&self, minute: u16) -> bool {
        if self.from <= self.until {
            (self.from..self.until).contains(&minute)
        } else {
            minute >= self.from || minute < self.until
        }
    }

    /// Minutes from `minute` until bedtime starts (0 within it).
    pub fn minutes_until(&self, minute: u16) -> u16 {
        if self.contains(minute) { 0 } else { (self.from + 1440 - minute) % 1440 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Minutes a day, Monday first; none for no limit that day.
    pub daily: [Option<u32>; 7],
    pub bedtime: Option<Bedtime>,
}

/// What happens to games found that a grown-up hasn't looked at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NewGames {
    /// They wait, hidden, until a grown-up allows each.
    #[default]
    Ask,
    /// They show at once.
    Allow,
}

/// Which button says yes in Chanoma's menus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confirm {
    /// The controller's own habit: the right-hand button on Nintendo's,
    /// the bottom one on others.
    #[default]
    Auto,
    Bottom,
    Right,
}

/// A site a grown-up added.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CustomService {
    pub name: String,
    pub url: String,
}

/// Everything the grown-ups set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub pin: Option<PinHash>,
    /// How many digits the PIN has, so the pad checks it once that many
    /// are typed (a four-digit PIN keeps no secret in its length).
    pub pin_length: Option<u8>,
    /// The services kids see, by id, in order.
    pub services: Vec<String>,
    pub custom_services: Vec<CustomService>,
    pub game_folders: Vec<PathBuf>,
    pub video_folders: Vec<PathBuf>,
    /// Games kids may play, by key (`games::Game::key`).
    pub games_allowed: BTreeSet<String>,
    /// Games kept from kids even when new games are allowed.
    pub games_hidden: BTreeSet<String>,
    pub new_games: NewGames,
    pub limits: Limits,
    /// Minutes untouched before grown-up mode locks again.
    pub relock_minutes: u32,
    pub confirm: Confirm,
    /// The browser services open in (a command); none to find Chrome.
    pub browser: Option<String>,
    /// A grown-up has been through the first setup.
    pub setup_done: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            pin: None,
            pin_length: None,
            services: vec!["pbs-kids".into(), "youtube-kids".into()],
            custom_services: Vec::new(),
            game_folders: Vec::new(),
            video_folders: Vec::new(),
            games_allowed: BTreeSet::new(),
            games_hidden: BTreeSet::new(),
            new_games: NewGames::Ask,
            limits: Limits::default(),
            relock_minutes: 15,
            confirm: Confirm::Auto,
            browser: None,
            setup_done: false,
        }
    }
}

impl Settings {
    /// Whether kids may play the game keyed `key`.
    pub fn game_allowed(&self, key: &str) -> bool {
        !self.games_hidden.contains(key) && (self.games_allowed.contains(key) || self.new_games == NewGames::Allow)
    }

    /// Whether the game keyed `key` waits for a grown-up's say.
    pub fn game_waiting(&self, key: &str) -> bool {
        self.new_games == NewGames::Ask && !self.games_allowed.contains(key) && !self.games_hidden.contains(key)
    }
}

/// Today's time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    /// The day it counts for, `YYYY-MM-DD`.
    pub date: String,
    /// Seconds spent watching and playing.
    pub used: u64,
    /// Seconds a grown-up added to today's limit.
    pub granted: u64,
    /// Unix seconds bedtime is lifted until.
    pub bedtime_lifted_until: u64,
}

impl Usage {
    /// Start a new day if `local` is one: whether it was.
    pub fn roll(&mut self, local: &Local) -> bool {
        let date = local.date();
        if self.date == date {
            return false;
        }
        *self = Usage { date, ..Usage::default() };
        true
    }
}

/// Whether kids may watch and play now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Yes, for `left` seconds more (none: no end in sight today).
    Open { left: Option<u64> },
    /// Today's minutes are used up.
    TimeUp,
    /// It's bedtime, until this minute of the day.
    Bedtime { until: u16 },
}

impl Gate {
    pub fn open(&self) -> bool {
        matches!(self, Gate::Open { .. })
    }
}

/// The gate for kids at `local` (Unix seconds `now`), with `usage` rolled
/// to today.
pub fn gate(limits: &Limits, usage: &Usage, local: &Local, now: u64) -> Gate {
    let lifted = usage.bedtime_lifted_until > now;
    let mut left: Option<u64> = None;
    if let Some(b) = limits.bedtime {
        if lifted {
            left = Some(usage.bedtime_lifted_until - now);
        } else if b.contains(local.minute) {
            return Gate::Bedtime { until: b.until };
        } else {
            // Bedtime coming: what's left of this minute and those before it.
            let secs = u64::from(b.minutes_until(local.minute)) * 60 - u64::from(local.second.min(59));
            left = Some(secs);
        }
    }
    if let Some(limit) = limits.daily[usize::from(local.weekday % 7)] {
        let allowance = u64::from(limit) * 60 + usage.granted;
        let daily = allowance.saturating_sub(usage.used);
        if daily == 0 {
            return Gate::TimeUp;
        }
        left = Some(left.map_or(daily, |l| l.min(daily)));
    }
    Gate::Open { left }
}

/// A grown-up gives `minutes` more: added to today's limit, and bedtime
/// lifted that long from `now` if it's bedtime or comes sooner.
pub fn grant(limits: &Limits, usage: &mut Usage, local: &Local, now: u64, minutes: u32) {
    let secs = u64::from(minutes) * 60;
    if let Some(limit) = limits.daily[usize::from(local.weekday % 7)] {
        // Topped up from what's been used, so a grant after time ran out
        // is all play.
        let allowance = u64::from(limit) * 60 + usage.granted;
        usage.granted += usage.used.saturating_sub(allowance) + secs;
    }
    if let Some(b) = limits.bedtime {
        let until_bedtime = u64::from(b.minutes_until(local.minute)) * 60;
        if b.contains(local.minute) || until_bedtime < secs || usage.bedtime_lifted_until > now {
            usage.bedtime_lifted_until = usage.bedtime_lifted_until.max(now) + secs;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Clock, ManualClock};

    #[test]
    fn pbkdf2_matches_rfc_7914s_vector() {
        // RFC 7914 §11: PBKDF2-HMAC-SHA256, P="passwd", S="salt", c=1.
        let out = pbkdf2_sha256(b"passwd", b"salt", 1);
        assert_eq!(hex(&out), "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc");
        // P="password", S="salt", c=4096 (a widely used SHA-256 vector).
        let out = pbkdf2_sha256(b"password", b"salt", 4096);
        assert_eq!(hex(&out), "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a");
    }

    #[test]
    fn a_pin_verifies_and_its_salt_differs_each_time() {
        let a = PinHash::new("2468", 50);
        let b = PinHash::new("2468", 50);
        assert!(a.verify("2468"));
        assert!(!a.verify("2469"));
        assert!(!a.verify(""));
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.hash, b.hash);
        assert!(valid_pin("0000") && valid_pin("12345678"));
        assert!(!valid_pin("123") && !valid_pin("12a4") && !valid_pin("123456789"));
    }

    #[test]
    fn wrong_pins_lock_the_pad_for_longer_each_time() {
        let mut l = Lockout::default();
        l.failed(1000);
        l.failed(1000);
        assert_eq!(l.wait(1000), None, "two tries are free");
        l.failed(1000);
        assert_eq!(l.wait(1000), Some(30));
        l.failed(1030);
        assert_eq!(l.wait(1030), Some(60));
        for _ in 0..10 {
            l.failed(5000);
        }
        assert_eq!(l.wait(5000), Some(MAX_LOCK_SECS));
        l.succeeded();
        assert_eq!(l, Lockout::default());
    }

    #[test]
    fn bedtime_crosses_midnight() {
        let b = Bedtime { from: 20 * 60, until: 7 * 60 };
        assert!(b.contains(20 * 60) && b.contains(23 * 60) && b.contains(0) && b.contains(6 * 60 + 59));
        assert!(!b.contains(7 * 60) && !b.contains(12 * 60));
        assert_eq!(b.minutes_until(19 * 60 + 30), 30);
        assert_eq!(b.minutes_until(21 * 60), 0);
        let nap = Bedtime { from: 13 * 60, until: 14 * 60 };
        assert!(nap.contains(13 * 60 + 30) && !nap.contains(14 * 60));
    }

    fn saturday_limits() -> Limits {
        let mut l = Limits::default();
        l.daily[5] = Some(60);
        l.bedtime = Some(Bedtime { from: 20 * 60, until: 7 * 60 });
        l
    }

    #[test]
    fn the_gate_counts_down_to_the_limit_or_bedtime() {
        let limits = saturday_limits();
        let clock = ManualClock::at(2026, 10, 31, 15, 0);
        let mut usage = Usage::default();
        assert!(usage.roll(&clock.local()));
        let now = 0;
        assert_eq!(gate(&limits, &usage, &clock.local(), now), Gate::Open { left: Some(3600) });
        usage.used = 3000;
        assert_eq!(gate(&limits, &usage, &clock.local(), now), Gate::Open { left: Some(600) });
        usage.used = 3600;
        assert_eq!(gate(&limits, &usage, &clock.local(), now), Gate::TimeUp);
        // Late in the day bedtime comes first.
        usage.used = 0;
        let evening = ManualClock::at(2026, 10, 31, 19, 50);
        assert_eq!(gate(&limits, &usage, &evening.local(), now), Gate::Open { left: Some(600) });
        let night = ManualClock::at(2026, 10, 31, 21, 0);
        assert_eq!(gate(&limits, &usage, &night.local(), now), Gate::Bedtime { until: 7 * 60 });
        // Sunday has no limit; bedtime still holds.
        let sunday = ManualClock::at(2026, 11, 1, 10, 0);
        let mut usage = Usage { used: 99_999, ..Usage::default() };
        usage.roll(&sunday.local());
        assert_eq!(gate(&limits, &usage, &sunday.local(), now), Gate::Open { left: Some(10 * 3600) });
    }

    #[test]
    fn a_grant_after_time_runs_out_is_all_play_and_lifts_bedtime() {
        let limits = saturday_limits();
        let clock = ManualClock::at(2026, 10, 31, 15, 0);
        let mut usage = Usage::default();
        usage.roll(&clock.local());
        // Used 70 minutes (a game ran past the end while saving).
        usage.used = 70 * 60;
        assert_eq!(gate(&limits, &usage, &clock.local(), 1000), Gate::TimeUp);
        grant(&limits, &mut usage, &clock.local(), 1000, 15);
        assert_eq!(gate(&limits, &usage, &clock.local(), 1000), Gate::Open { left: Some(15 * 60) });
        // At bedtime a grant lifts it for that long.
        let night = ManualClock::at(2026, 10, 31, 21, 0);
        let mut usage = Usage::default();
        usage.roll(&night.local());
        grant(&limits, &mut usage, &night.local(), 50_000, 30);
        assert_eq!(gate(&limits, &usage, &night.local(), 50_000), Gate::Open { left: Some(30 * 60) });
        assert_eq!(gate(&limits, &usage, &night.local(), 51_800), Gate::Bedtime { until: 7 * 60 });
    }

    #[test]
    fn a_new_day_starts_afresh() {
        let mut usage = Usage { date: "2026-10-30".into(), used: 500, granted: 60, bedtime_lifted_until: 9 };
        let clock = ManualClock::at(2026, 10, 31, 8, 0);
        assert!(usage.roll(&clock.local()));
        assert_eq!(usage, Usage { date: "2026-10-31".into(), ..Usage::default() });
        assert!(!usage.roll(&clock.local()));
    }

    #[test]
    fn new_games_wait_unless_allowed() {
        let mut s = Settings::default();
        assert!(!s.game_allowed("snes/Kirby.sfc") && s.game_waiting("snes/Kirby.sfc"));
        s.games_allowed.insert("snes/Kirby.sfc".into());
        assert!(s.game_allowed("snes/Kirby.sfc") && !s.game_waiting("snes/Kirby.sfc"));
        s.new_games = NewGames::Allow;
        assert!(s.game_allowed("gb/Tetris.gb"));
        s.games_hidden.insert("gb/Tetris.gb".into());
        assert!(!s.game_allowed("gb/Tetris.gb") && !s.game_waiting("gb/Tetris.gb"));
    }

    #[test]
    fn old_settings_files_fill_in_what_they_lack() {
        let s: Settings = serde_json::from_str(r#"{"services":["netflix"],"setup_done":true}"#).unwrap();
        assert_eq!(s.services, vec!["netflix".to_owned()]);
        assert!(s.setup_done && s.pin.is_none() && s.relock_minutes == 15);
    }
}
