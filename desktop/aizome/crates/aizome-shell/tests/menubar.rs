//! The menu bar, headless: what it shows for the boards' scene, and its
//! pictures in Night and Day.

mod common;

use aizome_kit::KeyboardInteractivity;
use aizome_kit::platform::{LayerChange, OpenKind};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;
use aizome_shell::menubar::BarMsg;
use aizome_shell::shell::{Msg, Shell};
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

/// Each change `output`'s bar has asked for in when it has the keyboard,
/// in order (it opens on demand).
fn bar_keyboard(h: &Harness<Shell>, output: u32) -> Vec<KeyboardInteractivity> {
    let bar = h
        .platform
        .windows
        .iter()
        .find_map(|(id, w)| match &w.spec.kind {
            OpenKind::Layer(o) if o.namespace == "aizome-menubar" && o.output == Some(output) => Some(*id),
            _ => None,
        })
        .expect("the output's bar");
    h.platform
        .layer_changes
        .iter()
        .filter_map(|(w, c)| match c {
            LayerChange::Keyboard(k) if *w == bar => Some(*k),
            _ => None,
        })
        .collect()
}

/// Right-clicks the Desk shown as `label` in `bar` and picks Rename, its
/// menu's first item.
fn rename_from_the_menu(h: &mut Harness<Shell>, bar: WindowKey, label: &str) {
    let at = h.find(bar, label).expect("the Desk").center();
    h.press(bar, at, PointerButton::Right, 1);
    h.release(bar, at, PointerButton::Right);
    assert_eq!(h.popups(bar).len(), 1, "the Desk's menu");
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert!(h.popups(bar).is_empty(), "the menu closed");
}

/// Rename in a Desk's menu (#231): the menu had the keyboard, and the
/// compositor gives it to the focused window as the menu closes, so the bar
/// asks for it outright, and lets go when the rename is over.
#[test]
fn rename_from_a_desks_menu_takes_the_keyboard_until_it_is_over() {
    use KeyboardInteractivity::{Exclusive, OnDemand};
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let field = aizome_shell::menubar::rename_id("eDP-1", 2);

    // Done, with Return.
    rename_from_the_menu(&mut h, bar, "2 Build");
    assert_eq!(h.program().renaming, Some((2, "Build".into())));
    assert_eq!(bar_keyboard(&h, 1), [Exclusive], "the bar asks for the keyboard");
    assert_eq!(h.focused_name(bar).as_deref(), Some(field.as_str()), "and the field has it in the bar");
    h.focus_window(bar, true);
    h.named(bar, NamedKey::Backspace, Modifiers::SUPER);
    h.type_text(bar, "Ship");
    assert_eq!(h.program().renaming, Some((2, "Ship".into())));
    assert_eq!(bar_keyboard(&h, 1), [Exclusive], "typing asks nothing more");
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().renaming, None);
    assert_eq!(bar_keyboard(&h, 1), [Exclusive, OnDemand], "done: on demand again");
    h.focus_window(bar, false);

    // Given up, with Escape.
    rename_from_the_menu(&mut h, bar, "2 Build");
    h.focus_window(bar, true);
    h.named(bar, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().renaming, None);
    assert_eq!(bar_keyboard(&h, 1), [Exclusive, OnDemand, Exclusive, OnDemand]);
    h.focus_window(bar, false);

    // Given up, as the field says when it's cancelled.
    rename_from_the_menu(&mut h, bar, "1 Mail");
    h.send(Msg::Bar(bar, BarMsg::RenameCancel));
    assert_eq!(h.program().renaming, None);
    assert_eq!(bar_keyboard(&h, 1).len(), 6);
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&OnDemand));

    // The bar lost the keyboard (the lock, the palette: what's above it).
    rename_from_the_menu(&mut h, bar, "1 Mail");
    h.focus_window(bar, true);
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&Exclusive));
    h.focus_window(bar, false);
    assert_eq!(h.program().renaming, None, "the rename is over with it");
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&OnDemand));

    // A click beside the field, in the bar: the field hears nothing more
    // (Escape neither), so the bar mustn't keep the keyboard.
    rename_from_the_menu(&mut h, bar, "2 Build");
    h.focus_window(bar, true);
    let beside = h.find(bar, "1 Mail").expect("the first Desk").x0 - 40.0;
    h.click(bar, Point::new(beside, 12.0));
    assert_eq!(h.program().renaming, None, "the rename is over with the field's focus");
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&OnDemand));
    assert_eq!(bar_keyboard(&h, 1).len(), 10);
    assert_eq!(h.program().rename_keys, None);
}

