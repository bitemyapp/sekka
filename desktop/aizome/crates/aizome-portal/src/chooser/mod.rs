//! The chooser's window: Files in miniature. Hako's sidebar and path bar
//! over a list of the folder (sortable by name, date, size and kind),
//! the app's filters and choices along the bottom, and its verb on the
//! default button. It opens over the app that asked (its exported
//! window), answers once, and quits.

pub mod model;
mod view;

use std::path::{Path, PathBuf};

use aizome_kit::prelude::*;
use aizome_kit::{Shortcut, ShortcutKey};
use hako::model::entry::{Entry, SortKey};
use hako::model::places::{self, Place, PlaceKind};

use crate::memory::Memory;
use crate::request::{Answer, Mode, Request};
use model::{Browser, Loc, Step};

#[derive(Clone, Debug)]
pub enum Msg {
    Go(Loc),
    Back,
    Forward,
    Up,
    /// One of the person's places, by kind (the Go menu's).
    Place(PlaceKind),
    Listed(u64, Result<Vec<Entry>, String>),
    /// The current Desk's name and folder, from the compositor.
    Desk(Option<(String, PathBuf)>),
    Select(Vec<usize>),
    Activate(usize),
    /// A column's header: sort by it, ascending or not.
    Sort(usize, bool),
    SortBy(SortKey),
    Name(String),
    Filter(usize),
    Choice(usize, String),
    Hidden(bool),
    Accept,
    Cancel,
    /// Replacing was confirmed.
    Replace(PathBuf),
    GoTo,
    NewFolder,
    SheetText(String),
    SheetSubmit,
    SheetCancel,
}

/// A sheet over the chooser: where to go, or a new folder's name.
pub(crate) struct Sheet {
    pub key: WindowKey,
    pub kind: SheetKind,
    pub text: String,
    pub problem: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SheetKind {
    GoTo,
    NewFolder,
}

/// The sidebar's places.
#[derive(Default)]
pub struct Places {
    pub standard: Vec<Place>,
    pub desk: Option<(String, PathBuf)>,
    pub disk: Option<Place>,
    pub volumes: Vec<Place>,
}

impl Places {
    fn read(home: &Path) -> Places {
        Places {
            standard: places::standard_places(home),
            desk: None,
            disk: places::system_disk(home),
            volumes: volumes(),
        }
    }
}

/// Removable volumes mounted where UDisks mounts them for the person.
fn volumes() -> Vec<Place> {
    let Some(user) = std::env::var_os("USER") else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(Path::new("/run/media").join(user)) else { return Vec::new() };
    let mut out: Vec<Place> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| Place { name: hako::fsops::names::display(&e.file_name()), path: e.path(), kind: PlaceKind::Volume })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub struct Chooser {
    pub browser: Browser,
    /// What it answered: `None` until then, or if cancelled.
    pub answer: Option<Answer>,
    pub places: Places,
    pub(crate) window: WindowKey,
    pub(crate) sheet: Option<Sheet>,
    memory: Memory,
    memory_path: PathBuf,
    home: PathBuf,
    /// Ask the compositor for the current Desk (off for tests).
    pub ask_desk: bool,
    /// The present, for dates (tests fix it).
    pub now: Option<i64>,
}

impl Chooser {
    /// A chooser for `request`, remembering in `memory_path`.
    pub fn new(request: Request, memory_path: PathBuf) -> Chooser {
        Chooser::with_home(request, memory_path, hako::home_dir())
    }

    /// The same, for someone whose home is `home`.
    pub fn with_home(request: Request, memory_path: PathBuf, home: PathBuf) -> Chooser {
        let memory = Memory::load(&memory_path);
        let browser = Browser::new(request, &memory, None, &home);
        Chooser {
            browser,
            answer: None,
            places: Places::read(&home),
            window: WindowKey::NONE,
            sheet: None,
            memory,
            memory_path,
            home,
            ask_desk: true,
            now: None,
        }
    }

    fn read(&mut self, generation: u64, cx: &mut Cx<Msg>) {
        let loc = self.browser.loc.clone();
        cx.spawn(move || Msg::Listed(generation, model::read(&loc)));
    }

