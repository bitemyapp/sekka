mod common;
use common::*;

/// An occluded client must not render at the fullscreen client's frame rate.
#[test]
fn fullscreen_throttles_covered_clients_and_resumes_them_when_visible() {
    let c = Compositor::start(Options { renderer: Some("pixman"), ..Default::default() });
    let background = c.client(Spec::new("animated-background", 0x224466).animated());
    c.wait_window("animated-background");
    background.wait_seen("background animates", |s| s.frames >= 4);
    use common::layer::{Anchor, KeyboardInteractivity, Layer, LayerSpec, ShellClient, ShellCmd};
    let shell = ShellClient::spawn(&c);
    shell.send(ShellCmd::Layer(
        LayerSpec::new("panel", Layer::Top, KeyboardInteractivity::None)
            .anchor(Anchor::Top | Anchor::Left | Anchor::Right, (0, 30)),
    ));
    shell.wait("panel mapped", |s| s.shown.iter().any(|n| n == "panel"));
    shell.send(ShellCmd::Frame("panel"));
    shell.wait("visible panel gets a frame", |s| s.frames.get("panel") == Some(&1));
    let foreground = c.client(Spec::new("animated-fullscreen", 0x6688aa).animated());
    let front = c.wait_window("animated-fullscreen");
    c.ok(aizome_ipc::Request::Fullscreen { id: Some(front.id), state: Some(true) });
    foreground.wait_seen("fullscreen configured", |s| s.fullscreen && s.size == (1280, 800));
    c.settle();
    shell.send(ShellCmd::Frame("panel"));
    let behind = background.seen().frames;
    let ahead = foreground.seen().frames;
    foreground.wait_seen("fullscreen continues animating", |s| s.frames >= ahead + 12);
    assert!(
        background.seen().frames <= behind + 2,
        "covered client rendered {} frames during 12 fullscreen frames",
        background.seen().frames - behind
    );
    assert_eq!(shell.seen.lock().unwrap().frames.get("panel"), Some(&1), "covered panel got a frame callback");
    c.ok(aizome_ipc::Request::Fullscreen { id: Some(front.id), state: Some(false) });
    c.ok(aizome_ipc::Request::Move { id: Some(front.id), x: 800, y: 400 });
    background.wait_seen("uncovered client resumes", |s| s.frames >= behind + 6);
    shell.wait("uncovered panel gets its pending frame", |s| s.frames.get("panel") == Some(&2));
}
