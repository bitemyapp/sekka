//! Hako, the program: its state, its messages, and what they do.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::KeyEvent;
use aizome_kit::{MsgSender, Point};

use super::browser::{Browser, Load, Loc, Rename};
use super::dnd::{Dest, DragEvent, Target};
use super::fileview::{Layout, Thumbs, ViewKey};
use super::jobs::{self, Answer, Done, Job, JobEvent, Work};
use super::settings::Settings;
use crate::fsops::archive::{self, Format};
use crate::fsops::names;
use crate::fsops::sys;
use crate::fsops::transfer::Mode;
use crate::fsops::trash::{self, Trash};
use crate::fsops::undo::{History, Record, Step};
use crate::model::entry::{self, Entry, Sort, SortKey};
use crate::model::mime::MimeDb;
use crate::model::places::{self, Place};
use crate::model::thumbs::{self, Thumbnails};
use crate::model::uri;

/// What Hako is started with: where things are, and which of the
/// system's services it talks to (tests talk to none).
#[derive(Clone, Debug)]
pub struct Options {
    pub home: PathBuf,
    pub trash: Trash,
    pub thumbnails: Option<Thumbnails>,
    /// The compositor (Desks), UDisks (volumes), the app index (Open
    /// With), recent files.
    pub services: bool,
    /// Watch folders on screen for changes.
    pub watch: bool,
    /// Where settings are kept (none: not kept).
    pub settings: Option<PathBuf>,
    /// The present, for pictures that mustn't change with the day (none:
    /// the clock's).
    pub now: Option<i64>,
}

impl Options {
    pub fn from_env() -> Options {
        Options {
            home: crate::home_dir(),
            trash: Trash::from_env(),
            thumbnails: Some(Thumbnails::from_env()),
            services: true,
            watch: true,
            settings: Some(crate::xdg_dir("XDG_CONFIG_HOME", ".config").join("aizome/hako.toml")),
            now: None,
        }
    }

    /// Everything under `root`, nothing of the system's.
    pub fn sandbox(root: &Path) -> Options {
        Options {
            home: root.join("home"),
            trash: Trash {
                home: root.join("data/Trash"),
                home_dir: Some(root.join("home")),
                uid: sys::uid(),
                topdirs: Some(Vec::new()),
            },
            thumbnails: Some(Thumbnails { root: root.join("cache/thumbnails") }),
            services: false,
            watch: false,
            settings: None,
            now: None,
        }
    }
}

/// What a launch asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// A window at a folder (or at a file's folder, with the file
    /// selected).
    Open(PathBuf),
    /// A window showing `FILE` selected in its folder.
    Reveal(PathBuf),
}

/// Requests from a command line: `PATH…`, `--select FILE`.
pub fn parse_args(args: &[String], cwd: &Path) -> Vec<Request> {
    let mut out = Vec::new();
    let mut it = args.iter();
    let abs = |s: &str| {
        let p = uri::path_of(s).unwrap_or_else(|| PathBuf::from(s));
        names::normalize(&if p.is_absolute() { p } else { cwd.join(p) })
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--select" | "-s" => {
                if let Some(f) = it.next() {
                    out.push(Request::Reveal(abs(f)));
                }
            }
            s if s.starts_with("--select=") => out.push(Request::Reveal(abs(&s["--select=".len()..]))),
            s if s.starts_with('-') => {}
            s => out.push(Request::Open(abs(s))),
        }
    }
    out
}

/// A sheet over a window, and what it's for.
#[derive(Clone, Debug)]
pub enum Sheet {
    Info { window: WindowKey, entry: Box<Entry> },
    QuickLook { window: WindowKey, entry: Box<Entry> },
    Ask { job: u64, all: bool },
    Confirm { window: WindowKey, what: Confirm },
    Message { title: String, body: String },
    Unlock { window: WindowKey, block: String, name: String, pass: String, error: String, busy: bool },
}

/// Something asked before it's done.
#[derive(Clone, Debug)]
pub enum Confirm {
    Erase(Vec<trash::Entry>),
    EmptyTrash,
}

/// Files cut or copied.
#[derive(Clone, Debug, PartialEq)]
pub struct FileClip {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
    /// The clipboard's text as Hako set it (to know it's still ours).
    pub text: String,
}

/// A preview's contents.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub image: Option<Image>,
    pub text: Option<String>,
    pub mime: Option<String>,
    /// Made for Quick Look (the picture itself, more of the text), not
    /// just the preview column.
    pub full: bool,
}

/// Something big that messages share, which says nothing of itself in
/// debug output.
pub struct Shared<T>(pub Arc<T>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}

