//! The keyboard for the shell's surfaces and menus, and the popup layer:
//! an exclusive layer surface takes the keys while it shows, an on-demand
//! one when clicked, a grabbing popup (a menu) until it closes, after which
//! they go back where they were; and menus show above every window and
//! layer surface and take the pointer there. Keys and clicks go through
//! `Request::Synthesize`, the same paths as libinput's.

mod common;

use aizome_ipc::{Request, Synthetic};
use common::layer::*;
use common::*;

const KEY_ESC: u32 = 1;
const KEY_F: u32 = 33;
const KEY_O: u32 = 24;

fn motion(c: &Compositor, x: f64, y: f64) {
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x, y }] });
}

fn click(c: &Compositor, x: f64, y: f64) {
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::Click { button: "left".into() }],
    });
}

fn stationary_click(c: &Compositor) {
    // No PointerMotion: a freshly opened/reanchored layer can be under a
    // stationary pointer, as the Deskbar is after releasing its grip.
    c.ok(Request::Synthesize { events: vec![Synthetic::Click { button: "left".into() }] });
}

fn reopened_deskbar_receives_stationary_click(fallback: bool) {
    let c = Compositor::start(Options {
        logic: fallback.then(|| std::path::Path::new("/does-not-exist/aizome-logic.so")),
        ..Options::default()
    });
    let shell = ShellClient::spawn(&c);
    for (name, replacement, from, to, start, end) in [
        ("right-old", "left-new", Anchor::Right, Anchor::Left, 1163.0, 115.0),
        ("left-old", "right-new", Anchor::Left, Anchor::Right, 115.0, 1163.0),
    ] {
        let layer = |name, edge| {
            LayerSpec::new(name, Layer::Top, KeyboardInteractivity::None)
                .anchor(Anchor::Top | edge, (232, 100))
                .top_margin(24)
                .zone(-1)
        };
        shell.layer(layer(name, from));
        c.settle();
        motion(&c, start, 29.0);
        c.ok(Request::Synthesize {
            events: vec![
                Synthetic::PointerButton { button: "left".into(), pressed: true },
                Synthetic::PointerMotion { x: end, y: 29.0 },
                Synthetic::PointerButton { button: "left".into(), pressed: false },
            ],
        });
        shell
            .wait("grip drag has a press and release", |s| s.buttons.iter().filter(|(n, _, _)| n == name).count() == 2);
        // sync_deskbar destroys/recreates its layer when the edge changes.
        shell.send(ShellCmd::Destroy(name));
        shell.layer(layer(replacement, to));
        c.settle();
        stationary_click(&c);
        shell.wait("stationary click delivered to replacement layer", |s| {
            s.buttons.iter().filter(|(n, _, _)| n == replacement).count() == 2
        });
        assert_eq!(
            shell
                .seen()
                .buttons
                .iter()
                .filter(|(n, _, _)| n == replacement)
                .map(|(_, b, p)| (*b, *p))
                .collect::<Vec<_>>(),
            vec![(0x110, true), (0x110, false)]
        );
        assert_eq!(shell.seen().buttons.iter().filter(|(n, _, _)| n == name).count(), 2);
        shell.send(ShellCmd::Destroy(replacement));
        c.settle();
    }
}

#[test]
fn reopened_deskbar_takes_a_stationary_click_on_either_edge() {
    reopened_deskbar_receives_stationary_click(false);
}

#[test]
fn fallback_reopened_deskbar_takes_a_stationary_click_on_either_edge() {
    reopened_deskbar_receives_stationary_click(true);
}

