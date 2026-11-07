//! Tiling (`[windows] layout = "tiled"`): windows open filling the Desk
//! beside the Deskbar, keep to it as it changes, and stay put; dialogs,
//! windows that can't grow to fill and windows floated by hand float; and
//! tiling going off gives windows back their free places.

mod common;

use aizome_ipc::{Rect, Request, Response, Synthetic, WindowInfo};
use common::layer::{Anchor, KeyboardInteractivity, Layer, LayerSpec, ShellClient, ShellCmd};
use common::*;

const TILED: &str = "[appearance]\neffects = \"austere\"\n[windows]\nlayout = \"tiled\"\n";
const FLOATING: &str = "[appearance]\neffects = \"austere\"\n";
const BAR: i32 = 24;
const DESKBAR: i32 = 232;

/// The 1280×800 output, filled: inside the frame, below the tab.
fn filled() -> Rect {
    Rect::new(1, 25, 1278, 774)
}

fn tiled() -> Compositor {
    Compositor::start(Options { settings: TILED, ..Default::default() })
}

fn win<'a>(ws: &'a [WindowInfo], app_id: &str) -> &'a WindowInfo {
    ws.iter().find(|w| w.app_id == app_id).unwrap_or_else(|| panic!("no {app_id}: {ws:?}"))
}

fn pointer(c: &Compositor, events: Vec<Synthetic>) {
    c.ok(Request::Synthesize { events });
}

fn button(pressed: bool) -> Synthetic {
    Synthetic::PointerButton { button: "left".into(), pressed }
}

fn motion(x: i32, y: i32) -> Synthetic {
    Synthetic::PointerMotion { x: x as f64, y: y as f64 }
}

fn refused(r: Response) -> bool {
    matches!(r, Response::Error { message } if message.contains("tiled"))
}

#[test]
fn new_windows_open_filled_and_dialogs_float() {
    let c = tiled();
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    assert!(w.fill && w.geometry == filled(), "{w:?}");
    let seen = a.wait_seen("filled", |s| s.maximized && s.size == (1278, 774));
    assert_eq!(seen.configured.first(), Some(&(1278, 774)), "told before it first drew: {:?}", seen.configured);
    // A dialog floats over its window.
    a.send(Cmd::Dialog("Open".into(), 0xcccc22));
    let ws = c.wait_windows("a dialog", |ws| ws.iter().any(|w| w.parent.is_some()));
    let d = ws.iter().find(|w| w.parent.is_some()).unwrap();
    assert!(!d.fill && (d.geometry.w, d.geometry.h) == (200, 150), "{d:?}");
    // Another app's window fills too, on its own (the default stacks only
    // the same app's).
    let _b = c.client(Spec::new("org.example.viewer", 0xcc2222));
    let b = c.wait_window("org.example.viewer");
    assert!(b.fill && b.geometry == filled() && b.group.is_none(), "{b:?}");
}

#[test]
fn windows_that_cant_grow_to_fill_float() {
    let c = tiled();
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    let _calc = c.client(Spec::new("org.example.calculator", 0xcccccc).fixed_size(240, 160));
    let ws = c.wait_windows("the calculator", |ws| ws.iter().any(|w| w.app_id == "org.example.calculator"));
    let calc = win(&ws, "org.example.calculator");
    assert!(calc.float && !calc.fill && (calc.geometry.w, calc.geometry.h) == (240, 160), "{calc:?}");
    // One that says it's of a fixed size only once it has been told a
    // size, as the kit's windows do: it keeps its own.
    let late = c.client(Spec::new("org.example.timer", 0xcc22cc).fixed_size(300, 200).late_limits());
    let ws = c.wait_windows("the timer", |ws| ws.iter().any(|w| w.app_id == "org.example.timer"));
    let timer = win(&ws, "org.example.timer");
    assert!(timer.float && (timer.geometry.w, timer.geometry.h) == (300, 200), "{timer:?}");
    assert_eq!(late.seen().size, (300, 200));
    let _panel = c.client(Spec::new("org.example.panel", 0x22cc22).size(500, 300).max_size(600, 400));
    let ws = c.wait_windows("the panel", |ws| ws.iter().any(|w| w.app_id == "org.example.panel"));
    let panel = win(&ws, "org.example.panel");
    assert!(panel.float && !panel.fill && panel.geometry.w <= 600, "a largest size short of the area: {panel:?}");
    assert!(win(&ws, "org.example.editor").fill, "{ws:?}");
}

