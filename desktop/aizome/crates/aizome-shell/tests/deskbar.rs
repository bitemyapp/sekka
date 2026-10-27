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
fn the_bar_has_controls_to_hide_and_show_the_deskbar() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let hide = h.find(bar, "Hide Deskbar").expect("visible hide control");
    h.click(bar, hide.center());
    assert!(h.program().deskbar_cfg.collapsed);
    assert!(h.windows().iter().all(|k| h.title(*k) != "Deskbar"));
    let show = h.find(bar, "5 apps").expect("visible show control");
    h.click(bar, show.center());
    assert!(!h.program().deskbar_cfg.collapsed);
    assert!(h.windows().iter().any(|k| h.title(*k) == "Deskbar"));
    assert!(h.find(bar, "Hide Deskbar").is_some());
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

#[test]
fn overflowing_deskbar_scrolls_to_the_shelf_and_tracks_output_size() {
    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    let mut small = output();
    small.frame.y1 = 240.0;
    h.set_outputs(vec![small]);
    let db = window_titled(&h, "Deskbar");
    let size = aizome_shell::deskbar::size(&h.program().deskbar_model());
    assert_eq!(size.height, 240.0 - aizome_shell::menubar::HEIGHT);
    h.resize(db, size);
    assert!(h.find(db, "Drop to park").is_none(), "the fixture overflows");
    h.scroll(db, Point::new(100.0, 100.0), Vec2::new(0.0, 2000.0));
    let after = h.find(db, "Drop to park").expect("park row after scroll");
    assert!(after.y0 >= 0.0 && after.y1 <= size.height, "shelf is reachable: {after:?}");
    h.set_outputs(vec![output()]);
    let expanded = aizome_shell::deskbar::size(&h.program().deskbar_model());
    assert!(expanded.height > size.height && expanded.height < H);
}

#[test]
fn app_groups_can_be_reordered_without_pinning_them() {
    use aizome_shell::actions::Action;
    let mut h = scene(false);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    let from = h.find(db, "Editor").expect("editor").center();
    let to = h.find(db, "Thunderbird").expect("mail").center() + Vec2::new(0.0, 8.0);
    h.drag(db, from, to, 8);
    let ids = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        h.program().deskbar_model().apps.into_iter().map(|a| a.app_id).collect::<Vec<_>>()
    };
    assert_eq!(ids(&h), ["foot", "org.aizome.Hako", "firefox", "thunderbird", "org.aizome.Sumi"]);
    assert!(h.program().deskbar_cfg.pinned.is_empty());
    h.send(Msg::Run(Action::MoveDeskbarApp { app_id: "org.aizome.Sumi".into(), down: false }));
    assert_eq!(ids(&h), ["foot", "org.aizome.Hako", "firefox", "org.aizome.Sumi", "thunderbird"]);
    // Settings reloads keep the user's order, including dormant app preferences.
    let saved = h.program().settings.deskbar_settings().order;
    h.send(Msg::SettingsChanged);
    assert_eq!(ids(&h), saved);
    h.send(Msg::Run(Action::MoveDeskbarApp { app_id: "foot".into(), down: false }));
    assert_eq!(ids(&h), saved, "moving the first app up is a no-op");
}

#[test]
fn reordering_uses_content_coordinates_after_scrolling() {
    let mut h = scene(false);
    let mut output = output();
    output.frame.y1 = 240.0;
    h.set_outputs(vec![output]);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    h.scroll(db, Point::new(100.0, 100.0), Vec2::new(0.0, 60.0));
    let from = h.find(db, "Foot").expect("terminal after scroll").center();
    let to = h.find(db, "Hako").expect("files after scroll").center() + Vec2::new(0.0, 8.0);
    h.drag(db, from, to, 5);
    let order = h.program().deskbar_model().apps.into_iter().map(|a| a.app_id).collect::<Vec<_>>();
    assert_eq!(order, ["org.aizome.Sumi", "org.aizome.Hako", "foot", "firefox", "thunderbird"]);
}

