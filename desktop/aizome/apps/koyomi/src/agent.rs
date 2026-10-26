//! `aizome-koyomi --agent`: a small user service that watches the
//! calendars and posts each alert as a desktop notification
//! (`org.freedesktop.Notifications`, which Aizome's shell serves) at its
//! time, with Snooze and Open.
//!
//! It remembers what it has shown (`$XDG_STATE_HOME/aizome/koyomi/agent.json`)
//! so a restart doesn't repeat alerts, and catches up on those missed
//! while the computer slept, for events not yet over.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use crate::format;
use crate::occur::{self, Index, Occurrence};
use crate::settings::{self, Settings, write_atomic};
use crate::store::Store;

/// A notification to post.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// What identifies the alert: calendar, UID, instance, alarm, time.
    pub key: String,
    pub summary: String,
    pub body: String,
    /// The day its event is on, for Open.
    pub date: String,
}

/// What the person did with a notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Snooze(u32),
    Open(u32),
    Closed(u32),
}

/// Posts notifications: D-Bus in the service, a list in tests.
pub trait Notifier {
    /// Post `note` (over notification `replaces`, if not 0): its id.
    fn notify(&mut self, note: &Note, replaces: u32) -> Option<u32>;
}

/// What the agent keeps between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentState {
    /// When it last looked: alerts after this and up to now are due.
    pub last_check: Option<i64>,
    /// Alerts shown, by key, with when they were due (seconds).
    pub fired: BTreeMap<String, i64>,
    /// Alerts put off, with when they come back.
    pub snoozed: Vec<(i64, Note)>,
}

impl AgentState {
    pub fn path() -> PathBuf {
        settings::state_dir().join("agent.json")
    }

    pub fn load() -> AgentState {
        std::fs::read_to_string(AgentState::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = AgentState::path();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = write_atomic(&path, json.as_bytes());
        }
    }
}

/// How far back missed alerts are still shown (the computer slept).
const CATCH_UP: SignedDuration = SignedDuration::from_hours(6);
/// How often the calendars' files are looked at.
pub const POLL: SignedDuration = SignedDuration::from_secs(30);

/// The agent's mind: the calendars, what it has shown, what's on screen.
pub struct Agent {
    pub store: Store,
    pub tz: TimeZone,
    pub state: AgentState,
    pub snooze: SignedDuration,
    index: Index,
    /// Notifications on screen: their id and what they're about.
    shown: HashMap<u32, Note>,
    dirty: bool,
}

impl Agent {
    pub fn new(store: Store, tz: TimeZone, state: AgentState, settings: &Settings) -> Agent {
        let index = Index::new(&store.calendars, &tz);
        Agent {
            store,
            tz,
            state,
            snooze: SignedDuration::from_mins(settings.snooze_minutes.max(1)),
            index,
            shown: HashMap::new(),
            dirty: false,
        }
    }

    /// Read the calendars again if their files changed.
    pub fn refresh(&mut self) {
        if self.store.changed_on_disk() {
            self.store.reload();
            self.index = Index::new(&self.store.calendars, &self.tz);
        }
    }

    fn note(&self, at: Timestamp, o: &Occurrence, alarm: usize, h24: bool) -> Note {
        let cal = &self.store.calendars[o.cal];
        let e = &cal.events[o.event];
        let key = format!(
            "{}|{}|{}|{}|{}",
            cal.id,
            e.uid,
            o.rid.as_ref().or(e.recurrence_id.as_ref()).map(|r| r.civil().to_string()).unwrap_or_default(),
            alarm,
            at.as_second()
        );
        let summary = if e.summary.trim().is_empty() { "New Event".to_owned() } else { e.summary.clone() };
        let today = at.to_zoned(self.tz.clone()).date();
        let mut parts = Vec::new();
        let lead = o.instant.duration_since(at).as_mins();
        if o.all_day {
            parts.push(if o.start.date() == today {
                "Today, all day".to_owned()
            } else if today.tomorrow().ok() == Some(o.start.date()) {
                "Tomorrow, all day".to_owned()
            } else {
                format!("{}, all day", format::short_date(o.start.date()))
            });
        } else {
            let when = if lead > 0 && lead < 24 * 60 {
                format!("In {}", format::minutes(lead))
            } else if lead == 0 {
                "Now".into()
            } else {
                String::new()
            };
            let range = if o.start.date() == today {
                format::time_range(o.start.time(), o.end.time(), h24)
            } else {
                format::span(o.start, o.end, false, h24)
            };
            parts.push(if when.is_empty() { range } else { format!("{when} · {range}") });
        }
        if let Some(l) = e.location.lines().next().filter(|l| !l.trim().is_empty()) {
            parts.push(l.trim().to_owned());
        }
        Note { key, summary, body: parts.join(" · "), date: o.start.date().to_string() }
    }

