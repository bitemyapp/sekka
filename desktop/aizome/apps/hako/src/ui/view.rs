//! A browser window: the toolbar, the path bar, the sidebar, the columns
//! (or a list, or icons), the preview, and the status bar.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use super::app::{Hako, Msg};
use super::browser::{Browser, Column, Load, Loc};
use super::dnd::{self, Dest};
use super::fileview::{Detail, Handlers, Layout, file_view};
use super::keys::{self, KEY_DOT};
use super::look;
use super::sidebar::{Crumb, PlaceRow, caption};
use super::strip::strip;
use crate::model::entry::Entry;
use crate::model::format;
use crate::model::places::PlaceKind;

/// The window `w`'s view.
pub fn window(h: &Hako, w: WindowKey, b: &Browser) -> Element<Msg> {
    let n = b.columns.len() as u64;
    b.spots.retain(|id| {
        id < n || (dnd::SIDEBAR..dnd::SIDEBAR + 64).contains(&id) || (dnd::PATHBAR..dnd::PATHBAR + 64).contains(&id)
    });
    let main =
        sidebar_layout(sidebar(h, w, b), content(h, w, b)).first_size(h.settings.sidebar).on_resize(Msg::SidebarWidth);
    let body =
        column![toolbar(h, w, b), pathbar(h, w, b), main, status(h, w, b)].width(Length::Fill).height(Length::Fill);
    let body = dnd::window_bounds(body.into(), b.spots.clone());
    keys::catcher(body, window_key).width(Length::Fill).height(Length::Fill)
}

/// The arrows, when nothing took them (Quick Look's sheet is up): they
/// move through the items it shows.
pub(crate) fn quick_look_key(w: WindowKey, k: &aizome_kit::sidestep_ui::KeyEvent) -> Option<Msg> {
    use aizome_kit::sidestep_ui::{Key, NamedKey};
    let arrow = matches!(
        k.key,
        Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight)
    );
    (arrow && k.modifiers.is_empty()).then(|| Msg::Key(w, k.clone()))
}

/// What a key no control took means to the window: ⇧❖. (by the key's
/// place) shows hidden files.
pub fn window_key(k: &aizome_kit::sidestep_ui::KeyEvent) -> Option<Msg> {
    let m = k.modifiers;
    (m.super_key() && m.shift() && k.code == KEY_DOT).then_some(Msg::ToggleHidden)
}

fn toolbar(h: &Hako, w: WindowKey, b: &Browser) -> Element<Msg> {
    let layout = match b.layout {
        Layout::Column => 0,
        Layout::List => 1,
        Layout::Icons => 2,
    };
    let in_trash = b.focused().is_some_and(|c| c.loc == Loc::Trash) && b.search.is_none();
    let has_sel = !b.selection().is_empty();
    let folder = b.target_folder().is_some() && b.search.is_none();
    let mut items: Vec<Element<Msg>> = vec![
        icon_button(Icon::ChevronLeft)
            .toolbar()
            .on_press_maybe((!b.back.is_empty()).then_some(Msg::Back(w)))
            .tooltip("Back (❖[)")
            .label("Back")
            .into(),
        icon_button(Icon::ChevronRight)
            .toolbar()
            .on_press_maybe((!b.forward.is_empty()).then_some(Msg::Forward(w)))
            .tooltip("Forward (❖])")
            .label("Forward")
            .into(),
        toolbar_separator(),
        segmented_icons([Icon::Columns3, Icon::List, Icon::LayoutGrid], Some(layout))
            .on_select(move |i| Msg::SetLayout(w, [Layout::Column, Layout::List, Layout::Icons][i.min(2)]))
            .label("View")
            .tooltip("Columns, list or icons (❖1, ❖2, ❖3)")
            .into(),
        toolbar_separator(),
        icon_button(Icon::FolderPlus)
            .toolbar()
            .on_press_maybe(folder.then_some(Msg::NewFolder(w)))
            .tooltip("New Folder (⇧❖N)")
            .label("New Folder")
            .into(),
    ];
    if in_trash {
        items.push(
            icon_button(Icon::Undo2)
                .toolbar()
                .on_press_maybe(has_sel.then_some(Msg::PutBack(w)))
                .tooltip("Put Back")
                .label("Put Back")
                .into(),
        );
        items.push(
            button("Empty Trash")
                .toolbar()
                .on_press_maybe((!h.trash_is_empty()).then_some(Msg::AskEmptyTrash(w)))
                .into(),
        );
    } else {
        items.push(
            icon_button(Icon::Trash)
                .toolbar()
                .on_press_maybe((has_sel && b.search.is_none()).then_some(Msg::Trash(w)))
                .tooltip("Move to Trash (❖⌫)")
                .label("Move to Trash")
                .into(),
        );
    }
    items.push(spacer().into());
    let query = b.search.as_ref().map(|s| s.query.clone()).unwrap_or_default();
    items.push(
        search_field(&query)
            .id("search")
            .width(220.0)
            .placeholder("Search")
            .on_change(move |s| Msg::Search(w, s))
            .into(),
    );
    aizome_kit::toolbar(items)
}

