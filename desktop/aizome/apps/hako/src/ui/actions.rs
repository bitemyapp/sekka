//! What the program does for its messages beyond the basics in `app`:
//! going places, selections, keys, drags and drops, search, disks, the
//! system's services, and answering jobs' questions.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::KeyEvent;

use super::app::{Hako, Msg, Sheet};
use super::browser::{Drag, Load, Loc, Search};
use super::dnd::{Dest, DragEvent, Target};
use super::fileview::{Layout, ViewKey};
use super::jobs::{Answer, Work, drop_mode};
use crate::fsops::is_within;
use crate::fsops::transfer::Mode;
use crate::model::entry::{self, Entry};
use crate::model::uri;

impl Hako {
    /// Go to a path: a folder, or a file in its folder.
    pub(crate) fn go_path(&mut self, cx: &mut Cx<Msg>, w: WindowKey, p: &Path) {
        if p.is_dir() {
            let loc = self.loc_for(p);
            self.go(cx, w, loc, None, true);
        } else if let (Some(dir), Some(name)) = (p.parent(), p.file_name()) {
            let loc = self.loc_for(dir);
            self.go(cx, w, loc, Some(name.to_os_string()), true);
        }
    }

    pub(crate) fn history_step(&mut self, cx: &mut Cx<Msg>, w: WindowKey, back: bool) {
        let w = self.owner(w);
        let Some(b) = self.windows.get_mut(&w) else { return };
        let here =
            b.location().cloned().map(|l| (l, b.columns.last().and_then(|c| c.single().map(|e| e.name.clone()))));
        let next = if back { b.back.pop() } else { b.forward.pop() };
        let Some((loc, sel)) = next else { return };
        if let Some(h) = here {
            if back { b.forward.push(h) } else { b.back.push(h) }
        }
        self.go(cx, w, loc, sel, false);
    }

    /// A folder just made shows: rename it.
    pub(crate) fn maybe_rename_new(&mut self, cx: &mut Cx<Msg>, w: WindowKey) {
        let Some((rw, path)) = self.rename_when_loaded.clone() else { return };
        if rw != w {
            return;
        }
        let Some(b) = self.windows.get(&w) else { return };
        let ready = b.focused().is_some_and(|c| c.load == Load::Ready && c.single().is_some_and(|e| e.path == path));
        if ready {
            self.rename_when_loaded = None;
            self.start_rename(cx, w);
        }
    }

    pub(crate) fn on_select(&mut self, cx: &mut Cx<Msg>, w: WindowKey, col: usize, rows: Vec<usize>) {
        let g = self.next_gen();
        let Some(b) = self.windows.get_mut(&w) else { return };
        if b.rename.as_ref().is_some_and(|r| !(r.column == col && rows == [r.row])) {
            self.commit_rename(cx, w);
        }
        let Some(b) = self.windows.get_mut(&w) else { return };
        let opened = b.select(col, rows, g);
        if opened {
            self.start_loads(cx, w);
        }
        self.after_change(cx, w);
    }

    pub(crate) fn on_view_key(&mut self, cx: &mut Cx<Msg>, w: WindowKey, col: usize, k: ViewKey) {
        match k {
            ViewKey::Into => {
                if let Some(b) = self.windows.get_mut(&w)
                    && b.into_next(col)
                {
                    let f = b.focus;
                    cx.focus(&format!("col-{f}"));
                    self.after_change(cx, w);
                }
            }
            ViewKey::Out => {
                if let Some(b) = self.windows.get_mut(&w)
                    && b.out_of(col)
                {
                    let f = b.focus;
                    cx.focus(&format!("col-{f}"));
                    self.after_change(cx, w);
                }
            }
            ViewKey::QuickLook => cx.send(Msg::QuickLook(w)),
            ViewKey::Rename => self.start_rename(cx, w),
            ViewKey::ClickAway => {
                // A click away from a name being edited keeps it.
                self.commit_rename(cx, w);
            }
            ViewKey::Escape => {
                let renaming = self.windows.get(&w).is_some_and(|b| b.rename.is_some());
                // A sheet over the window closes first (the view kept the
                // keyboard under it).
                let sheet = self
                    .sheets
                    .iter()
                    .find(|(_, s)| match s {
                        Sheet::Info { window, .. }
                        | Sheet::QuickLook { window, .. }
                        | Sheet::Confirm { window, .. } => *window == w,
                        _ => false,
                    })
                    .map(|(k, _)| *k);
                if let Some(s) = sheet {
                    self.sheets.remove(&s);
                    cx.close_window(s);
                } else if renaming {
                    cx.send(Msg::RenameCancel(w));
                } else if let Some(b) = self.windows.get_mut(&w) {
                    b.drag = None;
                }
            }
        }
    }

