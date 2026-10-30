//! Launch attempts, their observed outcomes, and a window for their logs.
//! A process starting is not evidence that an application opened a window.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use aizome_kit::{column, prelude::*, row};
use aizome_sys::apps::{App, LaunchEvent, LaunchObserver, LaunchOptions};

use crate::{
    actions::Action,
    launch::Launch,
    shell::{Msg, Shell, Surface},
};

const LIMIT: usize = 64;
const WAIT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub enum Event {
    Begin {
        launch: Launch,
        token: Option<String>,
        cwd: Option<PathBuf>,
    },
    Process(u64, LaunchEvent),
    Failed(u64, String),
    Check(u64),
    Select(u64),
    Logs(u64, String),
    Refresh,
    Copy,
    /// Open the selected attempt's output in Nisshi, the log viewer.
    OpenLogs,
    Close,
}

pub struct Attempt {
    pub name: String,
    pub app_id: Option<String>,
    pub journal_id: String,
    pub outcome: String,
    pub messages: Vec<String>,
    pub logs: String,
    pub ready: bool,
    pub failed: bool,
    pub activated: bool,
    expects_window: bool,
    had_window: bool,
    windows_before: BTreeSet<aizome_ipc::WindowId>,
    /// The processes it started.
    pids: Vec<u32>,
    /// The window that confirmed it.
    window: Option<aizome_ipc::WindowId>,
    exited: bool,
    notification: Option<u32>,
    started: Instant,
}

impl Attempt {
    fn record(&mut self, message: impl Into<String>) {
        if self.messages.len() >= 128 {
            self.messages.remove(0);
        }
        self.messages.push(format!("+{:.3}s  {}", self.started.elapsed().as_secs_f64(), message.into()));
    }

    fn details(&self) -> String {
        format!(
            "{}\n{}\n\n{}\n\nApplication output (last 200 journal entries, at most 64 KiB):\n{}\n\nJournal identifier: {}",
            self.name,
            self.outcome,
            self.messages.join("\n"),
            self.logs,
            self.journal_id
        )
    }
}

#[derive(Default)]
pub struct Startup {
    pub attempts: BTreeMap<u64, Attempt>,
    next: u64,
    pub window: Option<WindowKey>,
    selected: Option<u64>,
}

