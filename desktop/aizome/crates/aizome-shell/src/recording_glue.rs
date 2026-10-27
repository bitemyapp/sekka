//! Screen recordings in the shell: starting one (from the palette, ⌥Print,
//! the Capture menu or `aizome-shell msg record`), the picker, the
//! systray's indicator and its tick, stopping, and the notification with
//! the file. What to run and how is `recording`'s.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_ipc::{Rect as IpcRect, Request};
use aizome_kit::Cx;
use aizome_kit::layout::Rect;

use crate::actions::Action;
use crate::menus::model::{Entry, Menu as ShellMenu, sc};
use crate::recording::run::{self, Ending, Event, Timing};
use crate::recording::{Ask, Audio, Capture, Live, PICK_TAG, Phase, Plan, Target};
use crate::shell::{Msg, Shell};

impl Shell {
    /// Start a recording, unless one is going (one at a time).
    pub fn record(&mut self, ask: Ask, cx: &mut Cx<Msg>) {
        if let Some(r) = &self.recording {
            log::info!("a recording is already {:?}; not starting another", r.phase);
            return;
        }
        match &ask.target {
            Target::Pick => {
                self.record_gen += 1;
                self.recording = Some(Live {
                    ask: ask.clone(),
                    phase: Phase::Picking,
                    path: None,
                    recorder: None,
                    since: None,
                    handle: None,
                    serial: self.record_gen,
                    tick: 0,
                });
                self.ipc.send(Request::PickArea { tag: PICK_TAG.into(), prompt: None, targets: Vec::new() });
            }
            Target::Screen => {
                let output = self.focused_screen().map(|s| s.name());
                match output {
                    Some(o) => self.start_recording(ask, Capture::Output(o), cx),
                    None => log::warn!("recording: no display"),
                }
            }
            Target::Output(o) => {
                let o = o.clone();
                self.start_recording(ask, Capture::Output(o), cx);
            }
            Target::Area(r) => match crate::recording::capture_for(*r, None, &self.wm.outputs) {
                Some(c) => self.start_recording(ask, c, cx),
                None => log::warn!("recording: {r:?} is on no display"),
            },
        }
    }

    /// The picker closed: record what was picked (nothing: Esc).
    pub fn area_picked(&mut self, tag: &str, area: Option<IpcRect>, output: Option<&str>, cx: &mut Cx<Msg>) {
        if tag != PICK_TAG || !self.recording.as_ref().is_some_and(|r| r.phase == Phase::Picking) {
            return;
        }
        let ask = self.recording.take().map(|r| r.ask).expect("picking");
        if let Some(c) = area.and_then(|a| crate::recording::capture_for(a, output, &self.wm.outputs)) {
            self.start_recording(ask, c, cx);
        }
    }

    fn start_recording(&mut self, ask: Ask, capture: Capture, cx: &mut Cx<Msg>) {
        let settings = self.settings.recording_settings();
        let audio = ask.audio.unwrap_or(Audio { desktop: settings.desktop_audio, microphone: settings.microphone });
        let home = aizome_config::home_dir();
        let user_dirs = std::fs::read_to_string(aizome_config::config_dir().join("user-dirs.dirs")).ok();
        let env_videos = std::env::var("XDG_VIDEOS_DIR").ok();
        let dir = crate::recording::folder(&settings, env_videos.as_deref(), user_dirs.as_deref(), &home);
        // (Tests leave the disk alone.)
        if let Err(e) = if self.opts.live { std::fs::create_dir_all(&dir) } else { Ok(()) } {
            self.recording_note("Screen recording failed", &format!("Can't make {}: {e}", dir.display()), None, cx);
            return;
        }
        let path = crate::recording::file_in(&dir, crate::clock::Local::now(), settings.format);
        let plan = Plan { capture, audio, path: path.clone(), settings };
        self.record_gen += 1;
        let serial = self.record_gen;
        let mut live = Live {
            ask,
            phase: Phase::Starting,
            path: Some(path.clone()),
            recorder: None,
            since: None,
            handle: None,
            serial,
            tick: 0,
        };
        if self.opts.live {
            // What can record here, once the probe the shell started has
            // said (a recording asked for sooner waits for it, uncounted).
            let probes = self.recorders.clone();
            probes.start();
            let attempts = move || {
                let found = probes.found()?;
                let attempts = crate::recording::attempts(&plan, crate::recording::on_path, Some(&found));
                log::info!(
                    "recording {:?} to {} with {:?}",
                    plan.capture,
                    plan.path.display(),
                    attempts.iter().map(|a| a.recorder).collect::<Vec<_>>()
                );
                Some(attempts)
            };
            let tx = self.sender();
            live.handle = Some(run::start(
                attempts,
                path,
                Some(run::note_path()),
                run::log_path(),
                Timing::default(),
                move |e| {
                    if let Some(tx) = &tx {
                        tx.send(Msg::Recording(serial, e));
                    }
                },
            ));
            self.recording = Some(live);
        } else {
            // Tests: no recorder, as if one started.
            self.recording = Some(live);
            self.recording_event(serial, Event::Started { recorder: "test".into(), pid: 0 }, cx);
        }
    }