#[test]
fn a_layer_appearing_under_a_stationary_pointer_does_not_steal_a_held_button() {
    let (c, a, shell) = setup();
    let (x, y) = centre(&c.window("org.example.editor").unwrap());
    motion(&c, x, y);
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerButton { button: "left".into(), pressed: true }] });
    a.wait_seen("window got initial press", |s| s.buttons == [(0x110, true)]);
    shell.layer(
        LayerSpec::new("cover", Layer::Overlay, KeyboardInteractivity::None)
            .anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right, (0, 0)),
    );
    c.settle();
    // A second press and both releases still belong to the implicit grab.
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerButton { button: "right".into(), pressed: true },
            Synthetic::PointerButton { button: "right".into(), pressed: false },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
        ],
    });
    a.wait_seen("grab retained all buttons", |s| {
        s.buttons == [(0x110, true), (0x111, true), (0x111, false), (0x110, false)]
    });
    assert!(shell.seen().buttons.is_empty(), "new layer stole a held grab");
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

/// A menu's item that starts something typed in the bar (Rename in a
/// Desk's menu, #231): the keys go to the window as the menu closes, so
/// the bar asks for them outright, and has them until it's on demand
/// again. `together`: the compositor hears of the menu's closing and of the
/// asking in one go, as a rule; else apart, the window having the keys in
/// between.
fn a_bar_that_turns_exclusive_as_its_menu_closes_has_the_keys(together: bool) {
    let (c, a, shell) = setup();
    shell.layer(bar());
    c.settle();
    click(&c, 300.0, 10.0);
    shell.wait("the bar has the keyboard", |s| s.focus.as_deref() == Some("bar"));
    shell.send(menu("menu", "bar", true));
    shell.wait("the menu has the keyboard", |s| s.focus.as_deref() == Some("menu"));
    // An item chosen: the menu closes, then the bar asks.
    shell.send(ShellCmd::Destroy("menu"));
    if !together {
        a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    }
    shell.send(ShellCmd::Keyboard { name: "bar", keyboard: KeyboardInteractivity::Exclusive });
    shell.wait("the bar has the keyboard", |s| s.focus.as_deref() == Some("bar"));
    a.wait_seen("the window lost it", |s| !s.keyboard_focus);
    tap(&c, "f");
    shell.wait("f in the bar", |s| s.keys_to("bar") == [KEY_F]);
    // It asked outright: a click in the window doesn't take them back.
    let (x, y) = centre(&c.window("org.example.editor").unwrap());
    click(&c, x, y);
    tap(&c, "o");
    shell.wait("o in the bar", |s| s.keys_to("bar") == [KEY_F, KEY_O]);
    assert!(a.seen().keys.is_empty(), "the window got keys: {:?}", a.seen().keys);
    // On demand again, it lets go: the click that opened the menu doesn't
    // keep the keys for it.
    shell.send(ShellCmd::Keyboard { name: "bar", keyboard: KeyboardInteractivity::OnDemand });
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    shell.wait("the bar let go", |s| s.focus.is_none());
    tap(&c, "f");
    a.wait_seen("f in the window", |s| s.keys.iter().any(|(k, _)| *k == KEY_F));
    assert_eq!(shell.seen().keys_to("bar"), [KEY_F, KEY_O]);
}

#[test]
fn a_bar_that_turns_exclusive_as_its_menu_closes_has_the_keys_until_on_demand_again() {
    a_bar_that_turns_exclusive_as_its_menu_closes_has_the_keys(true);
}

#[test]
fn a_bar_that_turns_exclusive_after_its_menu_closed_has_the_keys_until_on_demand_again() {
    a_bar_that_turns_exclusive_as_its_menu_closes_has_the_keys(false);
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

/// The real shell under test, started in the compositor's private session
/// and writing what its windows show for tests from outside
/// (`AIZOME_KIT_INSPECT`), once it has its bars up.
fn real_shell(c: &Compositor) -> common::desktop::Program {
    use common::desktop::{Program, command, shell_state, wait};
    let shell = std::env::var("AIZOME_LAYOUT_TEST_SHELL").expect("shell binary");
    let inspect = c.dir.join("inspect");
    std::fs::create_dir(&inspect).unwrap();
    let program = Program(
        command(c, &shell)
            .env("AIZOME_KIT_INSPECT", &inspect)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(c.dir.join("shell.log")).unwrap())
            .spawn()
            .unwrap(),
    );
    wait("shell ready", || {
        shell_state(c, &shell).is_some_and(|s| s["screens"].as_array().is_some_and(|a| !a.is_empty()))
    });
    program
}