impl Shell {
    pub(crate) fn start_observed(
        &mut self,
        launch: Launch,
        token: Option<String>,
        cwd: Option<PathBuf>,
        cx: &mut Cx<Msg>,
    ) {
        let entry: Option<App> = match &launch {
            Launch::App { app_id, .. } => self.apps.as_deref().and_then(|i| i.match_window_identity(app_id)).cloned(),
            Launch::Argv(argv) => {
                argv.first().and_then(|program| self.apps.as_deref()?.match_program_identity(program)).cloned()
            }
        };
        let name = entry.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| match &launch {
            Launch::App { app_id, .. } => app_id.clone(),
            Launch::Argv(argv) => argv.first().cloned().unwrap_or_else(|| "Command".into()),
        });
        let app_id = entry.as_ref().map(|e| e.app_id().to_owned());
        let expects_window = matches!(launch, Launch::App { .. })
            && entry.as_ref().is_some_and(|e| {
                !e.terminal && (e.startup_notify == Some(true) || e.app_id().starts_with("org.aizome."))
            });
        let had_window =
            app_id.as_ref().is_some_and(|id| self.wm.windows.values().any(|w| self.same_app(id, &w.app_id)));
        self.startup.next += 1;
        let id = self.startup.next;
        let tx = cx.stream();
        let observer = LaunchObserver::new(move |event| {
            tx.send(Msg::Startup(Event::Process(id, event)));
        });
        let mut attempt = Attempt {
            name,
            app_id,
            journal_id: observer.journal_id().into(),
            outcome: "Starting".into(),
            messages: vec![],
            logs: "Select Refresh to read the application output.".into(),
            ready: false,
            failed: false,
            activated: false,
            expects_window,
            had_window,
            windows_before: self.wm.windows.keys().copied().collect(),
            pids: vec![],
            window: None,
            exited: false,
            notification: None,
            started: Instant::now(),
        };
        if let Some(e) = &entry {
            attempt.record(format!(
                "Application entry: {}",
                e.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| e.id.clone())
            ));
        }
        attempt.record(format!("Requested: {launch:?}"));
        if had_window {
            attempt.record("The application already had a window.");
        }
        while self.startup.attempts.len() >= LIMIT {
            let oldest = *self.startup.attempts.keys().next().unwrap();
            self.startup.attempts.remove(&oldest);
        }
        self.startup.attempts.insert(id, attempt);
        cx.after(WAIT, Msg::Startup(Event::Check(id)));
        let options = LaunchOptions {
            activation_token: token,
            working_dir: cwd,
            targets: match &launch {
                Launch::App { targets, .. } => targets.clone(),
                _ => vec![],
            },
            terminal: self
                .apps
                .as_deref()
                .and_then(|i| i.terminal_command())
                .unwrap_or_else(|| vec!["foot".into(), "--".into()]),
            ..Default::default()
        };
        let tx = cx.stream();
        let task = std::thread::Builder::new().name("application-launch".into()).spawn(move || {
            let result = match (&launch, entry) {
                (Launch::App { .. }, Some(entry)) => {
                    aizome_sys::apps::launch_observed(&entry, &options, Some(&observer))
                }
                (Launch::App { app_id, .. }, None) => Err(aizome_sys::Error::NotFound(format!("application {app_id}"))),
                // Its scope is the app's, when the command is a known app's.
                (Launch::Argv(argv), entry) => aizome_sys::apps::launch_command_observed(
                    argv,
                    entry.as_ref().map(|e| e.app_id()),
                    &options,
                    &observer,
                ),
            };
            if let Err(error) = result {
                tx.send(Msg::Startup(Event::Failed(id, error.to_string())));
            }
        });
        if let Err(error) = task {
            self.startup_event(Event::Failed(id, format!("Could not create a launch worker: {error}")), cx);
        }
    }

    fn same_app(&self, a: &str, b: &str) -> bool {
        a == b
            || self.apps.as_deref().is_some_and(|index| {
                index.match_window_identity(a).zip(index.match_window_identity(b)).is_some_and(|(a, b)| a.id == b.id)
            })
    }

    /// Only a newly mapped window confirms a graphical launch. Existing
    /// windows and a successful helper exit are recorded separately.
    ///
    /// A window confirms one launch: the one whose process opened it, else
    /// the oldest of that app's still waiting. Two quick launches of an app
    /// need a window each, so a second that never opens one is reported.
    pub(crate) fn startup_window(&mut self, window: &aizome_ipc::WindowInfo) {
        let app_id = &window.app_id;
        if self.startup.attempts.values().any(|a| a.window == Some(window.id)) {
            return;
        }
        let waiting: Vec<u64> = self
            .startup
            .attempts
            .iter()
            .filter(|(_, a)| {
                !a.ready
                    && !a.failed
                    && a.started.elapsed() < Duration::from_secs(120)
                    && !a.windows_before.contains(&window.id)
                    && a.app_id.as_ref().is_some_and(|id| self.same_app(id, app_id))
            })
            .map(|(id, _)| *id)
            .collect();
        let pid = window.pid.and_then(|p| u32::try_from(p).ok());
        let own = waiting.iter().copied().find(|id| pid.is_some_and(|p| self.startup.attempts[id].pids.contains(&p)));
        let Some(id) = own.or_else(|| waiting.first().copied()) else { return };
        let a = self.startup.attempts.get_mut(&id).unwrap();
        a.ready = true;
        a.window = Some(window.id);
        a.outcome = "Window opened".into();
        a.record(format!("Compositor observed a window for {app_id}."));
        if let Some(note) = a.notification.take() {
            self.dismiss_note(note);
        }
    }

    pub(crate) fn startup_event(&mut self, event: Event, cx: &mut Cx<Msg>) {
        match event {
            Event::Begin { launch, token, cwd } => self.start_observed(launch, token, cwd, cx),
            Event::Select(id) => self.show_startup(Some(id), cx),
            Event::Refresh => self.read_startup_logs(cx),
            Event::Copy => {
                if let Some(a) = self.startup.selected.and_then(|id| self.startup.attempts.get(&id)) {
                    cx.set_clipboard_text(&a.details());
                }
            }
            Event::OpenLogs => {
                if let Some(a) = self.startup.selected.and_then(|id| self.startup.attempts.get(&id)) {
                    let argv = logs_command(&a.journal_id);
                    self.launch(crate::launch::Launch::Argv(argv), cx);
                }
            }
            Event::Close => {
                if let Some(w) = self.startup.window.take() {
                    self.surfaces.remove(&w);
                    cx.close_window(w);
                }
            }
            Event::Logs(id, logs) => {
                if let Some(a) = self.startup.attempts.get_mut(&id) {
                    a.logs = logs;
                }
            }
            Event::Process(id, event) => {
                let Some(a) = self.startup.attempts.get_mut(&id) else { return };
                let mut failed = false;
                match event {
                    LaunchEvent::Started(p) => {
                        a.record(format!(
                            "Process started: pid {}, scope {}. Arguments: {:?}",
                            p.pid,
                            p.unit.as_deref().unwrap_or("none"),
                            p.argv
                        ));
                        a.pids.push(p.pid);
                        if !a.ready && !a.failed {
                            a.outcome = "Process started; no window confirmed".into();
                        }
                    }
                    LaunchEvent::Activated => {
                        a.activated = true;
                        a.record("Application accepted D-Bus activation.");
                        if !a.ready {
                            a.outcome = "Activation accepted; no new window confirmed".into();
                        }
                    }
                    LaunchEvent::Message(s) => a.record(s),
                    LaunchEvent::Exited { pid, status } => {
                        a.exited = true;
                        let success = status.as_ref().is_ok_and(|s| s.success());
                        let crash = status.as_ref().is_ok_and(crashed);
                        let status = status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|e| format!("could not read exit status: {e}"));
                        a.record(format!("Process {pid}: {status}"));
                        if a.ready {
                            // Its window opened: how it ends is its own
                            // (a terminal passes on its shell's status),
                            // unless it crashed while starting.
                            if crash && a.started.elapsed() <= Duration::from_secs(30) {
                                a.failed = true;
                                a.outcome = format!("Closed during startup: {status}");
                                failed = true;
                            }
                        } else {
                            a.failed |= !success;
                            if success && a.failed {
                                return;
                            }
                            a.outcome = if success {
                                if a.had_window {
                                    "Command completed; application already had a window".into()
                                } else {
                                    "Command completed; no window confirmed".into()
                                }
                            } else {
                                format!("Launch failed: {status}")
                            };
                            failed = !success && a.started.elapsed() <= Duration::from_secs(120);
                        }
                    }
                }
                if failed {
                    self.notify_startup(id, cx);
                }
            }
            Event::Failed(id, error) => {
                if let Some(a) = self.startup.attempts.get_mut(&id) {
                    a.failed = true;
                    a.outcome = "Could not start".into();
                    a.record(error);
                }
                self.notify_startup(id, cx);
            }
            Event::Check(id) => {
                let Some(a) = self.startup.attempts.get_mut(&id) else { return };
                if a.expects_window && !a.ready && !a.failed && !a.had_window && !a.activated {
                    a.outcome = if a.exited {
                        "Command finished without a confirmed window".into()
                    } else {
                        "Still starting; no window confirmed after 15 seconds".into()
                    };
                    a.record(a.outcome.clone());
                    self.notify_startup(id, cx);
                }
            }
        }
    }

    fn notify_startup(&mut self, id: u64, cx: &mut Cx<Msg>) {
        let Some(a) = self.startup.attempts.get_mut(&id) else { return };
        let previous = a.notification.take();
        let n = crate::notifications::store::Incoming {
            app_name: "Aizome".into(),
            summary: format!("{}: {}", a.name, if a.failed { "could not open" } else { "waiting for a window" }),
            body: format!("{}. Show Details to see what happened and the application output.", a.outcome),
            desktop_entry: a.app_id.clone(),
            // A failure stays until seen; a slow start is only news, and
            // its window may come long after the shell stops waiting.
            urgency: if a.failed {
                crate::notifications::store::Urgency::Critical
            } else {
                crate::notifications::store::Urgency::Normal
            },
            ..Default::default()
        };
        let action = Action::LaunchDetails { id: Some(id) };
        if let Some(note) = previous {
            self.dismiss_note(note);
        }
        let note = self.shell_note(
            n,
            vec![
                ("default".into(), "Show Details".into(), action.clone()),
                ("details".into(), "Show Details".into(), action),
            ],
            cx,
        );
        if let Some(a) = self.startup.attempts.get_mut(&id) {
            a.notification = Some(note);
        }
    }

    pub(crate) fn show_startup(&mut self, id: Option<u64>, cx: &mut Cx<Msg>) {
        if self.locked() {
            return;
        }
        self.startup.selected = id
            .filter(|id| self.startup.attempts.contains_key(id))
            .or_else(|| self.startup.attempts.keys().next_back().copied());
        if let Some(w) = self.startup.window {
            cx.activate(w);
        } else {
            let w = cx.open_window(WindowSpec::new("Application Launches").size(900.0, 600.0));
            self.surfaces.insert(w, Surface::Startup);
            self.startup.window = Some(w);
        }
        self.read_startup_logs(cx);
    }

    fn read_startup_logs(&mut self, cx: &mut Cx<Msg>) {
        let Some(id) = self.startup.selected else { return };
        let Some(a) = self.startup.attempts.get_mut(&id) else { return };
        a.logs = "Reading application output…".into();
        let journal_id = a.journal_id.clone();
        let tx = cx.stream();
        std::thread::spawn(move || {
            tx.send(Msg::Startup(Event::Logs(id, read_logs(&journal_id))));
        });
    }

    pub(crate) fn startup_view(&self) -> Element<Msg> {
        let list = self
            .startup
            .attempts
            .iter()
            .rev()
            .map(|(id, a)| {
                let summary = if a.failed {
                    "Failed"
                } else if a.ready {
                    "Window opened"
                } else if a.activated {
                    "Activation accepted"
                } else if a.exited {
                    "Command completed"
                } else {
                    "Starting"
                };
                let name: String = a.name.chars().take(32).collect();
                button(format!("{name}\n{summary}"))
                    .on_press(Msg::Startup(Event::Select(*id)))
                    .width(Length::Fill)
                    .height(52.0)
                    .into()
            })
            .collect();
        let selected = self.startup.selected.and_then(|id| self.startup.attempts.get(&id));
        let details = selected.map(Attempt::details).unwrap_or_else(|| "Applications opened from the Toolchest, palette and Deskbar appear here. The last 64 attempts in this shell session are kept.".into());
        let enabled = selected.is_some();
        column![
            text("Application Launches").size(20.0),
            row![
                scroll(aizome_kit::column(list).spacing(6.0)).width(260.0).height(Length::Fill),
                scroll(
                    column![text(details).mono().size(12.0).wrap().selectable().width(Length::Fill)]
                        .width(Length::Fill)
                )
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .align(aizome_kit::Align::Start)
            .spacing(16.0)
            .height(Length::Fill),
            row![
                button("Copy Details").enabled(enabled).on_press(Msg::Startup(Event::Copy)),
                button("Refresh Logs").enabled(enabled).on_press(Msg::Startup(Event::Refresh)),
                button("Open in Logs")
                    .enabled(enabled)
                    .on_press(Msg::Startup(Event::OpenLogs))
                    .tooltip("This launch's output in Logs (Nisshi), with what else was written then"),
                spacer(),
                button("Close").cancel().on_press(Msg::Startup(Event::Close)),
            ]
            .spacing(8.0),
        ]
        .spacing(12.0)
        .padding(16.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

/// Nisshi, the log viewer, on one launch's output: where its colours,
/// search and what else was written at the time are a press away.
pub(crate) fn logs_command(journal_id: &str) -> Vec<String> {
    vec!["aizome-nisshi".into(), "--launch".into(), journal_id.into()]
}

/// Whether a process ended by crashing: a signal for a fault or an abort,
/// or a shell's status for its child's (128 + the signal), not one asked
/// for (TERM, KILL, INT from someone ending it) or an exit code it chose.
fn crashed(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    let fault = |s: i32| {
        [libc::SIGSEGV, libc::SIGABRT, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGTRAP, libc::SIGSYS]
            .contains(&s)
    };
    status.signal().is_some_and(fault) || status.code().is_some_and(|c| c > 128 && fault(c - 128))
}

/// Read only this launch's journal stream. Bound time and memory, drain
/// stdout concurrently, and keep all journal I/O off the interface thread.
fn read_logs(identifier: &str) -> String {
    use std::{
        io::Read,
        process::{Command, Stdio},
        sync::mpsc,
    };
    let mut child = match Command::new("journalctl")
        .args(["--user", "-b", "--no-pager", "--output=short-precise", "--lines=200"])
        .arg(format!("SYSLOG_IDENTIFIER={identifier}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("Could not read the journal: {e}"),
    };
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    for (which, mut stream) in
        [(true, Box::new(out) as Box<dyn Read + Send>), (false, Box::new(err) as Box<dyn Read + Send>)]
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = (|| -> std::io::Result<()> {
                let mut buffer = [0u8; 8192];
                loop {
                    let n = stream.read(&mut buffer)?;
                    if n == 0 {
                        return Ok(());
                    }
                    // Keep draining after the cap so journalctl does not
                    // get SIGPIPE and turn truncation into a false error.
                    let keep = n.min(65_537usize.saturating_sub(bytes.len()));
                    bytes.extend_from_slice(&buffer[..keep]);
                }
            })();
            let _ = tx.send((which, result.map(|_| bytes)));
        });
    }
    drop(tx);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Err(e) => break Err(e.to_string()),
            Ok(None) if Instant::now() >= deadline => break Err("journal query timed out".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    if status.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut stdout = String::new();
    let mut stderr = String::new();
    for _ in 0..2 {
        if let Ok((out, bytes)) = rx.recv_timeout(Duration::from_secs(1)) {
            let text = match bytes {
                Ok(mut b) => {
                    let truncated = b.len() > 65_536;
                    b.truncate(65_536);
                    format!("{}{}", String::from_utf8_lossy(&b), if truncated { "\n[Output truncated]" } else { "" })
                }
                Err(e) => format!("Could not read journal output: {e}"),
            };
            if out {
                stdout = text;
            } else {
                stderr = text;
            }
        }
    }
    match status {
        Ok(s) if s.success() => {
            if stdout.trim().is_empty() || stdout.trim() == "-- No entries --" {
                "No application output was recorded for this attempt. D-Bus activation and failures before execution may have no process output.".into()
            } else {
                stdout
            }
        }
        Ok(s) => format!("Could not read the journal ({s}): {stderr}\n{stdout}"),
        Err(e) => format!("Could not read the journal: {e}\n{stderr}\n{stdout}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::Options;
    use aizome_kit::testing::Harness;
    use std::os::unix::process::ExitStatusExt;

    fn attempt() -> Attempt {
        Attempt {
            name: "Koe failure fixture".into(),
            app_id: Some("org.aizome.Koe".into()),
            journal_id: "test-launch".into(),
            outcome: "Starting".into(),
            messages: vec![],
            logs: String::new(),
            ready: false,
            failed: false,
            activated: false,
            expects_window: true,
            had_window: false,
            windows_before: BTreeSet::new(),
            pids: vec![],
            window: None,
            exited: false,
            notification: None,
            started: Instant::now(),
        }
    }

    fn fixture() -> Harness<Shell> {
        let mut h = Harness::new(Shell::new(Options::default()));
        h.program_mut().startup.attempts.insert(1, attempt());
        h
    }

    fn opened(id: aizome_ipc::WindowId, pid: Option<i32>) -> Msg {
        Msg::Ipc(crate::ipc::IpcEvent::Event(aizome_ipc::Event::WindowOpened {
            window: aizome_ipc::WindowInfo { id, app_id: "org.aizome.Koe".into(), pid, ..Default::default() },
        }))
    }

    fn exit(h: &mut Harness<Shell>, code: i32) {
        h.send(Msg::Startup(Event::Process(
            1,
            LaunchEvent::Exited { pid: 123, status: Ok(std::process::ExitStatus::from_raw(code << 8)) },
        )));
    }

    /// Optional illustrations rendered by the actual UI with synthetic events.
    /// These are mocks, not evidence of an application or compositor running.
    fn save_mock(h: &mut Harness<Shell>, name: &str) {
        let Some(dir) = std::env::var_os("AIZOME_STARTUP_SCREENSHOTS") else { return };
        h.program_mut().startup.attempts.get_mut(&1).unwrap().name = "Koe (mock)".into();
        h.send(Msg::Startup(Event::Select(1)));
        assert!(h.settle(Duration::from_secs(7), |s| s.startup.attempts[&1].logs != "Reading application output…"));
        h.send(Msg::Startup(Event::Logs(1, "Mock: synthetic lifecycle events; no application output.".into())));
        std::fs::create_dir_all(&dir).unwrap();
        let w = h.program().startup.window.unwrap();
        h.save_png(w, PathBuf::from(dir).join(name), 1.0);
    }

    #[test]
    fn failure_is_visible_and_details_show_the_error_and_copyable_output() {
        let mut h = fixture();
        exit(&mut h, 23);
        let note = h.program().notes.list.last().unwrap();
        assert!(note.summary.contains("could not open"));
        assert!(note.buttons().any(|(_, title)| title == "Show Details"));
        let id = note.id;
        h.program_mut().note_action(id, "details");
        h.send(Msg::Nothing);
        let w = h.program().startup.window.expect("details opened from notification");
        assert!(h.settle(Duration::from_secs(7), |s| s.startup.attempts[&1].logs != "Reading application output…"));
        h.send(Msg::Startup(Event::Logs(1, "koe: daemon did not become ready\nexit status 23".into())));
        h.render(w, 1.0);
        let inspected = format!("{:?}", h.inspected(w));
        assert!(inspected.contains("daemon did not become ready"));
        assert!(inspected.contains("Copy Details"));
        h.send(Msg::Startup(Event::Copy));
        assert!(h.platform.clipboard.as_deref().unwrap_or_default().contains("exit status 23"));
        if let Some(dir) = std::env::var_os("AIZOME_STARTUP_SCREENSHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            h.save_png(w, PathBuf::from(dir).join("startup-details.png"), 1.0);
        }
        h.send(Msg::Startup(Event::Close));
        assert!(h.program().startup.window.is_none());
    }

    #[test]
    fn open_in_logs_opens_this_launch_in_nisshi() {
        let mut h = fixture();
        let journal_id = "aizome-launch-0123456789abcdef0123456789abcdef";
        h.program_mut().startup.attempts.get_mut(&1).unwrap().journal_id = journal_id.into();
        exit(&mut h, 23);
        h.send(Msg::Startup(Event::Select(1)));
        let w = h.program().startup.window.expect("the launches window");
        assert!(h.settle(Duration::from_secs(7), |s| s.startup.attempts[&1].logs != "Reading application output…"));
        // Activation is asked for and recorded; no token comes, so nothing
        // actually starts.
        h.program_mut().opts.live = true;
        h.render(w, 1.0);
        h.click_text(w, "Open in Logs");
        let launches: Vec<_> = h.program().launches.values().map(|(l, _)| l.clone()).collect();
        assert_eq!(
            launches,
            [crate::launch::Launch::Argv(vec!["aizome-nisshi".into(), "--launch".into(), journal_id.into()])]
        );
        assert_eq!(h.platform.activation_requests.len(), 1, "the window it opens may take the focus");
    }

    #[test]
    fn process_creation_is_not_window_readiness_and_slow_start_is_not_killed() {
        let mut h = fixture();
        h.send(Msg::Startup(Event::Process(
            1,
            LaunchEvent::Started(aizome_sys::apps::Launched { pid: 123, unit: None, argv: vec!["fixture".into()] }),
        )));
        assert!(!h.program().startup.attempts[&1].ready);
        h.send(Msg::Startup(Event::Check(1)));
        assert!(!h.program().startup.attempts[&1].failed);
        let note = h.program().notes.list.last().unwrap();
        assert!(note.summary.contains("waiting for a window"));
        // News, not a card that stays until the window comes.
        assert_eq!(note.urgency, crate::notifications::store::Urgency::Normal);
        save_mock(&mut h, "mock-waiting.png");
        h.send(Msg::Ipc(crate::ipc::IpcEvent::Event(aizome_ipc::Event::WindowOpened {
            window: aizome_ipc::WindowInfo { id: 42, app_id: "org.aizome.Koe".into(), ..Default::default() },
        })));
        assert!(h.program().startup.attempts[&1].ready);
        assert_eq!(h.program().startup.attempts[&1].outcome, "Window opened");
        save_mock(&mut h, "mock-window-opened.png");
    }

    #[test]
    fn successful_handoff_and_dbus_acceptance_are_distinct_from_window_readiness() {
        let mut h = fixture();
        h.program_mut().startup.attempts.get_mut(&1).unwrap().had_window = true;
        exit(&mut h, 0);
        h.send(Msg::Startup(Event::Check(1)));
        assert!(h.program().notes.list.is_empty());
        assert!(!h.program().startup.attempts[&1].ready);
        assert!(h.program().startup.attempts[&1].outcome.contains("already had a window"));
        let mut h = fixture();
        h.send(Msg::Startup(Event::Process(1, LaunchEvent::Activated)));
        h.send(Msg::Startup(Event::Check(1)));
        assert!(h.program().notes.list.is_empty());
        assert!(h.program().startup.attempts[&1].activated);
        assert!(!h.program().startup.attempts[&1].ready);
    }

    #[test]
    fn late_window_identity_confirms_only_a_new_window_and_early_crashes_are_visible() {
        let mut h = fixture();
        h.program_mut().startup.attempts.get_mut(&1).unwrap().windows_before.insert(41);
        let changed = |id| {
            Msg::Ipc(crate::ipc::IpcEvent::Event(aizome_ipc::Event::WindowChanged {
                window: aizome_ipc::WindowInfo { id, app_id: "org.aizome.Koe".into(), ..Default::default() },
            }))
        };
        h.send(changed(41));
        assert!(!h.program().startup.attempts[&1].ready, "existing window is not evidence of a new launch");
        h.send(changed(42));
        assert!(h.program().startup.attempts[&1].ready);
        exit(&mut h, 139);
        assert!(h.program().startup.attempts[&1].failed);
        assert!(h.program().startup.attempts[&1].outcome.contains("Closed during startup"));
        let note = h.program().notes.list.last().unwrap();
        assert!(note.summary.contains("could not open"));
        assert_eq!(note.urgency, crate::notifications::store::Urgency::Critical);
    }

    #[test]
    fn a_window_confirms_one_launch() {
        let mut h = fixture();
        let mut second = attempt();
        second.pids.push(456);
        h.program_mut().startup.attempts.insert(2, second);
        // The second launch's own process opened it.
        h.send(opened(42, Some(456)));
        assert!(!h.program().startup.attempts[&1].ready);
        assert!(h.program().startup.attempts[&2].ready);
        // The same window again (its title changed) confirms nothing more.
        h.send(Msg::Ipc(crate::ipc::IpcEvent::Event(aizome_ipc::Event::WindowChanged {
            window: aizome_ipc::WindowInfo { id: 42, app_id: "org.aizome.Koe".into(), ..Default::default() },
        })));
        assert!(!h.program().startup.attempts[&1].ready);
        // Another window, from a process neither started: the oldest waiting.
        h.send(opened(43, Some(789)));
        assert!(h.program().startup.attempts[&1].ready);
        assert_eq!(h.program().startup.attempts[&1].window, Some(43));
        // A third launch with no window of its own is still asked about.
        h.program_mut().startup.attempts.insert(3, attempt());
        h.send(Msg::Startup(Event::Check(3)));
        assert!(!h.program().startup.attempts[&3].ready);
        assert!(h.program().notes.list.last().unwrap().summary.contains("waiting for a window"));
    }

    #[test]
    fn after_its_window_opens_only_a_crash_is_a_failure() {
        let mut h = fixture();
        h.send(opened(42, None));
        // A terminal passing on its shell's status, or an app closed soon.
        exit(&mut h, 1);
        let a = &h.program().startup.attempts[&1];
        assert!(a.ready && !a.failed);
        assert_eq!(a.outcome, "Window opened");
        assert!(a.messages.iter().any(|m| m.contains("exit status: 1")));
        assert!(h.program().notes.list.is_empty());
        // Ended by someone (TERM): not a crash either.
        let mut h = fixture();
        h.send(opened(42, None));
        h.send(Msg::Startup(Event::Process(
            1,
            LaunchEvent::Exited { pid: 123, status: Ok(std::process::ExitStatus::from_raw(libc::SIGTERM)) },
        )));
        assert!(!h.program().startup.attempts[&1].failed);
        // Aborted: it is.
        let mut h = fixture();
        h.send(opened(42, None));
        h.send(Msg::Startup(Event::Process(
            1,
            LaunchEvent::Exited { pid: 123, status: Ok(std::process::ExitStatus::from_raw(libc::SIGABRT)) },
        )));
        assert!(h.program().startup.attempts[&1].failed);
        assert!(h.program().notes.list.last().unwrap().summary.contains("could not open"));
    }

    #[test]
    fn shell_launch_path_reports_real_child_failure() {
        let mut h = Harness::new(Shell::new(Options::default()));
        h.send(Msg::Startup(Event::Begin {
            launch: Launch::Argv(vec![
                "sh".into(),
                "-c".into(),
                "sleep 0.1; echo startup-fixture-failure >&2; exit 23".into(),
            ]),
            token: None,
            cwd: None,
        }));
        assert!(h.settle(Duration::from_secs(10), |s| s.startup.attempts.values().any(|a| a.failed)));
        assert_eq!(h.program().startup.attempts.len(), 1);
        assert_eq!(h.program().notes.list.len(), 1);
        let a = h.program().startup.attempts.values().next().unwrap();
        assert!(a.messages.iter().any(|m| m.contains("Process started")));
        assert!(a.messages.iter().any(|m| m.contains("exit status: 23")));
    }

    #[test]
    #[ignore = "requires this user's journal; writes only uniquely identified fixture output"]
    fn journal_output_is_isolated_between_launch_attempts() {
        assert!(std::path::Path::new("/run/systemd/journal/stdout").exists());
        let mut observers = Vec::new();
        for marker in ["startup-test-only-first", "startup-test-only-second"] {
            let (tx, rx) = std::sync::mpsc::channel();
            let observer = LaunchObserver::new(move |event| {
                let _ = tx.send(event);
            });
            aizome_sys::apps::launch_command_observed(
                &["sh".into(), "-c".into(), format!("echo {marker}; echo stderr-{marker} >&2; exit 23")],
                None,
                &LaunchOptions { use_systemd: false, ..Default::default() },
                &observer,
            )
            .unwrap();
            loop {
                if matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), LaunchEvent::Exited { .. }) {
                    break;
                }
            }
            observers.push(observer);
        }
        for (index, observer) in observers.iter().enumerate() {
            let wanted = if index == 0 { "startup-test-only-first" } else { "startup-test-only-second" };
            let unwanted = if index == 0 { "startup-test-only-second" } else { "startup-test-only-first" };
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let output = read_logs(observer.journal_id());
                if output.contains(&format!("stderr-{wanted}")) {
                    assert!(!output.contains(unwanted), "another launch's output leaked: {output}");
                    break;
                }
                assert!(Instant::now() < deadline, "journal lost the application's output: {output}");
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}
