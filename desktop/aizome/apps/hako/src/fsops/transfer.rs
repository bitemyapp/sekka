//! Copying and moving items, duplicating them, and the Trash's errands
//! (trashing, restoring, deleting from it): see the module above for the
//! rules they keep.

use std::ffi::OsStr;
use std::fs::{self, File, FileTimes, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::delete;
use super::names;
use super::sys;
use super::trash::{self, Trash};
use super::{Cancelled, Conflict, Env, ErrorChoice, Failure, Report, Resolution, is_within};

/// Copy, or move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Copy,
    Move,
}

/// An item and where it's to go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub source: PathBuf,
    pub target: PathBuf,
}

/// Copy or move `sources` into the folder `dest`, each under its own
/// name. Replaced items go to `trash` when there is one (else they're
/// deleted once their replacements are in place).
pub fn transfer(sources: &[PathBuf], dest: &Path, mode: Mode, env: &mut Env, trash: Option<&Trash>) -> Report {
    let items: Vec<Item> = sources
        .iter()
        .map(|s| Item { source: s.clone(), target: dest.join(s.file_name().unwrap_or(OsStr::new("item"))) })
        .collect();
    transfer_items(&items, mode, env, trash)
}

/// Copy each of `sources` beside itself, as "name copy".
pub fn duplicate(sources: &[PathBuf], env: &mut Env) -> Report {
    let items: Vec<Item> = sources
        .iter()
        .filter_map(|s| {
            let dir = s.parent()?;
            let is_dir = fs::symlink_metadata(s).is_ok_and(|m| m.is_dir());
            Some(Item { source: s.clone(), target: dir.join(names::duplicate_name(dir, s.file_name()?, is_dir)) })
        })
        .collect();
    transfer_items(&items, Mode::Copy, env, None)
}

/// Copy or move each item to its target.
pub fn transfer_items(items: &[Item], mode: Mode, env: &mut Env, trash: Option<&Trash>) -> Report {
    let mut report = Report::default();
    let mut w = Worker { env, mode, trash, problems: Vec::new() };
    if w.count(items).is_err() {
        report.cancelled = true;
        return report;
    }
    for item in items {
        if w.env.control.checkpoint().is_err() {
            report.cancelled = true;
            break;
        }
        w.env.progress.current = Some(item.source.clone());
        w.env.report();
        match w.place_top(&item.source, &item.target) {
            Ok(Placed::Done(to)) => report.done.push((item.source.clone(), to)),
            Ok(Placed::Skipped) => report.skipped.push(item.source.clone()),
            Err(Stop::Skip(why)) => report.failed.push((item.source.clone(), why)),
            Err(Stop::Cancel) => {
                report.cancelled = true;
                break;
            }
        }
    }
    report.problems = std::mem::take(&mut w.problems);
    w.env.progress.current = None;
    w.env.report();
    report
}

/// Move `paths` to the Trash: the report's `done` pairs each with where
/// it went in a can, and the [`trash::Trashed`] records come back for
/// undo.
pub fn trash_items(paths: &[PathBuf], trash: &Trash, env: &mut Env) -> (Report, Vec<trash::Trashed>) {
    let mut report = Report::default();
    let mut trashed = Vec::new();
    env.progress.items_total = paths.len() as u64;
    for p in paths {
        if env.control.checkpoint().is_err() {
            report.cancelled = true;
            break;
        }
        env.progress.current = Some(p.clone());
        env.report();
        loop {
            match trash.trash(p) {
                Ok(t) => {
                    report.done.push((p.clone(), t.file.clone()));
                    trashed.push(t);
                }
                Err(e) => {
                    let mut f = Failure::new(p, "move to the Trash", &e);
                    if e.kind() == io::ErrorKind::PermissionDenied && trash.refusal(p).is_some() {
                        f.can_retry = false;
                    }
                    match env.ask_error(f.clone()) {
                        ErrorChoice::Retry => continue,
                        ErrorChoice::Skip => report.failed.push((p.clone(), f.message)),
                        ErrorChoice::Cancel => report.cancelled = true,
                    }
                }
            }
            break;
        }
        if report.cancelled {
            break;
        }
        env.progress.items_done += 1;
    }
    env.progress.current = None;
    env.report();
    (report, trashed)
}

/// Put trashed items back where they came from (making the folders on
/// the way if they've gone), asking about conflicts there.
pub fn restore(entries: &[trash::Entry], trash: &Trash, env: &mut Env) -> Report {
    let mut report = Report::default();
    let mut items = Vec::new();
    for e in entries {
        match &e.original {
            Some(o) => items.push((e, Item { source: e.file.clone(), target: o.clone() })),
            None => report.failed.push((e.file.clone(), "where it came from isn't known".into())),
        }
    }
    let mut w = Worker { env, mode: Mode::Move, trash: None, problems: Vec::new() };
    let plain: Vec<Item> = items.iter().map(|(_, i)| i.clone()).collect();
    if w.count(&plain).is_err() {
        report.cancelled = true;
        return report;
    }
    for (entry, item) in items {
        if w.env.control.checkpoint().is_err() {
            report.cancelled = true;
            break;
        }
        w.env.progress.current = Some(item.target.clone());
        w.env.report();
        if let Some(parent) = item.target.parent()
            && !parent.is_dir()
            && let Err(e) = fs::create_dir_all(parent)
        {
            report
                .failed
                .push((item.source.clone(), format!("its folder couldn't be made again: {}", super::describe(&e))));
            continue;
        }
        match w.place_top(&item.source, &item.target) {
            Ok(Placed::Done(to)) => {
                trash.forget(entry);
                report.done.push((item.source.clone(), to));
            }
            Ok(Placed::Skipped) => report.skipped.push(item.source.clone()),
            Err(Stop::Skip(why)) => report.failed.push((item.source.clone(), why)),
            Err(Stop::Cancel) => {
                report.cancelled = true;
                break;
            }
        }
    }
    report.problems = std::mem::take(&mut w.problems);
    report
}

