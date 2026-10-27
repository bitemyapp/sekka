//! Sumi, the program: a window per document, and what each message does
//! to them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_kit::{EditorAction, FindRequest, MsgSender, Selection};

use crate::autosave::{self, Session, Store};
use crate::doc::{self, Doc, Loaded};
use crate::files::{self, Encoding, LineEnding};
use crate::find;
use crate::format::{self, Inline, List, Style};
use crate::recent;
use crate::settings::{self, Settings};
use crate::syntax::Language;

pub const APP_ID: &str = "org.aizome.Sumi";
/// A new window's size: the board's editor (700 × 520 with its frame and
/// the focus rail).
pub const WINDOW_SIZE: Size = Size::new(698.0, 516.0);
/// Unsaved text is kept this long after the last change.
const AUTOSAVE_AFTER: Duration = Duration::from_millis(800);
/// How soon a colouring pass follows the frame that asked for it.
const HIGHLIGHT_PASS: Duration = Duration::from_millis(4);
/// Files bigger than this are read on a thread of their own.
const READ_IN_BACKGROUND: u64 = 2 << 20;
/// Documents bigger than this are searched on a thread of their own.
const SEARCH_IN_BACKGROUND: usize = 1 << 20;

/// Where Sumi keeps things; nothing is kept where there's none (tests).
#[derive(Clone, Debug, Default)]
pub struct Places {
    /// Autosaves and the session.
    pub state: Option<PathBuf>,
    /// The shared recently-used.xbel.
    pub recent: Option<PathBuf>,
    pub settings: Option<PathBuf>,
    /// Where the panels start for a document without a file (else
    /// ~/Documents, else home).
    pub documents: Option<PathBuf>,
}

impl Places {
    /// The user's: `$XDG_STATE_HOME/aizome/sumi`, `$XDG_DATA_HOME/recently-used.xbel`,
    /// `$XDG_CONFIG_HOME/aizome/sumi.toml`.
    pub fn user() -> Places {
        Places {
            state: Some(autosave::default_dir()),
            recent: Some(recent::default_path()),
            settings: Some(settings::default_path()),
            documents: None,
        }
    }
}

/// A command for a document window.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Save,
    SaveAs,
    Revert,
    Close,
    SetLanguage(Language),
    SetEncoding(Encoding),
    SetLineEnding(LineEnding),
    Reopen(Encoding),
    Inline(Inline),
    Link,
    Heading(u8),
    Quote,
    List(List),
    CodeBlock,
    Rule,
    CopyStyle,
    PasteStyle,
    ShiftRight,
    ShiftLeft,
    ZoomIn,
    ZoomOut,
    FindShow,
    ReplaceShow,
    FindHide,
    FindNext,
    FindPrevious,
    UseSelection,
    FindQuery(String),
    FindReplacement(String),
    FindCase,
    FindWord,
    FindRegex,
    ReplaceOne,
    ReplaceAll,
    GotoLine,
}

/// What a dirty document's window does on closing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseChoice {
    Save,
    Discard,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// A command for the window with the keyboard (from the menus).
    Cmd(Command),
    /// A command for this window (its toolbar, find bar, keys).
    On(WindowKey, Command),
    Edit(WindowKey, EditorAction),
    Find(WindowKey, FindRequest),
    New,
    Open,
    Opened(Option<Vec<PathBuf>>),
    OpenRecent(PathBuf),
    ClearRecent,
    SaveTo(WindowKey, Option<PathBuf>),
    /// Save again, over a file changed on disk.
    Overwrite(WindowKey),
    /// Save in UTF-8, the encoding chosen couldn't hold the text.
    SaveUtf8(WindowKey),
    Reload(WindowKey, Option<Encoding>),
    Loaded(WindowKey, u64, Result<Box<Loaded>, String>),
    Close(WindowKey, CloseChoice),
    Searched(WindowKey, u64, u64, Result<Arc<Vec<Range<usize>>>, String>),
    Autosave(WindowKey, u64),
    /// Line colours need more working out: a pass a moment from now.
    HighlightSoon,
    /// Line colours need more working out.
    Highlight,
    ToggleNumbers,
    ToggleWrap,
    /// Bigger, smaller, or (none) as it was.
    Zoom(Option<bool>),
    Font(bool),
    LinkText(String),
    LinkUrl(String),
    LinkDone(bool),
    GotoText(String),
    GotoDone(bool),
    /// Open an address (a link ❖-clicked) in its app.
    OpenUrl(String),
    /// Another launch's arguments.
    Launch(Vec<String>),
    LaunchDone,
    Quit,
    Nothing,
}

/// A sheet over a document's window.
pub enum Sheet {
    Link { parent: WindowKey, text: String, url: String, range: Range<usize> },
    Goto { parent: WindowKey, line: String, max: usize },
}

pub struct Sumi {
    pub docs: BTreeMap<WindowKey, Doc>,
    pub settings: Settings,
    pub places: Places,
    store: Store,
    /// Files opened, newest first (Open Recent).
    pub recent: Vec<PathBuf>,
    pub sheets: HashMap<WindowKey, Sheet>,
    /// What Copy Style copied.
    pub style: Option<Style>,
    /// The one-instance socket, handed over at launch.
    pub instance: Option<std::os::unix::net::UnixListener>,
    notify: Option<MsgSender<Msg>>,
    /// The window with the keyboard, as of the last message.
    focused: Option<WindowKey>,
    /// Windows to close once they're saved.
    closing: HashSet<WindowKey>,
    seq: u64,
    /// Whether line colours have a time budget (tests can lift it).
    pub highlight_budget: Duration,
    /// A colouring pass is due.
    highlight_soon: bool,
    /// Links ❖-clicked, newest last; opened with xdg-open unless `open_links`
    /// is off (tests).
    pub links: Vec<String>,
    pub open_links: bool,
}

