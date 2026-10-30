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

/// The open Deskbar's layer options.
fn deskbar_layer(h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>) -> aizome_kit::LayerOptions {
    let w = h.platform.windows.values().rfind(|w| w.title == "Deskbar").expect("a Deskbar");
    match &w.spec.kind {
        aizome_kit::platform::OpenKind::Layer(o) => o.clone(),
        other => panic!("the Deskbar isn't a layer: {other:?}"),
    }
}

#[test]
fn the_deskbar_moves_to_the_left_and_widens() {
    use aizome_config::DeskbarSide;
    use aizome_kit::Anchors;
    use aizome_shell::deskbar::{SHADOW, WIDTH};

    let mut h = scene(false);
    assert!(deskbar_layer(&h).anchors.contains(Anchors::TOP | Anchors::RIGHT));
    let mut d = h.program().settings.deskbar_settings();
    assert_eq!((d.side, d.width), (DeskbarSide::Right, WIDTH), "the defaults are today's Deskbar");

    d.side = DeskbarSide::Left;
    d.width = 300.0;
    h.program_mut().settings.set_shell_section("deskbar", &d);
    h.send(Msg::SettingsChanged);

    let o = deskbar_layer(&h);
    assert!(o.anchors.contains(Anchors::TOP | Anchors::LEFT), "{:?}", o.anchors);
    assert!(!o.anchors.contains(Anchors::RIGHT), "{:?}", o.anchors);
    let m = h.program().deskbar_model();
    assert_eq!(aizome_shell::deskbar::size(&m).width, 300.0 + SHADOW);
    // The face starts at the surface's left edge; the shadow falls right.
    assert_eq!(aizome_shell::deskbar::input_region(&m)[0].x0, 0.0);
    assert_eq!(aizome_shell::deskbar::input_region(&m)[0].x1, 300.0);
    // A window dragged up onto the strip still finds its Desk: the surface
    // is flush with the output's left edge now.
    let s = h.program();
    let hits: Vec<Option<u32>> =
        (0..300).map(|x| s.desk_at_deskbar_point(Point::new(900.0 + x as f64, -12.0))).collect();
    assert!(hits.contains(&Some(1)) && hits.contains(&Some(2)) && hits.contains(&Some(3)), "{hits:?}");
    // The systray header no longer follows a Deskbar that isn't under it.
    assert_eq!(s.header_width(), aizome_shell::menubar::HEADER);

    // Back at the right, wider: the header follows it.
    let mut d = h.program().settings.deskbar_settings();
    d.side = DeskbarSide::Right;
    h.program_mut().settings.set_shell_section("deskbar", &d);
    h.send(Msg::SettingsChanged);
    assert!(deskbar_layer(&h).anchors.contains(Anchors::TOP | Anchors::RIGHT));
    assert_eq!(h.program().header_width(), 300.0);
}

#[test]
fn deskbar_widths_stay_in_range() {
    use aizome_shell::deskbar::{
        Density, MAX_WIDTH, RAIL_WIDTH, SLIM_WIDTH, WIDTH, clamp_width, density, fit_width, most_width,
    };
    assert_eq!(clamp_width(60.0), RAIL_WIDTH, "narrow snaps to the Rail");
    assert_eq!(clamp_width(100.0), SLIM_WIDTH, "past half way, Slim");
    assert_eq!((density(48.0), density(150.0), density(232.0)), (Density::Rail, Density::Slim, Density::Full));
    assert_eq!(clamp_width(9000.0), MAX_WIDTH);
    assert_eq!(clamp_width(301.4), 301.0);
    assert_eq!(clamp_width(f64::NAN), WIDTH);
    // Within a narrow display's share, and the Rail when not even Slim fits.
    assert_eq!((most_width(1440.0), most_width(800.0), most_width(f64::NAN)), (MAX_WIDTH, 360.0, MAX_WIDTH));
    assert_eq!(fit_width(400.0, most_width(800.0)), 360.0);
    assert_eq!(fit_width(232.0, most_width(800.0)), 232.0);
    assert_eq!(fit_width(48.0, most_width(800.0)), RAIL_WIDTH);
    assert_eq!(fit_width(150.0, 100.0), RAIL_WIDTH);
    assert_eq!(fit_width(150.0, 130.0), 130.0);
}

