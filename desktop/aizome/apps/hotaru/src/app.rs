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
use crate::settings::{self, Action, Bindings, Settings};
use crate::terminal_view::{Selection, colors_for, terminal_view};

#[derive(Clone)]
pub enum Msg {
    NewTab,
    Launch(PtyCommand),
    SelectTab(u64),
    CloseTab,
    Split(Axis),
    ClosePane,
    FocusPane(u64),
    RestoreFocus(u64, u64),
    MoveFocus(Direction),
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
    Exited(u64, String),
    Failed(u64, String),
    Notice(u64, String),
    MatchLink(u64, Option<Arc<crate::automatic_links::Request>>),
    MatchedLink,
    ResolvedLink,
    HoverLink(u64, Option<crate::links::Link>),
    OpenLink(u64, crate::links::Link),
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
    Copy,
    CopyScreen,
    CopyAll,
    CopySnapshot(u64, u64, Arc<ContentSnapshot>),
    CopyReady(u64, u64, PreparedClipboard),
    CopyFailed(u64, u64, String),
    PasteReady(u64, u64, ClipboardRead, Option<Arc<str>>),
    Paste,
    Find,
    Search(String),
    SearchRefresh(u64),
    SearchSnapshot(u64, u64, Arc<ContentSnapshot>),
    SearchReady(u64, u64, u64, crate::search::Cache),
    SearchFailed(u64, u64, String),
    SearchStep(bool),
    SearchNavigated(u64, u64, bool),
    CloseFind,
    FontSize(i8),
    Theme(Arc<TerminalColors>),
}

pub struct Pane {
    pub id: u64,
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

pub struct Hotaru {
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub active: u64,
    /// The size text is drawn at: the configured size, zoomed by ❖+ and ❖−.
    pub font_size: f64,
    pub find: bool,
    pub search: String,
    pub(crate) find_state: crate::search::State,
    window: WindowKey,
    next_id: u64,
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
    focus_timer: Option<TimerHandle>,
    focus_generation: u64,
    automatic_worker: Option<crate::automatic_links::Worker>,
    automatic_request: Option<Arc<crate::automatic_links::Request>>,
    automatic_hit: Option<crate::links::Link>,
    resolver_worker: Option<crate::link_resolver::Worker>,
    resolver_request: Option<Arc<crate::link_resolver::Request>>,
    /// The Settings window, while it's open.
    pub settings_window: Option<WindowKey>,
    pub settings_page: Page,
    pub(crate) settings_notice: Option<Notice>,
    /// The configuration is a symbolic link (a system's): changes aren't saved.
    pub settings_managed: bool,
    /// Monospaced families installed, once looked for.
    pub(crate) monospace: Option<Arc<[String]>>,
    /// The action a shortcut is being typed for in Settings.
    pub recording: Option<Action>,
    open_after_save: bool,
}

impl Hotaru {
    pub fn new(command: PtyCommand) -> Self {
        Self {
            tabs: Vec::new(),
            panes: Vec::new(),
            active: 0,
            font_size: settings::DEFAULT_FONT_SIZE,
            find: false,
            search: String::new(),
            find_state: crate::search::State::default(),
            window: WindowKey::NONE,
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
            focus_timer: None,
            focus_generation: 0,
            automatic_worker: None,
            automatic_request: None,
            automatic_hit: None,
            resolver_worker: None,
            resolver_request: None,
            settings_window: None,
            settings_page: Page::default(),
            settings_notice: None,
            settings_managed: false,
            monospace: None,
            recording: None,
            open_after_save: false,
        }
    }

    /// A deterministic renderer fixture: never starts a subprocess.
    pub fn preview() -> Self {
        let mut app = Self::new(PtyCommand::shell());
        app.demo = true;
        app
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active)
    }

