//! Notes as Markdown files: `~/Notes/**/*.md`, folders as subdirectories,
//! each note's title its first line. What isn't in the text (whether it's
//! pinned, its colour) is kept in a small index beside them,
//! `~/Notes/.fusen.json`, so the files stay plain for people, `grep` and
//! agents.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A note's colour: Aizome's amber and dyes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteColor {
    #[default]
    Amber,
    Kamenozoki,
    Hanada,
    Green,
    Rose,
}

impl NoteColor {
    pub const ALL: [NoteColor; 5] =
        [NoteColor::Amber, NoteColor::Kamenozoki, NoteColor::Hanada, NoteColor::Green, NoteColor::Rose];

    pub fn name(self) -> &'static str {
        match self {
            NoteColor::Amber => "Amber",
            NoteColor::Kamenozoki => "Kamenozoki",
            NoteColor::Hanada => "Hanada",
            NoteColor::Green => "Green",
            NoteColor::Rose => "Rose",
        }
    }

    /// The dye itself (the list's mark), as Night has it.
    pub fn dye(self) -> u32 {
        match self {
            NoteColor::Amber => 0xe0af68,
            NoteColor::Kamenozoki => 0xa9c4ff,
            NoteColor::Hanada => 0x7aa2f7,
            NoteColor::Green => 0x9ece6a,
            NoteColor::Rose => 0xf7768e,
        }
    }

    /// A sticky's paper: the dye, paled, the same in Night and Day.
    pub fn paper(self) -> u32 {
        match self {
            NoteColor::Amber => 0xf4dcab,
            NoteColor::Kamenozoki => 0xdbe6ff,
            NoteColor::Hanada => 0xc3d5fb,
            NoteColor::Green => 0xd5ebbd,
            NoteColor::Rose => 0xfbc4ce,
        }
    }

    /// The ink on its paper, and the caret's.
    pub fn ink(self) -> (u32, u32) {
        match self {
            NoteColor::Amber => (0x2a2418, 0x8a5a10),
            NoteColor::Kamenozoki => (0x1a1f33, 0x3d59a1),
            NoteColor::Hanada => (0x161c2e, 0x24418a),
            NoteColor::Green => (0x1b2414, 0x3f6a1c),
            NoteColor::Rose => (0x2e161b, 0xa3203b),
        }
    }
}

/// What the index keeps for a note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub pinned: bool,
    pub color: NoteColor,
    /// Fusen named the file after the title, and renames it with it.
    pub auto_named: bool,
}

/// `~/Notes/.fusen.json`: notes by their path under the folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Index {
    pub notes: BTreeMap<String, Meta>,
}

pub const INDEX: &str = ".fusen.json";

/// A note, as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// Its name while Fusen runs (paths change with titles).
    pub id: u64,
    pub path: PathBuf,
    /// The folder under the notes folder ("" for the notes folder itself).
    pub folder: String,
    pub title: String,
    pub preview: String,
    pub modified: SystemTime,
    pub text: Arc<str>,
    pub meta: Meta,
}

/// The title a note's text gives it: its first line that has words, past
/// front matter, without a heading's #s.
pub fn title_of(text: &str) -> String {
    lines_past_front_matter(text)
        .map(clean)
        .find(|l| !l.is_empty())
        .map(|l| truncate(&l, 120))
        .unwrap_or_else(|| "New Note".to_owned())
}

/// The line under the title, for the list.
pub fn preview_of(text: &str) -> String {
    let mut it = lines_past_front_matter(text).map(clean).filter(|l| !l.is_empty());
    it.next();
    it.next().map(|l| truncate(&l, 160)).unwrap_or_default()
}

fn lines_past_front_matter(text: &str) -> impl Iterator<Item = &str> {
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim_end() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim_end() == "---" || l.trim_end() == "..." {
                break;
            }
        }
    }
    lines
}

