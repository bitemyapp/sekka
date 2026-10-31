//! Working with files through Hako's window, headless, in a sandbox of
//! its own: keys and clicks, renaming, new folders, the clipboard, the
//! Trash, drags, conflicts, undo, search and the location bar.

mod common;

use std::path::PathBuf;

use aizome_kit::sidestep_ui::{DropAction, Key, Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use aizome_kit::{Point, WindowKey};
use common::*;
use hako::ui::app::{Request, Sheet};
use hako::ui::browser::Loc;
use hako::ui::fileview::Layout;
use hako::ui::{Hako, Msg};

fn names(h: &Harness<Hako>, w: WindowKey, col: usize) -> Vec<String> {
    h.program()
        .browser(w)
        .unwrap()
        .columns
        .get(col)
        .map(|c| c.rows.iter().map(|e| e.display.clone()).collect())
        .unwrap_or_default()
}

fn selected(h: &Harness<Hako>, w: WindowKey) -> Vec<String> {
    h.program()
        .browser(w)
        .unwrap()
        .selection()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// The middle of row `row` of column `col`'s view.
fn row_at(h: &Harness<Hako>, w: WindowKey, col: usize, row: usize) -> Point {
    let r = h.find_id(w, &format!("col-{col}")).unwrap_or_else(|| panic!("column {col} shows"));
    let list = h.program().browser(w).unwrap().layout == Layout::List;
    let top = if list { 24.0 } else { 0.0 };
    Point::new(r.x0 + 60.0, r.y0 + top + row as f64 * 24.0 + 12.0)
}

fn click_name(h: &mut Harness<Hako>, w: WindowKey, col: usize, name: &str) {
    let i = names(h, w, col)
        .iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("{name} in column {col}: {:?}", names(h, w, col)));
    let p = row_at(h, w, col, i);
    h.click(w, p);
}

fn open(s: &Sandbox, rel: &str, layout: Layout) -> (Harness<Hako>, WindowKey) {
    let mut h = harness(s, ThemeChoice::Night);
    let w = h.window();
    h.focus_window(w, true);
    h.send(Msg::SetLayout(w, layout));
    h.send(Msg::GoPath(w, s.home().join(rel)));
    settle(&mut h);
    (h, w)
}

#[test]
fn keys_walk_the_columns() {
    let s = Sandbox::new("keys");
    populate(&s);
    let (mut h, w) = open(&s, "", Layout::Column);
    assert_eq!(h.program().browser(w).unwrap().focus, 0);
    // ↓ selects the first, then the next; a folder selected opens.
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(selected(&h, w), ["Desktop"]);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(selected(&h, w), ["Documents"]);
    assert_eq!(names(&h, w, 1), ["architecture.md", "budget.csv", "roadmap.md"]);
    // → goes in, selecting the first item; its preview shows.
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    settle(&mut h);
    let b = h.program().browser(w).unwrap();
    assert_eq!(b.focus, 1);
    assert_eq!(selected(&h, w), ["architecture.md"]);
    assert_eq!(h.program().preview_entry(w).unwrap().display, "architecture.md");
    // Typing selects by name.
    h.type_text(w, "ro");
    assert_eq!(selected(&h, w), ["roadmap.md"]);
    // ← comes back out, the column after let go.
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    let b = h.program().browser(w).unwrap();
    assert_eq!(b.focus, 0);
    assert!(b.columns[1].selected.is_empty());
    assert_eq!(selected(&h, w), ["Documents"]);
    // ❖↑ goes to the enclosing folder; Back and Forward remember.
    h.send(Msg::GoPath(w, s.home().join("work/sidestep")));
    settle(&mut h);
    h.key(w, Key::Named(NamedKey::ArrowUp), Modifiers::SUPER);
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join("work"))));
    assert_eq!(selected(&h, w), ["sidestep"]);
    h.command(w, '[');
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join("work/sidestep"))));
    h.command(w, ']');
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join("work"))));
}

