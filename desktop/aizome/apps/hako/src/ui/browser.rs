//! A window's browser: its columns (NeXT's browser: each folder selected
//! opens the next column), which one has the keyboard, its history, and
//! what it's doing (renaming, editing the location, searching, dragging).
//! In list and icon views the same columns are kept and the focused one
//! is shown.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use super::dnd::{Spots, Target};
use super::fileview::Layout;
use crate::model::entry::Entry;

/// What a column lists.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Loc {
    Folder(PathBuf),
    /// Every trash can's items.
    Trash,
    /// Recently used files.
    Recent,
}

impl Loc {
    pub fn folder(&self) -> Option<&Path> {
        match self {
            Loc::Folder(p) => Some(p),
            _ => None,
        }
    }
}

/// Where a column's listing stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    /// To be read.
    Wanted,
    /// Being read, under this generation.
    Loading(u64),
    Ready,
    Failed(String),
}

/// A column: a place and what's in it.
#[derive(Clone, Debug)]
pub struct Column {
    pub loc: Loc,
    /// Everything in it, sorted.
    pub all: Arc<Vec<Entry>>,
    /// What shows (hidden items left out unless shown).
    pub rows: Rc<Vec<Entry>>,
    /// Rows selected, sorted.
    pub selected: Rc<Vec<usize>>,
    pub load: Load,
    /// Names to select once it's read (a file revealed, items pasted).
    pub pending: Option<Vec<OsString>>,
    /// Scroll to this row, once per generation.
    pub scroll: Option<(usize, u64)>,
}

impl Column {
    pub fn new(loc: Loc) -> Column {
        Column {
            loc,
            all: Arc::new(Vec::new()),
            rows: Rc::new(Vec::new()),
            selected: Rc::new(Vec::new()),
            load: Load::Wanted,
            pending: None,
            scroll: None,
        }
    }

    /// The entries selected.
    pub fn selection(&self) -> Vec<&Entry> {
        self.selected.iter().filter_map(|&i| self.rows.get(i)).collect()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selection().into_iter().map(|e| e.path.clone()).collect()
    }

    /// The one entry selected, if just one is.
    pub fn single(&self) -> Option<&Entry> {
        match self.selected.as_slice() {
            [i] => self.rows.get(*i),
            _ => None,
        }
    }

    /// Show `all` (with hidden items if `hidden`), keeping the selection
    /// by name.
    pub fn set_entries(&mut self, all: Arc<Vec<Entry>>, hidden: bool) {
        let keep: Vec<OsString> = self.selection().into_iter().map(|e| e.name.clone()).collect();
        self.all = all;
        self.refilter(hidden);
        let names = self.pending.take().unwrap_or(keep);
        self.select_names(&names);
    }

    /// The rows again from `all`, after the hidden-files setting changed.
    pub fn refilter(&mut self, hidden: bool) {
        let keep: Vec<OsString> = self.selection().into_iter().map(|e| e.name.clone()).collect();
        let rows: Vec<Entry> = self.all.iter().filter(|e| hidden || !e.hidden || self.is_virtual()).cloned().collect();
        self.rows = Rc::new(rows);
        self.select_names(&keep);
    }

    fn is_virtual(&self) -> bool {
        !matches!(self.loc, Loc::Folder(_))
    }

    /// Select the rows of these names (in the column's own names).
    pub fn select_names(&mut self, names: &[OsString]) {
        let sel: Vec<usize> =
            self.rows.iter().enumerate().filter(|(_, e)| names.contains(&e.name)).map(|(i, _)| i).collect();
        self.selected = Rc::new(sel);
    }
}

/// An item being renamed.
#[derive(Clone, Debug, PartialEq)]
pub struct Rename {
    pub column: usize,
    pub row: usize,
    pub path: PathBuf,
    pub text: String,
    pub error: Option<String>,
}

