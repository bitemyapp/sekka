//! The compositor end to end: a headless instance, Wayland test clients,
//! IPC and screenshot pixels, under GLES and pixman.

mod common;

use aizome_ipc::{DeskSel, Request, Synthetic};
use common::*;

// Night palette.
const FOCUS: u32 = 0xe0af68;
const TAB_IDLE: u32 = 0x2f334d;
const BEVEL_LO: u32 = 0x0b0c12;
const DESKTOP: u32 = 0x16161e;
/// `slab` (black at 50%) over the desktop.
const SHADOW_ON_DESKTOP: u32 = 0x0b0b0f;

fn opts(renderer: Option<&'static str>) -> Options<'static> {
    Options { renderer, ..Default::default() }
}

fn check_decorations(renderer: Option<&'static str>) {
    // Lean, the default tier: hard shadows.
    let c = Compositor::start(Options { renderer, settings: "", ..Default::default() });
    let _a = c.client(Spec::new("test.red", 0xcc2222).size(400, 300));
    let w = c.wait_window("test.red");
    assert!(w.focused, "a new window takes focus");
    assert!(!w.client_side_decorations);
    c.settle();
    let img = c.screenshot();
    let g = w.geometry;
    assert_eq!((g.w, g.h), (400, 300));
    // Content.
    assert_px(&img, g.x + 200, g.y + 150, rgb(0xcc2222), "content");
    // The focused tab: amber, 22pt tall, its border on top.
    assert_px(&img, g.x + 2, g.y - 14, rgb(FOCUS), "focused tab");
    assert_px(&img, g.x + 2, g.y - 25, rgb(BEVEL_LO), "tab top border");
    assert_px(&img, g.x - 1, g.y - 14, rgb(BEVEL_LO), "tab left border");
    // The rail: 2pt of focus colour above the content, the frame edge above it.
    let far = g.x + g.w - 10; // beyond the tab
    assert_px(&img, far, g.y - 1, rgb(FOCUS), "rail");
    assert_px(&img, far, g.y - 2, rgb(FOCUS), "rail");
    assert_px(&img, far, g.y - 3, rgb(BEVEL_LO), "frame top edge");
    assert_px(&img, far, g.y - 4, rgb(DESKTOP), "above the frame, beside the tab");
    // The frame's 1pt edge.
    assert_px(&img, g.x - 1, g.y + 100, rgb(BEVEL_LO), "frame left edge");
    assert_px(&img, g.x + g.w, g.y + 100, rgb(BEVEL_LO), "frame right edge");
    assert_px(&img, g.x + 100, g.y + g.h, rgb(BEVEL_LO), "frame bottom edge");
    // The hard shadow: 3pt right and down, not blurred.
    let right = g.x + g.w + 1; // first column past the frame
    assert_px(&img, right, g.y + 100, rgb(SHADOW_ON_DESKTOP), "shadow right");
    assert_px(&img, right + 2, g.y + 100, rgb(SHADOW_ON_DESKTOP), "shadow right, 3rd column");
    assert_px(&img, right + 3, g.y + 100, rgb(DESKTOP), "past the shadow");
    let bottom = g.y + g.h + 1;
    assert_px(&img, g.x + 100, bottom + 2, rgb(SHADOW_ON_DESKTOP), "shadow bottom");
    assert_px(&img, g.x + 100, bottom + 3, rgb(DESKTOP), "past the shadow");
    // Offset 3pt: the frame's left edge casts nothing straight down.
    assert_px(&img, g.x - 1, bottom, rgb(DESKTOP), "no shadow left of the offset");
    assert_px(&img, g.x + 2, bottom, rgb(SHADOW_ON_DESKTOP), "shadow starts 3pt in");
    // The frame's right shadow starts 3pt down.
    assert_px(&img, right, g.y - 3, rgb(DESKTOP), "no shadow beside the frame's top");
}

#[test]
fn decorations_gles() {
    check_decorations(None);
}

#[test]
fn decorations_pixman() {
    check_decorations(Some("pixman"));
}

#[test]
fn unfocused_tabs_are_idle() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc).title("A"));
    let a = c.wait_window("test.a");
    let _b = c.client(Spec::new("test.b", 0x22cc22).title("B"));
    c.wait_windows("b focused", |ws| ws.iter().any(|w| w.app_id == "test.b" && w.focused));
    let ws = c.windows();
    let a2 = ws.iter().find(|w| w.app_id == "test.a").unwrap();
    let b = ws.iter().find(|w| w.app_id == "test.b").unwrap();
    assert_eq!(a.geometry, a2.geometry);
    assert_ne!((a.geometry.x, a.geometry.y), (b.geometry.x, b.geometry.y), "the second window cascades");
    c.settle();
    let img = c.screenshot();
    // B's tab is amber; A's (cascaded up-left of B, so its tab shows) idle.
    assert_px(&img, b.geometry.x + 2, b.geometry.y - 14, rgb(FOCUS), "focused tab");
    assert_px(&img, a.geometry.x + 2, a.geometry.y - 14, rgb(TAB_IDLE), "idle tab");
    // A's rail isn't amber.
    let rail = img.px(a.geometry.x + 5, a.geometry.y - 1);
    assert!(!near(rail, rgb(FOCUS)), "idle rail");
}

