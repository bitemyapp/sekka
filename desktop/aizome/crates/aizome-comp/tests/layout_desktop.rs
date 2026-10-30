//! Optional end-to-end UI check with the real shell and installed apps.
//! See docs/ipc.md for the command. No connection to the login session's
//! display, configuration, browser profile or D-Bus is used.

mod common;

use std::path::Path;
use std::time::Duration;

use aizome_ipc::{Request, Synthetic};
use common::desktop::*;
use common::*;

#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL pointing at the shell under test"]
fn real_deskbar_autohides_after_leaving_its_app_rows() {
    let shell = std::env::var("AIZOME_LAYOUT_TEST_SHELL").expect("shell binary");
    let c = Compositor::start(Options {
        settings: "[appearance]\neffects = \"austere\"\n[deskbar]\nauto_hide = true\n",
        ..Default::default()
    });
    let _shell = program(&c, &shell, &[], "shell");
    wait("shell ready", || {
        shell_state(&c, &shell).is_some_and(|s| s["screens"].as_array().is_some_and(|a| !a.is_empty()))
    });
    let shown = || {
        shell_state(&c, &shell)
            .is_some_and(|s| s["surfaces"].as_array().is_some_and(|ss| ss.iter().any(|v| v == "Deskbar")))
    };
    assert!(!shown(), "auto-hide starts closed");
    let _client = c.client(Spec::new("deskbar-repro", 0x6688aa));
    let w = c.wait_window("deskbar-repro");
    c.ok(Request::Fill { id: Some(w.id), state: Some(true) });
    c.settle();
    let before = c.window("deskbar-repro").unwrap().geometry;
    shell_msg(&c, &shell, serde_json::json!({"request":"deskbar", "collapsed":false}));
    wait("Deskbar revealed", shown);
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: 1220.0, y: 40.0 }] });
    c.settle();
    std::thread::sleep(Duration::from_millis(500));
    assert!(shown(), "hovering an app row keeps the real shell open");
    assert_eq!(c.window("deskbar-repro").unwrap().geometry, before);
    keep(&c.screenshot(), "deskbar-auto-hide-revealed");
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: 200.0, y: 400.0 }] });
    wait("Deskbar auto-hidden", || !shown());
    assert_eq!(c.window("deskbar-repro").unwrap().geometry, before);
}

/// The real shell's Deskbar edge, dragged out 90pt at either side, makes
/// it 90pt wider. The Deskbar keeps its size while the button's held (an
/// outline shows the width on a surface of its own), so the windows beside
/// it are fitted once, when it's let go: a drag out and back leaves them as
/// they were.
#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL pointing at the shell under test"]
fn real_deskbar_edge_resizes_at_either_side() {
    let shell = std::env::var("AIZOME_LAYOUT_TEST_SHELL").expect("shell binary");
    for side in ["right", "left"] {
        let settings = format!("[appearance]\neffects = \"austere\"\n[deskbar]\nside = \"{side}\"\n");
        let c = Compositor::start(Options { settings: &settings, ..Default::default() });
        let _shell = program(&c, &shell, &[], "shell");
        wait("the shell's Deskbar", || {
            shell_state(&c, &shell)
                .is_some_and(|s| s["surfaces"].as_array().is_some_and(|ss| ss.iter().any(|v| v == "Deskbar")))
        });
        c.settle();
        // A free window that fits beside the Deskbar as it is, and not
        // beside one 90pt wider.
        let _app = c.client(Spec::new("beside", 0x886644).size(1000, 600));
        c.wait_window("beside");
        c.settle();
        let geometry = || c.window("beside").expect("the free window").geometry;
        let before = geometry();
        let button = |pressed| Synthetic::PointerButton { button: "left".into(), pressed };
        let drag = |path: &[f64], y: f64, release: bool| {
            c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: path[0], y }] });
            c.settle();
            c.ok(Request::Synthesize { events: vec![button(true)] });
            for pair in path.windows(2) {
                for i in 1..=30 {
                    let x = pair[0] + (pair[1] - pair[0]) * f64::from(i) / 30.0;
                    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x, y }] });
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            c.settle();
            std::thread::sleep(Duration::from_millis(300));
            c.settle();
            if release {
                c.ok(Request::Synthesize { events: vec![button(false)] });
            }
        };
        // On a 1280pt output, 2pt inside the 232pt face's inner edge, under
        // the grip.
        let y = 24.0 + 10.0 + 6.0;
        let (from, to, out) = if side == "left" { (230.0, 320.0, 380.0) } else { (1050.0, 960.0, 900.0) };
        drag(&[from, to], y, false);
        assert_eq!(geometry(), before, "at the {side}: the windows wait for the release");
        c.ok(Request::Synthesize { events: vec![button(false)] });
        let saved = || std::fs::read_to_string(c.dir.join("config/aizome/settings.toml")).unwrap_or_default();
        wait("a new width saved", || saved().contains("width = ") && !saved().contains("width = 232.0"));
        assert!(saved().contains("width = 322.0"), "at the {side}:\n{}", saved());
        wait("the window fitted beside the wider Deskbar", || geometry() != before);
        let fitted = geometry();
        assert!(fitted.w < before.w, "at the {side}: {before:?} -> {fitted:?}");
        // Out 60pt from the new edge and back before letting go: nothing
        // changes, and the window keeps its size.
        drag(&[to, out, to], y, true);
        c.settle();
        std::thread::sleep(Duration::from_millis(500));
        c.settle();
        assert_eq!(geometry(), fitted, "at the {side}: a drag out and back leaves the window alone");
        assert!(saved().contains("width = 322.0"), "at the {side}:\n{}", saved());
    }
}

