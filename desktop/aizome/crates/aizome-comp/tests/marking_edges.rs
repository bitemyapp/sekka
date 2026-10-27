mod common;
use aizome_ipc::{Request, Synthetic};
use common::*;

/// A quick right-click must keep the same edge protection as holding the
/// button until the marking menu appears.
#[test]
fn a_quick_marking_menu_click_near_the_top_keeps_every_verb_visible() {
    let c = Compositor::start(Options::default());
    let a = c.client(Spec::new("test.menu-edge", 0xcc2222));
    let w = c.wait_window("test.menu-edge");
    c.ok(Request::Move { id: Some(w.id), x: 400, y: 30 });
    c.settle();
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: 460.0, y: 18.0 },
            Synthetic::PointerButton { button: "right".into(), pressed: true },
            Synthetic::PointerButton { button: "right".into(), pressed: false },
        ],
    });
    c.settle();
    let img = c.screenshot();
    keep(&img, "marking-menu-top-edge");
    // The menu is inset to y=144; the fill chip is 96 points above it.
    assert_px(&img, 440, 45, rgb(0x2a2e44), "the entire fill chip is on screen");
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x: 460.0, y: 48.0 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
            Synthetic::PointerButton { button: "left".into(), pressed: false },
        ],
    });
    a.wait_seen("fill from the visible menu", |s| s.maximized);
}
