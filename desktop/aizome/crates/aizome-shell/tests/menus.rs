//! Menus, headless: the Toolchest and the menu at the pointer as kit
//! popups of the bar, torn-off palettes, and their pictures.

mod common;

use std::time::Duration;

use aizome_ipc::Event;
use aizome_kit::prelude::*;
use aizome_shell::ipc::IpcEvent;
use aizome_shell::menubar::{BarMsg, Slot};
use aizome_shell::shell::Msg;
use aizome_shell::tearoff::TearMsg;
use common::*;

fn pointer_menu(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, x: f64, y: f64) {
    h.send(Msg::Ipc(IpcEvent::Event(Event::Binding {
        action: "app-menu".into(),
        pointer: Some((x, y)),
        output: Some("eDP-1".into()),
    })));
    // Services wait for the selected text, at most 150 ms.
    h.advance(Duration::from_millis(200));
}

#[test]
fn the_menu_at_the_pointer_gathers_the_apps_menus() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    pointer_menu(&mut h, 600.0, 300.0);
    let popups = h.popups(bar);
    assert_eq!(popups.len(), 1, "a menu opened");
    let (_menu, at) = popups[0];
    assert_eq!(at.origin(), Point::new(600.0, 300.0));
    // (The kit's menus draw their rows themselves: check what was shown.)
    let (name, menus) = h.program().focused_menus();
    let p = aizome_shell::menus::build::pointer_menu(
        &name,
        &menus,
        vec![],
        aizome_shell::actions::Action::None,
        aizome_shell::actions::Action::None,
    );
    let labels: Vec<&str> = p.items.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["Info", "File", "Edit", "Search", "Format", "Window", "Help", "Services", "Hide", "Quit"]);
}

#[test]
fn the_toolchest_opens_under_its_button() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    h.send(Msg::Bar(bar, BarMsg::Press(Slot::Toolchest, Rect::new(0.0, 0.0, 92.0, 24.0))));
    let (_menu, at) = h.popups(bar)[0];
    assert_eq!(at.origin(), Point::new(0.0, 23.0));
}

#[test]
fn torn_off_menus_stay_with_their_app() {
    let mut h = scene(false);
    h.send(Msg::TearOff {
        app: "org.aizome.Sumi".into(),
        path: vec!["Format".into()],
        output: "eDP-1".into(),
        at: Point::new(104.0, 412.0),
    });
    let _p = window_titled(&h, "Palette (torn off)");
    let (title, menu) = h.program().tear_menu(&h.program().tearoffs[0]).expect("its menu");
    assert_eq!(title, "Format");
    assert!(menu.items.iter().any(|e| e.label == "Bold"));
    // Another app focused: it hides; pinned, it stays.
    h.send(Msg::Ipc(IpcEvent::Event(Event::WindowFocused { id: Some(3) })));
    assert!(h.windows().iter().all(|k| h.title(*k) != "Palette (torn off)"));
    h.send(Msg::Tear(TearMsg::Pin(0, true)));
    assert!(h.windows().iter().any(|k| h.title(*k) == "Palette (torn off)"));
    h.send(Msg::Tear(TearMsg::Close(0)));
    assert!(h.program().tearoffs.is_empty());
}

#[test]
fn a_palette_follows_its_strip_and_stays_on_screen() {
    use aizome_shell::widgets::drawn::DragPhase;
    let mut h = scene(false);
    h.send(Msg::TearOff {
        app: "org.aizome.Sumi".into(),
        path: vec!["Format".into()],
        output: "eDP-1".into(),
        at: Point::new(104.0, 412.0),
    });
    let at = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        let t = &h.program().tearoffs[0];
        (t.x, t.y)
    };
    // Through the drag the pointer's place comes relative to where the
    // palette was at the press (the compositor keeps that surface's place
    // for the press), however often it moves: the palette goes as far as
    // the pointer has, once.
    h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::Start(Point::new(60.0, 10.0)))));
    for y in [20.0, 40.0, 70.0, 100.0] {
        h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::Move(Point::new(60.0, y)))));
    }
    assert_eq!(at(&h), (104.0, 502.0));
    h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::End(Point::new(30.0, 100.0)))));
    assert_eq!(at(&h), (74.0, 502.0));
    // Far past the screen's edges, it stops at them, whole.
    h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::Start(Point::new(60.0, 10.0)))));
    h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::End(Point::new(5000.0, 5000.0)))));
    let (x, y) = at(&h);
    let size = h.size(window_titled(&h, "Palette (torn off)"));
    assert_eq!((x + size.width, y + size.height), (W, H), "the palette sits in the bottom right corner");
    h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::Start(Point::new(60.0, 10.0)))));
    h.send(Msg::Tear(TearMsg::Drag(0, DragPhase::End(Point::new(-5000.0, -5000.0)))));
    assert_eq!(at(&h), (0.0, 24.0), "below the bar");
}

