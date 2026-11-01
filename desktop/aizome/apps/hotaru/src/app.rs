use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aizome_kit::prelude::*;
use aizome_kit::{ClipboardRead, PreparedClipboard};
use hotaru_core::{ContentSnapshot, Snapshot, Terminal, TerminalColors};
use hotaru_io::PtyCommand;

use crate::clipboard::{ClipboardWorker, CopySource, Job};
use crate::panes::{Axis, Direction, PaneTree};
use crate::preferences::{Notice, Page};
use crate::session::{Command, Defaults, Session};
use crate::settings::{self, Access, Action, Bindings, Settings};
use crate::terminal_view::{Selection, colors_for, terminal_view};

/// Messages for a window name it, or a pane of it. Menu commands and
/// shortcuts (`Action`) are for the front window, and become messages
/// naming it.
#[derive(Clone)]
pub enum Msg {
    NewWindow,
    NewTab(WindowKey),
    /// A later launch handed over by `instance`; true once the compositor
    /// has been asked what it said (a roundtrip) about the window named.
    Launched(crate::instance::Launch, bool),
    SelectTab(u64),
    CloseTab(WindowKey),
    Split(WindowKey, Axis),
    ClosePane(WindowKey),
    FocusPane(u64),
    RestoreFocus(u64, u64),
    MoveFocus(WindowKey, Direction),
    Geometry(u64, Rect),
    Action(Action),
    SetTheme(settings::Theme),
    SetWeight(u16),
    SetCopyFormat(settings::CopyFormat),
    SetFontSize(f64),
    SetFontFamily(String),
    SetLigatures(bool),
    SetCursorStyle(settings::CursorStyle),
    SetCursorBlink(bool),
    SetScrollback(usize),
    SetShellIntegration(bool),
    SetClipboardRead(settings::Access),
    SetClipboardWrite(settings::Access),
    SetCopyOnSelect(bool),
    SetNotifications(bool),
    SetWaitAfterCommand(bool),
    SettingsPage(Page),
    MonospaceFamilies(Arc<[String]>),
    OpenConfigFile,
    ConfigOpened(Result<(), aizome_kit::urls::OpenUrlError>),
    /// Settings waits for a new shortcut for the action, or stops waiting.
    RecordShortcut(Action),
    StopRecording(Action),
    Recorded(Action, Shortcut),
    RemoveShortcut(Action, Shortcut),
    RestoreShortcuts,
    RestoredShortcuts,
    ConfigLoaded(u64, Result<Settings, String>),
    ConfigSaved(u64, Result<(), String>),
    Frame(u64, Arc<Snapshot>),
    ShapeScale(u64, f64),
    Shaped(u64, Arc<crate::font::Frame>),
    FontFailed(u64, u64, String),
    Title(u64, String),
    WorkingDirectory(u64, std::path::PathBuf),
    /// The program ended: its status, and whether it failed as it started.
    Exited(u64, String, bool),
    Failed(u64, String),
    Notice(u64, String),
    MatchLink(u64, Option<Arc<crate::automatic_links::Request>>),
    MatchedLink,
    ResolvedLink,
    HoverLink(u64, Option<crate::links::Link>),
    OpenLink(u64, crate::links::Link),
    CopyLink(Arc<str>),
    LinkOpened(u64, Result<(), aizome_kit::urls::OpenUrlError>),
    Bell(u64),
    Input(u64, Vec<u8>),
    PasteText(u64, Arc<str>),
    Key(u64, Arc<hotaru_core::input::KeyEvent>),
    Mouse(u64, crate::input::MouseInput),
    Wheel(u64, crate::input::MouseInput, i32),
    TerminalFocus(u64, bool),
    Resize(u64, usize, usize),
    ResizePixels(u64, usize, usize, u16, u16),
    Scroll(u64, i32),
    Select(u64, Selection),
    Copy(WindowKey),
    CopyScreen(WindowKey),
    CopyAll(WindowKey),
    CopySnapshot(u64, u64, Arc<ContentSnapshot>),
    CopyReady(u64, u64, PreparedClipboard),
    CopyFailed(u64, u64, String),
    PasteReady(u64, u64, ClipboardRead, Option<Arc<str>>),
    Paste(WindowKey),
    /// A pointer selection in the pane is finished: with copy on select,
    /// it becomes the primary selection.
    Selected(u64),
    PrimaryReady(u64, PreparedClipboard),
    /// Paste the primary selection, as a middle click does.
    PastePrimary(u64),
    /// A program's own clipboard request (OSC 52 or 5522), from its PTY worker.
    Clipboard(crate::selections::Request),
    /// What a read will be answered with, captured before anyone is asked.
    ClipboardCaptured(crate::selections::Request, Option<Arc<[u8]>>, crate::selections::Preview),
    /// What was chosen about the request being asked about.
    ClipboardAnswer(u64, crate::selections::Answer),
    /// A program's notification (OSC 9 or 777): its title and body.
    Notify(u64, String, String),
    Find(WindowKey),
    Search(WindowKey, String),
    SearchRefresh(WindowKey, u64),
    SearchSnapshot(u64, u64, Arc<ContentSnapshot>),
    SearchReady(u64, u64, u64, crate::search::Cache),
    SearchFailed(u64, u64, String),
    SearchStep(WindowKey, bool),
    SearchNavigated(u64, u64, bool),
    CloseFind(WindowKey),
    FontSize(WindowKey, i8),
    Theme(Arc<TerminalColors>),
}

pub struct Pane {
    pub id: u64,
    /// The window whose tabs hold it.
    pub window: WindowKey,
    pub title: String,
    /// Last bounded local directory reported by this shell via OSC 7.
    pub working_directory: Option<std::path::PathBuf>,
    /// Automatic paths use only an accepted OSC 7 report, not launch guesses.
    link_directory: Option<std::path::PathBuf>,
    pub snapshot: Arc<Snapshot>,
    pub session: Option<Session>,
    pub selection: Option<Selection>,
    pub status: String,
    pub(crate) hovered_link: Option<crate::links::Link>,
    pub ended: bool,
    pub bell: bool,
    pub rect: Rect,
    pub(crate) rendered: Option<Arc<crate::font::Frame>>,
    font_worker: Option<crate::font::Worker>,
    font_epoch: u64,
    input_activity: u64,
    font_scale: f64,
}

impl Pane {
    pub fn displayed_snapshot(&self) -> Option<&Arc<Snapshot>> {
        self.rendered.as_ref().map(|frame| &frame.snapshot)
    }
    pub fn font_metrics(&self) -> Option<aizome_kit::sidestep_ui::grid::Metrics> {
        self.rendered.as_ref().map(|frame| frame.metrics)
    }
    pub fn resolved_font_family(&self) -> Option<&str> {
        self.rendered.as_ref().map(|frame| frame.primary_family.as_str())
    }
}

pub struct Tab {
    pub id: u64,
    pub tree: PaneTree,
    pub active: u64,
    /// The active pane fills the tab, its splits set aside until unzoomed.
    pub zoomed: bool,
}

/// A terminal window: its tabs (their panes are the app's, by id), what
/// it finds and how large its text is.
pub struct TerminalWindow {
    pub key: WindowKey,
    pub tabs: Vec<Tab>,
    pub active: u64,
    /// The size text is drawn at: the configured size, zoomed by ❖+ and ❖−.
    pub font_size: f64,
    pub find: bool,
    pub search: String,
    pub(crate) find_state: crate::search::State,
    /// What New Tab and splits start: what the window was opened with.
    command: PtyCommand,
    focus_timer: Option<TimerHandle>,
    focus_generation: u64,
}

impl TerminalWindow {
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active)
    }
}

/// One process for every window on the display: windows share the
/// settings, workers and font caches, and a later launch opens a window or
/// a tab here (`instance`).
pub struct Hotaru {
    /// The terminal windows, oldest first.
    pub windows: Vec<TerminalWindow>,
    /// The terminal window menu commands go to: the key window, or the
    /// last one that was.
    front: WindowKey,
    pub panes: Vec<Pane>,
    next_id: u64,
    /// What the first window starts.
    command: PtyCommand,
    demo: bool,
    pub instance: Option<crate::instance::Instance>,
    clipboard: Option<ClipboardWorker>,
    clipboard_token: u64,
    clipboard_cancel: Arc<AtomicBool>,
    clipboard_format: settings::CopyFormat,
    clipboard_read: Option<ClipboardRead>,
    colors: Option<Arc<TerminalColors>>,
    pub settings: Settings,
    pub settings_path: Option<std::path::PathBuf>,
    bindings: Arc<Bindings>,
    settings_worker: Option<settings::Worker>,
    settings_token: u64,
    settings_pending: bool,
    closing: bool,
    desktop_appearance: aizome_kit::theme::Appearance,
    automatic_worker: Option<crate::automatic_links::Worker>,
    automatic_request: Option<Arc<crate::automatic_links::Request>>,
    automatic_hit: Option<crate::links::Link>,
    resolver_worker: Option<crate::link_resolver::Worker>,
    resolver_request: Option<Arc<crate::link_resolver::Request>>,
    /// The Settings window, while it's open, and the terminal window it
    /// belongs to.
    pub settings_window: Option<WindowKey>,
    settings_parent: WindowKey,
    pub settings_page: Page,
    pub(crate) settings_notice: Option<Notice>,
    /// The configuration is a symbolic link (a system's): changes aren't saved.
    pub settings_managed: bool,
    /// Monospaced families installed, once looked for.
    pub(crate) monospace: Option<Arc<[String]>>,
    /// The action a shortcut is being typed for in Settings.
    pub recording: Option<Action>,
    open_after_save: bool,
    /// The primary selection and programs' clipboard requests (OSC 52).
    selections: Option<crate::selections::Worker>,
    primary_token: u64,
    primary_cancel: Arc<AtomicBool>,
    /// A program's clipboard request being asked about.
    asking: Option<crate::selections::Question>,
    ask_token: u64,
    /// The pane with the keyboard, while the window has it.
    focused_pane: Option<u64>,
    /// When a program's notification last reached the desktop.
    notified: Option<std::time::Instant>,
}

impl Hotaru {
    pub fn new(command: PtyCommand) -> Self {
        Self {
            windows: Vec::new(),
            front: WindowKey::NONE,
            panes: Vec::new(),
            next_id: 1,
            command,
            demo: false,
            instance: None,
            clipboard: None,
            clipboard_token: 0,
            clipboard_cancel: Arc::new(AtomicBool::new(false)),
            clipboard_format: settings::CopyFormat::Mixed,
            clipboard_read: None,
            colors: None,
            settings: Settings::default(),
            settings_path: None,
            bindings: Arc::new(Bindings::new(&Settings::default())),
            settings_worker: None,
            settings_token: 0,
            settings_pending: false,
            closing: false,
            desktop_appearance: aizome_kit::theme::Appearance::default(),
            automatic_worker: None,
            automatic_request: None,
            automatic_hit: None,
            resolver_worker: None,
            resolver_request: None,
            settings_window: None,
            settings_parent: WindowKey::NONE,
            settings_page: Page::default(),
            settings_notice: None,
            settings_managed: false,
            monospace: None,
            recording: None,
            open_after_save: false,
            selections: None,
            primary_token: 0,
            primary_cancel: Arc::new(AtomicBool::new(false)),
            asking: None,
            ask_token: 0,
            focused_pane: None,
            notified: None,
        }
    }

    /// A deterministic renderer fixture: never starts a subprocess.
    pub fn preview() -> Self {
        let mut app = Self::new(PtyCommand::shell());
        app.demo = true;
        app
    }

    pub fn window(&self, key: WindowKey) -> Option<&TerminalWindow> {
        self.windows.iter().find(|w| w.key == key)
    }

    fn window_mut(&mut self, key: WindowKey) -> Option<&mut TerminalWindow> {
        self.windows.iter_mut().find(|w| w.key == key)
    }

    /// The terminal window menu commands go to.
    pub fn front(&self) -> Option<&TerminalWindow> {
        self.window(self.front)
    }

    pub fn front_mut(&mut self) -> Option<&mut TerminalWindow> {
        self.window_mut(self.front)
    }

    /// The front window's tab in front.
    pub fn active_tab(&self) -> Option<&Tab> {
        self.front()?.active_tab()
    }

    /// The front window's pane with the keyboard.
    pub fn active_pane(&self) -> Option<&Pane> {
        self.pane_in(self.front)
    }

    /// Window `w`'s pane with the keyboard, in its tab in front.
    fn pane_in(&self, w: WindowKey) -> Option<&Pane> {
        let pane = self.window(w)?.active_tab()?.active;
        self.panes.iter().find(|p| p.id == pane)
    }

    /// The window whose tabs hold pane `id`.
    fn owner(&self, id: u64) -> Option<WindowKey> {
        self.panes.iter().find(|pane| pane.id == id).map(|pane| pane.window)
    }

    /// Where a command goes: the key window if it's a terminal window,
    /// else the one that last was (as when Settings has the keyboard).
    fn front_for(&mut self, cx: &mut Cx<Msg>) -> WindowKey {
        if let Some(key) = cx.key_window()
            && self.window(key).is_some()
        {
            self.front = key;
        }
        if self.window(self.front).is_none() {
            self.front = self.windows.last().map_or(WindowKey::NONE, |w| w.key);
        }
        self.front
    }

    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// The desktop's appearance, whatever theme Hotaru keeps to.
    pub fn desktop_appearance(&self) -> aizome_kit::theme::Appearance {
        self.desktop_appearance
    }