#[test]
fn clicks_select_and_double_clicks_open() {
    let s = Sandbox::new("clicks");
    populate(&s);
    let (mut h, w) = open(&s, "Documents", Layout::List);
    click_name(&mut h, w, 0, "budget.csv");
    assert_eq!(selected(&h, w), ["budget.csv"]);
    // ⇧ extends, ❖ toggles.
    let p = row_at(&h, w, 0, 2);
    h.press(w, p, PointerButton::Left, 1);
    h.release(w, p, PointerButton::Left);
    let p0 = row_at(&h, w, 0, 0);
    let win = w;
    h.key(win, Key::Named(NamedKey::Home), Modifiers::SHIFT);
    assert_eq!(selected(&h, w), ["architecture.md", "budget.csv", "roadmap.md"]);
    let _ = p0;
    // A double click on a folder (in the list) goes in.
    h.send(Msg::GoPath(w, s.home().clone()));
    settle(&mut h);
    let i = names(&h, w, 0).iter().position(|n| n == "work").unwrap();
    let p = row_at(&h, w, 0, i);
    h.press(w, p, PointerButton::Left, 1);
    h.release(w, p, PointerButton::Left);
    h.press(w, p, PointerButton::Left, 2);
    h.release(w, p, PointerButton::Left);
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join("work"))));
    // Return opens too.
    h.named(w, NamedKey::Home, Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join("work/sidestep"))));
}

#[test]
fn new_folders_are_named_and_renames_undo() {
    let s = Sandbox::new("rename");
    populate(&s);
    let (mut h, w) = open(&s, "work", Layout::Column);
    h.key(w, Key::Character("n".into()), Modifiers::SUPER | Modifiers::SHIFT);
    settle(&mut h);
    assert!(s.home().join("work/untitled folder").is_dir());
    let b = h.program().browser(w).unwrap();
    assert!(b.rename.is_some(), "a new folder is named at once");
    assert_eq!(h.focused_name(w).as_deref(), Some("rename"));
    // Type a new name over it.
    h.command(w, 'a');
    h.type_text(w, "plans\n");
    settle(&mut h);
    assert!(s.home().join("work/plans").is_dir());
    assert!(!s.home().join("work/untitled folder").exists());
    assert_eq!(selected(&h, w), ["plans"]);
    // A taken name is refused, in words.
    h.send(Msg::StartRename(WindowKey::NONE));
    h.command(w, 'a');
    h.type_text(w, "notes.txt\n");
    let r = h.program().browser(w).unwrap().rename.clone().expect("still renaming");
    assert!(r.error.unwrap().contains("taken"));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    settle(&mut h);
    // Undo the rename, then the new folder; redo the folder.
    h.command(w, 'z');
    settle(&mut h);
    assert!(s.home().join("work/untitled folder").is_dir());
    h.command(w, 'z');
    settle(&mut h);
    assert!(!s.home().join("work/untitled folder").exists(), "an undone new folder goes to the Trash");
    h.key(w, Key::Character("z".into()), Modifiers::SUPER | Modifiers::SHIFT);
    settle(&mut h);
    assert!(s.home().join("work/untitled folder").is_dir());
}

#[test]
fn copy_cut_and_paste() {
    let s = Sandbox::new("clipboard");
    populate(&s);
    let (mut h, w) = open(&s, "Documents/roadmap.md", Layout::Column);
    h.command(w, 'c');
    assert_eq!(h.platform.clipboard.as_deref(), Some(s.home().join("Documents/roadmap.md").to_str().unwrap()));
    h.send(Msg::GoPath(w, s.home().join("work")));
    settle(&mut h);
    h.command(w, 'v');
    settle(&mut h);
    assert_eq!(std::fs::read_to_string(s.home().join("work/roadmap.md")).unwrap(), "# Roadmap\n");
    assert!(s.home().join("Documents/roadmap.md").exists(), "a copy");
    assert_eq!(selected(&h, w), ["roadmap.md"], "what came is selected");
    // Again: a conflict, answered by the sheet.
    h.command(w, 'v');
    assert!(wait_for(&mut h, |p| p.sheets.values().any(|s| matches!(s, Sheet::Ask { .. }))), "a conflict asks");
    h.save_png(w, out("conflict-night.png"), 2.0);
    assert!(h.click_text(w, "Keep Both"));
    settle(&mut h);
    assert!(s.home().join("work/roadmap 2.md").exists());
    // Cut and paste moves.
    h.send(Msg::GoPath(w, s.home().join("Documents/budget.csv")));
    settle(&mut h);
    h.command(w, 'x');
    h.send(Msg::GoPath(w, s.home().join("work")));
    settle(&mut h);
    h.command(w, 'v');
    settle(&mut h);
    assert!(s.home().join("work/budget.csv").exists() && !s.home().join("Documents/budget.csv").exists());
    // Undo puts it back.
    h.command(w, 'z');
    settle(&mut h);
    assert!(s.home().join("Documents/budget.csv").exists() && !s.home().join("work/budget.csv").exists());
    // Another program's paths paste too.
    h.platform.clipboard = Some(format!("file://{}\n", s.home().join("Music/eighth.flac").display()));
    h.command(w, 'v');
    settle(&mut h);
    assert!(s.home().join("work/eighth.flac").exists());
}

