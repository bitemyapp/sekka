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
//! name that's been taken since, is reported, not overwritten. A step
//! stays in the [`History`] until its undo (or redo) comes back: what it
//! didn't carry out stays to try again, so a failed undo doesn't lose its
//! step, nor let the next ⌘Z undo the step before it unasked.

use std::path::PathBuf;

use aizome_typestate::{Current, Generation, Ticket};

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

/// How many steps are kept.
const DEPTH: usize = 64;

/// Names the undos and redos the history hands out, for their
/// [`Ticket`]s.
pub enum Inverting {}

/// The undo and redo stacks: what the history's states share.
#[derive(Debug, Default)]
pub struct Stacks {
    undo: Vec<Step>,
    redo: Vec<Step>,
    inversions: Generation<Inverting>,
}

impl Stacks {
    /// Put `step` on top of the undo stack: whether that pushed the oldest
    /// step out.
    fn push_undo(&mut self, step: Step) -> bool {
        self.undo.push(step);
        let full = self.undo.len() > DEPTH;
        if full {
            self.undo.remove(0);
        }
        full
    }
}

/// The step being undone or redone. It stays in the history, here, until
/// its result comes back: only that takes it out.
#[derive(Debug)]
pub struct InFlight {
    ticket: Ticket<Inverting>,
    step: Step,
    /// Its place in the stack it came from, where what isn't carried out
    /// goes back.
    at: usize,
    /// For a redo: a new action since made what's left of it impossible
    /// to redo.
    forgotten: bool,
}

aizome_typestate::machine! {
    /// The undo and redo stacks, and the undo or redo being carried out
    /// (one at a time). A step leaves the history only through the result
    /// of its inversion ([`Undoing::finish`]): what was carried out can be
    /// redone (or undone again), and what wasn't stays where it was.
    #[derive(Debug)]
    pub struct History over Stacks {
        /// Where the history is.
        #[derive(Debug)]
        stored HistoryState;
        /// The history's state, typed.
        typed HistoryAny;
        states {
            /// Nothing is being undone or redone. Only the end of an undo or redo
            /// comes back here; giving one up can't:
            ///
            /// ```compile_fail,E0624
            /// use hako::fsops::undo::{Idle, Stacks};
            /// // Back to idle without the undo's result: its step would be lost.
            /// fn give_up(stacks: &mut Stacks) -> Idle<'_> {
            ///     Idle::enter(stacks, ())
            /// }
            /// ```
            ///
            /// ```
            /// use hako::fsops::undo::{Fresh, Idle, Undoing};
            /// fn done<'s>(undoing: Undoing<'s>, fresh: Fresh) -> Idle<'s> {
            ///     undoing.finish(fresh)
            /// }
            /// ```
            Idle,
            /// A step being undone.
            Undoing(InFlight),
            /// A step being redone.
            Redoing(InFlight),
        }
        // `History::default()` is the way in.
        new(private);
    }
}

impl Default for History {
    fn default() -> History {
        History::new(Stacks::default())
    }
}

/// An undo or redo to carry out ([`invert`]): a copy of its step's record
/// and the ticket its result comes back with. Only the history makes one.
#[derive(Debug)]
pub struct Inversion {
    ticket: Ticket<Inverting>,
    record: Record,
    label: String,
    redo: bool,
}

impl Inversion {
    /// The step's name ("Move of 3 Items").
    pub fn label(&self) -> &str {
        &self.label
    }

    /// A redo, not an undo.
    pub fn is_redo(&self) -> bool {
        self.redo
    }
}

/// What an undo or redo came to, for the history: the record that redoes
/// (or undoes again) the part carried out, and the part that wasn't (a
/// name taken since, a conflict stopped at), which stays to try again.
/// Only [`invert`] makes one, so no caller can declare a failed undo done:
///
/// ```compile_fail,E0451
/// use aizome_typestate::Ticket;
/// use hako::fsops::undo::{Inverted, Inverting};
/// // What a failed undo came to: nothing to redo, and its step forgotten.
/// fn failed(ticket: Ticket<Inverting>) -> Inverted {
///     Inverted { ticket, inverse: None, left: None }
/// }
/// ```
///
/// ```
/// use hako::fsops::Env;
/// use hako::fsops::trash::Trash;
/// use hako::fsops::undo::{Inversion, Inverted, invert};
/// fn undo(inversion: Inversion, trash: &Trash, env: &mut Env) -> Inverted {
///     invert(inversion, trash, env).1
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Inverted {
    ticket: Ticket<Inverting>,
    inverse: Option<Record>,
    left: Option<Record>,
}