impl<T> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Shared")
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Nothing: a test's way to let background work arrive.
    Nop,
    // Windows.
    NewWindow,
    CloseWindow(WindowKey),
    Requests(Vec<Request>),
    // Going places.
    Go(WindowKey, Loc),
    GoPath(WindowKey, PathBuf),
    Back(WindowKey),
    Forward(WindowKey),
    Up(WindowKey),
    OpenSelection(WindowKey),
    Loaded(WindowKey, u64, Arc<Result<Vec<Entry>, String>>),
    DirChanged(PathBuf),
    Reload(PathBuf, u64),
    // Columns and views.
    Select(WindowKey, usize, Vec<usize>),
    Activate(WindowKey, usize, usize),
    ViewKey(WindowKey, usize, ViewKey),
    ContextMenu(WindowKey, usize, Option<usize>, Point),
    Drag(WindowKey, usize, DragEvent),
    /// A drag has waited over a folder: open it (if it still waits there).
    Spring(WindowKey, Target, u64),
    DropUrls(WindowKey, Dest, Vec<String>),
    SetLayout(WindowKey, Layout),
    SortBy(SortKey),
    SortAscending(bool),
    FoldersFirst(bool),
    ToggleHidden,
    ColumnWidth(f64),
    SidebarWidth(f64),
    // The location bar.
    EditPath(WindowKey),
    FocusSearch(WindowKey),
    PathText(WindowKey, String),
    PathSubmit(WindowKey),
    PathCancel(WindowKey),
    // Renaming.
    StartRename(WindowKey),
    RenameText(WindowKey, String),
    RenameCommit(WindowKey),
    RenameCancel(WindowKey),
    // Search.
    Search(WindowKey, String),
    SearchSelect(WindowKey, Vec<usize>),
    SearchActivate(WindowKey, usize),
    SearchMenu(WindowKey, Option<usize>, Point),
    SearchDone(WindowKey, u64, Arc<Vec<Entry>>),
    SearchIndex(PathBuf, Shared<aizome_search::files::FileIndex>),
    EndSearch(WindowKey),
    // Files.
    NewFolder(WindowKey),
    Duplicate(WindowKey),
    Trash(WindowKey),
    PutBack(WindowKey),
    AskErase(WindowKey),
    AskEmptyTrash(WindowKey),
    Confirmed(WindowKey),
    Compress(WindowKey, Format),
    Extract(WindowKey),
    Copy(WindowKey),
    Cut(WindowKey),
    Paste(WindowKey, bool),
    CopyPath(WindowKey),
    SelectAll(WindowKey),
    Undo,
    Redo,
    OpenWith(WindowKey, PathBuf, String),
    ShowInFolder(WindowKey, PathBuf),
    GetInfo(WindowKey),
    QuickLook(WindowKey),
    CloseSheet(WindowKey),
    // Background results.
    Thumb(PathBuf, Option<Image>),
    Preview(PathBuf, Arc<Preview>),
    FolderSize(PathBuf, u64, u64),
    Apps(Arc<aizome_sys::apps::AppIndex>),
    Desks(Vec<aizome_ipc::DeskInfo>),
    DisksChanged,
    Mount(String),
    UnlockPass(WindowKey, String),
    UnlockSubmit(WindowKey),
    Eject(String),
    // Jobs.
    Job(u64, JobEvent),
    Answer(u64, Answer),
    AskAll(u64, bool),
    PauseJob(u64),
    CancelJob(u64),
    ShowOperations,
    /// A job has run a while: if it still runs, show the operations.
    JobSlow(u64),
    // The keyboard.
    Key(WindowKey, KeyEvent),
}

/// The program.
pub struct Hako {
    pub opts: Options,
    pub settings: Settings,
    pub windows: BTreeMap<WindowKey, Browser>,
    pub sheets: HashMap<WindowKey, Sheet>,
    pub jobs: Vec<Job>,
    pub(crate) next_job: u64,
    pub ops_window: Option<WindowKey>,
    pub history: History,
    pub clip: Option<FileClip>,
    pub places: Vec<Place>,
    pub system_disk: Option<Place>,
    pub desk: Option<aizome_ipc::DeskInfo>,
    pub disks: Option<aizome_sys::disks::Disks>,
    pub volumes: Vec<aizome_sys::disks::Volume>,
    pub mounting: HashSet<String>,
    pub(crate) mount_tasks: Vec<(String, WindowKey, aizome_sys::Task<String>)>,
    pub(crate) unlock_tasks: Vec<(WindowKey, String, aizome_sys::Task<String>)>,
    /// Volumes being ejected: the drive to eject once the unmount is done.
    pub(crate) eject_tasks: Vec<(String, Option<String>, aizome_sys::Task<()>)>,
    pub apps: Option<Arc<aizome_sys::apps::AppIndex>>,
    pub thumbs: Rc<Thumbs>,
    pub(crate) thumb_wanted: VecDeque<(PathBuf, String)>,
    pub(crate) thumb_asked: HashSet<PathBuf>,
    pub(crate) thumb_running: usize,
    pub previews: HashMap<PathBuf, Arc<Preview>>,
    pub sizes: HashMap<PathBuf, (u64, u64)>,
    pub(crate) watcher: Option<crate::model::watch::Watcher>,
    pub(crate) reloads: HashMap<PathBuf, u64>,
    pub(crate) sender: Option<MsgSender<Msg>>,
    pub(crate) generation: u64,
    pub(crate) pending: Vec<Request>,
    pub instance: Option<std::os::unix::net::UnixListener>,
    pub(crate) search_index: HashMap<PathBuf, (Instant, Arc<aizome_search::files::FileIndex>)>,
    /// Faded rows' paths (cut), shared with the views.
    pub(crate) cut_paths: HashSet<PathBuf>,
    /// A folder just made, to rename once its listing shows it.
    pub(crate) rename_when_loaded: Option<(WindowKey, PathBuf)>,
    /// How many items the Trash holds, as last counted.
    pub trash_count: usize,
    /// The window that last had the keyboard: menu commands are for it.
    pub focus_window: Option<WindowKey>,
    /// What a sandbox would have opened in other apps.
    pub opened: Vec<PathBuf>,
    /// Hako's own drag gone out to the desktop, and its files, to know
    /// them if they're dropped back on one of its windows before it ends.
    pub outbound: Option<(DragId, Vec<PathBuf>)>,
    /// Free space by folder, as last asked (views ask often).
    pub(crate) space: std::cell::RefCell<HashMap<PathBuf, (Instant, Option<u64>)>>,
}

