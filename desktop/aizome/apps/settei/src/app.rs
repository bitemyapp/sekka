//! The window: the sidebar (search, the user's card, the panes in groups)
//! and the chosen pane on the right, with its title strip and the status
//! strip under it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::text::Span;
use aizome_kit::{column, row};

use crate::form::PAGE_WIDTH;
use crate::pane::{AnyPane, Group, PaneMsg, SettingEntry, ViewCx, entry_name};
use crate::search::{self, Hit, PaneInfo};
use crate::shared::Shared;
use crate::widgets::{avatar, press, tile};

/// The sidebar's width.
pub const SIDEBAR: f64 = 232.0;
/// How long a revealed option stays highlighted.
const REVEAL_FOR: Duration = Duration::from_millis(2600);

/// The window's messages; a pane's own travel as [`AppMsg::Pane`].
#[derive(Clone)]
pub enum AppMsg {
    /// The pane at this index in the sidebar.
    Show(usize),
    /// A pane by id, and an option in it to reveal.
    Open(String, Option<String>),
    Search(String),
    /// Return in the search field: open the best result.
    SearchSubmit,
    Back,
    Forward,
    FocusSearch,
    Pane(&'static str, PaneMsg),
    /// An `aizome-sys` service changed.
    SystemChanged,
    /// `settings.toml` changed on disk.
    SettingsOnDisk,
    /// Ask the compositor to apply the settings.
    Reload,
    ClearStatus(u64),
    Said(String, bool),
    Unreveal(u64),
    OpenSheet(&'static str),
    CloseSheet,
    /// A second launch's arguments.
    Launched(Vec<String>),
    /// A change reached settings.toml.
    Written,
    Nothing,
}

/// Where to start: a pane and an option in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Start {
    pub pane: Option<String>,
    pub entry: Option<String>,
    /// The keyboard to choose in the setup sheet (`--keyboard`, as the
    /// shell's "Set up …?" names it): “Name (vendor:product)”, or either.
    pub keyboard: Option<String>,
}

impl Start {
    /// `keyboard`, `keyboard.caps-lock` or `keyboard/caps-lock`.
    pub fn parse(target: &str) -> Start {
        let (pane, entry) = match target.split_once(['.', '/', ':']) {
            Some((p, e)) if !e.is_empty() => (p, Some(e.to_owned())),
            _ => (target.trim_end_matches(['.', '/', ':']), None),
        };
        Start { pane: Some(pane.to_owned()), entry, keyboard: None }
    }

