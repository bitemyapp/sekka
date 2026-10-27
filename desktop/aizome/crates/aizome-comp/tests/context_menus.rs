//! Context menus over an input method. ibus grabs the keyboard while a
//! text field has it, as Telegram's message field nearly always does: a
//! right-click there opens the field's menu all the same, the menu keeps
//! the keys until it closes (once the input method has let go of the
//! keyboard too), and the input method has them back after. The menu never
//! takes the input method's grab's place, and only the client typed into
//! or pointed at, or the shell, opens a menu over that grab.

mod common;

use aizome_ipc::{InputState, Request, Response, Synthetic};
use common::layer::*;
use common::text::*;
use common::*;

const KEY_ESC: u32 = 1;
const KEY_F: u32 = 33;

fn button(c: &Compositor, button: &str, pressed: bool) {
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerButton { button: button.into(), pressed }] });
}

fn motion(c: &Compositor, (x, y): (f64, f64)) {
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x, y }] });
}

fn input(c: &Compositor) -> InputState {
    match c.request(Request::InputState) {
        Response::InputState { state } => state,
        r => panic!("{r:?}"),
    }
}

/// An input method, and the app with its field focused: the input method
/// holds the keyboard.
fn setup(field_in_menu: bool) -> (Compositor, InputMethod, TextApp) {
    setup_with(InputMethod::spawn, field_in_menu)
}

fn setup_with(ime: fn(&Compositor) -> InputMethod, field_in_menu: bool) -> (Compositor, InputMethod, TextApp) {
    let c = Compositor::start(Options::default());
    let ime = ime(&c);
    let app = TextApp::spawn(&c, field_in_menu);
    c.wait_window(APP_ID);
    app.wait("the field has the keyboard", |s| s.focus == Some("window"));
    ime.wait("the input method has the keyboard", |s| s.active && s.grabbed);
    (c, ime, app)
}

/// Right-press on the field, which opens its menu, then let go.
fn right_click(c: &Compositor, app: &TextApp) {
    motion(c, centre(&c.window(APP_ID).unwrap()));
    button(c, "right", true);
    let s = app.wait("the menu shown", |s| s.menu_shown || s.menu_dismissed);
    assert!(!s.menu_dismissed, "the compositor closed the menu as it opened");
    button(c, "right", false);
    app.wait("the menu has the keyboard", |s| s.focus == Some("menu") || s.menu_dismissed);
}

#[test]
fn a_right_click_in_a_text_field_opens_its_menu_over_the_input_method() {
    let (c, ime, app) = setup(false);
    right_click(&c, &app);
    // The field lost the keyboard, so the input method lets go of it, and
    // the menu's grab takes it.
    ime.wait("the input method let go", |s| !s.active && !s.grabbed);
    c.settle();
    assert!(!app.seen().menu_dismissed, "the menu closed:\n{}", c.log_text());
    assert_eq!(app.seen().focus, Some("menu"));
    assert!(input(&c).keyboard_grab, "the menu holds the keyboard no longer");
    tap(&c, "escape");
    app.wait("Escape in the menu", |s| s.keys_to("menu") == [KEY_ESC]);
    assert!(ime.seen().keys.is_empty(), "the input method got keys: {:?}", ime.seen().keys);
    // Closed, the keys go back to the field, through the input method.
    app.send(FieldCmd::CloseMenu);
    app.wait("the field has the keyboard again", |s| s.focus == Some("window"));
    ime.wait("the input method has the keyboard again", |s| s.active && s.grabbed);
    tap(&c, "f");
    ime.wait("f to the input method", |s| s.keys == [KEY_F]);
    assert!(!app.seen().menu_dismissed);
}

#[test]
fn a_menu_keeps_the_keys_when_the_window_is_focused_again() {
    let (c, ime, app) = setup(false);
    right_click(&c, &app);
    ime.wait("the input method let go", |s| !s.grabbed);
    // Focusing its window (the Deskbar, the switcher) looks again at where
    // the keys go: still to its menu.
    let id = c.window(APP_ID).unwrap().id;
    c.ok(Request::Focus { id });
    c.settle();
    assert!(!app.seen().menu_dismissed, "the menu closed");
    assert_eq!(app.seen().focus, Some("menu"));
    tap(&c, "escape");
    app.wait("Escape in the menu", |s| s.keys_to("menu") == [KEY_ESC]);
    assert!(app.seen().keys_to("window").is_empty());
}

#[test]
fn a_field_in_a_menu_types_through_the_input_method() {
    let (c, ime, app) = setup(true);
    right_click(&c, &app);
    // The input method moves from the window's field to the menu's, and
    // holds the keyboard for it.
    ime.wait("the input method in the menu's field", |s| s.activations == 2 && s.active && s.grabbed);
    // Looking again at where the keys go leaves them in the menu, though
    // the keyboard's grab is the input method's.
    let id = c.window(APP_ID).unwrap().id;
    c.ok(Request::Focus { id });
    c.settle();
    assert!(!app.seen().menu_dismissed, "the menu closed");
    assert_eq!(app.seen().focus, Some("menu"));
    tap(&c, "f");
    ime.wait("f to the input method", |s| s.keys == [KEY_F]);
    // Closed, the window's field has the keyboard and the input method.
    app.send(FieldCmd::CloseMenu);
    app.wait("the field has the keyboard again", |s| s.focus == Some("window"));
    ime.wait("the input method back in the window's field", |s| s.activations == 3 && s.grabbed);
}