#[test]
fn pictures_of_menus() {
    for day in [false, true] {
        let mut h = scene(day);
        let bar = window_titled(&h, "Menu bar");
        let suffix = if day { "day" } else { "night" };
        // The menu at the pointer, its Edit submenu open.
        pointer_menu(&mut h, 600.0, 160.0);
        let (menu, _) = h.popups(bar)[0];
        let img = h.render_popup(menu, 2.0);
        let (w, hh) = img.pixel_size();
        aizome_kit::testing::write_png(
            &shots().join(format!("menu-pointer-{suffix}.png")),
            w,
            hh,
            &img.to_rgba8().unwrap(),
        )
        .unwrap();
        h.send(Msg::Run(aizome_shell::actions::Action::None));
        // The Toolchest.
        h.send(Msg::Bar(bar, BarMsg::Press(Slot::Toolchest, Rect::new(0.0, 0.0, 92.0, 24.0))));
        let (menu, _) = *h.popups(bar).last().unwrap();
        let img = h.render_popup(menu, 2.0);
        let (w, hh) = img.pixel_size();
        aizome_kit::testing::write_png(
            &shots().join(format!("menu-toolchest-{suffix}.png")),
            w,
            hh,
            &img.to_rgba8().unwrap(),
        )
        .unwrap();
        // The bar's Edit menu.
        h.send(Msg::Bar(bar, BarMsg::Press(Slot::Menu(2), Rect::new(190.0, 0.0, 230.0, 24.0))));
        let (menu, _) = *h.popups(bar).last().unwrap();
        let img = h.render_popup(menu, 2.0);
        let (w, hh) = img.pixel_size();
        aizome_kit::testing::write_png(
            &shots().join(format!("menu-edit-{suffix}.png")),
            w,
            hh,
            &img.to_rgba8().unwrap(),
        )
        .unwrap();
        // A torn-off Format palette.
        h.send(Msg::TearOff {
            app: "org.aizome.Sumi".into(),
            path: vec!["Format".into()],
            output: "eDP-1".into(),
            at: Point::new(104.0, 412.0),
        });
        let p = window_titled(&h, "Palette (torn off)");
        let name = format!("palette-format-{suffix}.png");
        h.save_png(p, shots().join(&name), 2.0);
        aizome_kit::testing::assert_golden(&mut h, p, golden(&name), 2.0);
    }
}

/// The bar's titles, as laid out, and the one lit.
fn titles(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, bar: WindowKey) -> Vec<(Slot, Rect)> {
    let theme = h.theme().clone();
    h.program_mut().title_rects(bar, &theme)
}

fn lit(h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>) -> Option<Slot> {
    let screen = h.program().screens[0].clone();
    h.program().bar_model(&screen).open
}

fn center_of(titles: &[(Slot, Rect)], slot: Slot) -> Point {
    titles.iter().find(|(s, _)| *s == slot).expect("the title").1.center()
}

#[test]
fn a_title_stays_lit_while_its_menu_is_open_and_the_pointer_slides_along() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let t = titles(&mut h, bar);
    // The geometry is the bar's: the titles are where the bar draws them.
    let format = t.iter().find(|(s, _)| *s == Slot::Menu(4)).unwrap().1;
    assert_eq!(h.find(bar, "Format").map(|r| (r.x0, r.x1)), Some((format.x0, format.x1)));

    h.click(bar, center_of(&t, Slot::Menu(4)));
    assert_eq!(h.popups(bar).len(), 1);
    assert_eq!(lit(&h), Some(Slot::Menu(4)), "Format is lit");
    // Escape closes it; the title goes back.
    h.named(bar, NamedKey::Escape, Modifiers::NONE);
    assert!(h.popups(bar).is_empty());
    assert_eq!(lit(&h), None);

    // With a menu open (a moment later: a press right as a menu closes
    // is taken for the press that closed it), the pointer onto another
    // title opens that one's.
    std::thread::sleep(Duration::from_millis(60));
    h.click(bar, center_of(&t, Slot::Menu(4)));
    h.move_to(bar, center_of(&t, Slot::Menu(2)));
    assert_eq!(lit(&h), Some(Slot::Menu(2)), "Edit's menu now");
    let (_, at) = h.popups(bar)[0];
    assert_eq!(at.x0, t.iter().find(|(s, _)| *s == Slot::Menu(2)).unwrap().1.x0);
    // Pressing its title again closes it, and it stays closed.
    h.click(bar, center_of(&t, Slot::Menu(2)));
    assert!(h.popups(bar).is_empty());
    assert_eq!(lit(&h), None);
    // Without a menu open, the pointer passing over titles opens nothing.
    h.move_to(bar, center_of(&t, Slot::Menu(5)));
    assert!(h.popups(bar).is_empty());
}

