//! Alarms: what they are, where they're kept
//! (`$XDG_DATA_HOME/aizome/tokei/alarms.json`), and how they ring when
//! Tokei isn't running: each enabled alarm is a transient systemd user
//! timer (`systemd-run --user --on-calendar=… --unit=aizome-alarm-<id>`)
//! that runs `aizome-tokei --ring <id>`, waking the computer if the alarm
//! asks.
//!
//! Transient timers end with the user's systemd manager (at logout), so
//! Tokei schedules them again whenever it starts, and `aizome-tokei --sync`
//! does it at login. The [`Scheduler`] is a trait: tests use
//! [`Recorder`], which runs nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

/// An alarm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alarm {
    /// Eight hex digits, unique among the alarms (the timer's name).
    pub id: String,
    pub label: String,
    pub hour: u8,
    pub minute: u8,
    /// The days it repeats on, 1 (Monday) to 7 (Sunday); none: once, at
    /// the next time it comes round.
    pub days: Vec<u8>,
    pub enabled: bool,
    /// Wake the computer from sleep for it.
    pub wake: bool,
    /// A sound's name in the freedesktop sound theme, or `tone`.
    pub sound: String,
    pub snooze_minutes: u32,
}

impl Default for Alarm {
    fn default() -> Alarm {
        Alarm {
            id: String::new(),
            label: String::new(),
            hour: 7,
            minute: 0,
            days: Vec::new(),
            enabled: true,
            wake: false,
            sound: "alarm-clock-elapsed".into(),
            snooze_minutes: 9,
        }
    }
}

/// The days' two-letter names, Monday first.
pub const DAY_NAMES: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const DAY_SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl Alarm {
    /// Its name in systemd: `aizome-alarm-<id>`.
    pub fn unit(&self) -> String {
        unit_for(&self.id)
    }

    pub fn repeats(&self) -> bool {
        !self.days.is_empty()
    }

    fn sorted_days(&self) -> Vec<u8> {
        let mut d: Vec<u8> = self.days.iter().copied().filter(|d| (1..=7).contains(d)).collect();
        d.sort_unstable();
        d.dedup();
        d
    }

    /// How its repeating reads: "Every day", "Weekdays", "Weekends",
    /// "Mon, Wed, Fri", or "Once".
    pub fn repeat_words(&self) -> String {
        let d = self.sorted_days();
        match d.as_slice() {
            [] => "Once".into(),
            [1, 2, 3, 4, 5, 6, 7] => "Every day".into(),
            [1, 2, 3, 4, 5] => "Weekdays".into(),
            [6, 7] => "Weekends".into(),
            _ => d.iter().map(|i| DAY_SHORT[usize::from(*i - 1)]).collect::<Vec<_>>().join(", "),
        }
    }

    /// When it next rings after `now`, in `tz`.
    pub fn next(&self, now: Timestamp, tz: &TimeZone) -> Option<Timestamp> {
        let here = now.to_zoned(tz.clone());
        let days = self.sorted_days();
        for ahead in 0..8i64 {
            let date = here.date().checked_add(jiff::Span::new().days(ahead)).ok()?;
            let wd = date.weekday().to_monday_one_offset() as u8;
            if !days.is_empty() && !days.contains(&wd) {
                continue;
            }
            let at = DateTime::from_parts(date, jiff::civil::time(self.hour as i8, self.minute as i8, 0, 0));
            let Ok(z) = at.to_zoned(tz.clone()) else { continue };
            if z.timestamp() > now {
                return Some(z.timestamp());
            }
        }
        None
    }

    /// systemd's `OnCalendar=` for it: `Mon,Fri *-*-* 07:30:00` for a
    /// repeating alarm, the next date and time for one that rings once.
    pub fn calendar(&self, now: Timestamp, tz: &TimeZone) -> Option<String> {
        let time = format!("{:02}:{:02}:00", self.hour, self.minute);
        if self.repeats() {
            let d = self.sorted_days();
            let days = if d.len() == 7 {
                String::new()
            } else {
                d.iter().map(|i| DAY_SHORT[usize::from(*i - 1)]).collect::<Vec<_>>().join(",") + " "
            };
            Some(format!("{days}*-*-* {time}"))
        } else {
            let next = self.next(now, tz)?.to_zoned(tz.clone());
            Some(format!("{} {time}", next.date()))
        }
    }
}