/// The sidebar's width at first.
pub const SIDEBAR: f64 = 200.0;

impl Hako {
    pub fn new(opts: Options) -> Hako {
        let settings = opts.settings.as_deref().map(Settings::load).unwrap_or_default();
        let places = places::standard_places(&opts.home);
        Hako {
            settings,
            windows: BTreeMap::new(),
            sheets: HashMap::new(),
            jobs: Vec::new(),
            next_job: 1,
            ops_window: None,
            history: History::default(),
            clip: None,
            places,
            system_disk: None,
            desk: None,
            disks: None,
            volumes: Vec::new(),
            mounting: HashSet::new(),
            mount_tasks: Vec::new(),
            unlock_tasks: Vec::new(),
            eject_tasks: Vec::new(),
            apps: None,
            thumbs: Rc::new(Thumbs::default()),
            thumb_wanted: VecDeque::new(),
            thumb_asked: HashSet::new(),
            thumb_running: 0,
            previews: HashMap::new(),
            sizes: HashMap::new(),
            watcher: None,
            reloads: HashMap::new(),
            sender: None,
            generation: 1,
            pending: Vec::new(),
            instance: None,
            search_index: HashMap::new(),
            cut_paths: HashSet::new(),
            rename_when_loaded: None,
            trash_count: 0,
            focus_window: None,
            opened: Vec::new(),
            outbound: None,
            space: Default::default(),
            opts,
        }
    }

    /// Open these on launch.
    pub fn with_requests(mut self, r: Vec<Request>) -> Hako {
        self.pending = r;
        self
    }