#[test]
fn an_input_method_keeping_its_grab_has_the_keys_after_a_menu() {
    // One that keeps the keyboard while no field is active: the menu opens
    // over its grab, never in its place.
    let (c, ime, app) = setup_with(InputMethod::spawn_holding, false);
    right_click(&c, &app);
    ime.wait("the input method deactivated", |s| !s.active);
    c.settle();
    assert!(!app.seen().menu_dismissed, "the menu closed:\n{}", c.log_text());
    assert_eq!(app.seen().focus, Some("menu"));
    // Closed, the field's keys still go through the input method's grab.
    app.send(FieldCmd::CloseMenu);
    app.wait("the field has the keyboard again", |s| s.focus == Some("window"));
    ime.wait("the input method active again", |s| s.active && s.activations == 2);
    tap(&c, "f");
    ime.wait("f to the input method", |s| s.keys == [KEY_F]);
    assert!(app.seen().keys.is_empty(), "the window got keys: {:?}", app.seen().keys);
}

#[test]
fn only_the_client_typed_into_opens_a_menu_over_the_input_method() {
    let c = Compositor::start(Options::default());
    let ime = InputMethod::spawn(&c);
    let other = TextApp::spawn_as(&c, "org.example.other", false);
    c.wait_window("org.example.other");
    other.wait("the other window has the keyboard", |s| s.focus == Some("window"));
    let app = TextApp::spawn(&c, false);
    c.wait_window(APP_ID);
    app.wait("the field has the keyboard", |s| s.focus == Some("window"));
    ime.wait("the input method has the keyboard", |s| s.active && s.grabbed);
    motion(&c, centre(&c.window(APP_ID).unwrap()));
    c.settle();
    // A menu of a window neither typed into nor pointed at, with a serial
    // from when it had the keyboard: refused, the field keeps the keys.
    other.send(FieldCmd::OpenMenu);
    let s = other.wait("the other's menu", |s| s.menu_shown || s.menu_dismissed);
    assert!(s.menu_dismissed, "the other window's menu opened over the field");
    c.settle();
    assert_eq!(app.seen().focus, Some("window"));
    let s = ime.seen();
    assert!(s.active && s.grabbed, "{s:?}");
    tap(&c, "f");
    ime.wait("f to the input method", |s| s.keys == [KEY_F]);
}

#[test]
fn the_shells_menus_open_over_the_input_method() {
    let (c, ime, app) = setup(false);
    let shell = ShellClient::spawn(&c);
    shell.layer(
        LayerSpec::new("bar", Layer::Top, KeyboardInteractivity::OnDemand)
            .anchor(Anchor::Top | Anchor::Left | Anchor::Right, (0, 30)),
    );
    // As for F10, the keys in the field: a grab with serial 0.
    shell.send(ShellCmd::Popup {
        name: "menu",
        parent: "bar",
        at: (10, 30),
        size: (200, 150),
        color: 0xcc3333,
        grab: true,
    });
    let s = shell.wait("the menu has the keyboard", |s| s.focus.as_deref() == Some("menu") || !s.dismissed.is_empty());
    assert!(s.dismissed.is_empty(), "the menu closed:\n{}", c.log_text());
    ime.wait("the input method let go", |s| !s.active && !s.grabbed);
    c.settle();
    assert!(input(&c).keyboard_grab, "the menu holds the keyboard no longer");
    tap(&c, "escape");
    shell.wait("Escape in the menu", |s| s.keys_to("menu") == [KEY_ESC]);
    // Closed, the keys go back to the field, through the input method.
    shell.send(ShellCmd::Destroy("menu"));
    app.wait("the field has the keyboard again", |s| s.focus == Some("window"));
    ime.wait("the input method has the keyboard again", |s| s.active && s.grabbed);
    tap(&c, "f");
    ime.wait("f to the input method", |s| s.keys == [KEY_F]);
}

#[test]
fn a_menu_refused_for_the_pointer_leaves_the_keyboard_be() {
    let (c, ime, app) = setup(false);
    // Moving the window by its tab holds the pointer.
    let w = c.window(APP_ID).unwrap();
    let tab = ((w.geometry.x + 24) as f64, (w.geometry.y - 11) as f64);
    motion(&c, tab);
    button(&c, "left", true);
    motion(&c, (tab.0 + 40.0, tab.1 + 30.0));
    let st = input(&c);
    assert!(st.pointer_grab.as_deref().is_some_and(|g| g != "popup"), "{st:?}");
    // The field's menu for a key meanwhile: the keyboard check lets it
    // over the input method, the pointer's refuses it.
    app.send(FieldCmd::OpenMenu);
    let s = app.wait("the menu", |s| s.menu_shown || s.menu_dismissed);
    assert!(s.menu_dismissed, "the menu opened over a move");
    c.settle();
    button(&c, "left", false);
    // The field kept the keyboard all along, and the input method its grab.
    assert_eq!(app.seen().focus, Some("window"));
    let s = ime.seen();
    assert!(s.active && s.grabbed && s.activations == 1, "{s:?}");
    tap(&c, "f");
    ime.wait("f to the input method", |s| s.keys == [KEY_F]);
}