/// Moves the Deskbar to the left, as Settei would.
fn to_the_left(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, auto_hide: bool) {
    let mut d = h.program().settings.deskbar_settings();
    d.side = aizome_config::DeskbarSide::Left;
    d.auto_hide = auto_hide;
    h.program_mut().settings.set_shell_section("deskbar", &d);
    h.send(Msg::SettingsChanged);
}

#[test]
fn the_edge_follows_the_pointer_from_where_it_was_pressed() {
    use aizome_kit::platform::LayerChange;
    use aizome_shell::widgets::drawn::DragPhase::{End, Move, Start};
    let edge = |phase| Msg::Deskbar(DeskbarMsg::Edge(phase));
    let sizes = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        h.platform.layer_changes.iter().filter(|(_, c)| matches!(c, LayerChange::Size(_))).count()
    };
    let width = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        h.platform.layer_changes.iter().rev().find_map(|(_, c)| match c {
            LayerChange::Size(s) => Some(s.width),
            _ => None,
        })
    };
    // Only how far the pointer went from the press counts: the compositor
    // reports it from where the surface was then.
    let mut h = scene(false);
    let resized = sizes(&h);
    h.send(edge(Start(Point::new(5.0, 100.0))));
    for dx in [20.0, 44.0, 66.0, 88.0] {
        h.send(edge(Move(Point::new(5.0 - dx, 100.0))));
        assert_eq!(h.program().deskbar_preview_width(), Some(232.0 + dx), "{dx} left of the press");
        // The Deskbar keeps its size while the button's held, so the
        // compositor doesn't fit the windows beside it at every step.
        assert_eq!(h.program().deskbar_model().width, 232.0, "{dx}: the Deskbar waits for the release");
        assert_eq!(sizes(&h), resized, "{dx}: no layer resize during the drag");
    }
    assert!(h.windows().iter().any(|k| h.title(*k) == "Deskbar width"), "the width shows over it");
    h.send(edge(End(Point::new(5.0 - 88.0, 100.0))));
    assert_eq!(h.program().settings.deskbar_settings().width, 320.0, "saved where it was let go");
    assert_eq!(h.program().deskbar_model().width, 320.0);
    assert_eq!(h.program().deskbar_preview_width(), None);
    assert!(h.windows().iter().all(|k| h.title(*k) != "Deskbar width"), "the preview goes");
    assert_eq!(sizes(&h), resized + 1, "one resize, when let go");
    assert_eq!(width(&h), Some(320.0 + aizome_shell::deskbar::SHADOW));

    // Out and back without letting go: nothing changes, nothing's saved.
    let resized = sizes(&h);
    h.send(edge(Start(Point::new(5.0, 100.0))));
    h.send(edge(Move(Point::new(-75.0, 100.0))));
    h.send(edge(Move(Point::new(5.0, 100.0))));
    h.send(edge(End(Point::new(5.0, 100.0))));
    assert_eq!(sizes(&h), resized, "an edge dragged out and back resizes nothing");
    assert_eq!(h.program().settings.deskbar_settings().width, 320.0);

    // At the left the surface stays put, and the edge follows the same way.
    to_the_left(&mut h, false);
    h.send(edge(Start(Point::new(318.0, 100.0))));
    h.send(edge(Move(Point::new(258.0, 100.0))));
    assert_eq!(h.program().deskbar_preview_width(), Some(260.0));
    h.send(edge(End(Point::new(238.0, 100.0))));
    assert_eq!(h.program().settings.deskbar_settings().width, 240.0);
}