    /// The families Settings offers: those found, with the configured one
    /// and Aizome's own even before they are (or if the search can't see
    /// fonts installed for this process only).
    pub fn monospace_families(&self) -> Arc<[String]> {
        let mut families: Vec<String> = self.monospace.as_deref().unwrap_or_default().to_vec();
        for name in [self.settings.primary_family(), "IBM Plex Mono"] {
            if !families.iter().any(|family| family.eq_ignore_ascii_case(name)) {
                families.push(name.to_owned());
            }
        }
        families.sort_by_key(|family| family.to_lowercase());
        families.into()
    }

    fn focus_active(&self, w: WindowKey, cx: &mut Cx<Msg>) {
        if let Some(pane) = self.pane_in(w) {
            cx.focus_in(w, &format!("terminal-{}", pane.id));
            cx.set_title(w, format!("{} — Hotaru", pane.title));
        }
    }

    fn focus_after_rebuild(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        self.focus_active(w, cx);
        let pane = self.pane_in(w).map(|pane| pane.id);
        let Some(window) = self.window_mut(w) else { return };
        if let Some(timer) = window.focus_timer.take() {
            cx.cancel(timer);
        }
        window.focus_generation = window.focus_generation.wrapping_add(1);
        if let Some(pane) = pane {
            // A split edit reparents a still-named leaf. An immediate focus can
            // succeed on its old node, which is removed during the next build.
            window.focus_timer =
                Some(cx.after(std::time::Duration::ZERO, Msg::RestoreFocus(pane, window.focus_generation)));
        }
    }

    /// The panes in window `w`'s tabs.
    fn pane_count(&self, w: WindowKey) -> usize {
        self.panes.iter().filter(|pane| pane.window == w).count()
    }

    /// A pane for window `w` running `command`, else what the window runs;
    /// with `inherit_cwd`, in the folder of its pane with the keyboard.
    fn new_pane(&mut self, w: WindowKey, cx: &mut Cx<Msg>, inherit_cwd: bool, command: Option<PtyCommand>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut terminal = Terminal::new(80, 24, self.settings.scrollback);
        terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
        let cursor = (self.settings.cursor_style.shape(), self.settings.cursor_blink);
        terminal.set_default_cursor(cursor.0, cursor.1);
        let colors = colors_for(cx.theme());
        terminal.set_default_colors(colors.clone());
        if self.demo {
            terminal.feed(b"\x1b[1;36mHotaru\x1b[0m  \xe8\x9b\x8d\r\n\r\nA little light for your work.\r\n\r\n\x1b[32mcallen\x1b[0m \x1b[34m~/work/sekka\x1b[0m\r\n\xe2\x9d\xaf cargo test\r\n\x1b[32m   Compiling\x1b[0m hotaru-core\r\n\x1b[32m    Finished\x1b[0m test profile\r\n\r\n\x1b[1mtest result:\x1b[0m ok\r\n\r\n\xe2\x9d\xaf ");
        }
        let mut command =
            command.or_else(|| self.window(w).map(|window| window.command.clone())).unwrap_or_else(PtyCommand::shell);
        // The system's Bash-to-Fish handoff must not replace a shell Hotaru
        // chose. The parent PID scopes this to our own PTYs, not terminals
        // later launched from one of them which inherit the environment.
        command.env.push(("HOTARU_SHELL_PARENT".into(), std::process::id().to_string().into()));
        // The default shell, not a command someone named: the setting's.
        if command.integrate_shell {
            command.program = self.settings.shell_program();
        }
        if inherit_cwd
            && command.integrate_shell
            && let Some(directory) = self.pane_in(w).and_then(|pane| pane.working_directory.clone())
        {
            command.cwd = Some(directory);
        }
        command.integrate_shell &= self.settings.shell_integration;
        let working_directory = command.cwd.clone();
        let (session, status) = if self.demo {
            (None, "Preview".into())
        } else {
            let defaults = Defaults { colors, scrollback: self.settings.scrollback, cursor };
            match Session::spawn(id, command, defaults, cx.stream()) {
                Ok(session) => (Some(session), "Shell".into()),
                Err(e) => (None, format!("Could not start shell: {e}")),
            }
        };
        self.panes.push(Pane {
            id,
            window: w,
            title: "Shell".into(),
            working_directory,
            link_directory: None,
            snapshot: Arc::new(terminal.snapshot()),
            session,
            selection: None,
            status,
            hovered_link: None,
            ended: false,
            bell: false,
            rect: Rect::ZERO,
            rendered: None,
            font_worker: crate::font::Worker::new(id, cx.stream())
                .map_err(|error| log::warn!("Font worker: {error}"))
                .ok(),
            font_epoch: 0,
            input_activity: 0,
            font_scale: 1.0,
        });
        self.shape_pane(id, false);
        id
    }

    /// A tab in window `w`, running `command` else what the window runs.
    fn new_tab(&mut self, w: WindowKey, cx: &mut Cx<Msg>, inherit_cwd: bool, command: Option<PtyCommand>) {
        if self.pane_count(w) >= 64 {
            if let Some(id) = self.pane_in(w).map(|pane| pane.id) {
                self.notice(id, "The 64 pane limit has been reached.");
            }
            return;
        }
        let id = self.new_pane(w, cx, inherit_cwd, command);
        let Some(window) = self.window_mut(w) else { return };
        window.tabs.push(Tab { id, tree: PaneTree::Leaf(id), active: id, zoomed: false });
        window.active = id;
        self.focus_after_rebuild(w, cx);
    }

    /// Open a terminal window, `command` running in its first tab.
    fn open_terminal_window(&mut self, command: PtyCommand, cx: &mut Cx<Msg>) -> WindowKey {
        let key = cx.open_window(WindowSpec::new("Hotaru").size(960.0, 640.0).min_size(320.0, 180.0));
        if key == WindowKey::NONE {
            return key;
        }
        let find_state = crate::search::State {
            worker: crate::search::Worker::new(cx.stream()).map_err(|error| log::warn!("Search worker: {error}")).ok(),
            ..Default::default()
        };
        self.windows.push(TerminalWindow {
            key,
            tabs: Vec::new(),
            active: 0,
            font_size: self.settings.font_size,
            find: false,
            search: String::new(),
            find_state,
            command,
            focus_timer: None,
            focus_generation: 0,
        });
        self.front = key;
        self.new_tab(key, cx, false, None);
        key
    }

    /// New Window: a shell of its own, in the front pane's folder.
    fn new_window(&mut self, cx: &mut Cx<Msg>) {
        let front = self.front_for(cx);
        let mut command = PtyCommand::shell();
        command.cwd = self
            .pane_in(front)
            .and_then(|pane| pane.working_directory.clone())
            .or_else(|| self.window(front).and_then(|window| window.command.cwd.clone()));
        self.open_terminal_window(command, cx);
    }

    /// A later launch (`instance`): a tab in the window it names, if that's
    /// one of ours, else a window of its own.
    fn launch(&mut self, launch: crate::instance::Launch, asked: bool, cx: &mut Cx<Msg>) {
        use crate::instance::Target;
        let named = match &launch.target {
            Target::Window(name) => {
                let found =
                    self.windows.iter().map(|w| w.key).find(|&key| cx.window_identifier(key).as_deref() == Some(name));
                // The compositor told us the window's name before it named
                // the window to the launch: if that isn't heard yet, it is
                // once the compositor has answered what was asked since.
                if found.is_none() && !asked {
                    cx.after_roundtrip(Msg::Launched(launch, true));
                    return;
                }
                found
            }
            Target::NewWindow => None,
        };
        let crate::instance::Launch { command, activation_token, .. } = launch;
        let Some(w) = named else {
            self.open_terminal_window(command, cx);
            return;
        };
        self.new_tab(w, cx, false, Some(command));
        self.front = w;
        // The launcher's token brings a hidden, minimised or covered window
        // forward with the keyboard.
        match activation_token {
            Some(token) => cx.activate_with_token(w, &token),
            None => cx.activate(w),
        }
    }
    /// Window `w`'s tab at `index` (0-based), or its last for `None`.
    fn select_tab_at(&mut self, w: WindowKey, index: Option<usize>, cx: &mut Cx<Msg>) {
        let Some(window) = self.window(w) else { return };
        let tab = match index {
            Some(index) => window.tabs.get(index),
            None => window.tabs.last(),
        };
        if let Some(id) = tab.map(|tab| tab.id) {
            self.update(Msg::SelectTab(id), cx);
        }
    }

    /// Window `w`'s tab `step` places along, round from the last to the first.
    fn step_tab(&mut self, w: WindowKey, step: isize, cx: &mut Cx<Msg>) {
        let Some(window) = self.window(w) else { return };
        let Some(index) = window.tabs.iter().position(|tab| tab.id == window.active) else { return };
        let count = window.tabs.len() as isize;
        self.select_tab_at(w, Some((index as isize + step).rem_euclid(count) as usize), cx);
    }