#[test]
fn ipc_focus_raises_and_activates() {
    let c = Compositor::start(opts(None));
    let a = c.client(Spec::new("test.a", 0x2222cc));
    let _wa = c.wait_window("test.a");
    let b = c.client(Spec::new("test.b", 0x22cc22));
    let wb = c.wait_window("test.b");
    b.wait_seen("b activated", |s| s.activated);
    let wa = c.window("test.a").unwrap();
    c.ok(Request::Focus { id: wa.id });
    a.wait_seen("a activated", |s| s.activated);
    b.wait_seen("b deactivated", |s| !s.activated);
    let ws = c.windows();
    assert_eq!(ws.last().unwrap().id, wa.id, "raised to the top");
    assert!(ws.iter().find(|w| w.id == wb.id).is_some_and(|w| !w.focused));
}

#[test]
fn activating_a_hidden_app_announces_every_restored_window() {
    use aizome_ipc::{AppSel, Event};
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.editor", 0x2222cc));
    let a = c.wait_window("test.editor");
    let _b = c.client(Spec::new("test.editor", 0x22cc22));
    let windows = c.wait_windows("two editor windows", |ws| ws.len() == 2);
    let expected: std::collections::BTreeSet<_> = windows.iter().map(|w| w.id).collect();
    c.ok(Request::HideApp { app: AppSel { app_id: Some("test.editor".into()), pid: None } });
    c.wait_windows("both hidden", |ws| ws.iter().all(|w| w.hidden));
    let events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    c.ok(Request::Focus { id: a.id });
    assert!(c.windows().iter().all(|w| !w.hidden), "the app was restored");
    let mut announced = std::collections::BTreeSet::new();
    for event in events {
        let Ok(event) = event else { break };
        if let Event::WindowChanged { window } = event
            && !window.hidden
        {
            announced.insert(window.id);
        }
        if announced == expected {
            break;
        }
    }
    assert_eq!(announced, expected, "the shell must hear that every restored window is visible");
}

#[test]
fn activating_a_stack_tab_announces_the_previous_tabs_state() {
    use aizome_ipc::Event;
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc));
    let a = c.wait_window("test.a");
    let _b = c.client(Spec::new("test.b", 0x22cc22));
    let b = c.wait_window("test.b");
    c.ok(Request::Stack { a: a.id, b: b.id });
    let _cover = c.client(Spec::new("test.cover", 0xcc2222));
    c.wait_window("test.cover");
    let events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    c.ok(Request::Focus { id: a.id });
    assert!(
        events.map_while(Result::ok).any(|event| {
            matches!(event, Event::WindowChanged { window } if window.id == b.id && !window.group_active)
        }),
        "the shell must hear that the previous tab is inactive"
    );
}