/// A search under way.
#[derive(Clone, Debug)]
pub struct Search {
    pub query: String,
    /// Where it looks.
    pub root: PathBuf,
    pub results: Rc<Vec<Entry>>,
    pub selected: Rc<Vec<usize>>,
    pub generation: u64,
    pub running: bool,
}

/// A drag from this window under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub sources: Vec<PathBuf>,
    /// What it's over, if a drop there would do something.
    pub target: Option<Target>,
    /// The column it started in.
    pub from: usize,
    /// Counts targets, so a folder opens only if the drag waits over it.
    pub spring: u64,
}

/// A window's browser.
pub struct Browser {
    pub columns: Vec<Column>,
    /// The column with the keyboard.
    pub focus: usize,
    pub layout: Layout,
    pub back: Vec<(Loc, Option<OsString>)>,
    pub forward: Vec<(Loc, Option<OsString>)>,
    pub rename: Option<Rename>,
    pub path_edit: Option<String>,
    pub path_error: Option<String>,
    pub search: Option<Search>,
    pub drag: Option<Drag>,
    /// Which column the strip keeps in sight, and when it was asked.
    pub reveal: (usize, u64),
    pub spots: Spots,
    /// Give the focused column the keyboard once it shows (after going
    /// somewhere).
    pub focus_pending: bool,
}

impl Browser {
    pub fn new(layout: Layout) -> Browser {
        Browser {
            columns: Vec::new(),
            focus: 0,
            layout,
            back: Vec::new(),
            forward: Vec::new(),
            rename: None,
            path_edit: None,
            path_error: None,
            search: None,
            drag: None,
            reveal: (0, 0),
            spots: Spots::default(),
            focus_pending: false,
        }
    }

    /// The column with the keyboard.
    pub fn focused(&self) -> Option<&Column> {
        self.columns.get(self.focus)
    }

    pub fn focused_mut(&mut self) -> Option<&mut Column> {
        self.columns.get_mut(self.focus)
    }

    /// Where the window is: the place of the column with the keyboard
    /// (what follows it shows what's selected there).
    pub fn location(&self) -> Option<&Loc> {
        self.focused().map(|c| &c.loc)
    }

    /// The folder new things go in: the focused column's.
    pub fn target_folder(&self) -> Option<&Path> {
        self.focused().and_then(|c| c.loc.folder())
    }

    /// The items the person means: the focused column's selection.
    pub fn selection(&self) -> Vec<PathBuf> {
        if let Some(s) = &self.search {
            return s.selected.iter().filter_map(|&i| s.results.get(i)).map(|e| e.path.clone()).collect();
        }
        self.focused().map(Column::selected_paths).unwrap_or_default()
    }

    /// The entries the person means.
    pub fn selected_entries(&self) -> Vec<Entry> {
        if let Some(s) = &self.search {
            return s.selected.iter().filter_map(|&i| s.results.get(i)).cloned().collect();
        }
        self.focused().map(|c| c.selection().into_iter().cloned().collect()).unwrap_or_default()
    }

    /// Show `loc`, opening columns from `root` down to it (each selecting
    /// the next), and select `select` in it once it's read.
    pub fn go(&mut self, root: Loc, loc: &Loc, select: Option<OsString>, generation: u64) {
        self.columns.clear();
        self.rename = None;
        self.search = None;
        match (&root, loc) {
            (Loc::Folder(r), Loc::Folder(p)) if p.starts_with(r) => {
                let mut dir = r.clone();
                self.columns.push(Column::new(Loc::Folder(dir.clone())));
                if let Ok(rest) = p.strip_prefix(r) {
                    for part in rest.components() {
                        let name = part.as_os_str().to_os_string();
                        if let Some(c) = self.columns.last_mut() {
                            c.pending = Some(vec![name.clone()]);
                        }
                        dir.push(&name);
                        self.columns.push(Column::new(Loc::Folder(dir.clone())));
                    }
                }
            }
            _ => self.columns.push(Column::new(loc.clone())),
        }
        if let Some(name) = select
            && let Some(c) = self.columns.last_mut()
        {
            c.pending = Some(vec![name]);
        }
        self.focus = self.columns.len().saturating_sub(1);
        self.reveal = (self.focus, generation);
        self.focus_pending = true;
    }