#[test]
fn the_trash_takes_and_gives_back() {
    let s = Sandbox::new("trash");
    populate(&s);
    let (mut h, w) = open(&s, "Documents/roadmap.md", Layout::Column);
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    settle(&mut h);
    assert!(!s.home().join("Documents/roadmap.md").exists());
    assert!(s.path().join("data/Trash/files/roadmap.md").exists());
    assert!(!names(&h, w, 1).contains(&"roadmap.md".to_owned()), "the column follows");
    // Undo restores; redo trashes again.
    h.command(w, 'z');
    settle(&mut h);
    assert!(s.home().join("Documents/roadmap.md").exists());
    h.key(w, Key::Character("z".into()), Modifiers::SUPER | Modifiers::SHIFT);
    settle(&mut h);
    assert!(!s.home().join("Documents/roadmap.md").exists());
    // The Trash lists it by its own name, and Put Back puts it back.
    h.send(Msg::Go(w, Loc::Trash));
    settle(&mut h);
    assert_eq!(names(&h, w, 0), ["roadmap.md"]);
    h.send(Msg::SetLayout(w, Layout::List));
    h.save_png(w, out("trash-night.png"), 2.0);
    click_name(&mut h, w, 0, "roadmap.md");
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    settle(&mut h);
    assert!(s.home().join("Documents/roadmap.md").exists(), "❖⌫ in the Trash puts back");
    // Deleting for good asks first.
    h.send(Msg::GoPath(w, s.home().join("Documents/budget.csv")));
    settle(&mut h);
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    settle(&mut h);
    h.send(Msg::Go(w, Loc::Trash));
    settle(&mut h);
    click_name(&mut h, w, 0, "budget.csv");
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER | Modifiers::ALT);
    assert!(h.texts(w).iter().any(|t| t.contains("for good")), "asked: {:?}", h.texts(w));
    h.save_png(w, out("erase-night.png"), 2.0);
    // Return is Cancel there.
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    settle(&mut h);
    assert!(s.path().join("data/Trash/files/budget.csv").exists());
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER | Modifiers::ALT);
    assert!(h.click_text(w, "Delete"));
    settle(&mut h);
    assert!(!s.path().join("data/Trash/files/budget.csv").exists());
    assert!(names(&h, w, 0).is_empty());
    // Empty Trash, also asked.
    h.send(Msg::GoPath(w, s.home().join("Documents/architecture.md")));
    settle(&mut h);
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    settle(&mut h);
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER | Modifiers::SHIFT);
    assert!(h.click_text(w, "Empty Trash"));
    settle(&mut h);
    assert!(h.program().trash_is_empty());
    assert_eq!(std::fs::read_dir(s.path().join("data/Trash/files")).unwrap().count(), 0);
}