    pub fn active_pane(&self) -> Option<&Pane> {
        let pane = self.active_tab()?.active;
        self.panes.iter().find(|p| p.id == pane)
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

    fn focus_active(&self, cx: &mut Cx<Msg>) {
        if let Some(pane) = self.active_pane() {
            cx.focus(&format!("terminal-{}", pane.id));
            cx.set_title(self.window, format!("{} — Hotaru", pane.title));
        }
    }

    fn focus_after_rebuild(&mut self, cx: &mut Cx<Msg>) {
        self.focus_active(cx);
        if let Some(timer) = self.focus_timer.take() {
            cx.cancel(timer);
        }
        self.focus_generation = self.focus_generation.wrapping_add(1);
        if let Some(pane) = self.active_pane() {
            // A split edit reparents a still-named leaf. An immediate focus can
            // succeed on its old node, which is removed during the next build.
            self.focus_timer =
                Some(cx.after(std::time::Duration::ZERO, Msg::RestoreFocus(pane.id, self.focus_generation)));
        }
    }

    fn new_pane(&mut self, cx: &mut Cx<Msg>, inherit_cwd: bool) -> u64 {
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
        let mut command = self.command.clone();
        if inherit_cwd
            && command.integrate_shell
            && let Some(directory) = self.active_pane().and_then(|pane| pane.working_directory.clone())
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

    fn new_tab(&mut self, cx: &mut Cx<Msg>, inherit_cwd: bool) {
        if self.panes.len() >= 64 {
            if let Some(id) = self.active_pane().map(|pane| pane.id) {
                self.notice(id, "The 64 pane limit has been reached.");
            }
            return;
        }
        let id = self.new_pane(cx, inherit_cwd);
        self.tabs.push(Tab { id, tree: PaneTree::Leaf(id), active: id, zoomed: false });
        self.active = id;
        self.focus_after_rebuild(cx);
    }

    /// The tab at `index` (0-based), or the last for `None`.
    fn select_tab_at(&mut self, index: Option<usize>, cx: &mut Cx<Msg>) {
        let tab = match index {
            Some(index) => self.tabs.get(index),
            None => self.tabs.last(),
        };
        if let Some(id) = tab.map(|tab| tab.id) {
            self.update(Msg::SelectTab(id), cx);
        }
    }

    /// The tab `step` places along, round from the last to the first.
    fn step_tab(&mut self, step: isize, cx: &mut Cx<Msg>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == self.active) else { return };
        let count = self.tabs.len() as isize;
        self.select_tab_at(Some((index as isize + step).rem_euclid(count) as usize), cx);
    }

    fn toggle_zoom(&mut self, cx: &mut Cx<Msg>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == self.active) else { return };
        // A lone pane already fills its tab.
        tab.zoomed = !tab.zoomed && matches!(tab.tree, PaneTree::Split { .. });
        self.focus_after_rebuild(cx);
    }