    pub(crate) fn next_gen(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    pub fn browser(&self, w: WindowKey) -> Option<&Browser> {
        self.windows.get(&w)
    }

    /// The browser window a key means: a sheet's window, or (for
    /// [`WindowKey::NONE`], what menu commands carry) the window with the
    /// keyboard.
    pub(crate) fn owner(&self, key: WindowKey) -> WindowKey {
        if key == WindowKey::NONE {
            return self
                .focus_window
                .filter(|w| self.windows.contains_key(w))
                .or_else(|| self.windows.keys().next_back().copied())
                .unwrap_or(WindowKey::NONE);
        }
        match self.sheets.get(&key) {
            Some(
                Sheet::Info { window, .. }
                | Sheet::QuickLook { window, .. }
                | Sheet::Confirm { window, .. }
                | Sheet::Unlock { window, .. },
            ) => *window,
            _ => key,
        }
    }

    /// Show `sheet` over window `w`, its default button with the keyboard
    /// (so Return and Escape answer it, not what's behind).
    pub(crate) fn open_sheet(&mut self, cx: &mut Cx<Msg>, w: WindowKey, sheet: Sheet) -> WindowKey {
        let s = cx.show_sheet(w);
        self.sheets.insert(s, sheet);
        cx.focus("sheet-default");
        s
    }

    // Windows.

    pub(crate) fn open_window(&mut self, cx: &mut Cx<Msg>, loc: Loc, select: Option<OsString>) -> WindowKey {
        let (w, h) = self.settings.window;
        let key = cx.open_window(WindowSpec::new("Files").size(w, h).min_size(520.0, 320.0));
        self.windows.insert(key, Browser::new(self.settings.layout));
        self.go(cx, key, loc, select, false);
        key
    }

    pub(crate) fn handle_request(&mut self, cx: &mut Cx<Msg>, r: Request) {
        let (loc, select) = match &r {
            Request::Open(p) if p.is_dir() => (self.loc_for(p), None),
            Request::Open(p) | Request::Reveal(p) => match (p.parent(), p.file_name()) {
                (Some(dir), Some(name)) => (self.loc_for(dir), Some(name.to_os_string())),
                _ => (Loc::Folder(p.clone()), None),
            },
        };
        let w = self.open_window(cx, loc, select);
        cx.activate(w);
    }

    /// A folder as a place: the Trash's files folder is the Trash.
    pub(crate) fn loc_for(&self, dir: &Path) -> Loc {
        if self.opts.trash.cans().iter().any(|c| c.files() == dir) {
            Loc::Trash
        } else {
            Loc::Folder(dir.to_path_buf())
        }
    }

    /// Where the columns start for `loc`: the deepest of the places, the
    /// home folder and the volumes that holds it, else `/`.
    pub(crate) fn root_for(&self, loc: &Loc) -> Loc {
        let Loc::Folder(p) = loc else { return loc.clone() };
        let mut roots: Vec<PathBuf> = self.places.iter().map(|pl| pl.path.clone()).collect();
        roots.extend(self.volumes.iter().filter_map(|v| v.mount_point.as_ref().map(PathBuf::from)));
        if let Some(d) = self.desk.as_ref().and_then(|d| d.folder.as_ref()) {
            roots.push(PathBuf::from(d));
        }
        roots.push(PathBuf::from("/"));
        roots
            .into_iter()
            .filter(|r| p.starts_with(r))
            .max_by_key(|r| r.as_os_str().len())
            .map(Loc::Folder)
            .unwrap_or(Loc::Folder(PathBuf::from("/")))
    }

    /// Show `loc` in window `w`.
    pub(crate) fn go(&mut self, cx: &mut Cx<Msg>, w: WindowKey, loc: Loc, select: Option<OsString>, history: bool) {
        let root = self.root_for(&loc);
        let g = self.next_gen();
        let Some(b) = self.windows.get_mut(&w) else { return };
        if history {
            b.push_history();
        }
        b.go(root, &loc, select, g);
        self.start_loads(cx, w);
        self.after_change(cx, w);
        cx.focus(&format!("col-{}", self.windows.get(&w).map_or(0, |b| b.focus)));
    }

    /// Read the columns that want reading.
    pub(crate) fn start_loads(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let sort = self.settings.sort;
        let trash = self.opts.trash.clone();
        let services = self.opts.services;
        let mut gens = Vec::new();
        if let Some(b) = self.windows.get(&w) {
            for (i, c) in b.columns.iter().enumerate() {
                if c.load == Load::Wanted {
                    gens.push((i, c.loc.clone()));
                }
            }
        }
        for (i, loc) in gens {
            let g = self.next_gen();
            if let Some(c) = self.windows.get_mut(&w).and_then(|b| b.columns.get_mut(i)) {
                c.load = Load::Loading(g);
            }
            let trash = trash.clone();
            cx.spawn(move || Msg::Loaded(w, g, Arc::new(read_loc(&loc, sort, &trash, services))));
        }
    }

    /// After the columns changed: the title, what's watched, thumbnails
    /// and previews wanted.
    pub(crate) fn after_change(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        self.update_title(cx, w);
        self.update_watch();
        self.want_preview(cx, w);
    }

    pub(crate) fn update_title(&self, cx: &mut Cx<Msg>, w: WindowKey) {
        let Some(b) = self.windows.get(&w) else { return };
        let title = if let Some(s) = &b.search {
            format!("Search: {}", s.query)
        } else {
            match b.location() {
                Some(Loc::Folder(p)) if *p == self.opts.home => "Home".to_owned(),
                Some(Loc::Folder(p)) => {
                    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
                }
                Some(Loc::Trash) => "Trash".into(),
                Some(Loc::Recent) => "Recent".into(),
                None => "Files".into(),
            }
        };
        cx.set_title(w, title);
    }

    pub(crate) fn update_watch(&mut self) {
        let Some(watcher) = &self.watcher else { return };
        let mut dirs: Vec<PathBuf> = self.windows.values().flat_map(Browser::folders).collect();
        // The home Trash always, for the sidebar's Empty Trash.
        dirs.push(self.opts.trash.home_can().files());
        for b in self.windows.values() {
            if b.columns.iter().any(|c| c.loc == Loc::Trash) {
                dirs.extend(self.opts.trash.cans().iter().map(|c| c.files()));
            }
        }
        dirs.sort();
        dirs.dedup();
        watcher.set(&dirs);
    }

    pub(crate) fn loaded(&mut self, cx: &mut Cx<Msg>, w: WindowKey, g: u64, result: Arc<Result<Vec<Entry>, String>>) {
        let hidden = self.settings.hidden;
        let fresh = self.next_gen();
        let Some(b) = self.windows.get_mut(&w) else { return };
        let Some(i) = b.columns.iter().position(|c| c.load == Load::Loading(g)) else { return };
        let c = &mut b.columns[i];
        match &*result {
            Ok(entries) => {
                let pending = c.pending.is_some();
                c.set_entries(Arc::new(entries.clone()), hidden);
                c.load = Load::Ready;
                if pending && let Some(&row) = c.selected.first() {
                    c.scroll = Some((row, fresh));
                }
            }
            Err(e) => {
                c.load = Load::Failed(e.clone());
                c.all = Arc::new(Vec::new());
                c.rows = Rc::new(Vec::new());
                c.selected = Rc::new(Vec::new());
            }
        }
        // Keep valid descendants on refresh, and close those whose
        // selected parent moved out of this listing (including Put Back).
        // A failed read does not prove that a known child disappeared:
        // an explicit path may pass through an unlistable directory.
        if result.is_ok() && b.refresh_after(i, fresh) {
            self.start_loads(cx, w);
        }
        // A rename asked for (a new folder) starts once it's there.
        let entries: Vec<(PathBuf, String)> = self
            .windows
            .get(&w)
            .and_then(|b| b.columns.get(i))
            .map(|c| {
                c.rows
                    .iter()
                    .filter(|e| thumbs::can_thumbnail(&e.mime))
                    .map(|e| (e.path.clone(), e.mime.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        self.want_thumbs(cx, entries);
        // After going somewhere, the column with the selection takes the
        // keyboard once it's read.
        if let Some(b) = self.windows.get_mut(&w)
            && b.focus_pending
            && b.focus == i
        {
            b.focus_pending = false;
            cx.focus(&format!("col-{i}"));
        }
        self.after_change(cx, w);
    }

    pub(crate) fn reload_dir(&mut self, cx: &mut Cx<Msg>, dir: &Path) {
        let windows: Vec<WindowKey> = self.windows.keys().copied().collect();
        let in_trash = self.opts.trash.cans().iter().any(|c| c.files() == dir);
        if in_trash {
            self.trash_changed();
        }
        for w in windows {
            let mut any = false;
            if let Some(b) = self.windows.get_mut(&w) {
                for c in &mut b.columns {
                    let hit = match &c.loc {
                        Loc::Folder(p) => p == dir,
                        Loc::Trash => in_trash,
                        Loc::Recent => false,
                    };
                    if hit && matches!(c.load, Load::Ready | Load::Failed(_)) {
                        c.load = Load::Wanted;
                        any = true;
                    }
                }
            }
            if any {
                self.start_loads(cx, w);
            }
        }
    }

    /// Select `names` in the folder `dir` shows (in every window that
    /// has the focus there), once it's read again.
    pub(crate) fn select_after(&mut self, dir: &Path, names: Vec<OsString>) {
        for b in self.windows.values_mut() {
            let focus = b.focus;
            if let Some(c) = b.columns.get_mut(focus)
                && c.loc.folder() == Some(dir)
            {
                c.pending = Some(names.clone());
            }
        }
    }

    // Thumbnails and previews.

    pub(crate) fn want_thumbs(&mut self, cx: &mut Cx<Msg>, entries: Vec<(PathBuf, String)>) {
        if self.opts.thumbnails.is_none() {
            return;
        }
        for (p, m) in entries.into_iter().take(2000) {
            if self.thumb_asked.insert(p.clone()) {
                self.thumb_wanted.push_back((p, m));
            }
        }
        self.run_thumbs(cx);
    }

    pub(crate) fn run_thumbs(&mut self, cx: &mut Cx<Msg>) {
        let Some(cache) = self.opts.thumbnails.clone() else { return };
        while self.thumb_running < 2 {
            let Some((p, m)) = self.thumb_wanted.pop_front() else { break };
            self.thumb_running += 1;
            let cache = cache.clone();
            cx.spawn(move || {
                let img = thumbs::make(&cache, &p, &m).and_then(|t| Image::open(t).ok()).map(|i| i.with_scale(0.5));
                Msg::Thumb(p, img)
            });
        }
    }

    /// The entry the preview column (or Quick Look) shows for window `w`.
    pub fn preview_entry(&self, w: WindowKey) -> Option<Entry> {
        let b = self.windows.get(&w)?;
        if b.search.is_some() {
            return None;
        }
        let c = b.columns.last()?;
        let e = c.single()?;
        (!e.is_folder()).then(|| e.clone())
    }

    pub(crate) fn want_preview(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let Some(e) = self.preview_entry(w) else { return };
        self.load_preview(cx, &e, false);
    }

    pub(crate) fn load_preview(&mut self, cx: &mut Cx<Msg>, e: &Entry, full: bool) {
        if self.previews.contains_key(&e.path) && !full {
            return;
        }
        if self.previews.len() > 32 {
            self.previews.clear();
        }
        let (path, size, cache) = (e.path.clone(), e.size, self.opts.thumbnails.clone());
        let mime = e.mime.to_string();
        cx.spawn(move || {
            let db = MimeDb::get();
            let mime = if mime == crate::model::mime::UNKNOWN { db.for_file(&path, true).to_string() } else { mime };
            let mut p = Preview { mime: Some(mime.clone()), full, ..Default::default() };
            if thumbs::can_thumbnail(&mime) {
                p.image = if full && size < 12 << 20 {
                    Image::open(&path).ok()
                } else {
                    cache
                        .as_ref()
                        .and_then(|c| thumbs::make(c, &path, &mime))
                        .and_then(|t| Image::open(t).ok())
                        .map(|i| i.with_scale(0.5))
                };
            } else if db.is_text(&mime) || mime == crate::model::mime::EMPTY {
                p.text = read_head(&path, if full { 64 << 10 } else { 4 << 10 });
            }
            Msg::Preview(path, Arc::new(p))
        });
    }

    // Jobs.

    pub(crate) fn start_job(&mut self, cx: &mut Cx<Msg>, w: WindowKey, work: Work) {
        let id = self.next_job;
        self.next_job += 1;
        let title = work.title();
        let tx = cx.stream();
        let (control, answers) = jobs::start(id, work, self.opts.trash.clone(), tx, Msg::Job);
        self.jobs.push(Job {
            id,
            title,
            window: w,
            control,
            progress: Default::default(),
            question: None,
            sheet: None,
            answers,
            started: Instant::now(),
        });
        cx.after(Duration::from_millis(600), Msg::JobSlow(id));
    }

    pub(crate) fn on_job(&mut self, cx: &mut Cx<Msg>, id: u64, ev: JobEvent) {
        match ev {
            JobEvent::Progress(p) => {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                    j.progress = p;
                }
            }
            JobEvent::Ask(q) => {
                let window = self.jobs.iter().find(|j| j.id == id).map(|j| j.window);
                let parent = window
                    .filter(|w| self.windows.contains_key(w))
                    .or(self.ops_window)
                    .or_else(|| self.windows.keys().next().copied());
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                    j.question = Some(q);
                }
                if let Some(p) = parent {
                    let s = self.open_sheet(cx, p, Sheet::Ask { job: id, all: false });
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.sheet = Some(s);
                    }
                }
            }
            JobEvent::Done(done) => {
                let Some(pos) = self.jobs.iter().position(|j| j.id == id) else { return };
                let job = self.jobs.remove(pos);
                if let Some(s) = job.sheet {
                    self.sheets.remove(&s);
                    cx.close_window(s);
                }
                self.finished(cx, job.window, &job.title, &done);
                if self.jobs.is_empty()
                    && let Some(ops) = self.ops_window.take()
                {
                    cx.close_window(ops);
                }
            }
        }
    }

    pub(crate) fn finished(&mut self, cx: &mut Cx<Msg>, w: WindowKey, title: &str, done: &Done) {
        if let Some(step) = &done.step {
            match done.inverted {
                None => self.history.push(step.clone()),
                Some(false) => self.history.undone(step.clone()),
                Some(true) => self.history.redone(step.clone()),
            }
        }
        // Select what came where the person is.
        let mut by_dir: HashMap<PathBuf, Vec<OsString>> = HashMap::new();
        for (_, to) in &done.report.done {
            if let (Some(d), Some(n)) = (to.parent(), to.file_name()) {
                by_dir.entry(d.to_path_buf()).or_default().push(n.to_os_string());
            }
        }
        for (d, ns) in by_dir {
            self.select_after(&d, ns);
        }
        for d in &done.changed {
            self.reload_dir(cx, d);
        }
        self.trash_changed();
        let r = &done.report;
        let mut problems: Vec<(PathBuf, String)> = r.failed.clone();
        problems.extend(r.problems.iter().cloned());
        if !problems.is_empty() && self.windows.contains_key(&w) {
            let n = problems.len();
            let shown: Vec<String> = problems
                .iter()
                .take(6)
                .map(|(p, why)| {
                    format!(
                        "“{}”: {why}",
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.display().to_string())
                    )
                })
                .collect();
            let more = if n > 6 { format!("\n…and {} more.", n - 6) } else { String::new() };
            let head =
                if n == 1 { "An item couldn't be done.".to_owned() } else { format!("{n} items couldn't be done.") };
            self.open_sheet(
                cx,
                w,
                Sheet::Message { title: format!("{title}: {head}"), body: shown.join("\n") + &more },
            );
        }
    }

    /// The Trash's contents may have changed: count them again (for the
    /// sidebar and Empty Trash, which views mustn't read the disk for).
    pub(crate) fn trash_changed(&mut self) {
        self.trash_count = self.opts.trash.list().len();
    }

    /// The present (or the test's).
    pub fn now(&self) -> i64 {
        self.opts.now.unwrap_or_else(crate::model::format::now)
    }

    /// Whether thumbnails are still being made or waited for.
    pub fn thumbs_pending(&self) -> bool {
        self.thumb_running > 0 || !self.thumb_wanted.is_empty()
    }

    pub fn trash_is_empty(&self) -> bool {
        self.trash_count == 0
    }

    /// The space free where `dir` is, asked of the system at most every
    /// ten seconds.
    pub fn free_space(&self, dir: &Path) -> Option<u64> {
        if !self.opts.services {
            return None;
        }
        let mut cache = self.space.borrow_mut();
        if let Some((at, v)) = cache.get(dir)
            && at.elapsed() < Duration::from_secs(10)
        {
            return *v;
        }
        let v = sys::space(dir).map(|(free, _)| free);
        cache.insert(dir.to_path_buf(), (Instant::now(), v));
        v
    }

    // The selection, and the column it's in.

    pub(crate) fn focused_folder(&self, w: WindowKey) -> Option<PathBuf> {
        let b = self.windows.get(&w)?;
        if b.search.is_some() {
            return None;
        }
        b.target_folder().map(Path::to_path_buf)
    }

    pub(crate) fn in_trash(&self, w: WindowKey) -> bool {
        self.windows.get(&w).is_some_and(|b| b.search.is_none() && b.focused().is_some_and(|c| c.loc == Loc::Trash))
    }

    /// The Trash's entries for the selection, where it's in the Trash.
    pub(crate) fn trash_selection(&self, w: WindowKey) -> Vec<trash::Entry> {
        let Some(b) = self.windows.get(&w) else { return Vec::new() };
        let list = self.opts.trash.list();
        b.selection().iter().filter_map(|p| list.iter().find(|e| &e.file == p).cloned()).collect()
    }

    // Opening.

    pub(crate) fn open_entry(&mut self, cx: &mut Cx<Msg>, w: WindowKey, e: &Entry) {
        if e.is_folder() {
            if e.trashed.is_some() || self.in_trash(w) {
                // Folders in the Trash open as folders, read-only in spirit.
                self.go(cx, w, Loc::Folder(e.path.clone()), None, true);
            } else {
                self.go(cx, w, Loc::Folder(e.path.clone()), None, true);
            }
            return;
        }
        if archive::is_archive(&e.display) && !e.display.to_ascii_lowercase().ends_with(".iso") {
            // Archives open into themselves, as Finder's do.
            if let Some(dest) = e.path.parent() {
                let work = Work::Extract { archive: e.path.clone(), dest: dest.to_path_buf() };
                self.start_job(cx, w, work);
            }
            return;
        }
        self.launch_default(cx, &e.path, &e.mime);
    }

    pub(crate) fn launch_default(&mut self, cx: &mut Cx<Msg>, path: &Path, mime: &str) {
        if !self.opts.services {
            // A sandbox (tests) starts nothing: it notes what it would open.
            self.opened.push(path.to_path_buf());
            return;
        }
        let mime = if mime == crate::model::mime::UNKNOWN {
            MimeDb::get().for_file(path, true).to_string()
        } else {
            mime.to_owned()
        };
        if let Some(apps) = &self.apps
            && let Some(id) = apps.default_for(&mime)
            && let Some(app) = apps.get(&id)
        {
            launch_app(app.clone(), path);
            return;
        }
        cx.open_url(&uri::file_uri(path));
    }

    pub(crate) fn open_selection(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let Some(b) = self.windows.get(&w) else { return };
        let es = b.selected_entries();
        match es.as_slice() {
            [one] => {
                let one = one.clone();
                self.open_entry(cx, w, &one);
            }
            many => {
                for e in many.iter().filter(|e| !e.is_folder()).cloned().collect::<Vec<_>>() {
                    self.launch_default(cx, &e.path, &e.mime);
                }
            }
        }
    }

    // The clipboard.

    pub(crate) fn copy(&mut self, cx: &mut Cx<Msg>, w: WindowKey, cut: bool) {
        let Some(b) = self.windows.get(&w) else { return };
        let paths = b.selection();
        if paths.is_empty() || (cut && self.in_trash(w)) {
            return;
        }
        let text = uri::plain_paths(&paths);
        cx.set_clipboard_text(&text);
        super::clipboard::offer_files(&paths, cut, &text);
        self.cut_paths = if cut { paths.iter().cloned().collect() } else { HashSet::new() };
        self.clip = Some(FileClip { paths, cut, text });
    }

    pub(crate) fn paste(&mut self, cx: &mut Cx<Msg>, w: WindowKey, force_move: bool) {
        let Some(dest) = self.focused_folder(w) else { return };
        let text = cx.clipboard_text().unwrap_or_default();
        let (paths, cut) = match &self.clip {
            Some(c) if c.text == text => (c.paths.clone(), c.cut),
            _ => {
                // Another program's: files as a file manager puts them (a
                // cut moves), else paths or file URIs as text, a line each.
                let (cut, ps) =
                    super::clipboard::foreign_files().unwrap_or_else(|| (false, uri::parse_uri_list(&text)));
                let ps: Vec<PathBuf> = ps.into_iter().filter(|p| std::fs::symlink_metadata(p).is_ok()).collect();
                if ps.is_empty() {
                    return;
                }
                (ps, cut)
            }
        };
        let mode = if cut || force_move { Mode::Move } else { Mode::Copy };
        if cut {
            self.clip = None;
            self.cut_paths.clear();
        }
        self.start_job(cx, w, Work::Transfer { sources: paths, dest, mode });
    }

    // Renaming.

    pub(crate) fn start_rename(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let in_trash = self.in_trash(w);
        let Some(b) = self.windows.get_mut(&w) else { return };
        if b.search.is_some() || in_trash {
            return;
        }
        let col = b.focus;
        let Some(c) = b.columns.get(col) else { return };
        let (Some(e), [row]) = (c.single(), c.selected.as_slice()) else { return };
        b.rename = Some(Rename { column: col, row: *row, path: e.path.clone(), text: e.display.clone(), error: None });
        cx.focus("rename");
    }

    pub(crate) fn commit_rename(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let Some(b) = self.windows.get_mut(&w) else { return };
        let Some(r) = b.rename.clone() else { return };
        let text = r.text.trim().to_owned();
        let old = r.path.file_name().map(names::display).unwrap_or_default();
        if text == old {
            b.rename = None;
            cx.focus(&format!("col-{}", r.column));
            return;
        }
        if let Some(why) = names::check_name(&text) {
            b.rename.as_mut().unwrap().error = Some(why.into());
            return;
        }
        let Some(dir) = r.path.parent() else { return };
        let to = dir.join(&text);
        match sys::rename_noreplace(&r.path, &to) {
            Ok(()) => {
                b.rename = None;
                self.history.push(Step {
                    record: Record::Rename { from: r.path.clone(), to: to.clone() },
                    label: "Rename".into(),
                });
                let dir = dir.to_path_buf();
                self.select_after(&dir, vec![OsString::from(&text)]);
                self.reload_dir(cx, &dir);
                cx.focus(&format!("col-{}", r.column));
            }
            Err(e) => {
                let why = if e.kind() == std::io::ErrorKind::AlreadyExists {
                    format!("“{text}” is taken. Choose another name.")
                } else {
                    format!("It can't be renamed: {}.", crate::fsops::describe(&e))
                };
                b.rename.as_mut().unwrap().error = Some(why);
            }
        }
    }

    pub(crate) fn new_folder(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let Some(dir) = self.focused_folder(w) else { return };
        let name = names::free_name(&dir, std::ffi::OsStr::new("untitled folder"), true);
        let path = dir.join(&name);
        match std::fs::create_dir(&path) {
            Ok(()) => {
                self.history
                    .push(Step { record: Record::Made { paths: vec![path.clone()] }, label: "New Folder".into() });
                if let Some(b) = self.windows.get_mut(&w) {
                    let focus = b.focus;
                    b.columns.truncate(focus + 1);
                    if let Some(c) = b.columns.get_mut(focus) {
                        c.pending = Some(vec![name.clone()]);
                    }
                }
                self.rename_when_loaded = Some((w, path));
                self.reload_dir(cx, &dir);
            }
            Err(e) => {
                self.open_sheet(
                    cx,
                    w,
                    Sheet::Message { title: "The folder couldn't be made.".into(), body: crate::fsops::describe(&e) },
                );
            }
        }
    }
}

/// The first `limit` bytes of a file as text (up to a whole character).
fn read_head(path: &Path, limit: usize) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    let s = match String::from_utf8(buf) {
        Ok(s) => s,
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let mut b = e.into_bytes();
            b.truncate(valid);
            String::from_utf8(b).ok()?
        }
    };
    Some(s.replace('\t', "    "))
}

