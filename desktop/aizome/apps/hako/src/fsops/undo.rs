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

// Typestate code: every state and every case is named, so a new one
// added later can't fall into an arm written for others.
#![deny(clippy::wildcard_enum_match_arm)]

use std::path::PathBuf;

use aizome_typestate::{Answer, Fresh, Generation, Ticket};

use super::transfer::{self, Item, KeptDir, RemovedDir};
use super::trash::{Trash, Trashed};
use super::{Env, Report, sys};

/// What an operation did, to undo.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    /// `from` was renamed `to`.
    Rename { from: PathBuf, to: PathBuf },
    /// Each item moved from the first path to the second.
    Move { pairs: Vec<(PathBuf, PathBuf)> },
    /// A move inverted through merges: only the children actually moved,
    /// plus empty containers removed or recreated along the way.
    MoveParts(Box<MoveParts>),
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
            // Merge parents are best-effort cleanup alongside pending
            // children, not another undo action once those children are done.
            Record::MoveParts(parts) => parts.pairs.len() + parts.removed.len() + parts.created.len(),
            Record::Made { paths } => paths.len(),
            Record::Trash { items } => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Precise inverse of a merged move. Its fields are private because the
/// transfer worker, not a top-level progress report, supplies its receipts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveParts {
    pairs: Vec<(PathBuf, PathBuf)>,
    removed: Vec<RemovedDir>,
    created: Vec<KeptDir>,
    /// Source containers to remove if the remaining children leave them
    /// empty. These never authorize moving their other children.
    merge_parents: Vec<KeptDir>,
}

impl MoveParts {
    fn record(self) -> Record {
        if self.removed.is_empty() && self.created.is_empty() && self.merge_parents.is_empty() {
            Record::Move { pairs: self.pairs }
        } else {
            Record::MoveParts(Box::new(self))
        }
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
        [] | [_, _, ..] => format!("{} Items", paths.len()),
    }
}

/// How many steps are kept.
const DEPTH: usize = 64;

/// Names the undos and redos the history hands out, for their
/// [`Ticket`]s.
pub enum Inverting {}

/// A step in a stack, and whether the last try to undo (or redo) it
/// carried out nothing.
#[derive(Debug)]
struct Held {
    step: Step,
    failed: bool,
}

/// The undo and redo stacks: what the history's states share.
#[derive(Debug, Default)]
struct Stacks {
    undo: Vec<Held>,
    redo: Vec<Held>,
    inversions: Generation<Inverting>,
}

/// Put `held` in `stack` at `at` (its top, if `at` is past it), within
/// the depth: whether the oldest step went to make room.
fn put(stack: &mut Vec<Held>, at: usize, held: Held) -> bool {
    stack.insert(at.min(stack.len()), held);
    let full = stack.len() > DEPTH;
    if full {
        stack.remove(0);
    }
    full
}

/// The step being undone or redone. It stays in the history, here, until
/// its result comes back: only that takes it out.
#[derive(Debug)]
struct InFlight {
    held: Held,
    /// Its place in the stack it came from, where what isn't carried out
    /// goes back.
    at: usize,
    /// A new action since made what this would leave to redo impossible
    /// to redo (an undo's inverse, or what's left of a redo).
    forgotten: bool,
}

aizome_typestate::machine! {
    /// The undo and redo stacks, and the undo or redo being carried out
    /// (one at a time). A step leaves the history only through the result
    /// of its inversion ([`Undoing::finish`]): what was carried out can be
    /// redone (or undone again), and what wasn't stays where it was, unless
    /// a second try carried out nothing either, when it's left out and the
    /// person told ([`Filed::Dropped`]).
    #[derive(Debug)]
    pub struct History over Stacks {
        #[derive(Debug)]
        stored HistoryState;
        /// The history's state, typed.
        typed HistoryAny;
        states {
            /// Nothing is being undone or redone. Only the end of an undo or
            /// redo comes back here; giving one up can't:
            ///
            /// ```compile_fail,E0624
            /// use hako::fsops::undo::{Idle, Undoing};
            /// // Back to idle without the undo's result: its step would be lost.
            /// fn give_up<'s>(undoing: Undoing<'s>) -> Idle<'s> {
            ///     Idle::enter(undoing.ctx, ())
            /// }
            /// ```
            ///
            /// ```
            /// use aizome_typestate::Fresh;
            /// use hako::fsops::undo::{Idle, Inverted, Inverting, Undoing};
            /// fn done<'s>(undoing: Undoing<'s>, fresh: Fresh<Inverting, Inverted>) -> Idle<'s> {
            ///     undoing.finish(fresh).0
            /// }
            /// ```
            Idle,
            /// A step being undone.
            Undoing(InFlight),
            /// A step being redone.
            Redoing(InFlight),
        }
        // The stacks and the stored state stay this module's;
        // `History::default()` is the way in.
        access(private);
        new(private);
    }
}

impl Default for History {
    fn default() -> History {
        History::new(Stacks::default())
    }
}

/// What became of a step when its undo or redo came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filed {
    /// All or part of it was carried out; what's left stays to try again.
    Done,
    /// Nothing of it was carried out: it stays to try again.
    Kept,
    /// Nothing of it was carried out a second time in a row: it's left out
    /// of the history, so the steps before it can be reached. What to tell
    /// the person.
    Dropped(String),
    /// It wasn't the undo or redo in flight: nothing changed.
    Stale,
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

    /// The result of this undo or redo if it never ran, or stopped without
    /// a result (its job couldn't start, or panicked): nothing carried out,
    /// all of it left. The history keeps its step. (Only a result that
    /// keeps all of the step can be made this way.)
    pub fn abandoned(&self) -> Inverted {
        Inverted { ticket: self.ticket, inverse: None, left: Some(self.record.clone()), declined: false }
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
///     Inverted { ticket, inverse: None, left: None, declined: false }
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
    /// What wasn't carried out, the person chose to leave (Stop, or Skip
    /// at a conflict), and nothing failed.
    declined: bool,
}