    /// The alerts due by `now` that haven't been shown, oldest first;
    /// they're marked shown.
    pub fn due(&mut self, now: Timestamp, h24: bool) -> Vec<Note> {
        let floor = now.checked_sub(CATCH_UP).unwrap_or(now);
        let from = self.state.last_check.and_then(|s| Timestamp::from_second(s).ok()).unwrap_or(now).max(floor);
        // Alerts exactly at `from` were seen by the last look, but a
        // first look takes its own instant.
        let to = now.checked_add(SignedDuration::from_secs(1)).unwrap_or(now);
        let mut out = Vec::new();
        for (at, o, ai) in occur::alarms(&self.store.calendars, &self.index, &self.tz, from, to) {
            // A missed alert for an event already over isn't news.
            if at < now.checked_sub(SignedDuration::from_mins(2)).unwrap_or(now) && o.end_instant <= now {
                continue;
            }
            let n = self.note(at, &o, ai, h24);
            if self.state.fired.contains_key(&n.key) {
                continue;
            }
            self.state.fired.insert(n.key.clone(), at.as_second());
            out.push(n);
        }
        let (ready, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.state.snoozed).into_iter().partition(|(at, _)| *at <= now.as_second());
        self.state.snoozed = later;
        out.extend(ready.into_iter().map(|(_, n)| n));
        self.state.last_check = Some(now.as_second());
        // Forget what's long past.
        let old = now.as_second() - 3 * 86_400;
        self.state.fired.retain(|_, at| *at > old);
        self.dirty = true;
        out
    }

    /// When to look next: the next alert or snooze, or the next poll.
    pub fn next_wake(&self, now: Timestamp) -> Timestamp {
        let poll = now.checked_add(POLL).unwrap_or(now);
        let mut next = poll;
        for (at, _) in &self.state.snoozed {
            if let Ok(t) = Timestamp::from_second(*at) {
                next = next.min(t);
            }
        }
        if let Some((at, _, _)) = occur::alarms(
            &self.store.calendars,
            &self.index,
            &self.tz,
            now.checked_add(SignedDuration::from_secs(1)).unwrap_or(now),
            poll,
        )
        .first()
        {
            next = next.min(*at);
        }
        next.max(now)
    }

    /// Post `notes`.
    pub fn post(&mut self, notes: Vec<Note>, notifier: &mut dyn Notifier) {
        for n in notes {
            // A snoozed alert comes back over its old notification, if
            // that's still up.
            let replaces = self.shown.iter().find(|(_, s)| s.key == n.key).map(|(id, _)| *id).unwrap_or(0);
            if let Some(id) = notifier.notify(&n, replaces) {
                self.shown.retain(|_, s| s.key != n.key);
                self.shown.insert(id, n);
            }
        }
    }

    /// What the person did. Open gives the day to show.
    pub fn handle(&mut self, reply: Reply, now: Timestamp) -> Option<String> {
        match reply {
            Reply::Snooze(id) => {
                if let Some(n) = self.shown.remove(&id) {
                    let at = now.checked_add(self.snooze).unwrap_or(now);
                    self.state.snoozed.push((at.as_second(), n));
                    self.dirty = true;
                }
                None
            }
            Reply::Open(id) => self.shown.remove(&id).map(|n| n.date),
            Reply::Closed(id) => {
                self.shown.remove(&id);
                None
            }
        }
    }