/// The path bar: the folders from the top down to the one shown, each a
/// place to go and to drop into; or, while editing (❖L), the location.
fn pathbar(h: &Hako, w: WindowKey, b: &Browser) -> Element<Msg> {
    let inner: Element<Msg> = if let Some(text) = &b.path_edit {
        row![
            text_field(text)
                .id("location")
                .mono()
                .placeholder("A folder or file: ~/Documents, /etc")
                .error(b.path_error.clone())
                .on_change(move |s| Msg::PathText(w, s))
                .on_submit(Msg::PathSubmit(w))
                .width(Length::Fill),
            button("Cancel").compact().cancel().on_press(Msg::PathCancel(w)),
            button("Go").compact().default().on_press(Msg::PathSubmit(w)),
        ]
        .spacing(6.0)
        .width(Length::Fill)
        .into()
    } else if let Some(s) = &b.search {
        row![
            icon(Icon::Search).size(14.0).role(Role::Text3),
            text(format!(
                "{} in “{}”",
                if s.running { "Searching" } else { "Results" },
                s.root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| s.root.display().to_string())
            ))
            .size(12.0)
            .role(Role::Text2),
            spacer(),
            button("Done").compact().on_press(Msg::EndSearch(w)),
        ]
        .spacing(6.0)
        .width(Length::Fill)
        .into()
    } else {
        let mut items: Vec<Element<Msg>> = Vec::new();
        let crumbs = crumbs(h, b);
        let n = crumbs.len();
        let lit = b.drag.as_ref().and_then(|d| d.target.as_ref()).map(|t| t.spot);
        for (i, (label, icon, path)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                items.push(super::view::chevron());
            }
            let id = dnd::PATHBAR + i as u64;
            let dest =
                path.as_ref().map(|p| if p == Path::new("\u{0}trash") { Dest::Trash } else { Dest::Folder(p.clone()) });
            let on_press = path
                .as_ref()
                .filter(|_| i + 1 < n)
                .map(|p| if p == Path::new("\u{0}trash") { Msg::Go(w, Loc::Trash) } else { Msg::GoPath(w, p.clone()) });
            items.push(
                Crumb {
                    icon,
                    label: label.into(),
                    current: i + 1 == n,
                    lit: lit == Some(id),
                    on_press,
                    drop: dest.map(|d| (d, id, b.spots.clone(), drop_fn(w))),
                }
                .into(),
            );
        }
        items.push(spacer().into());
        items.push(
            icon_button(Icon::Pencil)
                .compact()
                .plain()
                .on_press(Msg::EditPath(w))
                .tooltip("Go to a folder (❖L)")
                .label("Go to a folder")
                .into(),
        );
        aizome_kit::row(items).spacing(0.0).width(Length::Fill).into()
    };
    container(inner).padding([2.0, 6.0]).width(Length::Fill).fill(Tone::Surface).edges(Edge::Line, Sides::BOTTOM).into()
}

