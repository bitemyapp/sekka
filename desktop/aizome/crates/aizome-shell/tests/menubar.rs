//! The menu bar, headless: what it shows for the boards' scene, and its
//! pictures in Night and Day.

mod common;

use aizome_shell::shell::Msg;
use common::*;

#[test]
fn the_bar_shows_the_focused_apps_menus_and_the_desks() {
    let h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let texts = h.texts(bar);
    for want in [
        "Toolchest",
        "Editor",
        "File",
        "Edit",
        "Search",
        "Format",
        "Window",
        "Help",
        "1 Mail",
        "2 Build",
        "3 Write",
        "21:42",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
}

#[test]
fn with_no_window_focused_the_desktop_is_the_app() {
    let mut h = scene(false);
    h.send(Msg::Ipc(aizome_shell::ipc::IpcEvent::Event(aizome_ipc::Event::WindowFocused { id: None })));
    let bar = window_titled(&h, "Menu bar");
    let texts = h.texts(bar);
    assert!(texts.iter().any(|t| t == "Aizome"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Desks"), "{texts:?}");
}

#[test]
fn an_app_without_menus_gets_the_fallback_ones() {
    let mut h = scene(false);
    h.send(Msg::Ipc(aizome_shell::ipc::IpcEvent::Event(aizome_ipc::Event::WindowFocused { id: Some(3) })));
    let bar = window_titled(&h, "Menu bar");
    let texts = h.texts(bar);
    assert!(texts.iter().any(|t| t == "Foot"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Window"), "{texts:?}");
    assert!(!texts.iter().any(|t| t == "Format"), "{texts:?}");
}

#[test]
fn the_clock_tick_repaints_only_the_clock() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    h.take_damage(bar);
    h.program_mut().now.minute = 43;
    h.send(Msg::Nothing);
    let damage = h.take_damage(bar);
    assert!(!damage.is_empty(), "the clock repainted");
    let area: f64 = damage.iter().map(|r| r.area()).sum();
    assert!(area < 60.0 * 24.0, "only the clock: {damage:?}");
    let clock = h.find(bar, "21:43").expect("the clock");
    assert!(damage.iter().all(|d| d.intersect(clock.inflate(4.0, 4.0)).area() > 0.0), "{damage:?} vs {clock:?}");
}

#[test]
fn meters_repaint_only_their_bars() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    h.take_damage(bar);
    let mut m = meters();
    m.cores[0] = 90.0;
    h.send(Msg::Meters(m));
    let damage = h.take_damage(bar);
    let area: f64 = damage.iter().map(|r| r.area()).sum();
    assert!(area > 0.0 && area < 40.0 * 30.0, "{damage:?}");
}

#[test]
fn window_drag_targets_follow_a_header_with_the_deskbar_hide_control() {
    use aizome_kit::Point;
    let mut h = scene(false);
    // Enough ordinary status items for the header to exceed its minimum width.
    h.program_mut().tray.bluetooth = Some((true, true));
    h.program_mut().tray.display = Some(0.8);
    h.send(Msg::Nothing);
    let bar = window_titled(&h, "Menu bar");
    let left = W - aizome_shell::deskbar::WIDTH - aizome_shell::deskbar::SHADOW;
    for (title, index) in [("1 Mail", 1), ("2 Build", 2), ("3 Write", 3)] {
        let r = h.find(bar, title).expect("visible Desk");
        // Include each tab's left edge, where a stale width selects its neighbour.
        for x in [r.x0 + 3.0, r.center().x, r.x1 - 3.0] {
            assert_eq!(h.program().desk_at_deskbar_point(Point::new(x - left, -12.0)), Some(index), "{title}");
        }
    }
}

#[test]
fn pictures_of_the_bar() {
    for day in [false, true] {
        let mut h = scene(day);
        let bar = window_titled(&h, "Menu bar");
        let name = if day { "menubar-day.png" } else { "menubar-night.png" };
        h.save_png(bar, shots().join(name), 2.0);
        aizome_kit::testing::assert_golden(&mut h, bar, golden(name), 2.0);
    }
}