#[test]
fn stacks_share_a_frame_and_switch() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc).title("Alpha"));
    let wa = c.wait_window("test.a");
    let _b = c.client(Spec::new("test.b", 0x22cc22).title("Beta").size(300, 200));
    let wb = c.wait_window("test.b");
    c.ok(Request::Stack { a: wa.id, b: wb.id });
    let ws = c.wait_windows("b resized into a's frame", |ws| {
        let a = ws.iter().find(|w| w.id == wa.id).unwrap();
        let b = ws.iter().find(|w| w.id == wb.id).unwrap();
        a.geometry == b.geometry && a.group.is_some() && a.group == b.group
    });
    let b = ws.iter().find(|w| w.id == wb.id).unwrap();
    assert!(b.group_active && b.focused);
    assert_eq!(b.group_members, vec![wa.id, wb.id]);
    c.settle();
    let g = b.geometry;
    let img = c.screenshot();
    assert_px(&img, g.x + 20, g.y + 20, rgb(0x22cc22), "B shows");
    // Two tabs side by side: Alpha's (background) then Beta's (focused).
    assert_px(&img, g.x + 2, g.y - 14, rgb(0x2a2e45), "Alpha's background tab");
    let row: Vec<[u8; 3]> = (g.x..g.x + g.w).map(|x| img.px(x, g.y - 14)).collect();
    let amber = row.iter().position(|p| near(*p, rgb(FOCUS))).expect("Beta's focused tab");
    assert!(amber > 64, "Beta's tab comes after Alpha's");
    // Showing Alpha (clicking its tab).
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: g.x as f64 + 30.0, y: g.y as f64 - 12.0 },
            Synthetic::Click { button: "left".into() },
        ],
    });
    c.wait_windows("alpha shows", |ws| ws.iter().any(|w| w.id == wa.id && w.group_active && w.focused));
    c.settle();
    let img = c.screenshot();
    assert_px(&img, g.x + 20, g.y + 20, rgb(0x2222cc), "A shows");
}

#[test]
fn desks_hide_and_show_windows() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let wa = c.wait_window("test.a");
    c.settle();
    let g = wa.geometry;
    let img = c.screenshot();
    assert_px(&img, g.x + 10, g.y + 10, rgb(0xcc2222), "on desk 1");
    c.ok(Request::SetDesk { desk: DeskSel::Index(2), output: None });
    c.settle();
    let img = c.screenshot();
    assert_px(&img, g.x + 10, g.y + 10, rgb(DESKTOP), "gone on desk 2");
    assert_px(&img, g.x + 2, g.y - 14, rgb(DESKTOP), "its tab too");
    c.ok(Request::MoveToDesk { id: Some(wa.id), desk: DeskSel::Index(2), follow: false });
    c.settle();
    let img = c.screenshot();
    assert_px(&img, g.x + 10, g.y + 10, rgb(0xcc2222), "moved to desk 2");
    let w = c.window("test.a").unwrap();
    assert_eq!(w.desk, Some(2));
}

#[test]
fn clicking_focuses_and_dragging_the_tab_moves() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc));
    let wa = c.wait_window("test.a");
    let _b = c.client(Spec::new("test.b", 0x22cc22));
    c.wait_windows("b focused", |ws| ws.iter().any(|w| w.app_id == "test.b" && w.focused));
    // Click A's content where B doesn't cover it.
    let p = (wa.geometry.x as f64 + 5.0, wa.geometry.y as f64 + 5.0);
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x: p.0, y: p.1 }, Synthetic::Click { button: "left".into() }],
    });
    c.wait_windows("a focused by a click", |ws| ws.iter().any(|w| w.id == wa.id && w.focused));
    // Drag A by its tab, on the title just past the close box: the tab is
    // only as wide as the title, so further along, in Archivo, is the
    // zoom box.
    let tab = (wa.geometry.x as f64 + 30.0, wa.geometry.y as f64 - 12.0);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: tab.0, y: tab.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: tab.0 + 20.0, y: tab.1 + 10.0 },
            Synthetic::PointerMotion { x: tab.0 + 100.0, y: tab.1 + 50.0 },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
        ],
    });
    let w = c.window("test.a").unwrap();
    assert_eq!((w.geometry.x, w.geometry.y), (wa.geometry.x + 100, wa.geometry.y + 50));
}