/// Delete items from the Trash for good. Only what's in a trash can is
/// taken: anything else is refused.
pub fn erase(entries: &[trash::Entry], trash: &Trash, env: &mut Env) -> Report {
    let mut report = Report::default();
    env.progress.preparing = true;
    env.report();
    for e in entries {
        if let Some((n, b)) = delete::measure(&e.file, env.control, u64::MAX) {
            env.progress.items_total += n;
            env.progress.bytes_total += b;
        } else {
            report.cancelled = true;
            return report;
        }
    }
    env.progress.preparing = false;
    for e in entries {
        let real = fs::canonicalize(e.file.parent().unwrap_or(Path::new("/"))).unwrap_or_default().join(&e.name);
        if !trash.in_a_can(&real) {
            report.failed.push((e.file.clone(), "it isn't in the Trash".into()));
            continue;
        }
        env.progress.current = Some(e.file.clone());
        let Env { control, reporter, progress, .. } = &mut *env;
        let r = delete::remove_tree(&e.file, control, &mut |bytes| {
            progress.items_done += 1;
            progress.bytes_done += bytes;
            reporter.report(progress);
        });
        match r {
            Ok(r) if r.failed.is_empty() => {
                trash.forget(e);
                report.done.push((e.file.clone(), e.file.clone()));
            }
            Ok(r) => {
                let (p, why) = r.failed[0].clone();
                report.failed.push((e.file.clone(), format!("{}: {why}", p.display())));
            }
            Err(Cancelled) => {
                report.cancelled = true;
                break;
            }
        }
    }
    report
}

/// Empty every trash can, infos without items included.
pub fn empty_trash(trash: &Trash, env: &mut Env) -> Report {
    let entries = trash.list();
    let report = erase(&entries, trash, env);
    if !report.cancelled {
        for can in trash.cans() {
            if let Ok(rd) = fs::read_dir(can.info()) {
                for e in rd.flatten() {
                    let name = e.file_name();
                    let n = name.as_encoded_bytes();
                    if let Some(stem) = n.strip_suffix(b".trashinfo") {
                        // SAFETY: a prefix of an OsStr's encoded bytes,
                        // cut at an ASCII suffix, is valid encoded bytes.
                        let stem = unsafe { OsStr::from_encoded_bytes_unchecked(stem) };
                        if fs::symlink_metadata(can.files().join(stem)).is_err() {
                            let _ = fs::remove_file(e.path());
                        }
                    }
                }
            }
        }
    }
    report
}

/// How a top-level item ended.
enum Placed {
    Done(PathBuf),
    Skipped,
}

/// Why an item stopped.
enum Stop {
    Cancel,
    /// Left out, and why.
    Skip(String),
}

type Step<T> = Result<T, Stop>;

/// The sources a move across filesystems may delete once their copies
/// are in place: files and links copied whole, then folders (which are
/// removed only if they're empty by then).
#[derive(Default)]
struct Arrived {
    leaves: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

struct Worker<'x, 'a> {
    env: &'x mut Env<'a>,
    mode: Mode,
    trash: Option<&'x Trash>,
    problems: Vec<(PathBuf, String)>,
}

fn is_cancel(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|i| i.is::<Cancelled>())
}