    pub fn save_if_changed(&mut self) {
        if std::mem::take(&mut self.dirty) {
            self.state.save();
        }
    }
}

/// Notifications over D-Bus, and the thread hearing what's done with them.
pub struct Dbus {
    conn: zbus::blocking::Connection,
}

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

impl Dbus {
    pub fn connect() -> zbus::Result<Dbus> {
        Ok(Dbus { conn: zbus::blocking::Connection::session()? })
    }

    /// The bus at `address` (tests serve a notification daemon of their own).
    pub fn at(address: &str) -> zbus::Result<Dbus> {
        Ok(Dbus { conn: zbus::blocking::connection::Builder::address(address)?.build()? })
    }

    /// Hear actions and closings on threads of their own, as replies.
    pub fn listen(&self, tx: Sender<Reply>) -> zbus::Result<()> {
        for (signal, closed) in [("ActionInvoked", false), ("NotificationClosed", true)] {
            let proxy = zbus::blocking::Proxy::new(&self.conn, DEST, PATH, DEST)?;
            let it = proxy.receive_signal(signal)?;
            let tx = tx.clone();
            std::thread::Builder::new()
                .name(format!("koyomi-{signal}"))
                .spawn(move || {
                    for m in it {
                        let reply = if closed {
                            m.body().deserialize::<(u32, u32)>().ok().map(|(id, _)| Reply::Closed(id))
                        } else {
                            m.body().deserialize::<(u32, String)>().ok().map(|(id, key)| match key.as_str() {
                                "snooze" => Reply::Snooze(id),
                                _ => Reply::Open(id),
                            })
                        };
                        if let Some(r) = reply
                            && tx.send(r).is_err()
                        {
                            return;
                        }
                    }
                })
                .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        }
        Ok(())
    }
}

impl Notifier for Dbus {
    fn notify(&mut self, n: &Note, replaces: u32) -> Option<u32> {
        use zbus::zvariant::Value;
        let actions = vec!["default", "Open", "snooze", "Snooze", "open", "Open"];
        let mut hints: HashMap<&str, Value> = HashMap::new();
        hints.insert("desktop-entry", Value::from("org.aizome.Koyomi"));
        hints.insert("urgency", Value::U8(1));
        hints.insert("category", Value::from("x-aizome.calendar.alert"));
        let body = (
            "Koyomi",
            replaces,
            "org.aizome.Koyomi",
            n.summary.as_str(),
            n.body.as_str(),
            actions,
            hints,
            0i32, // stays until dismissed
        );
        match self.conn.call_method(Some(DEST), PATH, Some(DEST), "Notify", &body) {
            Ok(reply) => reply.body().deserialize::<u32>().ok(),
            Err(e) => {
                log::warn!("Notify: {e}");
                None
            }
        }
    }
}

/// Open Koyomi at `date` (the running one, if there is one).
fn open_app(date: &str) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("aizome-koyomi"));
    if let Err(e) = std::process::Command::new(exe).arg("--show").arg(date).spawn() {
        log::warn!("starting Koyomi: {e}");
    }
}

