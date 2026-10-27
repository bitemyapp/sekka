//! StatusNotifierItem + DBusMenu, consumed directly by Aizome's systray.
//! Menu actions use the same serialized daemon protocol as keyboard shortcuts.
use crate::{
    desktop, ipc,
    protocol::{Action, Phase, Request, Status},
};
use ksni::{TrayMethods, menu::StandardItem};
use std::{path::PathBuf, time::Duration};
use tokio::sync::{mpsc, watch};

/// What a menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Daemon(Action),
    /// Koe's window, on a page (`--page`).
    Open(&'static str),
    /// Turn dictation off: the setting, then this daemon.
    TurnOff,
}

struct Tray {
    status: Status,
    actions: mpsc::Sender<Item>,
    error: Option<String>,
}

impl Tray {
    fn label(&self) -> &'static str {
        if self.status.teaching {
            return match self.status.phase {
                Phase::Recording => "Recording teaching example",
                Phase::Transcribing => "Checking teaching example",
                Phase::Idle => "Review example — microphone off",
                _ => "Teaching — needs attention",
            };
        }
        match self.status.phase {
            Phase::Idle => "Ready",
            Phase::Recording => "Listening",
            Phase::Paused => "Paused",
            Phase::Finishing => "Stopping microphone",
            Phase::Transcribing => "Transcribing",
            Phase::Delivering => "Pasting",
            Phase::Error => "Microphone off — needs attention",
        }
    }
    fn item(&self, label: &str, item: Item, enabled: bool) -> ksni::MenuItem<Self> {
        StandardItem {
            label: label.into(),
            enabled,
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.actions.try_send(item);
            }),
            ..Default::default()
        }
        .into()
    }
}

impl ksni::Tray for Tray {
    // Opening the tray is never an implicit request to open the microphone.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "org.aizome.Koe".into()
    }
    fn title(&self) -> String {
        format!("Koe — {}", self.label())
    }
    fn status(&self) -> ksni::Status {
        if self.status.phase == Phase::Error || self.error.is_some() {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![icon(self.status.phase)]
    }
    fn attention_icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![icon(Phase::Error)]
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let mut description =
            self.error.as_ref().or(self.status.error.as_ref()).cloned().unwrap_or_else(|| match self.status.phase {
                Phase::Recording => "Microphone on. Stop dictation to transcribe.".into(),
                Phase::Paused => "Microphone off. Resume or stop dictation.".into(),
                Phase::Finishing => "Stopping microphone. You can cancel from the menu.".into(),
                Phase::Transcribing | Phase::Delivering => {
                    "Microphone off. Finishing dictation. You can cancel from the menu.".into()
                }
                _ => "Your dictation shortcut starts and stops it. Click for controls.".into(),
            });
        if self.status.phase != Phase::Recording && (self.error.is_some() || self.status.error.is_some()) {
            description = format!("Microphone off. {description}");
        }
        ksni::ToolTip { title: self.title(), description, ..Default::default() }
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let phase = self.status.phase;
        let idle = matches!(phase, Phase::Idle | Phase::Error);
        let recording = matches!(phase, Phase::Recording | Phase::Paused);
        vec![
            StandardItem { label: self.title(), enabled: false, ..Default::default() }.into(),
            ksni::MenuItem::Separator,
            self.item(
                if recording { "Stop Dictation" } else { "Start Dictation" },
                Item::Daemon(Action::Toggle),
                !self.status.teaching && (idle || recording),
            ),
            self.item(
                if phase == Phase::Paused { "Resume Dictation" } else { "Pause Dictation" },
                Item::Daemon(if phase == Phase::Paused { Action::Resume } else { Action::Pause }),
                recording && !self.status.teaching,
            ),
            self.item(
                if self.status.teaching {
                    "Discard Teaching Example"
                } else if phase == Phase::Error {
                    "Dismiss Error"
                } else {
                    "Cancel Dictation"
                },
                Item::Daemon(Action::Cancel),
                phase != Phase::Idle || self.status.teaching,
            ),
            ksni::MenuItem::Separator,
            self.item(
                "Paste Last Transcript",
                Item::Daemon(Action::PasteLast),
                self.status.has_last && idle && !self.status.teaching,
            ),
            self.item("Copy Last Transcript", Item::Daemon(Action::CopyLast), self.status.has_last),
            self.item("Forget Last Transcript", Item::Daemon(Action::ClearLast), self.status.has_last),
            self.item("Unload Model", Item::Daemon(Action::Unload), idle && self.status.model_loaded),
            ksni::MenuItem::Separator,
            self.item("Vocabulary and Teaching…", Item::Open("vocabulary"), true),
            self.item("Open Koe…", Item::Open("dictation"), true),
            ksni::MenuItem::Separator,
            self.item("Turn Off Dictation", Item::TurnOff, true),
        ]
    }
}