#[test]
fn putting_back_a_folder_closes_its_trash_column() {
    let s = Sandbox::new("restore-folder-column");
    s.file("home/parent/child.txt", "restored content\n");
    let (mut h, w) = open(&s, "parent", Layout::Column);
    // Select the folder in its parent, then send it to the Trash.
    h.send(Msg::GoPath(w, s.home()));
    settle(&mut h);
    click_name(&mut h, w, 0, "parent");
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    settle(&mut h);
    assert!(!s.home().join("parent").exists());
    h.send(Msg::Go(w, Loc::Trash));
    settle(&mut h);
    click_name(&mut h, w, 0, "parent");
    settle(&mut h);
    assert_eq!(names(&h, w, 1), ["child.txt"]);
    h.save_png(w, out("trash-folder-before-restore.png"), 2.0);
    h.key(w, Key::Named(NamedKey::Backspace), Modifiers::SUPER);
    settle(&mut h);
    assert_eq!(std::fs::read_to_string(s.home().join("parent/child.txt")).unwrap(), "restored content\n");
    assert!(!s.path().join("data/Trash/files/parent").exists());
    assert!(names(&h, w, 0).is_empty());
    h.save_png(w, out("trash-folder-after-restore.png"), 2.0);
    let b = h.program().browser(w).unwrap();
    assert_eq!(b.columns.len(), 1, "the restored folder no longer belongs to Trash");
    assert_eq!(b.focus, 0);
    assert!(!h.texts(w).iter().any(|t| t.contains("can't be read")));
}

#[test]
fn an_explicit_path_can_open_through_a_hidden_ancestor() {
    let s = Sandbox::new("hidden-ancestor");
    s.file("home/.private/deep/note.txt", "reachable\n");
    let (mut h, w) = open(&s, ".private/deep", Layout::Column);
    assert!(!names(&h, w, 0).contains(&".private".to_owned()));
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join(".private/deep"))));
    assert_eq!(names(&h, w, 2), ["note.txt"]);
    // The chosen location remains usable even though its ancestor's
    // row was intentionally hidden in the initial Home listing.
    h.named(w, NamedKey::Home, Modifiers::NONE);
    assert_eq!(selected(&h, w), ["note.txt"]);
    s.file("home/new.txt", "unrelated\n");
    h.send(Msg::DirChanged(s.home()));
    assert!(wait_for(&mut h, |p| p.browser(w).unwrap().columns[0].rows.iter().any(|e| e.display == "new.txt")));
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join(".private/deep"))));
    assert_eq!(selected(&h, w), ["note.txt"]);
}

#[test]
fn an_explicit_path_can_open_through_an_unlistable_ancestor() {
    use std::os::unix::fs::PermissionsExt;

    let s = Sandbox::new("unlistable-ancestor");
    s.file("home/private/deep/note.txt", "reachable\n");
    let private = s.home().join("private");
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o111)).unwrap();
    let (h, w) = open(&s, "private/deep", Layout::Column);
    // Restore permissions before assertions so sandbox cleanup can run.
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let b = h.program().browser(w).unwrap();
    assert!(matches!(b.columns[1].load, hako::ui::browser::Load::Failed(_)));
    assert_eq!(b.location(), Some(&Loc::Folder(private.join("deep"))));
    assert_eq!(names(&h, w, 2), ["note.txt"]);
}

#[test]
fn refreshing_an_ancestor_preserves_or_closes_the_path_and_keyboard_focus() {
    for layout in [Layout::Column, Layout::List, Layout::Icons] {
        let s = Sandbox::new("refresh-ancestor");
        populate(&s);
        let (mut h, w) = open(&s, "work/sidestep/crates", layout);
        let before: Vec<_> = h.program().browser(w).unwrap().columns.iter().map(|c| c.loc.clone()).collect();
        assert_eq!(before.len(), 4);
        assert_eq!(h.program().browser(w).unwrap().focus, 3);
        // An unrelated addition refreshes Home but must not collapse
        // the selected work/sidestep/crates path or steal its focus.
        s.file("home/new.txt", "unrelated\n");
        h.send(Msg::DirChanged(s.home()));
        assert!(wait_for(&mut h, |p| p.browser(w).unwrap().columns[0].rows.iter().any(|e| e.display == "new.txt")));
        settle(&mut h);
        let b = h.program().browser(w).unwrap();
        assert_eq!(b.columns.iter().map(|c| c.loc.clone()).collect::<Vec<_>>(), before);
        assert_eq!(b.focus, 3);
        // Another program moves the ancestor out. Home survives; the
        // obsolete descendants must close, and keyboard input goes Home.
        std::fs::rename(s.home().join("work"), s.path().join("moved-work")).unwrap();
        h.send(Msg::DirChanged(s.home()));
        assert!(wait_for(&mut h, |p| !p.browser(w).unwrap().columns[0].rows.iter().any(|e| e.display == "work")));
        settle(&mut h);
        let b = h.program().browser(w).unwrap();
        assert_eq!(b.columns.len(), 1, "{layout:?}: no obsolete descendants");
        assert_eq!(b.focus, 0);
        assert_eq!(h.focused_name(w).as_deref(), Some("col-0"));
        h.named(w, NamedKey::Home, Modifiers::NONE);
        assert_eq!(selected(&h, w), ["Desktop"]);
        assert!(s.path().join("moved-work/sidestep/crates/sidestep-ui/src/lib.rs").is_file());
    }
}