pub(crate) fn chevron<Msg: 'static>() -> Element<Msg> {
    icon(Icon::ChevronRight).size(10.0).role(Role::Faint).into()
}

/// The path bar's folders: label, icon, path (`\0trash` for the Trash).
fn crumbs(h: &Hako, b: &Browser) -> Vec<(String, Option<Icon>, Option<PathBuf>)> {
    let mut out = Vec::new();
    match b.location() {
        Some(Loc::Folder(p)) => {
            let home = &h.opts.home;
            // A volume's folders start at the volume.
            let volume = h
                .volumes
                .iter()
                .filter_map(|v| v.mount_point.as_ref().map(|m| (PathBuf::from(m), v)))
                .filter(|(m, _)| p.starts_with(m) && m != Path::new("/"))
                .max_by_key(|(m, _)| m.as_os_str().len());
            let (start, rest) = if let Some((m, v)) = volume.filter(|(m, _)| !home.starts_with(m)) {
                let icon = if v.image_file.is_some() { Icon::Disc } else { Icon::Usb };
                out.push((v.name.clone(), Some(icon), Some(m.clone())));
                (m.clone(), p.strip_prefix(&m).unwrap_or(Path::new("")).to_path_buf())
            } else if p.starts_with(home) {
                out.push(("Home".to_owned(), Some(Icon::House), Some(home.clone())));
                (home.clone(), p.strip_prefix(home).unwrap_or(Path::new("")).to_path_buf())
            } else {
                let name = h
                    .system_disk
                    .as_ref()
                    .filter(|d| d.path == Path::new("/"))
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "Computer".into());
                out.push((name, Some(Icon::HardDrive), Some(PathBuf::from("/"))));
                (PathBuf::from("/"), p.strip_prefix("/").unwrap_or(p).to_path_buf())
            };
            let mut at = start;
            for c in rest.components() {
                at.push(c);
                out.push((c.as_os_str().to_string_lossy().into_owned(), None, Some(at.clone())));
            }
        }
        Some(Loc::Trash) => out.push(("Trash".into(), Some(Icon::Trash), Some(PathBuf::from("\u{0}trash")))),
        Some(Loc::Recent) => out.push(("Recent".into(), Some(Icon::Clock), None)),
        None => {}
    }
    // A file selected in the last folder.
    if let Some(e) = b.columns.last().and_then(Column::single).filter(|e| !e.is_folder()) {
        out.push((e.display.clone(), None, None));
    }
    out
}

fn drop_fn(w: WindowKey) -> Rc<dyn Fn(Dest, Vec<String>) -> Msg> {
    Rc::new(move |d, urls| Msg::DropUrls(w, d, urls))
}

