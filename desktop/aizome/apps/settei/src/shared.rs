//! What the window and every pane share: the settings file, the system
//! services, the window, and the status strip.

use std::path::PathBuf;
use std::time::Duration;

use aizome_config::Settings;
use aizome_kit::{Cx, WindowKey};

use crate::app::AppMsg;
use crate::system::System;

/// How long a status line stays.
const STATUS_FOR: Duration = Duration::from_secs(5);
/// Changes within this of each other ask the compositor to reload once.
const RELOAD_AFTER: Duration = Duration::from_millis(120);

/// A line in the status strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    pub error: bool,
    pub generation: u64,
}

pub struct Shared {
    /// `settings.toml` as last read or written.
    pub settings: Settings,
    /// Where it lives: `aizome_config::Settings::path()`, or a test's.
    pub settings_path: PathBuf,
    /// Fixtures instead of the live system; nothing changes the machine.
    pub sample: bool,
    pub window: WindowKey,
    pub sys: System,
    pub status: Option<Status>,
    status_generation: u64,
    reload_pending: bool,
    writer: Option<Writer>,
    /// Programs launched (in the sample: only recorded), for tests.
    pub launched: Vec<Vec<String>>,
    /// Times the compositor was asked to reload, for tests.
    pub reloads: u32,
}

impl Shared {
    pub fn new(settings_path: PathBuf, sample: bool) -> Shared {
        let settings = Settings::load_from(&settings_path).unwrap_or_else(|e| {
            log::warn!("{}: {e}; showing the defaults", settings_path.display());
            Settings::default()
        });
        Shared {
            settings,
            settings_path,
            sample,
            window: WindowKey::NONE,
            sys: System::empty(sample),
            status: None,
            status_generation: 0,
            reload_pending: false,
            writer: None,
            launched: Vec::new(),
            reloads: 0,
        }
    }

    /// Read the file again (someone else wrote it), unless writes of ours
    /// are still on their way to it.
    /// Read the file again (it changed on disk); whether what it says
    /// changed.
    pub fn reread(&mut self, cx: &mut Cx<AppMsg>) -> bool {
        if self.writer.as_ref().is_some_and(|w| w.pending() > 0) {
            return false;
        }
        if let Ok(s) = Settings::load_from(&self.settings_path)
            && s != self.settings
        {
            self.settings = s;
            cx.set_appearance(aizome_kit::theme::Appearance::from(&self.settings));
            return true;
        }
        false
    }

    /// Apply `f` to the settings as the file has them (read again, so
    /// nobody else's edit is lost), show it at once, and write the file off
    /// the main thread; once it's written the compositor is asked to apply
    /// it. A file that doesn't parse is kept beside the new one.
    pub fn change(&mut self, f: impl FnOnce(&mut Settings), cx: &mut Cx<AppMsg>) {
        // While our own writes are on their way, the newest is ours.
        let pending = self.writer.as_ref().is_some_and(|w| w.pending() > 0);
        let mut fresh = if pending {
            self.settings.clone()
        } else {
            match Settings::load_from(&self.settings_path) {
                Ok(s) => s,
                Err(aizome_config::Error::Parse(e)) => {
                    let keep = self.settings_path.with_extension("toml.broken");
                    let _ = std::fs::copy(&self.settings_path, &keep);
                    self.say(format!("settings.toml didn't parse ({e}); kept it as {}", keep.display()), true, cx);
                    self.settings.clone()
                }
                Err(_) => self.settings.clone(),
            }
        };
        f(&mut fresh);
        let text = match fresh.to_toml() {
            Ok(t) => t,
            Err(e) => {
                self.say(format!("Couldn't write the settings: {e}"), true, cx);
                return;
            }
        };
        self.settings = fresh;
        // This window first, without waiting for the file's watcher.
        cx.set_appearance(aizome_kit::theme::Appearance::from(&self.settings));
        let writer = self.writer.get_or_insert_with(|| Writer::start(cx.stream()));
        writer.sent += 1;
        let _ = writer.tx.send((self.settings_path.clone(), text));
    }

    /// A write reached the disk: ask the compositor to apply the file, once
    /// for a burst of changes (a slider dragged).
    pub fn written(&mut self, cx: &mut Cx<AppMsg>) {
        if !self.reload_pending {
            self.reload_pending = true;
            cx.after(RELOAD_AFTER, AppMsg::Reload);
        }
    }