/// The real palette and compositor, with a desktop ID different from the
/// window's app ID. The Exec marker catches accidental duplicate launches.
#[test]
#[ignore = "requires AIZOME_ACTIVATION_TEST_SHELL pointing to a built aizome-shell"]
fn palette_reveals_running_apps_on_their_desks() {
    use aizome_ipc::{AppSel, DeskSel, Response};
    let shell = std::env::var("AIZOME_ACTIVATION_TEST_SHELL")
        .or_else(|_| std::env::var("AIZOME_LAYOUT_TEST_SHELL"))
        .expect("shell binary");
    let c = Compositor::start(Options::default());
    let apps = c.dir.join("data/applications");
    std::fs::create_dir_all(&apps).unwrap();
    let marker = c.dir.join("duplicate-launch");
    std::fs::write(
        apps.join("org.example.ActivationFixture.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=ActivationFixture\nStartupWMClass=FixtureWindow\nExec=touch {}\n",
            marker.display()
        ),
    )
    .unwrap();
    let _shell = program(&c, &shell, &[], "shell");
    wait("the shell's Deskbar", || {
        shell_state(&c, &shell)
            .is_some_and(|s| s["surfaces"].as_array().is_some_and(|ss| ss.iter().any(|s| s == "Deskbar")))
    });
    let a = c.client(Spec::new("FixtureWindow", 0x2266cc));
    let wa = c.wait_window("FixtureWindow");
    let b = c.client(Spec::new("test.cover", 0x22cc22));
    let wb = c.wait_window("test.cover");
    c.ok(Request::Move { id: Some(wa.id), x: 120, y: 150 });
    c.ok(Request::Move { id: Some(wb.id), x: 120, y: 150 });
    b.wait_seen("cover has the keyboard", |s| s.keyboard_focus);

    let activate = || {
        shell_msg(&c, &shell, serde_json::json!({"request":"palette", "scope":"apps", "query":"ActivationFixture"}));
        wait("palette open", || shell_state(&c, &shell).is_some_and(|s| s["palette"] == "ActivationFixture"));
        a.wait_seen("palette took the keyboard", |s| !s.keyboard_focus);
        c.settle();
        key(&c, "return");
        wait("palette closed", || shell_state(&c, &shell).is_some_and(|s| s["palette"].is_null()));
        a.wait_seen("app has the keyboard", |s| s.keyboard_focus);
        let ws = c.windows();
        assert!(ws.iter().any(|w| w.id == wa.id && w.focused && !w.minimised && !w.hidden && w.group_active));
        let selected = ws.iter().find(|w| w.id == wa.id).unwrap();
        let top = ws.last().unwrap();
        assert!(
            top.id == wa.id || selected.group.is_some() && top.group == selected.group,
            "the app's stack was raised"
        );
        c.settle();
        assert_px(&c.screenshot(), 200, 250, rgb(0x2266cc), "the selected app is visible above its cover");
        assert!(!marker.exists(), "activation launched a duplicate process");
    };
    activate();
    // Asking for the already-focused app still returns the keyboard.
    activate();
    c.ok(Request::AddDesk { name: "Other".into(), folder: None, index: None });
    c.ok(Request::MoveToDesk { id: Some(wa.id), desk: DeskSel::Index(2), follow: false });
    c.ok(Request::Minimise { id: Some(wa.id) });
    c.ok(Request::HideApp { app: AppSel { app_id: Some("FixtureWindow".into()), pid: None } });
    activate();
    assert!(matches!(c.request(Request::Desks), Response::Desks { current: 2, .. }));
    // A sticky window is brought forward without leaving the current Desk.
    c.ok(Request::Sticky { id: Some(wa.id), state: Some(true) });
    c.ok(Request::SetDesk { desk: DeskSel::Index(1), output: None });
    c.ok(Request::Focus { id: wb.id });
    activate();
    assert!(matches!(c.request(Request::Desks), Response::Desks { current: 1, .. }));
    // An inactive stack tab is selected as well as raised.
    c.ok(Request::Stack { a: wa.id, b: wb.id });
    c.ok(Request::Focus { id: wb.id });
    activate();
}