#[test]
fn the_close_box_closes() {
    let c = Compositor::start(opts(None));
    let a = c.client(Spec::new("test.a", 0x2222cc));
    let wa = c.wait_window("test.a");
    let close = (wa.geometry.x as f64 + 10.0, wa.geometry.y as f64 - 14.0);
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x: close.0, y: close.1 }, Synthetic::Click { button: "left".into() }],
    });
    a.wait_seen("close", |s| s.close_requested);
    c.wait_windows("closed", |ws| ws.is_empty());
}

#[test]
fn client_side_decorations_get_the_tab_only() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.csd", 0x22cccc).csd());
    let w = c.wait_window("test.csd");
    assert!(w.client_side_decorations);
    c.settle();
    let g = w.geometry;
    let img = c.screenshot();
    assert_px(&img, g.x + 2, g.y - 10, rgb(FOCUS), "the tab sits right on the content");
    assert_px(&img, g.x + 2, g.y + 2, rgb(0x22cccc), "no rail over the content");
    assert_px(&img, g.x - 1, g.y + 100, rgb(DESKTOP), "no frame");
}

#[test]
fn ipc_resize_keeps_to_the_clients_minimum() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.min", 0x2222cc).min_size(320, 240));
    let w = c.wait_window("test.min");
    c.ok(Request::Resize { id: Some(w.id), rect: aizome_ipc::Rect::new(100, 100, 200, 100) });
    let ws = c.wait_windows("resized", |ws| ws[0].geometry.x == 100);
    assert_eq!(ws[0].geometry, aizome_ipc::Rect::new(100, 100, 320, 240), "no smaller than the client allows");
}

#[test]
fn fill_takes_the_usable_area() {
    let c = Compositor::start(opts(None));
    let a = c.client(Spec::new("test.a", 0x2222cc));
    let wa = c.wait_window("test.a");
    c.ok(Request::Fill { id: Some(wa.id), state: Some(true) });
    a.wait_seen("maximized", |s| s.maximized && s.size == (1280 - 2, 800 - 26));
    let w = c.wait_windows("filled", |ws| ws[0].fill && ws[0].geometry.w == 1278);
    assert_eq!(w[0].geometry, aizome_ipc::Rect::new(1, 25, 1278, 774));
    c.ok(Request::Fill { id: Some(wa.id), state: Some(false) });
    a.wait_seen("restored", |s| !s.maximized && s.size == (400, 300));
}

#[test]
fn idle_means_no_frames() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc));
    c.wait_window("test.a");
    c.settle();
    let before = frames(&c);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let after = frames(&c);
    assert_eq!(before, after, "nothing changed, nothing drawn");
    // Moving a window draws, without the client redrawing.
    let w = c.window("test.a").unwrap();
    c.ok(Request::Move { id: Some(w.id), x: w.geometry.x + 30, y: w.geometry.y });
    c.settle();
    assert!(frames(&c) > after);
}

fn frames(c: &Compositor) -> u64 {
    match c.request(Request::Stats { reset: false }) {
        aizome_ipc::Response::Stats { stats } => stats.outputs[0].frames_rendered,
        r => panic!("{r:?}"),
    }
}

#[test]
fn austere_has_no_shadow() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    c.settle();
    let g = w.geometry;
    let img = c.screenshot();
    assert_px(&img, g.x + g.w + 1, g.y + 100, rgb(DESKTOP), "no shadow right");
    assert_px(&img, g.x + 100, g.y + g.h + 2, rgb(DESKTOP), "no shadow below");
    assert_px(&img, g.x + 2, g.y - 14, rgb(FOCUS), "the tab is still there");
}