#[test]
fn drags_move_within_a_disk_and_drops_copy_in() {
    let s = Sandbox::new("drag");
    populate(&s);
    let (mut h, w) = open(&s, "work/notes.txt", Layout::Column);
    // Drag notes.txt (column 1) onto Documents (column 0). (What takes
    // drops says where it is when it's drawn.)
    h.render(w, 1.0);
    let from = row_at(&h, w, 1, names(&h, w, 1).iter().position(|n| n == "notes.txt").unwrap());
    let to = row_at(&h, w, 0, names(&h, w, 0).iter().position(|n| n == "Documents").unwrap());
    h.drag(w, from, to, 6);
    settle(&mut h);
    assert!(s.home().join("Documents/notes.txt").exists());
    assert!(!s.home().join("work/notes.txt").exists(), "moved, on one disk");
    // A drop on itself does nothing.
    h.send(Msg::GoPath(w, s.home().join("work/sidestep")));
    settle(&mut h);
    h.render(w, 1.0);
    let p = row_at(&h, w, 0, names(&h, w, 0).iter().position(|n| n == "work").unwrap());
    h.drag(w, p, Point::new(p.x, p.y + 1.0 + 5.0), 3);
    settle(&mut h);
    assert!(s.home().join("work/sidestep").is_dir());
    // Another program's files dropped on a column are copied there.
    let docs = h.find_id(w, "col-1").unwrap().center();
    let url = format!("file://{}", s.home().join("Music/eighth.flac").display());
    assert!(h.drop(w, docs, vec![url], None));
    settle(&mut h);
    assert!(s.home().join("work/eighth.flac").exists() && s.home().join("Music/eighth.flac").exists());
}

#[test]
fn hidden_files_show_on_asking() {
    let s = Sandbox::new("hidden");
    populate(&s);
    let (mut h, w) = open(&s, "", Layout::List);
    assert!(!names(&h, w, 0).contains(&".bashrc".to_owned()));
    h.send(Msg::ToggleHidden);
    assert!(names(&h, w, 0).contains(&".bashrc".to_owned()));
    let k = aizome_kit::sidestep_ui::KeyEvent {
        key: Key::Character(">".into()),
        text: None,
        pressed: true,
        repeat: false,
        code: hako::ui::keys::KEY_DOT,
        modifiers: Modifiers::SUPER | Modifiers::SHIFT,
        composing: None, native: Default::default(),
    };
    assert!(matches!(hako::ui::view::window_key(&k), Some(Msg::ToggleHidden)));
}