    fn go(&mut self, loc: Loc, cx: &mut Cx<Msg>) {
        let g = self.browser.go(loc);
        self.read(g, cx);
    }

    fn step(&mut self, step: Step, cx: &mut Cx<Msg>) {
        match step {
            Step::Stay => {}
            Step::Go(loc) => self.go(loc, cx),
            Step::Replace(path) => {
                let name = path.file_name().map(hako::fsops::names::display).unwrap_or_default();
                let place = path.parent().and_then(|p| p.file_name()).map(hako::fsops::names::display);
                let message = match place {
                    Some(p) => {
                        format!("A file of that name is already in “{p}”. Replacing it overwrites what it holds.")
                    }
                    None => "A file of that name is already there. Replacing it overwrites what it holds.".into(),
                };
                cx.alert(
                    self.window,
                    Alert::new(format!("“{name}” already exists. Replace it?"), message)
                        .destructive_button("Replace", Msg::Replace(path))
                        .cancel_button("Cancel"),
                );
            }
            Step::Done(answer) => self.finish(Some(answer), cx),
        }
    }

    fn finish(&mut self, answer: Option<Answer>, cx: &mut Cx<Msg>) {
        if let Some(a) = &answer
            && let Some(dir) = self.browser.chosen_in(a)
        {
            self.memory.chose_in(&self.browser.request.app_id, &dir);
        }
        self.memory.hidden = self.browser.hidden;
        self.memory.sort = self.browser.sort;
        if let Err(e) = self.memory.save(&self.memory_path) {
            log::warn!("the file chooser couldn't remember where it was: {e}");
        }
        self.answer = answer;
        cx.quit();
    }

    fn place_path(&self, kind: PlaceKind) -> Option<PathBuf> {
        if kind == PlaceKind::Home {
            return Some(self.home.clone());
        }
        self.places.standard.iter().find(|p| p.kind == kind).map(|p| p.path.clone())
    }

    fn open_sheet(&mut self, kind: SheetKind, cx: &mut Cx<Msg>) {
        if self.sheet.is_some() {
            return;
        }
        let text = match kind {
            SheetKind::GoTo => String::new(),
            SheetKind::NewFolder => "untitled folder".into(),
        };
        let key = cx.show_sheet(self.window);
        self.sheet = Some(Sheet { key, kind, text, problem: None });
        cx.focus("chooser-sheet-field");
    }

    fn close_sheet(&mut self, cx: &mut Cx<Msg>) {
        if let Some(s) = self.sheet.take() {
            cx.close_window(s.key);
        }
    }

