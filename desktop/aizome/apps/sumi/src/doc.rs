//! A document: its text (the kit's `EditorContent`), its file, encoding and
//! line endings, its language and the line states that colour it, whether
//! it's changed since it was saved, and its find bar.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aizome_kit::{EditorAction, EditorContent, Selection, WindowKey};
use ropey::Rope;

use crate::autosave::SessionDoc;
use crate::files::{self, Encoding, LineEnding, Stamp};
use crate::find::Query;
use crate::syntax::{Highlight, Language};

/// The find bar's state.
#[derive(Clone, Debug, Default)]
pub struct Find {
    pub shown: bool,
    pub replace: bool,
    pub query: Query,
    pub replacement: String,
    /// What was found, in characters, for the text as it was at
    /// `version`.
    pub matches: Arc<Vec<Range<usize>>>,
    pub current: Option<usize>,
    pub error: Option<String>,
    pub version: Option<u64>,
    /// The search in flight (or done last).
    pub seq: u64,
}

impl Find {
    /// "3 of 12", "No matches", an error.
    pub fn summary(&self) -> Option<String> {
        if self.query.text.is_empty() {
            return None;
        }
        if let Some(e) = &self.error {
            return Some(e.clone());
        }
        let n = self.matches.len();
        Some(match (n, self.current) {
            (0, _) => "No matches".to_owned(),
            (n, Some(i)) if n >= crate::find::LIMIT => format!("{} of {}+", i + 1, n),
            (n, Some(i)) => format!("{} of {n}", i + 1),
            (1, None) => "1 match".to_owned(),
            (n, None) if n >= crate::find::LIMIT => format!("{n}+ matches"),
            (n, None) => format!("{n} matches"),
        })
    }
}

/// A file read into text.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub path: PathBuf,
    pub content: EditorContent,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    pub mixed: bool,
    pub binary: bool,
    pub stamp: Option<Stamp>,
    pub language: Language,
}

/// Read `path` (in `encoding`, or the one it's in).
pub fn load(path: &Path, encoding: Option<Encoding>) -> Result<Loaded, String> {
    if path.is_dir() {
        return Err("It's a folder.".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "There's no such file.".to_owned(),
        std::io::ErrorKind::PermissionDenied => "You don't have permission to read it.".to_owned(),
        _ => e.to_string(),
    })?;
    let stamp = files::stamp(path);
    let d = files::decode(&bytes, encoding);
    drop(bytes);
    let first = d.text.lines().next().unwrap_or("");
    let language = Language::detect(Some(path), first);
    Ok(Loaded {
        path: path.to_path_buf(),
        content: EditorContent::new(&d.text),
        encoding: d.encoding,
        line_ending: d.line_ending,
        mixed: d.mixed,
        binary: d.binary,
        stamp,
        language,
    })
}

/// The bytes both ropes start with.
fn common_prefix(a: &Rope, b: &Rope) -> usize {
    let mut ca = a.chunks();
    let mut cb = b.chunks();
    let (mut sa, mut sb): (&[u8], &[u8]) = (&[], &[]);
    let mut n = 0;
    loop {
        if sa.is_empty() {
            match ca.next() {
                Some(s) => sa = s.as_bytes(),
                None => return n,
            }
            continue;
        }
        if sb.is_empty() {
            match cb.next() {
                Some(s) => sb = s.as_bytes(),
                None => return n,
            }
            continue;
        }
        let k = sa.len().min(sb.len());
        if sa[..k] == sb[..k] {
            n += k;
            sa = &sa[k..];
            sb = &sb[k..];
        } else {
            return n + sa.iter().zip(sb).position(|(x, y)| x != y).unwrap_or(0);
        }
    }
}

/// The bytes both ropes end with (at most `max`).
fn common_suffix(a: &Rope, b: &Rope, max: usize) -> usize {
    let mut ca = a.chunks_at_byte(a.len_bytes()).0.reversed();
    let mut cb = b.chunks_at_byte(b.len_bytes()).0.reversed();
    let (mut sa, mut sb): (&[u8], &[u8]) = (&[], &[]);
    let mut n = 0;
    while n < max {
        if sa.is_empty() {
            match ca.next() {
                Some(s) => sa = s.as_bytes(),
                None => break,
            }
            continue;
        }
        if sb.is_empty() {
            match cb.next() {
                Some(s) => sb = s.as_bytes(),
                None => break,
            }
            continue;
        }
        let k = sa.len().min(sb.len()).min(max - n);
        let (ta, tb) = (&sa[sa.len() - k..], &sb[sb.len() - k..]);
        if ta == tb {
            n += k;
            sa = &sa[..sa.len() - k];
            sb = &sb[..sb.len() - k];
        } else {
            return n + ta.iter().rev().zip(tb.iter().rev()).position(|(x, y)| x != y).unwrap_or(0);
        }
    }
    n.min(max)
}