#[test]
fn an_edge_drag_ends_with_the_deskbar() {
    use aizome_shell::actions::Action;
    use aizome_shell::widgets::drawn::DragPhase::{Move, Start};
    let mut h = scene(false);
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Start(Point::new(5.0, 100.0)))));
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Move(Point::new(-40.0, 100.0)))));
    assert!(h.program().deskbar_preview_width().is_some());
    // Folded away mid-drag: its release can't come, and the preview goes.
    h.send(Msg::Run(Action::SetDeskbarCollapsed(true)));
    assert_eq!(h.program().deskbar_preview_width(), None);
    assert!(h.windows().iter().all(|k| h.title(*k) != "Deskbar width"));
    h.send(Msg::Run(Action::SetDeskbarCollapsed(false)));
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Move(Point::new(-80.0, 100.0)))));
    assert_eq!(h.program().deskbar_preview_width(), None, "the old drag is over");
    assert_eq!(h.program().settings.deskbar_settings().width, 232.0);

    // Its display going goes the same way.
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Start(Point::new(5.0, 100.0)))));
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Move(Point::new(-40.0, 100.0)))));
    assert!(h.program().deskbar_preview_width().is_some());
    h.set_outputs(Vec::new());
    assert_eq!(h.program().deskbar_preview_width(), None);
    h.set_outputs(vec![output()]);
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Move(Point::new(-80.0, 100.0)))));
    assert_eq!(h.program().deskbar_preview_width(), None);
    assert!(h.windows().iter().all(|k| h.title(*k) != "Deskbar width"));
    assert_eq!(h.program().settings.deskbar_settings().width, 232.0);
}

#[test]
fn locking_the_screen_ends_an_edge_drag_where_it_began() {
    use aizome_kit::sidestep_ui::PointerButton;
    use aizome_shell::actions::{Action, SessionVerb};
    use aizome_shell::deskbar::{SHADOW, WIDTH};
    let mut h = scene(false);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    let at = Point::new(SHADOW + 2.0, h.find(db, "Foot").expect("Foot's row").center().y);
    h.enter(db, at);
    h.press(db, at, PointerButton::Left, 1);
    h.move_to(db, at - Vec2::new(60.0, 0.0));
    assert_eq!(h.program().deskbar_preview_width(), Some(WIDTH + 60.0));
    h.send(Msg::Run(Action::Session(SessionVerb::Lock)));
    assert_eq!(h.program().deskbar_preview_width(), None, "the outline goes with the lock");
    assert!(h.windows().iter().all(|k| h.title(*k) != "Deskbar width"));
    // The release still comes to the Deskbar, and takes nothing.
    h.release(db, at - Vec2::new(60.0, 0.0), PointerButton::Left);
    assert_eq!(h.program().settings.deskbar_settings().width, WIDTH);
    assert_eq!(h.program().deskbar_width(), WIDTH);
}

#[test]
fn the_grip_menu_says_how_wide_a_narrowed_deskbar_is() {
    use aizome_shell::deskbar::MAX_WIDTH;
    use aizome_shell::menus::model::{Entry, Kind};
    let sizes = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        let items = h.program().deskbar_place_menu().items;
        // Past At the Left and At the Right.
        items.into_iter().filter(|e| e.kind == Kind::Action).skip(2).collect::<Vec<_>>()
    };
    let ticked = |items: &[Entry]| items.iter().filter(|e| e.checked).map(|e| e.label.clone()).collect::<Vec<_>>();
    let set_width = |h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, w: f64| {
        let mut d = h.program().settings.deskbar_settings();
        d.width = w;
        h.program_mut().settings.set_shell_section("deskbar", &d);
        h.send(Msg::SettingsChanged);
    };
    let mut h = scene(false);
    set_width(&mut h, 360.0);
    let items: Vec<Entry> = sizes(&h);
    assert_eq!(ticked(&items), ["Wider"], "room for it: the size it is");
    assert_eq!(items.len(), 5, "no line about the display: {items:?}");

    // 400pt asked for, on an 800pt display: 360pt, which is Wider's width,
    // but Wider isn't what was asked for.
    set_width(&mut h, MAX_WIDTH);
    let mut narrow = output();
    narrow.frame.x1 = 800.0;
    h.set_outputs(vec![narrow.clone()]);
    let items: Vec<Entry> = sizes(&h);
    assert_eq!(ticked(&items), ["As Wide as This Display Allows"]);
    let line = items.last().unwrap();
    assert_eq!((line.detail.as_deref(), line.enabled), (Some("360pt"), false));
    let wider = items.iter().find(|e| e.label == "Wider").unwrap();
    assert!(wider.enabled, "Wider can still be chosen: it fits");

    // Wide (300pt) on a 600pt display: 270pt, and Wide doesn't fit.
    set_width(&mut h, 300.0);
    narrow.frame.x1 = 600.0;
    h.set_outputs(vec![narrow]);
    let items: Vec<Entry> = sizes(&h);
    assert_eq!(ticked(&items), ["As Wide as This Display Allows"]);
    assert_eq!(items.last().unwrap().detail.as_deref(), Some("270pt"));
    assert!(!items.iter().find(|e| e.label == "Wide").unwrap().enabled);
    assert_eq!(h.program().settings.deskbar_settings().width, 300.0, "the setting stays");

    // Back on the wide display: Wide again.
    h.set_outputs(vec![output()]);
    let items: Vec<Entry> = sizes(&h);
    assert_eq!(ticked(&items), ["Wide"]);
    assert_eq!(items.len(), 5);
}