impl Answer<Inverting> for Inverted {
    fn ticket(&self) -> Ticket<Inverting> {
        self.ticket
    }
}

impl<'s> Idle<'s> {
    /// Something was done: it can be undone, and what was undone before
    /// can't be redone any more.
    pub fn push(self, step: Step) -> Idle<'s> {
        let at = self.ctx.undo.len();
        put(&mut self.ctx.undo, at, Held { step, failed: false });
        self.ctx.redo.clear();
        self
    }

    /// Begin undoing the last step. It stays in the history (in
    /// [`Undoing`]) until its result comes back; the job gets a copy.
    pub fn undo(self) -> Result<(Undoing<'s>, Inversion), Idle<'s>> {
        let Some(held) = self.ctx.undo.pop() else { return Err(self) };
        let at = self.ctx.undo.len();
        let inversion = self.ctx.inversion(&held.step, false);
        Ok((Undoing::enter(self.ctx, InFlight { held, at, forgotten: false }), inversion))
    }

    /// Begin redoing the last step undone, the same way.
    pub fn redo(self) -> Result<(Redoing<'s>, Inversion), Idle<'s>> {
        let Some(held) = self.ctx.redo.pop() else { return Err(self) };
        let at = self.ctx.redo.len();
        let inversion = self.ctx.inversion(&held.step, true);
        Ok((Redoing::enter(self.ctx, InFlight { held, at, forgotten: false }), inversion))
    }
}

impl Stacks {
    /// The request to undo or redo `step`, with a fresh ticket.
    fn inversion(&mut self, step: &Step, redo: bool) -> Inversion {
        Inversion { ticket: self.inversions.issue(), record: step.record.clone(), label: step.label.clone(), redo }
    }

    /// What's left of a step, back in `stack` at `at`, or dropped when a
    /// second try carried out nothing either.
    fn refile(stack: &mut Vec<Held>, at: usize, held: Held, inverted: &Inverted, redo: bool) -> Filed {
        let carried = inverted.inverse.is_some();
        // A try the person stopped or skipped isn't a failure.
        let failed = !carried && !inverted.declined;
        match &inverted.left {
            None => Filed::Done,
            Some(_) if failed && held.failed => {
                let (done, menu) = if redo { ("redone", "Redo") } else { ("undone", "Undo") };
                let label = held.step.label;
                Filed::Dropped(format!("“{label}” couldn't be {done} a second time, so it's left out of {menu}."))
            }
            Some(record) => {
                let step = Step { record: record.clone(), label: held.step.label };
                put(stack, at, Held { step, failed });
                if carried { Filed::Done } else { Filed::Kept }
            }
        }
    }
}

impl<'s> Undoing<'s> {
    /// Something was done meanwhile: it goes on top, and the step being
    /// undone keeps its place below it. What the undo carries out can't be
    /// redone after it.
    pub fn push(mut self, step: Step) -> Undoing<'s> {
        let at = self.ctx.undo.len();
        if put(&mut self.ctx.undo, at, Held { step, failed: false }) {
            self.data.at = self.data.at.saturating_sub(1);
        }
        self.ctx.redo.clear();
        self.data.forgotten = true;
        self
    }

    /// The proof that `inverted` is this undo's result.
    pub fn check(&self, inverted: Inverted) -> Result<Fresh<Inverting, Inverted>, Inverted> {
        self.ctx.inversions.check_answer(inverted)
    }

    /// The undo's result: what it undid can be redone, and what it didn't
    /// stays to undo, where it was. Only a result checked fresh will do:
    ///
    /// ```compile_fail,E0308
    /// use hako::fsops::undo::{Filed, Idle, Inverted, Undoing};
    /// fn apply<'s>(undoing: Undoing<'s>, inverted: Inverted) -> (Idle<'s>, Filed) {
    ///     undoing.finish(inverted)
    /// }
    /// ```
    ///
    /// ```
    /// use hako::fsops::undo::{HistoryAny, Inverted, Undoing};
    /// fn apply<'s>(undoing: Undoing<'s>, inverted: Inverted) -> HistoryAny<'s> {
    ///     match undoing.check(inverted) {
    ///         Ok(fresh) => undoing.finish(fresh).0.into(),
    ///         Err(_stale) => undoing.into(),
    ///     }
    /// }
    /// ```
    pub fn finish(self, fresh: Fresh<Inverting, Inverted>) -> (Idle<'s>, Filed) {
        let inverted = fresh.into_answer();
        // Its ticket is spent: a copy of this result can't be filed again.
        self.ctx.inversions.retire();
        let InFlight { held, at, forgotten } = self.data;
        let label = held.step.label.clone();
        let filed = Stacks::refile(&mut self.ctx.undo, at, held, &inverted, false);
        if let Some(record) = inverted.inverse
            && !forgotten
        {
            self.ctx.redo.push(Held { step: Step { record, label }, failed: false });
        }
        (Idle::enter(self.ctx, ()), filed)
    }
}