/// Where two texts differ: the first changed character, and where the
/// change ends in the old and in the new.
pub fn diff(old: &Rope, new: &Rope) -> (usize, usize, usize) {
    let p = common_prefix(old, new);
    let max = old.len_bytes().min(new.len_bytes()) - p;
    let q = common_suffix(old, new, max);
    let s = new.byte_to_char(p);
    let e_old = old.byte_to_char(old.len_bytes() - q);
    let e_new = new.byte_to_char(new.len_bytes() - q);
    (s, e_old.max(s), e_new.max(s))
}

pub struct Doc {
    /// Its name in the autosave folder.
    pub id: String,
    pub window: WindowKey,
    pub content: EditorContent,
    pub path: Option<PathBuf>,
    /// "Untitled N", for a document without a file.
    pub untitled: u32,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    pub language: Language,
    /// The text as last saved or read.
    pub saved: Rope,
    pub text_dirty: bool,
    /// The encoding or line endings were changed since.
    pub format_dirty: bool,
    /// The file as it was when last read or written.
    pub stamp: Option<Stamp>,
    pub highlight: Arc<Mutex<Highlight>>,
    /// Bumped when the colours change without the text.
    pub styles: u64,
    pub find: Find,
    /// Bumped with each edit, so a burst autosaves once.
    pub autosave_seq: u64,
    /// The autosave holds the text as it is.
    pub autosaved: bool,
    /// A file being read into it.
    pub loading: Option<u64>,
    /// Something to say in the status (read as Latin-1, mixed endings).
    pub notice: Option<String>,
}

impl Doc {
    pub fn new(id: String, window: WindowKey, untitled: u32) -> Doc {
        Doc {
            id,
            window,
            content: EditorContent::new(""),
            path: None,
            untitled,
            encoding: Encoding::Utf8,
            line_ending: LineEnding::Lf,
            language: Language::Markdown,
            saved: Rope::new(),
            text_dirty: false,
            format_dirty: false,
            stamp: None,
            highlight: Arc::new(Mutex::new(Highlight::new(Language::Markdown))),
            styles: 0,
            find: Find::default(),
            autosave_seq: 0,
            autosaved: true,
            loading: None,
            notice: None,
        }
    }

    pub fn dirty(&self) -> bool {
        self.text_dirty || self.format_dirty
    }

    /// Untouched: untitled, empty, unchanged (a file opened replaces it).
    pub fn pristine(&self) -> bool {
        self.path.is_none() && !self.dirty() && self.content.len_chars() == 0 && self.loading.is_none()
    }

    pub fn name(&self) -> String {
        match &self.path {
            Some(p) => {
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
            }
            None if self.untitled <= 1 => "Untitled".to_owned(),
            None => format!("Untitled {}", self.untitled),
        }
    }

    pub fn title(&self) -> String {
        if self.dirty() { format!("{} — Edited", self.name()) } else { self.name() }
    }

    /// The toolbar's status: "Ln 48, Col 12 · UTF-8 · LF".
    pub fn status(&self) -> String {
        let (line, col) = self.content.cursor_position();
        format!("Ln {}, Col {} · {} · {}", line + 1, col + 1, self.encoding.short(), self.line_ending.short())
    }

    pub fn set_language(&mut self, lang: Language) {
        self.language = lang;
        if let Ok(mut h) = self.highlight.lock() {
            h.reset(lang);
        }
        self.styles += 1;
    }

    /// A file read: its text, encoding, line endings and language.
    pub fn install(&mut self, l: Loaded) {
        self.saved = l.content.rope().clone();
        self.content = l.content;
        self.path = Some(l.path);
        self.encoding = l.encoding;
        self.line_ending = l.line_ending;
        self.stamp = l.stamp;
        self.text_dirty = false;
        self.format_dirty = false;
        self.loading = None;
        self.notice = if l.binary {
            Some("Not text: shown as it reads".to_owned())
        } else if l.mixed {
            Some(format!("Mixed line endings: saved as {}", l.line_ending.short()))
        } else if l.encoding == Encoding::Latin1 {
            Some("Read as Latin-1".to_owned())
        } else {
            None
        };
        self.set_language(l.language);
        self.find.version = None;
    }