/// The sidebar: places, the Desk's folder, disks, Recent and the Trash.
fn sidebar(h: &Hako, w: WindowKey, b: &Browser) -> Element<Msg> {
    let here = b.columns.first().map(|c| c.loc.clone());
    let lit = b.drag.as_ref().and_then(|d| d.target.as_ref()).map(|t| t.spot);
    let mut items: Vec<Element<Msg>> = Vec::new();
    let next_id = std::cell::Cell::new(dnd::SIDEBAR);
    let place = |items: &mut Vec<Element<Msg>>,
                 kind: PlaceKind,
                 label: String,
                 detail: Option<String>,
                 loc: Loc,
                 eject: Option<String>,
                 busy: bool| {
        let (icon, role) = look::place_icon(kind);
        let dest = match &loc {
            Loc::Folder(p) => Some(Dest::Folder(p.clone())),
            Loc::Trash => Some(Dest::Trash),
            Loc::Recent => None,
        };
        let row = PlaceRow {
            icon,
            role,
            label: label.into(),
            detail: detail.map(Into::into),
            selected: here.as_ref() == Some(&loc) && b.search.is_none(),
            lit: lit == Some(next_id.get()),
            busy,
            on_press: Msg::Go(w, loc.clone()),
            on_eject: eject.map(Msg::Eject),
            on_menu: None,
            drop: dest.map(|d| (d, next_id.get(), b.spots.clone(), drop_fn(w))),
        };
        items.push(row.into());
        next_id.set(next_id.get() + 1);
    };
    items.push(caption("Places"));
    for p in &h.places {
        place(&mut items, p.kind, p.name.clone(), None, Loc::Folder(p.path.clone()), None, false);
    }
    if let Some(d) = &h.desk
        && let Some(f) = &d.folder
    {
        // The Desk by its name when it has one ("Desk: Build"), its folder
        // by its own.
        let named = !d.name.trim().is_empty() && d.name.trim() != d.index.to_string();
        items.push(caption(&if named { format!("Desk: {}", d.name.trim()) } else { format!("Desk {}", d.index) }));
        let label = PathBuf::from(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| f.clone());
        place(&mut items, PlaceKind::Desk, label, None, Loc::Folder(PathBuf::from(f)), None, false);
    }
    items.push(caption("Disks"));
    if let Some(d) = &h.system_disk {
        let free = h.free_space(&d.path).map(|f| format!("{} free", format::size(f)));
        place(&mut items, PlaceKind::Disk, d.name.clone(), free, Loc::Folder(d.path.clone()), None, false);
    } else {
        place(&mut items, PlaceKind::Disk, "Computer".into(), None, Loc::Folder(PathBuf::from("/")), None, false);
    }
    for v in &h.volumes {
        let kind = if v.image_file.is_some() { PlaceKind::Image } else { PlaceKind::Volume };
        let busy = h.mounting.contains(&v.block);
        let can_eject = v.mount_point.is_some() || v.ejectable;
        match &v.mount_point {
            Some(m) => place(
                &mut items,
                kind,
                v.name.clone(),
                None,
                Loc::Folder(PathBuf::from(m)),
                can_eject.then(|| v.block.clone()),
                busy,
            ),
            None => {
                // Not mounted: a press mounts it (and goes there).
                let (icon, role) = look::place_icon(kind);
                items.push(
                    PlaceRow {
                        icon,
                        role,
                        label: v.name.clone().into(),
                        detail: Some(if v.locked { "locked".into() } else { "not mounted".into() }),
                        selected: false,
                        lit: false,
                        busy,
                        on_press: Msg::Mount(v.block.clone()),
                        on_eject: v.ejectable.then(|| Msg::Eject(v.block.clone())),
                        on_menu: None,
                        drop: None,
                    }
                    .into(),
                );
                next_id.set(next_id.get() + 1);
            }
        }
    }
    items.push(space(8.0));
    if h.opts.services {
        place(&mut items, PlaceKind::Recent, "Recent".into(), None, Loc::Recent, None, false);
    }
    place(&mut items, PlaceKind::Trash, "Trash".into(), None, Loc::Trash, None, false);
    scroll(aizome_kit::column(items).spacing(0.0).padding([0.0, 0.0, 8.0, 0.0]).width(Length::Fill)).into()
}

/// What fills the window beside the sidebar.
fn content(h: &Hako, w: WindowKey, b: &Browser) -> Element<Msg> {
    if let Some(s) = &b.search {
        let handlers = Rc::new(Handlers {
            select: Box::new(move |rows| Msg::SearchSelect(w, rows)),
            activate: Box::new(move |i| Msg::SearchActivate(w, i)),
            key: Box::new(move |k| match k {
                super::fileview::ViewKey::QuickLook => Msg::QuickLook(w),
                super::fileview::ViewKey::Escape => Msg::EndSearch(w),
                _ => Msg::Nop,
            }),
            menu: Box::new(move |row, at| Msg::SearchMenu(w, row, at)),
            drag: Box::new(move |e| Msg::Drag(w, 0, e)),
            drop: Box::new(move |d, u| Msg::DropUrls(w, d, u)),
            sort: Box::new(|_| Msg::Nop),
        });
        let message = if s.running && s.results.is_empty() {
            "Searching…".to_owned()
        } else {
            format!("Nothing under “{}” matches “{}”.", s.root.display(), s.query)
        };
        return file_view(Layout::List, s.results.clone(), handlers, b.spots.clone(), 0)
            .home(Rc::new(h.opts.home.clone()))
            .now(h.now())
            .selected(Rc::new((*s.selected).clone()))
            .details(vec![Detail::Name, Detail::Where, Detail::Modified, Detail::Size])
            .thumbs(h.thumbs.clone())
            .message(Some(message))
            .id("col-0")
            .into();
    }
    match b.layout {
        Layout::Column => {
            let mut cols: Vec<Element<Msg>> =
                b.columns.iter().enumerate().map(|(i, c)| column_view(h, w, b, i, c, Layout::Column)).collect();
            let preview = h.preview_entry(w);
            let last_width = if preview.is_some() { 300.0 } else { h.settings.column_width };
            if let Some(e) = preview {
                cols.push(preview_column(h, &e));
            }
            strip(cols, h.settings.column_width, last_width, b.reveal, Rc::new(Msg::ColumnWidth))
        }
        layout => match b.columns.get(b.focus) {
            Some(c) => column_view(h, w, b, b.focus, c, layout),
            None => text("").into(),
        },
    }
}