    fn toggle_zoom(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(window) = self.window_mut(w) else { return };
        let active = window.active;
        let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == active) else { return };
        // A lone pane already fills its tab.
        tab.zoomed = !tab.zoomed && matches!(tab.tree, PaneTree::Split { .. });
        self.focus_after_rebuild(w, cx);
    }

    fn split_pane(&mut self, w: WindowKey, axis: Axis, cx: &mut Cx<Msg>) {
        let Some(target) = self.pane_in(w).map(|pane| pane.id) else { return };
        if self.pane_count(w) >= 64 {
            self.notice(target, "The 64 pane limit has been reached.");
            return;
        }
        let id = self.new_pane(w, cx, true, None);
        let split = self.next_id;
        self.next_id += 1;
        if let Some(window) = self.window_mut(w) {
            let active = window.active;
            if let Some(tab) = window.tabs.iter_mut().find(|tab| tab.id == active) {
                tab.tree.split(target, id, split, axis);
                tab.active = id;
                tab.zoomed = false;
            }
        }
        self.focus_after_rebuild(w, cx);
    }

    fn close_tab(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(window) = self.window(w) else { return };
        let Some(index) = window.tabs.iter().position(|tab| tab.id == window.active) else { return };
        self.clipboard_request();
        self.remove_tab(w, index, cx);
    }

    /// Take away window `w`'s tab at `index`; the window goes with the last.
    fn remove_tab(&mut self, w: WindowKey, index: usize, cx: &mut Cx<Msg>) {
        let Some(window) = self.window_mut(w) else { return };
        let tab = window.tabs.remove(index);
        let shown = tab.id == window.active;
        if shown && !window.tabs.is_empty() {
            window.active = window.tabs[index.min(window.tabs.len() - 1)].id;
        }
        let emptied = window.tabs.is_empty();
        let removed = tab.tree.leaves();
        self.panes.retain(|pane| !removed.contains(&pane.id));
        if emptied {
            self.close_terminal_window(w, cx);
        } else if shown {
            self.focus_after_rebuild(w, cx);
        }
    }

    /// Window `w` has no tab left: it closes, with Settings if it's its
    /// (Settings alone would be a terminal without a terminal). The last
    /// window waits for settings still being saved.
    fn close_terminal_window(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        if self.settings_window.is_some() && self.settings_parent == w {
            self.close_settings(cx);
        }
        if self.settings_pending && self.windows.iter().all(|window| window.key == w) {
            self.closing = true;
            return;
        }
        self.forget_window(w, cx);
        cx.close_window(w);
    }

    /// Window `w` is going: its panes' sessions end, its timers stop, and
    /// a question for one of its programs is no longer asked.
    fn forget_window(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(index) = self.windows.iter().position(|window| window.key == w) else { return };
        let mut window = self.windows.remove(index);
        for timer in [window.focus_timer.take(), window.find_state.timer.take()].into_iter().flatten() {
            cx.cancel(timer);
        }
        // Its question goes with it (the pane may be gone already: the
        // window's last tab closed).
        let asked = self.asking.as_ref().map(|question| question.request.pane);
        if asked.is_some_and(|pane| self.owner(pane).is_none_or(|owner| owner == w)) {
            self.asking = None;
        }
        self.panes.retain(|pane| pane.window != w);
        if self.front == w {
            self.front = self.windows.last().map_or(WindowKey::NONE, |window| window.key);
        }
    }

    fn close_pane(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(id) = self.pane_in(w).map(|pane| pane.id) else { return };
        self.clipboard_request();
        self.remove_pane(id, cx);
    }

    /// Take pane `id` out of whichever tab holds it, and that tab away with
    /// its last pane. Another tab's pane leaves the one in front alone.
    fn remove_pane(&mut self, id: u64, cx: &mut Cx<Msg>) {
        let Some(w) = self.owner(id) else { return };
        let Some(window) = self.window_mut(w) else { return };
        let Some(index) = window.tabs.iter().position(|tab| tab.tree.leaves().contains(&id)) else { return };
        let leaves = window.tabs[index].tree.leaves();
        if leaves.len() == 1 {
            self.remove_tab(w, index, cx);
            return;
        }
        let at = leaves.iter().position(|pane| *pane == id).unwrap();
        let active = window.active;
        let tab = &mut window.tabs[index];
        tab.tree = std::mem::replace(&mut tab.tree, PaneTree::Leaf(0)).remove(id).unwrap();
        if tab.active == id {
            tab.active = leaves[if at == 0 { 1 } else { at - 1 }];
            tab.zoomed = false;
        }
        tab.zoomed &= matches!(tab.tree, PaneTree::Split { .. });
        let shown = tab.id == active;
        self.panes.retain(|pane| pane.id != id);
        if shown {
            self.focus_after_rebuild(w, cx);
        }
    }

    fn cancel_automatic(&mut self) {
        if let Some(worker) = &self.resolver_worker {
            worker.cancel();
        }
        self.resolver_request = None;
        if let Some(worker) = &self.automatic_worker {
            worker.cancel();
        }
        self.automatic_request = None;
        self.automatic_hit = None;
    }
    fn current_link_request(&self, link: &crate::links::Link) -> bool {
        link.request()
            .is_none_or(|request| self.automatic_request.as_ref().is_some_and(|current| Arc::ptr_eq(request, current)))
    }
    fn valid_link_activation(&self, id: u64, link: &crate::links::Link) -> bool {
        self.current_link_request(link)
            && self.owner(id).and_then(|w| self.pane_in(w)).is_some_and(|pane| pane.id == id)
            && self.panes.iter().any(|pane| {
                pane.id == id && pane.displayed_snapshot().is_some_and(|snapshot| link.matches_snapshot(snapshot))
            })
    }
    fn notice(&mut self, id: u64, message: &str) {
        if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
            pane.status = message.into();
        }
    }

    fn focus_direction(&mut self, w: WindowKey, direction: Direction, cx: &mut Cx<Msg>) {
        let Some(tab) = self.window(w).and_then(TerminalWindow::active_tab) else { return };
        let Some(current) = self.pane_in(w) else { return };
        let target = match direction {
            Direction::Previous | Direction::Next => tab.tree.adjacent(tab.active, direction == Direction::Previous),
            _ => {
                let origin = current.rect.center();
                let leaves = tab.tree.leaves();
                self.panes
                    .iter()
                    .filter(|pane| leaves.contains(&pane.id) && pane.id != current.id)
                    .filter_map(|pane| {
                        let point = pane.rect.center();
                        let (distance, across) = match direction {
                            Direction::Left => (origin.x - point.x, origin.y - point.y),
                            Direction::Right => (point.x - origin.x, origin.y - point.y),
                            Direction::Up => (origin.y - point.y, origin.x - point.x),
                            Direction::Down => (point.y - origin.y, origin.x - point.x),
                            _ => unreachable!(),
                        };
                        (distance > 1.0).then_some((distance * distance + across * across, pane.id))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, id)| id)
            }
        };
        if let Some(id) = target {
            self.focus_pane(id, cx);
        }
    }

    fn focus_pane(&mut self, id: u64, cx: &mut Cx<Msg>) {
        let Some(w) = self.owner(id) else { return };
        let Some(window) = self.window_mut(w) else { return };
        if let Some(tab) = window.tabs.iter_mut().find(|tab| tab.tree.leaves().contains(&id)) {
            window.active = tab.id;
            // Going to another pane brings the tab's splits back, as Ghostty does.
            let unzoomed = tab.zoomed && tab.active != id;
            if unzoomed {
                tab.zoomed = false;
            }
            tab.active = id;
            if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                pane.bell = false;
            }
            if unzoomed { self.focus_after_rebuild(w, cx) } else { self.focus_active(w, cx) }
        }
    }

    fn send(&mut self, id: u64, command: Command) {
        let input = match &command {
            Command::Input(_) | Command::Paste(_) | Command::PasteEvent { .. } => true,
            Command::Key(key) => key.action != hotaru_core::input::KeyAction::Release,
            _ => false,
        };
        if input && let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
            pane.input_activity = pane.input_activity.wrapping_add(1);
        }
        if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id)
            && let Some(session) = &tab.session
            && let Err(e) = session.command(command)
        {
            tab.status = e.into();
        }
    }

    fn shape_pane(&mut self, id: u64, invalidate: bool) {
        let Some(w) = self.owner(id) else { return };
        let Some(window) = self.windows.iter().find(|window| window.key == w) else { return };
        let config = Arc::new(self.settings.fonts(window.font_size));
        let search = (window.find && window.find_state.pane == Some(id)).then_some(&window.find_state.caches);
        if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
            if invalidate {
                pane.font_epoch = pane.font_epoch.wrapping_add(1);
            }
            if let Some(worker) = &pane.font_worker {
                worker.submit(crate::font::Job {
                    epoch: pane.font_epoch,
                    snapshot: pane.snapshot.clone(),
                    config,
                    scale: pane.font_scale,
                    selection: pane.selection,
                    search: search.and_then(|caches| caches[usize::from(pane.snapshot.modes.alternate_screen)].clone()),
                });
            }
        }
    }

    fn shape_all(&mut self) {
        let ids: Vec<_> = self.panes.iter().map(|p| p.id).collect();
        for id in ids {
            self.shape_pane(id, true);
        }
    }

    /// Window `w`'s panes, shaped again.
    fn shape_window(&mut self, w: WindowKey) {
        let ids: Vec<_> = self.panes.iter().filter(|p| p.window == w).map(|p| p.id).collect();
        for id in ids {
            self.shape_pane(id, true);
        }
    }

    fn sync_search(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let pane = self.window(w).is_some_and(|window| window.find).then(|| self.pane_in(w).map(|pane| pane.id));
        let Some(window) = self.window_mut(w) else { return };
        let pane = pane.flatten();
        if window.find_state.pane != pane {
            if let Some(timer) = window.find_state.timer.take() {
                cx.cancel(timer);
            }
            window.find_state.invalidate(true);
            window.find_state.pane = pane;
            self.shape_window(w);
            self.request_search(w, cx);
        }
    }

    fn schedule_search(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(window) = self.window_mut(w) else { return };
        if !window.find || window.search.is_empty() || window.find_state.pane.is_none() {
            return;
        }
        window.find_state.dirty = true;
        if !window.find_state.in_flight && window.find_state.timer.is_none() {
            // Do not restart this timer for each output frame: a continuous
            // stream must eventually capture and present completed results.
            window.find_state.timer =
                Some(cx.after(std::time::Duration::from_millis(120), Msg::SearchRefresh(w, window.find_state.token)));
        }
    }

    fn request_search(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(window) = self.windows.iter_mut().find(|window| window.key == w) else { return };
        let state = &mut window.find_state;
        if let Some(timer) = state.timer.take() {
            cx.cancel(timer);
        }
        if !window.find || window.search.is_empty() || state.worker.is_none() {
            state.dirty = false;
            return;
        }
        if state.in_flight {
            state.dirty = true;
            return;
        }
        let Some(id) = state.pane else { return };
        state.token = state.token.wrapping_add(1);
        let token = state.token;
        state.in_flight = true;
        state.dirty = false;
        state.error = None;
        let Some(pane) = self.panes.iter().find(|pane| pane.id == id) else { return };
        if pane.ended && pane.session.is_none() {
            state.in_flight = false;
            state.error = Some("Search unavailable: terminal worker ended.".into());
            return;
        }
        if let Some(session) = &pane.session {
            if let Err(error) = session.command(Command::Search(crate::session::SearchControl::Capture {
                token,
                cancel: state.cancel.clone(),
            })) {
                state.in_flight = false;
                state.error = Some(error.into());
            }
        } else {
            // Preview has a bounded visible snapshot and no PTY owner.
            let snapshot = Arc::new(ContentSnapshot::from((*pane.snapshot).clone()));
            self.update(Msg::SearchSnapshot(token, id, snapshot), cx);
        }
    }

    fn navigate_search(&mut self, w: WindowKey, previous: bool, first: bool, cx: &mut Cx<Msg>) {
        let Some(pane) = self.pane_in(w) else { return };
        let (id, alternate) = (pane.id, pane.snapshot.modes.alternate_screen);
        let Some(window) = self.windows.iter_mut().find(|window| window.key == w) else { return };
        if !window.find || window.search.is_empty() {
            return;
        }
        let state = &mut window.find_state;
        let Some(cache) = state.caches[usize::from(alternate)].as_mut() else {
            self.request_search(w, cx);
            return;
        };
        let count = cache.results.matches.len();
        if count == 0 {
            return;
        }
        let index = if first {
            cache.selected.unwrap_or(0)
        } else {
            match cache.selected {
                None => {
                    if previous {
                        count - 1
                    } else {
                        0
                    }
                }
                Some(index) => (index + if previous { count - 1 } else { 1 }) % count,
            }
        };
        cache.selected = Some(index);
        let results = cache.results.clone();
        state.navigation = state.navigation.wrapping_add(1);
        let token = state.navigation;
        state.pending_navigation = Some((token, results.clone(), index));
        state.copy = None;
        let Some(pane) = self.panes.iter().find(|pane| pane.id == id) else { return };
        if let Some(session) = &pane.session {
            if let Err(error) = session.command(Command::Search(crate::session::SearchControl::Navigate {
                token,
                results,
                index,
                cancel: state.cancel.clone(),
            })) {
                state.error = Some(error.into());
            }
        } else {
            let valid = results.resolve_match(index, &pane.snapshot.position, pane.snapshot.generation).is_some();
            self.update(Msg::SearchNavigated(token, id, valid), cx);
        }
        self.shape_pane(id, true);
    }

    /// Whether a program's clipboard request waits for an answer.
    pub fn asking_about_clipboard(&self) -> bool {
        self.asking.is_some()
    }

    /// What the front window's find bar says of its search.
    pub fn search_status(&self) -> String {
        self.search_status_in(self.front)
    }

    fn search_status_in(&self, w: WindowKey) -> String {
        let Some(window) = self.window(w) else { return String::new() };
        window.find_state.label(self.pane_in(w).is_some_and(|pane| pane.snapshot.modes.alternate_screen))
    }

    pub fn search_selected(&self) -> Option<hotaru_core::search::Match> {
        let pane = self.active_pane()?;
        let (results, index) = self.front()?.find_state.copy.as_ref()?;
        results.resolve_match(*index, &pane.snapshot.position, pane.snapshot.generation)
    }

    fn clipboard_request(&mut self) -> u64 {
        self.clipboard_cancel.store(true, Ordering::Release);
        self.clipboard_cancel = Arc::new(AtomicBool::new(false));
        self.clipboard_format = self.settings.copy_format;
        if let Some(read) = self.clipboard_read.take() {
            read.cancel();
        }
        self.clipboard_token = self.clipboard_token.wrapping_add(1);
        self.clipboard_token
    }

    fn copy(&mut self, w: WindowKey, selected: bool) {
        let Some(tab) = self.pane_in(w) else { return };
        let found = selected.then(|| self.window(w).and_then(|window| window.find_state.copy.clone())).flatten();
        let selection = if selected && found.is_none() {
            let Some(selection) = tab.selection.filter(|s| !s.is_empty()) else { return };
            Some(selection)
        } else {
            None
        };
        let pane = tab.id;
        let snapshot = if selected {
            tab.rendered.as_ref().map_or_else(|| tab.snapshot.clone(), |frame| frame.snapshot.clone())
        } else {
            tab.snapshot.clone()
        };
        let source = if let Some((results, index)) = found {
            CopySource::Search { results, index }
        } else {
            CopySource::Viewport { snapshot, selection }
        };
        let token = self.clipboard_request();
        if let Some(worker) = &self.clipboard {
            worker.submit(Job::Copy {
                token,
                pane,
                source,
                format: self.clipboard_format,
                cancel: self.clipboard_cancel.clone(),
            });
        }
    }

    /// Copy on select: the pane's finished selection becomes the primary
    /// selection, as plain text, on the selection worker.
    fn copy_to_primary(&mut self, id: u64) {
        if !self.settings.copy_on_select {
            return;
        }
        let Some(pane) = self.panes.iter().find(|pane| pane.id == id) else { return };
        let Some(selection) = pane.selection.filter(|s| !s.is_empty()) else { return };
        let snapshot = pane.rendered.as_ref().map_or_else(|| pane.snapshot.clone(), |frame| frame.snapshot.clone());
        self.primary_cancel.store(true, Ordering::Release);
        self.primary_cancel = Arc::new(AtomicBool::new(false));
        self.primary_token = self.primary_token.wrapping_add(1);
        let job = crate::selections::Job::Primary {
            token: self.primary_token,
            pane: id,
            source: CopySource::Viewport { snapshot, selection: Some(selection) },
            cancel: self.primary_cancel.clone(),
        };
        if let Some(worker) = &self.selections {
            // Behind a full queue of reads, this selection isn't offered.
            worker.submit(job);
        }
    }

    /// A program asks to read or write the clipboard (OSC 52 or 5522):
    /// settings say whether it may, whether to ask, or not.
    fn program_clipboard(&mut self, request: crate::selections::Request, cx: &mut Cx<Msg>) {
        use crate::selections::{Answer, Kind, Target};
        if !self.panes.iter().any(|pane| pane.id == request.pane && !pane.ended) {
            return;
        }
        match &request.kind {
            Kind::KittyWrite { write, preview, .. } => match self.settings.clipboard_write {
                Access::Allow => self.answer(request, None, Answer::Allow, cx),
                // A session grant needs nobody asked.
                Access::Ask if write.granted => self.answer(request, None, Answer::Allow, cx),
                Access::Ask => {
                    let preview = preview.clone();
                    self.ask(request, None, &preview, cx);
                }
                Access::Deny => self.answer(request, None, Answer::Refuse, cx),
            },
            Kind::KittyRead(_) => {
                // Refused at once, so the program doesn't wait on an answer.
                if self.settings.clipboard_read == Access::Deny {
                    return self.answer(request, None, Answer::Refuse, cx);
                }
                let read = match request.target {
                    Target::Clipboard => cx.clipboard_read(),
                    Target::Primary => cx.primary_read(),
                };
                let (pane, busy) = (request.pane, request.busy());
                let job = crate::selections::Job::Read { request, read };
                if !self.selections.as_ref().is_some_and(|worker| worker.submit(job))
                    && let Some(busy) = busy
                {
                    self.send(pane, Command::Reply(busy));
                }
            }
            Kind::Write { data, preview } => match self.settings.clipboard_write {
                Access::Allow => publish(request.target, data.clone(), cx),
                Access::Ask => {
                    let preview = preview.clone();
                    self.ask(request, None, &preview, cx);
                }
                Access::Deny => {}
            },
            Kind::Read => {
                if self.settings.clipboard_read == Access::Deny {
                    // A refused read gets no answer, as Ghostty's.
                    return;
                }
                let read = match request.target {
                    Target::Clipboard => cx.clipboard_read(),
                    Target::Primary => cx.primary_read(),
                };
                let (pane, tag) = (request.pane, request.tag);
                let job = crate::selections::Job::Read { request, read };
                if !self.selections.as_ref().is_some_and(|worker| worker.submit(job)) {
                    self.send(pane, Command::Reply(crate::selections::reply(tag, b"")));
                }
            }
        }
    }

    /// A program that asked for paste events (mode 5522) is told a paste's
    /// types instead of given its text, and reads what it wants itself.
    fn paste_event(&mut self, id: u64, primary: bool, read: &ClipboardRead) -> bool {
        let Some(pane) = self.panes.iter().find(|pane| pane.id == id && !pane.ended) else { return false };
        if !pane.snapshot.modes.kitty_paste_events {
            return false;
        }
        let types: Vec<String> =
            read.types().into_iter().take(hotaru_core::kitty_clipboard::MAX_LISTING_MIMES).collect();
        self.send(id, Command::PasteEvent { primary, types: types.into() });
        true
    }

    /// Ask whether a program may have `request`; one question at a time.
    fn ask(
        &mut self,
        request: crate::selections::Request,
        reply: Option<Arc<[u8]>>,
        preview: &crate::selections::Preview,
        cx: &mut Cx<Msg>,
    ) {
        use crate::selections::{Answer, Kind};
        let Some(pane) = self.panes.iter().find(|pane| pane.id == request.pane) else { return };
        if self.asking.is_some() {
            let id = pane.id;
            self.notice(id, "A clipboard request was refused while another waits for an answer.");
            // Kitty's programs hear EBUSY, to ask again.
            if let Some(busy) = request.busy() {
                self.send(id, Command::Reply(busy));
            }
            return;
        }
        let pane_window = pane.window;
        let title = pane.title.chars().take(60).collect::<String>();
        // Kitty's programs give a name, theirs to choose: shown with the pane's title.
        let (name, pw) = match &request.kind {
            Kind::KittyRead(read) => (read.name.as_str(), read.pw.as_str()),
            Kind::KittyWrite { write, .. } => (write.name.as_str(), write.pw.as_str()),
            Kind::Read | Kind::Write { .. } => ("", ""),
        };
        let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
        let program = if name.is_empty() { title } else { format!("{name}” in “{title}") };
        let place = request.target.name();
        let shown = if preview.text.is_empty() { String::new() } else { format!(":\n\n“{}”", preview.text) };
        let types = preview.named_types();
        let (title, message, allow) = match request.kind {
            Kind::Read => {
                (format!("Let “{program}” read {place}?"), format!("It holds {}{shown}", preview.size()), "Allow")
            }
            Kind::Write { .. } => (
                format!("Let “{program}” replace {place}?"),
                format!("It would hold {}{shown}", preview.size()),
                "Replace",
            ),
            Kind::KittyRead(_) => (
                format!("Let “{program}” read {place}?"),
                format!("It asks for {types}: {}{shown}", preview.size()),
                "Allow",
            ),
            Kind::KittyWrite { .. } => (
                format!("Let “{program}” replace {place}?"),
                format!("It would hold {types}: {}{shown}", preview.size()),
                "Replace",
            ),
        };
        self.ask_token = self.ask_token.wrapping_add(1);
        let token = self.ask_token;
        // The question comes while someone may be typing: Return doesn't
        // answer it; Escape refuses. A program with a password may be let
        // go on without asking, until its pane resets.
        let mut alert = Alert::new(title, format!("{message}\n\nSettings → General chooses what programs may do."))
            .button(allow, Msg::ClipboardAnswer(token, Answer::Allow));
        if !pw.is_empty() {
            alert = alert.button("Allow for This Session", Msg::ClipboardAnswer(token, Answer::Remember));
        }
        cx.alert(pane_window, alert.cancel_with("Don’t Allow", Msg::ClipboardAnswer(token, Answer::Refuse)));
        self.asking = Some(crate::selections::Question { token, request, reply });
    }

    /// Carry out the answer about `request`: publish a write, send a read
    /// its captured answer, or refuse it (OSC 52's reads get an empty
    /// answer, Kitty's requests EPERM).
    fn answer(
        &mut self,
        request: crate::selections::Request,
        reply: Option<Arc<[u8]>>,
        answer: crate::selections::Answer,
        cx: &mut Cx<Msg>,
    ) {
        use crate::selections::{Answer, Kind};
        use hotaru_core::kitty_clipboard::{Direction, Status};
        if !self.panes.iter().any(|pane| pane.id == request.pane && !pane.ended) {
            return;
        }
        let allowed = matches!(answer, Answer::Allow | Answer::Remember);
        let refusal = request.refusal();
        match request.kind {
            Kind::KittyRead(read) => {
                if answer == Answer::Remember && !read.pw.is_empty() {
                    self.send(request.pane, Command::KittyGrant(read.pw.as_str().into(), Direction::Read));
                }
                if let Some(reply) = reply.filter(|_| allowed).or(refusal) {
                    self.send(request.pane, Command::Reply(reply));
                }
            }
            Kind::KittyWrite { write, data, .. } => {
                let mut status = Vec::new();
                if allowed {
                    if answer == Answer::Remember && !write.pw.is_empty() {
                        self.send(request.pane, Command::KittyGrant(write.pw.as_str().into(), Direction::Write));
                    }
                    publish(request.target, data, cx);
                    write.status(Status::Done, &mut status);
                } else {
                    write.status(Status::Eperm, &mut status);
                }
                self.send(request.pane, Command::Reply(status.into()));
            }
            Kind::Write { data, .. } if allowed => publish(request.target, data, cx),
            Kind::Write { .. } => {}
            Kind::Read => {
                let reply = reply.filter(|_| allowed).unwrap_or_else(|| crate::selections::reply(request.tag, b""));
                self.send(request.pane, Command::Reply(reply));
            }
        }
    }

    /// A program's notification reaches the desktop while its pane is out
    /// of sight (another tab, or the window not in use), once a second at most.
    fn notify(&mut self, id: u64, title: &str, body: &str, cx: &mut Cx<Msg>) {
        if !self.settings.desktop_notifications || self.focused_pane == Some(id) {
            return;
        }
        if self.notified.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        let Some(pane) = self.panes.iter().find(|pane| pane.id == id) else { return };
        let clean = |text: &str, limit: usize| text.chars().filter(|c| !c.is_control()).take(limit).collect::<String>();
        let (summary, body) = match (clean(title, 120), clean(body, 1000)) {
            (title, body) if title.is_empty() => (clean(&pane.title, 120), body),
            other => other,
        };
        if summary.is_empty() && body.is_empty() {
            return;
        }
        self.notified = Some(std::time::Instant::now());
        cx.notify(&summary, &body);
    }

    fn pane_tree_view(&self, window: &TerminalWindow, tree: &PaneTree) -> Element<Msg> {
        match tree {
            PaneTree::Leaf(id) => {
                let pane = self.panes.iter().find(|pane| pane.id == *id).expect("split leaf owns a pane");
                terminal_view(
                    pane.id,
                    pane.snapshot.clone(),
                    (window.font_size, self.settings.font_weight),
                    pane.selection,
                    self.bindings.clone(),
                    pane.rendered.clone(),
                    pane.input_activity,
                    self.automatic_hit
                        .as_ref()
                        .filter(|link| link.request().is_some_and(|request| request.pane == pane.id))
                        .cloned(),
                    self.context_menu(pane),
                )
                .id(format!("terminal-{}", pane.id))
                .key(pane.id)
                .width(Fill)
                .height(Fill)
            }
            PaneTree::Split { id, axis, first, second } => {
                let panes = split(self.pane_tree_view(window, first), self.pane_tree_view(window, second))
                    .min_sizes(80.0, 80.0)
                    .key(*id);
                if *axis == Axis::Down { panes.vertical().into() } else { panes.into() }
            }
        }
    }

    fn apply_settings(&mut self, settings: Settings, cx: &mut Cx<Msg>) {
        let cursor = (settings.cursor_style, settings.cursor_blink);
        let cursor_changed = cursor != (self.settings.cursor_style, self.settings.cursor_blink);
        for window in &mut self.windows {
            window.font_size = settings.font_size;
        }
        self.bindings = Arc::new(Bindings::new(&settings));
        self.settings = settings;
        self.apply_appearance(cx);
        if cursor_changed {
            self.apply_cursor();
        }
        self.shape_all();
    }

    /// Every pane's default cursor; a program's own choice stays in force.
    fn apply_cursor(&mut self) {
        let (shape, blinking) = (self.settings.cursor_style.shape(), self.settings.cursor_blink);
        let demo = self.demo;
        for pane in &mut self.panes {
            if let Some(session) = &pane.session {
                if let Err(error) = session.command(Command::Cursor(shape, blinking)) {
                    pane.status = error.into();
                }
            } else if demo {
                let snapshot = Arc::make_mut(&mut pane.snapshot);
                snapshot.cursor.shape = shape;
                snapshot.cursor.blinking = blinking;
            }
        }
        if demo {
            self.shape_all();
        }
    }

    fn show_settings(&mut self, cx: &mut Cx<Msg>) {
        if let Some(window) = self.settings_window {
            cx.activate(window);
            return;
        }
        // A dialog of the front window: never taken for a terminal window,
        // and closed with it.
        let parent = self.front_for(cx);
        let spec = WindowSpec::new("Hotaru Settings").size(720.0, 560.0).min_size(560.0, 400.0).dialog(parent);
        let window = cx.open_window(spec);
        if window == WindowKey::NONE {
            return;
        }
        self.settings_window = Some(window);
        self.settings_parent = parent;
        self.settings_notice = None;
        if self.monospace.is_none() {
            cx.spawn(|| Msg::MonospaceFamilies(crate::font::monospaced_families()));
        }
    }

    fn close_settings(&mut self, cx: &mut Cx<Msg>) {
        self.recording = None;
        if let Some(window) = self.settings_window.take() {
            cx.close_window(window);
        }
    }

    /// A shortcut-changing edit is made: use it, and keep it.
    fn rebind(&mut self, notice: Notice) {
        self.bindings = Arc::new(Bindings::new(&self.settings));
        self.save_settings();
        self.settings_notice = Some(notice);
    }

    fn record(&mut self, action: Action, shortcut: Shortcut) {
        use aizome_kit::ShortcutKey as K;
        let label = shortcut.label();
        let held = shortcut.command || shortcut.ctrl || shortcut.alt;
        let moves = matches!(shortcut.key, K::PageUp | K::PageDown | K::Home | K::End | K::Up | K::Down);
        let refusal = if !held && !matches!(shortcut.key, K::F(_)) && !(shortcut.shift && moves) {
            Some(format!("{label} is typing: hold ❖, ⌃ or ⌥ with the key."))
        } else if shortcut.ctrl
            && !(shortcut.command || shortcut.alt || shortcut.shift)
            && matches!(shortcut.key, K::Char(_) | K::Space)
        {
            Some(format!("{label} belongs to the programs in the terminal."))
        } else if settings::desktop_shortcut(shortcut) {
            Some(format!("{label} is the desktop's: Hotaru never hears it."))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.settings_notice = Some(Notice::failed(refusal));
            return;
        }
        let previous = self.bindings.bound(shortcut);
        if previous == Some(action) {
            self.settings_notice = Some(Notice::done(format!("{label} already runs {}.", action.label())));
            return;
        }
        let key = settings::format_shortcut(shortcut);
        if self.settings.keybindings.len() >= 128 && !self.settings.keybindings.contains_key(&key) {
            self.settings_notice = Some(Notice::failed("hotaru.toml holds at most 128 shortcut changes."));
            return;
        }
        self.settings.keybindings.retain(|written, _| settings::parse_shortcut(written) != Ok(shortcut));
        self.settings.keybindings.insert(key, action);
        self.rebind(Notice::done(match previous {
            Some(other) => format!("{label} runs {} now, not {}.", action.label(), other.label()),
            None => format!("{label} runs {} now.", action.label()),
        }));
    }

    fn unbind(&mut self, action: Action, shortcut: Shortcut) {
        // What the file says goes; a default is silenced with `ignore`.
        self.settings
            .keybindings
            .retain(|written, bound| !(settings::parse_shortcut(written) == Ok(shortcut) && *bound == action));
        if Bindings::is_default(shortcut, action) {
            self.settings.keybindings.insert(settings::format_shortcut(shortcut), Action::Ignore);
        }
        self.rebind(Notice::done(format!("{} no longer runs {}.", shortcut.label(), action.label())));
    }

    fn open_config_file(&mut self, cx: &mut Cx<Msg>) {
        let Some(path) = self.settings_path.clone() else { return };
        // An editor wants a file to open: write the settings out first.
        if !self.settings_managed && !path.exists() {
            self.open_after_save = true;
            self.save_settings();
            return;
        }
        let uri = crate::preferences::file_uri(&path);
        if let Err(error) = cx.open_url_with_result(&uri, Msg::ConfigOpened) {
            cx.send(Msg::ConfigOpened(Err(error)));
        }
    }

    fn find_selection(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(pane) = self.pane_in(w) else { return };
        let Some(selection) = pane.selection.filter(|selection| !selection.is_empty()) else { return };
        let text = selection.text(pane.displayed_snapshot().unwrap_or(&pane.snapshot));
        let needle: String = text.trim_end_matches(['\r', '\n']).chars().take(4096).collect();
        if needle.trim().is_empty() {
            return;
        }
        if let Some(window) = self.window_mut(w) {
            window.find = true;
        }
        self.update(Msg::Search(w, needle), cx);
    }

    fn pane_command(&mut self, w: WindowKey, command: Command) {
        let Some(id) = self.pane_in(w).map(|pane| pane.id) else { return };
        if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
            pane.selection = None;
        }
        self.send(id, command);
        self.shape_pane(id, true);
    }

    fn apply_appearance(&self, cx: &mut Cx<Msg>) {
        use aizome_kit::theme::ThemeChoice;
        let mut appearance = self.desktop_appearance;
        appearance.theme = match self.settings.theme {
            settings::Theme::Desktop => appearance.theme,
            settings::Theme::Night => ThemeChoice::Night,
            settings::Theme::Day => ThemeChoice::Day,
            settings::Theme::HighContrast => ThemeChoice::HighContrast,
        };
        if cx.appearance() != appearance {
            cx.set_appearance(appearance);
        }
    }

    fn save_settings(&mut self) {
        self.settings_token = self.settings_token.wrapping_add(1);
        if self.settings_managed {
            return;
        }
        if let Some(worker) = &self.settings_worker {
            self.settings_pending = true;
            worker.save(self.settings_token, self.settings.clone());
        }
    }

    /// A menu command or shortcut: for the front window.
    fn action(&mut self, action: Action, cx: &mut Cx<Msg>) {
        let w = self.front_for(cx);
        if self.settings_window.is_some() && cx.key_window() == self.settings_window {
            match action {
                // ❖W closes the window it's pressed in, as everywhere.
                Action::ClosePane | Action::CloseTab => {
                    self.close_settings(cx);
                    return;
                }
                Action::NewTab | Action::SplitRight | Action::SplitDown => cx.activate(w),
                _ => {}
            }
        }
        let page = self.pane_in(w).map(|pane| (pane.id, pane.snapshot.rows as i32));
        let message = match action {
            Action::Ignore => return,
            Action::NewWindow => Msg::NewWindow,
            Action::Settings => {
                self.show_settings(cx);
                return;
            }
            Action::NewTab => Msg::NewTab(w),
            Action::ClosePane => Msg::ClosePane(w),
            Action::CloseTab => Msg::CloseTab(w),
            Action::SplitRight => Msg::Split(w, Axis::Right),
            Action::SplitDown => Msg::Split(w, Axis::Down),
            Action::ZoomPane => {
                self.toggle_zoom(w, cx);
                return;
            }
            Action::PreviousPane => Msg::MoveFocus(w, Direction::Previous),
            Action::NextPane => Msg::MoveFocus(w, Direction::Next),
            Action::FocusLeft => Msg::MoveFocus(w, Direction::Left),
            Action::FocusRight => Msg::MoveFocus(w, Direction::Right),
            Action::FocusUp => Msg::MoveFocus(w, Direction::Up),
            Action::FocusDown => Msg::MoveFocus(w, Direction::Down),
            Action::PreviousTab | Action::NextTab => {
                self.step_tab(w, if action == Action::PreviousTab { -1 } else { 1 }, cx);
                return;
            }
            Action::SelectTab1
            | Action::SelectTab2
            | Action::SelectTab3
            | Action::SelectTab4
            | Action::SelectTab5
            | Action::SelectTab6
            | Action::SelectTab7
            | Action::SelectTab8 => {
                if let Some(index) = Action::SELECT_TAB.iter().position(|tab| *tab == action) {
                    self.select_tab_at(w, Some(index), cx);
                }
                return;
            }
            Action::LastTab => {
                self.select_tab_at(w, None, cx);
                return;
            }
            Action::Copy => Msg::Copy(w),
            Action::Paste => Msg::Paste(w),
            Action::CopyScreen => Msg::CopyScreen(w),
            Action::CopyAll => Msg::CopyAll(w),
            Action::Find => Msg::Find(w),
            Action::FindNext => Msg::SearchStep(w, false),
            Action::FindPrevious => Msg::SearchStep(w, true),
            Action::FindSelection => {
                self.find_selection(w, cx);
                return;
            }
            Action::ClearScreen => {
                self.pane_command(w, Command::Clear);
                return;
            }
            Action::ResetTerminal => {
                self.pane_command(w, Command::Reset);
                if let Some((id, _)) = page {
                    self.notice(id, "Terminal reset");
                }
                return;
            }
            Action::LargerText => Msg::FontSize(w, 1),
            Action::SmallerText => Msg::FontSize(w, -1),
            Action::ActualSize => {
                let size = self.settings.font_size;
                if let Some(window) = self.window_mut(w) {
                    window.font_size = size;
                }
                self.shape_window(w);
                return;
            }
            Action::ScrollToTop | Action::ScrollToBottom | Action::ScrollPageUp | Action::ScrollPageDown => {
                let Some((id, rows)) = page else { return };
                let lines = match action {
                    Action::ScrollToTop => i32::MAX,
                    Action::ScrollToBottom => i32::MIN,
                    Action::ScrollPageUp => rows,
                    _ => -rows,
                };
                Msg::Scroll(id, lines)
            }
            Action::ReloadConfig => {
                self.settings_token = self.settings_token.wrapping_add(1);
                self.settings_pending = false;
                if let Some(worker) = &self.settings_worker {
                    worker.load(self.settings_token);
                }
                return;
            }
        };
        self.update(message, cx);
    }

    /// A menu item for an action, with its shortcut unless Settings is
    /// waiting for a new one (which the menus mustn't take first).
    fn item(&self, label: &str, action: Action) -> MenuItem<Msg> {
        let item = MenuItem::new(label, Msg::Action(action));
        match self.bindings.shortcut(action) {
            Some(shortcut) if self.recording.is_none() => item.shortcut(shortcut),
            _ => item,
        }
    }

    /// What a right click in `pane` shows; a link under the pointer first.
    fn context_menu(&self, pane: &Pane) -> Menu<Msg> {
        let mut menu = Menu::new("Terminal");
        if let Some(link) = &pane.hovered_link {
            menu = menu
                .item(MenuItem::new("Open Link", Msg::OpenLink(pane.id, link.clone())))
                .item(MenuItem::new("Copy Link", Msg::CopyLink(link.target.clone())))
                .separator();
        }
        let selected = pane.selection.is_some_and(|s| !s.is_empty());
        let alone = self.window(pane.window).is_some_and(|w| w.tabs.iter().any(|tab| tab.tree.leaves() == [pane.id]));
        menu.item(self.item("Copy", Action::Copy).enabled(selected))
            .item(self.item("Paste", Action::Paste))
            .separator()
            .item(self.item("Split Right", Action::SplitRight))
            .item(self.item("Split Down", Action::SplitDown))
            .separator()
            .item(self.item("Clear Screen", Action::ClearScreen))
            .item(self.item("Reset Terminal", Action::ResetTerminal))
            .separator()
            .item(self.item("New Tab", Action::NewTab))
            .item(self.item(if alone { "Close Tab" } else { "Close Pane" }, Action::ClosePane))
            .separator()
            .item(self.item("Settings…", Action::Settings))
    }

    /// A tooltip naming the shortcut as it's bound now.
    fn tip(&self, tip: &str, action: Action) -> String {
        match self.bindings.shortcut(action) {
            Some(shortcut) => format!("{tip} · {}", shortcut.label()),
            None => tip.to_owned(),
        }
    }

    fn keyed(&self, item: MenuItem<Msg>, shortcut: Option<Shortcut>) -> MenuItem<Msg> {
        match shortcut {
            Some(shortcut) if self.recording.is_none() => item.shortcut(shortcut),
            _ => item,
        }
    }
}

