//! The keyboard for the shell's surfaces and menus, and the popup layer:
//! an exclusive layer surface takes the keys while it shows, an on-demand
//! one when clicked, a grabbing popup (a menu) until it closes, after which
//! they go back where they were; and menus show above every window and
//! layer surface and take the pointer there. Keys and clicks go through
//! `Request::Synthesize`, the same paths as libinput's.

mod common;

use aizome_ipc::{Request, Synthetic, WindowInfo};
use common::layer::*;
use common::*;

const KEY_ESC: u32 = 1;
const KEY_F: u32 = 33;
const KEY_O: u32 = 24;

fn tap(c: &Compositor, key: &str) {
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::Key { key: key.into(), pressed: true },
            Synthetic::Key { key: key.into(), pressed: false },
        ],
    });
}

fn motion(c: &Compositor, x: f64, y: f64) {
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x, y }] });
}

fn click(c: &Compositor, x: f64, y: f64) {
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::Click { button: "left".into() }],
    });
}

fn centre(w: &WindowInfo) -> (f64, f64) {
    ((w.geometry.x + w.geometry.w / 2) as f64, (w.geometry.y + w.geometry.h / 2) as f64)
}

/// A compositor with a focused window, and a shell client.
fn setup() -> (Compositor, TestClient, ShellClient) {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let shell = ShellClient::spawn(&c);
    (c, a, shell)
}

fn bar() -> LayerSpec {
    LayerSpec::new("bar", Layer::Top, KeyboardInteractivity::OnDemand)
        .anchor(Anchor::Top | Anchor::Left | Anchor::Right, (0, 30))
        .color(0x999999)
}

fn menu(name: &'static str, parent: &'static str, grab: bool) -> ShellCmd {
    ShellCmd::Popup { name, parent, at: (10, 30), size: (200, 150), color: 0xcc3333, grab }
}

#[test]
fn an_exclusive_layer_surface_takes_the_keys_while_it_shows() {
    let (c, a, shell) = setup();
    shell.layer(LayerSpec::new("palette", Layer::Overlay, KeyboardInteractivity::Exclusive));
    shell.wait("the palette has the keyboard", |s| s.focus.as_deref() == Some("palette"));
    a.wait_seen("the window lost it", |s| !s.keyboard_focus);
    tap(&c, "f");
    shell.wait("f in the palette", |s| s.keys_to("palette") == [KEY_F]);
    // Focusing a window doesn't take the keys from it.
    let id = c.window("org.example.editor").unwrap().id;
    c.ok(Request::Focus { id });
    tap(&c, "o");
    shell.wait("o in the palette", |s| s.keys_to("palette") == [KEY_F, KEY_O]);
    assert!(a.seen().keys.is_empty(), "the window got keys: {:?}", a.seen().keys);
    // Gone, it gives them back.
    shell.send(ShellCmd::Destroy("palette"));
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    tap(&c, "f");
    a.wait_seen("f in the window", |s| s.keys.iter().any(|(k, _)| *k == KEY_F));
}

#[test]
fn an_on_demand_layer_surface_takes_the_keys_when_clicked() {
    let (c, a, shell) = setup();
    shell.layer(bar());
    c.settle();
    assert_eq!(shell.seen().focus, None, "an on-demand surface waits for a click");
    assert!(a.seen().keyboard_focus);
    click(&c, 300.0, 10.0);
    shell.wait("the bar has the keyboard", |s| s.focus.as_deref() == Some("bar"));
    tap(&c, "f");
    shell.wait("f in the bar", |s| s.keys_to("bar") == [KEY_F]);
    // A click in the window takes them back.
    let (x, y) = centre(&c.window("org.example.editor").unwrap());
    click(&c, x, y);
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    shell.wait("the bar lost it", |s| s.focus.is_none());
}