/// Another Desk double-clicked while one is being renamed from its menu:
/// the field moves, with the focus, and the bar still has the keyboard.
#[test]
fn another_desk_renamed_meanwhile_gets_the_field_and_the_keys() {
    use KeyboardInteractivity::{Exclusive, OnDemand};
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    rename_from_the_menu(&mut h, bar, "2 Build");
    h.focus_window(bar, true);
    let at = h.find(bar, "1 Mail").expect("the Desk").center();
    h.press(bar, at, PointerButton::Left, 2);
    h.release(bar, at, PointerButton::Left);
    assert_eq!(h.program().renaming, Some((1, "Mail".into())));
    assert_eq!(h.focused_name(bar), Some(aizome_shell::menubar::rename_id("eDP-1", 1)));
    assert_eq!(bar_keyboard(&h, 1), [Exclusive]);
    h.type_text(bar, "s");
    assert_eq!(h.program().renaming, Some((1, "Mails".into())));
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(bar_keyboard(&h, 1), [Exclusive, OnDemand]);
}

/// A double click gave the bar the keyboard already: it stays on demand,
/// never exclusive, and a click elsewhere (the compositor takes the
/// keyboard back) ends the rename.
#[test]
fn a_double_click_renames_without_asking_for_the_keyboard() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let at = h.find(bar, "2 Build").expect("the Desk").center();
    h.focus_window(bar, true);
    h.press(bar, at, PointerButton::Left, 2);
    h.release(bar, at, PointerButton::Left);
    assert_eq!(h.program().renaming, Some((2, "Build".into())));
    assert_eq!(h.focused_name(bar), Some(aizome_shell::menubar::rename_id("eDP-1", 2)));
    h.named(bar, NamedKey::Backspace, Modifiers::SUPER);
    h.type_text(bar, "Ship");
    assert_eq!(h.program().renaming, Some((2, "Ship".into())));
    h.focus_window(bar, false);
    assert_eq!(h.program().renaming, None, "a click elsewhere gives it up");
    assert!(bar_keyboard(&h, 1).is_empty(), "the bar's keyboard was never asked for");
}

/// Each display has a bar: the one whose menu Rename was chosen from asks
/// for the keyboard, and its field is the one typed into.
#[test]
fn on_two_displays_the_bar_whose_menu_it_was_takes_the_keyboard() {
    use KeyboardInteractivity::{Exclusive, OnDemand};
    let mut h = scene(false);
    let other = aizome_kit::Output {
        id: 2,
        name: "DP-1".into(),
        connector: Some("DP-1".into()),
        frame: Rect::new(W, 0.0, 2.0 * W, H),
        ..output()
    };
    h.set_outputs(vec![output(), other]);
    let bar_on = |h: &Harness<Shell>, id: u32| {
        h.program().screens.iter().find(|s| s.output.id == id).and_then(|s| s.bar).expect("the display's bar")
    };
    let (first, second) = (bar_on(&h, 1), bar_on(&h, 2));
    h.resize(second, Size::new(W, 24.0));

    rename_from_the_menu(&mut h, second, "2 Build");
    assert_eq!(bar_keyboard(&h, 2), [Exclusive]);
    assert!(bar_keyboard(&h, 1).is_empty(), "the other display's bar is left alone");
    assert_eq!(h.focused_name(second), Some(aizome_shell::menubar::rename_id("DP-1", 2)));
    assert_eq!(h.focused_name(first), None);
    h.focus_window(second, true);
    h.named(second, NamedKey::Backspace, Modifiers::SUPER);
    h.type_text(second, "Ship");
    assert_eq!(h.program().renaming, Some((2, "Ship".into())));
    h.named(second, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().renaming, None);
    assert_eq!(bar_keyboard(&h, 2), [Exclusive, OnDemand]);
    assert!(bar_keyboard(&h, 1).is_empty());
}

/// A surface that asked for the keyboard outright keeps it from a window
/// focused later and, on another display, from the lock and the shell's
/// own dialogs: the shell ends the rename for them, and when the Desk goes
/// (no field is left to type into, or to hear Escape).
#[test]
fn a_rename_holding_the_keyboard_gives_way_to_a_window_focused_since_and_to_the_lock() {
    use KeyboardInteractivity::OnDemand;
    use aizome_ipc::Event;
    let event = |e| Msg::Ipc(aizome_shell::ipc::IpcEvent::Event(e));
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");

    rename_from_the_menu(&mut h, bar, "2 Build");
    // The focused window said again changes nothing.
    h.send(event(Event::WindowFocused { id: Some(1) }));
    assert!(h.program().renaming.is_some());
    h.send(event(Event::WindowFocused { id: Some(3) }));
    assert_eq!(h.program().renaming, None, "the window gets what's typed next");
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&OnDemand));

    rename_from_the_menu(&mut h, bar, "2 Build");
    let mut desks = snapshot().desks;
    desks.retain(|d| d.index != 2);
    h.send(event(Event::DesksChanged { desks }));
    assert_eq!(h.program().renaming, None, "the Desk went");
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&OnDemand));

    rename_from_the_menu(&mut h, bar, "1 Mail");
    h.send(event(Event::LockStateChanged { locked: true }));
    assert_eq!(h.program().renaming, None, "nothing is renamed under the lock");
    assert_eq!(bar_keyboard(&h, 1).last(), Some(&OnDemand));
    assert_eq!(h.program().rename_keys, None);
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