#[test]
fn the_location_bar_and_reveal() {
    let s = Sandbox::new("location");
    populate(&s);
    let (mut h, w) = open(&s, "", Layout::Column);
    h.command(w, 'l');
    assert_eq!(h.focused_name(w).as_deref(), Some("location"));
    h.command(w, 'a');
    h.type_text(w, "~/work/sidestep/Cargo.toml\n");
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().location(), Some(&Loc::Folder(s.home().join("work/sidestep"))));
    assert_eq!(selected(&h, w), ["Cargo.toml"]);
    // A place that isn't there says so.
    h.command(w, 'l');
    h.command(w, 'a');
    h.type_text(w, "/nowhere/at/all\n");
    assert!(h.program().browser(w).unwrap().path_error.is_some());
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    // `--select FILE` opens a window with it selected.
    let n = h.windows().len();
    h.send(Msg::Requests(vec![Request::Reveal(s.home().join("Pictures/mark.png"))], None));
    settle(&mut h);
    let w2 = *h.windows().last().unwrap();
    assert_eq!(h.windows().len(), n + 1);
    assert_eq!(h.program().browser(w2).unwrap().selection(), vec![s.home().join("Pictures/mark.png")]);
    // Files shown together in one folder share its window; the window
    // opened last takes the launcher's token.
    let n = h.windows().len();
    let downloads = s.home().join("Downloads");
    h.send(Msg::Requests(
        vec![
            Request::Reveal(downloads.join("release-notes.pdf")),
            Request::Reveal(s.home().join("Documents/roadmap.md")),
            Request::Reveal(downloads.join("fonts.tar.gz")),
        ],
        Some("launchers-token".into()),
    ));
    settle(&mut h);
    let ws = h.windows();
    assert_eq!(ws.len(), n + 2);
    assert_eq!(h.program().browser(ws[n]).unwrap().location(), Some(&Loc::Folder(downloads)));
    assert_eq!(selected(&h, ws[n]), ["fonts.tar.gz", "release-notes.pdf"]);
    assert_eq!(selected(&h, ws[n + 1]), ["roadmap.md"]);
    assert_eq!(h.platform.activations.last().unwrap().1.as_deref(), Some("launchers-token"));
}

#[test]
fn search_finds_by_name_under_the_folder() {
    let s = Sandbox::new("search");
    populate(&s);
    let (mut h, w) = open(&s, "work", Layout::Column);
    h.command(w, 'f');
    assert_eq!(h.focused_name(w).as_deref(), Some("search"));
    h.type_text(w, "readme");
    settle(&mut h);
    let b = h.program().browser(w).unwrap();
    let results: Vec<PathBuf> = b.search.as_ref().unwrap().results.iter().map(|e| e.path.clone()).collect();
    assert_eq!(results, vec![s.home().join("work/sidestep/README.md")]);
    h.save_png(w, out("search-night.png"), 2.0);
    h.send(Msg::EndSearch(w));
    assert!(h.program().browser(w).unwrap().search.is_none());
}

#[test]
fn quick_look_and_get_info() {
    let s = Sandbox::new("info");
    populate(&s);
    let (mut h, w) = open(&s, "Documents/architecture.md", Layout::Column);
    h.key(w, Key::Character(" ".into()), Modifiers::NONE);
    settle(&mut h);
    assert!(h.program().sheets.values().any(|s| matches!(s, Sheet::QuickLook { .. })));
    assert!(h.texts(w).iter().any(|t| t.contains("Parts and processes")), "the text shows");
    h.save_png(w, out("quicklook-night.png"), 2.0);
    // ↓ goes on to the next item, in the sheet and the column.
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(selected(&h, w), ["budget.csv"]);
    assert!(h.texts(w).iter().any(|t| t == "budget.csv"), "{:?}", h.texts(w));
    h.named(w, NamedKey::ArrowUp, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(selected(&h, w), ["architecture.md"]);
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().sheets.is_empty());
    h.command(w, 'i');
    settle(&mut h);
    assert!(h.texts(w).iter().any(|t| t == "Markdown document"));
    h.save_png(w, out("info-night.png"), 2.0);
}

#[test]
fn live_updates_follow_the_disk() {
    let s = Sandbox::new("live");
    populate(&s);
    let mut opts = hako::ui::Options::sandbox(s.path());
    opts.home = s.home();
    opts.watch = true;
    let mut h = Harness::with(Hako::new(opts), appearance(ThemeChoice::Night), false);
    let w = h.window();
    settle(&mut h);
    h.send(Msg::GoPath(w, s.home().join("Documents")));
    settle(&mut h);
    std::fs::write(s.home().join("Documents/new.txt"), "fresh").unwrap();
    assert!(
        wait_for(&mut h, |p| p.browser(w).unwrap().columns[0].rows.iter().any(|e| e.display == "new.txt")),
        "a file made outside shows"
    );
    let _ = &mut h;
}