impl<'s> Redoing<'s> {
    /// Something was done meanwhile: it can be undone, and nothing undone
    /// before can be redone any more, what's left of this redo included.
    pub fn push(mut self, step: Step) -> Redoing<'s> {
        let at = self.ctx.undo.len();
        put(&mut self.ctx.undo, at, Held { step, failed: false });
        self.ctx.redo.clear();
        self.data.forgotten = true;
        self
    }

    /// The proof that `inverted` is this redo's result.
    pub fn check(&self, inverted: Inverted) -> Result<Fresh<Inverting, Inverted>, Inverted> {
        self.ctx.inversions.check_answer(inverted)
    }

    /// The redo's result: what it redid can be undone again, and what it
    /// didn't stays to redo (unless something done since forgot it).
    pub fn finish(self, fresh: Fresh<Inverting, Inverted>) -> (Idle<'s>, Filed) {
        let inverted = fresh.into_answer();
        self.ctx.inversions.retire();
        let InFlight { held, at, forgotten } = self.data;
        let label = held.step.label.clone();
        // Something done since forgot what this would leave to redo.
        let filed = if forgotten { Filed::Done } else { Stacks::refile(&mut self.ctx.redo, at, held, &inverted, true) };
        if let Some(record) = inverted.inverse {
            let at = self.ctx.undo.len();
            put(&mut self.ctx.undo, at, Held { step: Step { record, label }, failed: false });
        }
        (Idle::enter(self.ctx, ()), filed)
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
            HistoryState::Idle(()) => self.ctx().undo.last().map(|h| h.step.label.as_str()),
            HistoryState::Undoing(_) | HistoryState::Redoing(_) => None,
        }
    }

    /// What Redo would redo, unless an undo or redo is being carried out.
    pub fn redo_label(&self) -> Option<&str> {
        match self.stored() {
            HistoryState::Idle(()) => self.ctx().redo.last().map(|h| h.step.label.as_str()),
            HistoryState::Undoing(_) | HistoryState::Redoing(_) => None,
        }
    }

    /// Begin undoing the last step, unless an undo or redo is running.
    pub fn begin_undo(&mut self) -> Option<Inversion> {
        self.with_typed(|s| match s {
            HistoryAny::Idle(s) => match s.undo() {
                Ok((undoing, inversion)) => (undoing.into(), Some(inversion)),
                Err(idle) => (idle.into(), None),
            },
            busy @ (HistoryAny::Undoing(_) | HistoryAny::Redoing(_)) => (busy, None),
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
            busy @ (HistoryAny::Undoing(_) | HistoryAny::Redoing(_)) => (busy, None),
        })
    }

    /// An undo's or redo's result came back: filed if it's the one in
    /// flight, else ignored. When that leaves its step out of the history,
    /// `tell` gets what to say to the person. Filing a result without
    /// somewhere to say it doesn't compile, so no window can leave a step
    /// out unannounced, and have the next ❖Z undo the one before it:
    ///
    /// ```compile_fail,E0061
    /// use hako::fsops::undo::{History, Inverted};
    /// fn job_done(history: &mut History, inverted: Inverted) {
    ///     history.finished(inverted);
    /// }
    /// ```
    ///
    /// ```
    /// use hako::fsops::undo::{History, Inverted};
    /// fn job_done(history: &mut History, inverted: Inverted, notes: &mut Vec<String>) {
    ///     history.finished(inverted, |note| notes.push(note));
    /// }
    /// ```
    pub fn finished(&mut self, inverted: Inverted, tell: impl FnOnce(String)) -> Filed {
        let filed = self.file(inverted);
        if let Filed::Dropped(note) = &filed {
            tell(note.clone());
        }
        filed
    }

    fn file(&mut self, inverted: Inverted) -> Filed {
        self.with_typed(|s| match s {
            HistoryAny::Undoing(s) => match s.check(inverted) {
                Ok(fresh) => {
                    let (idle, filed) = s.finish(fresh);
                    (idle.into(), filed)
                }
                Err(_) => (s.into(), Filed::Stale),
            },
            HistoryAny::Redoing(s) => match s.check(inverted) {
                Ok(fresh) => {
                    let (idle, filed) = s.finish(fresh);
                    (idle.into(), filed)
                }
                Err(_) => (s.into(), Filed::Stale),
            },
            idle @ HistoryAny::Idle(_) => (idle, Filed::Stale),
        })
    }
}

/// Carry out `inversion`: what happened, and its result for the history.
pub fn invert(inversion: Inversion, trash: &Trash, env: &mut Env) -> (Report, Inverted) {
    let Inversion { ticket, record, .. } = inversion;
    let (report, inverse, left, skipped_inside) = carry_out(&record, trash, env);
    let some = |r: Record| (!r.is_empty()).then_some(r);
    let declined = report.failed.is_empty()
        && report.problems.is_empty()
        && (skipped_inside || report.cancelled || !report.skipped.is_empty());
    (report, Inverted { ticket, inverse: some(inverse), left: some(left), declined })
}

/// No items.
fn nothing() -> Record {
    Record::Made { paths: Vec::new() }
}