    /// The paths selected in window `w`.
    pub(crate) fn selection_of(&self, w: WindowKey) -> Vec<PathBuf> {
        self.windows.get(&w).map(|b| b.selection()).unwrap_or_default()
    }

    pub(crate) fn any_window(&self) -> WindowKey {
        self.windows.keys().next().copied().unwrap_or(WindowKey::NONE)
    }

    // Drags and drops.

    /// Whether dropping `sources` at `dest` would do anything.
    pub(crate) fn drop_allowed(&self, sources: &[PathBuf], dest: &Dest) -> bool {
        match dest {
            Dest::Trash => !sources.iter().any(|s| self.opts.trash.in_a_can(s)),
            Dest::Folder(d) => {
                !sources.is_empty()
                    && sources.iter().all(|s| !is_within(d, s))
                    && !sources.iter().all(|s| s.parent() == Some(d.as_path()))
                    && !self.opts.trash.in_a_can(d)
            }
        }
    }

    pub(crate) fn on_drag(&mut self, cx: &mut Cx<Msg>, w: WindowKey, col: usize, ev: DragEvent) {
        match ev {
            DragEvent::Start => {
                let sources = self.selection_of(w);
                if let Some(b) = self.windows.get_mut(&w) {
                    b.drag = (!sources.is_empty()).then_some(Drag { sources, target: None, from: col, spring: 0 });
                }
            }
            DragEvent::Over(t) => {
                let allowed = match (&t, self.windows.get(&w).and_then(|b| b.drag.as_ref())) {
                    (Some(t), Some(d)) => self.drop_allowed(&d.sources, &t.dest),
                    _ => false,
                };
                let columns = self.windows.get(&w).is_some_and(|b| b.layout == Layout::Column && b.search.is_none());
                if let Some(d) = self.windows.get_mut(&w).and_then(|b| b.drag.as_mut()) {
                    d.target = if allowed { t } else { None };
                    d.spring += 1;
                    // Waiting over a folder in a column (this one or one
                    // after it, which opening it can't close) opens it.
                    if let Some(t) = &d.target
                        && columns
                        && t.row.is_some()
                        && t.spot < super::dnd::SIDEBAR
                        && t.spot as usize >= d.from
                    {
                        cx.after(Duration::from_millis(900), Msg::Spring(w, t.clone(), d.spring));
                    }
                }
            }
            DragEvent::Drop { target, copy } => {
                let Some(drag) = self.windows.get_mut(&w).and_then(|b| b.drag.take()) else { return };
                let Some(t) = target else { return };
                if !self.drop_allowed(&drag.sources, &t.dest) {
                    return;
                }
                match t.dest {
                    Dest::Trash => self.start_job(cx, w, Work::Trash { paths: drag.sources }),
                    Dest::Folder(d) => {
                        let mode = drop_mode(&drag.sources, &d, copy);
                        self.start_job(cx, w, Work::Transfer { sources: drag.sources, dest: d, mode });
                    }
                }
            }
            DragEvent::Cancel => {
                if let Some(b) = self.windows.get_mut(&w) {
                    b.drag = None;
                }
            }
            DragEvent::Leave(_) => {
                let Some(drag) = self.windows.get_mut(&w).and_then(|b| b.drag.take()) else { return };
                self.drag_out(cx, w, drag.sources);
            }
        }
    }

