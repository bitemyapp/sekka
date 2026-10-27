//! Hako as a kit [`Program`]: launching, updating, and which view each
//! window gets.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use aizome_kit::prelude::*;

use super::app::{Confirm, Hako, Msg, Sheet};
use super::browser::Loc;
use super::fileview::Layout;
use super::jobs::{Answer, Work};
use crate::fsops::ErrorChoice;
use crate::model::entry::{Entry, SortKey};
use crate::model::uri;

impl Program for Hako {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(crate::APP_ID, "Hako", "Files").icon(Icon::Folder).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.sender = Some(cx.stream());
        if self.opts.watch {
            let tx = cx.stream();
            self.watcher = crate::model::watch::Watcher::new(move |p| {
                tx.send(Msg::DirChanged(p));
            })
            .ok();
        }
        if self.opts.services {
            self.start_services(cx);
        }
        self.trash_changed();
        if let Some(listener) = self.instance.take() {
            super::instance::serve(listener, cx.stream(), Msg::Requests);
        }
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            let home = self.opts.home.clone();
            self.open_window(cx, Loc::Folder(home), None);
        } else {
            for r in pending {
                self.handle_request(cx, r);
            }
        }
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        // The kit's own reading of the command line: requests were taken
        // from it already.
        let _ = (paths, cx);
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if matches!(self.sheets.get(&window), Some(Sheet::Unlock { busy: true, .. })) {
            return false;
        }
        if let Some(s) = self.sheets.remove(&window) {
            if let Sheet::Ask { job, .. } = s {
                self.answer(cx, job, None);
            }
            return true;
        }
        if Some(window) == self.ops_window {
            self.ops_window = None;
            self.reparent_questions(cx, window);
            return true;
        }
        self.windows.remove(&window);
        if self.windows.is_empty() && !self.jobs.is_empty() {
            // Operations go on without their window, until they're done.
            self.show_operations(cx);
        }
        self.reparent_questions(cx, window);
        self.save_settings();
        true
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        if let Some(k) = cx.key_window() {
            let o = self.owner(k);
            if self.windows.contains_key(&o) {
                self.focus_window = Some(o);
            }
        }
        match msg {
            Msg::Nop => {}
            Msg::FocusSearch(w) => {
                let w = self.owner(w);
                cx.activate(w);
                cx.focus("search");
            }
            Msg::NewWindow => {
                let loc = self
                    .windows
                    .values()
                    .last()
                    .and_then(|b| b.location().cloned())
                    .unwrap_or(Loc::Folder(self.opts.home.clone()));
                self.open_window(cx, loc, None);
            }
            Msg::CloseWindow(w) => {
                let w = self.owner(w);
                self.windows.remove(&w);
                cx.close_window(w);
                self.reparent_questions(cx, w);
                if self.windows.is_empty() && !self.jobs.is_empty() {
                    self.show_operations(cx);
                }
                self.save_settings();
            }
            Msg::Requests(rs) => {
                for r in rs {
                    self.handle_request(cx, r);
                }
            }
            Msg::Go(w, loc) => {
                let w = self.owner(w);
                self.go(cx, w, loc, None, true);
            }
            Msg::GoPath(w, p) => {
                let w = self.owner(w);
                self.go_path(cx, w, &p);
            }
            Msg::Back(w) => self.history_step(cx, w, true),
            Msg::Forward(w) => self.history_step(cx, w, false),
            Msg::Up(w) => {
                let w = self.owner(w);
                let here = self.windows.get(&w).and_then(|b| b.location().cloned());
                match here {
                    Some(Loc::Folder(p)) => {
                        if let (Some(parent), Some(name)) = (p.parent(), p.file_name()) {
                            let loc = Loc::Folder(parent.to_path_buf());
                            self.go(cx, w, loc, Some(name.to_os_string()), true);
                        }
                    }
                    Some(_) => {}
                    None => {}
                }
            }
            Msg::OpenSelection(w) => {
                let w = self.owner(w);
                self.open_selection(cx, w);
            }
            Msg::Loaded(w, g, r) => {
                self.loaded(cx, w, g, r);
                self.maybe_rename_new(cx, w);
            }
            Msg::DirChanged(p) => {
                let g = self.next_gen();
                self.reloads.insert(p.clone(), g);
                cx.after(Duration::from_millis(150), Msg::Reload(p, g));
            }
            Msg::Reload(p, g) => {
                if self.reloads.get(&p) == Some(&g) {
                    self.reloads.remove(&p);
                    self.reload_dir(cx, &p);
                }
            }
            Msg::Select(w, col, rows) => self.on_select(cx, w, col, rows),
            Msg::Activate(w, col, row) => {
                let e = self.windows.get(&w).and_then(|b| b.columns.get(col)).and_then(|c| c.rows.get(row)).cloned();
                if let Some(e) = e {
                    self.open_entry(cx, w, &e);
                }
            }
            Msg::ViewKey(w, col, k) => self.on_view_key(cx, w, col, k),
            Msg::ContextMenu(w, col, row, at) => {
                let _ = row;
                if let Some(b) = self.windows.get_mut(&w) {
                    b.focus = col.min(b.columns.len().saturating_sub(1));
                }
                let menu = self.item_menu(w);
                cx.show_context_menu(w, at, menu);
            }
            Msg::Drag(w, col, ev) => self.on_drag(cx, w, col, ev),
            Msg::Spring(w, target, n) => self.spring(cx, w, target, n),
            Msg::DropUrls(w, dest, urls) => self.on_drop_urls(cx, w, dest, urls),
            Msg::SetLayout(w, layout) => {
                let w = self.owner(w);
                if let Some(b) = self.windows.get_mut(&w) {
                    b.layout = layout;
                    if layout != Layout::Column {
                        // The list shows the folder with the keyboard.
                        let focus = b.focus;
                        b.columns.truncate(focus + 1);
                    }
                    let focus = b.focus;
                    cx.focus(&format!("col-{focus}"));
                }
                self.settings.layout = layout;
                self.save_settings();
            }
            Msg::SortBy(key) => {
                if self.settings.sort.key == key {
                    self.settings.sort.ascending = !self.settings.sort.ascending;
                } else {
                    self.settings.sort.key = key;
                    self.settings.sort.ascending = key == SortKey::Name || key == SortKey::Kind;
                }
                self.resort(cx);
            }
            Msg::SortAscending(a) => {
                self.settings.sort.ascending = a;
                self.resort(cx);
            }
            Msg::FoldersFirst(f) => {
                self.settings.sort.folders_first = f;
                self.resort(cx);
            }
            Msg::ToggleHidden => {
                self.settings.hidden = !self.settings.hidden;
                let hidden = self.settings.hidden;
                for b in self.windows.values_mut() {
                    for c in &mut b.columns {
                        c.refilter(hidden);
                    }
                    let n = b.columns.len();
                    // Columns after a folder no longer shown go.
                    for i in 0..n {
                        if i + 1 < b.columns.len()
                            && b.columns[i].single().is_none_or(|e| {
                                Some(&e.path) != b.columns[i + 1].loc.folder().map(|p| p.to_path_buf()).as_ref()
                            })
                        {
                            b.columns.truncate(i + 1);
                            break;
                        }
                    }
                    b.focus = b.focus.min(b.columns.len().saturating_sub(1));
                }
                self.save_settings();
            }
            Msg::ColumnWidth(wd) => {
                self.settings.column_width = wd;
            }
            Msg::SidebarWidth(wd) => {
                self.settings.sidebar = wd;
            }
            Msg::EditPath(w) => {
                let w = self.owner(w);
                if let Some(b) = self.windows.get_mut(&w) {
                    let text = match b.location() {
                        Some(Loc::Folder(p)) => p.display().to_string(),
                        _ => String::new(),
                    };
                    b.path_edit = Some(text);
                    b.path_error = None;
                }
                cx.focus("location");
            }
            Msg::PathText(w, t) => {
                if let Some(b) = self.windows.get_mut(&w) {
                    b.path_edit = Some(t);
                    b.path_error = None;
                }
            }
            Msg::PathSubmit(w) => {
                let text = self.windows.get(&w).and_then(|b| b.path_edit.clone()).unwrap_or_default();
                let p = crate::fsops::names::expand_tilde(text.trim(), &self.opts.home);
                let p = if p.is_absolute() {
                    p
                } else {
                    self.focused_folder(w).unwrap_or_else(|| self.opts.home.clone()).join(p)
                };
                if std::fs::symlink_metadata(&p).is_ok() {
                    if let Some(b) = self.windows.get_mut(&w) {
                        b.path_edit = None;
                    }
                    self.go_path(cx, w, &p);
                } else if let Some(b) = self.windows.get_mut(&w) {
                    b.path_error = Some(format!("“{}” isn't there.", p.display()));
                }
            }
            Msg::PathCancel(w) => {
                if let Some(b) = self.windows.get_mut(&w) {
                    b.path_edit = None;
                    b.path_error = None;
                    let f = b.focus;
                    cx.focus(&format!("col-{f}"));
                }
            }
            Msg::StartRename(w) => {
                let w = self.owner(w);
                self.start_rename(cx, w);
            }
            Msg::RenameText(w, t) => {
                if let Some(r) = self.windows.get_mut(&w).and_then(|b| b.rename.as_mut()) {
                    r.text = t;
                    r.error = None;
                }
            }
            Msg::RenameCommit(w) => self.commit_rename(cx, w),
            Msg::RenameCancel(w) => {
                if let Some(b) = self.windows.get_mut(&w)
                    && let Some(r) = b.rename.take()
                {
                    cx.focus(&format!("col-{}", r.column));
                }
            }
            Msg::Search(w, q) => self.search(cx, w, q),
            Msg::SearchSelect(w, rows) => {
                if let Some(s) = self.windows.get_mut(&w).and_then(|b| b.search.as_mut()) {
                    s.selected = Rc::new(rows);
                }
            }
            Msg::SearchActivate(w, i) => {
                let e = self.windows.get(&w).and_then(|b| b.search.as_ref()).and_then(|s| s.results.get(i)).cloned();
                if let Some(e) = e {
                    if e.is_folder() {
                        self.go(cx, w, Loc::Folder(e.path.clone()), None, true);
                    } else {
                        self.open_entry(cx, w, &e);
                    }
                }
            }
            Msg::SearchMenu(w, _row, at) => {
                let menu = self.item_menu(w);
                cx.show_context_menu(w, at, menu);
            }
            Msg::SearchDone(w, g, results) => {
                if let Some(s) = self.windows.get_mut(&w).and_then(|b| b.search.as_mut())
                    && s.generation == g
                {
                    s.results = Rc::new((*results).clone());
                    s.selected = Rc::new(Vec::new());
                    s.running = false;
                }
            }
            Msg::SearchIndex(root, index) => {
                self.search_index.insert(root, (std::time::Instant::now(), index.0));
            }
            Msg::EndSearch(w) => {
                if let Some(b) = self.windows.get_mut(&w) {
                    b.search = None;
                    let f = b.focus;
                    cx.focus(&format!("col-{f}"));
                }
                self.after_change(cx, w);
            }
            Msg::NewFolder(w) => {
                let w = self.owner(w);
                self.new_folder(cx, w);
            }
            Msg::Duplicate(w) => {
                let w = self.owner(w);
                let sources = self.selection_of(w);
                if !sources.is_empty() && !self.in_trash(w) {
                    self.start_job(cx, w, Work::Duplicate { sources });
                }
            }
            Msg::Trash(w) => {
                let w = self.owner(w);
                if self.in_trash(w) {
                    return;
                }
                let paths = self.selection_of(w);
                if !paths.is_empty() {
                    self.start_job(cx, w, Work::Trash { paths });
                }
            }
            Msg::PutBack(w) => {
                let w = self.owner(w);
                let entries = self.trash_selection(w);
                if !entries.is_empty() {
                    self.start_job(cx, w, Work::Restore { entries });
                }
            }
            Msg::AskErase(w) => {
                let w = self.owner(w);
                let entries = self.trash_selection(w);
                if !entries.is_empty() {
                    self.open_sheet(cx, w, Sheet::Confirm { window: w, what: Confirm::Erase(entries) });
                }
            }
            Msg::AskEmptyTrash(w) => {
                let w = self.owner(w);
                if !self.trash_is_empty() {
                    self.open_sheet(cx, w, Sheet::Confirm { window: w, what: Confirm::EmptyTrash });
                }
            }
            Msg::Confirmed(sheet) => {
                if let Some(Sheet::Confirm { window, what }) = self.sheets.remove(&sheet) {
                    cx.close_window(sheet);
                    let work = match what {
                        Confirm::Erase(entries) => Work::Erase { entries },
                        Confirm::EmptyTrash => Work::EmptyTrash,
                    };
                    self.start_job(cx, window, work);
                }
            }
            Msg::Compress(w, format) => {
                let w = self.owner(w);
                let items = self.selection_of(w);
                if let Some(dir) = items.first().and_then(|p| p.parent()).map(PathBuf::from)
                    && !self.in_trash(w)
                {
                    let archive = dir.join(crate::fsops::archive::archive_name(&dir, &items, format));
                    self.start_job(cx, w, Work::Compress { items, archive, format });
                }
            }
            Msg::Extract(w) => {
                let w = self.owner(w);
                for p in self.selection_of(w) {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if crate::fsops::archive::is_archive(&name)
                        && let Some(dest) = p.parent().map(PathBuf::from)
                    {
                        self.start_job(cx, w, Work::Extract { archive: p, dest });
                    }
                }
            }
            Msg::Copy(w) => {
                let w = self.owner(w);
                self.copy(cx, w, false);
            }
            Msg::Cut(w) => {
                let w = self.owner(w);
                self.copy(cx, w, true);
            }
            Msg::Paste(w, force_move) => {
                let w = self.owner(w);
                self.paste(cx, w, force_move);
            }
            Msg::CopyPath(w) => {
                let w = self.owner(w);
                let ps = self.selection_of(w);
                let ps = if ps.is_empty() { self.focused_folder(w).into_iter().collect() } else { ps };
                if !ps.is_empty() {
                    cx.set_clipboard_text(&uri::plain_paths(&ps));
                }
            }
            Msg::SelectAll(w) => {
                let w = self.owner(w);
                let g = self.next_gen();
                if let Some(b) = self.windows.get_mut(&w) {
                    if let Some(s) = b.search.as_mut() {
                        s.selected = Rc::new((0..s.results.len()).collect());
                    } else {
                        let col = b.focus;
                        let n = b.columns.get(col).map_or(0, |c| c.rows.len());
                        b.select(col, (0..n).collect(), g);
                    }
                }
                self.after_change(cx, w);
            }
            Msg::Undo => {
                if let Some(step) = self.history.take_undo() {
                    let w = self.any_window();
                    self.start_job(cx, w, Work::Invert { step, redo: false });
                }
            }
            Msg::Redo => {
                if let Some(step) = self.history.take_redo() {
                    let w = self.any_window();
                    self.start_job(cx, w, Work::Invert { step, redo: true });
                }
            }
            Msg::OpenWith(_w, path, app) => {
                if let Some(a) = self.apps.as_ref().and_then(|i| i.get(&app)).cloned() {
                    super::app::launch_app(a, &path);
                }
            }
            Msg::ShowInFolder(w, p) => {
                let w = self.owner(w);
                if let (Some(dir), Some(name)) = (p.parent(), p.file_name()) {
                    let loc = Loc::Folder(dir.to_path_buf());
                    self.go(cx, w, loc, Some(name.to_os_string()), true);
                }
            }
            Msg::GetInfo(w) => {
                let w = self.owner(w);
                let entries = self.windows.get(&w).map(|b| b.selected_entries()).unwrap_or_default();
                let entries = if entries.is_empty() {
                    self.focused_folder(w).and_then(|p| Entry::read(&p).ok()).into_iter().collect()
                } else {
                    entries
                };
                for e in entries.into_iter().take(4) {
                    if e.is_folder() {
                        self.measure(cx, &e.path);
                    }
                    self.open_sheet(cx, w, Sheet::Info { window: w, entry: Box::new(e) });
                }
            }
            Msg::QuickLook(w) => {
                let w = self.owner(w);
                // Space again closes it.
                if let Some((&s, _)) =
                    self.sheets.iter().find(|(_, s)| matches!(s, Sheet::QuickLook { window, .. } if *window == w))
                {
                    self.sheets.remove(&s);
                    cx.close_window(s);
                    return;
                }
                let e = self.windows.get(&w).and_then(|b| b.selected_entries().last().cloned());
                if let Some(e) = e {
                    if e.is_folder() {
                        self.measure(cx, &e.path);
                    } else {
                        self.load_full_preview(cx, &e);
                    }
                    self.open_sheet(cx, w, Sheet::QuickLook { window: w, entry: Box::new(e) });
                }
            }
            Msg::CloseSheet(s) => {
                if matches!(self.sheets.get(&s), Some(Sheet::Unlock { busy: true, .. })) {
                    return;
                }
                if let Some(Sheet::Ask { job, .. }) = self.sheets.get(&s).cloned() {
                    self.answer(cx, job, None);
                }
                self.sheets.remove(&s);
                cx.close_window(s);
            }
            Msg::Thumb(p, img) => {
                self.thumb_done();
                if let Some(img) = img {
                    let t = thumbs_mut(&mut self.thumbs);
                    t.images.insert(p, img);
                    t.generation += 1;
                }
                self.run_thumbs_again(cx);
            }
            Msg::Preview(p, pv) => {
                // A preview for the column doesn't replace one for Quick Look.
                if !self.previews.get(&p).is_some_and(|old| old.full && !pv.full) {
                    self.previews.insert(p, pv);
                }
            }
            Msg::FolderSize(p, n, b) => {
                self.sizes.insert(p, (n, b));
            }
            Msg::Apps(index) => self.apps = Some(index),
            Msg::Desks(desks) => {
                self.desk = desks.into_iter().find(|d| d.current);
            }
            Msg::DisksChanged => self.disks_changed(cx),
            Msg::Mount(block) => self.mount(cx, block, self.any_window()),
            Msg::UnlockPass(s, text) => {
                if let Some(Sheet::Unlock { pass, busy: false, .. }) = self.sheets.get_mut(&s) {
                    *pass = text;
                }
            }
            Msg::UnlockSubmit(s) => self.unlock(s),
            Msg::Eject(block) => self.eject(block),
            Msg::Job(id, ev) => self.on_job(cx, id, ev),
            Msg::Answer(id, a) => {
                // The sheet's "Apply to all" goes with the answer.
                let all = self.sheets.values().any(|s| matches!(s, Sheet::Ask { job, all: true } if *job == id));
                let a = match a {
                    Answer::Conflict(r, _) => Answer::Conflict(r, all),
                    Answer::Error(e, _) => Answer::Error(e, all && e != ErrorChoice::Retry),
                };
                self.answer(cx, id, Some(a));
            }
            Msg::AskAll(id, on) => {
                for s in self.sheets.values_mut() {
                    if let Sheet::Ask { job, all } = s
                        && *job == id
                    {
                        *all = on;
                    }
                }
            }
            Msg::PauseJob(id) => {
                if let Some(j) = self.jobs.iter().find(|j| j.id == id) {
                    j.control.set_paused(!j.control.is_paused());
                }
            }
            Msg::CancelJob(id) => {
                if let Some(j) = self.jobs.iter().find(|j| j.id == id) {
                    j.control.cancel();
                }
                self.answer(cx, id, None);
            }
            Msg::ShowOperations => self.show_operations(cx),
            Msg::JobSlow(id) => {
                if self.jobs.iter().any(|j| j.id == id) {
                    self.show_operations(cx);
                }
            }
            Msg::Key(w, k) => self.on_key(cx, w, &k),
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some(s) = self.sheets.get(&window) {
            return super::sheets::view(self, window, s);
        }
        if Some(window) == self.ops_window {
            return super::sheets::operations(self);
        }
        match self.windows.get(&window) {
            Some(b) => super::view::window(self, window, b),
            None => text("").into(),
        }
    }

    fn menus(&self) -> MenuBar<Msg> {
        super::menus::menu_bar(self)
    }

    /// A drag gone out to the desktop is over: a drop on a Hako window
    /// came before this (it's finished only once Hako took it), so the
    /// same files dropped from elsewhere later are another program's.
    /// Hako offers only copies, so a drop leaves nothing to finish here.
    fn drag_ended(&mut self, drag: DragId, action: DropAction, cx: &mut Cx<Msg>) {
        if self.outbound.as_ref().is_some_and(|(d, _)| *d == drag) {
            self.outbound = None;
        }
        let _ = (action, cx);
    }
}

/// The thumbnails to change: taken out if no view holds them, else
/// copied.
fn thumbs_mut(this: &mut Rc<super::fileview::Thumbs>) -> &mut super::fileview::Thumbs {
    if Rc::get_mut(this).is_none() {
        let copy = super::fileview::Thumbs { images: this.images.clone(), generation: this.generation };
        *this = Rc::new(copy);
    }
    Rc::get_mut(this).expect("the only holder")
}