#[test]
fn the_operations_window_pauses_and_stops_jobs() {
    let s = Sandbox::new("ops");
    populate(&s);
    s.file("home/Documents/notes.txt", "already here");
    let (mut h, w) = open(&s, "work/notes.txt", Layout::Column);
    h.command(w, 'c');
    h.send(Msg::GoPath(w, s.home().join("Documents")));
    settle(&mut h);
    // The paste waits on the conflict; the operations window comes after
    // a moment.
    h.command(w, 'v');
    assert!(wait_for(&mut h, |p| p.jobs.first().is_some_and(|j| j.question.is_some())));
    h.advance(std::time::Duration::from_millis(700));
    let ops = h.program().ops_window.expect("the operations window");
    assert!(h.texts(ops).iter().any(|t| t.contains("Copying “notes.txt” to “Documents”")), "{:?}", h.texts(ops));
    assert!(h.texts(ops).iter().any(|t| t == "Waiting for an answer"));
    let id = h.program().jobs[0].id;
    h.send(Msg::PauseJob(id));
    assert!(h.program().jobs[0].paused());
    h.save_png(ops, out("operations-night.png"), 2.0);
    // Stop: the question goes, the job ends, nothing came, the window closes.
    h.send(Msg::CancelJob(id));
    settle(&mut h);
    assert!(h.program().jobs.is_empty());
    assert!(h.program().ops_window.is_none());
    assert_eq!(std::fs::read_to_string(s.home().join("Documents/notes.txt")).unwrap(), "already here");
    assert_eq!(std::fs::read_dir(s.home().join("Documents")).unwrap().count(), 4, "nothing else came");
}

#[test]
fn context_menus_offer_what_fits() {
    let s = Sandbox::new("menus");
    populate(&s);
    let (h, w) = open(&s, "Downloads/fonts.tar.gz", Layout::List);
    let labels = |m: &aizome_kit::Menu<Msg>| {
        m.spec()
            .items
            .iter()
            .filter_map(|i| match i {
                aizome_kit::menu::ItemSpec::Action(a) => Some(a.label.clone()),
                _ => None,
            })
            .collect::<Vec<String>>()
    };
    let m = h.program().item_menu(w);
    let l = labels(&m);
    for want in
        ["Open", "Get Info", "Rename…", "Duplicate", "Compress “fonts.tar.gz”", "Extract Here", "Copy", "Move to Trash"]
    {
        assert!(l.iter().any(|x| x == want), "{want} in {l:?}");
    }
}

#[test]
fn a_drag_that_waits_over_a_folder_opens_it() {
    let s = Sandbox::new("spring");
    populate(&s);
    let (mut h, w) = open(&s, "work/notes.txt", Layout::Column);
    h.render(w, 1.0);
    let from = row_at(&h, w, 1, names(&h, w, 1).iter().position(|n| n == "notes.txt").unwrap());
    let over = row_at(&h, w, 1, names(&h, w, 1).iter().position(|n| n == "sidestep").unwrap());
    h.press(w, from, PointerButton::Left, 1);
    for i in 1..=6 {
        h.move_to(w, from.lerp(over, i as f64 / 6.0));
    }
    // Waiting there opens sidestep in the next column.
    h.advance(std::time::Duration::from_millis(1000));
    settle(&mut h);
    assert_eq!(names(&h, w, 2), ["crates", "Cargo.toml", "README.md"]);
    h.render(w, 1.0);
    // Let go in it: the file goes in.
    let into = h.find_id(w, "col-2").unwrap();
    let at = Point::new(into.x0 + 80.0, into.y1 - 60.0);
    h.move_to(w, at);
    h.release(w, at, PointerButton::Left);
    settle(&mut h);
    assert!(s.home().join("work/sidestep/notes.txt").exists());
    assert!(!s.home().join("work/notes.txt").exists());
}