#[test]
fn f10_gives_the_bar_the_keyboard_and_the_arrows_go_across_it() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let binding = |a: &str| Msg::Ipc(IpcEvent::Event(Event::Binding { action: a.into(), pointer: None, output: None }));
    h.send(binding("menu-bar"));
    assert_eq!(lit(&h), Some(Slot::Menu(0)), "the app's menu, under its name");
    // → on a row without a submenu: File; ← twice: back past Sumi to the
    // Toolchest; once more round to Help at the end.
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(lit(&h), Some(Slot::Menu(1)));
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(lit(&h), Some(Slot::Toolchest));
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(lit(&h), Some(Slot::Menu(6)));
    assert_eq!(h.popups(bar).len(), 1, "one menu at a time");
    // Its first row is lit already: Return chooses it, and the menu goes.
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert!(h.popups(bar).is_empty());
    assert_eq!(lit(&h), None);
    // A notification's line gives way to the menus.
    h.send(Msg::Note(aizome_shell::notifications::server::NoteEvent::Notify(
        70,
        Box::new(aizome_shell::notifications::store::Incoming {
            app_name: "Mail".into(),
            summary: "Lunch?".into(),
            ..Default::default()
        }),
    )));
    assert!(h.program().banner.is_some());
    h.send(binding("menu-bar"));
    assert!(h.program().banner.is_none());
    assert_eq!(lit(&h), Some(Slot::Menu(0)));
}

#[test]
fn typing_in_the_toolchest_turns_it_into_the_palette() {
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let t = titles(&mut h, bar);
    h.click(bar, center_of(&t, Slot::Toolchest));
    assert_eq!(lit(&h), Some(Slot::Toolchest));
    h.type_text(bar, "w");
    let p = window_titled(&h, "Palette");
    assert_eq!(h.program().palette.as_ref().map(|p| p.query.as_str()), Some("w"));
    let _ = p;
    // Keys typed before the palette shows (the Toolchest still has them)
    // follow the first, rather than reaching the window underneath.
    h.type_text(bar, "in");
    assert_eq!(h.program().palette.as_ref().map(|p| p.query.as_str()), Some("win"));
    // Once the palette has had time to show, the Toolchest has made way
    // (the compositor closes it as soon as the palette maps).
    h.advance(std::time::Duration::from_millis(300));
    assert!(h.popups(bar).is_empty(), "the Toolchest made way");
}

/// Tear off the submenu open over the bar by its strip's pin.
fn tear_open_submenu(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, bar: WindowKey) {
    let popups = h.popups(bar);
    assert_eq!(popups.len(), 2, "a submenu is open");
    let (sub, r) = popups[1];
    h.click_popup(sub, Point::new(r.width() - 3.0 - 12.0, 12.0));
    assert!(h.popups(bar).is_empty(), "the menus closed");
}

#[test]
fn submenus_tear_off_into_palettes_of_their_own() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    let mut h = scene(false);
    let bar = window_titled(&h, "Menu bar");
    let down = |h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, n: usize| {
        for _ in 0..n {
            h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
        }
        h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    };
    // The Toolchest's Places ▸ (Applications ▸ is empty here, so ↓ skips it).
    let t = titles(&mut h, bar);
    h.click(bar, center_of(&t, Slot::Toolchest));
    down(&mut h, 1);
    tear_open_submenu(&mut h, bar);
    let torn = h.program().tearoffs[0].clone();
    assert_eq!((torn.app.as_str(), torn.path.clone()), ("toolchest", vec!["Places".to_owned()]));
    assert!(torn.x > 100.0, "beside the Toolchest, where Places opened: {torn:?}");
    let (title, menu) = h.program().tear_menu(&torn).expect("its menu");
    assert_eq!(title, "Places");
    assert!(menu.items.iter().any(|e| e.label == "Home"));

    // The menu at the pointer's Format ▸, and its Info ▸ (the app menu,
    // remembered by its own title).
    pointer_menu(&mut h, 600.0, 300.0);
    down(&mut h, 5);
    tear_open_submenu(&mut h, bar);
    pointer_menu(&mut h, 600.0, 300.0);
    down(&mut h, 1);
    tear_open_submenu(&mut h, bar);
    let paths: Vec<Vec<String>> = h.program().tearoffs.iter().map(|t| t.path.clone()).collect();
    assert_eq!(paths[1..], [vec!["Format".to_owned()], vec!["Editor".to_owned()]]);
    let (title, menu) = h.program().tear_menu(&h.program().tearoffs[1]).expect("Format");
    assert_eq!(title, "Format");
    assert!(menu.items.iter().any(|e| e.label == "Bold"));
    assert!(h.program().tearoffs[1].x > 600.0 && h.program().tearoffs[1].y > 300.0);
    let (title, _) = h.program().tear_menu(&h.program().tearoffs[2]).expect("the app menu");
    assert_eq!(title, "Editor");
    assert_eq!(h.windows().iter().filter(|k| h.title(**k) == "Palette (torn off)").count(), 3);
}
