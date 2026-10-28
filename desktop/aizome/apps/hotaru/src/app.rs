use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::{ClipboardRead, PreparedClipboardText};
use hotaru_core::{Snapshot, Terminal, TerminalColors};
use hotaru_io::PtyCommand;

use crate::clipboard::{ClipboardWorker, Job};
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
    ConfigLoaded(u64, Result<Settings, String>),
    ConfigSaved(u64, Result<(), String>),
    Frame(u64, Arc<Snapshot>),
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
    Scroll(u64, i32),
    Select(u64, Selection),
    Copy,
    CopyScreen,
    CopyAll,
    CopySnapshot(u64, Arc<Snapshot>),
    CopyReady(u64, PreparedClipboardText),
    PasteReady(u64, u64, ClipboardRead, Option<Arc<str>>),
    Paste,
    Find,
    Search(String),
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
    window: WindowKey,
    next_id: u64,
    command: PtyCommand,
    demo: bool,
    pub instance: Option<crate::instance::Instance>,
    clipboard: Option<ClipboardWorker>,
    clipboard_token: u64,
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
            window: WindowKey::NONE,
            next_id: 1,
            command,
            demo: false,
            instance: None,
            clipboard: None,
            clipboard_token: 0,
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
        });
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

    fn clipboard_request(&mut self) -> u64 {
        if let Some(read) = self.clipboard_read.take() {
            read.cancel();
        }
        self.clipboard_token = self.clipboard_token.wrapping_add(1);
        self.clipboard_token
    }

    fn copy(&mut self, selected: bool) {
        let Some(tab) = self.active_pane() else { return };
        let selection = if selected {
            let Some(selection) = tab.selection.filter(|s| !s.is_empty()) else { return };
            Some(selection)
        } else {
            None
        };
        let snapshot = tab.snapshot.clone();
        let token = self.clipboard_request();
        if let Some(worker) = &self.clipboard {
            worker.submit(Job::Copy { token, snapshot, selection });
        }
    }

    fn pane_tree_view(&self, tree: &PaneTree) -> Element<Msg> {
        match tree {
            PaneTree::Leaf(id) => {
                let pane = self.panes.iter().find(|pane| pane.id == *id).expect("split leaf owns a pane");
                terminal_view(
                    pane.id,
                    pane.snapshot.clone(),
                    self.font_size,
                    self.settings.font_weight,
                    pane.selection,
                    self.search.clone(),
                    self.bindings.clone(),
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
            Action::LargerText => Msg::FontSize(1),
            Action::SmallerText => Msg::FontSize(-1),
            Action::ActualSize => {
                self.font_size = 14.0;
                self.settings.font_size = 14.0;
                self.save_settings();
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
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    if let Some(session) = &tab.session {
                        session.received_frame();
                    }
                    if (tab.snapshot.cols, tab.snapshot.rows) != (frame.cols, frame.rows) {
                        tab.selection = None;
                    }
                    tab.snapshot = frame;
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
            }
            Msg::Key(id, event) => {
                if let Some(pane) = self.panes.iter_mut().find(|pane| pane.id == id) {
                    pane.selection = None;
                }
                self.send(id, Command::Key(event));
            }
            Msg::PasteText(id, text) => self.send(id, Command::Paste(text)),
            Msg::Mouse(id, event) => self.send(id, Command::Mouse(event)),
            Msg::Wheel(id, event, lines) => self.send(id, Command::Wheel(event, lines)),
            Msg::TerminalFocus(id, focused) => self.send(id, Command::Focus(focused)),
            Msg::Resize(id, cols, rows) => self.send(id, Command::Resize { cols, rows }),
            Msg::Scroll(id, lines) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.selection = None;
                }
                self.send(id, Command::Scroll(lines));
            }
            Msg::Select(id, selection) => {
                if let Some(tab) = self.panes.iter_mut().find(|t| t.id == id) {
                    tab.selection = Some(selection);
                }
            }
            Msg::Copy => self.copy(true),
            Msg::CopyScreen => self.copy(false),
            Msg::CopyAll => {
                let Some(id) = self.active_pane().map(|pane| pane.id) else { return };
                let token = self.clipboard_request();
                self.send(id, Command::CopyAll(token));
            }
            Msg::CopySnapshot(token, snapshot) => {
                if token == self.clipboard_token
                    && let Some(worker) = &self.clipboard
                {
                    worker.submit(Job::Copy { token, snapshot, selection: None });
                }
            }
            Msg::CopyReady(token, prepared) => {
                if token == self.clipboard_token {
                    cx.set_prepared_clipboard_text(prepared);
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
            Msg::Search(search) => self.search = search,
            Msg::CloseFind => {
                self.find = false;
                self.search.clear();
                self.focus_active(cx);
            }
            Msg::FontSize(delta) => {
                self.font_size = (self.font_size + f64::from(delta)).clamp(8.0, 72.0);
                if delta != 0 {
                    self.settings.font_size = self.font_size;
                    self.save_settings();
                }
            }
        }
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
                    search_field(&self.search).on_change(Msg::Search).id("find").width(Fill),
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
        let selected = self.active_pane().is_some_and(|t| t.selection.is_some_and(|s| !s.is_empty()));
        let file = Menu::new("File")
            .item(self.item("New Tab", Action::NewTab))
            .item(self.item("Split Right", Action::SplitRight))
            .item(self.item("Split Down", Action::SplitDown))
            .separator()
            .item(self.item("Close Pane", Action::ClosePane))
            .item(self.item("Close Tab", Action::CloseTab))
            .separator()
            .item(self.item("Reload Configuration", Action::ReloadConfig).enabled(self.settings_path.is_some()));
        let edit = Menu::new("Edit")
            .item(self.item("Copy", Action::Copy).enabled(selected))
            .item(self.item("Paste", Action::Paste))
            .item(MenuItem::new("Copy Visible Screen", Msg::CopyScreen))
            .item(self.item("Copy All Output", Action::CopyAll))
            .separator()
            .item(self.item("Find…", Action::Find));
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
        if let Some(read) = &self.clipboard_read {
            read.cancel();
        }
    }
}
