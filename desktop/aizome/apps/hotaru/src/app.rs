use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aizome_kit::prelude::*;
use aizome_kit::{ClipboardRead, PreparedClipboard};
use hotaru_core::{ContentSnapshot, Snapshot, Terminal, TerminalColors};
use hotaru_io::PtyCommand;

use crate::clipboard::{ClipboardWorker, CopySource, Job};
use crate::panes::{Axis, Direction, PaneTree};
use crate::session::{Command, Session};
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
    ConfigLoaded(u64, Result<Settings, String>),
    ConfigSaved(u64, Result<(), String>),
    Frame(u64, Arc<Snapshot>),
    ShapeScale(u64, f64),
    Shaped(u64, Arc<crate::font::Frame>),
    FontFailed(u64, u64, String),
    Title(u64, String),
    Exited(u64, String),
    Failed(u64, String),
    Notice(u64, String),
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
    pub snapshot: Arc<Snapshot>,
    pub session: Option<Session>,
    pub selection: Option<Selection>,
    pub status: String,
    pub ended: bool,
    pub bell: bool,
    pub rect: Rect,
    pub(crate) rendered: Option<Arc<crate::font::Frame>>,
    font_worker: Option<crate::font::Worker>,
    font_epoch: u64,
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
}

pub struct Hotaru {
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub active: u64,
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
}