pub fn unit_for(id: &str) -> String {
    format!("aizome-alarm-{id}")
}

/// A new alarm id not among `taken`.
pub fn new_id(taken: &[Alarm], seed: u64) -> String {
    let mut x = seed | 1;
    loop {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let id = format!("{:08x}", x as u32);
        if !taken.iter().any(|a| a.id == id) {
            return id;
        }
    }
}

/// The file the alarms are kept in.
pub fn alarms_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("aizome/tokei/alarms.json")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct File {
    alarms: Vec<Alarm>,
}

pub fn load(path: &Path) -> Vec<Alarm> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<File>(&s).ok())
        .map(|f| f.alarms)
        .unwrap_or_default()
}

pub fn save(path: &Path, alarms: &[Alarm]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(&File { alarms: alarms.to_vec() }).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Where alarms are scheduled.
pub trait Scheduler {
    /// Schedule `alarm` (replacing what was scheduled for it).
    fn schedule(&mut self, alarm: &Alarm, now: Timestamp, tz: &TimeZone) -> Result<(), String>;
    /// Unschedule the alarm with this id (and its snooze).
    fn cancel(&mut self, id: &str);
    /// Ring the alarm again `minutes` from now.
    fn snooze(&mut self, alarm: &Alarm, minutes: u32) -> Result<(), String>;
    /// The ids of the alarms scheduled now.
    fn scheduled(&mut self) -> Vec<String>;
}

/// Make what's scheduled match `alarms`: each enabled one scheduled
/// (unless it's a once-alarm whose time has gone), the rest not.
pub fn sync(s: &mut dyn Scheduler, alarms: &[Alarm], now: Timestamp, tz: &TimeZone) -> Vec<String> {
    let mut errors = Vec::new();
    let scheduled = s.scheduled();
    for id in &scheduled {
        if !alarms.iter().any(|a| &a.id == id && a.enabled) {
            s.cancel(id);
        }
    }
    for a in alarms.iter().filter(|a| a.enabled) {
        if let Err(e) = s.schedule(a, now, tz) {
            errors.push(format!("{}: {e}", a.unit()));
        }
    }
    errors
}

/// The real thing: `systemd-run --user` and `systemctl --user`.
pub struct Systemd {
    /// What the timer runs: `aizome-tokei`, found where it's installed.
    pub exe: PathBuf,
}

impl Default for Systemd {
    fn default() -> Systemd {
        let installed = PathBuf::from("/run/current-system/sw/bin/aizome-tokei");
        let exe = if installed.exists() {
            installed
        } else {
            std::env::current_exe().unwrap_or_else(|_| PathBuf::from("aizome-tokei"))
        };
        Systemd { exe }
    }
}

/// The arguments of `systemd-run` for an alarm (without the program).
pub fn run_args(alarm: &Alarm, calendar: &str, exe: &Path) -> Vec<String> {
    let mut args = vec![
        "--user".to_owned(),
        "--collect".to_owned(),
        format!("--unit={}", alarm.unit()),
        format!(
            "--description=Aizome alarm{}",
            if alarm.label.is_empty() { String::new() } else { format!(": {}", alarm.label) }
        ),
        format!("--on-calendar={calendar}"),
        // Ring on the minute, not within systemd's default minute.
        "--timer-property=AccuracySec=1s".to_owned(),
    ];
    if alarm.wake {
        args.push("--timer-property=WakeSystem=true".to_owned());
    }
    args.extend(["--".to_owned(), exe.display().to_string(), "--ring".to_owned(), alarm.id.clone()]);
    args
}

fn run(cmd: &str, args: &[String]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

fn stop(unit: &str) {
    let _ = run("systemctl", &["--user".into(), "stop".into(), format!("{unit}.timer"), format!("{unit}.service")]);
    let _ =
        run("systemctl", &["--user".into(), "reset-failed".into(), format!("{unit}.timer"), format!("{unit}.service")]);
}

impl Scheduler for Systemd {
    fn schedule(&mut self, alarm: &Alarm, now: Timestamp, tz: &TimeZone) -> Result<(), String> {
        stop(&alarm.unit());
        let Some(calendar) = alarm.calendar(now, tz) else { return Ok(()) };
        run("systemd-run", &run_args(alarm, &calendar, &self.exe)).map(|_| ())
    }

    fn cancel(&mut self, id: &str) {
        stop(&unit_for(id));
        stop(&format!("{}-snooze", unit_for(id)));
    }

    fn snooze(&mut self, alarm: &Alarm, minutes: u32) -> Result<(), String> {
        let unit = format!("{}-snooze", alarm.unit());
        stop(&unit);
        let args = vec![
            "--user".to_owned(),
            "--collect".to_owned(),
            format!("--unit={unit}"),
            "--description=Aizome alarm, snoozed".to_owned(),
            format!("--on-active={minutes}min"),
            "--timer-property=AccuracySec=1s".to_owned(),
            "--".to_owned(),
            self.exe.display().to_string(),
            "--ring".to_owned(),
            alarm.id.clone(),
        ];
        run("systemd-run", &args).map(|_| ())
    }

    fn scheduled(&mut self) -> Vec<String> {
        let Ok(out) = run(
            "systemctl",
            &[
                "--user".into(),
                "list-units".into(),
                "--plain".into(),
                "--no-legend".into(),
                "--type=timer".into(),
                "aizome-alarm-*".into(),
            ],
        ) else {
            return Vec::new();
        };
        out.lines()
            .filter_map(|l| l.split_whitespace().next())
            .filter_map(|u| u.strip_prefix("aizome-alarm-")?.strip_suffix(".timer"))
            .filter(|id| !id.ends_with("-snooze"))
            .map(str::to_owned)
            .collect()
    }
}

/// A scheduler that runs nothing and writes down what it was asked:
/// for tests, and for `AIZOME_TOKEI_DRY_RUN=1`.
#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    pub exe: PathBuf,
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder { log: Default::default(), exe: PathBuf::from("aizome-tokei") }
    }

    pub fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    /// The units scheduled, as the calls left them.
    pub fn units(&self) -> Vec<String> {
        let mut units: Vec<String> = Vec::new();
        for c in self.log.borrow().iter() {
            if let Some(rest) = c.strip_prefix("systemd-run ") {
                if let Some(u) = rest.split_whitespace().find_map(|a| a.strip_prefix("--unit=")) {
                    units.retain(|x| x != u);
                    units.push(u.to_owned());
                }
            } else if let Some(u) = c.strip_prefix("stop ") {
                units.retain(|x| x != u);
            }
        }
        units
    }
}