/// Proof that an [`Inverted`] is the result of the undo or redo in
/// flight, not a stale one: only [`Undoing::check`] and
/// [`Redoing::check`] make one.
#[derive(Debug)]
pub struct Fresh {
    current: Current<Inverting>,
    inverted: Inverted,
}

fn check(ctx: &Stacks, flight: &InFlight, inverted: Inverted) -> Result<Fresh, Inverted> {
    match ctx.inversions.check(inverted.ticket) {
        Some(current) if current.ticket() == flight.ticket => Ok(Fresh { current, inverted }),
        _ => Err(inverted),
    }
}

/// `step`'s record, as what's left of it: in the stack at `at` (or its
/// top), within the depth.
fn put_back(stack: &mut Vec<Step>, at: usize, left: Option<Record>, label: &str) {
    if let Some(record) = left {
        stack.insert(at.min(stack.len()), Step { record, label: label.to_owned() });
        if stack.len() > DEPTH {
            stack.remove(0);
        }
    }
}

impl<'s> Idle<'s> {
    /// Something was done: it can be undone, and what was undone before
    /// can't be redone any more.
    pub fn push(self, step: Step) -> Idle<'s> {
        self.ctx.push_undo(step);
        self.ctx.redo.clear();
        self
    }

    /// Begin undoing the last step. It stays in the history (in
    /// [`Undoing`]) until its result comes back; the job gets a copy.
    pub fn undo(self) -> Result<(Undoing<'s>, Inversion), Idle<'s>> {
        let Some(step) = self.ctx.undo.pop() else { return Err(self) };
        let at = self.ctx.undo.len();
        let ticket = self.ctx.inversions.issue();
        let inversion = Inversion { ticket, record: step.record.clone(), label: step.label.clone(), redo: false };
        Ok((Undoing::enter(self.ctx, InFlight { ticket, step, at, forgotten: false }), inversion))
    }

    /// Begin redoing the last step undone, the same way.
    pub fn redo(self) -> Result<(Redoing<'s>, Inversion), Idle<'s>> {
        let Some(step) = self.ctx.redo.pop() else { return Err(self) };
        let at = self.ctx.redo.len();
        let ticket = self.ctx.inversions.issue();
        let inversion = Inversion { ticket, record: step.record.clone(), label: step.label.clone(), redo: true };
        Ok((Redoing::enter(self.ctx, InFlight { ticket, step, at, forgotten: false }), inversion))
    }
}

impl<'s> Undoing<'s> {
    /// Something was done meanwhile: it goes on top, and the step being
    /// undone keeps its place below it.
    pub fn push(mut self, step: Step) -> Undoing<'s> {
        if self.ctx.push_undo(step) {
            self.data.at = self.data.at.saturating_sub(1);
        }
        self.ctx.redo.clear();
        self
    }

    /// The proof that `inverted` is this undo's result.
    pub fn check(&self, inverted: Inverted) -> Result<Fresh, Inverted> {
        check(self.ctx, &self.data, inverted)
    }

    /// The undo's result: what it undid can be redone, and what it didn't
    /// stays to undo, where it was. Only a result checked fresh will do:
    ///
    /// ```compile_fail,E0308
    /// use hako::fsops::undo::{Idle, Inverted, Undoing};
    /// fn apply<'s>(undoing: Undoing<'s>, inverted: Inverted) -> Idle<'s> {
    ///     undoing.finish(inverted)
    /// }
    /// ```
    ///
    /// ```
    /// use hako::fsops::undo::{HistoryAny, Inverted, Undoing};
    /// fn apply<'s>(undoing: Undoing<'s>, inverted: Inverted) -> HistoryAny<'s> {
    ///     match undoing.check(inverted) {
    ///         Ok(fresh) => undoing.finish(fresh).into(),
    ///         Err(_stale) => undoing.into(),
    ///     }
    /// }
    /// ```
    pub fn finish(self, fresh: Fresh) -> Idle<'s> {
        let Fresh { current, inverted } = fresh;
        let InFlight { ticket, step, at, .. } = self.data;
        debug_assert!(current.ticket() == ticket && inverted.ticket == ticket);
        put_back(&mut self.ctx.undo, at, inverted.left, &step.label);
        if let Some(record) = inverted.inverse {
            self.ctx.redo.push(Step { record, label: step.label });
        }
        Idle::enter(self.ctx, ())
    }
}