/// Carry out the inverse of `record`: what happened, the record of the
/// inverse (to redo, or undo again), and what's left of `record`: the
/// part not carried out whose items are still there to try again with.
fn carry_out(record: &Record, trash: &Trash, env: &mut Env) -> (Report, Record, Record, bool) {
    // An inaccessible item may still be there. Only definite absence
    // removes it from a step's retry record.
    let there = |p: &PathBuf| !std::fs::symlink_metadata(p).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
    match record {
        Record::Rename { from, to } => {
            let mut report = Report::default();
            if let Err(e) = std::fs::symlink_metadata(to) {
                let (why, left) = if e.kind() == std::io::ErrorKind::NotFound {
                    ("it isn't there any more".into(), nothing())
                } else {
                    (super::describe(&e), record.clone())
                };
                report.failed.push((to.clone(), why));
                return (report, nothing(), left, false);
            }
            match sys::rename_noreplace(to, from) {
                Ok(()) => {
                    report.done.push((to.clone(), from.clone()));
                    (report, Record::Rename { from: to.clone(), to: from.clone() }, nothing(), false)
                }
                Err(e) => {
                    let why = if e.kind() == std::io::ErrorKind::AlreadyExists {
                        format!("“{}” is taken now", from.file_name().unwrap_or_default().to_string_lossy())
                    } else {
                        super::describe(&e)
                    };
                    report.failed.push((to.clone(), why));
                    (report, nothing(), record.clone(), false)
                }
            }
        }
        Record::Move { pairs } => invert_move(&MoveParts { pairs: pairs.clone(), ..MoveParts::default() }, trash, env),
        Record::MoveParts(parts) => invert_move(parts, trash, env),
        Record::Made { paths } => {
            let (report, items) = transfer::trash_items(paths, trash, env);
            let trashed = |p: &PathBuf| report.done.iter().any(|(s, _)| s == p);
            let left = paths.iter().filter(|p| !trashed(p) && there(p)).cloned().collect();
            (report, Record::Trash { items }, Record::Made { paths: left }, false)
        }
        Record::Trash { items } => {
            let mut report = Report::default();
            let mut entries = Vec::new();
            for t in items {
                match t.entry(trash) {
                    Some(e) => entries.push(e),
                    None => report.failed.push((t.original().to_path_buf(), "it isn't in the Trash any more".into())),
                }
            }
            report.absorb(transfer::restore(&entries, trash, env));
            let paths = report.done.iter().map(|(_, to)| to.clone()).collect();
            // Only the original receipt can keep a step. A later replacement
            // may already have reused a name freed by this very restore.
            let left = items.iter().filter(|t| t.may_remain()).cloned().collect();
            (report, Record::Made { paths }, Record::Trash { items: left }, false)
        }
    }
}