    /// Remember where the window is, before going elsewhere.
    pub fn push_history(&mut self) {
        if let Some(loc) = self.location().cloned() {
            let sel = self.columns.last().and_then(|c| c.single().map(|e| e.name.clone()));
            if self.back.last().map(|(l, _)| l) != Some(&loc) {
                self.back.push((loc, sel));
            }
            if self.back.len() > 64 {
                self.back.remove(0);
            }
            self.forward.clear();
        }
    }

    /// Column `col`'s selection changed to `rows`: the columns after it
    /// close, and a folder selected alone opens the next. Whether a new
    /// column opened.
    pub fn select(&mut self, col: usize, rows: Vec<usize>, generation: u64) -> bool {
        let Some(c) = self.columns.get_mut(col) else { return false };
        let mut rows = rows;
        rows.retain(|&r| r < c.rows.len());
        rows.sort_unstable();
        rows.dedup();
        c.selected = Rc::new(rows);
        self.focus = col;
        // The strip keeps this column, and what it opens, in sight.
        self.reveal = (col, generation);
        if self.rename.as_ref().is_some_and(|r| r.column != col) {
            self.rename = None;
        }
        self.open_after(col, generation)
    }

    /// Make the columns after `col` match its selection.
    pub fn open_after(&mut self, col: usize, generation: u64) -> bool {
        let next = self
            .columns
            .get(col)
            .and_then(|c| c.single())
            .filter(|e| e.is_folder())
            .map(|e| Loc::Folder(e.path.clone()));
        if let Some(loc) = next {
            if self.columns.get(col + 1).is_some_and(|c| c.loc == loc) {
                self.columns.truncate(col + 2);
                return false;
            }
            self.columns.truncate(col + 1);
            self.columns.push(Column::new(loc));
            self.reveal = (col, generation);
            true
        } else {
            self.columns.truncate(col + 1);
            false
        }
    }

    /// Reconcile a refreshed listing with the columns it opened. A
    /// still-selected folder keeps the whole path below it; a folder
    /// moved, restored or removed closes that obsolete path.
    pub fn refresh_after(&mut self, col: usize, generation: u64) -> bool {
        if let Some(next) = self.columns.get(col + 1) {
            let c = &self.columns[col];
            let matches_next = |e: &Entry| e.is_folder() && next.loc.folder() == Some(e.path.as_path());
            // An explicit path can go through a hidden folder, which has
            // no selected visible row. Its entry still proves the link.
            if c.single().is_some_and(matches_next) || (c.selected.is_empty() && c.all.iter().any(matches_next)) {
                return false;
            }
            if self.focus > col {
                self.focus = col;
                self.focus_pending = true;
            }
            self.reveal = (col, generation);
            if self.rename.as_ref().is_some_and(|r| r.column > col) {
                self.rename = None;
            }
            // A drag's source or destination may refer to a closed column.
            self.drag = None;
        }
        self.open_after(col, generation)
    }

    /// → in column `col`: into the folder selected there, selecting its
    /// first item. Whether the focus moved.
    pub fn into_next(&mut self, col: usize) -> bool {
        if self.columns.get(col).and_then(|c| c.single()).is_none_or(|e| !e.is_folder()) {
            return false;
        }
        let Some(next) = self.columns.get_mut(col + 1) else { return false };
        if next.selected.is_empty() && !next.rows.is_empty() {
            next.selected = Rc::new(vec![0]);
            next.scroll = Some((0, next.scroll.map_or(1, |s| s.1 + 1)));
        }
        self.focus = col + 1;
        true
    }