/// The real bar `width` points wide (each display has one), as the kit
/// last wrote it.
fn real_bar(c: &Compositor, width: f64) -> Option<serde_json::Value> {
    std::fs::read_dir(c.dir.join("inspect")).ok()?.flatten().find_map(|entry| {
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
        (doc["title"] == "Menu bar" && doc["size"][0] == width).then_some(doc)
    })
}

/// The middle of the Desk that bar shows as `text`, in the bar, once it
/// does.
fn real_desk(c: &Compositor, width: f64, text: &str) -> (f64, f64) {
    let mut at = None;
    common::desktop::wait(&format!("{text} in the bar"), || {
        at = real_bar(c, width).and_then(|bar| {
            let desk = bar["elements"].as_array()?.iter().find(|e| e["visible"] == true && e["text"] == text)?;
            let r = desk["shown"].as_array()?;
            Some((r[0].as_f64()? + r[2].as_f64()? / 2.0, r[1].as_f64()? + r[3].as_f64()? / 2.0))
        });
        at.is_some()
    });
    at.unwrap()
}

/// The field a Desk is renamed in, in that bar: whether it has the focus
/// in the bar, and the bar the keyboard. None while the bar shows no field.
fn real_rename_field(c: &Compositor, width: f64) -> Option<(bool, bool)> {
    let bar = real_bar(c, width)?;
    let field = bar["elements"]
        .as_array()?
        .iter()
        .find(|e| e["name"].as_str().is_some_and(|n| n.starts_with("desk-rename")))?;
    Some((field["focused"] == true, bar["has_keyboard"] == true))
}

fn input_state(c: &Compositor) -> aizome_ipc::InputState {
    match c.request(Request::InputState) {
        aizome_ipc::Response::InputState { state } => state,
        other => panic!("{other:?}"),
    }
}

fn desk_names(c: &Compositor) -> Vec<String> {
    match c.request(Request::Desks) {
        aizome_ipc::Response::Desks { desks, .. } => desks.into_iter().map(|d| d.name).collect(),
        other => panic!("{other:?}"),
    }
}

/// Waits until the real bar `width` wide has a Desk's field with the focus
/// in it, and the keyboard: by the shell's account and by the compositor's
/// (a surface that's no window's has it, with no menu open).
fn wait_for_the_real_rename(c: &Compositor, width: f64) {
    use common::desktop::wait;
    wait("the field, with the focus in the bar", || real_rename_field(c, width).is_some_and(|(focused, _)| focused));
    wait("the bar has the keyboard", || {
        let i = input_state(c);
        i.pointer_grab.is_none()
            && i.keyboard_focus.as_deref() == Some("a surface")
            && real_rename_field(c, width) == Some((true, true))
    });
}

/// Right-clicks the point (a Desk in a real bar) and picks the first item
/// of its menu, Rename, from the keyboard.
fn rename_from_the_real_menu(c: &Compositor, (x, y): (f64, f64)) {
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::Click { button: "right".into() }],
    });
    common::desktop::wait("the Desk's menu", || input_state(c).pointer_grab.as_deref() == Some("popup"));
    c.settle();
    tap(c, "down");
    tap(c, "return");
}

