//! Deskbar preferences through the Settings controls and persisted file.

mod common;

use aizome_kit::layout::{Point, Size};

#[test]
fn auto_hide_is_discoverable_and_persists_without_manually_folding() {
    let (mut h, path) = common::open("deskbar-auto-hide", "desktop.auto-hide");
    let w = h.window();
    h.resize(w, Size::new(940.0, 1100.0));
    let row = h.find_id(w, "entry:desktop.auto-hide").expect("searchable auto-hide option");
    h.click(w, Point::new(row.x1 - 28.0, row.center().y));
    h.program().shared.flush();
    let saved = aizome_config::Settings::load_from(&path).unwrap().deskbar_settings();
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
