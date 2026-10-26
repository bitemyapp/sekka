//! Placement against the real layer-shell and xdg-shell protocols. Each
//! compositor has its own sockets and state; no input reaches the live desktop.

mod common;

use aizome_ipc::{Rect, Request, Response, Side, Synthetic};
use common::layer::{Anchor, KeyboardInteractivity, Layer, LayerSpec, ShellClient, ShellCmd};
use common::*;

const BAR: i32 = 24;
const DESKBAR: i32 = 232;

// Also run these regressions against an installed build, without replacing
// either the binary or the live session.
fn start(opts: Options<'_>) -> Compositor {
    let exe = std::env::var_os("AIZOME_LAYOUT_TEST_EXE").map(std::path::PathBuf::from);
    Compositor::start(Options { exe: exe.as_deref(), ..opts })
}

fn deskbar(width: i32) -> LayerSpec {
    LayerSpec::new("aizome-deskbar", Layer::Top, KeyboardInteractivity::OnDemand)
        .anchor(Anchor::Top | Anchor::Right, (width as u32, 360))
        .zone(-1)
        .top_margin(BAR)
}

fn shell(c: &Compositor) -> ShellClient {
    let shell = ShellClient::spawn(c);
    shell.layer(
        LayerSpec::new("aizome-menubar", Layer::Top, KeyboardInteractivity::None)
            .anchor(Anchor::Top | Anchor::Left | Anchor::Right, (0, BAR as u32))
            .zone(BAR),
    );
    shell.layer(deskbar(DESKBAR));
    c.settle();
    shell
}

fn clear_of_chrome(w: &aizome_ipc::WindowInfo, width: i32, height: i32) -> bool {
    let g = w.geometry;
    let (top, edge) = if w.client_side_decorations { (22, 0) } else { (25, 1) };
    g.x >= edge && g.y >= BAR + top && g.x + g.w + edge <= width - DESKBAR && g.y + g.h + edge <= height
}

#[test]
fn closing_the_launcher_preserves_filled_groups_splits_and_restore_geometry() {
    let c = start(Options::default());
    let shell = shell(&c);
    let _a = c.client(Spec::new("group-a", 0x884422));
    let a = c.wait_window("group-a");
    let _b = c.client(Spec::new("group-b", 0x228844));
    let b = c.wait_window("group-b");
    c.ok(Request::Stack { a: a.id, b: b.id });
    c.ok(Request::Fill { id: Some(b.id), state: Some(true) });
    let _left = c.client(Spec::new("left", 0x442288));
    let left = c.wait_window("left");
    c.ok(Request::Split { id: Some(left.id), side: Side::Left });
    let _right = c.client(Spec::new("right", 0x224488));
    let right = c.wait_window("right");
    c.ok(Request::Split { id: Some(right.id), side: Side::Right });
    c.settle();
    let before = c.windows();
    assert!(before.iter().all(|w| clear_of_chrome(w, 1280, 800)));

    shell.layer(LayerSpec::new("aizome-palette", Layer::Overlay, KeyboardInteractivity::Exclusive));
    let _new = c.client(Spec::new("launched", 0xaa8844));
    c.wait_window("launched");
    shell.send(ShellCmd::Destroy("aizome-palette"));
    c.settle();
    for old in before {
        let now = c.window(&old.app_id).unwrap();
        assert_eq!(now.geometry, old.geometry, "launch changed {}", old.app_id);
        assert_eq!(now.group, old.group);
    }
    // Switching to either tab must retain the same frame and saved free rect.
    c.ok(Request::Focus { id: a.id });
    c.ok(Request::Fill { id: Some(a.id), state: Some(false) });
    c.settle();
    assert_eq!(c.window("group-a").unwrap().geometry, a.geometry);
    assert_eq!(c.window("group-b").unwrap().geometry, a.geometry);
    keep(&c.screenshot(), "layout-launch");
}

#[test]
fn oversized_defaults_and_remembered_windows_fit_at_each_scale() {
    for output in ["1280x800@1", "2880x1800@2", "1920x1200@1.5"] {
        let c = start(Options { outputs: &[output], ..Default::default() });
        let _shell = shell(&c);
        let Response::Outputs { outputs } = c.request(Request::Outputs) else { panic!("outputs") };
        let (width, height) = (outputs[0].logical.w, outputs[0].logical.h);
        for csd in [false, true] {
            let spec = Spec::new(if csd { "large-csd" } else { "large-ssd" }, 0x338866).size(1800, 1200);
            let spec = if csd { spec.csd() } else { spec };
            let client = c.client(spec.clone());
            let w = c.wait_window(&spec.app_id);
            c.wait_windows("default fits", |ws| {
                ws.iter().any(|w| w.app_id == spec.app_id && clear_of_chrome(w, width, height))
            });
            client.wait_seen("client resized to fit", |s| s.size.0 <= width - DESKBAR && s.size.1 <= height - BAR - 22);
            let bounds = client.seen().bounds.expect("configure bounds");
            assert!(bounds.0 <= width - DESKBAR && bounds.1 <= height - BAR);
            // A deliberately moved free window is remembered, but reopening
            // must not restore it beneath the Deskbar.
            c.ok(Request::Resize { id: Some(w.id), rect: Rect::new(width - 100, height - 100, 1800, 1200) });
            client.wait_seen("oversized by user", |s| s.size == (1800, 1200));
            c.wait_windows("oversized committed", |ws| {
                ws.iter().any(|w| w.app_id == spec.app_id && w.geometry.w == 1800)
            });
            drop(client);
            c.wait_windows("closed", |ws| ws.iter().all(|other| other.id != w.id));
            let reopened = c.client(spec.clone());
            c.wait_windows("remembered fits", |ws| {
                ws.iter().any(|w| w.app_id == spec.app_id && clear_of_chrome(w, width, height))
            });
            reopened
                .wait_seen("restored client fits", |s| s.size.0 <= width - DESKBAR && s.size.1 <= height - BAR - 22);
        }
        keep(&c.screenshot(), &format!("layout-defaults-{output}"));
    }
}

