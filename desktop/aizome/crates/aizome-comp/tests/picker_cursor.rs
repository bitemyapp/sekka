mod common;
use aizome_ipc::{Request, Synthetic};
use common::*;
fn key(k: &str, pressed: bool) -> Synthetic {
    Synthetic::Key { key: k.into(), pressed }
}

/// Pointer motion over the desktop, tabs and client content must not replace
/// the picker's crosshair or window cursor before the selection completes.
#[test]
fn the_picker_keeps_its_cursor_until_done() {
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("test.cursor", 0xcc2222));
    let w = c.wait_window("test.cursor");
    let cursor = || {
        let aizome_ipc::Response::InputState { state } = c.request(Request::InputState) else { panic!() };
        state.cursor_name
    };
    let motion = |x, y| c.ok(Request::Synthesize { events: vec![Synthetic::PointerMotion { x, y }] });
    c.ok(Request::PickArea { tag: "cursor".into(), prompt: None, targets: Vec::new() });
    for (x, y) in [
        (10.0, 10.0),
        ((w.geometry.x + 60) as f64, (w.geometry.y - 12) as f64),
        ((w.geometry.x + 20) as f64, (w.geometry.y + 20) as f64),
    ] {
        motion(x, y);
        assert_eq!(cursor().as_deref(), Some("crosshair"));
    }
    c.ok(Request::Synthesize { events: vec![key("space", true), key("space", false)] });
    motion(20.0, 20.0);
    assert_eq!(cursor().as_deref(), Some("pointer"));
    c.ok(Request::Synthesize {
        events: vec![
            key("space", true),
            key("space", false),
            Synthetic::PointerButton { button: "left".into(), pressed: true },
        ],
    });
    motion(150.0, 150.0);
    assert_eq!(cursor().as_deref(), Some("crosshair"));
    c.ok(Request::Synthesize { events: vec![Synthetic::PointerButton { button: "left".into(), pressed: false }] });
    motion(10.0, 10.0);
    assert_eq!(cursor().as_deref(), Some("default"));
    c.ok(Request::PickArea { tag: "cursor".into(), prompt: None, targets: Vec::new() });
    c.ok(Request::Synthesize { events: vec![key("escape", true), key("escape", false)] });
    motion(10.0, 10.0);
    assert_eq!(cursor().as_deref(), Some("default"));
}