    /// Stop the recording going (or give up picking).
    pub fn stop_recording(&mut self, cx: &mut Cx<Msg>) {
        let Some(r) = self.recording.as_mut() else { return };
        match r.phase {
            Phase::Picking => self.recording = None,
            Phase::Stopping => {}
            Phase::Starting | Phase::Recording => {
                r.phase = Phase::Stopping;
                match &r.handle {
                    Some(h) => h.stop(),
                    None => {
                        // Tests: done at once.
                        let (serial, path) = (r.serial, r.path.clone().unwrap_or_default());
                        self.recording_event(serial, Event::Ended(Ending::Saved(path)), cx);
                    }
                }
            }
        }
    }

    /// ⌥Print: stop the recording going, else pick what to record.
    pub fn toggle_recording(&mut self, cx: &mut Cx<Msg>) {
        if self.recording.is_some() {
            self.stop_recording(cx);
        } else {
            self.record(Ask { target: Target::Pick, audio: None }, cx);
        }
    }

    /// What the recording's thread says.
    pub fn recording_event(&mut self, serial: u64, e: Event, cx: &mut Cx<Msg>) {
        let Some(r) = self.recording.as_mut().filter(|r| r.serial == serial) else { return };
        match e {
            Event::Started { recorder, pid } => {
                log::info!("recording with {recorder} (pid {pid})");
                r.recorder = Some(recorder);
                r.since = Some(Instant::now());
                if r.phase == Phase::Starting {
                    r.phase = Phase::Recording;
                }
                self.arm_record_tick(cx);
            }
            Event::GaveUp { recorder } => {
                // Found able, yet it couldn't: the next recording starts
                // with the one that could.
                self.recorders.strike(&recorder);
                r.recorder = None;
                r.since = None;
            }
            Event::Ended(ending) => {
                self.recording = None;
                self.recording_ended(ending, cx);
            }
        }
    }

    /// The indicator's time turns over each second of the recording.
    fn arm_record_tick(&mut self, cx: &mut Cx<Msg>) {
        let Some(r) = self.recording.as_mut() else { return };
        let Some(since) = r.since else { return };
        r.tick += 1;
        let into = since.elapsed().subsec_nanos() as u64;
        let delay = Duration::from_nanos(1_000_000_000 - into) + Duration::from_millis(2);
        cx.after(delay, Msg::RecordTick(r.serial, r.tick));
    }

    pub fn record_tick(&mut self, serial: u64, tick: u64, cx: &mut Cx<Msg>) {
        if self.recording.as_ref().is_some_and(|r| r.serial == serial && r.tick == tick && r.phase != Phase::Stopping) {
            self.arm_record_tick(cx);
        }
    }

    /// The bar's indicator, while recording.
    pub fn record_badge(&self) -> Option<crate::menubar::RecordBadge> {
        let r = self.recording.as_ref()?;
        match r.phase {
            Phase::Picking => None,
            Phase::Starting | Phase::Recording => Some(crate::menubar::RecordBadge {
                label: crate::recording::elapsed_label(r.since.map(|s| s.elapsed()).unwrap_or_default()),
                stopping: false,
            }),
            Phase::Stopping => Some(crate::menubar::RecordBadge { label: "Saving".into(), stopping: true }),
        }
    }

    /// The notification for a recording that ended, with the file.
    fn recording_ended(&mut self, ending: Ending, cx: &mut Cx<Msg>) {
        match ending {
            Ending::Saved(path) => {
                log::info!("recording saved: {}", path.display());
                let name = file_name(&path);
                self.recording_note("Screen recording saved", &name, Some(path), cx);
            }
            Ending::Incomplete(path) => {
                let body = format!("The recorder had to be stopped, so it may not play: {}", file_name(&path));
                self.recording_note("Screen recording stopped", &body, Some(path), cx);
            }
            Ending::Crashed { path, error } => {
                log::warn!("recording: {error}");
                let kept = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
                let body = if kept { format!("{error} What it recorded: {}", file_name(&path)) } else { error };
                self.recording_note("Screen recording stopped", &body, kept.then_some(path), cx);
            }
            Ending::Failed(error) => {
                log::warn!("recording: {error}");
                self.recording_note("Screen recording failed", &error, None, cx);
            }
            Ending::Cancelled => {}
        }
    }

    fn recording_note(&mut self, summary: &str, body: &str, file: Option<PathBuf>, cx: &mut Cx<Msg>) {
        let n = crate::notifications::store::Incoming {
            app_name: "Screen Recording".into(),
            summary: summary.into(),
            body: body.into(),
            category: Some(if file.is_some() { "transfer.complete" } else { "transfer.error" }.into()),
            ..Default::default()
        };
        let actions = match file {
            Some(path) => vec![
                ("default".into(), "Open".into(), Action::Open { path: path.clone() }),
                ("reveal".into(), "Show in Files".into(), Action::Reveal { path: path.clone() }),
                ("open".into(), "Open".into(), Action::Open { path }),
            ],
            None => Vec::new(),
        };
        self.shell_note(n, actions, cx);
    }

