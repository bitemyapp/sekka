//! Desktop & Deskbar preferences (the Deskbar, which new windows stack,
//! tiling) through the Settings controls and persisted file.

mod common;

use aizome_config::{Settings, StackNewWindows, WindowLayout};
use aizome_kit::layout::{Point, Size};
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use settei::{Settei, Start};

#[test]
fn auto_hide_is_discoverable_and_persists_without_manually_folding() {
    let (mut h, path) = common::open("deskbar-auto-hide", "desktop.auto-hide");
    let w = h.window();
    h.resize(w, Size::new(940.0, 1100.0));
    let row = h.find_id(w, "entry:desktop.auto-hide").expect("searchable auto-hide option");
    h.click(w, Point::new(row.x1 - 28.0, row.center().y));
    h.program().shared.flush();
    let saved = Settings::load_from(&path).unwrap().deskbar_settings();
    assert!(saved.auto_hide);
    assert!(!saved.collapsed, "manual folding is a separate preference");
    h.save_png(w, common::shots_dir().join("desktop-auto-hide.png"), 1.0);
}

#[test]
fn where_the_deskbar_stands_and_its_width_are_searchable() {
    let panes = settei::panes::all();
    let desktop = panes.iter().find(|p| p.id() == "desktop").unwrap();
    let entries = desktop.entries();
    let infos = [settei::search::PaneInfo { title: "Desktop & Deskbar", keywords: &[], entries: &entries }];
    let finds = |query: &str, id: &str| {
        settei::search::find(query, &infos).iter().any(|h| h.entries.iter().any(|e| e.id == id))
    };
    for query in ["deskbar left", "deskbar right", "move the deskbar", "deskbar side"] {
        assert!(finds(query, "deskbar-side"), "{query}");
    }
    for query in ["deskbar width", "icons only", "slim", "wide deskbar", "deskbar size"] {
        assert!(finds(query, "deskbar-width"), "{query}");
    }
}

fn choose(h: &mut Harness<Settei>, current: &str, index: usize) {
    let w = h.window();
    assert!(h.click_text(w, current), "{:?}", h.texts(w));
    let popup = h.popups(w).last().map(|(p, _)| *p).expect("the stacking menu");
    h.popup_keyboard(popup, Key::Named(NamedKey::Home), Modifiers::default());
    for _ in 0..index {
        h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    }
    h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::default());
    h.program().shared.flush();
}

#[test]
fn stacking_new_windows_is_a_choice_of_three_that_reads_the_old_switch() {
    let dir = common::temp_dir("stack-new-windows");
    let path = dir.join("settings.toml");
    std::fs::write(&path, "[windows]\nstack_new_windows_of_same_app = false\n").unwrap();
    let mut h = Harness::new(Settei::sample(path.clone(), Start::parse("desktop.stack-new")));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1100.0));
    let has = |h: &Harness<Settei>, text: &str| h.texts(w).iter().any(|t| t == text);
    assert!(has(&h, "Stack new windows"), "{:?}", h.texts(w));
    assert!(has(&h, "None"), "the old switch, off, shows as None: {:?}", h.texts(w));
    h.save_png(w, common::shots_dir().join("desktop-stack-none.png"), 1.0);

    choose(&mut h, "None", 2);
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("stack_new_windows = \"any-app\""), "{saved}");
    assert!(saved.contains("stack_new_windows_of_same_app = true"), "for compositors from before the choice: {saved}");
    assert!(has(&h, "All windows of any app"), "{:?}", h.texts(w));
    assert!(
        h.texts(w).iter().any(|t| t.starts_with("A new window joins the focused window as a tab, whatever its app.")),
        "the help says what the choice does: {:?}",
        h.texts(w)
    );
    h.save_png(w, common::shots_dir().join("desktop-stack-any-app.png"), 1.0);

    choose(&mut h, "All windows of any app", 1);
    let saved = Settings::load_from(&path).unwrap();
    assert_eq!(saved.windows.stack_new_windows, StackNewWindows::SameApp);
    h.save_png(w, common::shots_dir().join("desktop-stack-same-app.png"), 1.0);
}

#[test]
fn stacking_new_windows_is_found_by_what_it_stacks() {
    let panes = settei::panes::all();
    let desktop = panes.iter().find(|p| p.id() == "desktop").unwrap();
    let entries = desktop.entries();
    let infos = [settei::search::PaneInfo { title: "Desktop & Deskbar", keywords: &[], entries: &entries }];
    for query in ["stack new windows", "tabs", "same app", "any app", "all windows", "tabbed"] {
        let hits = settei::search::find(query, &infos);
        assert!(hits.iter().any(|h| h.entries.iter().any(|e| e.id == "stack-new")), "{query}");
    }
}

#[test]
fn tiling_is_a_switch_that_saves() {
    let (mut h, path) = common::open("tile-windows", "desktop.tile");
    let w = h.window();
    h.resize(w, Size::new(940.0, 1100.0));
    let row = h.find_id(w, "entry:desktop.tile").expect("searchable tiling option");
    h.click(w, Point::new(row.x1 - 28.0, row.center().y));
    h.program().shared.flush();
    assert_eq!(Settings::load_from(&path).unwrap().windows.layout, WindowLayout::Tiled);
    h.save_png(w, common::shots_dir().join("desktop-tile.png"), 1.0);
    h.click(w, Point::new(row.x1 - 28.0, row.center().y));
    h.program().shared.flush();
    assert_eq!(Settings::load_from(&path).unwrap().windows.layout, WindowLayout::Floating);
}

#[test]
fn tiling_is_found_by_its_names() {
    let panes = settei::panes::all();
    let desktop = panes.iter().find(|p| p.id() == "desktop").unwrap();
    let entries = desktop.entries();
    let infos = [settei::search::PaneInfo { title: "Desktop & Deskbar", keywords: &[], entries: &entries }];
    for query in ["tile windows", "tiling", "maximise", "maximize", "monocle"] {
        let hits = settei::search::find(query, &infos);
        assert!(hits.iter().any(|h| h.entries.iter().any(|e| e.id == "tile")), "{query}");
    }
}