/// A line without its Markdown markers.
fn clean(l: &str) -> String {
    let t = l.trim();
    let t = t.trim_start_matches('#').trim_start();
    let t = t.trim_start_matches('>').trim_start();
    let t = match aizome_sumi::syntax::markdown::list_marker(t) {
        Some(m) => &t[m..],
        None => t,
    };
    let t = t.strip_prefix("[ ] ").or_else(|| t.strip_prefix("[x] ")).or_else(|| t.strip_prefix("[X] ")).unwrap_or(t);
    t.replace("**", "").replace('`', "").trim().to_owned()
}

fn truncate(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_owned(),
    }
}

/// A file name for a title: what a file system takes, short.
pub fn file_stem(title: &str) -> String {
    let mut s: String =
        title
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                    ' '
                } else {
                    c
                }
            })
            .collect();
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_start_matches('.').trim();
    let s: String = s.chars().take(60).collect();
    let s = s.trim_end_matches(['.', ' ']).to_owned();
    if s.is_empty() { "Note".to_owned() } else { s }
}

/// `dir/stem.md`, or `dir/stem 2.md` … if that's taken (by another file
/// than `except`).
pub fn free_path(dir: &Path, stem: &str, except: Option<&Path>) -> PathBuf {
    for n in 1.. {
        let name = if n == 1 { format!("{stem}.md") } else { format!("{stem} {n}.md") };
        let p = dir.join(name);
        if !p.exists() || except == Some(p.as_path()) {
            return p;
        }
    }
    unreachable!()
}

/// The notes folder and its index.
pub struct Store {
    pub root: PathBuf,
    pub index: Index,
    next_id: u64,
}

impl Store {
    pub fn new(root: PathBuf) -> Store {
        let index = std::fs::read_to_string(root.join(INDEX))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Store { root, index, next_id: 0 }
    }

    pub fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// A note's path under the folder, as the index keys it.
    pub fn key(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().into_owned()
    }

    pub fn meta(&self, path: &Path) -> Meta {
        self.index.notes.get(&self.key(path)).copied().unwrap_or_default()
    }

    pub fn set_meta(&mut self, path: &Path, meta: Meta) {
        let k = self.key(path);
        if meta == Meta::default() {
            self.index.notes.remove(&k);
        } else {
            self.index.notes.insert(k, meta);
        }
        self.save_index();
    }

    /// A note moved: its index entry with it.
    pub fn moved(&mut self, from: &Path, to: &Path) {
        if let Some(m) = self.index.notes.remove(&self.key(from)) {
            self.index.notes.insert(self.key(to), m);
            self.save_index();
        }
    }

    pub fn save_index(&self) {
        if !self.root.is_dir() {
            return;
        }
        // Entries for notes that are gone go too.
        let mut index = self.index.clone();
        index.notes.retain(|k, _| self.root.join(k).exists());
        if let Ok(json) = serde_json::to_string_pretty(&index) {
            let _ = aizome_sumi::files::save_atomic(&self.root.join(INDEX), json.as_bytes());
        }
    }

    /// The folders under the notes folder ("Work", "Work/Projects"), in
    /// order.
    pub fn folders(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk_dirs(&self.root, &self.root, &mut out);
        out.sort_by_key(|f| f.to_lowercase());
        out
    }

    /// Every note, read.
    pub fn scan(&mut self, keep_ids: &BTreeMap<PathBuf, u64>) -> Vec<Note> {
        let mut files = Vec::new();
        walk_files(&self.root, &mut files);
        let mut notes = Vec::with_capacity(files.len());
        for path in files {
            let id = keep_ids.get(&path).copied().unwrap_or_else(|| self.fresh_id());
            if let Some(n) = self.read(&path, id) {
                notes.push(n);
            }
        }
        notes
    }