#[test]
fn a_grabbing_menu_of_a_layer_surface_gets_the_keys_and_gives_them_back() {
    let (c, a, shell) = setup();
    shell.layer(bar());
    // As after a shortcut that reached the shell over IPC: a grab with no
    // input serial of the shell's own.
    shell.send(menu("menu", "bar", true));
    shell.wait("the menu has the keyboard", |s| s.focus.as_deref() == Some("menu"));
    tap(&c, "escape");
    shell.wait("Escape in the menu", |s| s.keys_to("menu") == [KEY_ESC]);
    assert!(a.seen().keys.is_empty(), "the window got keys: {:?}", a.seen().keys);
    // Closed, the keys go back to the window, not to the bar.
    shell.send(ShellCmd::Destroy("menu"));
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    shell.wait("the shell lost it", |s| s.focus.is_none());
    tap(&c, "f");
    a.wait_seen("f in the window", |s| s.keys.iter().any(|(k, _)| *k == KEY_F));
}

#[test]
fn a_menu_opened_by_clicking_the_bar_gives_the_keys_to_the_window_after() {
    let (c, a, shell) = setup();
    shell.layer(bar());
    c.settle();
    // A click on the bar (a systray widget) that opens its menu.
    click(&c, 300.0, 10.0);
    shell.wait("the bar has the keyboard", |s| s.focus.as_deref() == Some("bar"));
    shell.send(menu("panel", "bar", true));
    shell.wait("the panel has the keyboard", |s| s.focus.as_deref() == Some("panel"));
    // Escape closed it: the keys go to the window, not back to the bar.
    shell.send(ShellCmd::Destroy("panel"));
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    shell.wait("the bar let go", |s| s.focus.is_none());
    tap(&c, "f");
    a.wait_seen("f in the window", |s| s.keys.iter().any(|(k, _)| *k == KEY_F));
    assert!(shell.seen().keys_to("bar").is_empty(), "the bar got keys: {:?}", shell.seen().keys_to("bar"));
}

#[test]
fn a_window_that_opens_takes_the_keys_from_a_clicked_layer_surface() {
    let (c, a, shell) = setup();
    shell.layer(bar());
    c.settle();
    click(&c, 300.0, 10.0);
    shell.wait("the bar has the keyboard", |s| s.focus.as_deref() == Some("bar"));
    // A window opening (launched from the bar, say) takes the focus and
    // the keys with it.
    let b = c.client(Spec::new("org.example.viewer", 0x22cc22));
    c.wait_window("org.example.viewer");
    b.wait_seen("the new window has the keyboard", |s| s.keyboard_focus);
    shell.wait("the bar let go", |s| s.focus.is_none());
    tap(&c, "o");
    b.wait_seen("o in the new window", |s| s.keys.iter().any(|(k, _)| *k == KEY_O));
    // Focusing a window by asking (a Deskbar row, the switcher) does too.
    click(&c, 300.0, 10.0);
    shell.wait("the bar has the keyboard", |s| s.focus.as_deref() == Some("bar"));
    let id = c.window("org.example.editor").unwrap().id;
    c.ok(Request::Focus { id });
    a.wait_seen("the editor has the keyboard", |s| s.keyboard_focus);
    shell.wait("the bar let go", |s| s.focus.is_none());
}

#[test]
fn an_exclusive_layer_surface_closes_open_menus_and_keeps_new_ones_shut() {
    let (c, _a, shell) = setup();
    shell.layer(bar());
    shell.send(menu("menu", "bar", true));
    shell.wait("the menu has the keyboard", |s| s.focus.as_deref() == Some("menu"));
    shell.layer(LayerSpec::new("palette", Layer::Overlay, KeyboardInteractivity::Exclusive));
    shell.wait("the menu closed", |s| s.dismissed.iter().any(|n| n == "menu"));
    shell.wait("the palette has the keyboard", |s| s.focus.as_deref() == Some("palette"));
    tap(&c, "f");
    shell.wait("f in the palette", |s| s.keys_to("palette") == [KEY_F]);
    // A menu of another surface doesn't open over it.
    shell.send(menu("menu2", "bar", true));
    shell.wait("the new menu closed", |s| s.dismissed.iter().any(|n| n == "menu2"));
    assert_eq!(shell.seen().focus.as_deref(), Some("palette"));
}