impl Drop for Sumi {
    fn drop(&mut self) {
        // Whatever autosaves are on their way get there.
        self.store.flush();
    }
}

impl Sumi {
    pub fn new(places: Places) -> Sumi {
        let settings = Settings::load(places.settings.as_deref());
        let store = Store::new(places.state.clone());
        let recent = places
            .recent
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| recent::used_by(&t, "Sumi"))
            .unwrap_or_default();
        Sumi {
            docs: BTreeMap::new(),
            settings,
            places,
            store,
            recent,
            sheets: HashMap::new(),
            style: None,
            instance: None,
            notify: None,
            focused: None,
            closing: HashSet::new(),
            seq: 0,
            highlight_budget: Duration::from_millis(3),
            highlight_soon: false,
            links: Vec::new(),
            open_links: true,
        }
    }

    /// The document of the window with the keyboard (as of the last
    /// message), else the newest.
    pub fn focused_doc(&self) -> Option<&Doc> {
        self.focused.and_then(|w| self.docs.get(&w)).or_else(|| self.docs.values().next_back())
    }

    fn target(&mut self, cx: &Cx<Msg>) -> Option<WindowKey> {
        let k = cx.key_window();
        let k = k.and_then(|k| if self.docs.contains_key(&k) { Some(k) } else { self.sheet_parent(k) });
        k.or(self.focused).filter(|w| self.docs.contains_key(w)).or_else(|| self.docs.keys().next_back().copied())
    }

    fn sheet_parent(&self, k: WindowKey) -> Option<WindowKey> {
        match self.sheets.get(&k)? {
            Sheet::Link { parent, .. } | Sheet::Goto { parent, .. } => Some(*parent),
        }
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn next_untitled(&self) -> u32 {
        let used: HashSet<u32> = self.docs.values().filter(|d| d.path.is_none()).map(|d| d.untitled).collect();
        (1..).find(|n| !used.contains(n)).unwrap_or(1)
    }

    /// A new window with a document in it.
    fn new_window(&mut self, cx: &mut Cx<Msg>, id: String, untitled: u32) -> WindowKey {
        let w = cx.open_window(
            WindowSpec::new("Untitled").size(WINDOW_SIZE.width, WINDOW_SIZE.height).min_size(360.0, 200.0),
        );
        let mut d = Doc::new(id, w, untitled);
        d.content.soft_tabs = self.settings.soft_tabs;
        if let Ok(mut h) = d.highlight.lock() {
            h.budget = self.highlight_budget;
            if let Some(tx) = &self.notify {
                let tx = tx.clone();
                h.notify = Some(Box::new(move || {
                    tx.send(Msg::HighlightSoon);
                }));
            }
        }
        cx.set_title(w, d.title());
        self.docs.insert(w, d);
        self.focused = Some(w);
        cx.focus("editor");
        w
    }

    pub fn new_doc(&mut self, cx: &mut Cx<Msg>) -> WindowKey {
        let n = self.next_untitled();
        let w = self.new_window(cx, autosave::new_id(), n);
        self.write_session();
        w
    }

    fn doc_for_path(&self, path: &Path) -> Option<WindowKey> {
        self.docs.values().find(|d| d.path.as_deref() == Some(path)).map(|d| d.window)
    }

    /// An Untitled document holding `text` (in the untouched Untitled
    /// window if there is one).
    pub fn new_untitled_with(&mut self, text: &str, cx: &mut Cx<Msg>) -> WindowKey {
        let reuse = self.target(cx).filter(|w| self.docs.get(w).is_some_and(Doc::pristine));
        let w = match reuse {
            Some(w) => w,
            None => self.new_doc(cx),
        };
        if let Some(d) = self.docs.get_mut(&w) {
            let first = text.lines().next().unwrap_or("");
            let lang = match Language::detect(None, first) {
                Language::Plain => Language::Markdown,
                l => l,
            };
            d.content = aizome_kit::EditorContent::new(text);
            d.content.soft_tabs = self.settings.soft_tabs;
            d.set_language(lang);
            d.text_dirty = !text.is_empty();
            d.autosaved = false;
            d.autosave_seq += 1;
        }
        self.edited(w, cx);
        cx.activate(w);
        w
    }

    /// Open `path`: bring its window forward if it's open, else read it
    /// into the untouched Untitled window or a new one.
    pub fn open_path(&mut self, path: &Path, cx: &mut Cx<Msg>) {
        // The shell's Services hand selected text over in a file of its
        // own (Open in Sumi): it becomes an untitled document.
        if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
            && path.starts_with(run.join("aizome-services"))
            && let Ok(text) = std::fs::read_to_string(path)
        {
            let _ = std::fs::remove_file(path);
            self.new_untitled_with(&text, cx);
            return;
        }
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let path = files::resolve(&path);
        if let Some(w) = self.doc_for_path(&path) {
            cx.activate(w);
            self.focused = Some(w);
            return;
        }
        let big = std::fs::metadata(&path).map(|m| m.len() > READ_IN_BACKGROUND).unwrap_or(false);
        let reuse = self.target(cx).filter(|w| self.docs.get(w).is_some_and(Doc::pristine));
        if big {
            let w = reuse.unwrap_or_else(|| self.new_window(cx, autosave::new_id(), 0));
            self.read_in_background(w, path, None, cx);
            return;
        }
        match doc::load(&path, None) {
            Ok(l) => {
                let w = reuse.unwrap_or_else(|| self.new_window(cx, autosave::new_id(), 0));
                self.installed(w, l, cx);
            }
            Err(e) => self.failed_to_open(&path, &e, cx),
        }
    }

    fn read_in_background(&mut self, w: WindowKey, path: PathBuf, encoding: Option<Encoding>, cx: &mut Cx<Msg>) {
        let seq = self.next_seq();
        if let Some(d) = self.docs.get_mut(&w) {
            d.loading = Some(seq);
            d.path = Some(path.clone());
            cx.set_title(w, d.title());
        }
        cx.spawn(move || Msg::Loaded(w, seq, doc::load(&path, encoding).map(Box::new)));
    }

    fn failed_to_open(&mut self, path: &Path, why: &str, cx: &mut Cx<Msg>) {
        let name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        let alert = Alert::new(format!("“{name}” couldn't be opened."), why.to_owned());
        match self.target(cx) {
            Some(w) => cx.alert(w, alert),
            None => cx.notify(&format!("“{name}” couldn't be opened"), why),
        }
    }

    fn installed(&mut self, w: WindowKey, l: Loaded, cx: &mut Cx<Msg>) {
        let path = l.path.clone();
        let Some(d) = self.docs.get_mut(&w) else { return };
        d.install(l);
        d.untitled = 0;
        cx.set_title(w, d.title());
        cx.focus("editor");
        let mime = files::mime_type(&path);
        self.remember(&path, mime);
        self.write_session();
    }

    /// Add to Open Recent and the desktop's recent files.
    fn remember(&mut self, path: &Path, mime: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(20);
        if let Some(list) = &self.places.recent {
            recent::add(list, path, mime, "Sumi", "'aizome-sumi %u'");
        }
    }

    fn write_session(&self) {
        let session = Session {
            documents: self
                .docs
                .values()
                .filter(|d| d.loading.is_none() || d.path.is_some())
                .map(Doc::session)
                .collect(),
        };
        self.store.write_session(&session);
    }

    /// Keep what's unsaved now (and the session).
    fn autosave_now(&mut self, w: WindowKey) {
        let Some(d) = self.docs.get_mut(&w) else { return };
        if d.text_dirty && !d.autosaved {
            self.store.write_text(&d.id, d.content.rope().clone());
        } else if !d.text_dirty {
            self.store.remove_text(&d.id);
        }
        d.autosaved = true;
        self.write_session();
    }

    /// Bring a document back as the session left it.
    fn restore(&mut self, sd: autosave::SessionDoc, cx: &mut Cx<Msg>) {
        let unsaved = if sd.unsaved { self.store.load_text(&sd.id) } else { None };
        let on_disk = sd.path.as_ref().is_some_and(|p| p.is_file());
        if unsaved.is_none() && !on_disk {
            return;
        }
        if let Some(p) = &sd.path
            && self.doc_for_path(p).is_some()
        {
            return;
        }
        let w = self.new_window(cx, sd.id.clone(), if sd.path.is_some() { 0 } else { sd.untitled.max(1) });
        let lang = Language::from_name(&sd.language);
        match unsaved {
            Some(text) => {
                let saved = sd.path.as_ref().and_then(|p| doc::load(p, Some(sd.encoding)).ok());
                let Some(d) = self.docs.get_mut(&w) else { return };
                d.path = sd.path.clone();
                d.encoding = sd.encoding;
                d.line_ending = sd.line_ending;
                d.content = aizome_kit::EditorContent::new(&text);
                d.content.soft_tabs = self.settings.soft_tabs;
                match saved {
                    Some(s) => {
                        d.saved = s.content.rope().clone();
                        d.stamp = sd.stamp.or(s.stamp);
                    }
                    None => d.saved = ropey::Rope::new(),
                }
                d.text_dirty = *d.content.rope() != d.saved;
                d.autosaved = true;
                let detected = Language::detect(d.path.as_deref(), text.lines().next().unwrap_or(""));
                d.set_language(lang.unwrap_or(detected));
                d.content.set_selection(Selection { anchor: sd.selection.0, head: sd.selection.1 });
                cx.set_title(w, d.title());
            }
            None => {
                let path = sd.path.clone().unwrap_or_default();
                match doc::load(&path, None) {
                    Ok(l) => {
                        self.installed(w, l, cx);
                        if let Some(d) = self.docs.get_mut(&w) {
                            if let Some(l) = lang {
                                d.set_language(l);
                            }
                            d.content.set_selection(Selection { anchor: sd.selection.0, head: sd.selection.1 });
                        }
                    }
                    Err(_) => {
                        self.docs.remove(&w);
                        cx.close_window(w);
                    }
                }
            }
        }
    }

    // Saving.

    /// Save `w` (to `to`, else its file, else ask where): whether it's
    /// saved now.
    fn save(&mut self, w: WindowKey, to: Option<PathBuf>, force: bool, cx: &mut Cx<Msg>) -> bool {
        let Some(d) = self.docs.get(&w) else { return false };
        let Some(path) = to.clone().or_else(|| d.path.clone()) else {
            self.ask_where(w, cx);
            return false;
        };
        let same_file = d.path.as_deref() == Some(path.as_path());
        if same_file && !force && d.changed_on_disk() {
            let name = d.name();
            cx.alert(
                w,
                Alert::new(
                    format!("“{name}” was changed by another program."),
                    "Saving replaces what's on disk now with this window's text.",
                )
                .destructive_button("Save Anyway", Msg::Overwrite(w))
                .button("Revert", Msg::Reload(w, None))
                .cancel_button("Cancel"),
            );
            return false;
        }
        let bytes = match d.bytes() {
            Ok(b) => b,
            Err(u) => {
                let enc = d.encoding.name();
                cx.alert(
                    w,
                    Alert::new(
                        format!("“{}” can't be saved in {enc}.", u.ch),
                        format!("{enc} has no “{}” (character {}). Save it in UTF-8 instead?", u.ch, u.at + 1),
                    )
                    .default_button("Save as UTF-8", Msg::SaveUtf8(w))
                    .cancel_button("Cancel"),
                );
                return false;
            }
        };
        if let Err(e) = files::save_atomic(&path, &bytes) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            cx.alert(w, Alert::new(format!("“{name}” couldn't be saved."), e.to_string()));
            return false;
        }
        let path = files::resolve(&path);
        let mime = files::mime_type(&path);
        let Some(d) = self.docs.get_mut(&w) else { return false };
        let was_untitled = d.path.is_none();
        d.path = Some(path.clone());
        d.untitled = 0;
        d.saved = d.content.rope().clone();
        d.text_dirty = false;
        d.format_dirty = false;
        d.stamp = files::stamp(&path);
        d.notice = None;
        if was_untitled || !same_file {
            let first = crate::syntax::cache::line_text(d.content.rope(), 0).into_owned();
            let detected = Language::detect(Some(&path), &first);
            if detected != Language::Plain || d.language == Language::Plain {
                d.set_language(detected);
            }
        }
        cx.set_title(w, d.title());
        self.store.remove_text(&d.id);
        d.autosaved = true;
        self.remember(&path, mime);
        self.write_session();
        if self.closing.remove(&w) {
            self.close_now(w, cx);
        }
        true
    }

    fn ask_where(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(d) = self.docs.get(&w) else { return };
        let name = match (&d.path, d.language) {
            (Some(p), _) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            (None, Language::Markdown) => format!("{}.md", d.name()),
            (None, _) => format!("{}.txt", d.name()),
        };
        let dir = d
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| self.places.documents.clone())
            .or_else(|| {
                let home = std::env::var_os("HOME").map(PathBuf::from)?;
                let docs = home.join("Documents");
                Some(if docs.is_dir() { docs } else { home })
            });
        let mut dialog = FileDialog::new().title("Save As").file_name(name);
        if let Some(dir) = dir {
            dialog = dialog.directory(dir);
        }
        cx.save_panel(w, dialog, move |p| Msg::SaveTo(w, p));
    }

    // Closing.

    /// The user asked to close `w`: whether it closes now (a dirty
    /// document asks first).
    fn request_close(&mut self, w: WindowKey, cx: &mut Cx<Msg>) -> bool {
        let Some(d) = self.docs.get(&w) else { return true };
        if !d.dirty() {
            self.forget(w);
            return true;
        }
        let name = d.name();
        cx.alert(
            w,
            Alert::new(format!("Save the changes to “{name}”?"), "If you don't, they're gone.")
                .default_button("Save", Msg::Close(w, CloseChoice::Save))
                .destructive_button("Don't Save", Msg::Close(w, CloseChoice::Discard))
                .cancel_button("Cancel"),
        );
        false
    }

    /// `w` is closing: its autosave and its place in the session go.
    fn forget(&mut self, w: WindowKey) {
        if let Some(d) = self.docs.remove(&w) {
            self.store.remove_text(&d.id);
        }
        self.closing.remove(&w);
        if self.focused == Some(w) {
            self.focused = None;
        }
        self.write_session();
        if self.docs.is_empty() {
            self.store.flush();
        }
    }

    fn close_now(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        self.forget(w);
        cx.close_window(w);
    }

    // Find.

    fn search(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let seq = self.next_seq();
        let Some(d) = self.docs.get_mut(&w) else { return };
        d.find.seq = seq;
        let version = d.content.version();
        if d.find.query.text.is_empty() {
            d.find.matches = Arc::new(Vec::new());
            d.find.current = None;
            d.find.error = None;
            d.find.version = Some(version);
            return;
        }
        let re = match d.find.query.compile() {
            Ok(re) => re,
            Err(e) => {
                d.find.error = Some(e);
                d.find.matches = Arc::new(Vec::new());
                d.find.current = None;
                d.find.version = Some(version);
                return;
            }
        };
        d.find.error = None;
        let rope = d.content.rope().clone();
        if rope.len_bytes() <= SEARCH_IN_BACKGROUND {
            let found = find::search(&find::text_of(&rope), &re, find::LIMIT);
            self.found(w, seq, version, Ok(Arc::new(found)));
        } else {
            cx.spawn(move || {
                Msg::Searched(w, seq, version, Ok(Arc::new(find::search(&find::text_of(&rope), &re, find::LIMIT))))
            });
        }
    }

    fn found(&mut self, w: WindowKey, seq: u64, version: u64, result: Result<Arc<Vec<Range<usize>>>, String>) {
        let Some(d) = self.docs.get_mut(&w) else { return };
        if d.find.seq != seq {
            return;
        }
        match result {
            Ok(m) => {
                d.find.matches = m;
                d.find.error = None;
            }
            Err(e) => {
                d.find.matches = Arc::new(Vec::new());
                d.find.error = Some(e);
            }
        }
        d.find.version = Some(version);
        // The match at or after the selection is the current one.
        let sel = d.content.selection().range();
        d.find.current = d
            .find
            .matches
            .iter()
            .position(|r| *r == sel)
            .or_else(|| d.find.matches.iter().position(|r| r.start >= sel.start));
        if d.find.current.is_none() && !d.find.matches.is_empty() {
            d.find.current = Some(0);
        }
    }

    /// The matches, searched again if the text changed since.
    fn fresh_matches(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let stale = self
            .docs
            .get(&w)
            .is_some_and(|d| d.find.version != Some(d.content.version()) && !d.find.query.text.is_empty());
        if stale {
            self.search(w, cx);
        }
    }

    /// Select the next (or previous) match.
    fn find_step(&mut self, w: WindowKey, backwards: bool, cx: &mut Cx<Msg>) {
        self.fresh_matches(w, cx);
        let Some(d) = self.docs.get_mut(&w) else { return };
        let n = d.find.matches.len();
        if n == 0 {
            return;
        }
        let sel = d.content.selection().range();
        let m = &d.find.matches;
        let i = if backwards {
            m.iter().rposition(|r| r.end <= sel.start && *r != sel).unwrap_or(n - 1)
        } else {
            m.iter()
                .position(|r| r.start >= sel.end && *r != sel)
                .or_else(|| m.iter().position(|r| r.start > sel.start))
                .unwrap_or(0)
        };
        let r = m[i].clone();
        d.find.current = Some(i);
        d.content.set_selection(Selection { anchor: r.start, head: r.end });
    }

    fn replace_one(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        self.fresh_matches(w, cx);
        let Some(d) = self.docs.get_mut(&w) else { return };
        let sel = d.content.selection().range();
        if !d.find.matches.contains(&sel) {
            self.find_step(w, false, cx);
            return;
        }
        let Ok(re) = d.find.query.compile() else { return };
        let matched = d.content.rope().slice(sel.clone()).to_string();
        let with = find::expand(&re, &matched, &d.find.replacement, d.find.query.regex);
        let at = sel.start;
        let len = with.chars().count();
        d.replace(sel, with, Selection::caret(at + len));
        self.edited(w, cx);
        self.search(w, cx);
        self.find_step(w, false, cx);
    }

    fn replace_all(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(d) = self.docs.get_mut(&w) else { return };
        let Ok(re) = d.find.query.compile() else { return };
        let text = find::text_of(d.content.rope()).into_owned();
        if let Some((range, with, _count)) = find::replace_all(&text, &re, &d.find.replacement, d.find.query.regex) {
            let at = range.start;
            d.replace(range, with, Selection::caret(at));
            self.edited(w, cx);
        }
        self.search(w, cx);
    }

    /// After an edit: autosave soon, the title, the matches.
    fn edited(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(d) = self.docs.get(&w) else { return };
        cx.set_title(w, d.title());
        cx.after(AUTOSAVE_AFTER, Msg::Autosave(w, d.autosave_seq));
        if d.find.shown && !d.find.query.text.is_empty() && d.content.rope().len_bytes() <= SEARCH_IN_BACKGROUND {
            self.search(w, cx);
        }
    }

    /// Apply a Format command's edit.
    fn apply(&mut self, w: WindowKey, edit: Option<format::Edit>, cx: &mut Cx<Msg>) {
        let Some(e) = edit else { return };
        let Some(d) = self.docs.get_mut(&w) else { return };
        d.replace(e.range, e.text, e.select);
        self.edited(w, cx);
    }

    fn command(&mut self, w: WindowKey, c: Command, cx: &mut Cx<Msg>) {
        if !self.docs.contains_key(&w) {
            return;
        }
        let md = self.docs[&w].language == Language::Markdown;
        let (rope, sel) = {
            let d = &self.docs[&w];
            (d.content.rope().clone(), d.content.selection())
        };
        match c {
            Command::Save => {
                self.save(w, None, false, cx);
            }
            Command::SaveAs => self.ask_where(w, cx),
            Command::Revert => {
                let Some(d) = self.docs.get(&w) else { return };
                if d.path.is_none() {
                    return;
                }
                let name = d.name();
                cx.alert(
                    w,
                    Alert::new(
                        format!("Revert to the saved “{name}”?"),
                        "The changes made since it was saved will be lost.",
                    )
                    .destructive_button("Revert", Msg::Reload(w, None))
                    .cancel_button("Cancel"),
                );
            }
            Command::Reopen(e) => {
                let dirty = self.docs[&w].dirty();
                if dirty {
                    let name = self.docs[&w].name();
                    cx.alert(
                        w,
                        Alert::new(
                            format!("Reopen “{name}” as {}?", e.name()),
                            "The changes made since it was saved will be lost.",
                        )
                        .destructive_button("Reopen", Msg::Reload(w, Some(e)))
                        .cancel_button("Cancel"),
                    );
                } else {
                    self.update(Msg::Reload(w, Some(e)), cx);
                }
            }
            Command::Close => {
                if self.request_close(w, cx) {
                    cx.close_window(w);
                }
            }
            Command::SetLanguage(l) => {
                if let Some(d) = self.docs.get_mut(&w) {
                    d.set_language(l);
                }
                self.write_session();
            }
            Command::SetEncoding(e) => {
                if let Some(d) = self.docs.get_mut(&w)
                    && d.encoding != e
                {
                    d.encoding = e;
                    d.format_dirty = true;
                    d.notice = None;
                    cx.set_title(w, d.title());
                }
                self.write_session();
            }
            Command::SetLineEnding(e) => {
                if let Some(d) = self.docs.get_mut(&w)
                    && d.line_ending != e
                {
                    d.line_ending = e;
                    d.format_dirty = true;
                    d.notice = None;
                    cx.set_title(w, d.title());
                }
                self.write_session();
            }
            Command::Inline(i) if md => self.apply(w, format::toggle_inline(&rope, sel, i), cx),
            Command::Link if md => {
                let r = sel.range();
                let selected: String = rope.slice(r.clone()).to_string();
                let looks_like_url =
                    |s: &str| s.starts_with("http://") || s.starts_with("https://") || s.starts_with("mailto:");
                let clip = cx
                    .clipboard_text()
                    .map(|c| c.trim().to_owned())
                    .filter(|c| looks_like_url(c) && !c.contains(char::is_whitespace));
                let (text, url) = if looks_like_url(selected.trim()) {
                    (String::new(), selected.trim().to_owned())
                } else {
                    (selected, clip.unwrap_or_default())
                };
                let key = cx.show_sheet(w);
                let focus = if text.is_empty() { "link-text" } else { "link-url" };
                self.sheets.insert(key, Sheet::Link { parent: w, text, url, range: r });
                cx.focus(focus);
            }
            Command::Heading(l) if md => self.apply(w, format::heading(&rope, sel, l), cx),
            Command::Quote if md => self.apply(w, format::toggle_quote(&rope, sel), cx),
            Command::List(k) if md => self.apply(w, format::toggle_list(&rope, sel, k), cx),
            Command::CodeBlock if md => self.apply(w, Some(format::code_block(&rope, sel)), cx),
            Command::Rule if md => self.apply(w, Some(format::rule(&rope, sel)), cx),
            Command::CopyStyle if md => self.style = Some(format::copy_style(&rope, sel)),
            Command::PasteStyle if md => {
                if let Some(s) = self.style.clone() {
                    self.apply(w, format::paste_style(&rope, sel, &s), cx);
                }
            }
            Command::ShiftRight | Command::ShiftLeft => {
                let a = if c == Command::ShiftRight { EditorAction::Indent } else { EditorAction::Outdent };
                // Shift Right indents whole lines, even with a caret.
                if c == Command::ShiftRight && sel.is_empty() {
                    let line = rope.char_to_line(sel.head);
                    let start = rope.line_to_char(line);
                    let unit =
                        if self.settings.soft_tabs > 0 { " ".repeat(self.settings.soft_tabs) } else { "\t".to_owned() };
                    let n = unit.chars().count();
                    if let Some(d) = self.docs.get_mut(&w) {
                        d.replace(start..start, unit, Selection::caret(sel.head + n));
                    }
                    self.edited(w, cx);
                } else {
                    self.update(Msg::Edit(w, a), cx);
                }
            }
            Command::ZoomIn => self.update(Msg::Zoom(Some(true)), cx),
            Command::ZoomOut => self.update(Msg::Zoom(Some(false)), cx),
            Command::FindShow | Command::ReplaceShow => {
                let Some(d) = self.docs.get_mut(&w) else { return };
                let opening = !d.find.shown;
                d.find.shown = true;
                if c == Command::ReplaceShow {
                    d.find.replace = true;
                }
                // Opening the bar, a single-line selection is what's looked for.
                let s = d.content.selected_text();
                if opening && !s.is_empty() && !s.contains('\n') && s.len() < 1000 {
                    d.find.query.text = if d.find.query.regex { regex::escape(&s) } else { s };
                }
                self.search(w, cx);
                cx.focus(if c == Command::ReplaceShow && !self.docs[&w].find.query.text.is_empty() {
                    "replace"
                } else {
                    "find"
                });
            }
            Command::FindHide => {
                if let Some(d) = self.docs.get_mut(&w) {
                    d.find.shown = false;
                    d.find.replace = false;
                }
                cx.focus("editor");
            }
            Command::FindNext => self.find_step(w, false, cx),
            Command::FindPrevious => self.find_step(w, true, cx),
            Command::UseSelection => {
                let Some(d) = self.docs.get_mut(&w) else { return };
                let s = d.content.selected_text();
                if s.is_empty() {
                    return;
                }
                d.find.query.text = if d.find.query.regex { regex::escape(&s) } else { s };
                self.search(w, cx);
            }
            Command::FindQuery(q) => {
                if let Some(d) = self.docs.get_mut(&w) {
                    d.find.query.text = q;
                }
                self.search(w, cx);
                // Show the first match from the caret on, as it's typed.
                let target = self.docs.get(&w).and_then(|d| d.find.current.map(|i| d.find.matches[i].clone()));
                if let (Some(r), Some(d)) = (target, self.docs.get_mut(&w)) {
                    d.content.set_selection(Selection { anchor: r.start, head: r.end });
                }
            }
            Command::FindReplacement(r) => {
                if let Some(d) = self.docs.get_mut(&w) {
                    d.find.replacement = r;
                }
            }
            Command::FindCase | Command::FindWord | Command::FindRegex => {
                if let Some(d) = self.docs.get_mut(&w) {
                    let q = &mut d.find.query;
                    match c {
                        Command::FindCase => q.case = !q.case,
                        Command::FindWord => q.word = !q.word,
                        _ => q.regex = !q.regex,
                    }
                }
                self.search(w, cx);
            }
            Command::ReplaceOne => self.replace_one(w, cx),
            Command::ReplaceAll => self.replace_all(w, cx),
            Command::GotoLine => {
                let max = rope.len_lines();
                let key = cx.show_sheet(w);
                self.sheets.insert(key, Sheet::Goto { parent: w, line: String::new(), max });
                cx.focus("goto-line");
            }
            // Format commands outside Markdown.
            _ => {}
        }
    }

    fn sheet_of(&self, parent_kind: fn(&Sheet) -> bool) -> Option<WindowKey> {
        self.sheets.iter().find(|(_, s)| parent_kind(s)).map(|(k, _)| *k)
    }
}