    /// From command-line arguments: `--pane ID` or `--pane=ID`, and
    /// `--keyboard NAME` or `--keyboard=NAME` (the first of each).
    pub fn from_args(args: &[String]) -> Start {
        let value = |flag: &str| {
            let mut it = args.iter();
            while let Some(a) = it.next() {
                if a == flag {
                    return it.next().cloned();
                }
                if let Some(v) = a.strip_prefix(flag).and_then(|v| v.strip_prefix('=')) {
                    return Some(v.to_owned());
                }
            }
            None
        };
        let start = value("--pane").map_or_else(Start::default, |p| Start::parse(&p));
        Start { keyboard: value("--keyboard"), ..start }
    }
}

pub struct Settei {
    pub panes: Vec<Box<dyn AnyPane>>,
    /// Each pane's entries, read once.
    entries: Vec<Vec<SettingEntry>>,
    pub current: usize,
    back: Vec<usize>,
    forward: Vec<usize>,
    pub query: String,
    pub hits: Vec<Hit>,
    matched: HashMap<&'static str, HashSet<String>>,
    pub revealed: Option<(&'static str, String)>,
    reveal_generation: u64,
    sheet: Option<(&'static str, WindowKey)>,
    pub shared: Shared,
    start: Start,
    /// The socket later launches hand their arguments to.
    pub listener: Option<std::os::unix::net::UnixListener>,
    picture: Option<Image>,
    pictures: crate::widgets::Pictures,
    watcher: Option<aizome_config::Watcher>,
    started: bool,
}

impl Settei {
    /// The window over `panes`, with settings at `settings_path`.
    pub fn new(panes: Vec<Box<dyn AnyPane>>, settings_path: PathBuf, sample: bool, start: Start) -> Settei {
        let entries = panes.iter().map(|p| p.entries()).collect();
        Settei {
            panes,
            entries,
            current: 0,
            back: Vec::new(),
            forward: Vec::new(),
            query: String::new(),
            hits: Vec::new(),
            matched: HashMap::new(),
            revealed: None,
            reveal_generation: 0,
            sheet: None,
            shared: Shared::new(settings_path, sample),
            start,
            listener: None,
            picture: None,
            pictures: Default::default(),
            watcher: None,
            started: false,
        }
    }

    /// Every pane, live, at the user's settings.
    pub fn live(start: Start) -> Settei {
        Settei::new(crate::panes::all(), aizome_config::Settings::path(), false, start)
    }

    /// Every pane over the sample system, with settings at `settings_path`.
    pub fn sample(settings_path: PathBuf, start: Start) -> Settei {
        Settei::new(crate::panes::all(), settings_path, true, start)
    }

    /// The pane with this id (or a name it's known by).
    pub fn find_pane(&self, id: &str) -> Option<usize> {
        self.target(id, None).map(|(i, _)| i)
    }

    /// A pane id or a name it's known by, and an option, as the pane and
    /// option to open.
    fn target(&self, id: &str, entry: Option<&str>) -> Option<(usize, Option<String>)> {
        let id = id.trim().to_lowercase();
        if let Some(i) = self.panes.iter().position(|p| p.id() == id) {
            return Some((i, entry.map(str::to_owned)));
        }
        let (pane, alias_entry) = crate::panes::resolve(&id);
        let i = self.panes.iter().position(|p| p.id() == pane)?;
        Some((i, entry.map(str::to_owned).or(alias_entry)))
    }

    /// The sheet showing, if one is.
    pub fn sheet_window(&self) -> Option<WindowKey> {
        self.sheet.map(|(_, w)| w)
    }

    pub fn pane_id(&self) -> &'static str {
        self.panes[self.current].id()
    }

    fn vcx<'a>(&'a self, pane: usize, empty: &'a HashSet<String>) -> ViewCx<'a> {
        let id = self.panes[pane].id();
        ViewCx {
            shared: &self.shared,
            pane: id,
            matched: self.matched.get(id).unwrap_or(empty),
            revealed: self.revealed.as_ref().filter(|(p, _)| *p == id).map(|(_, e)| e.as_str()),
        }
    }

    // Moving between panes.

    fn go(&mut self, to: usize, cx: &mut Cx<AppMsg>, record: bool) {
        if to >= self.panes.len() {
            return;
        }
        if to != self.current || !self.started {
            if self.started {
                self.panes[self.current].hidden_any(cx, &mut self.shared);
                if record {
                    self.back.push(self.current);
                    self.forward.clear();
                }
            }
            self.started = true;
            self.current = to;
            self.panes[to].shown_any(cx, &mut self.shared);
        }
        let title = self.panes[to].title();
        cx.set_title(self.shared.window, title);
    }

    fn open(&mut self, id: &str, entry: Option<&str>, cx: &mut Cx<AppMsg>) {
        let Some((i, entry)) = self.target(id, entry) else {
            self.shared.say(format!("There's no pane called “{id}”"), true, cx);
            return;
        };
        self.go(i, cx, true);
        if let Some(e) = entry {
            self.reveal(i, &e, cx);
        }
    }

    /// Show option `entry` of pane `i`: its sub-page, its row scrolled into
    /// view and highlighted for a moment.
    fn reveal(&mut self, i: usize, entry: &str, cx: &mut Cx<AppMsg>) {
        let pane = self.panes[i].id();
        if !self.entries[i].iter().any(|e| e.id == entry) {
            log::info!("{pane} has no option {entry}");
            return;
        }
        self.panes[i].focus_entry_any(entry, cx, &mut self.shared);
        self.revealed = Some((pane, entry.to_owned()));
        self.reveal_generation += 1;
        cx.after(REVEAL_FOR, AppMsg::Unreveal(self.reveal_generation));
        cx.focus(&entry_name(pane, entry));
    }

    fn search(&mut self, query: String) {
        self.query = query;
        let infos: Vec<PaneInfo> = self
            .panes
            .iter()
            .zip(&self.entries)
            .map(|(p, e)| PaneInfo { title: p.title(), keywords: p.keywords(), entries: e })
            .collect();
        self.hits = search::find(&self.query, &infos);
        self.matched.clear();
        for h in &self.hits {
            let id = self.panes[h.pane].id();
            self.matched.insert(id, h.entries.iter().map(|e| e.id.clone()).collect());
        }
    }

    fn refresh_picture(&mut self) {
        let me = self.shared.sys.accounts.me();
        self.picture = me.picture.and_then(|path| self.pictures.get(&path));
    }

    // The view.

    fn sidebar(&self) -> Element<AppMsg> {
        let search = search_field(&self.query)
            .placeholder("Search")
            .on_change(AppMsg::Search)
            .on_submit(AppMsg::SearchSubmit)
            .id("search")
            .width(Fill);
        let list: Element<AppMsg> = if self.query.trim().is_empty() { self.pane_list() } else { self.results() };
        let body = column![
            container(search).padding([12.0, 12.0, 8.0, 12.0]).width(Fill),
            self.user_card(),
            divider().edge(Edge::Line),
            scroll(container(list).padding([8.0, 8.0, 12.0, 8.0]).width(Fill)).key("sidebar").width(Fill).height(Fill),
        ]
        .width(SIDEBAR)
        .height(Fill);
        container(body).fill(Tone::Chrome).edges(Edge::BevelLo, Sides::RIGHT).height(Fill).into()
    }

    fn user_card(&self) -> Element<AppMsg> {
        let me = self.shared.sys.accounts.me();
        let picture = self.picture.clone();
        let name = if me.display_name().is_empty() { "You".to_owned() } else { me.display_name().to_owned() };
        let selected = self.pane_id() == "users" && self.query.is_empty();
        let line = row![
            avatar(picture, &me.initials(), &me.login, 32.0),
            column![text(name).weight(650.0), text("Account").size(11.0).role(Role::Text3)].spacing(2.0),
        ]
        .spacing(10.0)
        .align(Align::Center)
        .width(Fill);
        let users = self.find_pane("users");
        container(
            press(line)
                .padding([6.0, 8.0])
                .selected(selected)
                .on_press_maybe(users.map(AppMsg::Show))
                .named("Your account")
                .width(Fill),
        )
        .padding([0.0, 8.0, 8.0, 8.0])
        .width(Fill)
        .into()
    }

    fn pane_row(&self, i: usize, title: Element<AppMsg>, selected: bool) -> Element<AppMsg> {
        let p = &self.panes[i];
        let line =
            row![tile(p.icon(), p.tile(), 20.0), title].spacing(10.0).align(Align::Center).height(28.0).width(Fill);
        press(line).padding([0.0, 8.0]).selected(selected).on_press(AppMsg::Show(i)).named(p.title()).key(p.id()).into()
    }

    fn pane_list(&self) -> Element<AppMsg> {
        let empty = HashSet::new();
        let mut col = column![].spacing(1.0).width(Fill);
        let mut first = true;
        for g in Group::ALL {
            let members: Vec<usize> = (0..self.panes.len())
                .filter(|&i| self.panes[i].group() == g && self.panes[i].listed_any(&self.vcx(i, &empty)))
                .collect();
            if members.is_empty() {
                continue;
            }
            if !first {
                col = col.push(space(10.0));
            }
            first = false;
            for i in members {
                let t = text(self.panes[i].title()).into();
                col = col.push(self.pane_row(i, t, i == self.current));
            }
        }
        col.into()
    }

    fn results(&self) -> Element<AppMsg> {
        if self.hits.is_empty() {
            return container(text("No Results").role(Role::Text3))
                .padding([24.0, 8.0])
                .width(Fill)
                .align_x(Align::Center)
                .into();
        }
        let mut col = column![].spacing(1.0).width(Fill);
        for h in &self.hits {
            let p = &self.panes[h.pane];
            col = col.push(self.pane_row(h.pane, highlighted(p.title(), &h.ranges, 13.0), h.pane == self.current));
            for e in &h.entries {
                let line = container(highlighted(&e.title, &e.ranges, 12.0))
                    .padding([0.0, 0.0, 0.0, 30.0])
                    .height(24.0)
                    .align_y(Align::Center)
                    .width(Fill);
                col = col.push(
                    press(line)
                        .padding([0.0, 8.0])
                        .on_press(AppMsg::Open(p.id().to_owned(), Some(e.id.clone())))
                        .named(e.title.clone())
                        .key((p.id(), e.id.as_str())),
                );
            }
            col = col.push(space(4.0));
        }
        col.into()
    }

    fn title_strip(&self) -> Element<AppMsg> {
        let empty = HashSet::new();
        let p = &self.panes[self.current];
        let crumb = p.crumb_any();
        let can_back = !self.back.is_empty() || crumb.is_some();
        let mut bar = row![
            icon_button(Icon::ChevronLeft).toolbar().enabled(can_back).on_press(AppMsg::Back).tooltip("Back"),
            icon_button(Icon::ChevronRight)
                .toolbar()
                .enabled(!self.forward.is_empty())
                .on_press(AppMsg::Forward)
                .tooltip("Forward"),
            space(4.0),
            tile(p.icon(), p.tile(), 20.0),
        ]
        .spacing(6.0)
        .align(Align::Center)
        .padding([0.0, 12.0, 0.0, 8.0])
        .height(44.0)
        .width(Fill);
        bar = match crumb {
            None => bar.push(text(p.title()).size(15.0).weight(650.0)),
            Some(c) => bar
                .push(text(p.title()).size(15.0).role(Role::Text3))
                .push(icon(Icon::ChevronRight).size(12.0).role(Role::Text3))
                .push(text(c).size(15.0).weight(650.0)),
        };
        bar = bar.push(spacer());
        if let Some(t) = p.toolbar_any(&self.vcx(self.current, &empty)) {
            bar = bar.push(t);
        }
        container(bar).fill(Tone::Surface).edges(Edge::Line, Sides::BOTTOM).width(Fill).into()
    }

    fn status_strip(&self) -> Element<AppMsg> {
        let mut line = row![].spacing(8.0).align(Align::Center).padding([0.0, 12.0]).height(24.0).width(Fill);
        match &self.shared.status {
            Some(s) => {
                let (glyph, role) =
                    if s.error { (Icon::CircleAlert, Role::Danger) } else { (Icon::CircleCheck, Role::Success) };
                line = line
                    .push(icon(glyph).size(12.0).role(role))
                    .push(text(s.text.clone()).size(11.0).role(if s.error { Role::Danger } else { Role::Text2 }));
            }
            None => {
                line = line.push(
                    text(short_path(&self.shared.settings_path))
                        .mono()
                        .size(10.5)
                        .role(Role::Faint)
                        .tooltip(self.shared.settings_path.display().to_string()),
                );
            }
        }
        line = line.push(spacer());
        if self.shared.sample {
            line = line.push(badge("Sample system").role(Role::Warning));
        }
        container(line).fill(Tone::Chrome).edges(Edge::Line, Sides::TOP).width(Fill).into()
    }

    fn main_view(&self) -> Element<AppMsg> {
        let empty = HashSet::new();
        let vcx = self.vcx(self.current, &empty);
        let content = self.panes[self.current].view_any(&vcx);
        let page = container(container(content).max_width(PAGE_WIDTH).width(Fill))
            .align_x(Align::Center)
            .padding([20.0, 28.0, 32.0, 28.0])
            .width(Fill);
        let right = column![
            self.title_strip(),
            scroll(page).key(("pane", self.pane_id())).width(Fill).height(Fill),
            self.status_strip(),
        ]
        .width(Fill)
        .height(Fill);
        row![self.sidebar(), right].width(Fill).height(Fill).into()
    }
}

/// `title` with `ranges` (bytes) in the accent colour and underlined.
fn highlighted(title: &str, ranges: &[std::ops::Range<usize>], size: f64) -> Element<AppMsg> {
    if ranges.is_empty() {
        return text(title.to_owned()).size(size).into();
    }
    let mut spans = Vec::new();
    let mut at = 0;
    for r in ranges {
        if r.start > at && title.is_char_boundary(r.start) {
            spans.push(Span::new(&title[at..r.start]).size(size));
        }
        if title.is_char_boundary(r.start) && title.is_char_boundary(r.end) && r.end <= title.len() {
            spans.push(Span::new(&title[r.start..r.end]).size(size).role(Role::Accent).underline());
            at = r.end;
        }
    }
    if at < title.len() {
        spans.push(Span::new(&title[at..]).size(size));
    }
    rich_text(spans).into()
}

impl Program for Settei {
    type Msg = AppMsg;

    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Settei", "Settei", "Settings").icon(Icon::Settings2).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<AppMsg>) {
        let w = cx.open_window(WindowSpec::new("Settei").size(940.0, 660.0).min_size(780.0, 480.0));
        self.shared.window = w;
        // The settings this window edits, which the sample (or a test) may
        // keep somewhere other than where the kit looked.
        cx.set_appearance(aizome_kit::theme::Appearance::from(&self.shared.settings));
        let tx = cx.stream();
        self.shared.sys = crate::system::System::start(
            self.shared.sample,
            aizome_sys::Wake::new(move || {
                tx.send(AppMsg::SystemChanged);
            }),
        );
        let tx = cx.stream();
        self.watcher = Some(aizome_config::Watcher::spawn_for(
            self.shared.settings_path.clone(),
            Duration::from_secs(1),
            move || {
                tx.send(AppMsg::SettingsOnDisk);
            },
        ));
        if let Some(listener) = self.listener.take() {
            let tx = cx.stream();
            crate::instance::listen(listener, move |args| tx.send(AppMsg::Launched(args)));
        }
        self.refresh_picture();
        let start = std::mem::take(&mut self.start);
        let target = start.pane.as_deref().and_then(|p| self.target(p, start.entry.as_deref()));
        let first = target.as_ref().map_or(0, |(i, _)| *i);
        self.go(first, cx, false);
        if let Some(p) = &start.pane
            && target.is_none()
        {
            self.shared.say(format!("There's no pane called “{p}”"), true, cx);
        }
        if let Some((i, Some(e))) = target {
            self.shared.keyboard = start.keyboard;
            self.reveal(i, &e, cx);
            self.shared.keyboard = None;
        }
    }