#[test]
fn tiled_windows_stay_put() {
    let c = tiled();
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    assert!(refused(c.request(Request::Move { id: Some(w.id), x: 100, y: 100 })));
    assert!(refused(c.request(Request::Resize { id: Some(w.id), rect: Rect::new(100, 100, 300, 200) })));
    // Unfilling (the zoom box, the client's own unmaximize) leaves it filled.
    c.ok(Request::Fill { id: Some(w.id), state: Some(false) });
    // Its bottom edge doesn't offer to resize it, or resize it.
    pointer(&c, vec![motion(640, 799)]);
    let Response::InputState { state } = c.request(Request::InputState) else { panic!("input state") };
    assert!(!state.cursor_name.as_deref().unwrap_or("").contains("resize"), "{:?}", state.cursor_name);
    pointer(&c, vec![button(true), motion(640, 700), motion(640, 600), button(false)]);
    c.settle();
    let now = c.window("org.example.editor").unwrap();
    assert!(now.fill && now.geometry == filled(), "{now:?}");
    // Nor does ⌃❖-right-drag.
    let key = |k: &str, pressed| Synthetic::Key { key: k.into(), pressed };
    let right = |pressed| Synthetic::PointerButton { button: "right".into(), pressed };
    pointer(
        &c,
        vec![key("ctrl", true), key("super", true), motion(900, 600), right(true), motion(700, 400), right(false)],
    );
    pointer(&c, vec![key("super", false), key("ctrl", false)]);
    c.settle();
    let now = c.window("org.example.editor").unwrap();
    assert!(now.fill && now.geometry == filled(), "{now:?}");
    // A drag by its tab moves it only until it's dropped.
    let (x, y) = (w.geometry.x + 30, w.geometry.y - 10);
    pointer(&c, vec![motion(x, y), button(true), motion(x + 100, y + 80), motion(x + 200, y + 150)]);
    c.wait_windows("dragged", |ws| ws[0].geometry.x > 100);
    pointer(&c, vec![button(false)]);
    let ws = c.wait_windows("filled again", |ws| ws[0].fill && ws[0].geometry == filled());
    // Its free place is the one it had, not where it was dropped.
    c.ok(Request::Float { id: Some(w.id), state: Some(true) });
    let ws2 = c.wait_windows("floated", |ws| ws[0].float && !ws[0].fill);
    let free = ws2[0].geometry;
    assert_eq!((free.w, free.h), (1280 * 2 / 3, 800 * 3 / 4), "{ws:?} {ws2:?}");
    assert!(free.x < 300, "not where it was dropped: {free:?}");
}

#[test]
fn floating_by_hand_frees_a_tiled_window_and_bringing_it_down_tiles_it() {
    let c = tiled();
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    c.ok(Request::Float { id: Some(w.id), state: Some(true) });
    let ws = c.wait_windows("floated", |ws| ws[0].float && !ws[0].fill);
    assert!(ws[0].geometry.w < 1278 && ws[0].geometry.h < 774, "a smaller free place: {ws:?}");
    // The person's to place now.
    c.ok(Request::Move { id: Some(w.id), x: 50, y: 60 });
    c.wait_windows("moved", |ws| (ws[0].geometry.x, ws[0].geometry.y) == (50, 60));
    c.ok(Request::Float { id: Some(w.id), state: Some(false) });
    c.wait_windows("tiled again", |ws| !ws[0].float && ws[0].fill && ws[0].geometry == filled());
}

#[test]
fn a_split_stays_split() {
    let c = tiled();
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let a = c.wait_window("org.example.editor");
    c.ok(Request::Split { id: Some(a.id), side: aizome_ipc::Side::Left });
    c.wait_windows("split", |ws| !ws[0].fill && ws[0].geometry.w < 640);
    // Windows coming tile; the split keeps its half.
    let _b = c.client(Spec::new("org.example.viewer", 0xcc2222));
    c.wait_window("org.example.viewer");
    c.settle();
    let ws = c.windows();
    assert!(!win(&ws, "org.example.editor").fill && win(&ws, "org.example.editor").geometry.w < 640, "{ws:?}");
    assert!(win(&ws, "org.example.viewer").fill, "{ws:?}");
    // Fill takes it back into its tile.
    c.ok(Request::Fill { id: Some(a.id), state: None });
    c.wait_windows("filled", |ws| win(ws, "org.example.editor").geometry == filled());
}

#[test]
fn full_screen_and_back_returns_to_the_tile() {
    let c = tiled();
    let a = c.client(Spec::new("org.example.player", 0x2222cc));
    let w = c.wait_window("org.example.player");
    c.ok(Request::Fullscreen { id: Some(w.id), state: Some(true) });
    a.wait_seen("full screen", |s| s.fullscreen && s.size == (1280, 800));
    c.ok(Request::Fullscreen { id: Some(w.id), state: Some(false) });
    c.wait_windows("tiled again", |ws| ws[0].fill && ws[0].geometry == filled());
    a.wait_seen("filled", |s| !s.fullscreen && s.maximized && s.size == (1278, 774));
    // Opening full screen, it isn't filled first.
    let g = c.client(Spec::new("org.example.game", 0xcc2222).fullscreen(None));
    c.wait_windows("the game", |ws| ws.iter().any(|w| w.app_id == "org.example.game" && w.fullscreen));
    let seen = g.wait_seen("full screen", |s| s.fullscreen);
    assert!(!seen.configured.contains(&(1278, 774)), "{:?}", seen.configured);
}