    fn submit_sheet(&mut self, cx: &mut Cx<Msg>) {
        let Some(sheet) = &mut self.sheet else { return };
        let here = self.browser.loc.folder().map(Path::to_path_buf).unwrap_or_else(|| self.home.clone());
        let typed = sheet.text.trim().to_owned();
        if typed.is_empty() {
            return;
        }
        match sheet.kind {
            SheetKind::GoTo => {
                let path =
                    hako::fsops::names::normalize(&here.join(hako::fsops::names::expand_tilde(&typed, &self.home)));
                if path.is_dir() {
                    self.close_sheet(cx);
                    self.go(Loc::Folder(path), cx);
                } else if path.exists()
                    && let Some(parent) = path.parent()
                {
                    // A file: its folder, with it selected.
                    let parent = parent.to_path_buf();
                    self.browser.select_after = path.file_name().map(|n| n.to_os_string());
                    self.close_sheet(cx);
                    self.go(Loc::Folder(parent), cx);
                } else {
                    sheet.problem = Some(format!("There's no folder or file at “{}”.", path.display()));
                }
            }
            SheetKind::NewFolder => {
                if let Some(why) = hako::fsops::names::check_name(&typed) {
                    sheet.problem = Some(why.into());
                    return;
                }
                let path = here.join(&typed);
                match std::fs::create_dir(&path) {
                    Ok(()) => {
                        self.close_sheet(cx);
                        if self.browser.request.directory {
                            // Choosing folders: the new one, selected.
                            self.browser.select_after = Some(typed.into());
                            let g = self.browser.start_reading();
                            self.read(g, cx);
                        } else {
                            self.go(Loc::Folder(path), cx);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        sheet.problem = Some(format!("“{typed}” is already here."));
                    }
                    Err(e) => sheet.problem = Some(format!("The folder couldn't be made: {e}")),
                }
            }
        }
    }

    /// Why the open sheet's path or name won't do, if it won't.
    pub fn sheet_problem(&self) -> Option<&str> {
        self.sheet.as_ref()?.problem.as_deref()
    }

    /// New folders can be made where things are saved or folders chosen.
    pub(crate) fn makes_folders(&self) -> bool {
        (self.browser.mode() != Mode::Open || self.browser.request.directory) && self.browser.loc.folder().is_some()
    }
}

/// Follow the compositor for the current Desk's folder (once).
fn current_desk() -> Option<(String, PathBuf)> {
    use aizome_ipc::{Client, Request as Ipc, Response};
    let mut client = Client::connect().ok()?;
    let _ = client.stream().set_read_timeout(Some(std::time::Duration::from_millis(500)));
    let Ok(Response::Desks { desks, .. }) = client.request(&Ipc::Desks) else { return None };
    let d = desks.into_iter().find(|d| d.current)?;
    let folder = PathBuf::from(d.folder?);
    let named = !d.name.trim().is_empty() && d.name.trim() != d.index.to_string();
    let label = if named { format!("Desk: {}", d.name.trim()) } else { format!("Desk {}", d.index) };
    Some((label, folder))
}

impl Program for Chooser {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(crate::CHOOSER_APP_ID, "File Chooser", "File Chooser").icon(Icon::FolderOpen)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let (w, h) = (860.0, 540.0);
        let mut spec = WindowSpec::new(self.browser.request.window_title()).size(w, h).min_size(560.0, 360.0);
        if let Some(handle) = self.browser.request.wayland_parent() {
            spec = spec.foreign_parent(handle);
        }
        self.window = cx.open_window(spec);
        let g = self.browser.start_reading();
        self.read(g, cx);
        if self.ask_desk {
            cx.spawn(|| Msg::Desk(current_desk()));
        }
        cx.focus(if self.browser.mode() == Mode::Save { "chooser-name" } else { "chooser-files" });
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Go(loc) => self.go(loc, cx),
            Msg::Back => {
                if let Some(g) = self.browser.go_back() {
                    self.read(g, cx);
                }
            }
            Msg::Forward => {
                if let Some(g) = self.browser.go_forward() {
                    self.read(g, cx);
                }
            }
            Msg::Up => {
                if let Some(loc) = self.browser.up() {
                    // Back up into a folder: the one left, selected.
                    self.browser.select_after =
                        self.browser.loc.folder().and_then(|f| f.file_name()).map(|n| n.to_os_string());
                    self.go(loc, cx);
                }
            }
            Msg::Place(kind) => {
                if kind == PlaceKind::Recent && self.browser.offers_recent() {
                    self.go(Loc::Recent, cx);
                } else if let Some(p) = self.place_path(kind) {
                    self.go(Loc::Folder(p), cx);
                }
            }
            Msg::Listed(g, result) => self.browser.listed(g, result),
            Msg::Desk(desk) => {
                // The Desk's folder is where to start when nothing else
                // said where: before anything was chosen there.
                if let Some((_, folder)) = &desk
                    && self.browser.back.is_empty()
                    && self.browser.request.file().is_none()
                    && self.browser.request.folder().is_none()
                    && self.memory.folder(&self.browser.request.app_id).is_none()
                    && folder.is_dir()
                    && self.browser.loc != Loc::Folder(folder.clone())
                {
                    self.browser.loc = Loc::Folder(folder.clone());
                    let g = self.browser.start_reading();
                    self.read(g, cx);
                }
                self.places.desk = desk;
            }
            Msg::Select(rows) => self.browser.select(rows),
            Msg::Activate(i) => {
                let step = self.browser.activate(i);
                self.step(step, cx);
            }
            Msg::Sort(column, ascending) => {
                self.browser.sort.key = view::COLUMNS[column.min(view::COLUMNS.len() - 1)].1;
                self.browser.sort.ascending = ascending;
                self.browser.refresh();
            }
            Msg::SortBy(key) => {
                if self.browser.sort.key == key {
                    self.browser.sort.ascending = !self.browser.sort.ascending;
                } else {
                    self.browser.sort.key = key;
                    // Newest and largest first, as people look for them.
                    self.browser.sort.ascending = matches!(key, SortKey::Name | SortKey::Kind);
                }
                self.browser.refresh();
            }
            Msg::Name(n) => {
                self.browser.name = n;
                self.browser.problem = None;
            }
            Msg::Filter(i) => self.browser.set_filter(i),
            Msg::Choice(i, v) => self.browser.set_choice(i, v),
            Msg::Hidden(on) => {
                self.browser.hidden = on;
                self.browser.refresh();
            }
            Msg::Accept => {
                let step = self.browser.accept();
                self.step(step, cx);
            }
            Msg::Cancel => self.finish(None, cx),
            Msg::Replace(path) => {
                let answer = self.browser.answer(&[path]);
                self.finish(Some(answer), cx);
            }
            Msg::GoTo => self.open_sheet(SheetKind::GoTo, cx),
            Msg::NewFolder => {
                if self.makes_folders() {
                    self.open_sheet(SheetKind::NewFolder, cx);
                }
            }
            Msg::SheetText(t) => {
                if let Some(s) = &mut self.sheet {
                    s.text = t;
                    s.problem = None;
                }
            }
            Msg::SheetSubmit => self.submit_sheet(cx),
            Msg::SheetCancel => self.close_sheet(cx),
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        match &self.sheet {
            Some(s) if s.key == window => view::sheet(s),
            _ => view::window(self),
        }
    }

