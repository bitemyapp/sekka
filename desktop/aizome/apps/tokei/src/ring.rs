//! `aizome-tokei --ring <id>`: what an alarm's timer runs. It posts a
//! notification with Snooze and Stop, plays the alarm's sound until one is
//! chosen (or ten minutes pass), snoozes through the scheduler, and turns a
//! once-only alarm off when it's stopped.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::alarms::{self, Alarm, Scheduler};
use crate::sound::Player;

/// How long an alarm rings unanswered.
pub const RING_FOR: Duration = Duration::from_secs(600);

/// What the person did with the notification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Snooze,
    Stop,
    /// It rang out unanswered.
    Timeout,
}

/// Posts the alarm's notification and hears its answer.
pub trait Notifier {
    /// Post it: whether it's up.
    fn post(&mut self, summary: &str, body: &str, actions: &[(&str, &str)]) -> bool;
    /// An answer, if one came within `wait`.
    fn answer(&mut self, wait: Duration) -> Option<Answer>;
    /// Take it down.
    fn close(&mut self);
}

/// The notification's text: the label (or "Alarm"), and the time and
/// repeat.
pub fn words(a: &Alarm) -> (String, String) {
    let summary = if a.label.trim().is_empty() { "Alarm".to_owned() } else { a.label.trim().to_owned() };
    let body = format!("{:02}:{:02} · {}", a.hour, a.minute, a.repeat_words());
    (summary, body)
}

/// Ring alarm `id` from the file at `path`: the exit status.
pub fn ring_with(
    path: &Path,
    id: &str,
    notifier: &mut dyn Notifier,
    player: &mut dyn Player,
    scheduler: &mut dyn Scheduler,
    ring_for: Duration,
) -> i32 {
    let mut alarms = alarms::load(path);
    let Some(alarm) = alarms.iter().find(|a| a.id == id).cloned() else {
        log::info!("alarm {id} is gone");
        return 0;
    };
    if !alarm.enabled {
        return 0;
    }
    let (summary, body) = words(&alarm);
    let snooze = format!("Snooze {} min", alarm.snooze_minutes);
    let posted = notifier.post(&summary, &body, &[("snooze", &snooze), ("stop", "Stop")]);
    let started = Instant::now();
    let answer = loop {
        if !player.playing() {
            player.start(&alarm.sound);
        }
        if let Some(a) = notifier.answer(Duration::from_millis(250)) {
            break a;
        }
        // Without a notification to answer, a minute of ringing is enough.
        let limit = if posted { ring_for } else { ring_for.min(Duration::from_secs(60)) };
        if started.elapsed() >= limit {
            break Answer::Timeout;
        }
    };
    player.stop();
    match answer {
        Answer::Snooze => {
            notifier.close();
            if let Err(e) = scheduler.snooze(&alarm, alarm.snooze_minutes.max(1)) {
                log::warn!("snoozing {}: {e}", alarm.unit());
            }
        }
        Answer::Stop | Answer::Timeout => {
            if answer == Answer::Stop {
                notifier.close();
            }
            // A once-only alarm has rung: it's off now (as a phone's is).
            if !alarm.repeats()
                && let Some(a) = alarms.iter_mut().find(|a| a.id == id)
            {
                a.enabled = false;
                if let Err(e) = alarms::save(path, &alarms) {
                    log::warn!("{}: {e}", path.display());
                }
            }
        }
    }
    0
}

/// The desktop's notification server, over D-Bus.
pub struct Dbus {
    conn: Option<zbus::blocking::Connection>,
    id: u32,
    rx: Option<mpsc::Receiver<Answer>>,
}

impl Dbus {
    pub fn connect() -> Dbus {
        match zbus::blocking::Connection::session() {
            Ok(conn) => Dbus { conn: Some(conn), id: 0, rx: None },
            Err(e) => {
                log::warn!("no session bus: {e}");
                Dbus { conn: None, id: 0, rx: None }
            }
        }
    }

    fn proxy(&self) -> Option<zbus::blocking::Proxy<'static>> {
        let conn = self.conn.as_ref()?;
        zbus::blocking::Proxy::new(
            conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .ok()
    }
}

