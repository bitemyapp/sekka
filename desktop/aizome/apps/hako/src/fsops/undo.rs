//! Undo and redo: each operation leaves a [`Record`] of what it did, and
//! undoing one carries out its inverse, which leaves the record that
//! redoes it.
//!
//! - a rename is renamed back;
//! - a move is moved back (a rename, or a copy and delete across
//!   filesystems, asking about conflicts as any move does);
//! - items made (copies, duplicates, new folders, extracted archives) go
//!   to the Trash, and redoing restores them;
//! - items trashed are restored, and redoing trashes them again.
//!
//! Nothing is undone over something newer: an item that's gone, or a
//! name that's been taken since, is reported, not overwritten.

use std::path::PathBuf;

use super::transfer::{self, Item, Mode};
use super::trash::{Trash, Trashed};
use super::{Env, Report, sys};

/// What an operation did, to undo.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    /// `from` was renamed `to`.
    Rename { from: PathBuf, to: PathBuf },
    /// Each item moved from the first path to the second.
    Move { pairs: Vec<(PathBuf, PathBuf)> },
    /// These were made.
    Made { paths: Vec<PathBuf> },
    /// These went to the Trash.
    Trash { items: Vec<Trashed> },
}

impl Record {
    /// How many items it's about.
    pub fn len(&self) -> usize {
        match self {
            Record::Rename { .. } => 1,
            Record::Move { pairs } => pairs.len(),
            Record::Made { paths } => paths.len(),
            Record::Trash { items } => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An undoable step: its record, and its name for the Edit menu ("Move
/// of 3 Items").
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub record: Record,
    pub label: String,
}

/// "3 Items" or "“notes.txt”".
pub fn items_label(paths: &[PathBuf]) -> String {
    match paths {
        [one] => format!("“{}”", one.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()),
        _ => format!("{} Items", paths.len()),
    }
}

/// The undo and redo stacks.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

/// How many steps are kept.
const DEPTH: usize = 64;

impl History {
    /// Something was done: it can be undone, and what was undone before
    /// can't be redone any more.
    pub fn push(&mut self, step: Step) {
        if step.record.is_empty() {
            return;
        }
        self.undo.push(step);
        if self.undo.len() > DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn take_undo(&mut self) -> Option<Step> {
        self.undo.pop()
    }

    pub fn take_redo(&mut self) -> Option<Step> {
        self.redo.pop()
    }

    /// An undo was carried out: what redoes it.
    pub fn undone(&mut self, inverse: Step) {
        if !inverse.record.is_empty() {
            self.redo.push(inverse);
        }
    }

    /// A redo was carried out: what undoes it again.
    pub fn redone(&mut self, inverse: Step) {
        if !inverse.record.is_empty() {
            self.undo.push(inverse);
        }
    }
}

/// Carry out the inverse of `record`: what happened, and the record of
/// the inverse (to redo, or undo again).
pub fn invert(record: &Record, trash: &Trash, env: &mut Env) -> (Report, Record) {
    match record {
        Record::Rename { from, to } => {
            let mut report = Report::default();
            if std::fs::symlink_metadata(to).is_err() {
                report.failed.push((to.clone(), "it isn't there any more".into()));
                return (report, Record::Move { pairs: Vec::new() });
            }
            match sys::rename_noreplace(to, from) {
                Ok(()) => {
                    report.done.push((to.clone(), from.clone()));
                    (report, Record::Rename { from: to.clone(), to: from.clone() })
                }
                Err(e) => {
                    let why = if e.kind() == std::io::ErrorKind::AlreadyExists {
                        format!("“{}” is taken now", from.file_name().unwrap_or_default().to_string_lossy())
                    } else {
                        super::describe(&e)
                    };
                    report.failed.push((to.clone(), why));
                    (report, Record::Move { pairs: Vec::new() })
                }
            }
        }
        Record::Move { pairs } => {
            let items: Vec<Item> =
                pairs.iter().map(|(was, now)| Item { source: now.clone(), target: was.clone() }).collect();
            let report = transfer::transfer_items(&items, Mode::Move, env, Some(trash));
            let back = report.done.iter().map(|(now, was)| (now.clone(), was.clone())).collect();
            (report, Record::Move { pairs: back })
        }
        Record::Made { paths } => {
            let (report, items) = transfer::trash_items(paths, trash, env);
            (report, Record::Trash { items })
        }
        Record::Trash { items } => {
            let mut report = Report::default();
            let mut entries = Vec::new();
            for t in items {
                match trash.entry_for(&t.file) {
                    Some(e) => entries.push(e),
                    None => report.failed.push((t.original.clone(), "it isn't in the Trash any more".into())),
                }
            }
            report.absorb(transfer::restore(&entries, trash, env));
            let paths = report.done.iter().map(|(_, to)| to.clone()).collect();
            (report, Record::Made { paths })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::{TempDir, read};
    use crate::fsops::{Always, Control, ErrorChoice, Resolution};

    fn trash(t: &TempDir) -> Trash {
        Trash { home: t.join("Trash"), home_dir: Some(t.join("home")), uid: sys::uid(), topdirs: Some(Vec::new()) }
    }

    fn inv(record: &Record, tr: &Trash) -> (Report, Record) {
        let control = Control::new();
        let mut asker = Always(Resolution::Skip, ErrorChoice::Skip);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        invert(record, tr, &mut env)
    }

    #[test]
    fn renames_undo_and_redo() {
        let t = TempDir::new("undo-rename");
        let tr = trash(&t);
        let a = t.file("a.txt", "a");
        let b = t.join("b.txt");
        std::fs::rename(&a, &b).unwrap();
        let (r, redo) = inv(&Record::Rename { from: a.clone(), to: b.clone() }, &tr);
        assert!(r.is_clean());
        assert_eq!(read(&a), "a");
        let (_, undo_again) = inv(&redo, &tr);
        assert_eq!(read(&b), "a");
        // A name taken since isn't overwritten.
        t.file("a.txt", "newer");
        let (r, _) = inv(&undo_again, &tr);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(read(&a), "newer");
        assert_eq!(read(&b), "a");
    }

    #[test]
    fn moves_go_back() {
        let t = TempDir::new("undo-move");
        let tr = trash(&t);
        let a = t.file("from/a.txt", "a");
        let to = t.dir("to").join("a.txt");
        std::fs::rename(&a, &to).unwrap();
        let (r, redo) = inv(&Record::Move { pairs: vec![(a.clone(), to.clone())] }, &tr);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(read(&a), "a");
        assert!(!to.exists());
        let (r, _) = inv(&redo, &tr);
        assert!(r.is_clean());
        assert_eq!(read(&to), "a");
    }

    #[test]
    fn made_items_go_to_the_trash_and_come_back() {
        let t = TempDir::new("undo-made");
        let tr = trash(&t);
        let made = t.file("home/copy.txt", "c");
        let (r, redo) = inv(&Record::Made { paths: vec![made.clone()] }, &tr);
        assert!(r.is_clean());
        assert!(!made.exists());
        assert!(matches!(&redo, Record::Trash { items } if items.len() == 1));
        let (r, again) = inv(&redo, &tr);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(read(&made), "c");
        assert_eq!(again, Record::Made { paths: vec![made] });
    }

    #[test]
    fn the_history_keeps_order() {
        let mut h = History::default();
        let step = |n: &str| Step { record: Record::Made { paths: vec![PathBuf::from(n)] }, label: n.into() };
        h.push(step("one"));
        h.push(step("two"));
        assert_eq!(h.undo_label(), Some("two"));
        let s = h.take_undo().unwrap();
        h.undone(s);
        assert_eq!(h.redo_label(), Some("two"));
        assert_eq!(h.undo_label(), Some("one"));
        h.push(step("three"));
        assert_eq!(h.redo_label(), None, "doing something new forgets the redo");
        h.push(Step { record: Record::Made { paths: vec![] }, label: "nothing".into() });
        assert_eq!(h.undo_label(), Some("three"));
        assert_eq!(items_label(&[PathBuf::from("/x/notes.txt")]), "“notes.txt”");
        assert_eq!(items_label(&[PathBuf::from("a"), PathBuf::from("b")]), "2 Items");
    }
}