impl Program for Hotaru {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Hotaru", "Hotaru", "Terminal")
            .icon(Icon::from_name("terminal").unwrap_or(Icon::Search))
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.desktop_appearance = cx.appearance();
        self.apply_settings(self.settings.clone(), cx);
        self.open_terminal_window(self.command.clone(), cx);
        if let Some(path) = self.settings_path.clone() {
            match settings::Worker::new(path, cx.stream()) {
                Ok(worker) => self.settings_worker = Some(worker),
                Err(error) => log::warn!("Cannot save Hotaru settings: {error}"),
            }
        }
        match ClipboardWorker::new(cx.stream()) {
            Ok(worker) => self.clipboard = Some(worker),
            Err(e) => {
                if let Some(tab) = self.panes.first_mut() {
                    tab.status = format!("Clipboard unavailable: {e}");
                }
            }
        }
        self.selections =
            crate::selections::Worker::new(cx.stream()).map_err(|error| log::warn!("Selection worker: {error}")).ok();
        if let Some(instance) = self.instance.take()
            && let Err(e) = instance.serve(cx.stream())
        {
            log::warn!("Could not serve application instance: {e}");
        }
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        self.front_for(cx);
        match msg {
            Msg::NewWindow => self.new_window(cx),
            Msg::Launched(launch, asked) => self.launch(launch, asked, cx),
            Msg::Action(action) => self.action(action, cx),
            Msg::SetTheme(theme) => {
                self.settings.theme = theme;
                self.apply_appearance(cx);
                self.save_settings();
            }
            Msg::SetWeight(weight) => {
                self.settings.font_weight = weight;
                self.save_settings();
                self.shape_all();
            }
            Msg::SetCopyFormat(format) => {
                self.settings.copy_format = format;
                self.save_settings();
            }
            Msg::SetFontSize(points) => {
                let points = ((points * 2.0).round() / 2.0).clamp(8.0, 72.0);
                if points.is_finite() {
                    self.settings.font_size = points;
                    for window in &mut self.windows {
                        window.font_size = points;
                    }
                    self.save_settings();
                    self.shape_all();
                }
            }
            Msg::SetFontFamily(family) => {
                if family.is_empty() || family == self.settings.primary_family() {
                    return;
                }
                let mut changed = self.settings.clone();
                changed.set_primary_family(&family);
                match changed.validate() {
                    Ok(()) => {
                        self.settings = changed;
                        self.save_settings();
                        self.shape_all();
                    }
                    Err(error) => self.settings_notice = Some(Notice::failed(error)),
                }
            }
            Msg::SetLigatures(on) => {
                self.settings.set_ligatures(on);
                self.save_settings();
                self.shape_all();
            }
            Msg::SetCursorStyle(style) => {
                self.settings.cursor_style = style;
                self.apply_cursor();
                self.save_settings();
            }
            Msg::SetCursorBlink(on) => {
                self.settings.cursor_blink = on;
                self.apply_cursor();
                self.save_settings();
            }
            Msg::SetScrollback(rows) => {
                if rows != self.settings.scrollback {
                    self.settings.scrollback = rows;
                    self.save_settings();
                    self.settings_notice = Some(Notice::done("New tabs and splits keep the new scrollback."));
                }
            }
            Msg::SetShellIntegration(on) => {
                self.settings.shell_integration = on;
                self.save_settings();
            }
            Msg::SetClipboardRead(access) => {
                self.settings.clipboard_read = access;
                self.save_settings();
            }
            Msg::SetClipboardWrite(access) => {
                self.settings.clipboard_write = access;
                self.save_settings();
            }
            Msg::SetCopyOnSelect(on) => {
                self.settings.copy_on_select = on;
                self.save_settings();
            }
            Msg::SetNotifications(on) => {
                self.settings.desktop_notifications = on;
                self.save_settings();
            }
            Msg::SetWaitAfterCommand(on) => {
                self.settings.wait_after_command = on;
                self.save_settings();
            }
            Msg::SettingsPage(page) => {
                self.settings_page = page;
                self.recording = None;
            }
            Msg::MonospaceFamilies(families) => self.monospace = Some(families),
            Msg::OpenConfigFile => self.open_config_file(cx),
            Msg::ConfigOpened(result) => {
                if let Err(error) = result {
                    self.settings_notice = Some(Notice::failed(format!("Could not open hotaru.toml: {error}")));
                }
            }
            Msg::RecordShortcut(action) => {
                self.recording = Some(action);
                self.settings_notice = None;
                cx.focus(crate::preferences::RECORDER);
            }
            Msg::StopRecording(action) => {
                if self.recording == Some(action) {
                    self.recording = None;
                }
            }
            Msg::Recorded(action, shortcut) => {
                if self.recording == Some(action) {
                    self.record(action, shortcut);
                }
            }
            Msg::RemoveShortcut(action, shortcut) => self.unbind(action, shortcut),
            Msg::RestoreShortcuts => {
                let Some(window) = self.settings_window else { return };
                let count = self.settings.keybindings.len();
                cx.alert(
                    window,
                    Alert::new(
                        "Restore the default shortcuts?",
                        format!(
                            "The {count} shortcut change{} in hotaru.toml will be gone.",
                            if count == 1 { "" } else { "s" }
                        ),
                    )
                    .destructive_button("Restore Defaults", Msg::RestoredShortcuts)
                    .cancel_button("Cancel"),
                );
            }
            Msg::RestoredShortcuts => {
                self.recording = None;
                self.settings.keybindings.clear();
                self.rebind(Notice::done("Every shortcut is as Hotaru comes."));
            }
            Msg::ConfigLoaded(token, result) => {
                if token != self.settings_token {
                    return;
                }
                self.settings_pending = false;
                match result {
                    Ok(settings) => {
                        self.apply_settings(settings, cx);
                        self.settings_notice = Some(Notice::done("Read hotaru.toml again."));
                    }
                    Err(error) => {
                        if let Some(id) = self.active_pane().map(|pane| pane.id) {
                            self.notice(id, &format!("Configuration not loaded: {error}"));
                        }
                        self.settings_notice = Some(Notice::failed(format!("Not loaded: {error}")));
                    }
                }
            }
            Msg::ConfigSaved(token, result) => {
                if token != self.settings_token {
                    return;
                }
                self.settings_pending = false;
                match result {
                    Err(error) => {
                        log::warn!("Configuration not saved: {error}");
                        if let Some(id) = self.active_pane().map(|pane| pane.id) {
                            self.notice(id, &format!("Configuration not saved: {error}"));
                        }
                        // Made a link since Hotaru started: stop trying.
                        self.settings_managed |= self.settings_path.as_deref().is_some_and(std::path::Path::is_symlink);
                        self.settings_notice = Some(Notice::failed(format!("Not saved: {error}")));
                        self.open_after_save = false;
                    }
                    Ok(()) => {
                        if self.settings_notice.as_ref().is_some_and(|notice| notice.error) {
                            self.settings_notice = None;
                        }
                        if std::mem::take(&mut self.open_after_save) {
                            self.open_config_file(cx);
                        }
                    }
                }
                // The last window was closed while saving; it, and any a
                // Quit left open, go now.
                if self.closing {
                    self.close_settings(cx);
                    for window in cx.windows() {
                        self.forget_window(window, cx);
                        cx.close_window(window);
                    }
                }
            }
            Msg::Theme(colors) => {
                if self.settings.theme == settings::Theme::Desktop {
                    self.desktop_appearance = cx.appearance();
                } else {
                    self.apply_appearance(cx);
                    if *colors != colors_for(cx.theme()) {
                        return;
                    }
                }
                if self.colors.as_deref() != Some(colors.as_ref()) {
                    for tab in &mut self.panes {
                        if let Some(session) = &tab.session {
                            if let Err(error) = session.command(Command::Colors(colors.clone())) {
                                tab.status = error.into();
                            }
                        } else if self.demo {
                            let snapshot = Arc::make_mut(&mut tab.snapshot);
                            snapshot.palette = colors.palette;
                            snapshot.default_foreground = colors.foreground;
                            snapshot.default_background = colors.background;
                        }
                    }
                    self.colors = Some(colors);
                    self.shape_all();
                }
            }
            Msg::NewTab(w) => self.new_tab(w, cx, true, None),
            Msg::Split(w, axis) => self.split_pane(w, axis, cx),
            Msg::ClosePane(w) => self.close_pane(w, cx),
            Msg::FocusPane(id) => self.focus_pane(id, cx),
            Msg::RestoreFocus(id, generation) => {
                let Some(w) = self.owner(id) else { return };
                if let Some(window) = self.window_mut(w)
                    && generation == window.focus_generation
                {
                    window.focus_timer = None;
                    if self.pane_in(w).is_some_and(|pane| pane.id == id) {
                        self.focus_active(w, cx);
                    }
                }
            }
            Msg::MoveFocus(w, direction) => self.focus_direction(w, direction, cx),
            Msg::Geometry(id, rect) => {
                if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                    pane.rect = rect;
                }
            }
            Msg::SelectTab(id) => {
                if let Some(window) = self.windows.iter_mut().find(|window| window.tabs.iter().any(|tab| tab.id == id))
                {
                    window.active = id;
                    let w = window.key;
                    self.focus_after_rebuild(w, cx);
                }
            }
            Msg::CloseTab(w) => self.close_tab(w, cx),
            Msg::Frame(id, frame) => {
                let mut search_changed = false;
                let mut screen_changed = false;
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    if let Some(session) = &tab.session {
                        session.received_frame();
                    }
                    if (tab.snapshot.cols, tab.snapshot.rows) != (frame.cols, frame.rows) {
                        tab.selection = None;
                        tab.font_epoch = tab.font_epoch.wrapping_add(1);
                    }
                    search_changed = tab.snapshot.generation != frame.generation
                        || tab.snapshot.position.epoch != frame.position.epoch
                        || tab.snapshot.position.origin != frame.position.origin;
                    screen_changed = tab.snapshot.modes.alternate_screen != frame.modes.alternate_screen;
                    tab.snapshot = frame;
                }
                let w = self.owner(id).unwrap_or(WindowKey::NONE);
                if search_changed
                    && let Some(window) =
                        self.windows.iter_mut().find(|window| window.key == w && window.find_state.pane == Some(id))
                {
                    let state = &mut window.find_state;
                    let pane = self.panes.iter().find(|pane| pane.id == id);
                    if let Some((results, index)) = &state.copy
                        && let Some(pane) = pane
                        && results.resolve_match(*index, &pane.snapshot.position, pane.snapshot.generation).is_none()
                    {
                        state.copy = None;
                    }
                    if screen_changed {
                        state.invalidate(false);
                        self.request_search(w, cx);
                    } else {
                        // A worker result can precede its coalesced actor frame.
                        // Keep its logical selection in the owned capture and
                        // publish copy only once this frame validates it. A
                        // pointer clear removes cache.selected; navigation has
                        // its own acknowledgment and must not be superseded.
                        if state.pending_navigation.is_none()
                            && !state.retry_navigation
                            && let Some(pane) = pane
                            && let Some(cache) = &state.caches[usize::from(pane.snapshot.modes.alternate_screen)]
                        {
                            state.copy = cache
                                .selected
                                .filter(|&index| {
                                    cache
                                        .results
                                        .resolve_match(index, &pane.snapshot.position, pane.snapshot.generation)
                                        .is_some()
                                })
                                .map(|index| (cache.results.clone(), index));
                        }
                        self.schedule_search(w, cx);
                    }
                }
                self.shape_pane(id, false);
            }
            Msg::ShapeScale(id, scale) => {
                if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
                    if pane.font_scale == scale {
                        return;
                    }
                    pane.font_scale = scale;
                }
                self.shape_pane(id, true);
            }
            Msg::Shaped(id, frame) => {
                if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id)
                    && pane.font_epoch == frame.epoch
                    && pane.font_scale == frame.metrics.scale
                {
                    if pane.rendered.as_ref().is_none_or(|old| old.epoch != frame.epoch) {
                        log::debug!(
                            "terminal font {:?}, scale {}, grid {}x{} px, unavailable families {:?}, sprite thickness {} px (estimated {})",
                            frame.primary_family,
                            frame.metrics.scale,
                            frame.metrics.cell_width_px,
                            frame.metrics.cell_height_px,
                            frame.missing_families,
                            frame.metrics.box_thickness_px,
                            frame.metrics.box_thickness_estimated
                        );
                    }
                    if pane.hovered_link.as_ref().is_some_and(|link| !link.matches_snapshot(&frame.snapshot)) {
                        pane.hovered_link = None;
                    }
                    pane.rendered = Some(frame);
                }
            }
            Msg::FontFailed(id, epoch, error) => {
                if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id)
                    && pane.font_epoch == epoch
                {
                    pane.status = format!("Could not prepare terminal text: {error}");
                }
            }
            Msg::MatchLink(id, request) => {
                if let Some(request) = request {
                    let valid = request.pane == id
                        && self.panes.iter().any(|pane| {
                            pane.id == id
                                && pane
                                    .displayed_snapshot()
                                    .is_some_and(|snapshot| Arc::ptr_eq(snapshot, &request.snapshot))
                        });
                    if valid {
                        self.cancel_automatic();
                        self.automatic_request = Some(request.clone());
                        if self.automatic_worker.is_none() {
                            let tx = cx.stream();
                            self.automatic_worker =
                                crate::automatic_links::Worker::new(move || tx.send(Msg::MatchedLink))
                                    .map_err(|error| log::warn!("Automatic link worker: {error}"))
                                    .ok();
                        }
                        if let Some(worker) = &self.automatic_worker
                            && let Err(error) = worker.submit(request)
                        {
                            log::debug!("Automatic link admission: {error:?}");
                        }
                    }
                } else if self.automatic_request.as_ref().is_some_and(|request| request.pane == id) {
                    self.cancel_automatic();
                }
            }
            Msg::MatchedLink => {
                if let Some(response) = self.automatic_worker.as_ref().and_then(|worker| worker.take_response())
                    && self.automatic_request.as_ref().is_some_and(|request| Arc::ptr_eq(request, &response.request))
                    && self.panes.iter().any(|pane| {
                        pane.id == response.request.pane
                            && pane
                                .displayed_snapshot()
                                .is_some_and(|snapshot| Arc::ptr_eq(snapshot, &response.request.snapshot))
                    })
                {
                    self.automatic_hit = match response.result {
                        Ok(Some(found)) => crate::links::Link::automatic(response.request, found),
                        Ok(None) => None,
                        Err(error) => {
                            log::debug!("Automatic link match: {error:?}");
                            None
                        }
                    };
                }
            }
            Msg::HoverLink(id, link) => {
                let link = link.filter(|link| self.current_link_request(link));
                if let Some(pane) = self.panes.iter_mut().find(|p| p.id == id) {
                    pane.hovered_link = link.filter(|link| {
                        pane.displayed_snapshot().is_some_and(|snapshot| link.matches_snapshot(snapshot))
                    });
                }
            }
            Msg::CopyLink(target) => cx.set_clipboard_text(&target),
            Msg::OpenLink(id, link) => {
                if self.valid_link_activation(id, &link) {
                    if link.is_automatic() {
                        let request = Arc::new(crate::link_resolver::Request {
                            pane: id,
                            link,
                            cwd: self.panes.iter().find(|p| p.id == id).and_then(|p| p.link_directory.clone()),
                        });
                        self.resolver_request = Some(request.clone());
                        if self.resolver_worker.is_none() {
                            let tx = cx.stream();
                            self.resolver_worker =
                                crate::link_resolver::Worker::new(move || tx.send(Msg::ResolvedLink))
                                    .map_err(|error| log::warn!("Link path worker: {error}"))
                                    .ok();
                        }
                        if let Some(worker) = &self.resolver_worker {
                            worker.submit(request);
                        } else {
                            self.notice(id, "Could not resolve link");
                            if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                                pane.hovered_link = None;
                            }
                        }
                    } else if let Err(error) =
                        cx.open_url_with_result(&link.target, move |result| Msg::LinkOpened(id, result))
                    {
                        cx.send(Msg::LinkOpened(id, Err(error)));
                    }
                }
            }
            Msg::ResolvedLink => {
                let Some(response) = self.resolver_worker.as_ref().and_then(|worker| worker.take_response()) else {
                    return;
                };
                let request = &response.request;
                if self.resolver_request.as_ref().is_some_and(|current| Arc::ptr_eq(current, request))
                    && self.valid_link_activation(request.pane, &request.link)
                    && self.panes.iter().any(|p| p.id == request.pane && p.link_directory == request.cwd)
                {
                    self.resolver_request = None;
                    let id = request.pane;
                    match response.result {
                        Ok(target) => {
                            if let Err(error) =
                                cx.open_url_with_result(&target, move |result| Msg::LinkOpened(id, result))
                            {
                                cx.send(Msg::LinkOpened(id, Err(error)));
                            }
                        }
                        Err(error) => {
                            log::debug!("Link path resolution: {error:?}");
                            self.notice(id, "Could not resolve link");
                            if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                                pane.hovered_link = None;
                            }
                        }
                    }
                }
            }
            Msg::LinkOpened(id, result) => {
                if let Err(error) = result
                    && let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id)
                {
                    pane.status = format!("Could not open link: {error}");
                    // Keep the failure visible even while the pointer remains
                    // over the clicked link. A later new hover can show a target.
                    pane.hovered_link = None;
                }
            }
            Msg::WorkingDirectory(id, directory) => {
                if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                    pane.working_directory = (!directory.as_os_str().is_empty()).then_some(directory);
                    pane.link_directory = pane.working_directory.clone();
                }
            }
            Msg::Title(id, title) => {
                let Some(pane) = self.panes.iter_mut().find(|t| t.id == id) else { return };
                pane.title = title.chars().filter(|c| !c.is_control()).take(256).collect();
                if pane.title.is_empty() {
                    pane.title = "Shell".into();
                }
                let (w, title) = (pane.window, format!("{} — Hotaru", pane.title));
                if self.window(w).and_then(TerminalWindow::active_tab).is_some_and(|tab| tab.active == id) {
                    cx.set_title(w, title);
                }
            }
            Msg::Exited(id, status, abnormal) => {
                // As Control-D at the prompt: the pane goes, and with the
                // last of them, its tab and then the window.
                if !abnormal && !self.settings.wait_after_command {
                    self.remove_pane(id, cx);
                } else if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.status = if abnormal {
                        format!("The program failed as it started ({status}). Press a key to close.")
                    } else {
                        format!("Session ended: {status}. Press a key to close.")
                    };
                    tab.ended = true;
                }
            }
            Msg::Failed(id, status) => {
                if let Some(window) = self.windows.iter_mut().find(|window| window.find_state.pane == Some(id)) {
                    let state = &mut window.find_state;
                    if let Some(timer) = state.timer.take() {
                        cx.cancel(timer);
                    }
                    state.invalidate(false);
                    state.dirty = false;
                    state.error = Some("Search unavailable: terminal worker ended.".into());
                }
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.status = format!("Session failed: {status}");
                    tab.ended = true;
                    tab.session = None;
                }
            }
            Msg::Notice(id, status) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.status = status;
                }
            }
            Msg::Bell(id) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.bell = true;
                }
            }
            Msg::Input(id, bytes) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.selection = None;
                }
                self.send(id, Command::Input(bytes.into()));
                self.shape_pane(id, true);
            }
            Msg::Key(id, event) => {
                let mut ended = false;
                if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                    pane.selection = None;
                    ended = pane.ended;
                }
                // A kept pane whose program ended closes on any key.
                if ended
                    && event.action == hotaru_core::input::KeyAction::Press
                    && !matches!(event.key, hotaru_core::input::Key::Modifier(_))
                {
                    self.remove_pane(id, cx);
                    return;
                }
                self.send(id, Command::Key(event));
                self.shape_pane(id, true);
            }
            Msg::PasteText(id, text) => self.send(id, Command::Paste(text)),
            Msg::Mouse(id, event) => self.send(id, Command::Mouse(event)),
            Msg::Wheel(id, event, lines) => self.send(id, Command::Wheel(event, lines)),
            Msg::TerminalFocus(id, focused) => {
                if focused {
                    self.focused_pane = Some(id);
                    // Its window has the keyboard: commands go there now.
                    if let Some(w) = self.owner(id) {
                        self.front = w;
                    }
                } else if self.focused_pane == Some(id) {
                    self.focused_pane = None;
                }
                self.send(id, Command::Focus(focused));
            }
            Msg::Selected(id) => self.copy_to_primary(id),
            Msg::PrimaryReady(token, data) => {
                if token == self.primary_token {
                    cx.set_prepared_primary(data);
                }
            }
            Msg::PastePrimary(id) => {
                // A newer paste, or paste event, replaces one still reading.
                let token = self.clipboard_request();
                let read = cx.primary_read();
                if self.paste_event(id, true, &read) {
                    return;
                }
                self.clipboard_read = Some(read.clone());
                if let Some(worker) = &self.clipboard {
                    worker.submit(Job::Paste { token, tab: id, read });
                }
            }
            Msg::Clipboard(request) => self.program_clipboard(request, cx),
            Msg::ClipboardCaptured(request, reply, preview) => {
                use crate::selections::{Answer, Kind};
                // A Kitty read discloses nothing past the listing when nothing
                // came, or only the listing was asked for; a grant needs no
                // question either.
                if let Kind::KittyRead(read) = &request.kind {
                    let open = read.granted || read.prompt_exempt() || preview.bytes == 0;
                    match self.settings.clipboard_read {
                        Access::Deny => self.answer(request, None, Answer::Refuse, cx),
                        Access::Ask if !open => self.ask(request, reply, &preview, cx),
                        _ => self.answer(request, reply, Answer::Allow, cx),
                    }
                    return;
                }
                // Settings may have changed while the read was captured. With
                // nothing to read there's nothing to ask about: it's answered empty.
                match (self.settings.clipboard_read, reply) {
                    (Access::Deny, _) => {}
                    (_, None) => self.send(request.pane, Command::Reply(crate::selections::reply(request.tag, b""))),
                    (Access::Allow, Some(reply)) => self.send(request.pane, Command::Reply(reply)),
                    (Access::Ask, Some(reply)) => self.ask(request, Some(reply), &preview, cx),
                }
            }
            Msg::ClipboardAnswer(token, answer) => {
                let Some(question) = self.asking.take_if(|question| question.token == token) else { return };
                let w = self.owner(question.request.pane);
                self.answer(question.request, question.reply, answer, cx);
                if let Some(w) = w {
                    self.focus_active(w, cx);
                }
            }
            Msg::Notify(id, title, body) => self.notify(id, &title, &body, cx),
            Msg::Resize(id, cols, rows) => self.send(id, Command::Resize { cols, rows }),
            Msg::ResizePixels(id, cols, rows, width, height) => {
                self.send(id, Command::ResizePixels { cols, rows, width, height })
            }
            Msg::Scroll(id, lines) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.selection = None;
                }
                self.send(id, Command::Scroll(lines));
                self.shape_pane(id, true);
            }
            Msg::Select(id, selection) => {
                if let Some(window) = self.windows.iter_mut().find(|window| window.find_state.pane == Some(id)) {
                    let state = &mut window.find_state;
                    state.navigation = state.navigation.wrapping_add(1);
                    state.copy = None;
                    state.pending_navigation = None;
                    state.retry_navigation = false;
                    for cache in state.caches.iter_mut().flatten() {
                        cache.selected = None;
                    }
                }
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.selection = Some(selection);
                }
                self.shape_pane(id, true);
            }
            Msg::Copy(w) => self.copy(w, true),
            Msg::CopyScreen(w) => self.copy(w, false),
            Msg::CopyAll(w) => {
                let Some(id) = self.pane_in(w).map(|pane| pane.id) else { return };
                let token = self.clipboard_request();
                self.send(id, Command::CopyAll(token));
            }
            Msg::CopySnapshot(token, pane, snapshot) => {
                if token == self.clipboard_token
                    && self.panes.iter().any(|candidate| candidate.id == pane)
                    && let Some(worker) = &self.clipboard
                {
                    worker.submit(Job::Copy {
                        token,
                        pane,
                        source: CopySource::Content(snapshot),
                        format: self.clipboard_format,
                        cancel: self.clipboard_cancel.clone(),
                    });
                }
            }
            Msg::CopyReady(token, pane, prepared) => {
                if token == self.clipboard_token && self.panes.iter().any(|candidate| candidate.id == pane) {
                    cx.set_prepared_clipboard(prepared);
                }
            }
            Msg::CopyFailed(token, pane, error) => {
                if token == self.clipboard_token {
                    self.notice(pane, &error);
                }
            }
            Msg::Paste(w) => {
                let Some(id) = self.pane_in(w).map(|pane| pane.id) else { return };
                let token = self.clipboard_request();
                let read = cx.clipboard_read();
                if self.paste_event(id, false, &read) {
                    return;
                }
                self.clipboard_read = Some(read.clone());
                if let Some(worker) = &self.clipboard {
                    worker.submit(Job::Paste { token, tab: id, read });
                }
            }
            Msg::PasteReady(token, tab, read, text) => {
                if token == self.clipboard_token
                    && read.is_current()
                    && self.panes.iter().any(|t| t.id == tab && !t.ended)
                {
                    if let Some(text) = text {
                        self.send(tab, Command::Paste(text));
                    } else if let Some(target) = self.panes.iter_mut().find(|t| t.id == tab) {
                        target.status = "Clipboard text unavailable, too large, or timed out.".into();
                    }
                    if let Some(w) = self.owner(tab)
                        && self.pane_in(w).is_some_and(|pane| pane.id == tab)
                    {
                        self.focus_active(w, cx);
                    }
                }
                if token == self.clipboard_token {
                    self.clipboard_read = None;
                }
            }
            Msg::Find(w) => {
                if let Some(window) = self.window_mut(w) {
                    window.find = true;
                    cx.focus_in(w, "find");
                }
            }
            Msg::Search(w, search) => {
                if let Some(window) = self.window_mut(w) {
                    window.search = search;
                    window.find_state.invalidate(true);
                    self.request_search(w, cx);
                    self.shape_window(w);
                }
            }
            Msg::SearchRefresh(w, token) => {
                if let Some(window) = self.window_mut(w)
                    && token == window.find_state.token
                {
                    window.find_state.timer = None;
                    self.request_search(w, cx);
                }
            }
            Msg::SearchSnapshot(token, id, snapshot) => {
                if let Some(window) = self.windows.iter().find(|window| window.find_state.pane == Some(id))
                    && token == window.find_state.token
                    && window.find_state.in_flight
                    && let Some(worker) = &window.find_state.worker
                {
                    let state = &window.find_state;
                    let preserve = state.caches[usize::from(snapshot.modes.alternate_screen)]
                        .as_ref()
                        .map(|cache| (cache.results.clone(), cache.selected));
                    worker.submit(crate::search::Job {
                        token,
                        pane: id,
                        navigation: state.navigation,
                        snapshot: (*snapshot).clone(),
                        needle: window.search.as_bytes().to_vec(),
                        preserve,
                        cancel: state.cancel.clone(),
                    });
                }
            }
            Msg::SearchReady(token, id, navigation, cache) => {
                let w = self.owner(id).unwrap_or(WindowKey::NONE);
                let active = self.pane_in(w).map(|pane| pane.id);
                if let Some(window) = self.windows.iter_mut().find(|window| window.key == w)
                    && token == window.find_state.token
                    && window.find_state.pane == Some(id)
                    && window.find
                    && window.find_state.in_flight
                {
                    let state = &mut window.find_state;
                    let pane = active.and_then(|active| self.panes.iter().find(|pane| pane.id == active));
                    if navigation != state.navigation
                        || pane.is_none_or(|pane| pane.snapshot.position.epoch != cache.results.snapshot.position.epoch)
                    {
                        state.in_flight = false;
                        self.schedule_search(w, cx);
                        return;
                    }
                    let slot = usize::from(cache.results.snapshot.modes.alternate_screen);
                    let first = state.caches[slot].is_none();
                    if state.pending_navigation.is_none()
                        && !state.retry_navigation
                        && let Some(pane) = pane
                    {
                        state.copy = cache
                            .selected
                            .filter(|&index| {
                                cache
                                    .results
                                    .resolve_match(index, &pane.snapshot.position, pane.snapshot.generation)
                                    .is_some()
                            })
                            .map(|index| (cache.results.clone(), index));
                    }
                    state.caches[slot] = Some(cache);
                    state.in_flight = false;
                    state.error = None;
                    if first || std::mem::take(&mut state.retry_navigation) {
                        self.navigate_search(w, false, true, cx);
                    }
                    self.shape_pane(id, true);
                    if self.window(w).is_some_and(|window| window.find_state.dirty) {
                        self.schedule_search(w, cx);
                    }
                }
            }
            Msg::SearchFailed(token, id, error) => {
                if let Some(window) = self.windows.iter_mut().find(|window| window.find_state.pane == Some(id))
                    && token == window.find_state.token
                {
                    window.find_state.in_flight = false;
                    window.find_state.error = Some(error);
                }
            }
            Msg::SearchStep(w, previous) => self.navigate_search(w, previous, false, cx),
            Msg::SearchNavigated(token, id, valid) => {
                if let Some(window) = self.windows.iter_mut().find(|window| window.find_state.pane == Some(id))
                    && window.find_state.pending_navigation.as_ref().is_some_and(|(pending, _, _)| *pending == token)
                {
                    let (w, state) = (window.key, &mut window.find_state);
                    let (_, results, index) = state.pending_navigation.take().unwrap();
                    if valid {
                        state.copy = Some((results, index));
                    } else {
                        state.copy = None;
                        state.retry_navigation = true;
                        state.dirty = true;
                        self.schedule_search(w, cx);
                    }
                }
            }
            Msg::CloseFind(w) => {
                if let Some(window) = self.window_mut(w) {
                    window.find = false;
                    window.search.clear();
                }
                self.shape_window(w);
                self.focus_active(w, cx);
            }
            Msg::FontSize(w, delta) => {
                // ❖+ and ❖− zoom the window for now, as other terminals do;
                // Actual Size (❖0) comes back to the size Settings keeps.
                if let Some(window) = self.window_mut(w) {
                    window.font_size = (window.font_size + f64::from(delta)).clamp(8.0, 72.0);
                }
                self.shape_window(w);
            }
        }
        if self.automatic_request.as_ref().is_some_and(|request| {
            !self.panes.iter().any(|pane| {
                pane.id == request.pane
                    && pane.displayed_snapshot().is_some_and(|snapshot| Arc::ptr_eq(snapshot, &request.snapshot))
            })
        }) {
            self.cancel_automatic();
        }
        let keys: Vec<WindowKey> = self.windows.iter().map(|window| window.key).collect();
        for w in keys {
            self.sync_search(w, cx);
        }
    }
    fn view(&self, key: WindowKey) -> Element<Msg> {
        if Some(key) == self.settings_window {
            return crate::preferences::view(self);
        }
        let Some(window) = self.window(key) else {
            // Closing (as Quit waits for settings being saved).
            return aizome_kit::widgets::layout::column(Vec::new()).width(Fill).height(Fill).into();
        };
        let mut tabs: Vec<Element<Msg>> = window
            .tabs
            .iter()
            .map(|tab| {
                let title =
                    self.panes.iter().find(|pane| pane.id == tab.active).map_or("Shell", |pane| pane.title.as_str());
                let bell = tab.tree.leaves().iter().any(|id| self.panes.iter().any(|pane| pane.id == *id && pane.bell));
                button(if bell { format!("• {title}") } else { title.to_owned() })
                    .toolbar()
                    .selected(tab.id == window.active)
                    .on_press(Msg::SelectTab(tab.id))
                    .id(format!("tab-{}", tab.id))
                    .max_width(240.0)
                    .key(tab.id)
                    .into()
            })
            .collect();
        tabs.push(spacer().into());
        tabs.push(
            icon_button(Icon::Plus)
                .toolbar()
                .on_press(Msg::NewTab(key))
                .tooltip(self.tip("New Tab", Action::NewTab))
                .into(),
        );
        tabs.push(
            icon_button(Icon::Search).toolbar().on_press(Msg::Find(key)).tooltip(self.tip("Find", Action::Find)).into(),
        );
        let mut contents: Vec<Element<Msg>> = vec![toolbar(tabs)];
        if window.find {
            contents.push(
                row![
                    search_field(&window.search)
                        .on_change(move |text| Msg::Search(key, text))
                        .on_submit(Msg::SearchStep(key, false))
                        .id("find")
                        .width(Fill),
                    status_text(self.search_status_in(key)),
                    button("Previous")
                        .on_press(Msg::SearchStep(key, true))
                        .tooltip(self.tip("Previous match", Action::FindPrevious)),
                    button("Next")
                        .on_press(Msg::SearchStep(key, false))
                        .tooltip(self.tip("Next match", Action::FindNext)),
                    button("Done").on_press(Msg::CloseFind(key))
                ]
                .padding([6.0, 10.0])
                .spacing(8.0)
                .into(),
            );
        }
        if let Some(tab) = window.active_tab() {
            // Zoomed, the active pane stands alone; its splits wait in the tree.
            contents.push(if tab.zoomed {
                self.pane_tree_view(window, &PaneTree::Leaf(tab.active))
            } else {
                self.pane_tree_view(window, &tab.tree)
            });
        }
        if let Some(pane) = self.pane_in(key) {
            let mut bar = vec![
                status_text(pane.hovered_link.as_ref().map_or(pane.status.as_str(), |link| link.target.as_ref())),
                spacer().into(),
            ];
            if let Some(tab) = window.active_tab().filter(|tab| tab.zoomed) {
                let count = tab.tree.leaves().len();
                bar.push(status_text(format!("Zoomed: 1 of {count} panes")));
            }
            if (window.font_size - self.settings.font_size).abs() > f64::EPSILON {
                bar.push(status_text(format!("{} pt", window.font_size)));
            }
            bar.push(status_text(format!("{} × {}", pane.snapshot.cols, pane.snapshot.rows)));
            contents.push(status_bar(bar));
        }
        aizome_kit::widgets::layout::column(contents).width(Fill).height(Fill).into()
    }
    /// The front window's.
    fn menus(&self) -> MenuBar<Msg> {
        let front = self.front();
        let selected = front.is_some_and(|w| w.find_state.copy.is_some())
            || self.active_pane().is_some_and(|t| t.selection.is_some_and(|s| !s.is_empty()));
        let selection = self.active_pane().is_some_and(|t| t.selection.is_some_and(|s| !s.is_empty()));
        let tab = self.active_tab();
        let split = tab.is_some_and(|tab| matches!(tab.tree, PaneTree::Split { .. }));
        let zoomed = tab.is_some_and(|tab| tab.zoomed);
        let tabs = front.map_or(&[][..], |w| &w.tabs);
        let several = tabs.len() > 1;
        let role = |label: &str, role: MenuRole| MenuItem::<Msg>::role_item(label, role);
        // The app menu as standard, with Settings… and the file behind it.
        let app = Menu::new("Hotaru")
            .item(role("About Hotaru", MenuRole::About))
            .separator()
            .item(self.item("Settings…", Action::Settings))
            .item(self.item("Reload Configuration", Action::ReloadConfig).enabled(self.settings_path.is_some()))
            .separator()
            .submenu(Menu::new("Services").item(role("No Services Apply", MenuRole::Services).enabled(false)))
            .separator()
            .item(self.keyed(role("Hide Hotaru", MenuRole::Hide), Some(Shortcut::cmd('H'))))
            .item(self.keyed(role("Hide Others", MenuRole::HideOthers), Some(Shortcut::cmd('H').alt())))
            .separator()
            .item(self.keyed(role("Quit Hotaru", MenuRole::Quit), Some(Shortcut::cmd('Q'))));
        let file = Menu::new("File")
            .item(self.item("New Window", Action::NewWindow))
            .item(self.item("New Tab", Action::NewTab))
            .item(self.item("Split Right", Action::SplitRight))
            .item(self.item("Split Down", Action::SplitDown))
            .separator()
            .item(self.item("Close Pane", Action::ClosePane))
            .item(self.item("Close Tab", Action::CloseTab));
        let mut copy_format = Menu::new("Copy Format");
        for (name, format) in [
            ("Plain Text", settings::CopyFormat::Plain),
            ("ANSI Text", settings::CopyFormat::Vt),
            ("HTML", settings::CopyFormat::Html),
            ("Plain Text + HTML", settings::CopyFormat::Mixed),
        ] {
            copy_format = copy_format
                .item(MenuItem::new(name, Msg::SetCopyFormat(format)).checked(self.settings.copy_format == format));
        }
        let searching = front.is_some_and(|w| w.find && !w.search.is_empty());
        let edit = Menu::new("Edit")
            .item(self.item("Copy", Action::Copy).enabled(selected))
            .item(self.item("Paste", Action::Paste))
            .item(self.item("Copy Visible Screen", Action::CopyScreen))
            .item(self.item("Copy All Output", Action::CopyAll))
            .submenu(copy_format)
            .separator()
            .item(self.item("Find…", Action::Find))
            .item(self.item("Find Next", Action::FindNext).enabled(searching))
            .item(self.item("Find Previous", Action::FindPrevious).enabled(searching))
            .item(self.item("Use Selection for Find", Action::FindSelection).enabled(selection))
            .separator()
            .item(self.item("Clear Screen", Action::ClearScreen));
        let mut appearance = Menu::new("Appearance");
        for (name, theme) in [
            ("Follow Desktop", settings::Theme::Desktop),
            ("Night", settings::Theme::Night),
            ("Day", settings::Theme::Day),
            ("High Contrast", settings::Theme::HighContrast),
        ] {
            appearance =
                appearance.item(MenuItem::new(name, Msg::SetTheme(theme)).checked(self.settings.theme == theme));
        }
        let mut weight = Menu::new("Text Weight");
        for (value, name) in crate::preferences::WEIGHTS {
            weight =
                weight.item(MenuItem::new(name, Msg::SetWeight(value)).checked(self.settings.font_weight == value));
        }
        let view = Menu::new("View")
            .item(self.item("Larger Text", Action::LargerText))
            .item(self.item("Smaller Text", Action::SmallerText))
            .item(
                self.item("Actual Size", Action::ActualSize)
                    .enabled(front.is_some_and(|w| (w.font_size - self.settings.font_size).abs() > f64::EPSILON)),
            )
            .submenu(weight)
            .submenu(appearance)
            .separator()
            .item(self.item("Scroll to Top", Action::ScrollToTop))
            .item(self.item("Scroll to Bottom", Action::ScrollToBottom))
            .item(self.item("Page Up", Action::ScrollPageUp))
            .item(self.item("Page Down", Action::ScrollPageDown))
            .separator()
            .item(self.item("Reset Terminal", Action::ResetTerminal));
        let select_pane = Menu::new("Select Pane")
            .item(self.item("Left", Action::FocusLeft).enabled(split))
            .item(self.item("Right", Action::FocusRight).enabled(split))
            .item(self.item("Above", Action::FocusUp).enabled(split))
            .item(self.item("Below", Action::FocusDown).enabled(split));
        let mut window = Menu::new("Window")
            .item(self.keyed(role("Minimise", MenuRole::Minimise), Some(Shortcut::cmd('M'))))
            .item(role("Fill", MenuRole::Fill))
            .item(role("Stack with…", MenuRole::StackWith))
            .separator()
            .item(self.item("Previous Tab", Action::PreviousTab).enabled(several))
            .item(self.item("Next Tab", Action::NextTab).enabled(several))
            .separator()
            .item(self.item("Zoom Pane", Action::ZoomPane).checked(zoomed).enabled(split))
            .item(self.item("Previous Pane", Action::PreviousPane).enabled(split))
            .item(self.item("Next Pane", Action::NextPane).enabled(split))
            .submenu(select_pane)
            .separator();
        // The tabs by their titles, ❖1 to ❖8 and the last ❖9, as in Ghostty.
        let last = tabs.len().saturating_sub(1);
        for (index, tab) in tabs.iter().enumerate() {
            let title =
                self.panes.iter().find(|pane| pane.id == tab.active).map_or("Shell", |pane| pane.title.as_str());
            let title: String = title.chars().take(48).collect();
            let shortcut = match Action::SELECT_TAB.get(index) {
                Some(action) => self.bindings.shortcut(*action),
                None if index == last => self.bindings.shortcut(Action::LastTab),
                None => None,
            };
            window = window.item(self.keyed(
                MenuItem::new(title, Msg::SelectTab(tab.id)).checked(front.is_some_and(|w| tab.id == w.active)),
                shortcut,
            ));
        }
        let window = window
            .separator()
            .item(role("Bring All to Front", MenuRole::BringAllToFront))
            .separator()
            .item(role("Windows", MenuRole::WindowList).enabled(false));
        MenuBar::standard(&self.info())
            .replace("Hotaru", app)
            .with_file_menu(file)
            .replace("Edit", edit)
            .with_menu(view)
            .replace("Window", window)
    }
    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if Some(window) == self.settings_window {
            self.settings_window = None;
            self.recording = None;
            return true;
        }
        if self.settings_window.is_some() && self.settings_parent == window {
            self.close_settings(cx);
        }
        // The other windows go on; with the last, the app.
        if self.windows.iter().all(|w| w.key == window) {
            self.cancel_automatic();
            self.automatic_worker = None;
            self.resolver_worker = None;
            if self.settings_pending {
                self.closing = true;
                return false;
            }
            self.clipboard_request();
            self.clipboard = None;
        }
        self.forget_window(window, cx);
        true
    }
}

