//! Keep Awake in the shell (`aizome-awake`; docs/keep-awake.md): its cup
//! in the systray while it's on, its switch in the cup's panel and the
//! Battery panel, and the palette's command.
//!
//! The shell only asks `aizome-awake` to turn it on or off, and watches
//! whether logind lists the locks. The locks belong to a user service, so
//! restarting or reloading the shell never lets them go; the cup shows
//! Keep Awake as working only while logind lists them.

use std::time::Duration;

use aizome_awake::Status;
use aizome_kit::prelude::*;

use crate::menubar::TrayWidget;
use crate::shell::{Msg, Shell};

impl Shell {
    /// Only the real session's shell changes Keep Awake: never a headless
    /// or nested one's, nor a test's (they keep it to themselves).
    fn awake_is_ours(&self) -> bool {
        self.opts.live && crate::shell::real_session()
    }

    /// Turn Keep Awake on or off, on a thread; the result comes back as
    /// [`Msg::AwakeSet`] and the watcher confirms it with logind.
    pub fn set_keep_awake(&mut self, on: bool, cx: &mut Cx<Msg>) {
        if !self.awake_is_ours() {
            self.keep_awake_status(if on { Status::On } else { Status::Off }, cx);
            return;
        }
        cx.spawn(move || {
            let out = std::process::Command::new("aizome-awake").arg(if on { "on" } else { "off" }).output();
            Msg::AwakeSet(match out {
                Ok(o) if o.status.success() => Ok(on),
                Ok(o) => {
                    let e = String::from_utf8_lossy(&o.stderr);
                    Err(e.trim().trim_start_matches("aizome-awake: ").to_owned())
                }
                Err(e) => Err(format!("couldn't run aizome-awake: {e}")),
            })
        });
    }

    /// `aizome-awake` finished.
    pub fn keep_awake_set(&mut self, result: Result<bool, String>, cx: &mut Cx<Msg>) {
        match result {
            Ok(on) => self.keep_awake_status(if on { Status::On } else { Status::Off }, cx),
            Err(e) => {
                log::warn!("Keep Awake: {e}");
                let n = crate::notifications::store::Incoming {
                    app_name: "Keep Awake".into(),
                    summary: "Keep Awake needs attention".into(),
                    body: e,
                    urgency: crate::notifications::store::Urgency::Critical,
                    ..Default::default()
                };
                self.shell_note(n, Vec::new(), cx);
                // The watcher says what logind holds now.
            }
        }
    }

    /// What the watcher (or a change) says now.
    pub fn keep_awake_status(&mut self, status: Status, cx: &mut Cx<Msg>) {
        if self.awake == status {
            return;
        }
        self.awake = status;
        self.tray.awake = (status != Status::Off).then_some(status);
        // The cup goes with it: so does its panel.
        if status == Status::Off && self.panel.as_ref().is_some_and(|p| p.widget == TrayWidget::Awake) {
            self.close_panel(cx);
        }
    }
}

/// Watches Keep Awake, on a thread: the setting, and while it's on whether
/// logind lists the locks, every two seconds; the shell hears only changes.
pub fn watch(tx: impl Fn(Msg) -> bool + Send + 'static) {
    let spawned = std::thread::Builder::new().name("keep-awake".into()).spawn(move || {
        let mut bus = None;
        let mut last = None;
        loop {
            let s = read(&mut bus);
            if last != Some(s) {
                last = Some(s);
                if !tx(Msg::Awake(s)) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    if let Err(e) = spawned {
        log::warn!("Keep Awake's watcher: {e}");
    }
}

/// Keep Awake now. Off costs a stat; on, a call to logind (on a bus kept
/// between calls, and opened again when it fails).
fn read(bus: &mut Option<zbus::blocking::Connection>) -> Status {
    if !aizome_awake::requested() {
        return Status::Off;
    }
    let protected = (|| -> zbus::Result<bool> {
        if bus.is_none() {
            *bus = Some(zbus::blocking::Connection::system()?);
        }
        let reply = bus.as_ref().expect("opened above").call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "ListInhibitors",
            &(),
        )?;
        let list: Vec<aizome_awake::Inhibitor> = reply.body().deserialize()?;
        // Safety: geteuid has no preconditions.
        Ok(aizome_awake::protected_by(&list, unsafe { libc::geteuid() }, None))
    })();
    match protected {
        Ok(p) => Status::of(true, p),
        Err(e) => {
            log::warn!("Keep Awake: asking logind: {e}");
            *bus = None;
            Status::Unprotected
        }
    }
}