#[test]
fn rows_under_the_edge_show_their_hover_and_tooltip() {
    use aizome_shell::deskbar::{RAIL_WIDTH, SHADOW};
    use std::time::Duration;
    let mut h = scene(false);
    let mut d = h.program().settings.deskbar_settings();
    d.width = RAIL_WIDTH;
    h.program_mut().settings.set_shell_section("deskbar", &d);
    h.send(Msg::SettingsChanged);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    let tile = h.find(db, "Foot").expect("Foot's tile");
    // A pixel of the tile's background, clear of its icon and the edge, and
    // one where the edge draws its line.
    let (bg, line) = (Point::new(tile.x1 - 3.0, tile.y0 + 2.0), Point::new(SHADOW + 2.0, tile.y0 + 2.0));
    // The tooltips showing, by what they hang from.
    let tips = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        h.platform
            .windows
            .values()
            .filter_map(|w| match w.spec.kind {
                aizome_kit::platform::OpenKind::Popup { anchor, .. } => Some(anchor),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let pixels = |h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        let (w, _, rgba) = h.render_rgba(db, 1.0);
        let at = |p: Point| {
            let i = (p.y as usize * w as usize + p.x as usize) * 4;
            rgba[i..i + 4].to_vec()
        };
        (at(bg), at(line))
    };
    let (bg_still, line_still) = pixels(&mut h);
    h.enter(db, tile.center());
    let (bg_over, line_over) = pixels(&mut h);
    assert_ne!(bg_over, bg_still, "the tile shows it's hovered");
    h.advance(Duration::from_millis(700));
    let name = tips(&h);
    assert_eq!(name.len(), 1, "the tile's name, over its middle");
    // Under the edge: the edge shows its line, and the tile under it its
    // hover and its name, as in its middle.
    h.move_to(db, Point::new(SHADOW + 2.0, tile.center().y));
    assert!(tips(&h).is_empty(), "moved onto the edge, the tooltip waits again");
    let (bg_under, line_under) = pixels(&mut h);
    assert_ne!(line_under, line_over, "the edge's line");
    assert_eq!(bg_under, bg_over, "the tile's hover, under the edge");
    h.advance(Duration::from_millis(700));
    assert_eq!(tips(&h), name, "the tile's name, under the edge");
    // Off the Deskbar, both go.
    h.leave(db);
    assert!(tips(&h).is_empty());
    let (bg_gone, line_gone) = pixels(&mut h);
    assert_eq!((bg_gone, line_gone), (bg_still, line_still));
}

#[test]
fn a_narrow_display_keeps_the_deskbar_beside_the_windows() {
    use aizome_shell::deskbar::{MAX_WIDTH, SHADOW};
    use aizome_shell::widgets::drawn::DragPhase::{End, Move, Start};
    let mut h = scene(false);
    let mut d = h.program().settings.deskbar_settings();
    d.width = MAX_WIDTH;
    h.program_mut().settings.set_shell_section("deskbar", &d);
    h.send(Msg::SettingsChanged);
    assert_eq!(h.program().deskbar_width(), MAX_WIDTH, "room for it on the 1440pt display");
    // An 800pt display (1600 × 900 at 2×): 45% of it, so the Deskbar and
    // its shadow stand in one half and the compositor still sets filled
    // windows beside it.
    let mut narrow = output();
    narrow.frame.x1 = 800.0;
    h.set_outputs(vec![narrow]);
    let m = h.program().deskbar_model();
    assert_eq!(m.width, 360.0);
    assert!(aizome_shell::deskbar::size(&m).width < 400.0, "{m:?}");
    assert_eq!(h.program().settings.deskbar_settings().width, MAX_WIDTH, "the setting stays");
    // An edge drag can't take it past that either.
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Start(Point::new(5.0, 100.0)))));
    h.send(Msg::Deskbar(DeskbarMsg::Edge(Move(Point::new(-200.0, 100.0)))));
    assert_eq!(h.program().deskbar_preview_width(), Some(360.0));
    h.send(Msg::Deskbar(DeskbarMsg::Edge(End(Point::new(-200.0, 100.0)))));
    assert_eq!(h.program().settings.deskbar_settings().width, MAX_WIDTH, "nothing to save: it was as wide");
    assert_eq!(h.program().deskbar_width() + SHADOW, 363.0);
    // Back on the wide display, its width comes back.
    h.set_outputs(vec![output()]);
    assert_eq!(h.program().deskbar_width(), MAX_WIDTH);
}