#[test]
fn deskbar_arrival_width_changes_and_removal_refit_without_losing_restore() {
    let c = start(Options::default());
    let _app = c.client(Spec::new("filled", 0x886644));
    let original = c.wait_window("filled");
    let tab_client = c.client(Spec::new("tab", 0x448866).configure_delay(std::time::Duration::from_millis(80)));
    let tab = c.wait_window("tab");
    c.ok(Request::Stack { a: original.id, b: tab.id });
    c.ok(Request::Fill { id: Some(original.id), state: Some(true) });
    c.settle();
    let shell = shell(&c);
    c.wait_windows("Deskbar arrived", |ws| ws[0].geometry.w == 1280 - DESKBAR - 2);
    shell.send(ShellCmd::Resize { name: "aizome-deskbar", size: (300, 440) });
    c.wait_windows("Deskbar widened", |ws| ws[0].geometry.w == 978);
    shell.send(ShellCmd::Destroy("aizome-deskbar"));
    c.wait_windows("Deskbar removed", |ws| ws[0].geometry.w == 1278);
    tab_client.wait_seen("latest resize committed", |s| s.maximized && s.size == (1278, 750));
    c.settle();
    assert_eq!(c.window("filled").unwrap().geometry.w, 1278, "an old commit replaced the latest layout");
    c.ok(Request::Fill { id: Some(original.id), state: Some(false) });
    c.settle();
    assert_eq!(c.window("filled").unwrap().geometry, original.geometry);
    assert_eq!(c.window("tab").unwrap().geometry, original.geometry);
}

#[test]
fn a_late_deskbar_and_output_shrink_fit_free_windows_and_fullscreen_restores() {
    let c = start(Options::default());
    let app = c.client(Spec::new("free", 0x886644).size(1200, 740));
    let free = c.wait_window("free");
    let _shell = shell(&c);
    c.wait_windows("free window clears arriving Deskbar", |ws| clear_of_chrome(&ws[0], 1280, 800));
    app.wait_seen("free client fits", |s| s.size.0 <= 1046);
    c.ok(Request::Fullscreen { id: Some(free.id), state: Some(true) });
    app.wait_seen("fullscreen", |s| s.fullscreen && s.size == (1280, 800));
    // A scale change makes the same physical output smaller in points.
    let Response::Outputs { outputs } = c.request(Request::Outputs) else { panic!("outputs") };
    c.ok(Request::OutputConfig {
        name: outputs[0].name.clone(),
        mode: None,
        scale: Some(1.25),
        position: None,
        transform: None,
        enabled: None,
    });
    app.wait_seen("fullscreen refits changed output", |s| s.size == (1024, 640));
    c.ok(Request::Fullscreen { id: Some(free.id), state: Some(false) });
    c.wait_windows("restore fits smaller work area", |ws| clear_of_chrome(&ws[0], 1024, 640));
    app.wait_seen("restored client fits smaller work area", |s| !s.fullscreen && s.size.0 <= 790 && s.size.1 <= 590);
}

#[test]
fn refitting_one_output_does_not_claim_a_window_on_its_neighbour() {
    let c = start(Options { outputs: &["1280x800@1", "1280x800@1"], ..Default::default() });
    let shell = shell(&c);
    // Overlapping output rectangles are legal. A window belongs to the
    // display showing most of it, not every display its edge intersects.
    c.ok(Request::OutputConfig {
        name: "HEADLESS-2".into(),
        mode: None,
        scale: None,
        position: Some([1200, 0]),
        transform: None,
        enabled: None,
    });
    let _app = c.client(Spec::new("neighbour", 0x886644));
    let w = c.wait_window("neighbour");
    c.ok(Request::Move { id: Some(w.id), x: 1400, y: 100 });
    c.ok(Request::Fill { id: Some(w.id), state: Some(true) });
    c.settle();
    let before = c.window("neighbour").unwrap().geometry;
    // An unrelated overlay on the first output comes and goes.
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x: 100.0, y: 100.0 }] });
    shell.layer(LayerSpec::new("menu", Layer::Overlay, KeyboardInteractivity::None));
    shell.send(ShellCmd::Destroy("menu"));
    c.settle();
    assert_eq!(c.window("neighbour").unwrap().geometry, before);
    shell.send(ShellCmd::Resize { name: "aizome-deskbar", size: (300, 440) });
    c.settle();
    assert_eq!(c.window("neighbour").unwrap().geometry, before);
    assert_eq!(before, Rect::new(1201, 25, 1278, 774));
}