    /// A drag of `sources` left window `w`: the desktop carries it on to
    /// other programs, the Shelf and the desktop, as a list of files they
    /// may copy (never move: Hako deletes nothing for another program).
    /// Back over a Hako window it's Hako's own drag again (see
    /// `on_drop_urls`).
    pub(crate) fn drag_out(&mut self, cx: &mut Cx<Msg>, w: WindowKey, sources: Vec<PathBuf>) {
        let urls: Vec<String> = sources.iter().map(|p| uri::file_uri(p)).collect();
        // The chip under the pointer: the item (or how many), drawn when
        // the drag starts at the window's scale.
        let first = sources.first().and_then(|p| Entry::read(p).ok());
        let (size, chip) = super::dragout::chip(cx.theme(), first.as_ref(), sources.len());
        let hotspot = Point::new(12.0, (super::dragout::HEIGHT / 2.0).round());
        let source = DragSource::new().urls(urls).actions(true, false).icon_drawn(size, hotspot, chip);
        if let Some(drag) = cx.start_drag(w, source) {
            self.outbound = Some((drag, sources));
        }
    }

    /// A drag waited over a folder: open it in the next column.
    pub(crate) fn spring(&mut self, cx: &mut Cx<Msg>, w: WindowKey, target: Target, n: u64) {
        let g = self.next_gen();
        let Some(b) = self.windows.get_mut(&w) else { return };
        let still = b.drag.as_ref().is_some_and(|d| d.spring == n && d.target.as_ref() == Some(&target));
        let (Some(row), true) = (target.row, still) else { return };
        let col = target.spot as usize;
        if b.select(col, vec![row], g) {
            self.start_loads(cx, w);
        }
        self.after_change(cx, w);
    }

    /// Files dropped from another program.
    pub(crate) fn on_drop_urls(&mut self, cx: &mut Cx<Msg>, w: WindowKey, dest: Dest, urls: Vec<String>) {
        let sources: Vec<PathBuf> =
            urls.iter().filter_map(|u| uri::path_of(u)).filter(|p| std::fs::symlink_metadata(p).is_ok()).collect();
        if sources.is_empty() || !self.drop_allowed(&sources, &dest) {
            return;
        }
        // Hako's own drag, back from the desktop, moves within a disk as
        // drags inside a window do; what other programs drop is copied, as
        // they may still want it.
        let ours = self.outbound.as_ref().is_some_and(|(_, o)| *o == sources);
        match dest {
            Dest::Trash => self.start_job(cx, w, Work::Trash { paths: sources }),
            Dest::Folder(d) => {
                let mode = if ours { drop_mode(&sources, &d, false) } else { Mode::Copy };
                self.start_job(cx, w, Work::Transfer { sources, dest: d, mode });
            }
        }
    }

    // Sorting.

    pub(crate) fn resort(&mut self, cx: &mut Cx<Msg>) {
        let sort = self.settings.sort;
        let hidden = self.settings.hidden;
        for b in self.windows.values_mut() {
            for c in &mut b.columns {
                if c.load == Load::Ready && c.loc != Loc::Recent {
                    let mut all = (*c.all).clone();
                    entry::sort_entries(&mut all, sort);
                    c.set_entries(Arc::new(all), hidden);
                }
            }
        }
        self.save_settings();
        let _ = cx;
    }

    pub(crate) fn save_settings(&self) {
        if let Some(p) = &self.opts.settings {
            self.settings.save(p);
        }
    }

    // Search.