#[test]
fn reordering_tracks_scroll_changes_while_the_button_is_down() {
    use aizome_kit::sidestep_ui::PointerButton;
    let mut h = scene(false);
    let mut output = output();
    output.frame.y1 = 240.0;
    h.set_outputs(vec![output]);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    let from = h.find(db, "Editor").expect("editor").center();
    h.press(db, from, PointerButton::Left, 1);
    h.move_to(db, from + Vec2::new(0.0, 8.0));
    h.scroll(db, from + Vec2::new(0.0, 8.0), Vec2::new(0.0, 100.0));
    let to = h.find(db, "Thunderbird").expect("mail after scroll").center() + Vec2::new(0.0, 8.0);
    h.move_to(db, to);
    h.release(db, to, PointerButton::Left);
    let order = h.program().deskbar_model().apps.into_iter().map(|a| a.app_id).collect::<Vec<_>>();
    assert_eq!(order, ["foot", "org.aizome.Hako", "firefox", "thunderbird", "org.aizome.Sumi"]);
}

#[test]
fn auto_hide_reveals_from_the_bar_and_waits_for_pointer_exit() {
    use std::time::Duration;
    let mut h = scene(false);
    let mut config = h.program().settings.deskbar_settings();
    config.auto_hide = true;
    h.program_mut().settings.set_shell_section("deskbar", &config);
    h.send(Msg::SettingsChanged);
    assert!(h.program().deskbar.is_none());
    let bar = window_titled(&h, "Menu bar");
    let apps = h.find(bar, "5 apps").expect("reveal control").center();
    h.move_to(bar, apps);
    let db = window_titled(&h, "Deskbar");
    h.leave(bar);
    h.enter(db, Point::new(60.0, 20.0));
    h.advance(Duration::from_secs(1));
    assert_eq!(h.program().deskbar, Some(db), "the pointer is over its child row");
    h.leave(db);
    h.advance(Duration::from_millis(200));
    assert_eq!(h.program().deskbar, Some(db), "crossing between bar and Deskbar gets a grace period");
    h.enter(db, Point::new(60.0, 40.0));
    h.advance(Duration::from_millis(200));
    assert_eq!(h.program().deskbar, Some(db), "a stale hide timer must not close it");
    h.leave(db);
    h.advance(Duration::from_millis(400));
    assert!(h.program().deskbar.is_none());
    assert!(!h.program().settings.deskbar_settings().collapsed, "auto-hide is transient");
    config.auto_hide = false;
    h.program_mut().settings.set_shell_section("deskbar", &config);
    h.send(Msg::SettingsChanged);
    assert!(h.program().deskbar.is_some(), "disabling auto-hide restores the Deskbar");
}

#[test]
fn auto_hide_keeps_context_menus_usable() {
    use std::time::Duration;
    let mut h = scene(false);
    h.program_mut().deskbar_cfg.auto_hide = true;
    h.send(Msg::Run(aizome_shell::actions::Action::SetDeskbarCollapsed(false)));
    let db = window_titled(&h, "Deskbar");
    h.enter(db, Point::new(60.0, 20.0));
    h.send(Msg::Deskbar(DeskbarMsg::AppMenu("foot".into(), Point::new(40.0, 40.0))));
    h.leave(db);
    h.advance(Duration::from_secs(1));
    assert_eq!(h.program().deskbar, Some(db), "an open menu holds its parent surface");
    h.key(db, aizome_kit::sidestep_ui::Key::Named(aizome_kit::sidestep_ui::NamedKey::Escape), Default::default());
    h.advance(Duration::from_millis(400));
    assert!(h.program().deskbar.is_none(), "closing the menu permits hiding");
}

#[test]
fn auto_hide_waits_for_a_native_shelf_drag_to_finish_or_cancel() {
    use aizome_kit::sidestep_ui::{DropAction, PointerButton};
    use aizome_shell::actions::Action;
    use std::time::Duration;

    for action in [DropAction::Copy, DropAction::None] {
        let mut h = scene(false);
        with_mail_and_shelf(&mut h);
        h.program_mut().deskbar_cfg.auto_hide = true;
        h.send(Msg::Run(Action::SetDeskbarCollapsed(false)));
        let db = window_titled(&h, "Deskbar");
        h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
        let from = h.find(db, "wallpaper-draft.png").unwrap().center();
        h.enter(db, from);
        h.press(db, from, PointerButton::Left, 1);
        h.move_to(db, from + Vec2::new(-50.0, 0.0));
        assert_eq!(h.drags().len(), 1, "a native drag started");
        let drag = h.drags()[0].0;
        h.leave(db);
        h.advance(Duration::from_secs(1));
        assert_eq!(h.program().deskbar, Some(db), "the native drag holds its source open");
        h.end_drag(drag, action);
        h.advance(Duration::from_millis(200));
        assert_eq!(h.program().deskbar, Some(db), "completion retains the hide grace period");
        h.advance(Duration::from_millis(200));
        assert!(h.program().deskbar.is_none(), "auto-hide resumes after {action:?}");
        assert_eq!(h.program().shelf.items.len(), 3, "a copy or cancellation keeps the Shelf item");
    }
}