#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL and AIZOME_LAYOUT_TEST_CHROME; see docs/ipc.md"]
fn real_launcher_chrome_and_calculator_keep_the_deskbar_clear() {
    let shell = std::env::var("AIZOME_LAYOUT_TEST_SHELL").expect("shell binary");
    let chrome = std::env::var("AIZOME_LAYOUT_TEST_CHROME").expect("Chrome binary");
    assert!(Path::new(&shell).is_file() && Path::new(&chrome).is_file());
    let c = Compositor::start(Options {
        outputs: &["2880x1800@2"],
        // Ensure launched programs also cannot inherit the login session's bus.
        env: &[
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent-aizome-layout-bus"),
            ("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent-aizome-layout-bus"),
        ],
        ..Default::default()
    });
    let _shell = program(&c, &shell, &[], "shell");
    wait("the shell's Deskbar", || {
        shell_state(&c, &shell)
            .is_some_and(|s| s["surfaces"].as_array().is_some_and(|ss| ss.iter().any(|v| v == "Deskbar")))
    });
    c.settle();
    let profile = format!("--user-data-dir={}/chrome", c.dir.display());
    let _chrome = program(
        &c,
        &chrome,
        &[
            &profile,
            "--ozone-platform=wayland",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-background-mode",
            "--disable-gpu",
            "--window-size=1800,1200",
            "about:blank",
        ],
        "chrome",
    );
    let windows = c.wait_windows("Chrome", |ws| !ws.is_empty());
    let browser = windows[0].id;
    c.wait_windows("Chrome's default fits", |ws| {
        ws.iter().find(|w| w.id == browser).is_some_and(|w| w.geometry.x + w.geometry.w <= 1208)
    });
    let _tab = c.client(Spec::new("layout-tab", 0x6688aa).title("Layout regression"));
    let tab = c.wait_window("layout-tab");
    c.ok(Request::Stack { a: browser, b: tab.id });
    c.ok(Request::Fill { id: Some(browser), state: Some(true) });
    c.ok(Request::Focus { id: browser });
    c.settle();
    let before = c.windows().into_iter().find(|w| w.id == browser).unwrap().geometry;
    keep(&c.screenshot(), "layout-desktop-before");

    // Use the actual launcher and its keyboard path, as in the recording.
    shell_msg(&c, &shell, serde_json::json!({"request":"palette", "scope":"apps", "query":"Soroban"}));
    wait("the launcher", || shell_state(&c, &shell).is_some_and(|s| s["palette"] == "Soroban"));
    wait_layer_keyboard_focus(&c);
    key(&c, "return");
    c.wait_windows("Calculator launched", |ws| ws.iter().any(|w| w.app_id == "org.aizome.Soroban"));
    wait("launcher closed", || shell_state(&c, &shell).is_some_and(|s| s["palette"].is_null()));
    c.settle();
    for w in c.windows().into_iter().filter(|w| w.id == browser || w.id == tab.id) {
        assert_eq!(w.geometry, before, "launch changed {}", w.app_id);
        assert!(w.geometry.x + w.geometry.w <= 1208);
    }
    keep(&c.screenshot(), "layout-desktop-after-launch");
    // Repeat with keyboard open/escape, and toggle the Deskbar while grouped.
    key(&c, "super+space");
    wait("palette reopened", || shell_state(&c, &shell).is_some_and(|s| !s["palette"].is_null()));
    wait_layer_keyboard_focus(&c);
    key(&c, "escape");
    wait("palette dismissed", || shell_state(&c, &shell).is_some_and(|s| s["palette"].is_null()));
    shell_msg(&c, &shell, serde_json::json!({"request":"deskbar", "collapsed":true}));
    c.wait_windows("Deskbar collapsed", |ws| {
        ws.iter().any(|w| w.id == browser && w.geometry.w == if w.client_side_decorations { 1440 } else { 1438 })
    });
    shell_msg(&c, &shell, serde_json::json!({"request":"deskbar", "collapsed":false}));
    c.wait_windows("Deskbar restored", |ws| ws.iter().any(|w| w.id == browser && w.geometry == before));
    c.ok(Request::Focus { id: tab.id });
    c.settle();
    assert_eq!(c.window("layout-tab").unwrap().geometry, before);
    keep(&c.screenshot(), "layout-desktop-after-toggle");
    for w in c.windows() {
        c.ok(Request::Close { id: Some(w.id) });
    }
}