    pub(crate) fn search(&mut self, cx: &mut Cx<Msg>, w: WindowKey, q: String) {
        let g = self.next_gen();
        let root = self
            .windows
            .get(&w)
            .and_then(|b| {
                b.search.as_ref().map(|s| s.root.clone()).or_else(|| b.target_folder().map(Path::to_path_buf))
            })
            .unwrap_or_else(|| self.opts.home.clone());
        let Some(b) = self.windows.get_mut(&w) else { return };
        if q.trim().is_empty() {
            b.search = None;
            self.after_change(cx, w);
            return;
        }
        match b.search.as_mut() {
            Some(s) => {
                s.query = q.clone();
                s.generation = g;
                s.running = true;
            }
            None => {
                b.search = Some(Search {
                    query: q.clone(),
                    root: root.clone(),
                    results: Rc::new(Vec::new()),
                    selected: Rc::new(Vec::new()),
                    generation: g,
                    running: true,
                });
            }
        }
        b.rename = None;
        let index = self
            .search_index
            .get(&root)
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(120))
            .map(|(_, i)| i.clone());
        let tx = cx.stream();
        let (services, home) = (self.opts.services, self.opts.home.clone());
        std::thread::spawn(move || {
            let index = index.unwrap_or_else(|| Arc::new(build_index(&root, &home, services)));
            let hits = index.search(&q, 400, &|_| 0);
            let entries: Vec<Entry> = hits
                .into_iter()
                .filter(|h| h.path.starts_with(&root))
                .filter_map(|h| Entry::read(&h.path).ok())
                .collect();
            tx.send(Msg::SearchDone(w, g, Arc::new(entries)));
            tx.send(Msg::SearchIndex(root, super::app::Shared(index)));
        });
        self.update_title(cx, w);
    }

    // Sizes and previews.

    /// Count a folder's items and bytes, in the background.
    pub(crate) fn measure(&mut self, cx: &mut Cx<Msg>, p: &Path) {
        let p = p.to_path_buf();
        cx.spawn(move || {
            let (n, b) = crate::fsops::delete::measure(&p, &crate::fsops::Control::new(), 2_000_000).unwrap_or((0, 0));
            Msg::FolderSize(p, n.saturating_sub(1), b)
        });
    }

    pub(crate) fn load_full_preview(&mut self, cx: &mut Cx<Msg>, e: &Entry) {
        self.load_preview(cx, e, true);
    }

    pub(crate) fn thumb_done(&mut self) {
        self.thumb_running = self.thumb_running.saturating_sub(1);
    }

    pub(crate) fn run_thumbs_again(&mut self, cx: &mut Cx<Msg>) {
        // Only what's still on screen is worth making.
        let shown: std::collections::HashSet<PathBuf> = self
            .windows
            .values()
            .flat_map(|b| b.columns.iter().flat_map(|c| c.rows.iter().map(|e| e.path.clone())))
            .collect();
        self.thumb_wanted.retain(|(p, _)| shown.contains(p));
        self.run_thumbs(cx);
    }

    // Disks.

    pub(crate) fn disks_changed(&mut self, cx: &mut Cx<Msg>) {
        if let Some(d) = &self.disks {
            self.volumes = d.snapshot().volumes();
        }
        let mut unlocked = Vec::new();
        self.unlock_tasks.retain(|(sheet, block, task)| match task.try_result() {
            Some(result) => {
                unlocked.push((*sheet, block.clone(), result));
                false
            }
            None => true,
        });
        for (sheet, block, result) in unlocked {
            self.mounting.remove(&block);
            match result {
                Ok(cleartext) => {
                    if let Some(Sheet::Unlock { window, .. }) = self.sheets.remove(&sheet) {
                        cx.close_window(sheet);
                        if self.windows.contains_key(&window) {
                            cx.activate(window);
                            self.mount(cx, cleartext, window);
                        }
                    }
                }
                Err(e) => {
                    if let Some(Sheet::Unlock { error, busy, .. }) = self.sheets.get_mut(&sheet) {
                        *busy = false;
                        *error = e.to_string();
                        cx.activate(sheet);
                        cx.focus("unlock-pass");
                    }
                }
            }
        }
        let mut done = Vec::new();
        self.mount_tasks.retain(|(block, window, task)| match task.try_result() {
            Some(r) => {
                done.push((block.clone(), *window, r));
                false
            }
            None => true,
        });
        for (block, window, r) in done {
            self.mounting.remove(&block);
            match r {
                Ok(point) if self.windows.contains_key(&window) => {
                    self.go(cx, window, Loc::Folder(PathBuf::from(point)), None, true);
                }
                Ok(_) => {}
                Err(e) if self.windows.contains_key(&window) => {
                    self.open_sheet(
                        cx,
                        window,
                        Sheet::Message { title: "The disk couldn't be mounted.".into(), body: e.to_string() },
                    );
                }
                Err(_) => {}
            }
        }
        let mut ejected = Vec::new();
        self.eject_tasks.retain(|(block, drive, task)| match task.try_result() {
            Some(r) => {
                ejected.push((block.clone(), drive.clone(), r));
                false
            }
            None => true,
        });
        for (block, drive, r) in ejected {
            match (r, drive) {
                (Ok(()), Some(drive)) => {
                    // Unmounted: now the drive.
                    if let Some(d) = &self.disks {
                        let t = d.eject(&drive);
                        self.eject_tasks.push((block, None, t));
                    }
                }
                (Ok(()), None) => {
                    self.mounting.remove(&block);
                }
                (Err(e), _) => {
                    self.mounting.remove(&block);
                    self.message(cx, "The disk couldn't be ejected.", &e.to_string());
                }
            }
        }
    }

    pub(crate) fn mount(&mut self, cx: &mut Cx<Msg>, block: String, window: WindowKey) {
        if self.mounting.contains(&block) {
            return;
        }
        if let Some(v) = self.volumes.iter().find(|v| v.block == block && v.locked) {
            if let Some((&sheet, _)) =
                self.sheets.iter().find(|(_, s)| matches!(s, Sheet::Unlock { block: b, .. } if *b == block))
            {
                cx.activate(sheet);
                return;
            }
            let name = v.name.clone();
            self.open_sheet(
                cx,
                window,
                Sheet::Unlock { window, block, name, pass: String::new(), error: String::new(), busy: false },
            );
            cx.focus("unlock-pass");
            return;
        }
        let Some(d) = &self.disks else { return };
        if let Some(v) = self.volumes.iter().find(|v| v.block == block)
            && let Some(m) = &v.mount_point
        {
            let _ = m;
            return;
        }
        self.mounting.insert(block.clone());
        let task = d.mount(&block);
        self.mount_tasks.push((block, window, task));
    }

    pub(crate) fn unlock(&mut self, sheet: WindowKey) {
        let Some(d) = &self.disks else { return };
        let Some(Sheet::Unlock { block, pass, error, busy, .. }) = self.sheets.get_mut(&sheet) else { return };
        if *busy || pass.is_empty() {
            return;
        }
        *busy = true;
        error.clear();
        let passphrase = std::mem::take(pass);
        let task = d.unlock(block, &passphrase);
        self.mounting.insert(block.clone());
        self.unlock_tasks.push((sheet, block.clone(), task));
    }

    pub(crate) fn eject(&mut self, block: String) {
        let Some(d) = &self.disks else { return };
        let Some(v) = self.volumes.iter().find(|v| v.block == block).cloned() else { return };
        // Windows showing it go home first.
        let home = self.opts.home.clone();
        if let Some(m) = &v.mount_point {
            for b in self.windows.values_mut() {
                if b.columns.iter().any(|c| c.loc.folder().is_some_and(|f| f.starts_with(m))) {
                    b.go(Loc::Folder(home.clone()), &Loc::Folder(home.clone()), None, 0);
                }
            }
        }
        // Unmount, then (once that's done) eject the drive.
        let drive = v.drive.clone().filter(|_| v.ejectable);
        let task = if v.mount_point.is_some() {
            d.unmount(&block, false)
        } else if let Some(drive) = &drive {
            let t = d.eject(drive);
            self.mounting.insert(block.clone());
            self.eject_tasks.push((block, None, t));
            return;
        } else {
            return;
        };
        self.mounting.insert(block.clone());
        self.eject_tasks.push((block, drive, task));
    }

    pub(crate) fn message(&mut self, cx: &mut Cx<Msg>, title: &str, body: &str) {
        let w = self.any_window();
        if self.windows.contains_key(&w) {
            self.open_sheet(cx, w, Sheet::Message { title: title.into(), body: body.into() });
        }
    }

    // The system.

    pub(crate) fn start_services(&mut self, cx: &mut Cx<Msg>) {
        // Applications, for Open With.
        cx.spawn(|| Msg::Apps(Arc::new(aizome_sys::apps::AppIndex::load())));
        // Disks.
        let tx = cx.stream();
        let disks = aizome_sys::disks::Disks::new(aizome_sys::Wake::new(move || {
            tx.send(Msg::DisksChanged);
        }));
        self.volumes = disks.snapshot().volumes();
        self.disks = Some(disks);
        self.system_disk = crate::model::places::system_disk(&self.opts.home);
        // The Desks, from the compositor.
        let tx = cx.stream();
        std::thread::Builder::new().name("hako-desks".into()).spawn(move || follow_desks(tx)).ok();
    }

    // Jobs' questions and the operations window.

    /// Answer job `id`'s question (none: cancel it).
    pub(crate) fn answer(&mut self, cx: &mut Cx<Msg>, id: u64, a: Option<Answer>) {
        let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) else { return };
        let Some(q) = j.question.take() else { return };
        let a = a.unwrap_or(match q {
            super::jobs::Question::Conflict(_) => Answer::Conflict(crate::fsops::Resolution::Cancel, false),
            super::jobs::Question::Failure(_) => Answer::Error(crate::fsops::ErrorChoice::Cancel, false),
        });
        let _ = j.answers.send(a);
        if let Some(s) = j.sheet.take() {
            self.sheets.remove(&s);
            cx.close_window(s);
        }
    }

    pub(crate) fn show_operations(&mut self, cx: &mut Cx<Msg>) {
        if self.ops_window.is_some() || self.jobs.is_empty() {
            return;
        }
        let w = cx.open_window(
            WindowSpec::new("Operations").size(440.0, 96.0 + 64.0 * self.jobs.len().min(5) as f64).utility(),
        );
        self.ops_window = Some(w);
    }

    // Keys nothing else took.

    pub(crate) fn on_key(&mut self, cx: &mut Cx<Msg>, w: WindowKey, k: &KeyEvent) {
        use aizome_kit::sidestep_ui::{Key, NamedKey};
        let w = self.owner(w);
        let Some(sheet) = self
            .sheets
            .iter()
            .find(|(_, s)| matches!(s, Sheet::QuickLook { window, .. } if *window == w))
            .map(|(k, _)| *k)
        else {
            return;
        };
        let step: isize = match k.key {
            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowRight) => 1,
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowLeft) => -1,
            _ => return,
        };
        // Move the selection of what's shown, and show the next item.
        let g = self.next_gen();
        let Some(b) = self.windows.get_mut(&w) else { return };
        let next = if let Some(s) = b.search.as_mut() {
            let n = s.results.len();
            let cur = s.selected.last().copied().unwrap_or(0);
            let i = (cur as isize + step).clamp(0, n as isize - 1) as usize;
            s.selected = Rc::new(vec![i]);
            s.results.get(i).cloned()
        } else {
            let col = b.focus;
            let Some(c) = b.columns.get(col) else { return };
            let n = c.rows.len();
            if n == 0 {
                return;
            }
            let cur = c.selected.last().copied().unwrap_or(0);
            let i = (cur as isize + step).clamp(0, n as isize - 1) as usize;
            let e = c.rows.get(i).cloned();
            if b.select(col, vec![i], g) {
                self.start_loads(cx, w);
            }
            if let Some(c) = self.windows.get_mut(&w).and_then(|b| b.columns.get_mut(col)) {
                c.scroll = Some((i, g));
            }
            e
        };
        let Some(e) = next else { return };
        if e.is_folder() {
            self.measure(cx, &e.path);
        } else {
            self.load_full_preview(cx, &e);
        }
        if let Some(Sheet::QuickLook { entry, .. }) = self.sheets.get_mut(&sheet) {
            **entry = e;
        }
        self.after_change(cx, w);
    }
}