    /// ← in column `col`: back to the column before (this one's selection
    /// let go). Whether the focus moved.
    pub fn out_of(&mut self, col: usize) -> bool {
        if col == 0 {
            return false;
        }
        if let Some(c) = self.columns.get_mut(col) {
            c.selected = Rc::new(Vec::new());
        }
        self.columns.truncate(col + 1);
        self.focus = col - 1;
        true
    }

    /// Every folder a column shows, to watch.
    pub fn folders(&self) -> Vec<PathBuf> {
        self.columns.iter().filter_map(|c| c.loc.folder().map(Path::to_path_buf)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::TempDir;
    use crate::model::entry::{read_folder, sort_entries};

    fn load(b: &mut Browser, hidden: bool) {
        for c in &mut b.columns {
            if c.load == Load::Wanted
                && let Loc::Folder(d) = &c.loc
            {
                let mut es = read_folder(d).unwrap();
                sort_entries(&mut es, Default::default());
                c.set_entries(Arc::new(es), hidden);
                c.load = Load::Ready;
            }
        }
    }

    #[test]
    fn opens_columns_down_to_a_folder() {
        let t = TempDir::new("browser-go");
        t.file("a/b/c/file.txt", "x");
        t.file("a/other.txt", "o");
        let mut b = Browser::new(Layout::Column);
        b.go(Loc::Folder(t.path().to_path_buf()), &Loc::Folder(t.join("a/b")), Some("c".into()), 1);
        assert_eq!(b.columns.len(), 3);
        load(&mut b, false);
        // Each column selects the next, and the last its file to show.
        assert_eq!(b.columns[0].single().unwrap().display, "a");
        assert_eq!(b.columns[1].single().unwrap().display, "b");
        assert_eq!(b.columns[2].single().unwrap().display, "c");
        assert_eq!(b.focus, 2);
        assert_eq!(b.location(), Some(&Loc::Folder(t.join("a/b"))));
        // Selecting the folder in the last column opens it.
        assert!(b.open_after(2, 2));
        assert_eq!(b.columns.len(), 4);
        load(&mut b, false);
        assert_eq!(b.columns[3].rows[0].display, "file.txt");
        // Selecting a file in column 1 closes what was after it.
        let other = b.columns[1].rows.iter().position(|e| e.display == "other.txt").unwrap();
        assert!(!b.select(1, vec![other], 3));
        assert_eq!(b.columns.len(), 2);
        assert_eq!(b.focus, 1);
    }

    #[test]
    fn arrows_move_between_columns() {
        let t = TempDir::new("browser-keys");
        t.file("a/x.txt", "x");
        t.file("b.txt", "b");
        let mut b = Browser::new(Layout::Column);
        b.go(Loc::Folder(t.path().to_path_buf()), &Loc::Folder(t.path().to_path_buf()), None, 1);
        load(&mut b, false);
        assert!(b.select(0, vec![0], 2), "the folder a opens");
        load(&mut b, false);
        assert!(b.into_next(0));
        assert_eq!(b.focus, 1);
        assert_eq!(b.columns[1].single().unwrap().display, "x.txt");
        assert!(b.out_of(1));
        assert_eq!(b.focus, 0);
        assert!(b.columns[1].selected.is_empty());
        assert!(!b.out_of(0));
        // → on a file does nothing.
        b.select(0, vec![1], 3);
        assert!(!b.into_next(0));
    }

    #[test]
    fn hides_and_shows_hidden_items_keeping_the_selection() {
        let t = TempDir::new("browser-hidden");
        t.file(".dot", "d");
        t.file("plain", "p");
        let mut b = Browser::new(Layout::List);
        b.go(Loc::Folder(t.path().to_path_buf()), &Loc::Folder(t.path().to_path_buf()), Some("plain".into()), 1);
        load(&mut b, false);
        assert_eq!(b.columns[0].rows.len(), 1);
        assert_eq!(b.selection(), vec![t.join("plain")]);
        b.columns[0].refilter(true);
        assert_eq!(b.columns[0].rows.len(), 2);
        assert_eq!(b.selection(), vec![t.join("plain")]);
    }
}
