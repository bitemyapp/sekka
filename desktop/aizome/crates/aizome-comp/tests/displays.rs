//! Displays: turning outputs off and on, mirroring one on another (scaled
//! to fit, letterboxed), the main display, and Night Light over IPC.

mod common;

use std::time::Duration;

use aizome_ipc::{Event, OutputInfo, Request, Response};
use common::*;

fn outputs(c: &Compositor) -> Vec<OutputInfo> {
    match c.request(Request::Outputs) {
        Response::Outputs { outputs } => outputs,
        r => panic!("{r:?}"),
    }
}

fn output(c: &Compositor, name: &str) -> OutputInfo {
    outputs(c).into_iter().find(|o| o.name == name).unwrap_or_else(|| panic!("no {name}"))
}

fn config(name: &str, enabled: Option<bool>) -> Request {
    Request::OutputConfig { name: name.into(), mode: None, scale: None, position: None, transform: None, enabled }
}

fn two() -> Options<'static> {
    Options { outputs: &["1280x800@1", "1280x800@1"], settings: "", ..Default::default() }
}

#[test]
fn an_output_turned_off_leaves_the_layout_and_comes_back() {
    let c = Compositor::start(two());
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    c.ok(Request::Move { id: Some(w.id), x: 1500, y: 200 });
    c.wait_windows("on the second output", |ws| ws[0].output.as_deref() == Some("HEADLESS-2"));
    c.ok(config("HEADLESS-2", Some(false)));
    let o = output(&c, "HEADLESS-2");
    assert!(!o.enabled, "{o:?}");
    // Its window moved to the output left.
    c.wait_windows("the window rescued", |ws| ws[0].output.as_deref() == Some("HEADLESS-1"));
    let shot = c.screenshot();
    assert_eq!(shot.width, 1280, "only one output in the layout");
    // On again, right of the first.
    c.ok(config("HEADLESS-2", Some(true)));
    let o = output(&c, "HEADLESS-2");
    assert!(o.enabled);
    assert_eq!((o.logical.x, o.logical.w), (1280, 1280));
    assert_eq!(c.screenshot().width, 2560);
    drop(a);
}

#[test]
fn an_output_off_in_the_settings_is_listed_and_can_be_turned_on() {
    let c = Compositor::start(Options {
        outputs: &["1280x800@1", "1280x800@1"],
        settings: "[[outputs]]\nname = \"HEADLESS-2\"\nenabled = false\n",
        ..Default::default()
    });
    let o = output(&c, "HEADLESS-2");
    assert!(!o.enabled);
    assert_eq!(c.screenshot().width, 1280);
    c.ok(config("HEADLESS-2", Some(true)));
    assert!(output(&c, "HEADLESS-2").enabled);
}

#[test]
fn a_mirror_shows_its_source_scaled_to_fit() {
    // The source is 1280×800; the mirror 960×720 gets it 960×600, with
    // 60-pixel black bars above and below.
    let c = Compositor::start(Options { outputs: &["1280x800@1", "960x720@1"], settings: "", ..Default::default() });
    let _a = c.client(Spec::new("org.example.editor", 0x22cc22).size(400, 300));
    let w = c.wait_window("org.example.editor");
    c.ok(Request::Move { id: Some(w.id), x: 400, y: 300 });
    c.ok(Request::MirrorOutput { name: "HEADLESS-2".into(), source: Some("HEADLESS-1".into()) });
    let m = output(&c, "HEADLESS-2");
    assert_eq!(m.mirror.as_deref(), Some("HEADLESS-1"));
    assert_eq!((m.logical.x, m.logical.w), (0, 1280), "a mirror shows its source's place");
    assert_eq!(c.screenshot().width, 1280, "a mirror has no place of its own");
    c.settle();
    let path = c.dir.join("mirror.png");
    c.ok(Request::Screenshot {
        output: Some("HEADLESS-2".into()),
        path: Some(path.to_string_lossy().into()),
        window: None,
        area: None,
        clipboard: false,
    });
    let img = Image::load(&path);
    keep(&img, "displays-mirror");
    assert_eq!((img.width, img.height), (960, 720));
    assert_px(&img, 480, 20, rgb(0x000000), "the bar above");
    assert_px(&img, 480, 700, rgb(0x000000), "the bar below");
    // The window (400,300)–(800,600) on the source is (300,285)–(600,510)
    // here, at ×0.75 and 60 down.
    assert_px(&img, 450, 397, rgb(0x22cc22), "the window, scaled");
    assert!(!near(img.px(620, 397), rgb(0x22cc22)), "the window ends at 600: it was scaled, not cropped");
    // Its own picture again.
    c.ok(Request::MirrorOutput { name: "HEADLESS-2".into(), source: None });
    let m = output(&c, "HEADLESS-2");
    assert_eq!(m.mirror, None);
    assert_eq!(m.logical.x, 1280);
}