#[test]
fn a_click_on_the_inner_edge_is_the_rows() {
    use aizome_kit::sidestep_ui::PointerButton;
    use aizome_shell::deskbar::SHADOW;
    let mut h = scene(false);
    // Firefox's windows start folded; its count, at the row's right end,
    // unfolds them. At the left, that end is under the inner edge.
    to_the_left(&mut h, false);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    assert!(!h.texts(db).iter().any(|t| t == "docs.rs"), "folded: {:?}", h.texts(db));
    let firefox = h.find(db, "Firefox").expect("Firefox's row");
    let width = h.program().deskbar_width();
    let at = Point::new(width - 2.0, firefox.center().y);
    h.click(db, at);
    assert!(h.texts(db).iter().any(|t| t == "docs.rs"), "the click reached the count: {:?}", h.texts(db));
    // A drag from the same place resizes, and the row doesn't take it.
    h.press(db, at, PointerButton::Left, 1);
    h.move_to(db, at + Vec2::new(30.0, 0.0));
    assert_eq!(h.program().deskbar_preview_width(), Some(width + 30.0));
    h.release(db, at + Vec2::new(30.0, 0.0), PointerButton::Left);
    assert_eq!(h.program().settings.deskbar_settings().width, width + 30.0);
    assert!(h.texts(db).iter().any(|t| t == "docs.rs"), "still unfolded");

    // At the right, the edge is over the rows' left end: a right-click
    // there opens the app's menu, and the wheel scrolls the list.
    let mut h = scene(false);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    let foot = h.find(db, "Foot").expect("Foot's row");
    let at = Point::new(SHADOW + 2.0, foot.center().y);
    assert!(h.popups(db).is_empty());
    h.press(db, at, PointerButton::Right, 1);
    h.release(db, at, PointerButton::Right);
    assert_eq!(h.popups(db).len(), 1, "the app's menu, from under the edge");

    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    let mut small = output();
    small.frame.y1 = 240.0;
    h.set_outputs(vec![small]);
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    assert!(h.find(db, "Drop to park").is_none(), "the fixture overflows");
    h.scroll(db, Point::new(SHADOW + 2.0, 100.0), Vec2::new(0.0, 2000.0));
    assert!(h.find(db, "Drop to park").is_some(), "the wheel over the edge scrolls the rows");
}

#[test]
fn desktop_icons_make_room_for_a_deskbar_at_the_left_that_stays() {
    use aizome_shell::actions::Action;
    let mut h = scene(false);
    let output = h.program().screens[0].output.id;
    to_the_left(&mut h, false);
    assert_eq!(h.program().desktop_left(output), 232.0 + aizome_shell::deskbar::SHADOW);
    // One that hides itself overlays them while it shows, as it does windows.
    to_the_left(&mut h, true);
    h.send(Msg::Run(Action::SetDeskbarCollapsed(false)));
    assert!(h.windows().iter().any(|k| h.title(*k) == "Deskbar"), "revealed");
    assert_eq!(h.program().desktop_left(output), 0.0);
}