    /// One note, read.
    pub fn read(&self, path: &Path, id: u64) -> Option<Note> {
        let m = std::fs::metadata(path).ok()?;
        if m.len() > 4 << 20 {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let text = aizome_sumi::files::decode(&bytes, None).text;
        let folder = path
            .parent()
            .and_then(|p| p.strip_prefix(&self.root).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(Note {
            id,
            path: path.to_path_buf(),
            folder,
            title: title_of(&text),
            preview: preview_of(&text),
            modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            text: text.into(),
            meta: self.meta(path),
        })
    }
}

fn hidden(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

fn walk_dirs(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if hidden(&p) || !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
        walk_dirs(root, &p, out);
    }
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if hidden(&p) {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => walk_files(&p, out),
            Ok(_) if p.extension().is_some_and(|x| x == "md" || x == "markdown" || x == "txt") => out.push(p),
            _ => {}
        }
    }
}

/// Put `path` in the freedesktop Trash under `trash` (`~/.local/share/Trash`):
/// the file in `files/`, where it was in `info/`.
pub fn trash(path: &Path, trash: &Path) -> std::io::Result<()> {
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "note.md".into());
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_owned(), format!(".{e}")),
        None => (name.clone(), String::new()),
    };
    let mut target = files.join(&name);
    let mut n = 2;
    while target.exists()
        || info.join(format!("{}.trashinfo", target.file_name().unwrap_or_default().to_string_lossy())).exists()
    {
        target = files.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    let tname = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let date = aizome_sumi::recent::iso8601(SystemTime::now());
    let date = &date[..19];
    let abs = std::path::absolute(path)?;
    let uri = aizome_sumi::recent::file_uri(&abs);
    let encoded = uri
        .trim_start_matches("file://")
        .replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    std::fs::write(
        info.join(format!("{tname}.trashinfo")),
        format!("[Trash Info]\nPath={encoded}\nDeletionDate={date}\n"),
    )?;
    if std::fs::rename(path, &target).is_err() {
        std::fs::copy(path, &target)?;
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_previews() {
        assert_eq!(title_of("# Groceries\n- [ ] milk\n- [x] eggs\n"), "Groceries");
        assert_eq!(preview_of("# Groceries\n- [ ] milk\n- [x] eggs\n"), "milk");
        assert_eq!(title_of("---\npinned: true\n---\n\nIdeas for **Fusen**\nmore"), "Ideas for Fusen");
        assert_eq!(title_of("\n\n"), "New Note");
        assert_eq!(file_stem("a/b: c?"), "a b c");
        assert_eq!(file_stem("..."), "Note");
    }

    #[test]
    fn a_store_scans_folders_and_keeps_its_index() {
        let root = std::env::temp_dir().join(format!("fusen-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Work/Projects")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("Groceries.md"), "# Groceries\n- [ ] milk\n").unwrap();
        std::fs::write(root.join("Work/Plan.md"), "Plan\nship it\n").unwrap();
        std::fs::write(root.join(".hidden/x.md"), "x").unwrap();
        let mut s = Store::new(root.clone());
        assert_eq!(s.folders(), ["Work", "Work/Projects"]);
        let notes = s.scan(&BTreeMap::new());
        assert_eq!(notes.len(), 2);
        let plan = notes.iter().find(|n| n.title == "Plan").unwrap();
        assert_eq!((plan.folder.as_str(), plan.preview.as_str()), ("Work", "ship it"));
        s.set_meta(&plan.path, Meta { pinned: true, color: NoteColor::Rose, auto_named: false });
        let again = Store::new(root.clone());
        assert!(again.meta(&root.join("Work/Plan.md")).pinned);
        assert_eq!(free_path(&root, "Groceries", None), root.join("Groceries 2.md"));
        let trash_dir = root.join(".trash-test");
        trash(&root.join("Groceries.md"), &trash_dir).unwrap();
        assert!(!root.join("Groceries.md").exists());
        assert!(trash_dir.join("files/Groceries.md").exists());
        let info = std::fs::read_to_string(trash_dir.join("info/Groceries.md.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/") && info.contains("Groceries.md"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