impl Program for Sumi {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Sumi", "Text Editor").icon(Icon::FileText).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.notify = Some(cx.stream());
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), Msg::Launch);
        }
        let session = self.store.load_session();
        self.store.tidy(&session);
        for sd in session.documents {
            self.restore(sd, cx);
        }
        cx.send(Msg::LaunchDone);
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        for p in paths {
            self.open_path(&p, cx);
        }
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.sheets.remove(&window).is_some() {
            return true;
        }
        self.request_close(window, cx)
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        if let Some(k) = cx.key_window()
            && self.docs.contains_key(&k)
        {
            self.focused = Some(k);
        }
        match msg {
            Msg::Cmd(c) => {
                if let Some(w) = self.target(cx) {
                    self.command(w, c, cx);
                }
            }
            Msg::On(w, c) => {
                self.focused = Some(w);
                self.command(w, c, cx);
            }
            Msg::Edit(w, action) => {
                self.focused = Some(w);
                // Return in a Markdown list carries the list on.
                if action == EditorAction::Newline
                    && let Some(d) = self.docs.get(&w)
                    && d.language == Language::Markdown
                    && let Some(e) = format::continue_list(d.content.rope(), d.content.selection())
                {
                    self.apply(w, Some(e), cx);
                    return;
                }
                let Some(d) = self.docs.get_mut(&w) else { return };
                if d.loading.is_some() {
                    return;
                }
                let edits = action.edits();
                if d.perform(action) || edits {
                    self.edited(w, cx);
                }
            }
            Msg::Find(w, req) => {
                let c = match req {
                    FindRequest::Show => Command::FindShow,
                    FindRequest::Replace => Command::ReplaceShow,
                    FindRequest::Next => Command::FindNext,
                    FindRequest::Previous => Command::FindPrevious,
                    FindRequest::UseSelection(_) => Command::UseSelection,
                };
                self.command(w, c, cx);
            }
            Msg::New => {
                self.new_doc(cx);
            }
            Msg::Open => {
                let dir = self
                    .focused_doc()
                    .and_then(|d| d.path.as_ref())
                    .and_then(|p| p.parent())
                    .map(Path::to_path_buf)
                    .or_else(|| self.places.documents.clone());
                let mut dialog = FileDialog::new().title("Open").multiple();
                if let Some(d) = dir {
                    dialog = dialog.directory(d);
                }
                match self.target(cx) {
                    Some(w) => cx.open_panel(w, dialog, Msg::Opened),
                    None => {
                        let w = self.new_doc(cx);
                        cx.open_panel(w, dialog, Msg::Opened);
                    }
                }
            }
            Msg::Opened(paths) => {
                for p in paths.unwrap_or_default() {
                    self.open_path(&p, cx);
                }
            }
            Msg::OpenRecent(p) => {
                if p.exists() {
                    self.open_path(&p, cx);
                } else {
                    self.recent.retain(|r| *r != p);
                    self.failed_to_open(&p, "It isn't there any more.", cx);
                }
            }
            Msg::ClearRecent => {
                self.recent.clear();
                if let Some(list) = &self.places.recent
                    && let Ok(t) = std::fs::read_to_string(list)
                {
                    let _ = files::save_atomic(list, recent::forget(&t, "Sumi").as_bytes());
                }
            }
            Msg::SaveTo(w, Some(path)) => {
                self.save(w, Some(path), true, cx);
            }
            Msg::SaveTo(w, None) => {
                self.closing.remove(&w);
            }
            Msg::Overwrite(w) => {
                self.save(w, None, true, cx);
            }
            Msg::SaveUtf8(w) => {
                if let Some(d) = self.docs.get_mut(&w) {
                    d.encoding = Encoding::Utf8;
                }
                self.save(w, None, false, cx);
            }
            Msg::Reload(w, encoding) => {
                let Some(path) = self.docs.get(&w).and_then(|d| d.path.clone()) else { return };
                let big = std::fs::metadata(&path).map(|m| m.len() > READ_IN_BACKGROUND).unwrap_or(false);
                if big {
                    self.read_in_background(w, path, encoding, cx);
                } else {
                    match doc::load(&path, encoding) {
                        Ok(l) => {
                            let sel = self.docs[&w].content.selection();
                            self.installed(w, l, cx);
                            if let Some(d) = self.docs.get_mut(&w) {
                                d.content.set_selection(sel);
                                self.store.remove_text(&d.id);
                            }
                        }
                        Err(e) => self.failed_to_open(&path, &e, cx),
                    }
                }
            }
            Msg::Loaded(w, seq, result) => {
                if self.docs.get(&w).and_then(|d| d.loading) != Some(seq) {
                    return;
                }
                match result {
                    Ok(l) => self.installed(w, *l, cx),
                    Err(e) => {
                        let path = self.docs.get(&w).and_then(|d| d.path.clone()).unwrap_or_default();
                        if let Some(d) = self.docs.get_mut(&w) {
                            d.loading = None;
                            d.path = None;
                            d.untitled = 1;
                        }
                        self.failed_to_open(&path, &e, cx);
                    }
                }
            }
            Msg::Close(w, CloseChoice::Save) => {
                self.closing.insert(w);
                self.save(w, None, false, cx);
            }
            Msg::Close(w, CloseChoice::Discard) => self.close_now(w, cx),
            Msg::Searched(w, seq, version, result) => self.found(w, seq, version, result),
            Msg::Autosave(w, seq) => {
                if self.docs.get(&w).is_some_and(|d| d.autosave_seq == seq) {
                    self.autosave_now(w);
                }
            }
            Msg::HighlightSoon => {
                // In a frame of its own, after the one that asked: the pass
                // spends the colouring budget, and the frame that asked (a
                // jump to the end of a large file) has its own work.
                if !self.highlight_soon {
                    self.highlight_soon = true;
                    cx.after(HIGHLIGHT_PASS, Msg::Highlight);
                }
            }
            Msg::Highlight => {
                self.highlight_soon = false;
                for d in self.docs.values_mut() {
                    if let Ok(mut h) = d.highlight.lock() {
                        h.incomplete = false;
                        h.scheduled = false;
                        h.pump = true;
                    }
                    d.styles += 1;
                }
            }
            Msg::ToggleNumbers => {
                self.settings.line_numbers = !self.settings.line_numbers;
                self.settings.save(self.places.settings.as_deref());
            }
            Msg::ToggleWrap => {
                self.settings.wrap = !self.settings.wrap;
                self.settings.save(self.places.settings.as_deref());
            }
            Msg::Zoom(z) => {
                self.settings.size = match z {
                    Some(bigger) => self.settings.zoomed(bigger),
                    None => settings::DEFAULT_SIZE,
                };
                self.settings.save(self.places.settings.as_deref());
            }
            Msg::Font(proportional) => {
                self.settings.proportional = proportional;
                self.settings.save(self.places.settings.as_deref());
            }
            Msg::LinkText(t) => {
                if let Some(k) = self.sheet_of(|s| matches!(s, Sheet::Link { .. }))
                    && let Some(Sheet::Link { text, .. }) = self.sheets.get_mut(&k)
                {
                    *text = t;
                }
            }
            Msg::LinkUrl(u) => {
                if let Some(k) = self.sheet_of(|s| matches!(s, Sheet::Link { .. }))
                    && let Some(Sheet::Link { url, .. }) = self.sheets.get_mut(&k)
                {
                    *url = u;
                }
            }
            Msg::LinkDone(ok) => {
                let Some(k) = self.sheet_of(|s| matches!(s, Sheet::Link { .. })) else { return };
                let Some(Sheet::Link { parent, text, url, range }) = self.sheets.remove(&k) else { return };
                cx.close_window(k);
                cx.focus("editor");
                if ok && let Some(d) = self.docs.get(&parent) {
                    let sel = Selection { anchor: range.start, head: range.end };
                    let e = format::link(d.content.rope(), sel, &text, url.trim());
                    self.apply(parent, Some(e), cx);
                }
            }
            Msg::GotoText(t) => {
                if let Some(k) = self.sheet_of(|s| matches!(s, Sheet::Goto { .. }))
                    && let Some(Sheet::Goto { line, .. }) = self.sheets.get_mut(&k)
                {
                    *line = t;
                }
            }
            Msg::GotoDone(ok) => {
                let Some(k) = self.sheet_of(|s| matches!(s, Sheet::Goto { .. })) else { return };
                let Some(Sheet::Goto { parent, line, .. }) = self.sheets.remove(&k) else { return };
                cx.close_window(k);
                cx.focus("editor");
                if !ok {
                    return;
                }
                let Some(d) = self.docs.get_mut(&parent) else { return };
                let mut parts = line.trim().split([':', ',', ' ']).filter(|p| !p.is_empty());
                let Some(Ok(l)) = parts.next().map(str::parse::<usize>) else { return };
                let col = parts.next().and_then(|c| c.parse::<usize>().ok()).unwrap_or(1);
                let rope = d.content.rope();
                let l = l.clamp(1, rope.len_lines()) - 1;
                let start = rope.line_to_char(l);
                let len = crate::syntax::cache::line_text(rope, l).chars().count();
                d.content.set_selection(Selection::caret(start + (col.max(1) - 1).min(len)));
            }
            Msg::Launch(args) => {
                let paths: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();
                if paths.is_empty() {
                    match self.target(cx) {
                        Some(w) => cx.activate(w),
                        None => {
                            self.new_doc(cx);
                        }
                    }
                } else {
                    for p in paths {
                        self.open_path(&p, cx);
                    }
                    if let Some(w) = self.focused {
                        cx.activate(w);
                    }
                }
            }
            Msg::OpenUrl(url) => {
                if self.open_links {
                    cx.open_url(&url);
                }
                self.links.push(url);
            }
            Msg::LaunchDone => {
                if self.docs.is_empty() {
                    self.new_doc(cx);
                }
            }
            Msg::Quit => {
                // Everything is kept, saved or not, and comes back next time.
                let windows: Vec<WindowKey> = self.docs.keys().copied().collect();
                for w in windows {
                    if let Some(d) = self.docs.get_mut(&w) {
                        d.autosaved = false;
                    }
                    self.autosave_now(w);
                }
                self.store.flush();
                cx.quit();
            }
            Msg::Nothing => {}
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some(s) = self.sheets.get(&window) {
            return crate::view::sheet(window, s);
        }
        match self.docs.get(&window) {
            Some(d) => crate::view::window(d, &self.settings),
            None => aizome_kit::column(Vec::new()).into(),
        }
    }

    fn menus(&self) -> MenuBar<Msg> {
        crate::menus::menus(self)
    }
}