#[test]
fn long_titles_keep_the_tab_short() {
    let c = Compositor::start(opts(None));
    let long = "callen@ganymede: ~/work/a/very/long/path/that/goes/on/and/on/for/ever/and/ever/sidestep";
    let _a = c.client(Spec::new("test.a", 0xcc2222).size(900, 300).title(long));
    let w = c.wait_window("test.a");
    c.settle();
    let g = w.geometry;
    let img = c.screenshot();
    // A lone tab is at most 360pt wide: past that, the frame's top edge.
    assert_px(&img, g.x - 1 + 359, g.y - 14, rgb(BEVEL_LO), "the tab's right border");
    assert_px(&img, g.x - 1 + 365, g.y - 14, rgb(DESKTOP), "beside the tab");
}

#[test]
fn dragging_a_tab_onto_another_stacks_them() {
    let c = Compositor::start(Options { settings: "", ..Default::default() });
    let _a = c.client(Spec::new("test.a", 0x2222cc).title("cargo test"));
    let wa = c.wait_window("test.a");
    let _b = c.client(Spec::new("test.b", 0x22cc22).title("sidestep").size(500, 350));
    let wb = c.wait_window("test.b");
    // Move B well away so the windows don't overlap.
    c.ok(Request::Move { id: Some(wb.id), x: 700, y: 400 });
    c.ok(Request::Move { id: Some(wa.id), x: 100, y: 100 });
    c.ok(Request::Focus { id: wa.id });
    c.settle();
    // Grab A's tab and carry it just past the end of B's tab.
    let start = (130.0, 100.0 - 12.0);
    let over = (700.0 + 140.0, 400.0 - 12.0);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: start.0, y: start.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: start.0 + 30.0, y: start.1 + 30.0 },
            Synthetic::PointerMotion { x: over.0, y: over.1 },
        ],
    });
    c.settle();
    let img = c.screenshot();
    keep(&img, "drag-to-stack");
    // The dashed amber slot after B's tab.
    let row: Vec<[u8; 3]> = (700..1200).map(|x| img.px(x, 400 - 25)).collect();
    assert!(row.iter().any(|p| near(*p, rgb(FOCUS))), "a dashed amber slot along the tab row");
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerButton { button: "left".into(), pressed: false }] });
    let ws = c.wait_windows("stacked", |ws| ws.iter().all(|w| w.group.is_some()));
    let a = ws.iter().find(|w| w.id == wa.id).unwrap();
    let b = ws.iter().find(|w| w.id == wb.id).unwrap();
    assert_eq!(a.group, b.group);
    assert!(a.group_active && a.focused, "the dragged window shows");
    assert_eq!(a.group_members, vec![wb.id, wa.id]);
    c.wait_windows("a takes b's frame", |ws| ws.iter().all(|w| w.geometry == b.geometry));
    c.settle();
    keep(&c.screenshot(), "stacked");
    // Dragging the background tab (B) away tears it out.
    let tab_b = (701.0 + 20.0, 400.0 - 12.0);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: tab_b.0, y: tab_b.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: tab_b.0 + 20.0, y: tab_b.1 + 60.0 },
            Synthetic::PointerMotion { x: tab_b.0 - 400.0, y: tab_b.1 + 100.0 },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
        ],
    });
    let ws = c.wait_windows("unstacked", |ws| ws.iter().all(|w| w.group.is_none()));
    let b = ws.iter().find(|w| w.id == wb.id).unwrap();
    assert_eq!((b.geometry.x, b.geometry.y), (300, 500));
}

/// Runs a tool against the compositor if it's installed.
fn tool(c: &Compositor, cmd: &str, args: &[&str]) -> Option<std::process::Output> {
    let found =
        std::process::Command::new("sh").arg("-c").arg(format!("command -v {cmd}")).output().ok()?.status.success();
    if !found {
        eprintln!("{cmd} isn't installed; skipping");
        return None;
    }
    std::process::Command::new(cmd)
        .args(args)
        .env("WAYLAND_DISPLAY", &c.socket)
        .env("XDG_RUNTIME_DIR", &c.dir)
        .output()
        .ok()
}