impl Drop for Hotaru {
    fn drop(&mut self) {
        self.clipboard_cancel.store(true, Ordering::Release);
        self.primary_cancel.store(true, Ordering::Release);
        if let Some(read) = &self.clipboard_read {
            read.cancel();
        }
    }
}

/// Put a program's write on the selection it named.
fn publish(target: crate::selections::Target, data: PreparedClipboard, cx: &mut Cx<Msg>) {
    match target {
        crate::selections::Target::Clipboard => cx.set_prepared_clipboard(data),
        crate::selections::Target::Primary => cx.set_prepared_primary(data),
    }
}

#[cfg(test)]
mod automatic_link_tests {
    use super::*;
    use aizome_kit::testing::Harness;
    use hotaru_link_regex::{CellPoint, Limits, Matcher};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    fn link(snapshot: Arc<Snapshot>, pane: u64) -> crate::links::Link {
        let cell = CellPoint { row: 0, col: 2 };
        let mapped = crate::automatic_links::mapping::extract(
            &snapshot,
            cell,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        let matched = Matcher::new()
            .unwrap()
            .find_at(
                &mapped.source,
                cell,
                &Limits { deadline: Instant::now() + Duration::from_secs(5), ..Default::default() },
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
        crate::links::Link::automatic(
            Arc::new(crate::automatic_links::Request { pane, snapshot, cell, retained_bytes: 1024 }),
            matched,
        )
        .unwrap()
    }
    #[test]
    fn accepted_activation_requires_exact_displayed_frame_pane_and_request() {
        let mut h = Harness::new(Hotaru::preview());
        h.advance(Duration::ZERO);
        let id = h.program().active_pane().unwrap().id;
        let mut term = Terminal::new(80, 4, 0);
        term.feed(b"https://example.test/path");
        let snapshot = Arc::new(term.snapshot());
        h.send(Msg::Frame(id, snapshot.clone()));
        assert!(h.settle(Duration::from_secs(5), |app| {
            app.active_pane().unwrap().displayed_snapshot().is_some_and(|s| Arc::ptr_eq(s, &snapshot))
        }));
        let first = link(snapshot.clone(), id);
        h.send(Msg::MatchLink(id, first.request().cloned()));
        assert!(h.program().valid_link_activation(id, &first));
        assert!(!h.program().valid_link_activation(id + 1, &first));
        let replacement = link(snapshot, id);
        h.send(Msg::MatchLink(id, replacement.request().cloned()));
        assert!(!h.program().valid_link_activation(id, &first));
        assert!(h.program().valid_link_activation(id, &replacement));
        h.send(Msg::MatchLink(id, None));
        assert!(!h.program().valid_link_activation(id, &replacement));
        h.send(Msg::MatchLink(id, replacement.request().cloned()));
        h.send(Msg::Frame(id, Arc::new(term.snapshot())));
        assert!(h.settle(Duration::from_secs(5), |app| {
            app.active_pane().unwrap().displayed_snapshot().is_some_and(|s| !Arc::ptr_eq(s, &replacement.snapshot))
        }));
        assert!(!h.program().valid_link_activation(id, &replacement));
    }
}