#[test]
fn the_deskbar_controls_follow_it_to_the_left() {
    use aizome_shell::menubar::Slot;
    let mut h = scene(false);
    to_the_left(&mut h, false);
    // The settings read again, the sample tray back.
    h.program_mut().tray = tray();
    h.send(Msg::Nothing);
    let bar = window_titled(&h, "Menu bar");
    // After the Toolchest, over the Deskbar.
    let hide = h.find(bar, "Hide Deskbar").expect("the hide control");
    assert!(hide.x1 < 232.0, "{hide:?}");
    h.click(bar, hide.center());
    assert!(h.program().deskbar_cfg.collapsed);
    let show = h.find(bar, "5 apps").expect("the show control");
    assert!(show.x1 < 232.0, "{show:?}");
    // The header keeps its widgets, and the menus start past the control,
    // where the bar's geometry says.
    assert!(h.find(bar, "Network").is_some(), "{:?}", h.texts(bar));
    let theme = h.theme().clone();
    let titles = h.program_mut().title_rects(bar, &theme);
    let app = titles.iter().find(|(s, _)| *s == Slot::Menu(0)).expect("the app's name").1;
    assert!(app.x0 >= show.x1, "{app:?} after {show:?}");
    assert_eq!(
        h.find(bar, "Format").map(|r| (r.x0, r.x1)),
        titles.iter().find(|(s, _)| *s == Slot::Menu(4)).map(|t| (t.1.x0, t.1.x1))
    );
    // A window dragged up onto the strip still finds its Desk.
    let mail = h.find(bar, "1 Mail").expect("Desk 1 in the strip");
    assert_eq!(h.program().desk_at_deskbar_point(Point::new(mail.center().x, -12.0)), Some(1));
    h.click(bar, show.center());
    assert!(!h.program().deskbar_cfg.collapsed);
}

#[test]
fn critical_cards_stay_beside_the_deskbar_as_it_moves() {
    use aizome_kit::platform::LayerChange;
    use aizome_shell::notifications::server::NoteEvent;
    use aizome_shell::notifications::store::{Incoming, Urgency};
    let mut h = scene(false);
    let n = Incoming {
        app_name: "Power".into(),
        summary: "Battery low".into(),
        urgency: Urgency::Critical,
        ..Default::default()
    };
    h.send(Msg::Note(NoteEvent::Notify(40, Box::new(n))));
    let right = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| h.program().cards.map(|c| c.2);
    assert_eq!(right(&h), Some(232.0 + aizome_shell::deskbar::SHADOW + 12.0), "beside the Deskbar");
    to_the_left(&mut h, false);
    assert_eq!(right(&h), Some(12.0), "the Deskbar went left");
    // The only layer to change its margins: the cards', moving over.
    let margins = h.platform.layer_changes.iter().rev().find_map(|(_, c)| match c {
        LayerChange::Margins(m) => Some(*m),
        _ => None,
    });
    assert_eq!(margins.map(|m| m.right), Some(12), "{margins:?}");
}

#[test]
fn a_revealed_deskbar_moved_by_its_grip_still_hides() {
    use aizome_shell::actions::Action;
    use aizome_shell::widgets::drawn::DragPhase;
    use std::time::Duration;
    let mut h = scene(false);
    h.program_mut().deskbar_cfg.auto_hide = true;
    h.send(Msg::Run(Action::SetDeskbarCollapsed(false)));
    let db = window_titled(&h, "Deskbar");
    h.resize(db, aizome_shell::deskbar::size(&h.program().deskbar_model()));
    h.enter(db, Point::new(116.0, 5.0));
    // Let go over the left half: it opens again there, away from the
    // pointer, and the surface it left can't say the pointer went.
    h.send(Msg::Deskbar(DeskbarMsg::Grip(DragPhase::End(Point::new(-900.0, 5.0)))));
    assert_eq!(h.program().deskbar_cfg.side, aizome_config::DeskbarSide::Left);
    assert!(h.program().deskbar.is_some(), "still revealed");
    h.advance(Duration::from_secs(1));
    assert!(h.program().deskbar.is_none(), "hidden again");
}

#[test]
fn the_compact_deskbar_ends_under_its_shelf() {
    for width in [150.0, 48.0] {
        let mut h = scene(false);
        with_mail_and_shelf(&mut h);
        let mut d = h.program().settings.deskbar_settings();
        d.width = width;
        h.program_mut().settings.set_shell_section("deskbar", &d);
        h.send(Msg::SettingsChanged);
        let db = window_titled(&h, "Deskbar");
        let m = h.program().deskbar_model();
        h.resize(db, aizome_shell::deskbar::size(&m));
        let drop = h.find(db, "Drop to park").expect("the drop target");
        // Its 6pt of padding, then the shadow.
        assert_eq!(drop.y1 + 6.0 + aizome_shell::deskbar::SHADOW, m.height(), "{width} wide");
    }
}