impl Scheduler for Recorder {
    fn schedule(&mut self, alarm: &Alarm, now: Timestamp, tz: &TimeZone) -> Result<(), String> {
        self.log.borrow_mut().push(format!("stop {}", alarm.unit()));
        if let Some(calendar) = alarm.calendar(now, tz) {
            let args = run_args(alarm, &calendar, &self.exe);
            log::info!("would run: systemd-run {}", args.join(" "));
            self.log.borrow_mut().push(format!("systemd-run {}", args.join(" ")));
        }
        Ok(())
    }

    fn cancel(&mut self, id: &str) {
        self.log.borrow_mut().push(format!("stop {}", unit_for(id)));
    }

    fn snooze(&mut self, alarm: &Alarm, minutes: u32) -> Result<(), String> {
        self.log.borrow_mut().push(format!("systemd-run --unit={}-snooze --on-active={minutes}min", alarm.unit()));
        Ok(())
    }

    fn scheduled(&mut self) -> Vec<String> {
        self.units()
            .iter()
            .filter_map(|u| u.strip_prefix("aizome-alarm-"))
            .filter(|u| !u.ends_with("-snooze"))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn alarm(hour: u8, minute: u8, days: &[u8]) -> Alarm {
        Alarm { id: "0badcafe".into(), hour, minute, days: days.to_vec(), ..Alarm::default() }
    }

    #[test]
    fn repeats_read_as_words() {
        assert_eq!(alarm(7, 0, &[]).repeat_words(), "Once");
        assert_eq!(alarm(7, 0, &[5, 4, 3, 2, 1]).repeat_words(), "Weekdays");
        assert_eq!(alarm(7, 0, &[7, 6]).repeat_words(), "Weekends");
        assert_eq!(alarm(7, 0, &[1, 2, 3, 4, 5, 6, 7]).repeat_words(), "Every day");
        assert_eq!(alarm(7, 0, &[1, 3, 5]).repeat_words(), "Mon, Wed, Fri");
    }

    #[test]
    fn calendars_and_next_times() {
        let tz = TimeZone::get("Europe/Berlin").unwrap();
        // Saturday 17 October 2026, 22:30 in Berlin.
        let now = ts("2026-10-17T20:30:00Z");
        let once = alarm(7, 30, &[]);
        assert_eq!(once.calendar(now, &tz).as_deref(), Some("2026-10-18 07:30:00"));
        assert_eq!(once.next(now, &tz), Some(ts("2026-10-18T05:30:00Z")));
        // Later today: today.
        assert_eq!(alarm(23, 0, &[]).calendar(now, &tz).as_deref(), Some("2026-10-17 23:00:00"));
        let weekdays = alarm(6, 45, &[1, 2, 3, 4, 5]);
        assert_eq!(weekdays.calendar(now, &tz).as_deref(), Some("Mon,Tue,Wed,Thu,Fri *-*-* 06:45:00"));
        // Next Monday, 19 October.
        assert_eq!(weekdays.next(now, &tz), Some(ts("2026-10-19T04:45:00Z")));
        assert_eq!(alarm(8, 0, &[1, 2, 3, 4, 5, 6, 7]).calendar(now, &tz).as_deref(), Some("*-*-* 08:00:00"));
        // Across the change back to winter time on 25 October.
        let sunday = alarm(9, 0, &[7]);
        let after = ts("2026-10-19T00:00:00Z");
        assert_eq!(sunday.next(after, &tz), Some(ts("2026-10-25T08:00:00Z")));
    }

    #[test]
    fn systemd_run_is_asked_for_a_timer_that_rings_tokei() {
        let mut a = alarm(7, 30, &[1, 2, 3, 4, 5]);
        a.label = "Work".into();
        a.wake = true;
        let args =
            run_args(&a, "Mon,Tue,Wed,Thu,Fri *-*-* 07:30:00", Path::new("/run/current-system/sw/bin/aizome-tokei"));
        assert_eq!(
            args,
            [
                "--user",
                "--collect",
                "--unit=aizome-alarm-0badcafe",
                "--description=Aizome alarm: Work",
                "--on-calendar=Mon,Tue,Wed,Thu,Fri *-*-* 07:30:00",
                "--timer-property=AccuracySec=1s",
                "--timer-property=WakeSystem=true",
                "--",
                "/run/current-system/sw/bin/aizome-tokei",
                "--ring",
                "0badcafe",
            ]
        );
        a.wake = false;
        assert!(!run_args(&a, "x", Path::new("t")).iter().any(|s| s.contains("WakeSystem")));
    }

    #[test]
    fn sync_schedules_the_enabled_and_cancels_the_rest() {
        let tz = TimeZone::get("Europe/Berlin").unwrap();
        let now = ts("2026-10-17T20:30:00Z");
        let mut r = Recorder::new();
        let mut a = alarm(7, 30, &[1]);
        let mut b = alarm(8, 0, &[]);
        b.id = "00000002".into();
        sync(&mut r, &[a.clone(), b.clone()], now, &tz);
        assert_eq!(r.units(), ["aizome-alarm-0badcafe", "aizome-alarm-00000002"]);
        a.enabled = false;
        sync(&mut r, &[a.clone(), b.clone()], now, &tz);
        assert_eq!(r.units(), ["aizome-alarm-00000002"]);
        sync(&mut r, &[], now, &tz);
        assert!(r.units().is_empty());
    }

    #[test]
    fn alarms_round_trip_through_their_file() {
        let dir = std::env::temp_dir().join(format!("tokei-alarms-{}", std::process::id()));
        let path = dir.join("alarms.json");
        let mut a = alarm(6, 15, &[6, 7]);
        a.label = "Run".into();
        save(&path, std::slice::from_ref(&a)).unwrap();
        assert_eq!(load(&path), vec![a]);
        assert!(load(&dir.join("none.json")).is_empty());
        let _ = std::fs::remove_dir_all(dir);
        let ids = [alarm(0, 0, &[])];
        assert_ne!(new_id(&ids, 42), "0badcafe");
        assert_eq!(new_id(&ids, 42).len(), 8);
    }
}