    /// Carry out what the editor asked; whether the text changed. Lines the
    /// edit touched lose their colouring states.
    pub fn perform(&mut self, action: EditorAction) -> bool {
        let before = self.content.selection();
        // Where the change starts, when the action says.
        let hint = match &action {
            EditorAction::Select(_) | EditorAction::SelectAll => {
                self.content.perform(action);
                return false;
            }
            EditorAction::Replace { range, .. } => Some(range.start),
            EditorAction::Undo | EditorAction::Redo | EditorAction::ReplaceAll { .. } => None,
            _ => Some(before.range().start),
        };
        let undoing = matches!(action, EditorAction::Undo | EditorAction::Redo);
        let old = self.content.rope().clone();
        if !self.content.perform(action) {
            return false;
        }
        self.after_edit(&old, hint);
        if undoing {
            self.text_dirty = *self.content.rope() != self.saved;
        }
        true
    }

    /// Replace characters `range` with `text` and select `select`: one
    /// step to undo.
    pub fn replace(&mut self, range: Range<usize>, text: String, select: Selection) -> bool {
        let changed = self.perform(EditorAction::Replace { range, text });
        self.content.set_selection(select);
        changed
    }

    fn after_edit(&mut self, old: &Rope, hint: Option<usize>) {
        let new = self.content.rope();
        let after = self.content.selection();
        let (s, e_old, e_new) = match hint {
            Some(h) => {
                let r = after.range();
                let s = h.min(r.start);
                let e_new = r.end.max(s);
                let e_old =
                    (e_new as isize + old.len_chars() as isize - new.len_chars() as isize).max(s as isize) as usize;
                (s, e_old, e_new)
            }
            None => diff(old, new),
        };
        let first = new.char_to_line(s.min(new.len_chars()));
        let old_last = old.char_to_line(e_old.min(old.len_chars()));
        let new_last = new.char_to_line(e_new.min(new.len_chars()));
        if let Ok(mut h) = self.highlight.lock() {
            h.edited(first, old_last, new_last);
        }
        self.text_dirty = true;
        self.autosaved = false;
        self.autosave_seq += 1;
    }

    /// As the session keeps it.
    pub fn session(&self) -> SessionDoc {
        let s = self.content.selection();
        SessionDoc {
            id: self.id.clone(),
            path: self.path.clone(),
            untitled: self.untitled,
            language: self.language.id().to_owned(),
            encoding: self.encoding,
            line_ending: self.line_ending,
            selection: (s.anchor, s.head),
            unsaved: self.text_dirty,
            stamp: self.stamp,
        }
    }

    /// The text as the file will hold it.
    pub fn bytes(&self) -> Result<Vec<u8>, files::Unencodable> {
        let text = crate::find::text_of(self.content.rope());
        files::encode(&text, self.encoding, self.line_ending)
    }

    /// The file changed on disk since it was read or written here.
    pub fn changed_on_disk(&self) -> bool {
        match (&self.path, self.stamp) {
            (Some(p), Some(s)) => files::stamp(p).is_some_and(|now| now != s),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_find_the_change() {
        let a = Rope::from_str("one two three");
        let b = Rope::from_str("one TWO three");
        assert_eq!(diff(&a, &b), (4, 7, 7));
        let c = Rope::from_str("one three");
        assert_eq!(diff(&a, &c), (5, 9, 5), "\"wo t\" went");
        let big: String = (0..5000).map(|i| format!("line {i}\n")).collect();
        let x = Rope::from_str(&big);
        let mut y = x.clone();
        y.insert(30_000, "é");
        assert_eq!(diff(&x, &y), (30_000, 30_000, 30_001));
        assert_eq!(diff(&x, &x.clone()).0, x.len_chars());
    }

    #[test]
    fn edits_mark_their_lines_and_undo_comes_back_clean() {
        let mut d = Doc::new("t".into(), WindowKey::NONE, 1);
        d.set_language(Language::Rust);
        d.content = EditorContent::new("let a = 1;\nlet b = 2;\n");
        d.saved = d.content.rope().clone();
        d.content.set_selection(Selection::caret(4));
        d.perform(EditorAction::Insert("x".into()));
        assert!(d.dirty());
        assert_eq!(d.content.text(), "let xa = 1;\nlet b = 2;\n");
        d.perform(EditorAction::Undo);
        assert!(!d.dirty(), "back as saved");
        d.perform(EditorAction::Redo);
        assert!(d.dirty());
    }
}