#[test]
fn a_native_drag_keeps_its_hold_when_its_shelf_row_disappears() {
    use aizome_kit::sidestep_ui::{DropAction, PointerButton};
    use aizome_shell::actions::Action;
    use std::time::Duration;

    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    h.program_mut().deskbar_cfg.auto_hide = true;
    h.send(Msg::Run(Action::SetDeskbarCollapsed(false)));
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    let from = h.find(db, "wallpaper-draft.png").unwrap().center();
    h.enter(db, from);
    h.press(db, from, PointerButton::Left, 1);
    h.move_to(db, from + Vec2::new(-50.0, 0.0));
    let drag = h.drags()[0].0;
    h.leave(db);
    h.program_mut().shelf.items.clear();
    h.send(Msg::Nothing);
    h.advance(Duration::from_secs(1));
    assert_eq!(h.program().deskbar, Some(db), "the native lifecycle outlasts its source row");
    h.enter(db, Point::new(60.0, 40.0));
    h.end_drag(drag, DropAction::Copy);
    h.advance(Duration::from_secs(1));
    assert_eq!(h.program().deskbar, Some(db), "returning to the Deskbar prevents hiding after the drop");
    h.leave(db);
    h.advance(Duration::from_millis(400));
    assert!(h.program().deskbar.is_none());
}

#[test]
fn auto_hide_waits_for_a_window_drag_to_end() {
    use aizome_shell::widgets::drawn::DragPhase;
    use std::time::Duration;
    let mut h = scene(false);
    h.program_mut().deskbar_cfg.auto_hide = true;
    h.send(Msg::Run(aizome_shell::actions::Action::SetDeskbarCollapsed(false)));
    let db = window_titled(&h, "Deskbar");
    h.enter(db, Point::new(60.0, 40.0));
    h.send(Msg::Deskbar(DeskbarMsg::WindowDrag(1, DragPhase::Start(Point::new(60.0, 40.0)))));
    h.leave(db);
    h.advance(Duration::from_secs(1));
    assert_eq!(h.program().deskbar, Some(db));
    h.send(Msg::Deskbar(DeskbarMsg::WindowDrag(1, DragPhase::End(Point::new(-100.0, -10.0)))));
    h.advance(Duration::from_millis(400));
    assert!(h.program().deskbar.is_none());
}

#[test]
fn auto_hide_resumes_when_a_dragged_app_or_window_disappears() {
    use aizome_kit::sidestep_ui::PointerButton;
    use aizome_shell::actions::Action;
    use aizome_shell::ipc::{IpcEvent, SnapshotBox};
    use std::time::Duration;
    for row in ["Editor", "architecture.md"] {
        let mut h = scene(false);
        h.program_mut().deskbar_cfg.auto_hide = true;
        h.send(Msg::Run(Action::SetDeskbarCollapsed(false)));
        let db = window_titled(&h, "Deskbar");
        h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
        let from = h.find(db, row).expect("drag source").center();
        h.enter(db, from);
        h.press(db, from, PointerButton::Left, 1);
        h.move_to(db, from + Vec2::new(0.0, 8.0));
        let mut snapshot = snapshot();
        snapshot.windows.retain(|w| w.app_id != "org.aizome.Sumi");
        snapshot.focused = None;
        h.send(Msg::Ipc(IpcEvent::Connected(Box::new(SnapshotBox(snapshot)))));
        h.release(db, Point::new(-30.0, 50.0), PointerButton::Left);
        h.leave(db);
        h.advance(Duration::from_secs(1));
        assert!(h.program().deskbar.is_none(), "the vanished {row} row cannot suspend auto-hide forever");
    }
}
