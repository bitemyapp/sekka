//! The Deskbar, headless: apps and their windows from the compositor,
//! notifications filed under their app, the Shelf.

mod common;

use aizome_kit::prelude::*;
use aizome_shell::deskbar::DeskbarMsg;
use aizome_shell::shell::Msg;
use common::*;

#[test]
fn apps_list_their_windows_with_notes() {
    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    let db = window_titled(&h, "Deskbar");
    let texts = h.texts(db);
    for want in
        ["Editor", "architecture.md", "roadmap.md", "Foot", "cargo test", "Hako", "Downloads", "Firefox", "Thunderbird"]
    {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
    // Firefox's three windows fold; Mail's newest notification shows.
    assert!(!texts.iter().any(|t| t == "docs.rs"), "{texts:?}");
    assert!(texts.iter().any(|t| t.starts_with("Re: tiles at fractional scale")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "wallpaper-draft.png"));
    assert!(texts.iter().any(|t| t == "Drop to park"));
}

#[test]
fn folding_and_parking() {
    let mut h = scene(false);
    h.send(Msg::Deskbar(DeskbarMsg::Fold("firefox".into())));
    let db = window_titled(&h, "Deskbar");
    assert!(h.texts(db).iter().any(|t| t == "docs.rs"));
    h.send(Msg::Deskbar(DeskbarMsg::Park(vec!["file:///tmp/notes%20a.md".into()], None)));
    assert!(h.texts(db).iter().any(|t| t == "notes a.md"));
    let before = h.size(db).height;
    h.send(Msg::Deskbar(DeskbarMsg::Park(vec![], Some("hello there".into()))));
    let size = h.platform.layer_changes.iter().rev().find_map(|(_, c)| match c {
        aizome_kit::platform::LayerChange::Size(s) => Some(*s),
        _ => None,
    });
    assert!(size.is_some_and(|s| s.height > before), "the Deskbar grew: {size:?}");
}

#[test]
fn the_deskbar_folds_into_the_bar() {
    let mut h = scene(false);
    h.send(Msg::Run(aizome_shell::actions::Action::SetDeskbarCollapsed(true)));
    assert!(h.windows().iter().all(|k| h.title(*k) != "Deskbar"));
    let bar = window_titled(&h, "Menu bar");
    assert!(h.texts(bar).iter().any(|t| t == "5 apps"), "{:?}", h.texts(bar));
}

#[test]
fn pictures_of_the_deskbar() {
    for day in [false, true] {
        let mut h = scene(day);
        with_mail_and_shelf(&mut h);
        let db = window_titled(&h, "Deskbar");
        let size = h.program().deskbar_model();
        h.resize(db, aizome_shell::deskbar::size(&size));
        let name = if day { "deskbar-day.png" } else { "deskbar-night.png" };
        h.save_png(db, shots().join(name), 2.0);
        aizome_kit::testing::assert_golden(&mut h, db, golden(name), 2.0);
    }
    let _ = Size::ZERO;
}

#[test]
fn a_window_dragged_onto_a_desk_of_the_strip_finds_it() {
    let h = scene(false);
    let s = h.program();
    // The Deskbar's left edge, in the output.
    let left = W - aizome_shell::deskbar::WIDTH - aizome_shell::deskbar::SHADOW;
    // Over the bar's strip (y above the Deskbar's top), along it.
    let hits: Vec<Option<u32>> =
        (0..300).map(|x| s.desk_at_deskbar_point(Point::new(900.0 + x as f64 * 1.0 - left, -12.0))).collect();
    assert!(hits.contains(&Some(1)) && hits.contains(&Some(2)) && hits.contains(&Some(3)), "{hits:?}");
    assert_eq!(s.desk_at_deskbar_point(Point::new(40.0, 40.0)), None, "inside the Deskbar");
}

#[test]
fn shelf_items_drag_out_to_other_programs() {
    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    let db = window_titled(&h, "Deskbar");
    let size = aizome_shell::deskbar::size(&h.program().deskbar_model());
    h.resize(db, size);
    let row = h.find(db, "wallpaper-draft.png").expect("the Shelf's file");
    let from = row.center();
    h.drag(db, from, aizome_kit::layout::Point::new(from.x - 200.0, from.y), 6);
    assert_eq!(h.platform.drags.len(), 1, "a drag out started");
    let what = format!("{:?}", h.platform.drags[0].1);
    assert!(what.contains("text/uri-list") && what.contains("text/plain"), "{what}");
    assert_eq!(h.program().shelf.items.len(), 3, "a copy: it stays on the Shelf");
    assert_eq!(
        aizome_shell::shelf::file_url(std::path::Path::new("/home/u/My Notes/ä.md")),
        "file:///home/u/My%20Notes/%C3%A4.md"
    );
}