fn invert_move(parts: &MoveParts, trash: &Trash, env: &mut Env) -> (Report, Record, Record, bool) {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

    let mut report = Report::default();
    let mut inverse = MoveParts::default();
    let mut left = MoveParts::default();
    let mut restored = Vec::new();
    let mut targets = Vec::new();
    let mut blocked = Vec::new();
    let mut dirs = parts.removed.clone();
    dirs.sort_by_key(|d| d.path.components().count());
    for dir in dirs {
        if blocked.iter().any(|parent: &PathBuf| dir.path.starts_with(parent)) {
            left.removed.push(dir);
            continue;
        }
        if env.control.checkpoint().is_err() {
            report.cancelled = true;
            left.removed.push(dir);
            continue;
        }
        // Create just the containers this move removed, never recursively
        // invent a parent outside its receipt. Fill them before restoring
        // their permissions (a removed folder may have been read-only).
        match fs::DirBuilder::new().mode(0o700).create(&dir.path) {
            Ok(()) => {
                match fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                    .open(&dir.path)
                    .and_then(|handle| handle.metadata().map(|meta| (handle, meta)))
                {
                    Ok((handle, meta)) => {
                        let kept = KeptDir::at(&dir.path, &meta);
                        targets.push(kept.clone());
                        inverse.created.push(kept.clone());
                        restored.push((dir, handle));
                    }
                    Err(e) => {
                        report.failed.push((
                            dir.path.clone(),
                            format!("its new folder couldn't be recorded: {}", super::describe(&e)),
                        ));
                        blocked.push(dir.path.clone());
                        left.removed.push(dir);
                    }
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&dir.path).is_ok_and(|m| m.is_dir()) =>
            {
                if let Ok(meta) = fs::symlink_metadata(&dir.path) {
                    targets.push(KeptDir::at(&dir.path, &meta));
                }
            }
            Err(e) => {
                report
                    .failed
                    .push((dir.path.clone(), format!("its folder couldn't be made again: {}", super::describe(&e))));
                blocked.push(dir.path.clone());
                left.removed.push(dir);
            }
        }
    }
    let items: Vec<Item> = parts
        .pairs
        .iter()
        .filter_map(|(was, now)| {
            if blocked.iter().any(|parent| was.starts_with(parent)) {
                left.pairs.push((was.clone(), now.clone()));
                None
            } else {
                Some(Item { source: now.clone(), target: was.clone() })
            }
        })
        .collect();
    let sources: Vec<KeptDir> = parts.created.iter().chain(&parts.merge_parents).cloned().collect();
    let (moved_report, moves) = transfer::move_items(&items, env, trash, &sources, &targets);
    report.absorb(moved_report);
    inverse.pairs = moves.moved.into_iter().map(|i| (i.source, i.target)).collect();
    inverse.removed = moves.removed;
    left.pairs.extend(moves.pending.into_iter().map(|i| (i.target, i.source)));

    let mut cleanup: Vec<(KeptDir, bool)> = parts
        .created
        .iter()
        .cloned()
        .map(|p| (p, true))
        .chain(parts.merge_parents.iter().chain(&moves.merge_parents).cloned().map(|p| (p, false)))
        .collect();
    cleanup.sort_by_key(|(dir, _)| (std::cmp::Reverse(dir.path.components().count()), dir.path.clone()));
    cleanup.dedup_by(|a, b| a.0 == b.0);
    for (dir, created) in cleanup {
        let path = &dir.path;
        let pending = left.pairs.iter().any(|(_, source)| source.starts_with(path))
            || left.created.iter().chain(&left.merge_parents).any(|d| d.path.starts_with(path));
        if pending || env.control.is_cancelled() {
            if created {
                left.created.push(dir);
            } else {
                left.merge_parents.push(dir);
            }
            continue;
        }
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) if dir.matches(&meta) => meta,
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                report.problems.push((path.clone(), super::describe(&e)));
                if created {
                    left.created.push(dir);
                }
                continue;
            }
        };
        match fs::remove_dir(path) {
            Ok(()) => inverse.removed.push(RemovedDir { path: path.clone(), mode: meta.mode() & 0o7777 }),
            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty) => {}
            Err(e) => {
                report
                    .problems
                    .push((path.clone(), format!("the empty original folder is kept: {}", super::describe(&e))));
                if created {
                    left.created.push(dir);
                }
            }
        }
    }
    for (dir, handle) in restored.into_iter().rev() {
        if let Err(e) = handle.set_permissions(fs::Permissions::from_mode(dir.mode)) {
            report.problems.push((dir.path, format!("its permissions couldn't be restored: {}", super::describe(&e))));
        }
    }
    (report, inverse.record(), left.record(), moves.skipped)
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
        h.finished(inverted, |_| ());
        report
    }

    fn redo(h: &mut History, tr: &Trash) -> Report {
        let (report, inverted) = run(h.begin_redo().expect("something to redo"), tr);
        h.finished(inverted, |_| ());
        report
    }

    fn step(record: Record, label: &str) -> Step {
        Step { record, label: label.into() }
    }

    #[test]
    fn inaccessible_items_keep_their_undo_until_access_returns() {
        use std::os::unix::fs::PermissionsExt;
        if crate::fsops::testing::is_root() {
            return;
        }
        for kind in ["Rename", "Move", "Copy", "Trash"] {
            let t = TempDir::new("undo-inaccessible");
            let tr = trash(&t);
            let b = t.file("home/locked/b.txt", "b");
            let a = t.join("home/locked/a.txt");
            let (record, locked, probe) = match kind {
                "Rename" => (Record::Rename { from: a, to: b.clone() }, t.join("home/locked"), b.clone()),
                "Move" => (Record::Move { pairs: vec![(a, b.clone())] }, t.join("home/locked"), b.clone()),
                "Copy" => (Record::Made { paths: vec![b.clone()] }, t.join("home/locked"), b.clone()),
                "Trash" => {
                    let item = tr.trash(&b).unwrap();
                    let info = item.info().to_path_buf();
                    (Record::Trash { items: vec![item] }, info.parent().unwrap().to_path_buf(), info)
                }
                _ => unreachable!(),
            };
            let mut h = History::default();
            h.push(step(Record::Made { paths: vec![t.file("previous.txt", "previous")] }, "Previous copy"));
            h.push(step(record, kind));
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            let error = std::fs::symlink_metadata(&probe).unwrap_err();
            let (report, inverted) = run(h.begin_undo().unwrap(), &tr);
            let filed = h.finished(inverted, |_| ());
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
            assert!(!report.failed.is_empty(), "{kind}: {report:?}");
            assert_eq!(filed, Filed::Kept, "{kind}");
            assert_eq!(h.undo_label(), Some(kind));
            assert!(t.join("previous.txt").exists());
            assert!(undo(&mut h, &tr).is_clean(), "{kind}: retry succeeds");
            assert_eq!(h.undo_label(), Some("Previous copy"));
        }
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

    struct MergeAnswer(Resolution);

    impl super::super::Asker for MergeAnswer {
        fn conflict(&mut self, c: &super::super::Conflict) -> Resolution {
            if c.can_merge() { Resolution::Merge } else { self.0 }
        }

        fn error(&mut self, _: &super::super::Failure) -> ErrorChoice {
            ErrorChoice::Skip
        }
    }

    fn invert_merging(h: &mut History, tr: &Trash, redo: bool, answer: Resolution) -> Report {
        let inversion = if redo { h.begin_redo() } else { h.begin_undo() }.expect("a step");
        let control = Control::new();
        let mut asker = MergeAnswer(answer);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        let (report, inverted) = invert(inversion, tr, &mut env);
        h.finished(inverted, |note| panic!("a choice isn't a failed step: {note}"));
        report
    }

    #[test]
    fn a_partial_move_merge_retries_and_redoes_only_its_children() {
        let t = TempDir::new("undo-partial-move-merge");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        t.file("to/folder/nested/a", "original a");
        t.file("to/folder/nested/b", "original b");
        t.file("from/folder/nested/b", "unrelated b");
        t.file("from/folder/only-here", "unrelated child");
        let previous = t.file("previous", "previous step");
        let mut h = History::default();
        h.push(step(Record::Made { paths: vec![previous.clone()] }, "Previous"));
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move folder"));

        invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert_eq!(h.undo_label(), Some("Move folder"), "the skipped child stays to undo");
        assert_eq!(read(old.join("nested/a")), "original a");
        assert_eq!(read(now.join("nested/b")), "original b");
        // Repeatedly skipping the remaining child must not discard it, nor
        // accidentally include new source children in the retry.
        t.file("to/folder/later", "not part of the move");
        for _ in 0..2 {
            invert_merging(&mut h, &tr, false, Resolution::Skip);
            assert_eq!(h.undo_label(), Some("Move folder"));
            assert!(now.join("later").exists());
            assert!(previous.exists());
        }
        invert_merging(&mut h, &tr, true, Resolution::Skip);
        assert_eq!(read(now.join("nested/a")), "original a");
        assert_eq!(read(old.join("nested/b")), "unrelated b");
        assert_eq!(read(old.join("only-here")), "unrelated child");
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        std::fs::remove_file(old.join("nested/b")).unwrap();
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert_eq!(h.undo_label(), Some("Previous"));
        assert_eq!(read(old.join("nested/b")), "original b");
        assert_eq!(read(now.join("later")), "not part of the move");
        invert_merging(&mut h, &tr, true, Resolution::Skip);
        assert_eq!(read(now.join("nested/b")), "original b");
        assert_eq!(read(old.join("only-here")), "unrelated child");
    }

    #[test]
    fn a_cancelled_move_merge_keeps_completed_and_unvisited_children() {
        let t = TempDir::new("undo-cancelled-move-merge");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        for name in ["a", "b", "c"] {
            t.file(&format!("to/folder/{name}"), name);
        }
        t.file("from/folder/b", "unrelated b");
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move folder"));
        let r = invert_merging(&mut h, &tr, false, Resolution::Cancel);
        assert!(r.cancelled);
        assert_eq!(h.undo_label(), Some("Move folder"));
        assert_eq!(h.redo_label(), Some("Move folder"), "the completed child is still redoable");
        assert_eq!(read(old.join("a")), "a");
        assert_eq!(read(now.join("c")), "c");
        invert_merging(&mut h, &tr, true, Resolution::Skip);
        assert_eq!(read(now.join("a")), "a");
        assert_eq!(read(old.join("b")), "unrelated b");
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        std::fs::remove_file(old.join("b")).unwrap();
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert_eq!(h.undo_label(), None);
        assert_eq!(read(old.join("b")), "b");
        assert_eq!(read(old.join("c")), "c");
    }

    #[test]
    fn a_completed_move_merge_redoes_into_its_removed_parent_folders() {
        let t = TempDir::new("undo-complete-move-merge");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        t.file("to/folder/nested/a", "original a");
        t.dir("to/folder/empty");
        t.file("from/folder/nested/a", "unrelated a");
        t.file("from/folder/nested/only-here", "unrelated child");
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move folder"));
        assert!(invert_merging(&mut h, &tr, false, Resolution::KeepBoth).is_clean());
        assert!(!now.exists(), "the completed merge removed its empty source parents");
        assert_eq!(read(old.join("nested/a")), "unrelated a");
        assert!(invert_merging(&mut h, &tr, true, Resolution::Skip).is_clean());
        assert_eq!(read(now.join("nested/a")), "original a", "Keep Both's renamed child goes back");
        assert!(now.join("empty").is_dir());
        assert_eq!(read(old.join("nested/a")), "unrelated a");
        assert_eq!(read(old.join("nested/only-here")), "unrelated child");
        assert!(invert_merging(&mut h, &tr, false, Resolution::KeepBoth).is_clean());
        assert!(!now.exists(), "undo also removes only the parents it recreated");
        assert_eq!(read(old.join("nested/only-here")), "unrelated child");
    }

    #[test]
    fn move_merge_cleanup_keeps_a_replacement_directory() {
        let t = TempDir::new("undo-move-merge-replaced-dir");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        t.file("to/folder/a", "a");
        t.file("to/folder/b", "b");
        t.file("from/folder/b", "unrelated b");
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move"));
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        std::fs::rename(&now, t.join("set-aside")).unwrap();
        std::fs::create_dir(&now).unwrap();
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert!(now.is_dir(), "a replacement at the recorded name isn't our empty container");
        assert_eq!(read(t.join("set-aside/b")), "b");
        assert_eq!(read(old.join("b")), "unrelated b");
    }

    #[test]
    fn move_merge_redo_does_not_follow_a_replaced_parent() {
        let t = TempDir::new("redo-move-merge-symlink");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        t.file("to/folder/nested/a", "a");
        t.file("from/folder/nested/only-here", "unrelated child");
        let unrelated = t.dir("unrelated");
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move"));
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert!(!now.exists());
        std::os::unix::fs::symlink(&unrelated, &now).unwrap();
        let report = invert_merging(&mut h, &tr, true, Resolution::Skip);
        assert!(!report.failed.is_empty());
        assert_eq!(h.redo_label(), Some("Move"));
        assert_eq!(std::fs::read_dir(&unrelated).unwrap().count(), 0);
        assert_eq!(read(old.join("nested/a")), "a");
        std::fs::remove_file(&now).unwrap();
        assert!(invert_merging(&mut h, &tr, true, Resolution::Skip).is_clean());
        assert_eq!(read(now.join("nested/a")), "a");
        assert_eq!(read(old.join("nested/only-here")), "unrelated child");
    }

    #[test]
    fn move_merge_redo_checks_the_parent_again_after_recreating_it() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let t = TempDir::new("redo-move-merge-parent-changed");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        t.file("to/folder/a", "a");
        t.file("from/folder/only-here", "unrelated child");
        std::fs::set_permissions(&now, std::fs::Permissions::from_mode(0o755)).unwrap();
        let unrelated = t.dir("unrelated");
        std::fs::set_permissions(&unrelated, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move"));
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        let control = Control::new();
        let mut asker = MergeAnswer(Resolution::Skip);
        let mut swapped = false;
        let mut reporter = |p: &super::super::Progress| {
            if !swapped && p.current.is_some() {
                std::fs::rename(&now, t.join("recreated")).unwrap();
                std::os::unix::fs::symlink(&unrelated, &now).unwrap();
                swapped = true;
            }
        };
        let mut env = Env::new(&control, &mut asker, &mut reporter);
        let (report, result) = invert(h.begin_redo().unwrap(), &tr, &mut env);
        h.finished(result, |_| ());
        assert!(swapped);
        assert!(!report.failed.is_empty());
        assert_eq!(std::fs::read_dir(&unrelated).unwrap().count(), 0);
        assert_eq!(std::fs::metadata(&unrelated).unwrap().mode() & 0o777, 0o700, "chmod uses the recreated handle");
        assert_eq!(read(old.join("a")), "a");
    }

    #[test]
    fn move_merge_redo_keeps_unrelated_children_when_an_empty_source_stays() {
        use std::os::unix::fs::PermissionsExt;
        if crate::fsops::testing::is_root() {
            return;
        }
        let t = TempDir::new("undo-move-merge-remove-failed");
        let tr = trash(&t);
        let old = t.dir("from/folder");
        let now = t.dir("to/folder");
        t.file("to/folder/a", "a");
        t.file("from/folder/only-here", "unrelated child");
        std::fs::set_permissions(t.join("to"), std::fs::Permissions::from_mode(0o500)).unwrap();
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move"));
        let report = invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert!(!report.problems.is_empty(), "the empty source couldn't be removed");
        assert!(now.is_dir());
        assert!(invert_merging(&mut h, &tr, true, Resolution::Skip).is_clean());
        assert_eq!(read(now.join("a")), "a");
        assert_eq!(read(old.join("only-here")), "unrelated child");
    }

    #[test]
    fn a_partly_copied_move_keeps_its_source_pending_without_claiming_the_whole_copy() {
        use std::os::unix::fs::PermissionsExt;
        if crate::fsops::testing::is_root() {
            return;
        }
        let t = TempDir::new("undo-partial-copy");
        let Some(other) = TempDir::elsewhere("undo-partial-copy") else { return };
        let tr = trash(&t);
        let old = other.join("folder");
        let now = t.dir("folder");
        t.file("folder/a", "a");
        let unreadable = t.file("folder/b", "b");
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(old.clone(), now.clone())] }, "Move"));
        let report = invert_merging(&mut h, &tr, false, Resolution::Skip);
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!report.problems.is_empty());
        assert_eq!(h.undo_label(), Some("Move"));
        assert_eq!(h.redo_label(), None, "a source-retaining copy is not a whole-subtree move");
        assert_eq!(read(old.join("a")), "a");
        assert_eq!(read(now.join("b")), "b");
        invert_merging(&mut h, &tr, false, Resolution::Skip);
        assert_eq!(h.undo_label(), None);
        assert_eq!(read(old.join("b")), "b");
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
    fn restoring_same_named_items_does_not_keep_a_displaced_items_receipt() {
        let t = TempDir::new("undo-reused-trash-name");
        let tr = trash(&t);
        let a = t.file("one/a.txt", "original a");
        let b = t.file("two/a.txt", "original b");
        let items = vec![tr.trash(&a).unwrap(), tr.trash(&b).unwrap()];
        let freed = items[0].file().to_path_buf();
        t.file("two/a.txt", "newer b");
        let mut h = History::default();
        h.push(step(Record::Trash { items }, "Trash two files"));
        let control = Control::new();
        let mut asker = Always(Resolution::Replace, ErrorChoice::Cancel);
        let mut reporter = ();
        let mut env = Env::new(&control, &mut asker, &mut reporter);
        let (report, result) = invert(h.begin_undo().unwrap(), &tr, &mut env);
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(report.done.len(), 2);
        h.finished(result, |_| ());
        assert_eq!(read(&a), "original a");
        assert_eq!(read(&b), "original b");
        assert_eq!(read(&freed), "newer b", "the displaced item reused the first item's Trash name");
        assert_eq!((h.undo_label(), h.redo_label()), (None, Some("Trash two files")));
        assert!(h.begin_undo().is_none(), "neither completed item may remain in Undo");
        assert!(redo(&mut h, &tr).is_clean());
        assert!(undo(&mut h, &tr).is_clean());
        assert_eq!(read(&a), "original a");
        assert_eq!(read(&b), "original b");
        assert_eq!(read(&freed), "newer b", "redo and undo must leave the unrelated item alone");
    }

    #[test]
    fn a_partial_trash_restore_keeps_only_its_own_receipt_for_retry() {
        struct MergeThenSkip;
        impl super::super::Asker for MergeThenSkip {
            fn conflict(&mut self, conflict: &super::super::Conflict) -> Resolution {
                if conflict.can_merge() { Resolution::Merge } else { Resolution::Skip }
            }
            fn error(&mut self, _: &super::super::Failure) -> ErrorChoice {
                ErrorChoice::Cancel
            }
        }
        let t = TempDir::new("undo-partial-trash");
        let tr = trash(&t);
        let folder = t.dir("home/folder");
        t.file("home/folder/a", "original a");
        t.file("home/folder/b", "original b");
        let trashed = tr.trash(&folder).unwrap();
        t.file("home/folder/b", "newer b");
        let mut h = History::default();
        h.push(step(Record::Trash { items: vec![trashed.clone()] }, "Trash folder"));
        let control = Control::new();
        let mut asker = MergeThenSkip;
        let mut reporter = ();
        let mut env = Env::new(&control, &mut asker, &mut reporter);
        let (report, result) = invert(h.begin_undo().unwrap(), &tr, &mut env);
        assert_eq!(report.done.len(), 1);
        assert!(!report.problems.is_empty());
        h.finished(result, |_| ());
        assert_eq!(h.undo_label(), Some("Trash folder"));
        assert!(trashed.entry(&tr).is_some());
        assert_eq!(read(folder.join("a")), "original a");
        assert_eq!(read(trashed.file().join("b")), "original b");
        std::fs::remove_file(folder.join("b")).unwrap();
        let (report, result) = invert(h.begin_undo().unwrap(), &tr, &mut env);
        assert!(report.is_clean(), "{report:?}");
        h.finished(result, |_| ());
        assert_eq!(h.undo_label(), None);
        assert_eq!(read(folder.join("b")), "original b");
    }

    #[test]
    fn a_reused_trash_name_before_undo_is_not_the_recorded_item() {
        let t = TempDir::new("undo-replaced-receipt");
        let tr = trash(&t);
        let file = t.file("home/a", "original");
        let old = tr.trash(&file).unwrap();
        // Another file manager restores it, then trashes another item under
        // exactly the same original and Trash paths.
        let entry = old.entry(&tr).unwrap();
        std::fs::rename(old.file(), &file).unwrap();
        tr.forget(entry.gone().unwrap());
        t.file("home/a", "replacement");
        let new = tr.trash(&file).unwrap();
        assert_eq!(old.file(), new.file());
        let mut h = History::default();
        h.push(step(Record::Trash { items: vec![old] }, "Trash"));
        let report = undo(&mut h, &tr);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(h.undo_label(), None);
        assert!(!file.exists());
        assert_eq!(read(new.file()), "replacement");
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
        assert_eq!(h.finished(inverted.clone(), |_| ()), Filed::Kept);
        assert_eq!(h.undo_label(), Some("Newer"));
        // A result that isn't the one in flight changes nothing.
        assert_eq!(h.finished(inverted, |_| ()), Filed::Stale);
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
            full.push(Held {
                step: step(Record::Made { paths: vec![PathBuf::from(format!("/nowhere/{i}"))] }, "filler"),
                failed: false,
            });
        }
        h.with_typed(|s| match s {
            HistoryAny::Idle(s) => {
                s.ctx.undo = full;
                (s.into(), ())
            }
            other @ (HistoryAny::Undoing(_) | HistoryAny::Redoing(_)) => (other, ()),
        });
        assert!(redo(&mut h, &tr).is_clean());
        assert_eq!(h.ctx().undo.len(), DEPTH, "the oldest step makes room");
        assert_eq!(h.undo_label(), Some("Made"));
    }

    #[test]
    fn a_step_that_keeps_failing_is_left_out_and_said() {
        let t = TempDir::new("undo-keeps-failing");
        let tr = trash(&t);
        let (a, b) = (t.file("a.txt", "a"), t.join("b.txt"));
        std::fs::rename(&a, &b).unwrap();
        t.file("a.txt", "newer");
        let mut h = History::default();
        h.push(step(Record::Made { paths: vec![t.join("old")] }, "Older"));
        h.push(step(Record::Rename { from: a.clone(), to: b.clone() }, "Rename"));
        // The name is taken: the first failure keeps the step to try again.
        let (_, inverted) = run(h.begin_undo().unwrap(), &tr);
        assert_eq!(h.finished(inverted, |_| ()), Filed::Kept);
        assert_eq!(h.undo_label(), Some("Rename"));
        // Still taken: the second leaves it out, and says so.
        let (_, inverted) = run(h.begin_undo().unwrap(), &tr);
        let mut told = None;
        let filed = h.finished(inverted, |note| told = Some(note));
        assert!(matches!(filed, Filed::Dropped(_)), "a second failure drops the step: {filed:?}");
        let note = told.expect("the person is told");
        assert!(note.contains("“Rename”") && note.contains("left out of Undo"), "{note}");
        assert_eq!(h.undo_label(), Some("Older"), "the steps before it can be reached");
        assert_eq!(read(&b), "a");
    }

    #[test]
    fn an_abandoned_undo_keeps_its_step() {
        let mut h = History::default();
        h.push(step(Record::Made { paths: vec![PathBuf::from("/nowhere/made")] }, "Copy"));
        let inversion = h.begin_undo().unwrap();
        // Its job couldn't start, or panicked: no result but this.
        assert_eq!(h.finished(inversion.abandoned(), |_| ()), Filed::Kept);
        assert_eq!((h.undo_label(), h.redo_label()), (Some("Copy"), None));
    }

    #[test]
    fn an_undo_that_lands_after_a_new_action_isn_t_redone() {
        let t = TempDir::new("undo-after-action");
        let tr = trash(&t);
        let (a, b) = (t.file("a.txt", "a"), t.join("b.txt"));
        std::fs::rename(&a, &b).unwrap();
        let mut h = History::default();
        h.push(step(Record::Rename { from: a.clone(), to: b.clone() }, "Rename"));
        let inversion = h.begin_undo().unwrap();
        h.push(step(Record::Made { paths: vec![t.join("new")] }, "Newer"));
        let (r, inverted) = run(inversion, &tr);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(h.finished(inverted, |_| ()), Filed::Done);
        assert_eq!((h.undo_label(), h.redo_label()), (Some("Newer"), None), "a new action forgets redo");
    }

    #[test]
    fn a_skipped_or_stopped_undo_isn_t_a_failure() {
        let t = TempDir::new("undo-declined");
        let tr = trash(&t);
        let a = t.file("from/a.txt", "a");
        let to = t.dir("to").join("a.txt");
        std::fs::rename(&a, &to).unwrap();
        // Something has a.txt's old name: the undo's conflict is answered
        // Skip (by `run`'s asker), twice. The person chose that: it stays.
        t.file("from/a.txt", "newer");
        let mut h = History::default();
        h.push(step(Record::Move { pairs: vec![(a.clone(), to.clone())] }, "Move"));
        for _ in 0..2 {
            let (r, inverted) = run(h.begin_undo().unwrap(), &tr);
            assert_eq!(r.skipped, vec![to.clone()]);
            assert_eq!(h.finished(inverted, |note| panic!("nothing to tell: {note}")), Filed::Kept);
        }
        assert_eq!(h.undo_label(), Some("Move"));
    }
}