// Embed a small, crisp microphone as SNI's network-order ARGB pixels, so tray
// hosts do not need an installed icon theme. Recording is red, busy amber dots,
// paused blue, errors an amber warning triangle, and idle cream.
fn icon(phase: Phase) -> ksni::Icon {
    let color = match phase {
        Phase::Recording => [240, 90, 90],
        Phase::Paused => [117, 177, 229],
        Phase::Finishing | Phase::Transcribing | Phase::Delivering => [232, 177, 91],
        Phase::Error => [232, 177, 91],
        Phase::Idle => [242, 237, 227],
    };
    let mut argb = vec![0; 32 * 32 * 4];
    for y in 0i32..32 {
        for x in 0i32..32 {
            let capsule = (12..20).contains(&x)
                && (6..18).contains(&y)
                && ((9..15).contains(&y) || (x - 16).pow(2) + (y - if y < 9 { 9 } else { 14 }).pow(2) <= 16);
            let ring = (8..24).contains(&x)
                && (14..24).contains(&y)
                && (49..=64).contains(&((x - 16).pow(2) + (y - 15).pow(2)));
            let stem = (15..18).contains(&x) && (22..27).contains(&y);
            let foot = (11..22).contains(&x) && (26..28).contains(&y);
            let warning =
                (5..28).contains(&y) && ((y - 5 - 2 * (x - 16).abs()).abs() <= 2 || y >= 25 && (4..29).contains(&x));
            let exclamation = (15..18).contains(&x) && ((12..20).contains(&y) || (22..24).contains(&y));
            let dots = [6, 16, 26].iter().any(|cx| (x - cx).pow(2) + (y - 16).pow(2) <= 9);
            let marked = match phase {
                Phase::Error => warning || exclamation,
                Phase::Finishing | Phase::Transcribing | Phase::Delivering => dots,
                _ => capsule || ring || stem || foot,
            };
            if marked {
                let offset = ((y * 32 + x) * 4) as usize;
                argb[offset..offset + 4].copy_from_slice(&[255, color[0], color[1], color[2]]);
            }
        }
    }
    ksni::Icon { width: 32, height: 32, data: argb }
}

