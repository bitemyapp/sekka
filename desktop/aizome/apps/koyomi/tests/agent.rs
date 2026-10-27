//! The alert agent against a notification daemon of its own on a
//! private bus: an alert is posted with Snooze and Open, Snooze brings
//! it back later, Open says which day to show.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jiff::civil::datetime;
use jiff::tz::TimeZone;
use koyomi::agent::{Agent, AgentState, Dbus, Reply};
use koyomi::settings::Settings;
use koyomi::store::Store;
use zbus::zvariant::OwnedValue;

/// A `dbus-daemon` of the test's own.
struct Bus {
    child: Child,
    address: String,
    dir: PathBuf,
}

impl Bus {
    fn start() -> Option<Bus> {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("koyomi-bus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n<busconfig><type>session</type><listen>unix:dir={}</listen><auth>EXTERNAL</auth><policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
                dir.display()
            ),
        )
        .ok()?;
        let mut child = match Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping: no dbus-daemon ({e})");
                return None;
            }
        };
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        let address = line.trim().to_owned();
        (!address.is_empty()).then_some(Bus { child, address, dir })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

type Posted = Arc<Mutex<Vec<(u32, String, String, Vec<String>, String)>>>;

/// The shell's part: org.freedesktop.Notifications, writing down what's posted.
struct Daemon {
    posted: Posted,
    next: u32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Daemon {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &mut self,
        app: String,
        replaces: u32,
        _icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        _timeout: i32,
    ) -> u32 {
        let entry = hints.get("desktop-entry").and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default();
        let id = if replaces != 0 {
            replaces
        } else {
            self.next += 1;
            self.next
        };
        self.posted.lock().unwrap().push((id, format!("{app}: {summary}"), body, actions, entry));
        id
    }
}

#[test]
fn alerts_are_notifications_with_snooze_and_open() {
    let Some(bus) = Bus::start() else { return };
    let posted: Posted = Arc::default();
    let server = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at("/org/freedesktop/Notifications", Daemon { posted: posted.clone(), next: 0 })
        .unwrap()
        .build()
        .unwrap();

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("koyomi-agent-bus");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("work.ics"),
        "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Work\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART;TZID=Europe/London:20261019T093000\r\nDTEND;TZID=Europe/London:20261019T094500\r\nSUMMARY:Stand-up\r\nLOCATION:Room 4\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT10M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    )
    .unwrap();
    let tz = TimeZone::get("Europe/London").unwrap();
    let at = |h: i8, m: i8| datetime(2026, 10, 19, h, m, 0, 0).to_zoned(tz.clone()).unwrap().timestamp();
    let mut agent = Agent::new(Store::open(&dir), tz.clone(), AgentState::default(), &Settings::default());
    let mut dbus = Dbus::at(&bus.address).unwrap();
    let (tx, rx) = channel();
    dbus.listen(tx).unwrap();

    let notes = agent.due(at(9, 20), true);
    agent.post(notes, &mut dbus);
    let (id, summary, body, actions, entry) = posted.lock().unwrap()[0].clone();
    assert_eq!(summary, "Koyomi: Stand-up");
    assert_eq!(body, "In 10 min · 09:30–09:45 · Room 4");
    assert_eq!(actions, ["default", "Open", "snooze", "Snooze", "open", "Open"]);
    assert_eq!(entry, "org.aizome.Koyomi");

    // The person presses Snooze.
    server
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(id, "snooze"),
        )
        .unwrap();
    let reply = rx.recv_timeout(Duration::from_secs(5)).expect("the agent hears it");
    assert_eq!(reply, Reply::Snooze(id));
    assert_eq!(agent.handle(reply, at(9, 21)), None);
    assert!(agent.due(at(9, 25), true).is_empty());
    let again = agent.due(at(9, 31), true);
    assert_eq!(again.len(), 1);
    agent.post(again, &mut dbus);
    let id2 = posted.lock().unwrap()[1].0;
    // Then Open: the day to show.
    server
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(id2, "open"),
        )
        .unwrap();
    let reply = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(agent.handle(reply, at(9, 32)).as_deref(), Some("2026-10-19"));
    // Closing one is heard too.
    server
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationClosed",
            &(id2, 2u32),
        )
        .unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Reply::Closed(id2));
}