/// The real shell's bar (#231). Rename in a Desk's menu: what's typed next
/// goes to the Desk's name with no click on it, though the menu had the
/// keyboard and the compositor gives it to the window as a menu closes;
/// and once renamed, the window has the keys again. A double click on the
/// Desk, which gives the bar the keys itself, renames as before, and a
/// click in a window then gives the rename up.
#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL pointing at the shell under test"]
fn the_real_bar_renames_a_desk_from_its_menu_without_another_click() {
    use aizome_ipc::DeskSel;
    use common::desktop::wait;
    const BAR: f64 = 1280.0;
    let c = Compositor::start(Options::default());
    c.ok(Request::RenameDesk { desk: DeskSel::Index(1), name: "Mail".into() });
    c.ok(Request::RenameDesk { desk: DeskSel::Index(2), name: "Build".into() });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let _shell = real_shell(&c);

    rename_from_the_real_menu(&c, real_desk(&c, BAR, "2 Build"));
    wait_for_the_real_rename(&c, BAR);
    for key in ["backspace", "backspace", "backspace", "backspace", "backspace", "s", "h", "i", "p", "return"] {
        tap(&c, key);
    }
    wait("the Desk renamed", || desk_names(&c).get(1).is_some_and(|n| n == "ship"));
    assert!(a.seen().keys.is_empty(), "the window got keys: {:?}", a.seen().keys);
    // Over, the bar lets go of the keyboard.
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    tap(&c, "f");
    a.wait_seen("f in the window", |s| s.keys.iter().any(|(k, _)| *k == KEY_F));
    wait("the field gone", || real_rename_field(&c, BAR).is_none());

    // A double click on the Desk, as before.
    let double_click = |(x, y): (f64, f64)| {
        c.ok(Request::Synthesize {
            events: vec![
                Synthetic::PointerMotion { x, y },
                Synthetic::Click { button: "left".into() },
                Synthetic::Click { button: "left".into() },
            ],
        });
    };
    double_click(real_desk(&c, BAR, "1 Mail"));
    wait_for_the_real_rename(&c, BAR);
    tap(&c, "x");
    tap(&c, "return");
    wait("the Desk renamed", || desk_names(&c).first().is_some_and(|n| n == "Mailx"));
    // And a click in the window gives a rename up: the keys are the
    // window's, and the Desk keeps its name.
    double_click(real_desk(&c, BAR, "1 Mailx"));
    wait_for_the_real_rename(&c, BAR);
    tap(&c, "y");
    let (x, y) = centre(&c.window("org.example.editor").unwrap());
    click(&c, x, y);
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    wait("the field gone", || real_rename_field(&c, BAR).is_none());
    tap(&c, "o");
    a.wait_seen("o in the window", |s| s.keys.iter().any(|(k, _)| *k == KEY_O));
    assert_eq!(desk_names(&c)[..2], ["Mailx", "ship"]);
}

/// The same from the bar of a display the focused window isn't on: that
/// bar has the keyboard and the focused field, not the other display's
/// (which shows the Desk's field too).
#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL pointing at the shell under test"]
fn the_real_bar_of_another_display_renames_a_desk_from_its_menu() {
    use aizome_ipc::DeskSel;
    use common::desktop::wait;
    const FIRST: f64 = 1280.0;
    const SECOND: f64 = 960.0;
    let c = Compositor::start(Options { outputs: &["1280x800@1", "960x720@1"], ..Default::default() });
    c.ok(Request::RenameDesk { desk: DeskSel::Index(2), name: "Build".into() });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    assert_eq!(w.output.as_deref(), Some("HEADLESS-1"));
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let _shell = real_shell(&c);
    let second = match c.request(Request::Outputs) {
        aizome_ipc::Response::Outputs { outputs } => outputs.into_iter().find(|o| o.name == "HEADLESS-2").unwrap(),
        other => panic!("{other:?}"),
    };

    let (x, y) = real_desk(&c, SECOND, "2 Build");
    rename_from_the_real_menu(&c, (f64::from(second.logical.x) + x, f64::from(second.logical.y) + y));
    wait_for_the_real_rename(&c, SECOND);
    wait("the other display's bar shows the field, without the focus or the keyboard", || {
        real_rename_field(&c, FIRST) == Some((false, false))
    });
    for key in ["backspace", "backspace", "backspace", "backspace", "backspace", "s", "h", "i", "p", "return"] {
        tap(&c, key);
    }
    wait("the Desk renamed", || desk_names(&c).get(1).is_some_and(|n| n == "ship"));
    assert!(a.seen().keys.is_empty(), "the window got keys: {:?}", a.seen().keys);
    a.wait_seen("the window has the keyboard again", |s| s.keyboard_focus);
    wait("the fields gone", || real_rename_field(&c, FIRST).is_none() && real_rename_field(&c, SECOND).is_none());
}
