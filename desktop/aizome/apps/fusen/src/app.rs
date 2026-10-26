//! Fusen, the program: the notes, the main window, the stickies, and what
//! each message does.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use aizome_kit::menu::{EDIT, HELP};
use aizome_kit::prelude::*;
use aizome_kit::{EditorAction, EditorContent, MsgSender};
use aizome_sumi::format::{self, Inline, List};
use aizome_sumi::syntax::{Highlight, Language};
use serde::{Deserialize, Serialize};

use crate::notes::{self, Meta, Note, NoteColor, Store};
use crate::wm::{self, Place};

pub const APP_ID: &str = "org.aizome.Fusen";
/// The sidebar's Pinned, as a folder that isn't one.
pub const PINNED: &str = "\u{1}pinned";
const SAVE_AFTER: Duration = Duration::from_millis(500);
const RESCAN_EVERY: Duration = Duration::from_secs(3);
const TRACK_EVERY: Duration = Duration::from_secs(4);
pub const STICKY_SIZE: Size = Size::new(260.0, 240.0);

/// Where Fusen keeps things; tests give their own folders and no
/// compositor.
#[derive(Clone, Debug, Default)]
pub struct Places {
    /// The notes: `~/Notes`.
    pub notes: PathBuf,
    /// Stickies' places: `$XDG_STATE_HOME/aizome/fusen`.
    pub state: Option<PathBuf>,
    /// The freedesktop Trash: `$XDG_DATA_HOME/Trash`.
    pub trash: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME/autostart`, where the stickies' login item goes.
    pub autostart: Option<PathBuf>,
    /// Ask Aizome's compositor to keep stickies above and where they were.
    pub compositor: bool,
}

fn xdg(var: &str, home_rel: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(home_rel)))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl Places {
    pub fn user() -> Places {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
        Places {
            notes: home.join("Notes"),
            state: Some(xdg("XDG_STATE_HOME", ".local/state").join("aizome/fusen")),
            trash: Some(xdg("XDG_DATA_HOME", ".local/share").join("Trash")),
            autostart: Some(xdg("XDG_CONFIG_HOME", ".config").join("autostart")),
            compositor: true,
        }
    }
}

/// A note being edited (in the main window, a sticky, or both).
pub struct Open {
    pub content: EditorContent,
    pub highlight: Arc<Mutex<Highlight>>,
    pub styles: u64,
    pub dirty: bool,
    /// Last write failure; cleared only after a successful save.
    pub save_error: Option<String>,
    pub seq: u64,
}

impl Open {
    fn new(text: &str) -> Open {
        let mut h = Highlight::new(Language::Markdown);
        h.budget = Duration::from_millis(3);
        Open {
            content: EditorContent::new(text),
            highlight: Arc::new(Mutex::new(h)),
            styles: 0,
            dirty: false,
            save_error: None,
            seq: 0,
        }
    }

    /// The text changed under it (another program wrote the file).
    fn reload(&mut self, text: &str) {
        let sel = self.content.selection();
        self.content.set_text(text);
        self.content.set_selection(sel);
        if let Ok(mut h) = self.highlight.lock() {
            h.reset(Language::Markdown);
        }
        self.styles += 1;
    }
}

/// A sticky as kept between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSticky {
    /// The note, by its path under the notes folder.
    pub note: String,
    pub place: Option<Place>,
    pub float: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedStickies {
    pub stickies: Vec<SavedSticky>,
}

/// An open sticky.
#[derive(Clone, Debug)]
pub struct Sticky {
    pub note: u64,
    pub title: String,
    pub float: bool,
}

/// What a launch asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// New notes with these texts.
    pub new: Vec<String>,
    /// Files: a note to show, or text to take as a new note.
    pub files: Vec<PathBuf>,
    /// At login: only the stickies (the window too if there are none).
    pub stickies_only: bool,
}

impl Request {
    /// `--new TEXT`, `--new-from-stdin` (already read into `--new`),
    /// `--stickies`, files.
    pub fn parse(args: &[String]) -> Request {
        let mut r = Request::default();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--new" => {
                    i += 1;
                    r.new.push(args.get(i).cloned().unwrap_or_default());
                }
                "--stickies" => r.stickies_only = true,
                a if a.starts_with("--new=") => r.new.push(a["--new=".len()..].to_owned()),
                a if a.starts_with('-') => {}
                a => r.files.push(PathBuf::from(a)),
            }
            i += 1;
        }
        r
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    New,
    NewWith(String),
    NewFolder,
    FolderName(String),
    FolderDone(bool),
    Delete,
    Pin,
    Color(NoteColor),
    /// Open this note (else the selected one) as a sticky.
    Sticky(Option<u64>),
    /// Keep the key sticky above other windows, or not.
    Float(bool),
    Folder(Option<String>),
    MoveTo(String),
    Search(String),
    FocusSearch,
    Select(Option<u64>),
    Edit(u64, EditorAction),
    /// Tick or untick the task on line `n` of a note.
    Tick(u64, usize),
    Inline(Inline),
    List(List),
    OpenUrl(String),
    ShowFolder,
    ShowMain,
    CloseKey,
    Save(u64, u64),
    RetrySave,
    Close(WindowKey),
    Rescan,
    Track,
    Placed(Vec<(String, Place, bool)>),
    Highlight,
    Launch(Vec<String>),
    LaunchDone,
    Quit,
    Nothing,
}

pub struct Fusen {
    pub places: Places,
    pub store: Store,
    pub notes: Vec<Note>,
    pub folders: Vec<String>,
    /// The sidebar's choice: none for All Notes, a folder, or PINNED.
    pub folder: Option<String>,
    pub query: String,
    pub selected: Option<u64>,
    pub open: HashMap<u64, Open>,
    pub main: Option<WindowKey>,
    pub stickies: BTreeMap<WindowKey, Sticky>,
    pub saved: SavedStickies,
    /// The New Folder sheet (its window, and the name typed).
    pub folder_sheet: Option<(WindowKey, String)>,
    pub request: Request,
    pub instance: Option<std::os::unix::net::UnixListener>,
    /// Links ❖-clicked; opened unless `open_links` is off (tests).
    pub links: Vec<String>,
    pub open_links: bool,
    notify: Option<MsgSender<Msg>>,
    tracking: bool,
}

impl Drop for Fusen {
    fn drop(&mut self) {
        self.save_all();
    }
}

impl Fusen {
    pub fn new(places: Places, request: Request) -> Fusen {
        let store = Store::new(places.notes.clone());
        let saved = places
            .state
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("stickies.json")).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut f = Fusen {
            places,
            store,
            notes: Vec::new(),
            folders: Vec::new(),
            folder: None,
            query: String::new(),
            selected: None,
            open: HashMap::new(),
            main: None,
            stickies: BTreeMap::new(),
            saved,
            folder_sheet: None,
            request,
            instance: None,
            links: Vec::new(),
            open_links: true,
            notify: None,
            tracking: false,
        };
        f.rescan();
        f
    }

    pub fn note(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    fn note_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn selected_note(&self) -> Option<&Note> {
        self.selected.and_then(|s| self.note(s))
    }

    fn on_disk(n: &Note) -> bool {
        !n.path.as_os_str().is_empty()
    }

    /// Pinned first, then the newest.
    fn sort(&mut self) {
        self.notes.sort_by(|a, b| {
            b.meta.pinned.cmp(&a.meta.pinned).then(b.modified.cmp(&a.modified)).then(a.title.cmp(&b.title))
        });
    }

    /// The notes the list shows: the folder's, matching the search.
    pub fn shown(&self) -> Vec<&Note> {
        let q = self.query.trim().to_lowercase();
        self.notes
            .iter()
            .filter(|n| match self.folder.as_deref() {
                None => true,
                Some(PINNED) => n.meta.pinned,
                Some(f) => n.folder == f || n.folder.starts_with(&format!("{f}/")),
            })
            .filter(|n| q.is_empty() || n.title.to_lowercase().contains(&q) || n.text.to_lowercase().contains(&q))
            .collect()
    }

    /// Read the folder again: new notes, changed ones (not being edited),
    /// gone ones.
    pub fn rescan(&mut self) {
        let ids: BTreeMap<PathBuf, u64> =
            self.notes.iter().filter(|n| Self::on_disk(n)).map(|n| (n.path.clone(), n.id)).collect();
        let mut scanned = self.store.scan(&ids);
        // New notes not yet written stay; notes being edited keep their text.
        for n in &self.notes {
            if !Self::on_disk(n) {
                scanned.push(n.clone());
            } else if let Some(o) = self.open.get(&n.id)
                && o.dirty
            {
                if let Some(s) = scanned.iter_mut().find(|s| s.id == n.id) {
                    *s = n.clone();
                } else {
                    // An unreadable/missing directory must not erase a draft.
                    scanned.push(n.clone());
                }
            }
        }
        for s in &scanned {
            if let Some(o) = self.open.get_mut(&s.id)
                && !o.dirty
                && *o.content.rope() != *s.text
            {
                o.reload(&s.text);
            }
        }
        let live: Vec<u64> = scanned.iter().map(|n| n.id).collect();
        self.open.retain(|id, _| live.contains(id));
        if self.selected.is_some_and(|s| !live.contains(&s)) {
            self.selected = None;
        }
        self.notes = scanned;
        self.folders = self.store.folders();
        self.sort();
    }

    fn ensure_open(&mut self, id: u64) {
        if self.open.contains_key(&id) {
            return;
        }
        if let Some(n) = self.note(id) {
            let mut o = Open::new(&n.text);
            if let (Some(tx), Ok(mut h)) = (&self.notify, o.highlight.lock()) {
                let tx = tx.clone();
                h.notify = Some(Box::new(move || {
                    tx.send(Msg::Highlight);
                }));
            }
            o.styles = 0;
            self.open.insert(id, o);
        }
    }

    /// Show the main window (open it if it's closed).
    fn show_main(&mut self, cx: &mut Cx<Msg>) -> WindowKey {
        match self.main {
            Some(w) => {
                cx.activate(w);
                w
            }
            None => {
                let w = cx.open_window(WindowSpec::new("Fusen").size(900.0, 580.0).min_size(640.0, 360.0));
                self.main = Some(w);
                w
            }
        }
    }

    fn select(&mut self, id: Option<u64>) {
        // An empty new note goes when it's left.
        if let Some(old) = self.selected
            && Some(old) != id
            && self.note(old).is_some_and(|n| !Self::on_disk(n) && n.text.trim().is_empty())
            && !self.stickies.values().any(|s| s.note == old)
        {
            self.notes.retain(|n| n.id != old);
            self.open.remove(&old);
        }
        self.selected = id;
        if let Some(id) = id {
            self.ensure_open(id);
        }
    }

    /// The folder new notes go in.
    fn current_folder(&self) -> String {
        match self.folder.as_deref() {
            None | Some(PINNED) => String::new(),
            Some(f) => f.to_owned(),
        }
    }

    /// A new note with `text`, selected; written at once if it has text.
    fn new_note(&mut self, text: &str, cx: &mut Cx<Msg>) -> u64 {
        let id = self.store.fresh_id();
        let note = Note {
            id,
            path: PathBuf::new(),
            folder: self.current_folder(),
            title: notes::title_of(text),
            preview: notes::preview_of(text),
            modified: SystemTime::now(),
            text: text.into(),
            meta: Meta { auto_named: true, ..Meta::default() },
        };
        self.notes.insert(0, note);
        self.sort();
        self.select(Some(id));
        if !text.trim().is_empty() {
            if let Some(o) = self.open.get_mut(&id) {
                o.dirty = true;
            }
            self.write(id);
        }
        let w = self.show_main(cx);
        let _ = w;
        cx.focus("editor");
        id
    }

    /// Write a note's file (making the folder on first use), renaming it
    /// with its title if Fusen named it.
    fn write(&mut self, id: u64) -> bool {
        let Some(o) = self.open.get_mut(&id) else { return true };
        if !o.dirty {
            return true;
        }
        let text = o.content.text();
        let Some(n) = self.notes.iter().find(|n| n.id == id).cloned() else { return true };
        if !Self::on_disk(&n) && text.trim().is_empty() {
            o.dirty = false;
            o.save_error = None;
            return true;
        }
        let dir = if n.folder.is_empty() { self.store.root.clone() } else { self.store.root.join(&n.folder) };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return self.save_failed(id, &dir, e);
        }
        let title = notes::title_of(&text);
        let stem = notes::file_stem(&title);
        let path = if !Self::on_disk(&n) {
            notes::free_path(&dir, &stem, None)
        } else if n.meta.auto_named
            && n.path.file_stem().is_some_and(|s| s.to_string_lossy() != stem)
            && n.path.parent() == Some(dir.as_path())
        {
            let to = notes::free_path(&dir, &stem, Some(&n.path));
            if std::fs::rename(&n.path, &to).is_ok() {
                self.store.moved(&n.path, &to);
                // The old bytes now live at `to`, even if writing new bytes fails.
                // Keep that identity so rescan/retry cannot forget this draft.
                if let Some(m) = self.note_mut(id) {
                    m.path = to.clone();
                }
                to
            } else {
                n.path.clone()
            }
        } else {
            n.path.clone()
        };
        if let Err(e) = aizome_sumi::files::save_atomic(&path, text.as_bytes()) {
            return self.save_failed(id, &path, e);
        }
        if !Self::on_disk(&n) || n.meta != self.store.meta(&path) {
            self.store.set_meta(&path, n.meta);
        }
        if let Some(m) = self.note_mut(id) {
            m.path = path;
            m.title = title;
            m.preview = notes::preview_of(&text);
            m.text = text.into();
            m.modified = SystemTime::now();
        }
        if let Some(o) = self.open.get_mut(&id) {
            o.dirty = false;
            o.save_error = None;
        }
        self.folders = self.store.folders();
        true
    }

    fn save_failed(&mut self, id: u64, path: &Path, error: std::io::Error) -> bool {
        log::warn!("{}: {error}", path.display());
        if let Some(o) = self.open.get_mut(&id) {
            o.save_error = Some(format!("{}: {error}", path.display()));
        }
        false
    }

    fn save_all(&mut self) -> bool {
        let ids: Vec<u64> = self.open.iter().filter(|(_, o)| o.dirty).map(|(id, _)| *id).collect();
        let mut saved = true;
        for id in ids {
            // Try every draft, even when another note failed.
            saved &= self.write(id);
        }
        self.save_stickies();
        saved
    }

    fn save_alert(&self, window: WindowKey, retry: Msg, cx: &mut Cx<Msg>) {
        let errors = self
            .notes
            .iter()
            .filter_map(|n| self.open.get(&n.id)?.save_error.as_ref().map(|e| format!("{}: {e}", n.title)))
            .collect::<Vec<_>>()
            .join("\n");
        cx.alert(window, Alert::new("Some notes couldn't be saved",
            format!("{errors}\n\nKeep Fusen open to preserve your changes. Restore access to the notes folder, then retry."))
            .cancel_button("Keep Open")
            .default_button("Retry", retry));
    }

    /// The note the key window is about: a sticky's, else the selected one.
    fn target(&self, cx: &Cx<Msg>) -> Option<u64> {
        cx.key_window().and_then(|k| self.stickies.get(&k)).map(|s| s.note).or(self.selected)
    }

    fn edited(&mut self, id: u64, cx: &mut Cx<Msg>) {
        let Some(o) = self.open.get_mut(&id) else { return };
        o.dirty = true;
        o.seq += 1;
        let seq = o.seq;
        // The list follows as it's typed (its title from the first lines).
        let head: String = o.content.rope().slice(..o.content.len_chars().min(4096)).to_string();
        let (title, preview) = (notes::title_of(&head), notes::preview_of(&head));
        let changed_title = self.note(id).is_some_and(|n| n.title != title);
        if let Some(n) = self.note_mut(id) {
            n.title = title.clone();
            n.preview = preview;
            n.modified = SystemTime::now();
            n.text = head.into();
        }
        if changed_title {
            let windows: Vec<WindowKey> = self.stickies.iter().filter(|(_, s)| s.note == id).map(|(k, _)| *k).collect();
            for k in windows {
                let t = self.sticky_title(&title, Some(k));
                if let Some(s) = self.stickies.get_mut(&k) {
                    s.title = t.clone();
                }
                cx.set_title(k, t);
            }
        }
        self.sort();
        cx.after(SAVE_AFTER, Msg::Save(id, seq));
    }

    /// A title for a sticky no other sticky has.
    fn sticky_title(&self, title: &str, except: Option<WindowKey>) -> String {
        let taken = |t: &str| self.stickies.iter().any(|(k, s)| Some(*k) != except && s.title == t);
        if !taken(title) {
            return title.to_owned();
        }
        (2..).map(|n| format!("{title} ({n})")).find(|t| !taken(t)).unwrap_or_default()
    }

    fn open_sticky(&mut self, id: u64, place: Option<Place>, float: bool, cx: &mut Cx<Msg>) {
        if let Some((k, _)) = self.stickies.iter().find(|(_, s)| s.note == id) {
            cx.activate(*k);
            return;
        }
        let Some(n) = self.note(id) else { return };
        let title = self.sticky_title(&n.title, None);
        let size = place.map_or(STICKY_SIZE, |p| Size::new(p.w.max(120) as f64, p.h.max(80) as f64));
        self.ensure_open(id);
        let k = cx
            .open_window(WindowSpec::new(title.clone()).size(size.width, size.height).min_size(160.0, 100.0).utility());
        self.stickies.insert(k, Sticky { note: id, title: title.clone(), float });
        if self.places.compositor {
            cx.spawn(move || {
                wm::settle_sticky(&title, place, float);
                Msg::Nothing
            });
            if !self.tracking {
                self.tracking = true;
                cx.every(TRACK_EVERY, Msg::Track);
            }
        }
        self.save_stickies();
    }

    /// Keep where the stickies are, and the login item that brings them
    /// back.
    fn save_stickies(&mut self) {
        let mut list = Vec::new();
        for s in self.stickies.values() {
            let Some(n) = self.note(s.note) else { continue };
            if !Self::on_disk(n) {
                continue;
            }
            let key = self.store.key(&n.path);
            let place = self.saved.stickies.iter().find(|x| x.note == key).and_then(|x| x.place);
            list.push(SavedSticky { note: key, place, float: s.float });
        }
        self.saved.stickies = list;
        if let Some(dir) = &self.places.state {
            let _ = std::fs::create_dir_all(dir);
            if let Ok(json) = serde_json::to_string_pretty(&self.saved) {
                let _ = aizome_sumi::files::save_atomic(&dir.join("stickies.json"), json.as_bytes());
            }
        }
        if let Some(dir) = &self.places.autostart {
            let entry = dir.join("org.aizome.Fusen.Stickies.desktop");
            if self.saved.stickies.is_empty() {
                let _ = std::fs::remove_file(&entry);
            } else if !entry.exists() {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(
                    &entry,
                    "[Desktop Entry]\nType=Application\nName=Fusen Stickies\nComment=Your sticky notes, where you left them\nExec=aizome-fusen --stickies\nIcon=org.aizome.Fusen\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
                );
            }
        }
    }

    fn restore_stickies(&mut self, cx: &mut Cx<Msg>) {
        let saved = self.saved.stickies.clone();
        for s in saved {
            let path = self.store.root.join(&s.note);
            let Some(id) = self.notes.iter().find(|n| n.path == path).map(|n| n.id) else { continue };
            self.open_sticky(id, s.place, s.float, cx);
        }
    }

    fn take_request(&mut self, r: Request, cx: &mut Cx<Msg>) {
        let mut shown = false;
        for text in r.new {
            self.new_note(&text, cx);
            shown = true;
        }
        for f in r.files {
            let f = std::path::absolute(&f).unwrap_or(f);
            if let Some(id) = self.notes.iter().find(|n| n.path == f).map(|n| n.id) {
                self.select(Some(id));
                self.show_main(cx);
                shown = true;
                continue;
            }
            // Text handed over (the shell's Services write it to a file of
            // its own): a new note.
            if let Ok(text) = std::fs::read_to_string(&f) {
                let services = std::env::var_os("XDG_RUNTIME_DIR")
                    .is_some_and(|r| f.starts_with(PathBuf::from(r).join("aizome-services")));
                if services {
                    let _ = std::fs::remove_file(&f);
                }
                self.new_note(&text, cx);
                shown = true;
            }
        }
        if !shown && (!r.stickies_only || self.stickies.is_empty()) {
            self.show_main(cx);
        }
    }

    fn apply(&mut self, id: u64, edit: Option<format::Edit>, cx: &mut Cx<Msg>) {
        let Some(e) = edit else { return };
        let Some(o) = self.open.get_mut(&id) else { return };
        o.content.perform(EditorAction::Replace { range: e.range, text: e.text });
        o.content.set_selection(e.select);
        if let Ok(mut h) = o.highlight.lock() {
            h.reset(Language::Markdown);
        }
        self.edited(id, cx);
    }

    fn close_sticky(&mut self, k: WindowKey) -> bool {
        if let Some(s) = self.stickies.get(&k)
            && !self.write(s.note)
        {
            return false;
        }
        self.stickies.remove(&k);
        self.save_stickies();
        true
    }

    fn close(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if self.folder_sheet.as_ref().is_some_and(|(k, _)| *k == window) {
            self.folder_sheet = None;
        } else if self.stickies.contains_key(&window) {
            if !self.close_sticky(window) {
                self.save_alert(window, Msg::Close(window), cx);
                return false;
            }
        } else if self.main == Some(window) {
            if !self.save_all() {
                self.save_alert(window, Msg::Close(window), cx);
                return false;
            }
            self.main = None;
        }
        true
    }
}

impl Program for Fusen {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Fusen", "Notes").icon(Icon::StickyNote).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.notify = Some(cx.stream());
        if let Some(l) = self.instance.take() {
            aizome_sumi::instance::serve(l, cx.stream(), Msg::Launch);
        }
        self.restore_stickies(cx);
        cx.every(RESCAN_EVERY, Msg::Rescan);
        cx.send(Msg::LaunchDone);
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        self.close(window, cx)
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::New => {
                self.new_note("", cx);
            }
            Msg::NewWith(text) => {
                self.new_note(&text, cx);
            }
            Msg::NewFolder => {
                let w = self.show_main(cx);
                let k = cx.show_sheet(w);
                self.folder_sheet = Some((k, String::new()));
                cx.focus("folder-name");
            }
            Msg::FolderName(s) => {
                if let Some((_, name)) = &mut self.folder_sheet {
                    *name = s;
                }
            }
            Msg::FolderDone(ok) => {
                let Some((k, name)) = self.folder_sheet.take() else { return };
                cx.close_window(k);
                let name = notes::file_stem(name.trim());
                if ok && !name.is_empty() {
                    let base = self.current_folder();
                    let rel = if base.is_empty() { name } else { format!("{base}/{name}") };
                    if std::fs::create_dir_all(self.store.root.join(&rel)).is_ok() {
                        self.folders = self.store.folders();
                        self.folder = Some(rel);
                    }
                }
            }
            Msg::Delete => {
                let Some(id) = self.target(cx) else { return };
                let Some(n) = self.note(id).cloned() else { return };
                let shown: Vec<u64> = self.shown().iter().map(|n| n.id).collect();
                let next = shown
                    .iter()
                    .position(|x| *x == id)
                    .and_then(|i| shown.get(i + 1).or(i.checked_sub(1).and_then(|j| shown.get(j))))
                    .copied();
                if Self::on_disk(&n) {
                    let done = match &self.places.trash {
                        Some(t) => notes::trash(&n.path, t),
                        None => std::fs::remove_file(&n.path),
                    };
                    if let Err(e) = done {
                        if let Some(w) = self.main {
                            cx.alert(
                                w,
                                Alert::new(format!("“{}” couldn't be moved to the Trash.", n.title), e.to_string()),
                            );
                        }
                        return;
                    }
                }
                let windows: Vec<WindowKey> =
                    self.stickies.iter().filter(|(_, s)| s.note == id).map(|(k, _)| *k).collect();
                for k in windows {
                    self.stickies.remove(&k);
                    cx.close_window(k);
                }
                self.notes.retain(|x| x.id != id);
                self.open.remove(&id);
                self.selected = None;
                self.select(next);
                self.save_stickies();
                self.store.save_index();
            }
            Msg::Pin => {
                let Some(id) = self.target(cx) else { return };
                if let Some(n) = self.note_mut(id) {
                    n.meta.pinned = !n.meta.pinned;
                    let (path, meta) = (n.path.clone(), n.meta);
                    if !path.as_os_str().is_empty() {
                        self.store.set_meta(&path, meta);
                    }
                }
                self.sort();
            }
            Msg::Color(c) => {
                let Some(id) = self.target(cx) else { return };
                if let Some(n) = self.note_mut(id) {
                    n.meta.color = c;
                    let (path, meta) = (n.path.clone(), n.meta);
                    if !path.as_os_str().is_empty() {
                        self.store.set_meta(&path, meta);
                    }
                }
            }
            Msg::Sticky(id) => {
                let Some(id) = id.or(self.selected) else { return };
                // A note becomes a sticky once it's a file.
                self.ensure_open(id);
                let unwritten = self.note(id).is_some_and(|n| !Self::on_disk(n));
                if let Some(o) = self.open.get_mut(&id) {
                    o.dirty |= unwritten;
                }
                self.write(id);
                self.open_sticky(id, None, true, cx);
            }
            Msg::Float(on) => {
                let Some(k) = cx.key_window() else { return };
                if let Some(s) = self.stickies.get_mut(&k) {
                    s.float = on;
                    if self.places.compositor {
                        let title = s.title.clone();
                        cx.spawn(move || {
                            wm::set_float(&title, on);
                            Msg::Nothing
                        });
                    }
                }
                self.save_stickies();
            }
            Msg::Folder(f) => {
                self.folder = f;
                let shown: Vec<u64> = self.shown().iter().map(|n| n.id).collect();
                if self.selected.is_some_and(|s| !shown.contains(&s)) {
                    self.select(shown.first().copied());
                }
            }
            Msg::MoveTo(folder) => {
                let Some(id) = self.target(cx) else { return };
                let Some(n) = self.note(id).cloned() else { return };
                if n.folder == folder {
                    return;
                }
                if !Self::on_disk(&n) {
                    if let Some(m) = self.note_mut(id) {
                        m.folder = folder;
                    }
                    return;
                }
                let dir = if folder.is_empty() { self.store.root.clone() } else { self.store.root.join(&folder) };
                let stem =
                    n.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Note".into());
                let to = notes::free_path(&dir, &stem, None);
                if std::fs::create_dir_all(&dir).is_ok() && std::fs::rename(&n.path, &to).is_ok() {
                    self.store.moved(&n.path, &to);
                    if let Some(m) = self.note_mut(id) {
                        m.path = to;
                        m.folder = folder;
                    }
                }
            }
            Msg::Search(q) => {
                self.query = q;
                let shown: Vec<u64> = self.shown().iter().map(|n| n.id).collect();
                if self.selected.is_none_or(|s| !shown.contains(&s)) {
                    self.select(shown.first().copied());
                }
            }
            Msg::FocusSearch => {
                self.show_main(cx);
                cx.focus("search");
            }
            Msg::Select(id) => self.select(id),
            Msg::Edit(id, action) => {
                let Some(o) = self.open.get_mut(&id) else { return };
                // Return in a list carries it on.
                if action == EditorAction::Newline
                    && let Some(e) = format::continue_list(o.content.rope(), o.content.selection())
                {
                    self.apply(id, Some(e), cx);
                    return;
                }
                let old = o.content.rope().clone();
                let edits = o.content.perform(action);
                if edits {
                    // Notes are short: their colours start over at the first
                    // line that changed.
                    let (s, e_old, e_new) = aizome_sumi::doc::diff(&old, o.content.rope());
                    let new = o.content.rope();
                    if let Ok(mut h) = o.highlight.lock() {
                        h.edited(
                            new.char_to_line(s.min(new.len_chars())),
                            old.char_to_line(e_old.min(old.len_chars())),
                            new.char_to_line(e_new.min(new.len_chars())),
                        );
                    }
                    self.edited(id, cx);
                }
            }
            Msg::Tick(id, line) => {
                let Some(o) = self.open.get_mut(&id) else { return };
                let rope = o.content.rope();
                let text = aizome_sumi::syntax::cache::line_text(rope, line).into_owned();
                let Some((r, ticked)) = aizome_sumi::syntax::markdown::task_box(&text) else { return };
                let start = rope.line_to_char(line) + text[..r.start].chars().count();
                let sel = o.content.selection();
                let with = if ticked { "[ ]" } else { "[x]" };
                self.apply(id, Some(format::Edit { range: start..start + 3, text: with.to_owned(), select: sel }), cx);
            }
            Msg::Inline(i) => {
                let Some(id) = self.target(cx) else { return };
                let Some(o) = self.open.get(&id) else { return };
                let e = format::toggle_inline(o.content.rope(), o.content.selection(), i);
                self.apply(id, e, cx);
            }
            Msg::List(k) => {
                let Some(id) = self.target(cx) else { return };
                let Some(o) = self.open.get(&id) else { return };
                let e = format::toggle_list(o.content.rope(), o.content.selection(), k);
                self.apply(id, e, cx);
            }
            Msg::OpenUrl(url) => {
                if self.open_links {
                    cx.open_url(&url);
                }
                self.links.push(url);
            }
            Msg::ShowFolder => {
                let _ = std::fs::create_dir_all(&self.store.root);
                if self.open_links {
                    cx.open_url(&self.store.root.display().to_string());
                }
            }
            Msg::ShowMain => {
                self.show_main(cx);
            }
            Msg::CloseKey => {
                if let Some(k) = cx.key_window()
                    && self.close(k, cx)
                {
                    cx.close_window(k);
                }
            }
            Msg::Close(k) => {
                if self.close(k, cx) {
                    cx.close_window(k);
                }
            }
            Msg::RetrySave => {
                self.save_all();
            }
            Msg::Save(id, seq) => {
                if self.open.get(&id).is_some_and(|o| o.seq == seq) {
                    self.write(id);
                    self.save_stickies();
                }
            }
            Msg::Rescan => {
                if self.folder_sheet.is_none() {
                    self.rescan();
                }
            }
            Msg::Track => {
                if self.stickies.is_empty() {
                    return;
                }
                let titles: Vec<String> = self.stickies.values().map(|s| s.title.clone()).collect();
                cx.spawn(move || Msg::Placed(wm::places(&titles)));
            }
            Msg::Placed(places) => {
                let mut changed = false;
                for (title, place, float) in places {
                    let Some(s) = self.stickies.values().find(|s| s.title == title).cloned() else { continue };
                    let Some(n) = self.note(s.note) else { continue };
                    let key = self.store.key(&n.path);
                    if let Some(x) = self.saved.stickies.iter_mut().find(|x| x.note == key)
                        && (x.place != Some(place) || x.float != float)
                    {
                        x.place = Some(place);
                        x.float = float;
                        changed = true;
                    }
                }
                if changed
                    && let Some(dir) = &self.places.state
                    && let Ok(json) = serde_json::to_string_pretty(&self.saved)
                {
                    let _ = aizome_sumi::files::save_atomic(&dir.join("stickies.json"), json.as_bytes());
                }
            }
            Msg::Highlight => {
                for o in self.open.values_mut() {
                    if let Ok(mut h) = o.highlight.lock() {
                        h.incomplete = false;
                        h.scheduled = false;
                    }
                    o.styles += 1;
                }
            }
            Msg::Launch(args) => {
                let r = Request::parse(&args);
                self.take_request(Request { stickies_only: false, ..r }, cx);
            }
            Msg::LaunchDone => {
                let r = std::mem::take(&mut self.request);
                self.take_request(r, cx);
                if self.selected.is_none() {
                    let first = self.shown().first().map(|n| n.id);
                    self.select(first);
                }
            }
            Msg::Quit => {
                // Stickies stay stickies: they come back next time.
                if self.save_all() {
                    cx.quit();
                } else {
                    let w = self
                        .main
                        .or_else(|| self.stickies.keys().next().copied())
                        .unwrap_or_else(|| self.show_main(cx));
                    cx.activate(w);
                    self.save_alert(w, Msg::Quit, cx);
                }
            }
            Msg::Nothing => {}
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some((k, name)) = &self.folder_sheet
            && *k == window
        {
            return aizome_kit::column![
                text("New Folder").semibold().size(15.0),
                text_field(name)
                    .placeholder("Name")
                    .on_change(Msg::FolderName)
                    .on_submit(Msg::FolderDone(true))
                    .id("folder-name")
                    .width(Length::Fill),
                aizome_kit::row![
                    spacer(),
                    button("Cancel").cancel().on_press(Msg::FolderDone(false)),
                    button("Create").default().on_press(Msg::FolderDone(true))
                ]
                .spacing(8.0)
                .width(Length::Fill),
            ]
            .spacing(12.0)
            .padding(16.0)
            .width(320.0)
            .into();
        }
        if let Some(s) = self.stickies.get(&window) {
            return crate::view::sticky_window(self, s.note);
        }
        crate::view::main_window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let info = self.info();
        let name = info.name.clone();
        let role = |l: &str, r: MenuRole| MenuItem::<Msg>::role_item(l, r);
        let note = self.selected_note();
        let has = note.is_some();
        let app = Menu::new(name.clone())
            .item(role(&format!("About {name}"), MenuRole::About))
            .separator()
            .submenu(Menu::new("Services").item(role("No Services Apply", MenuRole::Services).enabled(false)))
            .separator()
            .item(role(&format!("Hide {name}"), MenuRole::Hide).shortcut(Shortcut::cmd('H')))
            .item(role("Hide Others", MenuRole::HideOthers).shortcut(Shortcut::cmd('H').alt()))
            .separator()
            .item(MenuItem::new(format!("Quit {name}"), Msg::Quit).shortcut(Shortcut::cmd('Q')));
        let mut move_to = Menu::new("Move to").item(
            MenuItem::new("Notes", Msg::MoveTo(String::new())).checked(note.is_some_and(|n| n.folder.is_empty())),
        );
        for f in &self.folders {
            move_to
                .push(MenuItem::new(f.clone(), Msg::MoveTo(f.clone())).checked(note.is_some_and(|n| n.folder == *f)));
        }
        let file = Menu::new("File")
            .item(MenuItem::new("New Note", Msg::New).shortcut(Shortcut::cmd('N')))
            .item(MenuItem::new("New Folder…", Msg::NewFolder).shortcut(Shortcut::cmd('N').shift()))
            .separator()
            .item(MenuItem::new("Show Notes", Msg::ShowMain).shortcut(Shortcut::cmd('0')))
            .item(MenuItem::new("Close", Msg::CloseKey).shortcut(Shortcut::cmd('W')))
            .separator()
            .submenu(move_to)
            .item(
                MenuItem::new("Delete Note", Msg::Delete)
                    .shortcut(Shortcut::cmd_key(aizome_kit::ShortcutKey::Backspace))
                    .enabled(has),
            )
            .separator()
            .item(MenuItem::new("Show Notes Folder", Msg::ShowFolder));
        let edit = Menu::new(EDIT)
            .item(role("Undo", MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
            .item(role("Redo", MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
            .separator()
            .item(role("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
            .item(role("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
            .item(role("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
            .item(role("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
            .separator()
            .item(MenuItem::new("Search Notes", Msg::FocusSearch).shortcut(Shortcut::cmd('F')));
        let format = Menu::new("Format")
            .item(MenuItem::new("Bold", Msg::Inline(Inline::Bold)).shortcut(Shortcut::cmd('B')))
            .item(MenuItem::new("Italic", Msg::Inline(Inline::Italic)).shortcut(Shortcut::cmd('I')))
            .item(MenuItem::new("Strikethrough", Msg::Inline(Inline::Strike)).shortcut(Shortcut::cmd('X').shift()))
            .item(MenuItem::new("Code", Msg::Inline(Inline::Code)).shortcut(Shortcut::cmd('E')))
            .separator()
            .item(MenuItem::new("Checklist", Msg::List(List::Task)).shortcut(Shortcut::cmd('L').shift()))
            .item(MenuItem::new("Bulleted List", Msg::List(List::Bullet)))
            .item(MenuItem::new("Numbered List", Msg::List(List::Numbered)));
        let mut colors = Menu::new("Colour");
        for c in NoteColor::ALL {
            colors.push(
                MenuItem::new(c.name(), Msg::Color(c)).checked(note.is_some_and(|n| n.meta.color == c)).enabled(has),
            );
        }
        let floating = self.stickies.values().any(|s| s.float);
        let note_menu = Menu::new("Note")
            .item(
                MenuItem::new(if note.is_some_and(|n| n.meta.pinned) { "Unpin Note" } else { "Pin Note" }, Msg::Pin)
                    .shortcut(Shortcut::cmd('P').alt())
                    .enabled(has),
            )
            .submenu(colors)
            .separator()
            .item(MenuItem::new("Open as Sticky", Msg::Sticky(None)).shortcut(Shortcut::cmd('T')).enabled(has))
            .item(
                MenuItem::new("Keep Sticky on Top", Msg::Float(!floating))
                    .checked(floating)
                    .enabled(!self.stickies.is_empty()),
            );
        MenuBar::standard(&info)
            .replace(&name, app)
            .with_file_menu(file)
            .replace(EDIT, edit)
            .with_menu(format)
            .with_menu(note_menu)
            .extend(HELP, std::iter::empty())
    }
}

/// A note's file under the notes folder, for tests and the CLI.
pub fn under(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}