fn shell(c: &Compositor) -> ShellClient {
    let shell = ShellClient::spawn(c);
    shell.layer(
        LayerSpec::new("aizome-menubar", Layer::Top, KeyboardInteractivity::None)
            .anchor(Anchor::Top | Anchor::Left | Anchor::Right, (0, BAR as u32))
            .zone(BAR),
    );
    shell.layer(
        LayerSpec::new("aizome-deskbar", Layer::Top, KeyboardInteractivity::OnDemand)
            .anchor(Anchor::Top | Anchor::Right, (DESKBAR as u32, 360))
            .zone(-1)
            .top_margin(BAR),
    );
    c.settle();
    shell
}

#[test]
fn tiled_windows_resize_with_the_menu_bar_and_deskbar() {
    let c = tiled();
    let shell = shell(&c);
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    assert_eq!(w.geometry, Rect::new(1, BAR + 25, 1280 - DESKBAR - 2, 800 - BAR - 26), "beside the Deskbar");
    shell.send(ShellCmd::Resize { name: "aizome-deskbar", size: (300, 440) });
    c.wait_windows("the Deskbar widened", |ws| ws[0].geometry.w == 1280 - 300 - 2);
    shell.send(ShellCmd::Destroy("aizome-deskbar"));
    c.wait_windows("the Deskbar gone", |ws| ws[0].geometry.w == 1278);
    a.wait_seen("the client follows", |s| s.maximized && s.size == (1278, 800 - BAR - 26));
    // A window opened now fills the area as it is.
    let _b = c.client(Spec::new("org.example.viewer", 0xcc2222));
    let b = c.wait_window("org.example.viewer");
    assert_eq!(b.geometry, Rect::new(1, BAR + 25, 1278, 800 - BAR - 26));
}

fn settings(c: &Compositor, text: &str) {
    std::fs::write(c.dir.join("config/aizome/settings.toml"), text).unwrap();
    c.ok(Request::ReloadConfig);
}

#[test]
fn tiling_on_and_off_live_undoes_only_what_it_did() {
    let c = Compositor::start(Options { settings: FLOATING, ..Default::default() });
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc).size(400, 300));
    let a = c.wait_window("org.example.editor");
    c.ok(Request::Move { id: Some(a.id), x: 77, y: 99 });
    let _f = c.client(Spec::new("org.example.viewer", 0x22cc22));
    let f = c.wait_window("org.example.viewer");
    c.ok(Request::Fill { id: Some(f.id), state: Some(true) });
    let _calc = c.client(Spec::new("org.example.calculator", 0xcccccc).fixed_size(240, 160));
    c.wait_windows("three", |ws| ws.len() == 3 && win(ws, "org.example.viewer").fill);

    settings(&c, TILED);
    let ws = c.wait_windows("tiled", |ws| win(ws, "org.example.editor").fill);
    assert_eq!(win(&ws, "org.example.editor").geometry, filled());
    assert!(win(&ws, "org.example.viewer").fill, "{ws:?}");
    c.wait_windows("the calculator floats", |ws| win(ws, "org.example.calculator").float);

    settings(&c, FLOATING);
    let ws = c.wait_windows("untiled", |ws| !win(ws, "org.example.editor").fill);
    assert_eq!(win(&ws, "org.example.editor").geometry, Rect::new(77, 99, 400, 300), "back where it was");
    assert!(win(&ws, "org.example.viewer").fill, "filled by hand, it stays filled: {ws:?}");
    c.wait_windows("the calculator comes down", |ws| !win(ws, "org.example.calculator").float);
}

#[test]
fn with_any_app_tiled_windows_open_as_tabs_of_one_frame() {
    let settings =
        "[appearance]\neffects = \"austere\"\n[windows]\nlayout = \"tiled\"\nstack_new_windows = \"any-app\"\n";
    let c = Compositor::start(Options { settings, ..Default::default() });
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    let _b = c.client(Spec::new("org.example.viewer", 0xcc2222));
    let ws = c.wait_windows("stacked", |ws| ws.len() == 2 && ws.iter().all(|w| w.group.is_some()));
    assert!(ws.iter().all(|w| w.fill && w.geometry == filled()), "{ws:?}");
    // Taken out of the group, a tab fills a frame of its own.
    let b = win(&ws, "org.example.viewer").id;
    c.ok(Request::Unstack { id: Some(b) });
    let ws = c.wait_windows("apart", |ws| ws.iter().all(|w| w.group.is_none()));
    assert!(ws.iter().all(|w| w.fill && w.geometry == filled()), "{ws:?}");
}