impl<'s> Redoing<'s> {
    /// Something was done meanwhile: it can be undone, and nothing undone
    /// before can be redone any more, what's left of this redo included.
    pub fn push(mut self, step: Step) -> Redoing<'s> {
        self.ctx.push_undo(step);
        self.ctx.redo.clear();
        self.data.forgotten = true;
        self
    }

    /// The proof that `inverted` is this redo's result.
    pub fn check(&self, inverted: Inverted) -> Result<Fresh, Inverted> {
        check(self.ctx, &self.data, inverted)
    }

    /// The redo's result: what it redid can be undone again, and what it
    /// didn't stays to redo (unless something done since forgot it).
    pub fn finish(self, fresh: Fresh) -> Idle<'s> {
        let Fresh { current, inverted } = fresh;
        let InFlight { ticket, step, at, forgotten } = self.data;
        debug_assert!(current.ticket() == ticket && inverted.ticket == ticket);
        if !forgotten {
            put_back(&mut self.ctx.redo, at, inverted.left, &step.label);
        }
        if let Some(record) = inverted.inverse {
            self.ctx.push_undo(Step { record, label: step.label });
        }
        Idle::enter(self.ctx, ())
    }
}

impl History {
    /// Something was done: it can be undone, and what was undone before
    /// can't be redone any more.
    pub fn push(&mut self, step: Step) {
        if step.record.is_empty() {
            return;
        }
        self.with_typed(|s| {
            let next = match s {
                HistoryAny::Idle(s) => s.push(step).into(),
                HistoryAny::Undoing(s) => s.push(step).into(),
                HistoryAny::Redoing(s) => s.push(step).into(),
            };
            (next, ())
        });
    }

    /// What Undo would undo, unless an undo or redo is being carried out.
    pub fn undo_label(&self) -> Option<&str> {
        match self.stored() {
            HistoryState::Idle(()) => self.ctx().undo.last().map(|s| s.label.as_str()),
            _ => None,
        }
    }

    /// What Redo would redo, unless an undo or redo is being carried out.
    pub fn redo_label(&self) -> Option<&str> {
        match self.stored() {
            HistoryState::Idle(()) => self.ctx().redo.last().map(|s| s.label.as_str()),
            _ => None,
        }
    }

    /// Begin undoing the last step, unless an undo or redo is running.
    pub fn begin_undo(&mut self) -> Option<Inversion> {
        self.with_typed(|s| match s {
            HistoryAny::Idle(s) => match s.undo() {
                Ok((undoing, inversion)) => (undoing.into(), Some(inversion)),
                Err(idle) => (idle.into(), None),
            },
            busy => (busy, None),
        })
    }

    /// Begin redoing the last step undone, unless an undo or redo is
    /// running.
    pub fn begin_redo(&mut self) -> Option<Inversion> {
        self.with_typed(|s| match s {
            HistoryAny::Idle(s) => match s.redo() {
                Ok((redoing, inversion)) => (redoing.into(), Some(inversion)),
                Err(idle) => (idle.into(), None),
            },
            busy => (busy, None),
        })
    }

    /// An undo's or redo's result came back: filed if it's the one in
    /// flight, else ignored.
    pub fn finished(&mut self, inverted: Inverted) {
        self.with_typed(|s| {
            let next = match s {
                HistoryAny::Undoing(s) => match s.check(inverted) {
                    Ok(fresh) => s.finish(fresh).into(),
                    Err(_) => s.into(),
                },
                HistoryAny::Redoing(s) => match s.check(inverted) {
                    Ok(fresh) => s.finish(fresh).into(),
                    Err(_) => s.into(),
                },
                idle => idle,
            };
            (next, ())
        });
    }
}

/// Carry out `inversion`: what happened, and its result for the history.
pub fn invert(inversion: Inversion, trash: &Trash, env: &mut Env) -> (Report, Inverted) {
    let Inversion { ticket, record, .. } = inversion;
    let (report, inverse, left) = carry_out(&record, trash, env);
    let some = |r: Record| (!r.is_empty()).then_some(r);
    (report, Inverted { ticket, inverse: some(inverse), left: some(left) })
}

/// No items.
fn nothing() -> Record {
    Record::Made { paths: Vec::new() }
}