    /// Wait until every change is in the file (tests; quitting).
    pub fn flush(&self) {
        if let Some(w) = &self.writer {
            w.wait();
        }
    }

    /// Ask the compositor to apply the file (`reload-config`): the shell
    /// and every app follow its `config-changed`.
    pub fn reload(&mut self, cx: &mut Cx<AppMsg>) {
        self.reload_pending = false;
        self.reloads += 1;
        if self.sample || aizome_ipc::socket_path().is_none() {
            return;
        }
        cx.spawn(|| {
            let r = aizome_ipc::Client::connect().and_then(|mut c| c.request(&aizome_ipc::Request::ReloadConfig));
            match r {
                Ok(aizome_ipc::Response::Error { message }) => {
                    AppMsg::Said(format!("The compositor refused the settings: {message}"), true)
                }
                Ok(_) => AppMsg::Nothing,
                // No compositor (another desktop): the file is enough.
                Err(e) => {
                    log::info!("reload-config: {e}");
                    AppMsg::Nothing
                }
            }
        });
    }

    /// Show `text` in the status strip for a few seconds.
    pub fn say(&mut self, text: String, error: bool, cx: &mut Cx<AppMsg>) {
        self.status_generation += 1;
        let g = self.status_generation;
        if error {
            log::warn!("{text}");
        }
        self.status = Some(Status { text, error, generation: g });
        cx.after(if error { STATUS_FOR * 2 } else { STATUS_FOR }, AppMsg::ClearStatus(g));
    }

    pub fn clear_status(&mut self, generation: u64) {
        if self.status.as_ref().is_some_and(|s| s.generation == generation) {
            self.status = None;
        }
    }

    /// Start a program, detached; in the sample only say so.
    pub fn launch(&mut self, argv: Vec<String>, cx: &mut Cx<AppMsg>) {
        let Some(program) = argv.first().cloned() else { return };
        self.launched.push(argv.clone());
        if self.sample {
            self.say(format!("Would open {}", argv.join(" ")), false, cx);
            return;
        }
        let spawned = std::process::Command::new(&program).args(&argv[1..]).stdin(std::process::Stdio::null()).spawn();
        match spawned {
            Ok(mut child) => {
                // Reap it when it exits.
                let _ = std::thread::Builder::new().name("settei-reap".into()).spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => self.say(format!("Couldn't open {program}: {e}"), true, cx),
        }
    }
}
/// Writes `settings.toml` off the main thread, in order: replacing a file
/// can wait on the disk (ext4 flushes a file renamed over another), which
/// a switch's frame shouldn't.
pub struct Writer {
    tx: std::sync::mpsc::Sender<(PathBuf, String)>,
    /// Writes finished, and a way to wait for them.
    done: std::sync::Arc<(std::sync::Mutex<u64>, std::sync::Condvar)>,
    sent: u64,
}

impl Writer {
    fn start(report: aizome_kit::MsgSender<AppMsg>) -> Writer {
        let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, String)>();
        let done = std::sync::Arc::new((std::sync::Mutex::new(0u64), std::sync::Condvar::new()));
        let d2 = done.clone();
        let _ = std::thread::Builder::new().name("settei-settings".into()).spawn(move || {
            for (path, text) in rx {
                let written = write_atomically(&path, &text);
                {
                    let mut n = d2.0.lock().unwrap_or_else(|e| e.into_inner());
                    *n += 1;
                    d2.1.notify_all();
                }
                let _ = report.send(match written {
                    Ok(()) => AppMsg::Written,
                    Err(e) => AppMsg::Said(format!("Couldn't save {}: {e}", path.display()), true),
                });
            }
        });
        Writer { tx, done, sent: 0 }
    }

    fn finished(&self) -> u64 {
        *self.done.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes not on the disk yet.
    pub fn pending(&self) -> u64 {
        self.sent.saturating_sub(self.finished())
    }

    /// Wait until every write sent is on the disk (tests; quitting).
    pub fn wait(&self) {
        let mut n = self.done.0.lock().unwrap_or_else(|e| e.into_inner());
        while *n < self.sent {
            n = self.done.1.wait(n).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// Write `text` to `path` through a temporary file beside it.
pub fn write_atomically(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}