#[test]
fn grim_wlr_randr_and_lswt_work() {
    let c = Compositor::start(Options { outputs: &["1280x800@1", "1024x768@1"], ..Default::default() });
    let _a = c.client(Spec::new("test.a", 0xcc2222).title("Screencopy"));
    let w = c.wait_window("test.a");
    c.settle();
    // wlr-screencopy.
    let shot = c.dir.join("grim.png");
    if let Some(out) = tool(&c, "grim", &["-o", "HEADLESS-1", shot.to_str().unwrap()]) {
        assert!(out.status.success(), "grim: {}", String::from_utf8_lossy(&out.stderr));
        let img = Image::load(&shot);
        assert_eq!((img.width, img.height), (1280, 800));
        assert_px(&img, w.geometry.x + 50, w.geometry.y + 50, rgb(0xcc2222), "grim sees the window");
    }
    // wlr-output-management.
    if let Some(out) = tool(&c, "wlr-randr", &["--output", "HEADLESS-2", "--pos", "1280,100", "--scale", "2"]) {
        assert!(out.status.success(), "wlr-randr: {}", String::from_utf8_lossy(&out.stderr));
        let aizome_ipc::Response::Outputs { outputs } = c.request(Request::Outputs) else { panic!() };
        let o = outputs.iter().find(|o| o.name == "HEADLESS-2").unwrap();
        assert_eq!((o.logical.x, o.logical.y, o.scale), (1280, 100, 2.0));
        assert_eq!((o.logical.w, o.logical.h), (512, 384));
    }
    // wlr-foreign-toplevel-management.
    if let Some(out) = tool(&c, "lswt", &[]) {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("test.a") && text.contains("Screencopy"), "lswt: {text}");
    }
}

#[test]
fn dragging_the_frame_edge_resizes() {
    let c = Compositor::start(opts(None));
    let a = c.client(Spec::new("test.a", 0x2222cc).size(400, 300));
    let w = c.wait_window("test.a");
    let g = w.geometry;
    // The bottom-right corner, just outside the frame.
    let corner = ((g.x + g.w + 3) as f64, (g.y + g.h + 3) as f64);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: corner.0, y: corner.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: corner.0 + 60.0, y: corner.1 + 40.0 },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
        ],
    });
    a.wait_seen("resized", |s| s.size == (460, 340));
    let w2 = c.wait_windows("the model follows", |ws| ws[0].geometry.w == 460);
    assert_eq!((w2[0].geometry.x, w2[0].geometry.y), (g.x, g.y), "the top-left stays");
    // From the left edge the right edge stays put.
    let left = ((g.x - 3) as f64, (g.y + 150) as f64);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: left.0, y: left.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: left.0 + 50.0, y: left.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
        ],
    });
    let ws = c.wait_windows("narrower", |ws| ws[0].geometry.w == 410);
    assert_eq!(ws[0].geometry.x + ws[0].geometry.w, g.x + 460, "anchored on the right");
}

#[test]
fn shift_dragging_a_tab_slides_it_along_the_top() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc).size(600, 300).title("Slide"));
    let w = c.wait_window("test.a");
    let g = w.geometry;
    let tab = (g.x as f64 + 40.0, g.y as f64 - 12.0);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::Key { key: "shift".into(), pressed: true },
            Synthetic::PointerMotion { x: tab.0, y: tab.1 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: tab.0 + 200.0, y: tab.1 + 30.0 },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
            Synthetic::Key { key: "shift".into(), pressed: false },
        ],
    });
    c.settle();
    let img = c.screenshot();
    // The window didn't move; its tab now starts 200pt in.
    let w2 = c.window("test.a").unwrap();
    assert_eq!(w2.geometry, g);
    assert_px(&img, g.x + 5, g.y - 14, rgb(DESKTOP), "the tab left its corner");
    assert_px(&img, g.x + 200 + 2, g.y - 14, rgb(FOCUS), "and sits 200pt along");
}