/// Column `i`'s view.
fn column_view(h: &Hako, w: WindowKey, b: &Browser, i: usize, c: &Column, layout: Layout) -> Element<Msg> {
    let handlers = Rc::new(Handlers {
        select: Box::new(move |rows| Msg::Select(w, i, rows)),
        activate: Box::new(move |r| Msg::Activate(w, i, r)),
        key: Box::new(move |k| Msg::ViewKey(w, i, k)),
        menu: Box::new(move |row, at| Msg::ContextMenu(w, i, row, at)),
        drag: Box::new(move |e| Msg::Drag(w, i, e)),
        drop: Box::new(move |d, u| Msg::DropUrls(w, d, u)),
        sort: Box::new(Msg::SortBy),
    });
    let message = match &c.load {
        Load::Failed(e) => Some(format!("This folder can't be read: {e}.")),
        Load::Ready if c.rows.is_empty() => Some(match c.loc {
            Loc::Trash => "The Trash is empty.".to_owned(),
            Loc::Recent => "Nothing has been opened lately.".to_owned(),
            _ if !c.all.is_empty() => format!("{} hidden ({}).", format::items(c.all.len()), "⇧❖. shows them"),
            _ => "Empty folder".to_owned(),
        }),
        _ => None,
    };
    let dest = match &c.loc {
        Loc::Folder(p) => Some(Dest::Folder(p.clone())),
        Loc::Trash => Some(Dest::Trash),
        Loc::Recent => None,
    };
    let lit = b.drag.as_ref().and_then(|d| d.target.as_ref()).filter(|t| t.spot == i as u64).map(|t| t.row);
    let faded: Vec<bool> = if h.cut_paths.is_empty() {
        Vec::new()
    } else {
        c.rows.iter().map(|e| h.cut_paths.contains(&e.path)).collect()
    };
    let details = match c.loc {
        Loc::Trash => vec![Detail::Name, Detail::Where, Detail::Trashed, Detail::Size],
        Loc::Recent => vec![Detail::Name, Detail::Where, Detail::Modified, Detail::Size],
        Loc::Folder(_) => vec![Detail::Name, Detail::Size, Detail::Kind, Detail::Modified],
    };
    let mut v = file_view(layout, c.rows.clone(), handlers, b.spots.clone(), i as u64)
        .home(Rc::new(h.opts.home.clone()))
        .now(h.now())
        .selected(c.selected.clone())
        .lit(lit)
        .faded(Rc::new(faded))
        .thumbs(h.thumbs.clone())
        .message(message)
        .dest(dest)
        .details(details)
        .sort(h.settings.sort.key, h.settings.sort.ascending);
    if let Some((row, g)) = c.scroll {
        v = v.scroll_to(row, g);
    }
    if let Some(r) = b.rename.as_ref().filter(|r| r.column == i) {
        let is_dir = c.rows.get(r.row).is_some_and(Entry::is_folder);
        let stem = crate::fsops::names::split_ext(std::ffi::OsStr::new(&r.text), is_dir).0.len();
        let field = text_field(&r.text)
            .id("rename")
            .key(("rename", r.path.clone()))
            .select_on_focus(0..stem)
            .error(r.error.clone())
            .on_change(move |s| Msg::RenameText(w, s))
            .on_submit(Msg::RenameCommit(w));
        v = v.rename(r.row, field.into());
    }
    v.id(format!("col-{i}")).key(("col", i, c.loc.clone())).into()
}

