//! The text of a `text_editor`: [`EditorContent`], which the program owns,
//! and the [`EditorAction`]s the editor sends back, which the program
//! applies with [`EditorContent::perform`].
//!
//! The text is a rope (ropey), so edits anywhere in a long document cost
//! the same, and handing it to a view is a cheap clone. Positions are
//! character indices. Undo groups a burst of typing into one step.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ropey::Rope;

use crate::editing::{next_word, prev_word};

/// The selection: from `anchor` to `head` (the caret), in characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Selection {
        Selection { anchor: at, head: at }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// How much a deletion takes when nothing is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Granularity {
    Char,
    Word,
    /// To the line's start (❖⌫) or end (⌃K).
    Line,
}

/// What the editor asks of its content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// Typed text, over the selection (a burst of typing undoes at once).
    Insert(String),
    /// Pasted (or dropped) text, over the selection: a step of its own.
    Paste(String),
    /// A new line, indented as the one it leaves.
    Newline,
    /// Tab: a tab (or spaces) at the caret, or the selected lines indented.
    Indent,
    /// ⇧Tab: the selected lines outdented.
    Outdent,
    Backspace(Granularity),
    Delete(Granularity),
    /// The caret and selection moved.
    Select(Selection),
    SelectAll,
    Undo,
    Redo,
    /// Replace characters `range` with `text` (find and replace).
    Replace {
        range: Range<usize>,
        text: String,
    },
    /// Replace every occurrence of `query` with `text`.
    ReplaceAll {
        query: String,
        text: String,
        ignore_case: bool,
    },
}

impl EditorAction {
    /// Whether it changes the text (not just the selection).
    pub fn edits(&self) -> bool {
        !matches!(self, EditorAction::Select(_) | EditorAction::SelectAll)
    }
}

/// An edit's record: what was at `at`, and what is now.
#[derive(Clone, Debug)]
struct Change {
    at: usize,
    removed: String,
    inserted: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Typing,
    Deleting,
    Other,
}

#[derive(Clone, Debug)]
struct Step {
    changes: Vec<Change>,
    before: Selection,
    after: Selection,
    kind: Kind,
    at: Instant,
}

/// Typing within this of the last keystroke undoes with it.
const GROUP: Duration = Duration::from_millis(1200);

/// The text a `text_editor` shows and edits, its selection, and its undo
/// history.
#[derive(Clone, Debug, Default)]
pub struct EditorContent {
    rope: Rope,
    selection: Selection,
    version: u64,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// A tab inserts this many spaces (0: a tab character).
    pub soft_tabs: usize,
}

/// What a view of an editor holds: the text (a cheap clone of the rope),
/// the selection, and the version (bumped with every edit).
#[derive(Clone, Debug)]
pub struct EditorSnapshot {
    pub rope: Rope,
    pub selection: Selection,
    pub version: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}

