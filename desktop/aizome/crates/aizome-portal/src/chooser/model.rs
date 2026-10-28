//! A chooser without its window: where it is, what shows there, what's
//! selected and typed, and what accepting means in each mode.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hako::fsops::names;
use hako::model::entry::{self, Entry, Sort};
use hako::model::mime::MimeDb;
use hako::model::uri;

use crate::memory::Memory;
use crate::request::{Answer, Filter, Mode, Request};

/// The most recent files listed.
const RECENT: usize = 100;

/// Where a chooser looks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loc {
    Folder(PathBuf),
    /// Files used lately, from every folder.
    Recent,
}

impl Loc {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Loc::Folder(p) => Some(p),
            Loc::Recent => None,
        }
    }
}

/// What accepting (or activating a row) comes to.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Nothing yet: `problem` may say why.
    Stay,
    /// Go somewhere first (a folder chosen while choosing files).
    Go(Loc),
    /// Ask before replacing this file.
    Replace(PathBuf),
    Done(Answer),
}

/// What a chooser shows and holds.
pub struct Browser {
    pub request: Request,
    /// The person's home: where `~` goes, and the path bar starts.
    pub home: PathBuf,
    pub loc: Loc,
    pub back: Vec<Loc>,
    pub forward: Vec<Loc>,
    /// What the location holds, as read.
    pub entries: Arc<Vec<Entry>>,
    /// What shows: hidden files and the filter applied, in order.
    pub shown: Arc<Vec<Entry>>,
    /// The location `shown` was read from.
    pub shown_loc: Option<Loc>,
    /// Rows of `shown`.
    pub selected: Vec<usize>,
    /// The name to save as.
    pub name: String,
    /// Which of the request's filters applies.
    pub filter: Option<usize>,
    /// Each choice's value, in the request's order.
    pub choices: Vec<String>,
    pub hidden: bool,
    pub sort: Sort,
    /// Reading the location.
    pub loading: bool,
    /// Why the location couldn't be read.
    pub error: Option<String>,
    /// Why the last accept didn't go through.
    pub problem: Option<String>,
    /// The name to select once the location is read.
    pub select_after: Option<OsString>,
    /// Bumped with each read, so a late one is ignored.
    pub generation: u64,
}

impl Browser {
    /// A chooser for `request`, where it asks to start: the file being
    /// saved again's folder, the app's suggestion, where the app last
    /// chose, the current Desk's folder, home.
    pub fn new(request: Request, memory: &Memory, desk: Option<&Path>, home: &Path) -> Browser {
        let file = request.file();
        let start = file
            .as_ref()
            .and_then(|f| f.parent())
            .map(Path::to_path_buf)
            .filter(|d| d.is_dir())
            .or_else(|| request.folder().filter(|d| d.is_dir()))
            .or_else(|| memory.folder(&request.app_id))
            .or_else(|| desk.map(Path::to_path_buf).filter(|d| d.is_dir()))
            .unwrap_or_else(|| home.to_path_buf());
        let name = match request.mode {
            Mode::Save => request
                .current_name
                .clone()
                .or_else(|| file.as_ref().and_then(|f| f.file_name()).map(names::display))
                .unwrap_or_default(),
            _ => String::new(),
        };
        let filter = match (&request.current_filter, request.filters.is_empty()) {
            (_, true) => None,
            (Some(f), false) => request.filters.iter().position(|x| x == f).or(Some(0)),
            (None, false) => Some(0),
        };
        let choices = request.choices.iter().map(|c| c.initial()).collect();
        let select_after = file.as_ref().and_then(|f| f.file_name()).map(|n| n.to_os_string());
        Browser {
            request,
            home: home.to_path_buf(),
            loc: Loc::Folder(start),
            back: Vec::new(),
            forward: Vec::new(),
            entries: Arc::default(),
            shown: Arc::default(),
            shown_loc: None,
            selected: Vec::new(),
            name,
            filter,
            choices,
            hidden: memory.hidden,
            sort: memory.sort,
            loading: true,
            error: None,
            problem: None,
            select_after,
            generation: 0,
        }
    }

    pub fn mode(&self) -> Mode {
        self.request.mode
    }

    /// Recent files are offered when choosing files to open.
    pub fn offers_recent(&self) -> bool {
        self.request.mode == Mode::Open && !self.request.directory
    }

    /// The filter in force: the one picked from the list, or the app's
    /// own when it gave no list.
    pub fn active_filter(&self) -> Option<&Filter> {
        match self.filter {
            Some(i) => self.request.filters.get(i),
            None if self.request.filters.is_empty() => self.request.current_filter.as_ref(),
            None => None,
        }
    }

    /// Go to `loc`, remembering where it was for Back: the generation
    /// to read it under.
    pub fn go(&mut self, loc: Loc) -> u64 {
        if loc != self.loc {
            let was = std::mem::replace(&mut self.loc, loc);
            self.back.push(was);
            self.forward.clear();
        }
        self.start_reading()
    }

    /// Back, if there's somewhere: the generation to read it under.
    pub fn go_back(&mut self) -> Option<u64> {
        let loc = self.back.pop()?;
        let was = std::mem::replace(&mut self.loc, loc);
        self.forward.push(was);
        Some(self.start_reading())
    }