    fn update(&mut self, msg: AppMsg, cx: &mut Cx<AppMsg>) {
        match msg {
            AppMsg::Show(i) => {
                // The pane showing again: back to its first page.
                if i == self.current && self.started {
                    for _ in 0..8 {
                        if !self.panes[i].back_any(cx, &mut self.shared) {
                            break;
                        }
                    }
                }
                self.go(i, cx, true);
            }
            AppMsg::Open(id, entry) => self.open(&id, entry.as_deref(), cx),
            AppMsg::Search(q) => self.search(q),
            AppMsg::SearchSubmit => {
                if let Some(h) = self.hits.first() {
                    let id = self.panes[h.pane].id().to_owned();
                    let entry = if h.pane_matched { None } else { h.entries.first().map(|e| e.id.clone()) };
                    self.open(&id, entry.as_deref(), cx);
                }
            }
            AppMsg::Back => {
                if self.panes[self.current].back_any(cx, &mut self.shared) {
                    return;
                }
                if let Some(i) = self.back.pop() {
                    self.forward.push(self.current);
                    self.go(i, cx, false);
                }
            }
            AppMsg::Forward => {
                if let Some(i) = self.forward.pop() {
                    self.back.push(self.current);
                    self.go(i, cx, false);
                }
            }
            AppMsg::FocusSearch => cx.focus("search"),
            AppMsg::Pane(id, m) => {
                if let Some(p) = self.panes.iter_mut().find(|p| p.id() == id) {
                    p.update_any(m, cx, &mut self.shared);
                }
            }
            AppMsg::SystemChanged => {
                self.refresh_picture();
                if self.started {
                    self.panes[self.current].system_changed_any(cx, &mut self.shared);
                }
            }
            AppMsg::SettingsOnDisk => {
                if self.shared.reread(cx) && self.started {
                    self.panes[self.current].settings_changed_any(cx, &mut self.shared);
                }
            }
            AppMsg::Reload => self.shared.reload(cx),
            AppMsg::ClearStatus(g) => self.shared.clear_status(g),
            AppMsg::Said(text, error) => self.shared.say(text, error, cx),
            AppMsg::Unreveal(g) => {
                if g == self.reveal_generation {
                    self.revealed = None;
                }
            }
            AppMsg::OpenSheet(id) => {
                if let Some((_, w)) = self.sheet.take() {
                    cx.close_window(w);
                }
                let w = cx.show_sheet(self.shared.window);
                self.sheet = Some((id, w));
            }
            AppMsg::CloseSheet => {
                if let Some((_, w)) = self.sheet.take() {
                    cx.close_window(w);
                }
            }
            AppMsg::Launched(args) => {
                let start = Start::from_args(&args);
                if let Some(p) = &start.pane {
                    self.shared.keyboard = start.keyboard.clone();
                    self.open(&p.clone(), start.entry.as_deref(), cx);
                    self.shared.keyboard = None;
                }
                cx.activate(self.shared.window);
            }
            AppMsg::Written => self.shared.written(cx),
            AppMsg::Nothing => {}
        }
    }