#[test]
fn clicking_another_window_closes_menus_and_focuses_it() {
    let (c, a, shell) = setup();
    let b = c.client(Spec::new("org.example.viewer", 0x22cc22));
    c.wait_window("org.example.viewer");
    b.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let ida = c.window("org.example.editor").unwrap().id;
    let idb = c.window("org.example.viewer").unwrap().id;
    c.ok(Request::Move { id: Some(ida), x: 100, y: 300 });
    c.ok(Request::Move { id: Some(idb), x: 700, y: 300 });
    shell.layer(bar());
    shell.send(menu("menu", "bar", true));
    shell.wait("the menu has the keyboard", |s| s.focus.as_deref() == Some("menu"));
    let (x, y) = centre(&c.window("org.example.editor").unwrap());
    click(&c, x, y);
    shell.wait("the menu closed", |s| s.dismissed.iter().any(|n| n == "menu"));
    c.wait_windows("the clicked window focused", |ws| ws.iter().any(|w| w.id == ida && w.focused));
    a.wait_seen("the clicked window has the keyboard", |s| s.keyboard_focus);
}

#[test]
fn menus_show_above_every_window_and_layer_surface() {
    let (c, _a, shell) = setup();
    let id = c.window("org.example.editor").unwrap().id;
    c.ok(Request::Move { id: Some(id), x: 300, y: 300 });
    shell.layer(
        LayerSpec::new("desktop", Layer::Bottom, KeyboardInteractivity::None)
            .anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right, (0, 0))
            .color(0x101010),
    );
    shell.layer(bar());
    // A Deskbar opened after the bar, on the same layer, and an overlay.
    shell.layer(
        LayerSpec::new("deskbar", Layer::Top, KeyboardInteractivity::None)
            .anchor(Anchor::Left, (200, 400))
            .color(0x777777),
    );
    shell.layer(
        LayerSpec::new("overlay", Layer::Overlay, KeyboardInteractivity::None)
            .anchor(Anchor::Top | Anchor::Left, (120, 120))
            .color(0x3333cc),
    );
    // The bar's menu reaches over the Deskbar (x 0–200, y 200–600) and
    // under the overlay (0–120 square); the desktop's goes over the window.
    shell.send(ShellCmd::Popup {
        name: "menu",
        parent: "bar",
        at: (50, 30),
        size: (150, 300),
        color: 0xcc3333,
        grab: false,
    });
    shell.send(ShellCmd::Popup {
        name: "dmenu",
        parent: "desktop",
        at: (350, 350),
        size: (150, 100),
        color: 0x33cc33,
        grab: false,
    });
    shell.wait("both menus drawn", |s| s.shown.iter().any(|n| n == "menu") && s.shown.iter().any(|n| n == "dmenu"));
    c.settle();
    let img = c.screenshot();
    keep(&img, "focus-menus-on-top");
    assert_px(&img, 100, 250, rgb(0xcc3333), "the bar's menu over the Deskbar");
    assert_px(&img, 100, 100, rgb(0xcc3333), "the bar's menu over an overlay");
    assert_px(&img, 20, 250, rgb(0x777777), "the Deskbar beside the menu");
    assert_px(&img, 20, 100, rgb(0x3333cc), "the overlay beside the menu");
    assert_px(&img, 400, 400, rgb(0x33cc33), "the desktop's menu over the window");
    // The pointer finds them there too.
    motion(&c, 100.0, 250.0);
    shell.wait("the pointer on the bar's menu", |s| s.pointer.as_deref() == Some("menu"));
    motion(&c, 400.0, 400.0);
    shell.wait("the pointer on the desktop's menu", |s| s.pointer.as_deref() == Some("dmenu"));
}