    pub fn go_forward(&mut self) -> Option<u64> {
        let loc = self.forward.pop()?;
        let was = std::mem::replace(&mut self.loc, loc);
        self.back.push(was);
        Some(self.start_reading())
    }

    /// The enclosing folder, if there is one.
    pub fn up(&self) -> Option<Loc> {
        Some(Loc::Folder(self.loc.folder()?.parent()?.to_path_buf()))
    }

    /// Read the location again (after a change in it).
    pub fn start_reading(&mut self) -> u64 {
        self.generation += 1;
        self.loading = true;
        self.problem = None;
        self.generation
    }

    /// The location's entries, read under `generation`: ignored when
    /// another read started since.
    pub fn listed(&mut self, generation: u64, result: Result<Vec<Entry>, String>) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        // Read again where it was: the selection stays.
        let keep: Vec<PathBuf> = if self.shown_loc.as_ref() == Some(&self.loc) {
            self.selected.iter().filter_map(|&i| self.shown.get(i)).map(|e| e.path.clone()).collect()
        } else {
            Vec::new()
        };
        self.shown_loc = Some(self.loc.clone());
        self.selected.clear();
        match result {
            Ok(entries) => {
                self.entries = Arc::new(entries);
                self.error = None;
            }
            Err(e) => {
                self.entries = Arc::default();
                self.error = Some(e);
            }
        }
        self.refresh();
        self.selected = self.shown.iter().enumerate().filter(|(_, e)| keep.contains(&e.path)).map(|(i, _)| i).collect();
        if let Some(name) = self.select_after.take()
            && let Some(i) = self.shown.iter().position(|e| e.name == name)
        {
            self.selected = vec![i];
        }
    }

    /// What shows of the entries, after a change of filter, order or
    /// hidden files. The selection is kept by path.
    pub fn refresh(&mut self) {
        let keep: Vec<PathBuf> =
            self.selected.iter().filter_map(|&i| self.shown.get(i)).map(|e| e.path.clone()).collect();
        let db = MimeDb::get();
        let filter = self.active_filter().cloned();
        let folders_only = self.request.directory;
        let mut shown: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| self.hidden || !e.hidden || self.loc == Loc::Recent)
            .filter(|e| {
                if e.is_folder() {
                    return true;
                }
                !folders_only && filter.as_ref().is_none_or(|f| f.admits(db, &e.display, &e.mime))
            })
            .cloned()
            .collect();
        // Recent files keep the order they were used in.
        if self.loc != Loc::Recent {
            entry::sort_entries(&mut shown, self.sort);
        }
        self.shown = Arc::new(shown);
        self.selected = self.shown.iter().enumerate().filter(|(_, e)| keep.contains(&e.path)).map(|(i, _)| i).collect();
    }

    /// Rows selected (by the list).
    pub fn select(&mut self, rows: Vec<usize>) {
        let n = self.shown.len();
        self.selected = rows.into_iter().filter(|&i| i < n).collect();
        if !self.request.multiple {
            self.selected.truncate(1);
        }
        self.problem = None;
        // Saving: a file picked names what's saved.
        if self.mode() == Mode::Save
            && let [i] = self.selected[..]
            && !self.shown[i].is_folder()
        {
            self.name = self.shown[i].display.clone();
        }
    }

    /// Pick the request's filter `i`. Saving, a name ending in the old
    /// filter's extension takes the new one's.
    pub fn set_filter(&mut self, i: usize) {
        if i >= self.request.filters.len() {
            return;
        }
        let old = self.active_filter().and_then(Filter::extension);
        self.filter = Some(i);
        let new = self.active_filter().and_then(Filter::extension);
        if self.mode() == Mode::Save
            && let (Some(old), Some(new)) = (old, new)
            && let Some(stem) = self.name.strip_suffix(&format!(".{old}"))
            && !stem.is_empty()
        {
            self.name = format!("{stem}.{new}");
        }
        self.refresh();
    }

    pub fn set_choice(&mut self, i: usize, value: String) {
        if let Some(c) = self.choices.get_mut(i) {
            *c = value;
        }
    }

    /// Whether accepting can do anything now (the default button's
    /// state).
    pub fn can_accept(&self) -> bool {
        match self.mode() {
            Mode::Open if self.request.directory => !self.selected.is_empty() || self.loc.folder().is_some(),
            Mode::Open => !self.selected.is_empty(),
            Mode::Save => !self.name.trim().is_empty() && self.loc.folder().is_some(),
            Mode::SaveFiles => self.loc.folder().is_some() || self.selected_folder().is_some(),
        }
    }

    /// The one folder selected, if that's what is.
    fn selected_folder(&self) -> Option<&Entry> {
        match self.selected[..] {
            [i] if self.shown[i].is_folder() => Some(&self.shown[i]),
            _ => None,
        }
    }

    /// A row activated (double-clicked, or Return in the list).
    pub fn activate(&mut self, i: usize) -> Step {
        let Some(e) = self.shown.get(i).cloned() else { return Step::Stay };
        if e.is_folder() {
            return Step::Go(Loc::Folder(e.path));
        }
        self.selected = vec![i];
        if self.mode() == Mode::Save {
            self.name = e.display.clone();
        }
        self.accept()
    }

    /// The accept button.
    pub fn accept(&mut self) -> Step {
        self.problem = None;
        match self.mode() {
            Mode::Open => self.accept_open(),
            Mode::Save => self.accept_save(),
            Mode::SaveFiles => self.accept_save_files(),
        }
    }

    fn accept_open(&mut self) -> Step {
        let picked: Vec<&Entry> = self.selected.iter().map(|&i| &self.shown[i]).collect();
        if self.request.directory {
            let chosen: Vec<PathBuf> = if picked.is_empty() {
                self.loc.folder().map(Path::to_path_buf).into_iter().collect()
            } else {
                picked.iter().filter(|e| e.is_folder()).map(|e| e.path.clone()).collect()
            };
            return if chosen.is_empty() { Step::Stay } else { Step::Done(self.answer(&chosen)) };
        }
        match picked[..] {
            [] => Step::Stay,
            [one] if one.is_folder() => Step::Go(Loc::Folder(one.path.clone())),
            _ if picked.iter().any(|e| e.is_folder()) => {
                self.problem = Some("Select files, or open one folder.".into());
                Step::Stay
            }
            _ => {
                let chosen: Vec<PathBuf> = picked.iter().map(|e| e.path.clone()).collect();
                Step::Done(self.answer(&chosen))
            }
        }
    }

    fn accept_save(&mut self) -> Step {
        let Some(dir) = self.loc.folder().map(Path::to_path_buf) else { return Step::Stay };
        let typed = self.name.trim();
        if typed.is_empty() {
            return Step::Stay;
        }
        // A path typed: from home with `~`, else from here.
        let target = if typed.starts_with('/') || typed == "~" || typed.starts_with("~/") {
            names::normalize(&names::expand_tilde(typed, &self.home))
        } else {
            names::normalize(&dir.join(typed))
        };
        if target.is_dir() {
            self.name.clear();
            return Step::Go(Loc::Folder(target));
        }
        let file_name = target.file_name().map(names::display).unwrap_or_default();
        if let Some(why) = names::check_name(&file_name) {
            self.problem = Some(why.into());
            return Step::Stay;
        }
        let parent = target.parent().unwrap_or(Path::new("/"));
        if !parent.is_dir() {
            self.problem = Some(format!("There's no folder “{}”.", parent.display()));
            return Step::Stay;
        }
        if std::fs::symlink_metadata(&target).is_ok() {
            return Step::Replace(target);
        }
        Step::Done(self.answer(&[target]))
    }

    fn accept_save_files(&mut self) -> Step {
        let dir = match self.selected_folder() {
            Some(e) => e.path.clone(),
            None => match self.loc.folder() {
                Some(d) => d.to_path_buf(),
                None => return Step::Stay,
            },
        };
        // Names already there get a number, as Files keeps both; names
        // given twice too.
        let mut taken: Vec<OsString> = Vec::new();
        let mut chosen = Vec::new();
        for file in &self.request.files {
            let name = crate::request::path_of_bytes(file);
            let Some(name) = name.file_name() else { continue };
            let free = names::free_name_with(name, false, |n| {
                !taken.iter().any(|t| t == n) && std::fs::symlink_metadata(dir.join(n)).is_err()
            });
            chosen.push(dir.join(&free));
            taken.push(free);
        }
        Step::Done(self.answer(&chosen))
    }

    /// The answer for `paths`, with the choices and filter as left.
    pub fn answer(&self, paths: &[PathBuf]) -> Answer {
        Answer {
            uris: paths.iter().map(|p| uri::file_uri(p)).collect(),
            choices: self.request.choices.iter().zip(&self.choices).map(|(c, v)| (c.id.clone(), v.clone())).collect(),
            current_filter: self.active_filter().cloned(),
            writable: self.mode() == Mode::Open,
        }
    }

    /// The folder the answer was chosen in, to start there next time.
    pub fn chosen_in(&self, answer: &Answer) -> Option<PathBuf> {
        if let Some(d) = self.loc.folder() {
            return Some(d.to_path_buf());
        }
        uri::path_of(answer.uris.first()?)?.parent().map(Path::to_path_buf)
    }
}

/// Read `loc`: a folder's entries, or the recent files that are still
/// there.
pub fn read(loc: &Loc) -> Result<Vec<Entry>, String> {
    match loc {
        Loc::Folder(dir) => entry::read_folder(dir).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => format!("You can't see what “{}” holds.", dir.display()),
            std::io::ErrorKind::NotFound => format!("“{}” isn't there any more.", dir.display()),
            _ => format!("“{}” couldn't be read: {e}", dir.display()),
        }),
        Loc::Recent => Ok(aizome_search::recent::load(aizome_search::recent::default_path())
            .into_iter()
            .filter_map(|r| Entry::read(&r.path).ok())
            .filter(|e| !e.is_folder())
            .take(RECENT)
            .collect()),
    }
}