#[test]
fn compress_and_extract_from_the_window() {
    if hako::fsops::archive::bsdtar().is_none() {
        eprintln!("no bsdtar here: skipped");
        return;
    }
    let s = Sandbox::new("archive-ui");
    populate(&s);
    let (mut h, w) = open(&s, "Documents/architecture.md", Layout::List);
    h.key(w, Key::Named(NamedKey::End), Modifiers::SHIFT);
    assert_eq!(selected(&h, w).len(), 3);
    h.send(Msg::Compress(WindowKey::NONE, hako::fsops::archive::Format::Zip));
    settle(&mut h);
    let zip = s.home().join("Documents/Archive.zip");
    assert!(zip.is_file());
    assert_eq!(selected(&h, w), ["Archive.zip"], "the archive is selected");
    // Undo takes it to the Trash.
    h.command(w, 'z');
    settle(&mut h);
    assert!(!zip.exists());
    h.key(w, Key::Character("z".into()), Modifiers::SUPER | Modifiers::SHIFT);
    settle(&mut h);
    assert!(zip.is_file());
    // Opening an archive extracts it beside itself.
    h.send(Msg::OpenSelection(WindowKey::NONE));
    settle(&mut h);
    assert!(s.home().join("Documents/Archive/roadmap.md").is_file());
}

#[test]
fn a_drag_out_of_the_window_goes_on_as_the_desktop_s() {
    let s = Sandbox::new("drag-out");
    populate(&s);
    let (mut h, w) = open(&s, "work/notes.txt", Layout::Column);
    h.render(w, 1.0);
    let notes = s.home().join("work/notes.txt");
    let url = format!("file://{}", notes.display());
    let drag_out = |h: &mut Harness<Hako>| {
        let from = row_at(h, w, 1, names(h, w, 1).iter().position(|n| n == "notes.txt").unwrap());
        h.drag(w, from, Point::new(-40.0, from.y), 8);
        settle(h);
        h.program().outbound.clone().expect("Hako's drag went out")
    };
    let folder =
        |h: &Harness<Hako>, name: &str| row_at(h, w, 0, names(h, w, 0).iter().position(|n| n == name).unwrap());
    let (first, files) = drag_out(&mut h);
    assert_eq!(files, std::slice::from_ref(&notes));
    assert!(notes.exists(), "nothing moved");
    // Dropped nowhere, it's over: the same files dropped later come from
    // another program, and are copied.
    h.end_drag(first, DropAction::None);
    assert_eq!(h.program().outbound, None);
    assert!(h.drop(w, folder(&h, "Music"), vec![url.clone()], None));
    settle(&mut h);
    assert!(s.home().join("Music/notes.txt").exists() && notes.exists(), "copied");
    // Out again and dropped back on a Hako window, it's Hako's own: it
    // moves. The first drag's end, heard late, forgets nothing.
    let (second, _) = drag_out(&mut h);
    h.end_drag(first, DropAction::None);
    assert!(h.program().outbound.is_some());
    assert!(h.drop(w, folder(&h, "Documents"), vec![url], None));
    settle(&mut h);
    assert!(s.home().join("Documents/notes.txt").exists() && !notes.exists());
    h.end_drag(second, DropAction::Copy);
    assert_eq!(h.program().outbound, None);
}

#[test]
fn a_question_outlives_its_window() {
    let s = Sandbox::new("orphan");
    populate(&s);
    s.file("home/Documents/notes.txt", "already here");
    let (mut h, w) = open(&s, "work/notes.txt", Layout::Column);
    h.send(Msg::NewWindow);
    settle(&mut h);
    h.command(w, 'c');
    h.send(Msg::GoPath(w, s.home().join("Documents")));
    settle(&mut h);
    h.command(w, 'v');
    assert!(wait_for(&mut h, |p| p.jobs.first().is_some_and(|j| j.question.is_some())));
    h.send(Msg::CloseWindow(w));
    let p = h.program();
    let j = &p.jobs[0];
    assert_ne!(j.window, w, "asked over the other window now");
    assert!(j.sheet.is_some_and(|s| p.sheets.contains_key(&s)));
    let other = j.window;
    assert!(h.click_text(other, "Skip"));
    settle(&mut h);
    assert!(h.program().jobs.is_empty());
}