    fn menus(&self) -> MenuBar<Msg> {
        let b = &self.browser;
        let go = Menu::new("Go")
            .item(MenuItem::new("Back", Msg::Back).shortcut(Shortcut::cmd('[')).enabled(!b.back.is_empty()))
            .item(MenuItem::new("Forward", Msg::Forward).shortcut(Shortcut::cmd(']')).enabled(!b.forward.is_empty()))
            .item(
                MenuItem::new("Enclosing Folder", Msg::Up)
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Up))
                    .enabled(b.up().is_some()),
            )
            .separator()
            .items(
                b.offers_recent().then(|| {
                    MenuItem::new("Recent", Msg::Place(PlaceKind::Recent)).shortcut(Shortcut::cmd('F').shift())
                }),
            )
            .item(MenuItem::new("Home", Msg::Place(PlaceKind::Home)).shortcut(Shortcut::cmd('H').shift()))
            .item(MenuItem::new("Desktop", Msg::Place(PlaceKind::Desktop)).shortcut(Shortcut::cmd('D').shift()))
            .item(MenuItem::new("Documents", Msg::Place(PlaceKind::Documents)).shortcut(Shortcut::cmd('O').shift()))
            .item(MenuItem::new("Downloads", Msg::Place(PlaceKind::Downloads)).shortcut(Shortcut::cmd('L').alt()))
            .separator()
            .item(MenuItem::new("Go to Folder…", Msg::GoTo).shortcut(Shortcut::cmd('G').shift()));
        let sort = |label: &str, key: SortKey| MenuItem::new(label, Msg::SortBy(key)).checked(b.sort.key == key);
        let view = Menu::new("View")
            .item(
                MenuItem::new("Show Hidden Files", Msg::Hidden(!b.hidden))
                    .shortcut(Shortcut::cmd('.').shift())
                    .checked(b.hidden),
            )
            .separator()
            .header("Sort By")
            .item(sort("Name", SortKey::Name))
            .item(sort("Date Modified", SortKey::Modified))
            .item(sort("Size", SortKey::Size))
            .item(sort("Kind", SortKey::Kind));
        let mut bar = MenuBar::standard(&self.info());
        if self.makes_folders() {
            bar = bar.with_file_menu(
                Menu::new("File")
                    .item(MenuItem::new("New Folder…", Msg::NewFolder).shortcut(Shortcut::cmd('N').shift())),
            );
        }
        bar.with_menu(view).with_menu(go)
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.sheet.as_ref().is_some_and(|s| s.key == window) {
            self.sheet = None;
            return true;
        }
        // Closing the window is cancelling.
        self.finish(None, cx);
        true
    }
}