impl EditorContent {
    pub fn new(text: &str) -> EditorContent {
        EditorContent { rope: Rope::from_str(text), ..EditorContent::default() }
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn selection(&self) -> Selection {
        self.selection
    }

    pub fn selected_text(&self) -> String {
        let r = self.selection.range();
        self.rope.slice(r).to_string()
    }

    /// Bumped with every change to the text.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The caret's line and column (characters), from 0.
    pub fn cursor_position(&self) -> (usize, usize) {
        let head = self.selection.head.min(self.rope.len_chars());
        let line = self.rope.char_to_line(head);
        (line, head - self.rope.line_to_char(line))
    }

    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            rope: self.rope.clone(),
            selection: self.selection,
            version: self.version,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
        }
    }

    /// Replace the whole text (a file loaded): history starts over.
    pub fn set_text(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.selection = Selection::caret(0);
        self.undo.clear();
        self.redo.clear();
        self.version += 1;
    }

    pub fn set_selection(&mut self, s: Selection) {
        let n = self.rope.len_chars();
        self.selection = Selection { anchor: s.anchor.min(n), head: s.head.min(n) };
    }

    /// Carry out what the editor asked: whether the text changed.
    pub fn perform(&mut self, action: EditorAction) -> bool {
        let n = self.rope.len_chars();
        match action {
            EditorAction::Select(s) => {
                self.set_selection(s);
                false
            }
            EditorAction::SelectAll => {
                self.selection = Selection { anchor: 0, head: n };
                false
            }
            EditorAction::Insert(text) => {
                let kind = if text.contains('\n') { Kind::Other } else { Kind::Typing };
                self.replace_selection(&text, kind)
            }
            EditorAction::Paste(text) => self.replace_selection(&text, Kind::Other),
            EditorAction::Newline => {
                let (line, _) = self.cursor_position();
                let indent: String = self.rope.line(line).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                self.replace_selection(&format!("\n{indent}"), Kind::Other)
            }
            EditorAction::Indent => {
                if self.selection.is_empty() || self.line_span().len() <= 1 && !self.whole_lines_selected() {
                    let tab = if self.soft_tabs > 0 { " ".repeat(self.soft_tabs) } else { "\t".into() };
                    self.replace_selection(&tab, Kind::Typing)
                } else {
                    self.indent_lines(true)
                }
            }
            EditorAction::Outdent => self.indent_lines(false),
            EditorAction::Backspace(g) => {
                if self.selection.is_empty() {
                    let head = self.selection.head.min(n);
                    let from = match g {
                        Granularity::Char => head.saturating_sub(1),
                        Granularity::Word => self.word_before(head),
                        Granularity::Line => {
                            let l = self.rope.char_to_line(head);
                            let start = self.rope.line_to_char(l);
                            if start == head { head.saturating_sub(1) } else { start }
                        }
                    };
                    self.selection = Selection { anchor: from, head };
                }
                self.replace_selection("", Kind::Deleting)
            }
            EditorAction::Delete(g) => {
                if self.selection.is_empty() {
                    let head = self.selection.head.min(n);
                    let to = match g {
                        Granularity::Char => (head + 1).min(n),
                        Granularity::Word => self.word_after(head),
                        Granularity::Line => {
                            let l = self.rope.char_to_line(head);
                            let end = self.line_end(l);
                            if end == head { (head + 1).min(n) } else { end }
                        }
                    };
                    self.selection = Selection { anchor: head, head: to };
                }
                self.replace_selection("", Kind::Deleting)
            }
            EditorAction::Undo => self.undo(),
            EditorAction::Redo => self.redo(),
            EditorAction::Replace { range, text } => {
                let r = range.start.min(n)..range.end.min(n);
                self.selection = Selection { anchor: r.start, head: r.end };
                self.replace_selection(&text, Kind::Other)
            }
            EditorAction::ReplaceAll { query, text, ignore_case } => {
                let found = self.find_all(&query, ignore_case);
                if found.is_empty() {
                    return false;
                }
                let before = self.selection;
                let mut changes = Vec::new();
                // From the end, so earlier ranges stay put.
                for r in found.iter().rev() {
                    let removed = self.rope.slice(r.clone()).to_string();
                    self.rope.remove(r.clone());
                    self.rope.insert(r.start, &text);
                    changes.push(Change { at: r.start, removed, inserted: text.clone() });
                }
                self.finish_step(changes, before, Kind::Other);
                true
            }
        }
    }

    fn line_end(&self, line: usize) -> usize {
        let start = self.rope.line_to_char(line);
        let slice = self.rope.line(line);
        let mut len = slice.len_chars();
        while len > 0 && matches!(slice.char(len - 1), '\n' | '\r') {
            len -= 1;
        }
        start + len
    }

    fn line_span(&self) -> Range<usize> {
        let r = self.selection.range();
        let a = self.rope.char_to_line(r.start);
        let mut b = self.rope.char_to_line(r.end);
        if b > a && self.rope.line_to_char(b) == r.end {
            b -= 1;
        }
        a..b + 1
    }

    fn whole_lines_selected(&self) -> bool {
        let r = self.selection.range();
        let a = self.rope.char_to_line(r.start);
        self.rope.line_to_char(a) == r.start && !self.selection.is_empty()
    }

    fn word_before(&self, at: usize) -> usize {
        let line = self.rope.char_to_line(at);
        let start = self.rope.line_to_char(line);
        if at == start {
            return at.saturating_sub(1);
        }
        let text: String = self.rope.slice(start..at).to_string();
        let b = prev_word(&text, text.len());
        start + text[..b].chars().count()
    }

    fn word_after(&self, at: usize) -> usize {
        let line = self.rope.char_to_line(at);
        let end = self.line_end(line);
        if at >= end {
            return (at + 1).min(self.rope.len_chars());
        }
        let text: String = self.rope.slice(at..end).to_string();
        let b = next_word(&text, 0);
        at + text[..b].chars().count()
    }

    /// Replace the selection with `text`, recording it.
    fn replace_selection(&mut self, text: &str, kind: Kind) -> bool {
        let before = self.selection;
        let r = before.range();
        let n = self.rope.len_chars();
        let r = r.start.min(n)..r.end.min(n);
        if r.is_empty() && text.is_empty() {
            return false;
        }
        let removed = self.rope.slice(r.clone()).to_string();
        self.rope.remove(r.clone());
        self.rope.insert(r.start, text);
        let at = r.start + text.chars().count();
        self.selection = Selection::caret(at);
        self.finish_step(vec![Change { at: r.start, removed, inserted: text.to_owned() }], before, kind);
        true
    }

    fn finish_step(&mut self, changes: Vec<Change>, before: Selection, kind: Kind) {
        self.version += 1;
        self.redo.clear();
        let now = Instant::now();
        // A burst of typing (or of deleting) is one step.
        if kind != Kind::Other
            && let Some(last) = self.undo.last_mut()
            && last.kind == kind
            && now.duration_since(last.at) < GROUP
            && last.after == before
            && changes.len() == 1
        {
            last.changes.extend(changes);
            last.after = self.selection;
            last.at = now;
            return;
        }
        self.undo.push(Step { changes, before, after: self.selection, kind, at: now });
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
    }

    fn indent_lines(&mut self, indent: bool) -> bool {
        let before = self.selection;
        let lines = self.line_span();
        let unit = if self.soft_tabs > 0 { " ".repeat(self.soft_tabs) } else { "\t".into() };
        let mut changes = Vec::new();
        for l in lines.clone().rev() {
            let start = self.rope.line_to_char(l);
            if indent {
                self.rope.insert(start, &unit);
                changes.push(Change { at: start, removed: String::new(), inserted: unit.clone() });
            } else {
                let line = self.rope.line(l);
                let take = if line.char(0) == '\t' {
                    1
                } else {
                    line.chars().take(self.soft_tabs.max(4)).take_while(|c| *c == ' ').count()
                };
                if take > 0 && line.len_chars() > 0 {
                    let removed = self.rope.slice(start..start + take).to_string();
                    self.rope.remove(start..start + take);
                    changes.push(Change { at: start, removed, inserted: String::new() });
                }
            }
        }
        if changes.is_empty() {
            return false;
        }
        let first = self.rope.line_to_char(lines.start);
        let last = self.line_end(lines.end - 1);
        self.selection = Selection { anchor: first, head: last };
        self.finish_step(changes, before, Kind::Other);
        true
    }

    fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        for c in step.changes.iter().rev() {
            let end = c.at + c.inserted.chars().count();
            self.rope.remove(c.at..end);
            self.rope.insert(c.at, &c.removed);
        }
        self.selection = step.before;
        self.version += 1;
        self.redo.push(step);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        for c in &step.changes {
            let end = c.at + c.removed.chars().count();
            self.rope.remove(c.at..end);
            self.rope.insert(c.at, &c.inserted);
        }
        self.selection = step.after;
        self.version += 1;
        self.undo.push(step);
        true
    }

    /// Every occurrence of `query`, in characters.
    pub fn find_all(&self, query: &str, ignore_case: bool) -> Vec<Range<usize>> {
        if query.is_empty() {
            return Vec::new();
        }
        let text = self.rope.to_string();
        let (hay, needle) =
            if ignore_case { (text.to_lowercase(), query.to_lowercase()) } else { (text.clone(), query.to_owned()) };
        if hay.len() != text.len() {
            // Case folding changed lengths: fall back to exact matching.
            return find_bytes(&text, query).into_iter().map(|r| to_chars(&text, r)).collect();
        }
        find_bytes(&hay, &needle).into_iter().map(|r| to_chars(&text, r)).collect()
    }

    /// The next occurrence of `query` after the selection (or before it),
    /// wrapping round.
    pub fn find_next(&self, query: &str, ignore_case: bool, backwards: bool) -> Option<Range<usize>> {
        let all = self.find_all(query, ignore_case);
        let sel = self.selection.range();
        if backwards {
            all.iter().rev().find(|r| r.end <= sel.start).or(all.last()).cloned()
        } else {
            all.iter().find(|r| r.start >= sel.end && *r != &sel).or(all.first()).cloned()
        }
    }
}