/// Run the agent until killed.
pub fn run() -> i32 {
    let settings = Settings::load();
    let h24 = settings.uses_24h(settings::desktop_24h());
    let tz = crate::tz::system().zone.unwrap_or(TimeZone::UTC);
    let store = Store::open(settings::data_dir());
    let mut agent = Agent::new(store, tz, AgentState::load(), &settings);
    let (tx, rx): (Sender<Reply>, Receiver<Reply>) = channel();
    let mut dbus = loop {
        match Dbus::connect() {
            Ok(d) => break d,
            Err(e) => {
                // The session bus comes up with the session: wait for it.
                log::warn!("session bus: {e}; trying again");
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    };
    if let Err(e) = dbus.listen(tx) {
        log::warn!("listening for notification actions: {e}");
    }
    log::info!("watching {} calendars in {}", agent.store.calendars.len(), agent.store.dir.display());
    loop {
        agent.refresh();
        let now = Timestamp::now();
        let notes = agent.due(now, h24);
        agent.post(notes, &mut dbus);
        agent.save_if_changed();
        let wake = agent.next_wake(now);
        let wait = Duration::from_millis(wake.duration_since(Timestamp::now()).as_millis().clamp(50, 60_000) as u64);
        match rx.recv_timeout(wait) {
            Ok(reply) => {
                if let Some(date) = agent.handle(reply, Timestamp::now()) {
                    open_app(&date);
                }
                // More replies waiting are handled before looking again.
                while let Ok(r) = rx.try_recv() {
                    if let Some(date) = agent.handle(r, Timestamp::now()) {
                        open_app(&date);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => std::thread::sleep(wait),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::datetime;

    #[derive(Default)]
    struct Posted(Vec<(Note, u32)>, u32);

    impl Notifier for Posted {
        fn notify(&mut self, n: &Note, replaces: u32) -> Option<u32> {
            self.1 += 1;
            self.0.push((n.clone(), replaces));
            Some(if replaces != 0 { replaces } else { self.1 })
        }
    }

    fn agent(name: &str) -> Agent {
        let dir = std::env::temp_dir().join(format!("koyomi-agent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("work.ics"),
            "BEGIN:VCALENDAR\nX-WR-CALNAME:Work\nBEGIN:VEVENT\nUID:a\nDTSTART;TZID=Europe/London:20261019T093000\nDTEND;TZID=Europe/London:20261019T094500\nRRULE:FREQ=DAILY;COUNT=3\nLOCATION:Room 4\nSUMMARY:Stand-up\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT10M\nEND:VALARM\nEND:VEVENT\nEND:VCALENDAR\n",
        )
        .unwrap();
        let tz = TimeZone::get("Europe/London").unwrap();
        let a = Agent::new(Store::open(&dir), tz, AgentState::default(), &Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
        a
    }

    fn at(h: i8, m: i8, day: i8) -> Timestamp {
        datetime(2026, 10, day, h, m, 0, 0).to_zoned(TimeZone::get("Europe/London").unwrap()).unwrap().timestamp()
    }

    #[test]
    fn alerts_come_once_at_their_time() {
        let mut a = agent("once");
        let mut n = Posted::default();
        assert!(a.due(at(9, 0, 19), true).is_empty());
        // Wakes at the alert, not before.
        assert_eq!(a.next_wake(at(9, 19, 19).checked_add(SignedDuration::from_secs(45)).unwrap()), at(9, 20, 19));
        let notes = a.due(at(9, 20, 19), true);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].summary, "Stand-up");
        assert_eq!(notes[0].body, "In 10 min · 09:30–09:45 · Room 4");
        assert_eq!(notes[0].date, "2026-10-19");
        a.post(notes, &mut n);
        assert!(a.due(at(9, 21, 19), true).is_empty(), "shown once");
        // Snoozed, it comes back ten minutes on.
        assert_eq!(a.handle(Reply::Snooze(1), at(9, 21, 19)), None);
        assert!(a.due(at(9, 25, 19), true).is_empty());
        let again = a.due(at(9, 31, 19), true);
        assert_eq!(again.len(), 1);
        a.post(again, &mut n);
        assert_eq!(n.0.len(), 2);
        assert_eq!(a.handle(Reply::Open(2), at(9, 32, 19)).as_deref(), Some("2026-10-19"));
    }

    #[test]
    fn missed_alerts_catch_up_but_not_for_past_events() {
        let mut a = agent("missed");
        a.state.last_check = Some(at(8, 0, 19).as_second());
        // Asleep from 08:00 to 09:40: the stand-up (till 09:45) still counts.
        let notes = a.due(at(9, 40, 19), true);
        assert_eq!(notes.len(), 1);
        a.state.last_check = Some(at(8, 0, 20).as_second());
        // Woke at 10:00: that day's stand-up is over.
        assert!(a.due(at(10, 0, 20), true).is_empty());
    }
}