#[test]
fn settings_apply_live() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc));
    let w = c.wait_window("test.a");
    c.settle();
    let img = c.screenshot();
    assert_px(&img, w.geometry.x + 2, w.geometry.y - 14, rgb(FOCUS), "Night's amber");
    std::fs::write(
        c.dir.join("config/aizome/settings.toml"),
        "[appearance]\ntheme = \"day\"\nfocus = \"cyan\"\neffects = \"austere\"\n",
    )
    .unwrap();
    c.ok(Request::ReloadConfig);
    c.settle();
    let img = c.screenshot();
    assert_px(&img, w.geometry.x + 2, w.geometry.y - 14, rgb(0x8fd8f7), "Day's cyan");
    assert_px(&img, 3, 3, rgb(0xc9cde0), "Day's desktop");
}

#[test]
fn windows_reopen_where_their_app_left_them() {
    let c = Compositor::start(opts(None));
    let a = c.client(Spec::new("test.remember", 0x2222cc).size(500, 320));
    let w = c.wait_window("test.remember");
    c.ok(Request::Move { id: Some(w.id), x: 77, y: 99 });
    c.wait_windows("moved", |ws| ws[0].geometry.x == 77);
    drop(a);
    c.wait_windows("closed", |ws| ws.is_empty());
    let _b = c.client(Spec::new("test.remember", 0x2222cc).size(500, 320));
    let w = c.wait_window("test.remember");
    assert_eq!((w.geometry.x, w.geometry.y), (77, 99));
}

#[test]
fn the_screenshot_chord_saves_and_announces() {
    let c = Compositor::start(opts(None));
    let _a = c.client(Spec::new("test.a", 0x2222cc));
    c.wait_window("test.a");
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: "super+shift+3".into() }] });
    let path = events.by_ref().find_map(|e| match e {
        Ok(aizome_ipc::Event::ScreenshotTaken { path }) => Some(path),
        _ => None,
    });
    let path = std::path::PathBuf::from(path.expect("a screenshot event"));
    assert!(path.starts_with(c.dir.join("home/Pictures/Screenshots")), "{}", path.display());
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("Screenshot ") && name.contains(" at ") && name.ends_with(".png"), "{name}");
    let img = Image::load(&path);
    assert_eq!((img.width, img.height), (1280, 800));
}

#[test]
fn the_settings_file_is_watched() {
    let c = Compositor::start(opts(None));
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        c.dir.join("config/aizome/settings.toml"),
        "[appearance]\neffects = \"austere\"\n[[desks]]\nname = \"Mail\"\n[[desks]]\nname = \"Build\"\nfolder = \"~/work\"\n",
    )
    .unwrap();
    let changed = events.by_ref().any(|e| matches!(e, Ok(aizome_ipc::Event::ConfigChanged)));
    assert!(changed, "config-changed without a reload request");
    let aizome_ipc::Response::Desks { desks, .. } = c.request(Request::Desks) else { panic!() };
    let names: Vec<&str> = desks.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Mail", "Build"]);
    assert_eq!(desks[1].folder.as_deref(), Some("~/work"));
}

#[test]
fn programs_started_here_know_the_session_isnt_real() {
    // So the shell doesn't take over the machine's polkit agent, sleep or
    // locking from a test.
    let c = Compositor::start(opts(None));
    let out = c.dir.join("env");
    let script = format!(
        "printf '%s %s' \"$XDG_CURRENT_DESKTOP\" \"$AIZOME_SESSION\" > {}.tmp && mv {0}.tmp {0}",
        out.display()
    );
    c.ok(Request::Spawn { argv: vec!["sh".into(), "-c".into(), script], cwd: None, in_desk_folder: false });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !out.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&out).unwrap_or_default(), "Aizome 0");
}
