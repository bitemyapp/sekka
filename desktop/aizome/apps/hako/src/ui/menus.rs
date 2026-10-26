//! Hako's menus: the menu bar (published to the shell, or drawn in the
//! window) and the context menus of items and places.

use std::path::PathBuf;

use aizome_kit::prelude::*;

use super::app::{Hako, Msg};
use super::browser::Loc;
use super::fileview::Layout;
use crate::fsops::archive::{self, Format};
use crate::model::entry::SortKey;

/// The window the menus act on: the last one the person used.
fn current(h: &Hako) -> WindowKey {
    h.owner(WindowKey::NONE)
}

pub fn menu_bar(h: &Hako) -> MenuBar<Msg> {
    // Commands go to the window with the keyboard when they're chosen;
    // what's enabled follows the one that had it last.
    let b = h.windows.get(&current(h));
    let w = WindowKey::NONE;
    let sel = b.map(|b| b.selection()).unwrap_or_default();
    let has_sel = !sel.is_empty();
    let in_trash = b.is_some_and(|b| b.search.is_none() && b.focused().is_some_and(|c| c.loc == Loc::Trash));
    let folder = b.is_some_and(|b| b.search.is_none() && b.target_folder().is_some());
    let single = sel.len() == 1;
    let archive_sel = sel.iter().any(|p| p.file_name().is_some_and(|n| archive::is_archive(&n.to_string_lossy())));
    let layout = b.map(|b| b.layout).unwrap_or(h.settings.layout);
    let item = |label: &str, msg: Msg| MenuItem::new(label, msg);

    let mut file = Menu::new("File")
        .item(item("New Window", Msg::NewWindow).shortcut(Shortcut::cmd('N')))
        .item(item("New Folder", Msg::NewFolder(w)).shortcut(Shortcut::cmd('N').shift()).enabled(folder && !in_trash))
        .separator()
        .item(item("Open", Msg::OpenSelection(w)).shortcut(Shortcut::cmd('O')).enabled(has_sel));
    if let Some(open_with) = open_with_menu(h, w, &sel) {
        file = file.submenu(open_with);
    }
    file = file
        .item(item("Quick Look", Msg::QuickLook(w)).enabled(has_sel))
        .item(item("Get Info", Msg::GetInfo(w)).shortcut(Shortcut::cmd('I')))
        .separator()
        .item(
            item("Rename…", Msg::StartRename(w))
                .shortcut(Shortcut::key(ShortcutKey::F(2)))
                .enabled(single && !in_trash),
        )
        .item(item("Duplicate", Msg::Duplicate(w)).shortcut(Shortcut::cmd('D')).enabled(has_sel && !in_trash))
        .item(item(&compress_label(&sel), Msg::Compress(w, Format::Zip)).enabled(has_sel && !in_trash))
        .submenu(compress_as(w, has_sel && !in_trash))
        .item(item("Extract", Msg::Extract(w)).enabled(archive_sel && !in_trash))
        .separator();
    if in_trash {
        file = file
            .item(
                item("Put Back", Msg::PutBack(w)).shortcut(Shortcut::cmd_key(ShortcutKey::Backspace)).enabled(has_sel),
            )
            .item(
                item("Delete Immediately…", Msg::AskErase(w))
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace).alt())
                    .enabled(has_sel),
            );
    } else {
        file = file.item(
            item("Move to Trash", Msg::Trash(w)).shortcut(Shortcut::cmd_key(ShortcutKey::Backspace)).enabled(has_sel),
        );
    }
    file = file
        .item(
            item("Empty Trash…", Msg::AskEmptyTrash(w))
                .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace).shift())
                .enabled(!h.trash_is_empty()),
        )
        .separator()
        .item(item("Close Window", Msg::CloseWindow(w)).shortcut(Shortcut::cmd('W')));

    let undo = match h.history.undo_label() {
        Some(l) => format!("Undo {l}"),
        None => "Undo".into(),
    };
    let redo = match h.history.redo_label() {
        Some(l) => format!("Redo {l}"),
        None => "Redo".into(),
    };
    let can_paste = folder && !in_trash;
    let edit = Menu::new("Edit")
        .item(
            MenuItem::new(undo, Msg::Undo)
                .role(MenuRole::Undo)
                .shortcut(Shortcut::cmd('Z'))
                .enabled(h.history.undo_label().is_some()),
        )
        .item(
            MenuItem::new(redo, Msg::Redo)
                .role(MenuRole::Redo)
                .shortcut(Shortcut::cmd('Z').shift())
                .enabled(h.history.redo_label().is_some()),
        )
        .separator()
        .item(item("Cut", Msg::Cut(w)).role(MenuRole::Cut).shortcut(Shortcut::cmd('X')).enabled(has_sel && !in_trash))
        .item(item("Copy", Msg::Copy(w)).role(MenuRole::Copy).shortcut(Shortcut::cmd('C')).enabled(has_sel))
        .item(item("Paste", Msg::Paste(w, false)).role(MenuRole::Paste).shortcut(Shortcut::cmd('V')).enabled(can_paste))
        .item(item("Move Here", Msg::Paste(w, true)).shortcut(Shortcut::cmd('V').alt()).enabled(can_paste))
        .item(item("Select All", Msg::SelectAll(w)).role(MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
        .separator()
        .item(item("Copy Path", Msg::CopyPath(w)).shortcut(Shortcut::cmd('C').alt()))
        .separator()
        .item(item("Find…", Msg::FocusSearch(w)).shortcut(Shortcut::cmd('F')));

    let sort = h.settings.sort;
    let mut sort_menu = Menu::new("Sort By");
    for (label, key) in [
        ("Name", SortKey::Name),
        ("Size", SortKey::Size),
        ("Kind", SortKey::Kind),
        ("Date Modified", SortKey::Modified),
    ] {
        sort_menu = sort_menu.item(item(label, Msg::SortBy(key)).checked(sort.key == key));
    }
    sort_menu = sort_menu
        .separator()
        .item(item("Ascending", Msg::SortAscending(true)).checked(sort.ascending))
        .item(item("Descending", Msg::SortAscending(false)).checked(!sort.ascending))
        .separator()
        .item(item("Folders First", Msg::FoldersFirst(!sort.folders_first)).checked(sort.folders_first));
    let view = Menu::new("View")
        .item(
            item("as Columns", Msg::SetLayout(w, Layout::Column))
                .shortcut(Shortcut::cmd('1'))
                .checked(layout == Layout::Column),
        )
        .item(
            item("as List", Msg::SetLayout(w, Layout::List))
                .shortcut(Shortcut::cmd('2'))
                .checked(layout == Layout::List),
        )
        .item(
            item("as Icons", Msg::SetLayout(w, Layout::Icons))
                .shortcut(Shortcut::cmd('3'))
                .checked(layout == Layout::Icons),
        )
        .separator()
        .submenu(sort_menu)
        .item(
            item("Show Hidden Files", Msg::ToggleHidden)
                .shortcut(Shortcut::cmd('.').shift())
                .checked(h.settings.hidden),
        );

    let mut go = Menu::new("Go")
        .item(item("Back", Msg::Back(w)).shortcut(Shortcut::cmd('[')).enabled(b.is_some_and(|b| !b.back.is_empty())))
        .item(
            item("Forward", Msg::Forward(w))
                .shortcut(Shortcut::cmd(']'))
                .enabled(b.is_some_and(|b| !b.forward.is_empty())),
        )
        .item(item("Enclosing Folder", Msg::Up(w)).shortcut(Shortcut::cmd_key(ShortcutKey::Up)))
        .item(
            item("Open Selected", Msg::OpenSelection(w))
                .shortcut(Shortcut::cmd_key(ShortcutKey::Down))
                .enabled(has_sel),
        )
        .separator();
    let shortcut_for = |kind| match kind {
        crate::model::places::PlaceKind::Home => Some(Shortcut::cmd('H').shift()),
        crate::model::places::PlaceKind::Desktop => Some(Shortcut::cmd('D').shift()),
        crate::model::places::PlaceKind::Documents => Some(Shortcut::cmd('O').shift()),
        crate::model::places::PlaceKind::Downloads => Some(Shortcut::cmd('L').alt()),
        _ => None,
    };
    for p in &h.places {
        let mut i = item(&p.name, Msg::Go(w, Loc::Folder(p.path.clone())));
        if let Some(s) = shortcut_for(p.kind) {
            i = i.shortcut(s);
        }
        go = go.item(i);
    }
    if h.opts.services {
        go = go.item(item("Recent", Msg::Go(w, Loc::Recent)).shortcut(Shortcut::cmd('F').shift()));
    }
    go = go
        .item(item("Trash", Msg::Go(w, Loc::Trash)))
        .separator()
        .item(item("Go to Folder…", Msg::EditPath(w)).shortcut(Shortcut::cmd('L')))
        .item(item("Go to Folder…", Msg::EditPath(w)).shortcut(Shortcut::cmd('G').shift()));

    MenuBar::standard(&aizome_kit::AppInfo::new(crate::APP_ID, "Hako", "Files"))
        .with_file_menu(file)
        .replace("Edit", edit)
        .insert(3, view)
        .insert(4, go)
}

fn compress_label(sel: &[PathBuf]) -> String {
    match sel {
        [one] => {
            format!("Compress “{}”", one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        }
        [] => "Compress".into(),
        _ => format!("Compress {} Items", sel.len()),
    }
}

fn compress_as(w: WindowKey, enabled: bool) -> Menu<Msg> {
    let mut m = Menu::new("Compress As");
    for f in Format::ALL {
        m = m.item(MenuItem::new(f.label(), Msg::Compress(w, f)).enabled(enabled));
    }
    m
}

/// "Open With ▸": the apps that open the selection's type, its default
/// first.
fn open_with_menu(h: &Hako, w: WindowKey, sel: &[PathBuf]) -> Option<Menu<Msg>> {
    let apps = h.apps.as_ref()?;
    let entries = h.windows.get(&h.owner(w))?.selected_entries();
    let [e] = entries.as_slice() else { return None };
    if e.is_folder() {
        return None;
    }
    let ids = apps.apps_for(&e.mime);
    let mut m = Menu::new("Open With");
    for (i, id) in ids.iter().take(12).enumerate() {
        if let Some(app) = apps.get(id) {
            let label = if i == 0 { format!("{} (default)", app.name) } else { app.name.clone() };
            m = m.item(MenuItem::new(label, Msg::OpenWith(w, sel[0].clone(), id.clone())));
        }
    }
    if ids.is_empty() {
        m = m.item(MenuItem::new("No apps open this kind", Msg::Nop).enabled(false));
    }
    Some(m)
}

/// The menu of what's selected in window `w` (or of where it is, if
/// nothing is).
pub fn item_menu(h: &Hako, w: WindowKey) -> Menu<Msg> {
    let Some(b) = h.windows.get(&w) else { return Menu::new("") };
    let sel = b.selection();
    let entries = b.selected_entries();
    let in_trash = b.search.is_none() && b.focused().is_some_and(|c| c.loc == Loc::Trash);
    let elsewhere = b.search.is_some() || b.focused().is_some_and(|c| c.loc == Loc::Recent);
    let mut m = Menu::new("");
    if sel.is_empty() {
        let folder = b.target_folder().is_some() && !in_trash;
        m = m
            .item(MenuItem::new("New Folder", Msg::NewFolder(w)).enabled(folder))
            .item(MenuItem::new("Paste", Msg::Paste(w, false)).enabled(folder))
            .separator()
            .item(MenuItem::new("Get Info", Msg::GetInfo(w)))
            .item(MenuItem::new("Show Hidden Files", Msg::ToggleHidden).checked(h.settings.hidden));
        if in_trash {
            m = m.separator().item(MenuItem::new("Empty Trash…", Msg::AskEmptyTrash(w)).enabled(!h.trash_is_empty()));
        }
        return m;
    }
    m = m.item(MenuItem::new("Open", Msg::OpenSelection(w)));
    if let Some(ow) = open_with_menu(h, w, &sel) {
        m = m.submenu(ow);
    }
    if elsewhere && let [one] = sel.as_slice() {
        m = m.item(MenuItem::new("Show in Enclosing Folder", Msg::ShowInFolder(w, one.clone())));
    }
    m = m
        .separator()
        .item(MenuItem::new("Get Info", Msg::GetInfo(w)))
        .item(MenuItem::new("Quick Look", Msg::QuickLook(w)));
    if in_trash {
        return m
            .separator()
            .item(MenuItem::new("Put Back", Msg::PutBack(w)))
            .item(MenuItem::new("Delete Immediately…", Msg::AskErase(w)))
            .separator()
            .item(MenuItem::new("Empty Trash…", Msg::AskEmptyTrash(w)));
    }
    m = m
        .item(MenuItem::new("Rename…", Msg::StartRename(w)).enabled(sel.len() == 1 && !elsewhere))
        .item(MenuItem::new("Duplicate", Msg::Duplicate(w)))
        .item(MenuItem::new(compress_label(&sel), Msg::Compress(w, Format::Zip)))
        .submenu(compress_as(w, true));
    if entries.iter().any(|e| archive::is_archive(&e.display)) {
        m = m.item(MenuItem::new("Extract Here", Msg::Extract(w)));
    }
    m.separator()
        .item(MenuItem::new("Cut", Msg::Cut(w)))
        .item(MenuItem::new("Copy", Msg::Copy(w)))
        .item(MenuItem::new("Copy Path", Msg::CopyPath(w)))
        .separator()
        .item(MenuItem::new("Move to Trash", Msg::Trash(w)))
}

impl Hako {
    pub fn item_menu(&self, w: WindowKey) -> Menu<Msg> {
        item_menu(self, w)
    }
}