/// Show the tray until the daemon's status stream closes. `settings` is
/// the daemon's configuration file, for the window it opens and for
/// Turn Off Dictation.
pub async fn run(path: PathBuf, settings: Option<PathBuf>, mut updates: watch::Receiver<Status>, notify: bool) {
    let (actions, mut commands) = mpsc::channel(8);
    let mut previous = updates.borrow_and_update().clone();
    let tray = Tray { status: previous.clone(), actions, error: None };
    // Keep the service alive if the shell starts later or restarts.
    let handle = match tray.assume_sni_available(true).spawn().await {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("Koe tray unavailable: {error}; dictation shortcuts remain available");
            return;
        }
    };
    loop {
        tokio::select! {
            result = updates.changed() => {
                if result.is_err() { break; }
                let status = updates.borrow_and_update().clone();
                // Meter updates are 10 Hz; SNI only needs state changes.
                if previous.phase == status.phase && previous.has_last == status.has_last
                    && previous.model_loaded == status.model_loaded && previous.error == status.error
                    && previous.teaching == status.teaching {
                    continue;
                }
                previous = status.clone();
                handle.update(|tray| {
                    if tray.status.phase != status.phase { tray.error = None; }
                    tray.status = status;
                }).await;
            }
            Some(item) = commands.recv() => {
                // Let the shell dismiss the menu and restore application focus.
                tokio::time::sleep(Duration::from_millis(200)).await;
                let error = match item {
                    Item::Open(page) => {
                        let mut args = vec![];
                        if let Some(settings) = &settings {
                            args.extend(["--config".into(), settings.display().to_string()]);
                        }
                        args.extend(["--socket".into(), path.display().to_string(), "--page".into(), page.into()]);
                        desktop::open_window(args).await.err().map(|e| format!("Cannot open Koe: {e:#}"))
                    }
                    Item::TurnOff => {
                        let saved = match &settings {
                            Some(file) => tokio::task::spawn_blocking({
                                let file = file.clone();
                                move || crate::service::set_enabled(&file, false)
                            }).await.map_err(anyhow::Error::from).and_then(|r| r).err(),
                            None => None,
                        };
                        // Stopping the microphone doesn't depend on
                        // persisting the choice for the next login.
                        request(&path, Action::Shutdown).await.or_else(|| saved.map(|error| {
                            format!("Dictation's service has stopped, but Koe couldn't save the off setting. \
                                     It may start again at login. {error:#}")
                        }))
                    }
                    Item::Daemon(action) => request(&path, action).await,
                };
                if let Some(ref message) = error {
                    eprintln!("Koe tray: {message}");
                    if notify { desktop::notify("Koe needs attention", message.clone()); }
                }
                handle.update(|tray| tray.error = error).await;
            }
        }
    }
    handle.shutdown().await;
}

/// Ask the daemon; what went wrong, if anything.
async fn request(path: &std::path::Path, action: Action) -> Option<String> {
    match ipc::request(path, &Request::new(action)).await {
        Ok(response) if response.ok => None,
        Ok(response) => response.error,
        Err(error) => Some(format!("{error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::Tray as _;

    #[tokio::test]
    async fn tray_tracks_phase_and_routes_controls_without_transcript_text() {
        let (actions, mut commands) = mpsc::channel(8);
        let mut tray = Tray { status: Status::default(), actions, error: None };
        assert_eq!(tray.title(), "Koe — Ready");
        let mut menu = tray.menu();
        let ksni::MenuItem::Standard(start) = &mut menu[2] else { panic!("start missing") };
        assert!(start.enabled);
        (start.activate)(&mut tray);
        assert_eq!(commands.recv().await, Some(Item::Daemon(Action::Toggle)));
        tray.status.phase = Phase::Transcribing;
        assert!(tray.tool_tip().description.contains("Finishing"));
        let menu = tray.menu();
        let ksni::MenuItem::Standard(start) = &menu[2] else { panic!() };
        let ksni::MenuItem::Standard(cancel) = &menu[4] else { panic!() };
        assert!(!start.enabled && cancel.enabled);
        assert_ne!(icon(Phase::Idle).data, icon(Phase::Recording).data);
        assert_eq!(tray.icon_pixmap()[0].data.len(), 32 * 32 * 4);
        tray.status.phase = Phase::Error;
        tray.status.error = Some("focused window changed".into());
        assert!(tray.title().contains("Microphone off"));
        assert!(tray.tool_tip().description.starts_with("Microphone off."));
        let mut menu = tray.menu();
        let ksni::MenuItem::Standard(dismiss) = &mut menu[4] else { panic!() };
        assert!(dismiss.enabled);
        assert_eq!(dismiss.label, "Dismiss Error");
        (dismiss.activate)(&mut tray);
        assert_eq!(commands.recv().await, Some(Item::Daemon(Action::Cancel)));
        // The window and the switch, whatever the phase.
        let mut menu = tray.menu();
        let n = menu.len();
        let ksni::MenuItem::Standard(off) = &mut menu[n - 1] else { panic!() };
        assert_eq!(off.label, "Turn Off Dictation");
        (off.activate)(&mut tray);
        assert_eq!(commands.recv().await, Some(Item::TurnOff));
        // Distinguish shapes, not just colors, at small tray sizes.
        let silhouette = |phase| icon(phase).data.chunks(4).map(|p| p[0]).collect::<Vec<_>>();
        assert_ne!(silhouette(Phase::Recording), silhouette(Phase::Error));
        assert_ne!(silhouette(Phase::Recording), silhouette(Phase::Delivering));
    }
}