/// Whether `path` (with `meta`, not followed) is where a filesystem is
/// mounted: on another device than the folder it's in.
pub fn is_mount_point(path: &Path, meta: &fs::Metadata) -> bool {
    let parent = path.parent().unwrap_or(Path::new("/"));
    meta.is_dir() && fs::metadata(parent).is_ok_and(|p| p.dev() != meta.dev())
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

impl Worker<'_, '_> {
    /// Ask about a failure: `Ok` to retry.
    fn ask(&mut self, f: Failure) -> Step<()> {
        match self.env.ask_error(f.clone()) {
            ErrorChoice::Retry => Ok(()),
            ErrorChoice::Skip => Err(Stop::Skip(f.message)),
            ErrorChoice::Cancel => Err(Stop::Cancel),
        }
    }

    fn checkpoint(&self) -> Step<()> {
        self.env.control.checkpoint().map_err(|_| Stop::Cancel)
    }

    /// Count what there is to do, for the progress.
    fn count(&mut self, items: &[Item]) -> Result<(), Cancelled> {
        self.env.progress.preparing = true;
        self.env.report();
        for item in items {
            let renames = self.mode == Mode::Move && self.renames(&item.source, &item.target);
            if renames {
                self.env.progress.items_total += 1;
            } else {
                let (n, b) = delete::measure(&item.source, self.env.control, u64::MAX).ok_or(Cancelled)?;
                self.env.progress.items_total += n;
                self.env.progress.bytes_total += b;
            }
            self.env.report();
        }
        self.env.progress.preparing = false;
        self.env.report();
        Ok(())
    }

    /// Whether moving `source` to `target` is a rename (the same
    /// filesystem), as far as one can tell before trying.
    fn renames(&self, source: &Path, target: &Path) -> bool {
        let Some(dir) = target.parent() else { return false };
        match (sys::device(source), fs::metadata(dir)) {
            (Ok(a), Ok(b)) => a == b.dev(),
            _ => false,
        }
    }

    fn source_meta(&mut self, source: &Path) -> Step<fs::Metadata> {
        loop {
            match fs::symlink_metadata(source) {
                Ok(m) => return Ok(m),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(Stop::Skip("it isn't there any more".into()));
                }
                Err(e) => self.ask(Failure::new(source, "read", &e))?,
            }
        }
    }

    /// Bring `source` to `target`, asking about whatever has the name.
    fn place_top(&mut self, source: &Path, target: &Path) -> Step<Placed> {
        let smeta = self.source_meta(source)?;
        let (src_n, mut target) = (names::normalize(source), names::normalize(target));
        let Some(dest) = target.parent().map(Path::to_path_buf) else {
            return Err(Stop::Skip("it can't go there".into()));
        };
        if src_n == target {
            match self.mode {
                Mode::Move => return Ok(Placed::Done(target)),
                Mode::Copy => {
                    target =
                        dest.join(names::duplicate_name(&dest, source.file_name().unwrap_or_default(), smeta.is_dir()))
                }
            }
        }
        if smeta.is_dir() && is_within(&target, &src_n) {
            return Err(Stop::Skip("a folder can't go inside itself".into()));
        }
        if self.mode == Mode::Move && smeta.is_dir() && is_mount_point(source, &smeta) {
            // Its files live on another disk: moving them would empty it.
            return Err(Stop::Skip("it's where a disk is mounted; unmount it, or copy what's on it".into()));
        }
        let mut replace = false;
        if let Ok(tmeta) = fs::symlink_metadata(&target) {
            if same_file(&smeta, &tmeta) {
                if self.mode == Mode::Move {
                    // Another name of the same file (a hard link, or a
                    // name that differs only in case where case doesn't
                    // count): a plain rename does what can be done.
                    return match fs::rename(source, &target) {
                        Ok(()) => Ok(Placed::Done(target)),
                        Err(e) => Err(Stop::Skip(super::describe(&e))),
                    };
                }
                target = dest.join(names::free_name(&dest, target.file_name().unwrap_or_default(), smeta.is_dir()));
            } else {
                let conflict = Conflict {
                    source: source.to_path_buf(),
                    target: target.clone(),
                    source_is_dir: smeta.is_dir(),
                    target_is_dir: tmeta.is_dir(),
                    source_size: smeta.len(),
                    target_size: tmeta.len(),
                    source_modified: smeta.modified().ok(),
                    target_modified: tmeta.modified().ok(),
                };
                match self.env.asker.conflict(&conflict) {
                    Resolution::Skip => return Ok(Placed::Skipped),
                    Resolution::Cancel => return Err(Stop::Cancel),
                    Resolution::KeepBoth => {
                        target =
                            dest.join(names::free_name(&dest, target.file_name().unwrap_or_default(), smeta.is_dir()));
                    }
                    Resolution::Merge if conflict.can_merge() => {
                        self.merge(source, &target)?;
                        return Ok(Placed::Done(target));
                    }
                    Resolution::Replace | Resolution::Merge => {
                        if is_within(&src_n, &target) {
                            return Err(Stop::Skip("it can't replace a folder it's inside".into()));
                        }
                        replace = true;
                    }
                }
            }
        }
        let aside = if replace { Some(self.set_aside(&target)?) } else { None };
        match self.place(source, &target, &smeta) {
            Ok(()) => {
                if let Some(aside) = aside {
                    self.dispose(&aside, &target);
                }
                Ok(Placed::Done(target))
            }
            Err(stop) => {
                if let Some(aside) = aside
                    && let Err(e) = sys::rename_noreplace(&aside, &target)
                {
                    self.problems.push((
                        target.clone(),
                        format!(
                            "the item it was to replace is kept as “{}” ({})",
                            aside.display(),
                            super::describe(&e)
                        ),
                    ));
                }
                Err(stop)
            }
        }
    }

    /// Move what's at `target` out of the way, under a hidden name beside
    /// it, until its replacement is in place.
    fn set_aside(&mut self, target: &Path) -> Step<PathBuf> {
        let dir = target.parent().unwrap_or(Path::new("/"));
        loop {
            let aside = dir.join(names::aside_name(target.file_name().unwrap_or_default()));
            match sys::rename_noreplace(target, &aside) {
                Ok(()) => return Ok(aside),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => self.ask(Failure::new(target, "replace", &e))?,
            }
        }
    }

    /// A replaced item, set aside: to the Trash (as the item it was), or
    /// deleted.
    fn dispose(&mut self, aside: &Path, was: &Path) {
        if let Some(trash) = self.trash
            && trash.trash_as(aside, was).is_ok()
        {
            return;
        }
        let r = delete::remove_quietly(aside);
        if let Some((p, why)) = r.failed.first() {
            self.problems.push((
                was.to_path_buf(),
                format!("the item it replaced couldn't all be removed ({}: {why})", p.display()),
            ));
        }
    }

    /// Bring `source` to `target`, where nothing is: a rename where it
    /// can be, else a copy (and, for a move, the source's deletion once
    /// the copy is safely in place).
    fn place(&mut self, source: &Path, target: &Path, smeta: &fs::Metadata) -> Step<()> {
        if self.mode == Mode::Move {
            loop {
                match self.rename_into(source, target, smeta) {
                    Ok(()) => {
                        self.env.progress.items_done += 1;
                        self.env.report();
                        return Ok(());
                    }
                    Err(e) if sys::is_cross_device(&e) => {
                        // Counted as a rename: count it as the copy it is.
                        if let Some((n, b)) = delete::measure(source, &super::Control::new(), u64::MAX) {
                            self.env.progress.items_total += n.saturating_sub(1);
                            self.env.progress.bytes_total += b;
                        }
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        return Err(Stop::Skip(format!(
                            "“{}” appeared there meanwhile",
                            names::display(target.file_name().unwrap_or_default())
                        )));
                    }
                    Err(e) => self.ask(Failure::new(source, "move", &e))?,
                }
            }
        }
        let dest = target.parent().unwrap_or(Path::new("/"));
        let partial = dest.join(names::partial_name(target.file_name().unwrap_or_default()));
        let mut arrived = (self.mode == Mode::Move).then(Arrived::default);
        let sync = self.mode == Mode::Move;
        match self.copy_any(source, &partial, sync, &mut arrived) {
            Ok(_) => {}
            Err(stop) => {
                delete::remove_quietly(&partial);
                return Err(stop);
            }
        }
        loop {
            match sys::rename_noreplace(&partial, target) {
                Ok(()) => break,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    delete::remove_quietly(&partial);
                    return Err(Stop::Skip(format!(
                        "“{}” appeared there meanwhile",
                        names::display(target.file_name().unwrap_or_default())
                    )));
                }
                Err(e) => {
                    if let Err(stop) = self.ask(Failure::new(target, "put in place", &e)) {
                        delete::remove_quietly(&partial);
                        return Err(stop);
                    }
                }
            }
        }
        if sync && let Err(e) = sys::sync_dir(dest) {
            // The copy may not be on the disk: keep the source.
            self.problems.push((source.to_path_buf(), format!("the original is kept: {}", super::describe(&e))));
            return Ok(());
        }
        if let Some(arrived) = arrived {
            for leaf in &arrived.leaves {
                if let Err(e) = fs::remove_file(leaf)
                    && e.kind() != io::ErrorKind::NotFound
                {
                    self.problems.push((
                        leaf.clone(),
                        format!("arrived, but the original couldn't be removed: {}", super::describe(&e)),
                    ));
                }
            }
            for dir in &arrived.dirs {
                // Folders with things left in them (skipped) stay.
                let _ = fs::remove_dir(dir);
            }
        }
        Ok(())
    }

    /// `rename` that refuses to overwrite, and moves a read-only folder
    /// of ours (which a rename to another folder needs to write to).
    fn rename_into(&self, source: &Path, target: &Path, smeta: &fs::Metadata) -> io::Result<()> {
        match sys::rename_noreplace(source, target) {
            Err(e)
                if e.kind() == io::ErrorKind::PermissionDenied
                    && smeta.is_dir()
                    && smeta.uid() == sys::uid()
                    && smeta.mode() & 0o200 == 0 =>
            {
                let mode = smeta.mode() & 0o7777;
                fs::set_permissions(source, fs::Permissions::from_mode(mode | 0o200))?;
                let r = sys::rename_noreplace(source, target);
                let now = if r.is_ok() { target } else { source };
                let _ = fs::set_permissions(now, fs::Permissions::from_mode(mode));
                r
            }
            r => r,
        }
    }

    /// Folders both: `source`'s contents into `target`, each placed (and
    /// asked about) on its own; a moved folder left empty goes.
    fn merge(&mut self, source: &Path, target: &Path) -> Step<()> {
        let names = self.read_names(source)?;
        let mut complete = true;
        for name in names {
            self.checkpoint()?;
            let (s, t) = (source.join(&name), target.join(&name));
            match self.place_top(&s, &t) {
                Ok(Placed::Done(_)) => {}
                Ok(Placed::Skipped) => complete = false,
                Err(Stop::Skip(why)) => {
                    complete = false;
                    self.problems.push((s, why));
                }
                Err(Stop::Cancel) => return Err(Stop::Cancel),
            }
        }
        if self.mode == Mode::Move && complete {
            let _ = fs::remove_dir(source);
        }
        self.env.progress.items_done += 1;
        Ok(())
    }

    fn read_names(&mut self, dir: &Path) -> Step<Vec<std::ffi::OsString>> {
        loop {
            match fs::read_dir(dir).and_then(|rd| rd.map(|e| e.map(|e| e.file_name())).collect::<io::Result<Vec<_>>>())
            {
                Ok(mut v) => {
                    v.sort();
                    return Ok(v);
                }
                Err(e) => self.ask(Failure::new(dir, "read the folder", &e))?,
            }
        }
    }

    /// Copy whatever `source` is to `dest` (which isn't there): whether
    /// all of it came.
    fn copy_any(&mut self, source: &Path, dest: &Path, sync: bool, arrived: &mut Option<Arrived>) -> Step<bool> {
        self.checkpoint()?;
        let meta = self.source_meta(source)?;
        self.env.progress.current = Some(source.to_path_buf());
        let ft = meta.file_type();
        if ft.is_symlink() {
            self.copy_link(source, dest, arrived)
        } else if ft.is_dir() {
            self.copy_dir(source, dest, &meta, sync, arrived)
        } else if ft.is_file() {
            self.copy_file(source, dest, &meta, sync, arrived)
        } else {
            self.ask(Failure::because(
                source,
                "copy",
                "it's a special file (a pipe, socket or device), which can't be copied",
            ))?;
            Err(Stop::Skip("it's a special file".into()))
        }
    }

    fn copy_link(&mut self, source: &Path, dest: &Path, arrived: &mut Option<Arrived>) -> Step<bool> {
        loop {
            match fs::read_link(source).and_then(|to| std::os::unix::fs::symlink(to, dest)) {
                Ok(()) => break,
                Err(e) => self.ask(Failure::new(source, "copy the link", &e))?,
            }
        }
        self.env.progress.items_done += 1;
        if let Some(a) = arrived {
            a.leaves.push(source.to_path_buf());
        }
        Ok(true)
    }

    fn copy_file(
        &mut self,
        source: &Path,
        dest: &Path,
        meta: &fs::Metadata,
        sync: bool,
        arrived: &mut Option<Arrived>,
    ) -> Step<bool> {
        loop {
            let mut counted = 0u64;
            match self.try_copy_file(source, dest, meta, sync, &mut counted) {
                Ok(()) => break,
                Err((action, e)) => {
                    let _ = fs::remove_file(dest);
                    self.env.progress.bytes_done = self.env.progress.bytes_done.saturating_sub(counted);
                    if is_cancel(&e) {
                        return Err(Stop::Cancel);
                    }
                    self.ask(Failure::new(source, action, &e))?;
                }
            }
        }
        self.env.progress.items_done += 1;
        self.env.report();
        if let Some(a) = arrived {
            a.leaves.push(source.to_path_buf());
        }
        Ok(true)
    }

    fn try_copy_file(
        &mut self,
        source: &Path,
        dest: &Path,
        meta: &fs::Metadata,
        sync: bool,
        counted: &mut u64,
    ) -> Result<(), (&'static str, io::Error)> {
        let mut from = File::open(source).map_err(|e| ("read", e))?;
        let mut to =
            OpenOptions::new().write(true).create_new(true).mode(0o600).open(dest).map_err(|e| ("write", e))?;
        let Env { control, reporter, progress, .. } = &mut *self.env;
        sys::copy_contents(&mut from, &mut to, &mut |n| {
            *counted += n;
            progress.bytes_done += n;
            reporter.report(progress);
            control.checkpoint().map_err(io::Error::other)
        })
        .map_err(|e| ("copy", e))?;
        // Its permissions, without set-user and set-group.
        to.set_permissions(fs::Permissions::from_mode(meta.mode() & 0o1777)).map_err(|e| ("copy", e))?;
        if let (Ok(a), Ok(m)) = (meta.accessed(), meta.modified()) {
            let _ = to.set_times(FileTimes::new().set_accessed(a).set_modified(m));
        }
        if sync {
            to.sync_all().map_err(|e| ("write", e))?;
        }
        Ok(())
    }

    fn copy_dir(
        &mut self,
        source: &Path,
        dest: &Path,
        meta: &fs::Metadata,
        sync: bool,
        arrived: &mut Option<Arrived>,
    ) -> Step<bool> {
        loop {
            match fs::DirBuilder::new().mode(0o700).create(dest) {
                Ok(()) => break,
                Err(e) => self.ask(Failure::new(dest, "make the folder", &e))?,
            }
        }
        let names = match self.read_names(source) {
            Ok(n) => n,
            Err(stop) => {
                let _ = fs::remove_dir(dest);
                return Err(stop);
            }
        };
        let mut complete = true;
        for name in names {
            let (s, d) = (source.join(&name), dest.join(&name));
            match self.copy_any(&s, &d, sync, arrived) {
                Ok(c) => complete &= c,
                Err(Stop::Skip(why)) => {
                    complete = false;
                    self.problems.push((s, why));
                }
                Err(Stop::Cancel) => return Err(Stop::Cancel),
            }
        }
        // The folder's own permissions and times, now that it's full.
        if let Err(e) = fs::set_permissions(dest, fs::Permissions::from_mode(meta.mode() & 0o1777)) {
            self.problems
                .push((source.to_path_buf(), format!("its permissions couldn't be copied: {}", super::describe(&e))));
        }
        if let (Ok(a), Ok(m), Ok(f)) = (meta.accessed(), meta.modified(), File::open(dest)) {
            let _ = f.set_times(FileTimes::new().set_accessed(a).set_modified(m));
        }
        if sync && let Err(e) = sys::sync_dir(dest) {
            return Err(Stop::Skip(format!("it couldn't be written safely: {}", super::describe(&e))));
        }
        self.env.progress.items_done += 1;
        if let Some(a) = arrived {
            a.dirs.push(source.to_path_buf());
        }
        Ok(complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::testing::{TempDir, is_root, read};
    use crate::fsops::{Always, Control, Progress};
    use std::collections::VecDeque;

    /// Answers from a script, and remembers what it was asked.
    #[derive(Default)]
    struct Script {
        conflicts: VecDeque<Resolution>,
        errors: VecDeque<ErrorChoice>,
        asked: Vec<String>,
    }

    impl crate::fsops::Asker for Script {
        fn conflict(&mut self, c: &Conflict) -> Resolution {
            self.asked.push(format!("conflict {}", c.target.file_name().unwrap().to_string_lossy()));
            self.conflicts.pop_front().expect("an answer for the conflict")
        }

        fn error(&mut self, f: &Failure) -> ErrorChoice {
            self.asked.push(format!("error {} {}", f.action, f.path.file_name().unwrap_or_default().to_string_lossy()));
            self.errors.pop_front().expect("an answer for the error")
        }
    }

    fn run(sources: &[PathBuf], dest: &Path, mode: Mode, asker: &mut dyn crate::fsops::Asker) -> Report {
        let control = Control::new();
        let mut last = Progress::default();
        let mut reporter = |p: &Progress| last = p.clone();
        let mut env = Env::new(&control, asker, &mut reporter);
        transfer(sources, dest, mode, &mut env, None)
    }

    fn quiet(sources: &[PathBuf], dest: &Path, mode: Mode) -> Report {
        run(sources, dest, mode, &mut Always(Resolution::Cancel, ErrorChoice::Cancel))
    }

    fn tree(t: &TempDir, rel: &str) -> PathBuf {
        t.file(&format!("{rel}/a.txt"), "alpha");
        t.file(&format!("{rel}/sub/b.txt"), "bravo");
        t.file(&format!("{rel}/sub/deeper/c.txt"), "charlie");
        std::os::unix::fs::symlink("sub/b.txt", t.join(format!("{rel}/link"))).unwrap();
        t.join(rel)
    }

    fn assert_tree(root: &Path) {
        assert_eq!(read(root.join("a.txt")), "alpha");
        assert_eq!(read(root.join("sub/b.txt")), "bravo");
        assert_eq!(read(root.join("sub/deeper/c.txt")), "charlie");
        assert_eq!(fs::read_link(root.join("link")).unwrap(), PathBuf::from("sub/b.txt"));
    }

    fn no_temporaries(dir: &Path) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            assert!(!names::is_temporary(&e.file_name()), "left behind: {}", e.path().display());
        }
    }

    #[test]
    fn copies_files_folders_and_links() {
        let t = TempDir::new("copy");
        let src = tree(&t, "src");
        let file = t.file("note.txt", "note");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
        let dest = t.dir("dest");
        let r = quiet(&[src.clone(), file.clone()], &dest, Mode::Copy);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(r.done, vec![(src.clone(), dest.join("src")), (file.clone(), dest.join("note.txt"))]);
        assert_tree(&dest.join("src"));
        assert_tree(&src);
        assert_eq!(read(dest.join("note.txt")), "note");
        let m = fs::metadata(dest.join("note.txt")).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o640);
        assert_eq!(m.modified().unwrap(), fs::metadata(&file).unwrap().modified().unwrap(), "times are kept");
        assert!(fs::symlink_metadata(dest.join("src/link")).unwrap().file_type().is_symlink(), "links stay links");
        no_temporaries(&dest);
    }

    #[test]
    fn copying_into_its_own_folder_duplicates() {
        let t = TempDir::new("copy-same");
        let f = t.file("doc.txt", "d");
        let r = quiet(std::slice::from_ref(&f), t.path(), Mode::Copy);
        assert_eq!(r.done, vec![(f.clone(), t.join("doc copy.txt"))]);
        let r = duplicate(
            std::slice::from_ref(&f),
            &mut Env::new(&Control::new(), &mut Always(Resolution::Cancel, ErrorChoice::Cancel), &mut ()),
        );
        assert_eq!(r.done, vec![(f, t.join("doc copy 2.txt"))]);
    }

    #[test]
    fn refuses_a_folder_into_itself() {
        let t = TempDir::new("into-itself");
        let src = tree(&t, "src");
        let inner = src.join("sub");
        for mode in [Mode::Copy, Mode::Move] {
            let r = quiet(std::slice::from_ref(&src), &inner, mode);
            assert_eq!(r.failed.len(), 1, "{r:?}");
            assert!(r.failed[0].1.contains("inside itself"));
            assert_tree(&src);
            assert!(!inner.join("src").exists());
        }
        // Moving a thing to where it is does nothing, quietly.
        let r = quiet(std::slice::from_ref(&src), t.path(), Mode::Move);
        assert_eq!(r.done, vec![(src.clone(), src.clone())]);
        assert_tree(&src);
    }

    #[test]
    fn moves_by_renaming() {
        let t = TempDir::new("move");
        let src = tree(&t, "src");
        let ino = fs::metadata(src.join("a.txt")).unwrap().ino();
        let dest = t.dir("dest");
        let r = quiet(std::slice::from_ref(&src), &dest, Mode::Move);
        assert!(r.is_clean(), "{r:?}");
        assert!(!src.exists());
        assert_tree(&dest.join("src"));
        assert_eq!(fs::metadata(dest.join("src/a.txt")).unwrap().ino(), ino, "a rename, not a copy");
    }

    #[test]
    fn conflicts_skip_keep_both_and_replace() {
        let t = TempDir::new("conflicts");
        let dest = t.dir("dest");
        t.file("dest/a.txt", "old a");
        t.file("dest/b.txt", "old b");
        t.file("dest/c.txt", "old c");
        let a = t.file("new/a.txt", "new a");
        let b = t.file("new/b.txt", "new b");
        let c = t.file("new/c.txt", "new c");
        let mut s = Script {
            conflicts: [Resolution::Skip, Resolution::KeepBoth, Resolution::Replace].into(),
            ..Default::default()
        };
        let r = run(&[a.clone(), b.clone(), c.clone()], &dest, Mode::Copy, &mut s);
        assert_eq!(s.asked, ["conflict a.txt", "conflict b.txt", "conflict c.txt"]);
        assert_eq!(r.skipped, vec![a]);
        assert_eq!(r.done, vec![(b, dest.join("b 2.txt")), (c, dest.join("c.txt"))]);
        assert_eq!(read(dest.join("a.txt")), "old a");
        assert_eq!(read(dest.join("b.txt")), "old b");
        assert_eq!(read(dest.join("b 2.txt")), "new b");
        assert_eq!(read(dest.join("c.txt")), "new c");
        no_temporaries(&dest);
        // Cancel stops everything after it.
        let d = t.file("new2/a.txt", "x");
        let e = t.file("new2/zzz.txt", "y");
        let mut s = Script { conflicts: [Resolution::Cancel].into(), ..Default::default() };
        let r = run(&[d, e], &dest, Mode::Copy, &mut s);
        assert!(r.cancelled);
        assert!(!dest.join("zzz.txt").exists());
    }

    #[test]
    fn replaced_items_go_to_the_trash() {
        let t = TempDir::new("replace-trash");
        let trash =
            Trash { home: t.join("Trash"), home_dir: Some(t.join("home")), uid: sys::uid(), topdirs: Some(Vec::new()) };
        let dest = t.dir("dest");
        t.file("dest/folder/old-only.txt", "kept in the trash");
        let src = t.file("new/folder/new.txt", "new");
        let control = Control::new();
        let mut asker = Always(Resolution::Replace, ErrorChoice::Cancel);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        let r = transfer(&[src.parent().unwrap().to_path_buf()], &dest, Mode::Copy, &mut env, Some(&trash));
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(read(dest.join("folder/new.txt")), "new");
        assert!(!dest.join("folder/old-only.txt").exists(), "a folder replaced is replaced, not merged");
        let entry = trash.list().into_iter().next().expect("the old folder in the Trash");
        assert_eq!(entry.original.as_deref(), Some(dest.join("folder").as_path()));
        assert_eq!(read(entry.file.join("old-only.txt")), "kept in the trash");
        no_temporaries(&dest);
    }

    #[test]
    fn merges_folders() {
        let t = TempDir::new("merge");
        let dest = t.dir("dest");
        t.file("dest/f/keep.txt", "keep");
        t.file("dest/f/same.txt", "old");
        t.file("src/f/same.txt", "new");
        t.file("src/f/added.txt", "added");
        let mut s = Script { conflicts: [Resolution::Merge, Resolution::Replace].into(), ..Default::default() };
        let r = run(&[t.join("src/f")], &dest, Mode::Move, &mut s);
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(s.asked, ["conflict f", "conflict same.txt"]);
        assert_eq!(read(dest.join("f/keep.txt")), "keep");
        assert_eq!(read(dest.join("f/same.txt")), "new");
        assert_eq!(read(dest.join("f/added.txt")), "added");
        assert!(!t.join("src/f").exists(), "a moved folder emptied by merging goes");
    }

    #[test]
    fn moves_across_filesystems_copy_sync_then_delete() {
        let t = TempDir::new("xdev");
        let Some(other) = TempDir::elsewhere("xdev") else {
            eprintln!("no second filesystem here: skipped");
            return;
        };
        let src = tree(&t, "src");
        let big = t.file("big.bin", &"x".repeat(3 * 1024 * 1024));
        let r = quiet(&[src.clone(), big.clone()], other.path(), Mode::Move);
        assert!(r.is_clean(), "{r:?}");
        assert_tree(&other.join("src"));
        assert_eq!(fs::metadata(other.join("big.bin")).unwrap().len(), 3 * 1024 * 1024);
        assert!(!src.exists() && !big.exists(), "the sources go once their copies are in place");
        no_temporaries(other.path());
        // And back, with a conflict replaced there.
        t.file("src/a.txt", "stale");
        let mut s = Script { conflicts: [Resolution::Merge, Resolution::Replace].into(), ..Default::default() };
        let r = run(&[other.join("src")], t.path(), Mode::Move, &mut s);
        assert!(r.is_clean(), "{r:?}");
        assert_tree(&src);
        assert!(!other.join("src").exists());
    }

    #[test]
    fn a_move_across_filesystems_keeps_what_did_not_arrive() {
        if is_root() {
            return;
        }
        let t = TempDir::new("xdev-partial");
        let Some(other) = TempDir::elsewhere("xdev-partial") else { return };
        let src = tree(&t, "src");
        let secret = t.file("src/sub/secret.txt", "unreadable");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
        let mut s = Script { errors: [ErrorChoice::Skip].into(), ..Default::default() };
        let r = run(std::slice::from_ref(&src), other.path(), Mode::Move, &mut s);
        assert_eq!(s.asked, ["error read secret.txt"]);
        assert_eq!(r.problems.len(), 1, "{r:?}");
        assert_eq!(r.done.len(), 1);
        // What came went; what didn't stays, with its folders.
        assert_eq!(read(other.join("src/a.txt")), "alpha");
        assert!(!src.join("a.txt").exists());
        assert!(!src.join("sub/b.txt").exists());
        assert!(fs::symlink_metadata(&secret).is_ok());
        assert!(!other.join("src/sub/secret.txt").exists());
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn errors_retry_skip_and_cancel() {
        if is_root() {
            return;
        }
        let t = TempDir::new("errors");
        let locked = t.file("src/locked.txt", "l");
        t.file("src/open.txt", "o");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let dest = t.dir("dest");
        // Retry once (still locked), then skip: the rest comes.
        let mut s = Script { errors: [ErrorChoice::Retry, ErrorChoice::Skip].into(), ..Default::default() };
        let r = run(&[t.join("src")], &dest, Mode::Copy, &mut s);
        assert_eq!(s.asked, ["error read locked.txt", "error read locked.txt"]);
        assert_eq!(read(dest.join("src/open.txt")), "o");
        assert!(!dest.join("src/locked.txt").exists());
        assert_eq!(r.problems.len(), 1);
        // Cancel takes the partial copy away.
        let mut s = Script { errors: [ErrorChoice::Cancel].into(), ..Default::default() };
        let r = run(&[t.join("src")], &t.dir("dest2"), Mode::Copy, &mut s);
        assert!(r.cancelled);
        assert_eq!(fs::read_dir(t.join("dest2")).unwrap().count(), 0, "nothing half-made is left");
        // A folder it can't write into.
        let ro = t.dir("ro");
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        let mut s = Script { errors: [ErrorChoice::Skip].into(), ..Default::default() };
        let r = run(&[t.join("src/open.txt")], &ro, Mode::Copy, &mut s);
        assert_eq!(r.failed.len(), 1);
        assert!(r.failed[0].1.contains("Permission denied"), "{r:?}");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn cancelling_mid_file_leaves_nothing() {
        let t = TempDir::new("cancel");
        let big = t.file("big.bin", &"y".repeat(sys::CHUNK * 3));
        let dest = t.dir("dest");
        let control = Control::new();
        let mut asker = Always(Resolution::Cancel, ErrorChoice::Cancel);
        let mut chunks = 0;
        let mut reporter = |p: &Progress| {
            if p.bytes_done > 0 {
                chunks += 1;
                if chunks == 2 {
                    control.cancel();
                }
            }
        };
        let mut env = Env::new(&control, &mut asker, &mut reporter);
        let r = transfer(std::slice::from_ref(&big), &dest, Mode::Copy, &mut env, None);
        assert!(r.cancelled);
        assert_eq!(fs::read_dir(&dest).unwrap().count(), 0, "the partial file is gone");
        assert!(big.exists(), "the source stays");
    }

    #[test]
    fn special_files_are_reported() {
        let t = TempDir::new("special");
        let fifo = t.join("src/pipe");
        t.dir("src");
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a valid path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        t.file("src/plain", "p");
        let mut s = Script { errors: [ErrorChoice::Skip].into(), ..Default::default() };
        let r = run(&[t.join("src")], &t.dir("dest"), Mode::Copy, &mut s);
        assert_eq!(s.asked, ["error copy pipe"]);
        assert_eq!(read(t.join("dest/src/plain")), "p");
        assert_eq!(r.problems.len(), 1);
    }

    #[test]
    fn progress_counts_bytes_and_items() {
        let t = TempDir::new("progress");
        let src = tree(&t, "src");
        let control = Control::new();
        let mut asker = Always(Resolution::Cancel, ErrorChoice::Cancel);
        let mut seen = Vec::new();
        let mut reporter = |p: &Progress| seen.push(p.clone());
        let mut env = Env::new(&control, &mut asker, &mut reporter);
        transfer(&[src], &t.dir("dest"), Mode::Copy, &mut env, None);
        let last = seen.last().unwrap();
        assert_eq!(last.bytes_total, 5 + 5 + 7);
        assert_eq!(last.bytes_done, last.bytes_total);
        assert_eq!(last.items_total, 7);
        assert_eq!(last.items_done, 7);
        assert!(seen.iter().any(|p| p.preparing));
        assert_eq!(last.fraction(), 1.0);
    }

    #[test]
    fn knows_mount_points() {
        let t = TempDir::new("mounts");
        let d = t.dir("plain");
        assert!(!is_mount_point(&d, &fs::symlink_metadata(&d).unwrap()));
        // Read only: /dev/shm is its own filesystem where it's mounted.
        let shm = Path::new("/dev/shm");
        if let (Ok(m), Ok(p)) = (fs::symlink_metadata(shm), fs::metadata("/dev"))
            && m.dev() != p.dev()
        {
            assert!(is_mount_point(shm, &m));
        }
    }

    #[test]
    fn trash_restore_and_erase() {
        let t = TempDir::new("trash-ops");
        let trash = Trash {
            home: t.join("data/Trash"),
            home_dir: Some(t.join("home")),
            uid: sys::uid(),
            topdirs: Some(Vec::new()),
        };
        let a = t.file("home/docs/a.txt", "a");
        let folder = tree(&t, "home/docs/folder");
        let control = Control::new();
        let mut asker = Always(Resolution::KeepBoth, ErrorChoice::Skip);
        let mut quiet = ();
        let mut env = Env::new(&control, &mut asker, &mut quiet);
        let (r, trashed) = trash_items(&[a.clone(), folder.clone(), t.join("home")], &trash, &mut env);
        assert_eq!(trashed.len(), 2);
        assert_eq!(r.failed.len(), 1, "the home folder isn't trashed");
        assert!(!a.exists() && !folder.exists());
        // Restore, with the folders on the way gone and a new a.txt there.
        fs::remove_dir(t.join("home/docs")).unwrap();
        t.file("home/docs/a.txt", "newer a");
        let entries = trash.list();
        let r = restore(&entries, &trash, &mut Env::new(&control, &mut asker, &mut ()));
        assert!(r.is_clean(), "{r:?}");
        assert_eq!(read(t.join("home/docs/a.txt")), "newer a");
        assert_eq!(read(t.join("home/docs/a 2.txt")), "a", "kept both");
        assert_tree(&folder);
        assert!(trash.list().is_empty());
        assert_eq!(fs::read_dir(t.join("data/Trash/info")).unwrap().count(), 0);
        // Erase refuses what isn't in a can, and empties what is.
        trash_items(std::slice::from_ref(&folder), &trash, &mut Env::new(&control, &mut asker, &mut ()));
        let mut fake = trash.list()[0].clone();
        fake.file = t.join("home/docs/a.txt");
        fake.name = "a.txt".into();
        let r = erase(&[fake], &trash, &mut Env::new(&control, &mut asker, &mut ()));
        assert_eq!(r.failed.len(), 1);
        assert!(t.join("home/docs/a.txt").exists());
        t.file("data/Trash/info/orphan.trashinfo", "[Trash Info]\nPath=/x\n");
        let r = empty_trash(&trash, &mut Env::new(&control, &mut asker, &mut ()));
        assert!(r.is_clean(), "{r:?}");
        assert!(trash.is_empty());
        assert_eq!(fs::read_dir(t.join("data/Trash/info")).unwrap().count(), 0);
    }
}