    fn view(&self, window: WindowKey) -> Element<AppMsg> {
        if let Some((id, w)) = self.sheet
            && w == window
        {
            let empty = HashSet::new();
            let i = self.panes.iter().position(|p| p.id() == id).unwrap_or(self.current);
            let content = self.panes[i].sheet_any(&self.vcx(i, &empty)).unwrap_or_else(|| text("").into());
            return container(content).padding([20.0, 24.0]).width(460.0).into();
        }
        self.main_view()
    }

    fn menus(&self) -> MenuBar<AppMsg> {
        let view = Menu::new("View")
            .item(MenuItem::new("Back", AppMsg::Back).shortcut(Shortcut::cmd('[')).enabled(!self.back.is_empty()))
            .item(
                MenuItem::new("Forward", AppMsg::Forward)
                    .shortcut(Shortcut::cmd(']'))
                    .enabled(!self.forward.is_empty()),
            )
            .separator()
            .item(MenuItem::new("Search Settings", AppMsg::FocusSearch).shortcut(Shortcut::cmd('L')));
        let mut go = Menu::new("Panes");
        let mut first = true;
        for g in Group::ALL {
            let members: Vec<usize> = (0..self.panes.len()).filter(|&i| self.panes[i].group() == g).collect();
            if members.is_empty() {
                continue;
            }
            if !first {
                go = go.separator();
            }
            first = false;
            for i in members {
                let p = &self.panes[i];
                go = go.item(MenuItem::new(p.title(), AppMsg::Show(i)).icon(p.icon()).checked(i == self.current));
            }
        }
        MenuBar::standard(&self.info()).with_menu(view).with_menu(go)
    }

    fn close_requested(&mut self, window: WindowKey, _cx: &mut Cx<AppMsg>) -> bool {
        if self.sheet.is_some_and(|(_, w)| w == window) {
            self.sheet = None;
        }
        true
    }
}

/// A path as people say it: `~/.config/aizome/settings.toml` in the home
/// folder, else just its name.
pub fn short_path(p: &std::path::Path) -> String {
    let home = aizome_config::home_dir();
    match p.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
    }
}