#[test]
fn mirroring_from_the_settings() {
    let c = Compositor::start(Options {
        outputs: &["1280x800@1", "1280x800@1"],
        settings: "[[outputs]]\nname = \"HEADLESS-2\"\nmirror = \"HEADLESS-1\"\n",
        ..Default::default()
    });
    assert_eq!(output(&c, "HEADLESS-2").mirror.as_deref(), Some("HEADLESS-1"));
    // Turning the source off gives the mirror its own picture.
    c.ok(config("HEADLESS-1", Some(false)));
    let m = output(&c, "HEADLESS-2");
    assert_eq!(m.mirror, None);
    assert!(m.enabled);
}

#[test]
fn the_main_display_is_reported() {
    let c = Compositor::start(Options {
        outputs: &["1280x800@1", "1280x800@1"],
        settings: "[[outputs]]\nname = \"HEADLESS-2\"\nprimary = true\n",
        ..Default::default()
    });
    assert!(output(&c, "HEADLESS-2").primary);
    assert!(!output(&c, "HEADLESS-1").primary);
    c.ok(Request::SetPrimaryOutput { name: "HEADLESS-1".into() });
    assert!(output(&c, "HEADLESS-1").primary);
    assert!(!output(&c, "HEADLESS-2").primary);
    // New windows open on the main display's when the pointer starts there.
    let c = Compositor::start(Options {
        outputs: &["1280x800@1", "1280x800@1"],
        settings: "[[outputs]]\nname = \"HEADLESS-2\"\nprimary = true\n",
        ..Default::default()
    });
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    assert_eq!(w.output.as_deref(), Some("HEADLESS-2"));
}

#[test]
fn night_light_over_ipc() {
    let c = Compositor::start(Options::default());
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let state = match c.request(Request::NightLight { enabled: None, temperature: None }) {
        Response::NightLight { state } => state,
        r => panic!("{r:?}"),
    };
    assert!(!state.enabled);
    assert_eq!(state.current, 6500);
    assert_eq!(state.temperature, 4000);
    assert!(state.outputs.is_empty(), "headless outputs have no gamma: {state:?}");
    // Always on, warmer.
    let state = match c.request(Request::NightLight { enabled: Some(true), temperature: Some(3400) }) {
        Response::NightLight { state } => state,
        r => panic!("{r:?}"),
    };
    assert!(state.enabled);
    assert_eq!(state.temperature, 3400);
    let got = events.by_ref().find_map(|e| match e {
        Ok(Event::NightLightChanged { state }) => Some(state),
        _ => None,
    });
    assert!(got.is_some_and(|s| s.enabled && s.temperature == 3400));
    assert!(matches!(c.request(Request::NightLight { enabled: None, temperature: Some(900) }), Response::Error { .. }));
}

#[test]
fn night_light_follows_its_schedule() {
    let c = Compositor::start(Options {
        settings: "[night_light]\nenabled = true\nschedule = \"always\"\ntemperature = 3000\n",
        ..Default::default()
    });
    let state = match c.request(Request::NightLight { enabled: None, temperature: None }) {
        Response::NightLight { state } => state,
        r => panic!("{r:?}"),
    };
    assert!(state.active);
    assert_eq!(state.current, 3000);
    assert_eq!(state.schedule, "always");
    assert_eq!((state.from, state.to, state.place), (None, None, None));
}

#[test]
fn night_light_runs_from_sunset_to_sunrise_where_the_computer_is() {
    // Paris, set by hand: sunset in the evening and sunrise in the
    // morning, whatever the date (and the test machine's zone, give or
    // take a day's hours), not 19:00 and 07:00.
    let c = Compositor::start(Options {
        settings: "[night_light]\nenabled = true\nschedule = \"sunset\"\nlatitude = 48.8667\nlongitude = 2.3333\n",
        ..Default::default()
    });
    let state = match c.request(Request::NightLight { enabled: None, temperature: None }) {
        Response::NightLight { state } => state,
        r => panic!("{r:?}"),
    };
    assert_eq!(state.schedule, "sunset");
    assert_eq!(state.place.as_deref(), Some("settings"));
    let (from, to) = (state.from.expect("today's sunset"), state.to.expect("today's sunrise"));
    assert_eq!((from.len(), to.len()), (5, 5), "{from} {to}");
    assert_ne!((from.as_str(), to.as_str()), ("19:00", "07:00"));
}

#[test]
fn the_renderer_setting_applies_at_the_next_start() {
    let c = Compositor::start(Options {
        renderer: Some("gles"),
        settings: "[performance]\nrenderer = \"software\"\n",
        ..Default::default()
    });
    let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!() };
    assert_eq!(stats.renderer_setting, "software");
    assert_eq!(stats.renderer_after_restart, stats.renderer != "pixman");
    // Without a renderer on the command line, the setting chooses.
    let c = Compositor::start(Options { settings: "[performance]\nrenderer = \"software\"\n", ..Default::default() });
    let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!() };
    assert_eq!(stats.renderer, "pixman");
    assert!(!stats.renderer_after_restart);
}