/// A repeatable heterogeneous tab-group demo using actual applications,
/// including switching members and tearing out the active tab with Alt.
#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_CHROME and AIZOME_LAYOUT_TEST_CALCULATOR; see docs/ipc.md"]
fn chrome_and_calculator_share_a_tab_group_and_tear_apart() {
    let chrome = std::env::var("AIZOME_LAYOUT_TEST_CHROME").expect("Chrome binary");
    let calculator = std::env::var("AIZOME_LAYOUT_TEST_CALCULATOR").expect("Calculator binary");
    let c = Compositor::start(Options { outputs: &["1440x900@1"], ..Default::default() });
    let profile = format!("--user-data-dir={}/chrome", c.dir.display());
    let _chrome = program(
        &c,
        &chrome,
        &[
            &profile,
            "--ozone-platform=wayland",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-background-mode",
            "--disable-gpu",
            "about:blank",
        ],
        "chrome",
    );
    let browser = c.wait_windows("Chrome", |ws| !ws.is_empty())[0].id;
    let _calculator = program(&c, &calculator, &[], "calculator");
    let calc = c.wait_window("org.aizome.Soroban");
    assert_ne!(c.windows().iter().find(|w| w.id == browser).unwrap().app_id, calc.app_id);
    c.ok(Request::Move { id: Some(browser), x: 140, y: 180 });
    c.ok(Request::Resize { id: Some(browser), rect: aizome_ipc::Rect::new(140, 180, 900, 560) });
    c.ok(Request::Stack { a: browser, b: calc.id });
    c.wait_windows("heterogeneous group", |ws| {
        ws.len() == 2 && ws.iter().all(|w| w.group.is_some() && w.group_members.len() == 2)
    });
    c.settle();
    keep(&c.screenshot(), "heterogeneous-group-calculator");
    let geometry = c.window("org.aizome.Soroban").unwrap().geometry;
    // Switch by clicking the first tab, then use normal focus to return.
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: (geometry.x + 30) as f64, y: (geometry.y - 12) as f64 },
            Synthetic::Click { button: "left".into() },
        ],
    });
    c.wait_windows("Chrome's tab selected", |ws| ws.iter().any(|w| w.id == browser && w.group_active && w.focused));
    c.settle();
    keep(&c.screenshot(), "heterogeneous-group-browser");
    // A separate gesture, outside the tab's double-click interval.
    std::thread::sleep(std::time::Duration::from_millis(450));
    // Alt-drag the now active first tab out, keeping both clients alive.
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::Key { key: "alt".into(), pressed: true },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerMotion { x: (geometry.x + 70) as f64, y: (geometry.y + 100) as f64 },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
            Synthetic::Key { key: "alt".into(), pressed: false },
        ],
    });
    c.wait_windows("both windows detached", |ws| ws.len() == 2 && ws.iter().all(|w| w.group.is_none()));
    c.settle();
    keep(&c.screenshot(), "heterogeneous-group-detached");
    for w in c.windows() {
        c.ok(Request::Close { id: Some(w.id) });
    }
}