/// Read what a column lists.
pub fn read_loc(loc: &Loc, sort: Sort, trash: &Trash, services: bool) -> Result<Vec<Entry>, String> {
    let mut entries = match loc {
        Loc::Folder(d) => entry::read_folder(d).map_err(|e| crate::fsops::describe(&e))?,
        Loc::Trash => trash
            .list()
            .into_iter()
            .filter_map(|t| {
                let mut e = Entry::read(&t.file).ok()?;
                e.display = t.display_name();
                if let Some(orig) = &t.original
                    && !e.is_folder()
                {
                    let name = orig.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if let Some(m) = MimeDb::get().by_name(&name) {
                        e.kind_name = MimeDb::get().kind_name(&m);
                        e.mime = m;
                    }
                }
                e.hidden = false;
                e.trashed = Some(entry::Trashed { original: t.original.clone(), deleted: t.deleted.clone() });
                Some(e)
            })
            .collect(),
        Loc::Recent => {
            if !services {
                Vec::new()
            } else {
                aizome_search::recent::load(aizome_search::recent::default_path())
                    .into_iter()
                    .take(200)
                    .filter_map(|r| Entry::read(&r.path).ok())
                    .collect()
            }
        }
    };
    if *loc != Loc::Recent {
        entry::sort_entries(&mut entries, sort);
    }
    Ok(entries)
}

/// Launch `app` on `path` (in the background).
pub(crate) fn launch_app(app: aizome_sys::apps::App, path: &Path) {
    let options =
        aizome_sys::apps::LaunchOptions { targets: vec![path.to_string_lossy().into_owned()], ..Default::default() };
    std::thread::spawn(move || {
        if let Err(e) = aizome_sys::apps::launch(&app, &options) {
            log::warn!("{}: {e}", app.id);
        }
    });
}