    fn split_pane(&mut self, axis: Axis, cx: &mut Cx<Msg>) {
        let Some(target) = self.active_pane().map(|pane| pane.id) else { return };
        if self.panes.len() >= 64 {
            self.notice(target, "The 64 pane limit has been reached.");
            return;
        }
        let id = self.new_pane(cx, true);
        let split = self.next_id;
        self.next_id += 1;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == self.active) {
            tab.tree.split(target, id, split, axis);
            tab.active = id;
            tab.zoomed = false;
        }
        self.focus_after_rebuild(cx);
    }

    fn close_tab(&mut self, cx: &mut Cx<Msg>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == self.active) else { return };
        self.clipboard_request();
        let removed = self.tabs.remove(index).tree.leaves();
        self.panes.retain(|pane| !removed.contains(&pane.id));
        if self.tabs.is_empty() {
            // Settings alone would be a terminal without a terminal.
            if let Some(window) = self.settings_window.take() {
                self.recording = None;
                cx.close_window(window);
            }
            if self.settings_pending {
                self.closing = true;
            } else {
                cx.close_window(self.window);
            }
        } else {
            self.active = self.tabs[index.min(self.tabs.len() - 1)].id;
            self.focus_after_rebuild(cx);
        }
    }

    fn close_pane(&mut self, cx: &mut Cx<Msg>) {
        let Some(tab) = self.active_tab() else { return };
        let leaves = tab.tree.leaves();
        if leaves.len() == 1 {
            self.close_tab(cx);
            return;
        }
        let id = tab.active;
        let index = leaves.iter().position(|pane| *pane == id).unwrap();
        let next = leaves[if index == 0 { 1 } else { index - 1 }];
        self.clipboard_request();
        let tab = self.tabs.iter_mut().find(|tab| tab.id == self.active).unwrap();
        tab.tree = std::mem::replace(&mut tab.tree, PaneTree::Leaf(0)).remove(id).unwrap();
        tab.active = next;
        tab.zoomed = false;
        self.panes.retain(|pane| pane.id != id);
        self.focus_after_rebuild(cx);
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
            && self.active_pane().is_some_and(|pane| pane.id == id)
            && self.panes.iter().any(|pane| {
                pane.id == id && pane.displayed_snapshot().is_some_and(|snapshot| link.matches_snapshot(snapshot))
            })
    }
    fn notice(&mut self, id: u64, message: &str) {
        if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
            pane.status = message.into();
        }
    }

    fn focus_direction(&mut self, direction: Direction, cx: &mut Cx<Msg>) {
        let Some(tab) = self.active_tab() else { return };
        let Some(current) = self.active_pane() else { return };
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
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.tree.leaves().contains(&id)) {
            self.active = tab.id;
            // Going to another pane brings the tab's splits back, as Ghostty does.
            let unzoomed = tab.zoomed && tab.active != id;
            if unzoomed {
                tab.zoomed = false;
            }
            tab.active = id;
            if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                pane.bell = false;
            }
            if unzoomed { self.focus_after_rebuild(cx) } else { self.focus_active(cx) }
        }
    }

    fn send(&mut self, id: u64, command: Command) {
        let input = match &command {
            Command::Input(_) | Command::Paste(_) => true,
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
        let config = Arc::new(self.settings.fonts(self.font_size));
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
                    search: (self.find && self.find_state.pane == Some(id))
                        .then(|| self.find_state.caches[usize::from(pane.snapshot.modes.alternate_screen)].clone())
                        .flatten(),
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

    fn sync_search(&mut self, cx: &mut Cx<Msg>) {
        let pane = self.find.then(|| self.active_pane().map(|pane| pane.id)).flatten();
        if self.find_state.pane != pane {
            if let Some(timer) = self.find_state.timer.take() {
                cx.cancel(timer);
            }
            self.find_state.invalidate(true);
            self.find_state.pane = pane;
            self.shape_all();
            self.request_search(cx);
        }
    }

    fn schedule_search(&mut self, cx: &mut Cx<Msg>) {
        if !self.find || self.search.is_empty() || self.find_state.pane.is_none() {
            return;
        }
        self.find_state.dirty = true;
        if !self.find_state.in_flight && self.find_state.timer.is_none() {
            // Do not restart this timer for each output frame: a continuous
            // stream must eventually capture and present completed results.
            self.find_state.timer =
                Some(cx.after(std::time::Duration::from_millis(120), Msg::SearchRefresh(self.find_state.token)));
        }
    }

    fn request_search(&mut self, cx: &mut Cx<Msg>) {
        if let Some(timer) = self.find_state.timer.take() {
            cx.cancel(timer);
        }
        if !self.find || self.search.is_empty() || self.find_state.worker.is_none() {
            self.find_state.dirty = false;
            return;
        }
        if self.find_state.in_flight {
            self.find_state.dirty = true;
            return;
        }
        let Some(id) = self.find_state.pane else { return };
        self.find_state.token = self.find_state.token.wrapping_add(1);
        let token = self.find_state.token;
        self.find_state.in_flight = true;
        self.find_state.dirty = false;
        self.find_state.error = None;
        let Some(pane) = self.panes.iter().find(|pane| pane.id == id) else { return };
        if pane.ended && pane.session.is_none() {
            self.find_state.in_flight = false;
            self.find_state.error = Some("Search unavailable: terminal worker ended.".into());
            return;
        }
        if let Some(session) = &pane.session {
            if let Err(error) = session.command(Command::Search(crate::session::SearchControl::Capture {
                token,
                cancel: self.find_state.cancel.clone(),
            })) {
                self.find_state.in_flight = false;
                self.find_state.error = Some(error.into());
            }
        } else {
            // Preview has a bounded visible snapshot and no PTY owner.
            self.update(Msg::SearchSnapshot(token, id, Arc::new(ContentSnapshot::from((*pane.snapshot).clone()))), cx);
        }
    }

    fn navigate_search(&mut self, previous: bool, first: bool, cx: &mut Cx<Msg>) {
        let Some(pane) = self.active_pane() else { return };
        let (id, alternate) = (pane.id, pane.snapshot.modes.alternate_screen);
        if !self.find || self.search.is_empty() {
            return;
        }
        let Some(cache) = self.find_state.caches[usize::from(alternate)].as_mut() else {
            self.request_search(cx);
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
        self.find_state.navigation = self.find_state.navigation.wrapping_add(1);
        let token = self.find_state.navigation;
        self.find_state.pending_navigation = Some((token, results.clone(), index));
        self.find_state.copy = None;
        if let Some(session) = &self.active_pane().unwrap().session {
            if let Err(error) = session.command(Command::Search(crate::session::SearchControl::Navigate {
                token,
                results,
                index,
                cancel: self.find_state.cancel.clone(),
            })) {
                self.find_state.error = Some(error.into());
            }
        } else {
            let snapshot = &self.active_pane().unwrap().snapshot;
            let valid = results.resolve_match(index, &snapshot.position, snapshot.generation).is_some();
            self.update(Msg::SearchNavigated(token, id, valid), cx);
        }
        self.shape_pane(id, true);
    }

    pub fn search_status(&self) -> String {
        self.find_state.label(self.active_pane().is_some_and(|pane| pane.snapshot.modes.alternate_screen))
    }

    pub fn search_selected(&self) -> Option<hotaru_core::search::Match> {
        let pane = self.active_pane()?;
        let (results, index) = self.find_state.copy.as_ref()?;
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

    fn copy(&mut self, selected: bool) {
        let Some(tab) = self.active_pane() else { return };
        let found = selected.then(|| self.find_state.copy.clone()).flatten();
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

    fn pane_tree_view(&self, tree: &PaneTree) -> Element<Msg> {
        match tree {
            PaneTree::Leaf(id) => {
                let pane = self.panes.iter().find(|pane| pane.id == *id).expect("split leaf owns a pane");
                terminal_view(
                    pane.id,
                    pane.snapshot.clone(),
                    (self.font_size, self.settings.font_weight),
                    pane.selection,
                    self.bindings.clone(),
                    pane.rendered.clone(),
                    pane.input_activity,
                    self.automatic_hit
                        .as_ref()
                        .filter(|link| link.request().is_some_and(|request| request.pane == pane.id))
                        .cloned(),
                )
                .id(format!("terminal-{}", pane.id))
                .key(pane.id)
                .width(Fill)
                .height(Fill)
            }
            PaneTree::Split { id, axis, first, second } => {
                let panes =
                    split(self.pane_tree_view(first), self.pane_tree_view(second)).min_sizes(80.0, 80.0).key(*id);
                if *axis == Axis::Down { panes.vertical().into() } else { panes.into() }
            }
        }
    }

    fn apply_settings(&mut self, settings: Settings, cx: &mut Cx<Msg>) {
        let cursor = (settings.cursor_style, settings.cursor_blink);
        let cursor_changed = cursor != (self.settings.cursor_style, self.settings.cursor_blink);
        self.font_size = settings.font_size;
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
        let spec = WindowSpec::new("Hotaru Settings").size(720.0, 560.0).min_size(560.0, 400.0);
        let window = cx.open_window(spec);
        if window == WindowKey::NONE {
            return;
        }
        self.settings_window = Some(window);
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

    fn find_selection(&mut self, cx: &mut Cx<Msg>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(selection) = pane.selection.filter(|selection| !selection.is_empty()) else { return };
        let text = selection.text(pane.displayed_snapshot().unwrap_or(&pane.snapshot));
        let needle: String = text.trim_end_matches(['\r', '\n']).chars().take(4096).collect();
        if needle.trim().is_empty() {
            return;
        }
        self.find = true;
        self.update(Msg::Search(needle), cx);
    }

    fn pane_command(&mut self, command: Command) {
        let Some(id) = self.active_pane().map(|pane| pane.id) else { return };
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

    fn action(&mut self, action: Action, cx: &mut Cx<Msg>) {
        if self.settings_window.is_some() && cx.key_window() == self.settings_window {
            match action {
                // ❖W closes the window it's pressed in, as everywhere.
                Action::ClosePane | Action::CloseTab => {
                    self.close_settings(cx);
                    return;
                }
                Action::NewTab | Action::SplitRight | Action::SplitDown => cx.activate(self.window),
                _ => {}
            }
        }
        let page = self.active_pane().map(|pane| (pane.id, pane.snapshot.rows as i32));
        let message = match action {
            Action::Ignore => return,
            Action::Settings => {
                self.show_settings(cx);
                return;
            }
            Action::NewTab => Msg::NewTab,
            Action::ClosePane => Msg::ClosePane,
            Action::CloseTab => Msg::CloseTab,
            Action::SplitRight => Msg::Split(Axis::Right),
            Action::SplitDown => Msg::Split(Axis::Down),
            Action::ZoomPane => {
                self.toggle_zoom(cx);
                return;
            }
            Action::PreviousPane => Msg::MoveFocus(Direction::Previous),
            Action::NextPane => Msg::MoveFocus(Direction::Next),
            Action::FocusLeft => Msg::MoveFocus(Direction::Left),
            Action::FocusRight => Msg::MoveFocus(Direction::Right),
            Action::FocusUp => Msg::MoveFocus(Direction::Up),
            Action::FocusDown => Msg::MoveFocus(Direction::Down),
            Action::PreviousTab | Action::NextTab => {
                self.step_tab(if action == Action::PreviousTab { -1 } else { 1 }, cx);
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
                    self.select_tab_at(Some(index), cx);
                }
                return;
            }
            Action::LastTab => {
                self.select_tab_at(None, cx);
                return;
            }
            Action::Copy => Msg::Copy,
            Action::Paste => Msg::Paste,
            Action::CopyScreen => Msg::CopyScreen,
            Action::CopyAll => Msg::CopyAll,
            Action::Find => Msg::Find,
            Action::FindNext => Msg::SearchStep(false),
            Action::FindPrevious => Msg::SearchStep(true),
            Action::FindSelection => {
                self.find_selection(cx);
                return;
            }
            Action::ClearScreen => {
                self.pane_command(Command::Clear);
                return;
            }
            Action::ResetTerminal => {
                self.pane_command(Command::Reset);
                if let Some((id, _)) = page {
                    self.notice(id, "Terminal reset");
                }
                return;
            }
            Action::LargerText => Msg::FontSize(1),
            Action::SmallerText => Msg::FontSize(-1),
            Action::ActualSize => {
                self.font_size = self.settings.font_size;
                self.shape_all();
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
        self.window = cx.open_window(WindowSpec::new("Hotaru").size(960.0, 640.0).min_size(320.0, 180.0));
        self.find_state.worker =
            crate::search::Worker::new(cx.stream()).map_err(|error| log::warn!("Search worker: {error}")).ok();
        self.new_tab(cx, false);
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
        if let Some(instance) = self.instance.take()
            && let Err(e) = instance.serve(cx.stream())
        {
            log::warn!("Could not serve application instance: {e}");
        }
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
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
                    self.font_size = points;
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
                if self.closing {
                    self.close_settings(cx);
                    cx.close_window(self.window);
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
            Msg::NewTab => self.new_tab(cx, true),
            Msg::Split(axis) => self.split_pane(axis, cx),
            Msg::ClosePane => self.close_pane(cx),
            Msg::FocusPane(id) => self.focus_pane(id, cx),
            Msg::RestoreFocus(id, generation) => {
                if generation == self.focus_generation {
                    self.focus_timer = None;
                    if self.active_pane().is_some_and(|pane| pane.id == id) {
                        self.focus_active(cx);
                    }
                }
            }
            Msg::MoveFocus(direction) => self.focus_direction(direction, cx),
            Msg::Geometry(id, rect) => {
                if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                    pane.rect = rect;
                }
            }
            Msg::Launch(command) => {
                let previous = std::mem::replace(&mut self.command, command);
                self.new_tab(cx, false);
                self.command = previous;
                cx.activate(self.window);
            }
            Msg::SelectTab(id) => {
                if self.tabs.iter().any(|tab| tab.id == id) {
                    self.active = id;
                    self.focus_after_rebuild(cx);
                }
            }
            Msg::CloseTab => self.close_tab(cx),
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
                if self.find_state.pane == Some(id) && search_changed {
                    if let Some((results, index)) = &self.find_state.copy
                        && let Some(pane) = self.panes.iter().find(|pane| pane.id == id)
                        && results.resolve_match(*index, &pane.snapshot.position, pane.snapshot.generation).is_none()
                    {
                        self.find_state.copy = None;
                    }
                    if screen_changed {
                        self.find_state.invalidate(false);
                        self.request_search(cx);
                    } else {
                        // A worker result can precede its coalesced actor frame.
                        // Keep its logical selection in the owned capture and
                        // publish copy only once this frame validates it. A
                        // pointer clear removes cache.selected; navigation has
                        // its own acknowledgment and must not be superseded.
                        if self.find_state.pending_navigation.is_none()
                            && !self.find_state.retry_navigation
                            && let Some(pane) = self.panes.iter().find(|pane| pane.id == id)
                            && let Some(cache) =
                                &self.find_state.caches[usize::from(pane.snapshot.modes.alternate_screen)]
                        {
                            self.find_state.copy = cache
                                .selected
                                .filter(|&index| {
                                    cache
                                        .results
                                        .resolve_match(index, &pane.snapshot.position, pane.snapshot.generation)
                                        .is_some()
                                })
                                .map(|index| (cache.results.clone(), index));
                        }
                        self.schedule_search(cx);
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
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.title = title.chars().filter(|c| !c.is_control()).take(256).collect();
                    if tab.title.is_empty() {
                        tab.title = "Shell".into();
                    }
                    if self.tabs.iter().any(|tab| tab.id == self.active && tab.active == id) {
                        cx.set_title(self.window, format!("{} — Hotaru", tab.title));
                    }
                }
            }
            Msg::Exited(id, status) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.status = format!("Session ended: {status}");
                    tab.ended = true;
                }
            }
            Msg::Failed(id, status) => {
                if self.find_state.pane == Some(id) {
                    if let Some(timer) = self.find_state.timer.take() {
                        cx.cancel(timer);
                    }
                    self.find_state.invalidate(false);
                    self.find_state.dirty = false;
                    self.find_state.error = Some("Search unavailable: terminal worker ended.".into());
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
                if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                    pane.selection = None;
                }
                self.send(id, Command::Key(event));
                self.shape_pane(id, true);
            }
            Msg::PasteText(id, text) => self.send(id, Command::Paste(text)),
            Msg::Mouse(id, event) => self.send(id, Command::Mouse(event)),
            Msg::Wheel(id, event, lines) => self.send(id, Command::Wheel(event, lines)),
            Msg::TerminalFocus(id, focused) => self.send(id, Command::Focus(focused)),
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
                if self.find_state.pane == Some(id) {
                    self.find_state.navigation = self.find_state.navigation.wrapping_add(1);
                    self.find_state.copy = None;
                    self.find_state.pending_navigation = None;
                    self.find_state.retry_navigation = false;
                    for cache in self.find_state.caches.iter_mut().flatten() {
                        cache.selected = None;
                    }
                }
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.selection = Some(selection);
                }
                self.shape_pane(id, true);
            }
            Msg::Copy => self.copy(true),
            Msg::CopyScreen => self.copy(false),
            Msg::CopyAll => {
                let Some(id) = self.active_pane().map(|pane| pane.id) else { return };
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
            Msg::Paste => {
                let Some(id) = self.active_pane().map(|pane| pane.id) else { return };
                let token = self.clipboard_request();
                let read = cx.clipboard_read();
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
                    if self.active_pane().is_some_and(|pane| pane.id == tab) {
                        self.focus_active(cx);
                    }
                }
                if token == self.clipboard_token {
                    self.clipboard_read = None;
                }
            }
            Msg::Find => {
                self.find = true;
                cx.focus("find");
            }
            Msg::Search(search) => {
                self.search = search;
                self.find_state.invalidate(true);
                self.request_search(cx);
                self.shape_all();
            }
            Msg::SearchRefresh(token) => {
                if token == self.find_state.token {
                    self.find_state.timer = None;
                    self.request_search(cx);
                }
            }
            Msg::SearchSnapshot(token, id, snapshot) => {
                if token == self.find_state.token
                    && self.find_state.pane == Some(id)
                    && self.find_state.in_flight
                    && let Some(worker) = &self.find_state.worker
                {
                    let preserve = self.find_state.caches[usize::from(snapshot.modes.alternate_screen)]
                        .as_ref()
                        .map(|cache| (cache.results.clone(), cache.selected));
                    worker.submit(crate::search::Job {
                        token,
                        pane: id,
                        navigation: self.find_state.navigation,
                        snapshot: (*snapshot).clone(),
                        needle: self.search.as_bytes().to_vec(),
                        preserve,
                        cancel: self.find_state.cancel.clone(),
                    });
                }
            }
            Msg::SearchReady(token, id, navigation, cache) => {
                if token == self.find_state.token
                    && self.find_state.pane == Some(id)
                    && self.find
                    && self.find_state.in_flight
                {
                    if navigation != self.find_state.navigation
                        || self
                            .active_pane()
                            .is_none_or(|pane| pane.snapshot.position.epoch != cache.results.snapshot.position.epoch)
                    {
                        self.find_state.in_flight = false;
                        self.schedule_search(cx);
                        return;
                    }
                    let slot = usize::from(cache.results.snapshot.modes.alternate_screen);
                    let first = self.find_state.caches[slot].is_none();
                    if self.find_state.pending_navigation.is_none()
                        && !self.find_state.retry_navigation
                        && let Some(pane) = self.active_pane()
                    {
                        self.find_state.copy = cache
                            .selected
                            .filter(|&index| {
                                cache
                                    .results
                                    .resolve_match(index, &pane.snapshot.position, pane.snapshot.generation)
                                    .is_some()
                            })
                            .map(|index| (cache.results.clone(), index));
                    }
                    self.find_state.caches[slot] = Some(cache);
                    self.find_state.in_flight = false;
                    self.find_state.error = None;
                    if first || std::mem::take(&mut self.find_state.retry_navigation) {
                        self.navigate_search(false, true, cx);
                    }
                    self.shape_pane(id, true);
                    if self.find_state.dirty {
                        self.schedule_search(cx);
                    }
                }
            }
            Msg::SearchFailed(token, id, error) => {
                if token == self.find_state.token && self.find_state.pane == Some(id) {
                    self.find_state.in_flight = false;
                    self.find_state.error = Some(error);
                }
            }
            Msg::SearchStep(previous) => self.navigate_search(previous, false, cx),
            Msg::SearchNavigated(token, id, valid) => {
                if self.find_state.pane == Some(id)
                    && self.find_state.pending_navigation.as_ref().is_some_and(|(pending, _, _)| *pending == token)
                {
                    let (_, results, index) = self.find_state.pending_navigation.take().unwrap();
                    if valid {
                        self.find_state.copy = Some((results, index));
                    } else {
                        self.find_state.copy = None;
                        self.find_state.retry_navigation = true;
                        self.find_state.dirty = true;
                        self.schedule_search(cx);
                    }
                }
            }
            Msg::CloseFind => {
                self.find = false;
                self.search.clear();
                self.shape_all();
                self.focus_active(cx);
            }
            Msg::FontSize(delta) => {
                // ❖+ and ❖− zoom for now, as other terminals do; Actual Size (❖0)
                // comes back to the size Settings keeps.
                self.font_size = (self.font_size + f64::from(delta)).clamp(8.0, 72.0);
                self.shape_all();
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
        self.sync_search(cx);
    }
    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.settings_window {
            return crate::preferences::view(self);
        }
        let mut tabs: Vec<Element<Msg>> = self
            .tabs
            .iter()
            .map(|tab| {
                let title =
                    self.panes.iter().find(|pane| pane.id == tab.active).map_or("Shell", |pane| pane.title.as_str());
                let bell = tab.tree.leaves().iter().any(|id| self.panes.iter().any(|pane| pane.id == *id && pane.bell));
                button(if bell { format!("• {title}") } else { title.to_owned() })
                    .toolbar()
                    .selected(tab.id == self.active)
                    .on_press(Msg::SelectTab(tab.id))
                    .max_width(240.0)
                    .key(tab.id)
                    .into()
            })
            .collect();
        tabs.push(spacer().into());
        tabs.push(
            icon_button(Icon::Plus).toolbar().on_press(Msg::NewTab).tooltip(self.tip("New Tab", Action::NewTab)).into(),
        );
        tabs.push(
            icon_button(Icon::Search).toolbar().on_press(Msg::Find).tooltip(self.tip("Find", Action::Find)).into(),
        );
        let mut contents: Vec<Element<Msg>> = vec![toolbar(tabs)];
        if self.find {
            contents.push(
                row![
                    search_field(&self.search)
                        .on_change(Msg::Search)
                        .on_submit(Msg::SearchStep(false))
                        .id("find")
                        .width(Fill),
                    status_text(self.search_status()),
                    button("Previous")
                        .on_press(Msg::SearchStep(true))
                        .tooltip(self.tip("Previous match", Action::FindPrevious)),
                    button("Next").on_press(Msg::SearchStep(false)).tooltip(self.tip("Next match", Action::FindNext)),
                    button("Done").on_press(Msg::CloseFind)
                ]
                .padding([6.0, 10.0])
                .spacing(8.0)
                .into(),
            );
        }
        if let Some(tab) = self.active_tab() {
            // Zoomed, the active pane stands alone; its splits wait in the tree.
            contents.push(if tab.zoomed {
                self.pane_tree_view(&PaneTree::Leaf(tab.active))
            } else {
                self.pane_tree_view(&tab.tree)
            });
        }
        if let Some(pane) = self.active_pane() {
            let mut bar = vec![
                status_text(pane.hovered_link.as_ref().map_or(pane.status.as_str(), |link| link.target.as_ref())),
                spacer().into(),
            ];
            if let Some(tab) = self.active_tab().filter(|tab| tab.zoomed) {
                let count = tab.tree.leaves().len();
                bar.push(status_text(format!("Zoomed: 1 of {count} panes")));
            }
            if (self.font_size - self.settings.font_size).abs() > f64::EPSILON {
                bar.push(status_text(format!("{} pt", self.font_size)));
            }
            bar.push(status_text(format!("{} × {}", pane.snapshot.cols, pane.snapshot.rows)));
            contents.push(status_bar(bar));
        }
        aizome_kit::widgets::layout::column(contents).width(Fill).height(Fill).into()
    }
    fn menus(&self) -> MenuBar<Msg> {
        let selected = self.find_state.copy.is_some()
            || self.active_pane().is_some_and(|t| t.selection.is_some_and(|s| !s.is_empty()));
        let selection = self.active_pane().is_some_and(|t| t.selection.is_some_and(|s| !s.is_empty()));
        let tab = self.active_tab();
        let split = tab.is_some_and(|tab| matches!(tab.tree, PaneTree::Split { .. }));
        let zoomed = tab.is_some_and(|tab| tab.zoomed);
        let several = self.tabs.len() > 1;
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
        let searching = self.find && !self.search.is_empty();
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
                    .enabled((self.font_size - self.settings.font_size).abs() > f64::EPSILON),
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
        let last = self.tabs.len().saturating_sub(1);
        for (index, tab) in self.tabs.iter().enumerate() {
            let title =
                self.panes.iter().find(|pane| pane.id == tab.active).map_or("Shell", |pane| pane.title.as_str());
            let title: String = title.chars().take(48).collect();
            let shortcut = match Action::SELECT_TAB.get(index) {
                Some(action) => self.bindings.shortcut(*action),
                None if index == last => self.bindings.shortcut(Action::LastTab),
                None => None,
            };
            window = window.item(
                self.keyed(MenuItem::new(title, Msg::SelectTab(tab.id)).checked(tab.id == self.active), shortcut),
            );
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
        self.cancel_automatic();
        self.automatic_worker = None;
        self.resolver_worker = None;
        if self.settings_pending {
            self.closing = true;
            return false;
        }
        self.close_settings(cx);
        self.clipboard_request();
        self.clipboard = None;
        self.tabs.clear();
        self.panes.clear();
        true
    }
}

impl Drop for Hotaru {
    fn drop(&mut self) {
        self.clipboard_cancel.store(true, Ordering::Release);
        if let Some(read) = &self.clipboard_read {
            read.cancel();
        }
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