    /// A recording an earlier shell started (this one restarted): its
    /// indicator again, and Stop.
    pub fn adopt_recording(&mut self) {
        let note = run::note_path();
        let Some(n) = run::running(&note) else { return };
        log::info!("taking back the recording {} by {} (pid {})", n.path.display(), n.recorder, n.pid);
        self.record_gen += 1;
        let serial = self.record_gen;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let since = Instant::now().checked_sub(Duration::from_secs(now.saturating_sub(n.since)));
        let tx = self.sender();
        let path = n.path.clone();
        let recorder = n.recorder.clone();
        let handle = run::adopt(n, note, run::log_path(), Timing::default(), move |e| {
            if let Some(tx) = &tx {
                tx.send(Msg::Recording(serial, e));
            }
        });
        self.recording = Some(Live {
            ask: Ask { target: Target::Screen, audio: None },
            phase: Phase::Recording,
            path: Some(path),
            recorder: Some(recorder),
            since: since.or(Some(Instant::now())),
            handle: Some(handle),
            serial,
            tick: 0,
        });
    }

    /// The recording, for `aizome-shell msg state`.
    pub fn recording_json(&self) -> serde_json::Value {
        match &self.recording {
            None => serde_json::Value::Null,
            Some(r) => serde_json::json!({
                "phase": format!("{:?}", r.phase).to_lowercase(),
                "recorder": r.recorder,
                "path": r.path,
                "seconds": r.since.map(|s| s.elapsed().as_secs()),
            }),
        }
    }

    /// The Capture menu (❖⇧5, as ⌘⇧5 on a Mac): screenshots, recordings,
    /// and Stop Recording while one goes.
    pub fn capture_menu(&self) -> ShellMenu {
        let screenshot = |a: &str| Action::Wm(Request::Action { action: a.into() });
        let mut items = Vec::new();
        if self.recording.as_ref().is_some_and(|r| r.phase != Phase::Picking) {
            items.push(
                Entry::action("Stop Recording", Action::StopRecording).glyph(aizome_kit::Icon::Square).detail("⌥Print"),
            );
            items.push(Entry::separator());
        }
        items.push(
            Entry::action("Screenshot of the Screen", screenshot("screenshot-screen"))
                .glyph(aizome_kit::Icon::Monitor)
                .shortcut(sc("⇧❖", "3")),
        );
        items.push(
            Entry::action("Screenshot of an Area", screenshot("screenshot-area"))
                .glyph(aizome_kit::Icon::Scan)
                .shortcut(sc("⇧❖", "4")),
        );
        items.push(
            Entry::action("Screenshot of the Window", screenshot("screenshot-window"))
                .glyph(aizome_kit::Icon::AppWindow),
        );
        items.push(Entry::separator());
        let idle = self.recording.is_none();
        let audio_choices = |target: Target| {
            [
                ("Without Audio", Audio { desktop: false, microphone: false }),
                ("With Desktop Audio", Audio { desktop: true, microphone: false }),
                ("With Microphone", Audio { desktop: false, microphone: true }),
                ("With Desktop Audio and Microphone", Audio { desktop: true, microphone: true }),
            ]
            .into_iter()
            .map(|(label, audio)| {
                Entry::action(label, Action::Record(Ask { target: target.clone(), audio: Some(audio) })).enabled(idle)
            })
            .collect::<Vec<_>>()
        };
        let mut screen =
            Entry::submenu("Record the Screen", audio_choices(Target::Screen)).glyph(aizome_kit::Icon::Video);
        screen.enabled = idle;
        let mut area = Entry::submenu("Record an Area", audio_choices(Target::Pick))
            .glyph(aizome_kit::Icon::Scan)
            .detail("⌥Print");
        area.enabled = idle;
        items.push(screen);
        items.push(area);
        items.push(Entry::separator());
        let settings = self.settings.recording_settings();
        let home = aizome_config::home_dir();
        let user_dirs = std::fs::read_to_string(aizome_config::config_dir().join("user-dirs.dirs")).ok();
        let env_videos = std::env::var("XDG_VIDEOS_DIR").ok();
        let folder = crate::recording::folder(&settings, env_videos.as_deref(), user_dirs.as_deref(), &home);
        items.push(Entry::action("Open Recordings", Action::Open { path: folder }).glyph(aizome_kit::Icon::FolderOpen));
        ShellMenu::new("Capture", items)
    }

    /// Show the Capture menu under the focused display's bar, in its
    /// middle (a key opened it: there's no pointer to put it at).
    pub fn open_capture_menu(&mut self, output: Option<&str>, cx: &mut Cx<Msg>) {
        let screen = output.and_then(|n| self.screen_named(n)).or(self.focused_screen()).cloned();
        let Some(screen) = screen else { return };
        let Some(bar) = screen.bar else { return };
        let menu = self.capture_menu();
        let x = (screen.output.frame.width() / 2.0 - 120.0).max(0.0);
        let at = Rect::new(x, crate::menubar::HEIGHT - 1.0, x + 1.0, crate::menubar::HEIGHT);
        let style = aizome_kit::MenuStyle::below().titled("Capture");
        self.show_menu(bar, at, &menu, style, cx);
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