impl Hotaru {
    pub fn new(command: PtyCommand) -> Self {
        Self {
            tabs: Vec::new(),
            panes: Vec::new(),
            active: 0,
            font_size: 14.0,
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

    fn new_pane(&mut self, cx: &mut Cx<Msg>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut terminal = Terminal::new(80, 24, self.settings.scrollback);
        terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
        let colors = colors_for(cx.theme());
        terminal.set_default_colors(colors.clone());
        if self.demo {
            terminal.feed(b"\x1b[1;36mHotaru\x1b[0m  \xe8\x9b\x8d\r\n\r\nA little light for your work.\r\n\r\n\x1b[32mcallen\x1b[0m \x1b[34m~/work/sekka\x1b[0m\r\n\xe2\x9d\xaf cargo test\r\n\x1b[32m   Compiling\x1b[0m hotaru-core\r\n\x1b[32m    Finished\x1b[0m test profile\r\n\r\n\x1b[1mtest result:\x1b[0m ok\r\n\r\n\xe2\x9d\xaf ");
        }
        let (session, status) = if self.demo {
            (None, "Preview".into())
        } else {
            match Session::spawn(id, self.command.clone(), colors, self.settings.scrollback, cx.stream()) {
                Ok(session) => (Some(session), "Shell".into()),
                Err(e) => (None, format!("Could not start shell: {e}")),
            }
        };
        self.panes.push(Pane {
            id,
            title: "Shell".into(),
            snapshot: Arc::new(terminal.snapshot()),
            session,
            selection: None,
            status,
            ended: false,
            bell: false,
            rect: Rect::ZERO,
            rendered: None,
            font_worker: crate::font::Worker::new(id, cx.stream())
                .map_err(|error| log::warn!("Font worker: {error}"))
                .ok(),
            font_epoch: 0,
            font_scale: 1.0,
        });
        self.shape_pane(id, false);
        id
    }

    fn new_tab(&mut self, cx: &mut Cx<Msg>) {
        if self.panes.len() >= 64 {
            if let Some(id) = self.active_pane().map(|pane| pane.id) {
                self.notice(id, "The 64 pane limit has been reached.");
            }
            return;
        }
        let id = self.new_pane(cx);
        self.tabs.push(Tab { id, tree: PaneTree::Leaf(id), active: id });
        self.active = id;
        self.focus_after_rebuild(cx);
    }

    fn split_pane(&mut self, axis: Axis, cx: &mut Cx<Msg>) {
        let Some(target) = self.active_pane().map(|pane| pane.id) else { return };
        if self.panes.len() >= 64 {
            self.notice(target, "The 64 pane limit has been reached.");
            return;
        }
        let id = self.new_pane(cx);
        let split = self.next_id;
        self.next_id += 1;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == self.active) {
            tab.tree.split(target, id, split, axis);
            tab.active = id;
        }
        self.focus_after_rebuild(cx);
    }

    fn close_tab(&mut self, cx: &mut Cx<Msg>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == self.active) else { return };
        self.clipboard_request();
        let removed = self.tabs.remove(index).tree.leaves();
        self.panes.retain(|pane| !removed.contains(&pane.id));
        if self.tabs.is_empty() {
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
        self.panes.retain(|pane| pane.id != id);
        self.focus_after_rebuild(cx);
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
            tab.active = id;
            if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                pane.bell = false;
            }
            self.focus_active(cx);
        }
    }

    fn send(&mut self, id: u64, command: Command) {
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
        self.font_size = settings.font_size;
        self.bindings = Arc::new(Bindings::new(&settings));
        self.settings = settings;
        self.apply_appearance(cx);
        self.shape_all();
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
        if let Some(worker) = &self.settings_worker {
            self.settings_pending = true;
            worker.save(self.settings_token, self.settings.clone());
        }
    }

    fn action(&mut self, action: Action, cx: &mut Cx<Msg>) {
        let message = match action {
            Action::Ignore => return,
            Action::NewTab => Msg::NewTab,
            Action::ClosePane => Msg::ClosePane,
            Action::CloseTab => Msg::CloseTab,
            Action::SplitRight => Msg::Split(Axis::Right),
            Action::SplitDown => Msg::Split(Axis::Down),
            Action::PreviousPane => Msg::MoveFocus(Direction::Previous),
            Action::NextPane => Msg::MoveFocus(Direction::Next),
            Action::FocusLeft => Msg::MoveFocus(Direction::Left),
            Action::FocusRight => Msg::MoveFocus(Direction::Right),
            Action::FocusUp => Msg::MoveFocus(Direction::Up),
            Action::FocusDown => Msg::MoveFocus(Direction::Down),
            Action::Copy => Msg::Copy,
            Action::Paste => Msg::Paste,
            Action::CopyAll => Msg::CopyAll,
            Action::Find => Msg::Find,
            Action::FindNext => Msg::SearchStep(false),
            Action::FindPrevious => Msg::SearchStep(true),
            Action::LargerText => Msg::FontSize(1),
            Action::SmallerText => Msg::FontSize(-1),
            Action::ActualSize => {
                self.font_size = 14.0;
                self.settings.font_size = 14.0;
                self.save_settings();
                self.shape_all();
                return;
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

    fn item(&self, label: &str, action: Action) -> MenuItem<Msg> {
        let item = MenuItem::new(label, Msg::Action(action));
        if let Some(shortcut) = self.bindings.shortcut(action) { item.shortcut(shortcut) } else { item }
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
        self.new_tab(cx);
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
            Msg::ConfigLoaded(token, result) => {
                if token != self.settings_token {
                    return;
                }
                self.settings_pending = false;
                match result {
                    Ok(settings) => self.apply_settings(settings, cx),
                    Err(error) => {
                        if let Some(id) = self.active_pane().map(|pane| pane.id) {
                            self.notice(id, &format!("Configuration not loaded: {error}"));
                        }
                    }
                }
            }
            Msg::ConfigSaved(token, result) => {
                if token != self.settings_token {
                    return;
                }
                self.settings_pending = false;
                if let Err(error) = result {
                    log::warn!("Configuration not saved: {error}");
                    if let Some(id) = self.active_pane().map(|pane| pane.id) {
                        self.notice(id, &format!("Configuration not saved: {error}"));
                    }
                }
                if self.closing {
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
            Msg::NewTab => self.new_tab(cx),
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
                self.new_tab(cx);
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
                            "terminal font {:?}, scale {}, grid {}x{} px, unavailable families {:?}",
                            frame.primary_family,
                            frame.metrics.scale,
                            frame.metrics.cell_width_px,
                            frame.metrics.cell_height_px,
                            frame.missing_families
                        );
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
            Msg::SearchReady(token, id, navigation, mut cache) => {
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
                    if let Some(pane) = self.active_pane() {
                        cache.selected = cache.selected.filter(|&index| {
                            cache
                                .results
                                .resolve_match(index, &pane.snapshot.position, pane.snapshot.generation)
                                .is_some()
                        });
                    }
                    if self.find_state.copy.is_some() {
                        self.find_state.copy = cache.selected.map(|index| (cache.results.clone(), index));
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
                self.font_size = (self.font_size + f64::from(delta)).clamp(8.0, 72.0);
                if delta != 0 {
                    self.settings.font_size = self.font_size;
                    self.save_settings();
                }
                self.shape_all();
            }
        }
        self.sync_search(cx);
    }
    fn view(&self, _window: WindowKey) -> Element<Msg> {
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
        tabs.push(icon_button(Icon::Plus).toolbar().on_press(Msg::NewTab).tooltip("New Tab · ❖T").into());
        tabs.push(icon_button(Icon::Search).toolbar().on_press(Msg::Find).tooltip("Find · ❖F").into());
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
                    button("Previous").on_press(Msg::SearchStep(true)).tooltip("Previous match · ❖⇧G"),
                    button("Next").on_press(Msg::SearchStep(false)).tooltip("Next match · ❖G"),
                    button("Done").on_press(Msg::CloseFind)
                ]
                .padding([6.0, 10.0])
                .spacing(8.0)
                .into(),
            );
        }
        if let Some(tab) = self.active_tab() {
            contents.push(self.pane_tree_view(&tab.tree));
        }
        if let Some(tab) = self.active_pane() {
            contents.push(status_bar(vec![
                status_text(tab.status.as_str()),
                spacer().into(),
                status_text(format!("{} × {}", tab.snapshot.cols, tab.snapshot.rows)),
            ]));
        }
        aizome_kit::widgets::layout::column(contents).width(Fill).height(Fill).into()
    }
    fn menus(&self) -> MenuBar<Msg> {
        let selected = self.find_state.copy.is_some()
            || self.active_pane().is_some_and(|t| t.selection.is_some_and(|s| !s.is_empty()));
        let file = Menu::new("File")
            .item(self.item("New Tab", Action::NewTab))
            .item(self.item("Split Right", Action::SplitRight))
            .item(self.item("Split Down", Action::SplitDown))
            .separator()
            .item(self.item("Close Pane", Action::ClosePane))
            .item(self.item("Close Tab", Action::CloseTab))
            .separator()
            .item(self.item("Reload Configuration", Action::ReloadConfig).enabled(self.settings_path.is_some()));
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
        let edit = Menu::new("Edit")
            .item(self.item("Copy", Action::Copy).enabled(selected))
            .item(self.item("Paste", Action::Paste))
            .item(MenuItem::new("Copy Visible Screen", Msg::CopyScreen))
            .item(self.item("Copy All Output", Action::CopyAll))
            .submenu(copy_format)
            .separator()
            .item(self.item("Find…", Action::Find))
            .item(self.item("Find Next", Action::FindNext).enabled(self.find && !self.search.is_empty()))
            .item(self.item("Find Previous", Action::FindPrevious).enabled(self.find && !self.search.is_empty()));
        let appearance = Menu::new("Appearance")
            .item(
                MenuItem::new("Follow Desktop", Msg::SetTheme(settings::Theme::Desktop))
                    .checked(self.settings.theme == settings::Theme::Desktop),
            )
            .item(
                MenuItem::new("Night", Msg::SetTheme(settings::Theme::Night))
                    .checked(self.settings.theme == settings::Theme::Night),
            )
            .item(
                MenuItem::new("Day", Msg::SetTheme(settings::Theme::Day))
                    .checked(self.settings.theme == settings::Theme::Day),
            )
            .item(
                MenuItem::new("High Contrast", Msg::SetTheme(settings::Theme::HighContrast))
                    .checked(self.settings.theme == settings::Theme::HighContrast),
            );
        let weight = Menu::new("Text Weight")
            .item(MenuItem::new("Regular", Msg::SetWeight(400)).checked(self.settings.font_weight == 400))
            .item(MenuItem::new("Medium", Msg::SetWeight(500)).checked(self.settings.font_weight == 500))
            .item(MenuItem::new("Semibold", Msg::SetWeight(600)).checked(self.settings.font_weight == 600));
        let view = Menu::new("View")
            .item(self.item("Previous Pane", Action::PreviousPane))
            .item(self.item("Next Pane", Action::NextPane))
            .item(self.item("Focus Left", Action::FocusLeft))
            .item(self.item("Focus Right", Action::FocusRight))
            .item(self.item("Focus Up", Action::FocusUp))
            .item(self.item("Focus Down", Action::FocusDown))
            .separator()
            .item(self.item("Larger Text", Action::LargerText))
            .item(self.item("Smaller Text", Action::SmallerText))
            .item(self.item("Actual Size", Action::ActualSize))
            .submenu(weight)
            .submenu(appearance);
        MenuBar::standard(&self.info()).with_file_menu(file).replace("Edit", edit).with_menu(view)
    }
    fn close_requested(&mut self, _window: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        if self.settings_pending {
            self.closing = true;
            return false;
        }
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