/// Follow the compositor's Desks, telling the program the current one.
fn follow_desks(tx: aizome_kit::MsgSender<Msg>) {
    use aizome_ipc::{Client, Event, Request, Response};
    let fetch = |c: &mut Client| match c.request(&Request::Desks) {
        Ok(Response::Desks { desks, .. }) => Some(desks),
        _ => None,
    };
    let Ok(mut client) = Client::connect() else { return };
    if let Some(d) = fetch(&mut client) {
        tx.send(Msg::Desks(d));
    }
    let Ok(events) = Client::connect().and_then(|c| c.events()) else { return };
    for ev in events {
        match ev {
            Ok(Event::DeskChanged { .. } | Event::DesksChanged { .. }) => {
                if let Some(d) = fetch(&mut client)
                    && !tx.send(Msg::Desks(d))
                {
                    return;
                }
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
    let _ = Instant::now();
}

/// An index of `root` for searching: the shared one (the palette's, of
/// the home folder) where `root` is in the home folder and it's fresh,
/// else a walk of `root`.
fn build_index(root: &Path, home: &Path, shared: bool) -> aizome_search::files::FileIndex {
    use aizome_search::files::{FileIndex, Limits};
    if shared
        && root.starts_with(home)
        && let Some(i) = FileIndex::load(&FileIndex::default_cache_path())
        && i.age().is_some_and(|a| a < Duration::from_secs(3600))
        && !i.is_empty()
    {
        return i;
    }
    FileIndex::build(&[root.to_path_buf()], &Limits { max_entries: 200_000, max_depth: 16, ..Limits::default() })
}

impl Hako {
    /// Window `gone` closed: the questions jobs asked over it ask again
    /// over another window (or the operations window), or, with none
    /// left, stop their jobs.
    pub(crate) fn reparent_questions(&mut self, cx: &mut Cx<Msg>, gone: WindowKey) {
        // A completed unlock must not navigate a different browser, or leave
        // a stale prompt blocking a later request after its owner closes.
        let unlocks: Vec<_> = self
            .sheets
            .iter()
            .filter_map(|(key, sheet)| matches!(sheet, Sheet::Unlock { window, .. } if *window == gone).then_some(*key))
            .collect();
        for sheet in unlocks {
            self.sheets.remove(&sheet);
            cx.close_window(sheet);
        }
        let asking: Vec<(u64, WindowKey)> =
            self.jobs.iter().filter(|j| j.window == gone).filter_map(|j| j.sheet.map(|s| (j.id, s))).collect();
        for (id, sheet) in asking {
            self.sheets.remove(&sheet);
            let to = self.windows.keys().next().copied().or(self.ops_window).or_else(|| {
                self.show_operations(cx);
                self.ops_window
            });
            match to {
                Some(p) => {
                    let s = self.open_sheet(cx, p, Sheet::Ask { job: id, all: false });
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.sheet = Some(s);
                        j.window = p;
                    }
                }
                None => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.sheet = None;
                    }
                    self.answer(cx, id, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod encrypted_tests {
    use super::*;
    use aizome_kit::testing::Harness;
    use aizome_sys::Task;

    #[test]
    fn mount_completion_keeps_its_original_window_and_does_not_reopen_a_closed_one() {
        let root = std::env::temp_dir().join(format!("hako-mount-target-{}", std::process::id()));
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("mounted")).unwrap();
        let mut h = Harness::new(Hako::new(super::super::app::Options::sandbox(&root)));
        let first = h.window();
        h.send(Msg::NewWindow);
        let second = *h.program().windows.keys().find(|w| **w != first).unwrap();
        h.focus_window(second, true);
        h.program_mut().mount_tasks.push((
            "cleartext".into(),
            first,
            Task::ready(Ok(root.join("mounted").display().to_string())),
        ));
        h.send(Msg::DisksChanged);
        assert_eq!(h.program().windows[&first].location(), Some(&Loc::Folder(root.join("mounted"))));
        assert_eq!(h.program().windows[&second].location(), Some(&Loc::Folder(root.join("home"))));
        h.send(Msg::CloseWindow(first));
        h.program_mut().mount_tasks.push((
            "cleartext".into(),
            first,
            Task::ready(Ok(root.join("mounted").display().to_string())),
        ));
        h.send(Msg::DisksChanged);
        assert_eq!(h.program().windows.len(), 1);
        assert_eq!(h.program().windows[&second].location(), Some(&Loc::Folder(root.join("home"))));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unlock_result_after_owner_closes_does_not_start_a_mount() {
        let root = std::env::temp_dir().join(format!("hako-unlock-target-{}", std::process::id()));
        std::fs::create_dir_all(root.join("home")).unwrap();
        let mut h = Harness::new(Hako::new(super::super::app::Options::sandbox(&root)));
        let owner = h.window();
        h.program_mut().volumes.push(aizome_sys::disks::Volume {
            block: "locked".into(),
            locked: true,
            ..Default::default()
        });
        h.send(Msg::Mount("locked".into()));
        let sheet = *h.program().sheets.keys().next().unwrap();
        h.send(Msg::CloseWindow(owner));
        assert!(h.program().sheets.is_empty());
        h.program_mut().unlock_tasks.push((sheet, "locked".into(), Task::ready(Ok("cleartext".into()))));
        h.send(Msg::DisksChanged);
        assert!(h.program().windows.is_empty());
        assert!(h.program().sheets.is_empty());
        assert!(h.program().mount_tasks.is_empty());
        assert!(h.program().mounting.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