fn find_bytes(hay: &str, needle: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let s = from + i;
        out.push(s..s + needle.len());
        from = s + needle.len().max(1);
        if from > hay.len() {
            break;
        }
    }
    out
}

fn to_chars(text: &str, r: Range<usize>) -> Range<usize> {
    let a = text[..r.start].chars().count();
    a..a + text[r].chars().count()
}

/// How a run of an editor's line looks: what a highlighter gives, by byte
/// range of the line.
#[derive(Clone, Debug, PartialEq)]
pub struct LineStyle {
    pub range: Range<usize>,
    pub span: crate::text::Span,
}

/// Styles lines for an editor (syntax, Markdown): called for the lines
/// that show, with each line's text and number.
pub type Highlighter = Arc<dyn Fn(usize, &str) -> Vec<LineStyle> + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_undo_in_bursts() {
        let mut c = EditorContent::new("");
        for ch in "hello".chars() {
            c.perform(EditorAction::Insert(ch.to_string()));
        }
        c.perform(EditorAction::Newline);
        c.perform(EditorAction::Insert("w".into()));
        assert_eq!(c.text(), "hello\nw");
        c.perform(EditorAction::Undo);
        assert_eq!(c.text(), "hello\n");
        c.perform(EditorAction::Undo);
        assert_eq!(c.text(), "hello");
        c.perform(EditorAction::Undo);
        assert_eq!(c.text(), "");
        c.perform(EditorAction::Redo);
        assert_eq!(c.text(), "hello");
        assert_eq!(c.selection(), Selection::caret(5));
    }

    #[test]
    fn newlines_keep_the_indent_and_deletions_take_words() {
        let mut c = EditorContent::new("    let x = 1;");
        c.set_selection(Selection::caret(14));
        c.perform(EditorAction::Newline);
        assert_eq!(c.text(), "    let x = 1;\n    ");
        c.set_selection(Selection::caret(9));
        c.perform(EditorAction::Backspace(Granularity::Word));
        assert_eq!(c.text(), "    let  = 1;\n    ");
        c.perform(EditorAction::Delete(Granularity::Line));
        assert_eq!(c.text(), "    let \n    ");
    }

    #[test]
    fn lines_indent_and_outdent() {
        let mut c = EditorContent::new("a\nb\nc");
        c.soft_tabs = 2;
        c.set_selection(Selection { anchor: 0, head: 3 });
        c.perform(EditorAction::Indent);
        assert_eq!(c.text(), "  a\n  b\nc");
        c.perform(EditorAction::Outdent);
        assert_eq!(c.text(), "a\nb\nc");
    }

    #[test]
    fn find_and_replace() {
        let mut c = EditorContent::new("Damage is per layer. damage contains");
        assert_eq!(c.find_all("damage", true), [0..6, 21..27]);
        c.set_selection(Selection { anchor: 0, head: 6 });
        assert_eq!(c.find_next("damage", true, false), Some(21..27));
        c.perform(EditorAction::ReplaceAll { query: "damage".into(), text: "harm".into(), ignore_case: true });
        assert_eq!(c.text(), "harm is per layer. harm contains");
        c.perform(EditorAction::Undo);
        assert_eq!(c.text(), "Damage is per layer. damage contains");
    }

    #[test]
    fn positions_are_lines_and_columns() {
        let mut c = EditorContent::new("ab\ncdé\n");
        c.set_selection(Selection::caret(6));
        assert_eq!(c.cursor_position(), (1, 3));
        assert_eq!(c.line_count(), 3);
    }
}