/// The preview of a file selected last: its picture (or icon), name,
/// kind, size and dates, and the start of its text.
fn preview_column(h: &Hako, e: &Entry) -> Element<Msg> {
    let p = h.previews.get(&e.path);
    let (ic, role) = look::entry_icon(e);
    let picture: Element<Msg> = match p.and_then(|p| p.image.clone()).or_else(|| h.thumbs.images.get(&e.path).cloned())
    {
        Some(img) => image(img).fit(Fit::Contain).width(Length::Fill).height(180.0).into(),
        None => container(icon(ic).size(96.0).role(role)).center().width(Length::Fill).height(180.0).into(),
    };
    let now = h.now();
    let mut facts: Vec<Element<Msg>> = vec![
        fact("Kind", e.kind_name.to_string()),
        fact("Size", if e.is_folder() { "—".into() } else { format::size_long(e.size) }),
    ];
    if let Some(c) = e.created {
        facts.push(fact("Created", format::date(c, now)));
    }
    if let Some(m) = e.modified {
        facts.push(fact("Modified", format::date(m, now)));
    }
    if let Some(t) = &e.link_target {
        facts.push(fact("Link to", t.display().to_string()));
    }
    let mut parts: Vec<Element<Msg>> = vec![
        picture,
        text(e.display.clone()).semibold().size(14.0).wrap().max_lines(3).center().width(Length::Fill).into(),
        aizome_kit::column(facts).spacing(3.0).width(Length::Fill).into(),
    ];
    if let Some(t) = p.and_then(|p| p.text.clone()) {
        let shown: String = t.lines().take(40).collect::<Vec<_>>().join("\n");
        parts.push(
            container(text(shown).mono().size(11.0).role(Role::Text2).wrap().max_lines(40))
                .padding(8.0)
                .fill(Tone::Sunk)
                .border(Edge::Line)
                .width(Length::Fill)
                .clip()
                .into(),
        );
    }
    scroll(aizome_kit::column(parts).spacing(12.0).padding(16.0).width(Length::Fill))
        .key(("preview", e.path.clone()))
        .into()
}

fn fact(label: &str, value: String) -> Element<Msg> {
    row![
        text(label.to_owned()).size(11.5).role(Role::Text3).width(72.0),
        text(value).size(11.5).role(Role::Text2).width(Length::Fill)
    ]
    .spacing(6.0)
    .width(Length::Fill)
    .into()
}

/// The status bar: what's here and selected, the space free, and
/// operations under way.
fn status(h: &Hako, w: WindowKey, b: &Browser) -> Element<Msg> {
    let (count, selected) = match &b.search {
        Some(s) => (s.results.len(), s.selected.len()),
        None => b.focused().map(|c| (c.rows.len(), c.selected.len())).unwrap_or((0, 0)),
    };
    let mut left = format::items(count);
    if selected > 0 {
        left.push_str(&format!(", {selected} selected"));
    }
    let mut items: Vec<Element<Msg>> = vec![status_text(left)];
    items.push(spacer().into());
    if let Some(j) = h.jobs.first() {
        let more = if h.jobs.len() > 1 { format!(" (+{})", h.jobs.len() - 1) } else { String::new() };
        items.push(
            button(format!("{} — {}%{more}", j.title, (j.progress.fraction() * 100.0).round()))
                .plain()
                .compact()
                .on_press(Msg::ShowOperations)
                .into(),
        );
        items.push(progress(j.progress.fraction()).width(80.0).into());
    } else if let Some(dir) = b.target_folder()
        && let Some(free) = h.free_space(dir)
    {
        items.push(status_text(format!("{} free", format::size(free))));
    }
    let _ = w;
    status_bar(items)
}