impl Notifier for Dbus {
    fn post(&mut self, summary: &str, body: &str, actions: &[(&str, &str)]) -> bool {
        let Some(proxy) = self.proxy() else { return false };
        // Hear the answer on threads of their own, before posting.
        let (tx, rx) = mpsc::channel();
        if let Ok(signals) = proxy.receive_signal("ActionInvoked") {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for m in signals {
                    if let Ok((id, key)) = m.body().deserialize::<(u32, String)>() {
                        let a = if key == "snooze" { Answer::Snooze } else { Answer::Stop };
                        let _ = tx.send((id, Some(a)));
                    }
                }
            });
        }
        if let Ok(signals) = proxy.receive_signal("NotificationClosed") {
            std::thread::spawn(move || {
                for m in signals {
                    if let Ok((id, _reason)) = m.body().deserialize::<(u32, u32)>() {
                        let _ = tx.send((id, None));
                    }
                }
            });
        }
        let mut list: Vec<&str> = Vec::new();
        for (k, l) in actions {
            list.push(k);
            list.push(l);
        }
        let mut hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        hints.insert("urgency", zbus::zvariant::Value::U8(2));
        hints.insert("resident", zbus::zvariant::Value::Bool(true));
        hints.insert("desktop-entry", zbus::zvariant::Value::from("org.aizome.Tokei"));
        hints.insert("category", zbus::zvariant::Value::from("x-aizome.alarm"));
        let reply: zbus::Result<u32> =
            proxy.call("Notify", &("Tokei", 0u32, "org.aizome.Tokei", summary, body, list, hints, 0i32));
        match reply {
            Ok(id) => {
                self.id = id;
                let (atx, arx) = mpsc::channel();
                std::thread::spawn(move || {
                    for (got, answer) in rx {
                        if got == id {
                            // Closed without an action: it was dismissed.
                            let _ = atx.send(answer.unwrap_or(Answer::Stop));
                            return;
                        }
                    }
                });
                self.rx = Some(arx);
                true
            }
            Err(e) => {
                log::warn!("posting the notification: {e}");
                false
            }
        }
    }

    fn answer(&mut self, wait: Duration) -> Option<Answer> {
        match &self.rx {
            Some(rx) => rx.recv_timeout(wait).ok(),
            None => {
                std::thread::sleep(wait);
                None
            }
        }
    }

    fn close(&mut self) {
        if self.id != 0
            && let Some(p) = self.proxy()
        {
            let _: zbus::Result<()> = p.call("CloseNotification", &(self.id,));
        }
    }
}

/// A notifier that answers as told: for tests.
pub struct Scripted {
    pub posted: Vec<(String, String, Vec<String>)>,
    pub answer: Option<Answer>,
    pub closed: bool,
}

impl Notifier for Scripted {
    fn post(&mut self, summary: &str, body: &str, actions: &[(&str, &str)]) -> bool {
        self.posted.push((summary.into(), body.into(), actions.iter().map(|(_, l)| l.to_string()).collect()));
        true
    }

    fn answer(&mut self, _wait: Duration) -> Option<Answer> {
        self.answer.take()
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarms::Recorder;
    use crate::sound::Silent;

    fn store(name: &str, alarms: &[Alarm]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tokei-ring-{name}-{}", std::process::id()));
        let path = dir.join("alarms.json");
        alarms::save(&path, alarms).unwrap();
        path
    }

    fn alarm(days: &[u8]) -> Alarm {
        Alarm {
            id: "0000beef".into(),
            label: "Tea".into(),
            hour: 16,
            minute: 5,
            days: days.to_vec(),
            ..Alarm::default()
        }
    }

    #[test]
    fn stop_turns_a_once_alarm_off() {
        let path = store("stop", &[alarm(&[])]);
        let mut n = Scripted { posted: Vec::new(), answer: Some(Answer::Stop), closed: false };
        let mut p = Silent::default();
        let mut s = Recorder::new();
        assert_eq!(ring_with(&path, "0000beef", &mut n, &mut p, &mut s, RING_FOR), 0);
        assert_eq!(
            n.posted,
            [("Tea".to_owned(), "16:05 · Once".to_owned(), vec!["Snooze 9 min".to_owned(), "Stop".to_owned()])]
        );
        assert_eq!(p.started, ["alarm-clock-elapsed"]);
        assert!(p.stopped > 0 && n.closed);
        assert!(!alarms::load(&path)[0].enabled);
        assert!(s.calls().is_empty());
    }

    #[test]
    fn snooze_schedules_it_again_and_repeating_alarms_stay_on() {
        let path = store("snooze", &[alarm(&[1, 2])]);
        let mut n = Scripted { posted: Vec::new(), answer: Some(Answer::Snooze), closed: false };
        let mut s = Recorder::new();
        ring_with(&path, "0000beef", &mut n, &mut Silent::default(), &mut s, RING_FOR);
        assert_eq!(s.calls(), ["systemd-run --unit=aizome-alarm-0000beef-snooze --on-active=9min"]);
        assert!(alarms::load(&path)[0].enabled);
        // Rung out: a repeating alarm stays on.
        let mut n = Scripted { posted: Vec::new(), answer: None, closed: false };
        ring_with(&path, "0000beef", &mut n, &mut Silent::default(), &mut Recorder::new(), Duration::from_millis(1));
        assert!(alarms::load(&path)[0].enabled);
        // A disabled or deleted alarm doesn't ring.
        let mut n = Scripted { posted: Vec::new(), answer: None, closed: false };
        ring_with(&path, "nothere0", &mut n, &mut Silent::default(), &mut Recorder::new(), RING_FOR);
        assert!(n.posted.is_empty());
    }
}