/// Carry out the inverse of `record`: what happened, the record of the
/// inverse (to redo, or undo again), and what's left of `record`: the
/// part not carried out whose items are still there to try again with.
fn carry_out(record: &Record, trash: &Trash, env: &mut Env) -> (Report, Record, Record) {
    let there = |p: &PathBuf| std::fs::symlink_metadata(p).is_ok();
    match record {
        Record::Rename { from, to } => {
            let mut report = Report::default();
            if !there(to) {
                report.failed.push((to.clone(), "it isn't there any more".into()));
                return (report, nothing(), nothing());
            }
            match sys::rename_noreplace(to, from) {
                Ok(()) => {
                    report.done.push((to.clone(), from.clone()));
                    (report, Record::Rename { from: to.clone(), to: from.clone() }, nothing())
                }
                Err(e) => {
                    let why = if e.kind() == std::io::ErrorKind::AlreadyExists {
                        format!("“{}” is taken now", from.file_name().unwrap_or_default().to_string_lossy())
                    } else {
                        super::describe(&e)
                    };
                    report.failed.push((to.clone(), why));
                    (report, nothing(), record.clone())
                }
            }
        }
        Record::Move { pairs } => {
            let items: Vec<Item> =
                pairs.iter().map(|(was, now)| Item { source: now.clone(), target: was.clone() }).collect();
            let report = transfer::transfer_items(&items, Mode::Move, env, trash);
            let back = report.done.iter().map(|(now, was)| (now.clone(), was.clone())).collect();
            let moved = |now: &PathBuf| report.done.iter().any(|(s, _)| s == now);
            let left = pairs.iter().filter(|(_, now)| !moved(now) && there(now)).cloned().collect();
            (report, Record::Move { pairs: back }, Record::Move { pairs: left })
        }
        Record::Made { paths } => {
            let (report, items) = transfer::trash_items(paths, trash, env);
            let trashed = |p: &PathBuf| report.done.iter().any(|(s, _)| s == p);
            let left = paths.iter().filter(|p| !trashed(p) && there(p)).cloned().collect();
            (report, Record::Trash { items }, Record::Made { paths: left })
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
            // Skipped, stopped, or put back in part: still in the Trash.
            let left = items.iter().filter(|t| trash.entry_for(&t.file).is_some()).cloned().collect();
            (report, Record::Made { paths }, Record::Trash { items: left })
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

    /// Carry out `inversion` as a job would, answering Skip.
    fn run(inversion: Inversion, tr: &Trash) -> (Report, Inverted) {
        let control = Control::new();
        let mut asker = Always(Resolution::Skip, ErrorChoice::Skip);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        invert(inversion, tr, &mut env)
    }

    fn undo(h: &mut History, tr: &Trash) -> Report {
        let (report, inverted) = run(h.begin_undo().expect("something to undo"), tr);
        h.finished(inverted);
        report
    }

    fn redo(h: &mut History, tr: &Trash) -> Report {
        let (report, inverted) = run(h.begin_redo().expect("something to redo"), tr);
        h.finished(inverted);
        report
    }

    fn step(record: Record, label: &str) -> Step {
        Step { record, label: label.into() }
    }

    #[test]
    fn renames_undo_and_redo() {
        let t = TempDir::new("undo-rename");
        let tr = trash(&t);
        let a = t.file("a.txt", "a");
        let b = t.join("b.txt");
        std::fs::rename(&a, &b).unwrap();
        let mut h = History::default();
        h.push(step(Record::Rename { from: a.clone(), to: b.clone() }, "Rename"));
        assert!(undo(&mut h, &tr).is_clean());
        assert_eq!(read(&a), "a");
        assert_eq!((h.undo_label(), h.redo_label()), (None, Some("Rename")));
        assert!(redo(&mut h, &tr).is_clean());
        assert_eq!(read(&b), "a");
        // A name taken since isn't overwritten, and the step stays.
        t.file("a.txt", "newer");
        let r = undo(&mut h, &tr);
        assert_eq!(r.failed.len(), 1);
        assert_eq!(read(&a), "newer");
        assert_eq!(read(&b), "a");
        assert_eq!((h.undo_label(), h.redo_label()), (Some("Rename"), None));
        // Once the name is free again, it undoes.
        std::fs::remove_file(&a).unwrap();
        assert!(undo(&mut h, &tr).is_clean());
        assert_eq!(read(&a), "a");
    }

    #[test]
    fn moves_go_back() {
        let t = TempDir::new("undo-move");
        let tr = trash(&t);
        let a = t.file("from/a.txt", "a");
        let to = t.dir("to").join("a.txt");
        std::fs::rename(&a, &to).unwrap();
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(a.clone(), to.clone())] }, "Move"));
        let r = undo(&mut h, &tr);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(read(&a), "a");
        assert!(!to.exists());
        assert!(redo(&mut h, &tr).is_clean());
        assert_eq!(read(&to), "a");
        assert_eq!(h.undo_label(), Some("Move"));
    }

    #[test]
    fn a_move_undone_in_part_keeps_the_rest() {
        let t = TempDir::new("undo-move-part");
        let tr = trash(&t);
        let (a, b) = (t.file("from/a.txt", "a"), t.file("from/b.txt", "b"));
        let to = t.dir("to");
        std::fs::rename(&a, to.join("a.txt")).unwrap();
        std::fs::rename(&b, to.join("b.txt")).unwrap();
        // b's old name is taken now: the undo skips it.
        t.file("from/b.txt", "newer b");
        let mut h = History::default();
        let pairs = vec![(a.clone(), to.join("a.txt")), (b.clone(), to.join("b.txt"))];
        h.push(step(Record::Move { pairs }, "Move of 2 Items"));
        let r = undo(&mut h, &tr);
        assert_eq!(r.skipped, vec![to.join("b.txt")]);
        assert_eq!(read(&a), "a");
        assert_eq!((h.undo_label(), h.redo_label()), (Some("Move of 2 Items"), Some("Move of 2 Items")));
        let inversion = h.begin_undo().unwrap();
        assert_eq!(inversion.record, Record::Move { pairs: vec![(b.clone(), to.join("b.txt"))] }, "what's left");
    }

    #[test]
    fn made_items_go_to_the_trash_and_come_back() {
        let t = TempDir::new("undo-made");
        let tr = trash(&t);
        let made = t.file("home/copy.txt", "c");
        let mut h = History::default();
        h.push(step(Record::Made { paths: vec![made.clone()] }, "Copy"));
        assert!(undo(&mut h, &tr).is_clean());
        assert!(!made.exists());
        let r = redo(&mut h, &tr);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(read(&made), "c");
        assert_eq!(h.begin_undo().unwrap().record, Record::Made { paths: vec![made] });
    }

    #[test]
    fn the_history_keeps_order() {
        let mut h = History::default();
        let made = |n: &str| step(Record::Made { paths: vec![PathBuf::from(n)] }, n);
        h.push(made("one"));
        h.push(made("two"));
        assert_eq!(h.undo_label(), Some("two"));
        h.push(Step { record: Record::Made { paths: vec![] }, label: "nothing".into() });
        assert_eq!(h.undo_label(), Some("two"), "nothing done is nothing to undo");
        assert_eq!(items_label(&[PathBuf::from("/x/notes.txt")]), "“notes.txt”");
        assert_eq!(items_label(&[PathBuf::from("a"), PathBuf::from("b")]), "2 Items");
    }

    #[test]
    fn a_step_in_flight_stays_in_the_history() {
        let t = TempDir::new("undo-in-flight");
        let tr = trash(&t);
        let mut h = History::default();
        let (a, b) = (t.file("a.txt", "a"), t.join("b.txt"));
        std::fs::rename(&a, &b).unwrap();
        h.push(step(Record::Made { paths: vec![t.join("old")] }, "Older"));
        h.push(step(Record::Rename { from: a.clone(), to: b.clone() }, "Rename"));
        let inversion = h.begin_undo().unwrap();
        // One at a time: nothing else to undo or redo meanwhile.
        assert_eq!((h.undo_label(), h.redo_label()), (None, None));
        assert!(h.begin_undo().is_none() && h.begin_redo().is_none());
        // Something finishes meanwhile; then the undo fails (the name is
        // taken). The step keeps its place, under what was done since.
        t.file("a.txt", "newer");
        h.push(step(Record::Made { paths: vec![t.join("new")] }, "Newer"));
        let (r, inverted) = run(inversion, &tr);
        assert_eq!(r.failed.len(), 1);
        h.finished(inverted.clone());
        assert_eq!(h.undo_label(), Some("Newer"));
        // A result that isn't the one in flight changes nothing.
        h.finished(inverted);
        assert_eq!(h.undo_label(), Some("Newer"));
        let _newer = h.begin_undo().unwrap();
        assert!(h.begin_redo().is_none(), "no redo while an undo runs");
    }

    #[test]
    fn redone_steps_keep_the_depth() {
        let t = TempDir::new("undo-depth");
        let tr = trash(&t);
        let mut h = History::default();
        let made = t.file("home/made.txt", "m");
        h.push(step(Record::Made { paths: vec![made.clone()] }, "Made"));
        assert!(undo(&mut h, &tr).is_clean());
        // A full stack can't take the redo yet...
        let mut full = Vec::new();
        for i in 0..DEPTH {
            full.push(step(Record::Made { paths: vec![PathBuf::from(format!("/nowhere/{i}"))] }, "filler"));
        }
        h.with_typed(|s| match s {
            HistoryAny::Idle(s) => {
                s.ctx.undo = full;
                (s.into(), ())
            }
            other => (other, ()),
        });
        assert!(redo(&mut h, &tr).is_clean());
        assert_eq!(h.ctx().undo.len(), DEPTH, "the oldest step makes room");
        assert_eq!(h.undo_label(), Some("Made"));
    }
}
