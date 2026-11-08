//! Displays: turning outputs off and on, mirroring one on another (scaled
//! to fit, letterboxed), screenshots across displays of different scales,
//! the main display, Night Light over IPC, and a laptop's panel going dark
//! while its lid is closed.

mod common;

use std::time::Duration;

use aizome_ipc::{Event, OutputInfo, Request, Response, Synthetic};
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

fn alpha(img: &Image, x: u32, y: u32) -> u8 {
    img.rgba[((y * img.width + x) * 4 + 3) as usize]
}

fn shot_of_area(c: &Compositor, x: i32, y: i32, w: i32, h: i32) -> Image {
    let path = c.dir.join(format!("area-{x}-{y}.png"));
    c.ok(Request::Screenshot {
        output: None,
        path: Some(path.to_string_lossy().into()),
        window: None,
        area: Some(aizome_ipc::Rect::new(x, y, w, h)),
        clipboard: false,
    });
    Image::load(&path)
}

/// A 2× panel with a 1× display beside it (#318). An area on the 1×
/// display is its own pixels; one across both, and the whole screen, are
/// at 2×, the 1× display's pixels doubled to fill its place. (Each display
/// was placed unscaled at the largest scale: a band low on the 1× display
/// came out empty, one higher up showed what was further down, and the
/// whole screen had the 1× display in a quarter of its place.)
#[test]
fn screenshots_across_displays_of_different_scales() {
    let c = Compositor::start(Options { outputs: &["1280x800@2", "800x600@1"], settings: "", ..Default::default() });
    let blue = rgb(0x2222cc);
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc).size(400, 300));
    let w = c.wait_window("org.example.editor");
    // (840, 250)–(1240, 550), on the 1× display at (640, 0)–(1440, 600).
    c.ok(Request::Move { id: Some(w.id), x: 840, y: 250 });
    c.wait_windows("on the 1× display", |ws| ws[0].output.as_deref() == Some("HEADLESS-2"));
    c.settle();

    // Print, and a band dragged low across the window.
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let button = |pressed| Synthetic::PointerButton { button: "left".into(), pressed };
    let motion = |x, y| Synthetic::PointerMotion { x, y };
    c.ok(Request::Synthesize {
        events: vec![
            motion(900.0, 400.0),
            Synthetic::Key { key: "print".into(), pressed: true },
            Synthetic::Key { key: "print".into(), pressed: false },
        ],
    });
    c.ok(Request::Synthesize {
        events: vec![button(true), motion(1000.0, 450.0), motion(1100.0, 500.0), button(false)],
    });
    let path = events
        .by_ref()
        .find_map(|e| match e {
            Ok(Event::ScreenshotTaken { path }) => Some(path),
            _ => None,
        })
        .expect("a screenshot");
    let band = Image::load(std::path::Path::new(&path));
    keep(&band, "displays-scales-band");
    assert_eq!((band.width, band.height), (200, 100), "the 1× display's own pixels");
    for (x, y) in [(0, 0), (100, 50), (199, 99)] {
        assert_px(&band, x, y, blue, "the window");
        assert_eq!(alpha(&band, x as u32, y as u32), 255);
    }

    // Across both: at 2×, the window from (840 − 600) × 2 = 480.
    let across = shot_of_area(&c, 600, 300, 400, 100);
    keep(&across, "displays-scales-across");
    assert_eq!((across.width, across.height), (800, 200));
    assert!(across.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255), "both displays fill it");
    assert!(!near(across.px(470, 100), blue), "the 1× display's desktop left of the window");
    assert_px(&across, 490, 100, blue, "the window's left edge, doubled into place");
    assert_px(&across, 799, 199, blue, "the window at the area's far corner");

    // The whole screen: 1440 × 600 points at 2×, the window at
    // (1680, 500)–(2480, 1100); the panel ends 400 points down.
    let full = c.screenshot();
    keep(&full, "displays-scales-full");
    assert_eq!((full.width, full.height), (2880, 1200));
    assert_px(&full, 1690, 510, blue, "the window's near corner");
    assert_px(&full, 2470, 1090, blue, "the window's far corner");
    assert!(!near(full.px(2490, 1090), blue), "and no further");
    assert_eq!(alpha(&full, 2879, 1199), 255, "the 1× display's far corner");
    assert_eq!(alpha(&full, 100, 1100), 0, "below the panel is off every display");
}

#[test]
fn fractional_screenshot_crops_match_the_native_capture() {
    for scale in [1.25, 1.5, 1.75] {
        let mode = format!("1920x1080@{scale}");
        let c = Compositor::start(Options { outputs: &[&mode], renderer: Some("pixman"), ..Default::default() });
        // A solid window would conceal repeated/skipped rows and columns.
        let _client = c.client(
            Spec::new("org.example.pixel-pattern", 0)
                .pixel_pattern(|x, y| ((x as u32 & 255) << 16) | ((y as u32 & 255) << 8) | ((x ^ y) as u32 & 255)),
        );
        let window = c.wait_window("org.example.pixel-pattern");
        c.ok(Request::Move { id: Some(window.id), x: 200, y: 100 });
        c.settle();
        let path = c.dir.join("native.png");
        c.ok(Request::Screenshot {
            output: Some("HEADLESS-1".into()),
            path: Some(path.to_string_lossy().into()),
            window: None,
            area: None,
            clipboard: false,
        });
        let native = Image::load(&path);
        for (x, y, w, h) in [(1, 1, 900, 500), (2, 3, 800, 450)] {
            let crop = shot_of_area(&c, x, y, w, h);
            assert_eq!(
                (crop.width, crop.height),
                ((w as f64 * scale).round() as u32, (h as f64 * scale).round() as u32)
            );
            let sx = (x as f64 * scale).round() as usize;
            let sy = (y as f64 * scale).round() as usize;
            let row_bytes = crop.width as usize * 4;
            for (row, pixels) in crop.rgba.chunks_exact(row_bytes).enumerate() {
                let start = ((sy + row) * native.width as usize + sx) * 4;
                assert_eq!(pixels, &native.rgba[start..start + row_bytes], "scale {scale}, crop ({x}, {y}), row {row}");
            }
        }
    }
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
    assert!(!stats.renderer_after_restart, "a startup override is not a pending change");
    for (setting, pending) in [("gpu", true), ("auto", true), ("software", false)] {
        std::fs::write(c.dir.join("config/aizome/settings.toml"), format!("[performance]\nrenderer = \"{setting}\"\n"))
            .unwrap();
        c.request(Request::Reload { logic: false, shell: false, from: None, shell_from: None, check: false });
        let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!() };
        assert_eq!(stats.renderer_setting, setting);
        assert_eq!(stats.renderer_after_restart, pending);
    }
    // Without a renderer on the command line, the setting chooses.
    let c = Compositor::start(Options { settings: "[performance]\nrenderer = \"software\"\n", ..Default::default() });
    let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!() };
    assert_eq!(stats.renderer, "pixman");
    assert!(!stats.renderer_after_restart);

    // A software renderer serving a GPU preference (override or fallback)
    // is also stable across logins until the preference actually changes.
    let c = Compositor::start(Options {
        renderer: Some("pixman"),
        settings: "[performance]\nrenderer = \"gpu\"\n",
        ..Default::default()
    });
    let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!() };
    assert_eq!(stats.renderer, "pixman");
    assert_eq!(stats.renderer_setting, "gpu");
    assert!(!stats.renderer_after_restart);
}

fn laptop() -> Options<'static> {
    Options { outputs: &["eDP-1=1280x800@1", "1280x800@1"], settings: "", ..Default::default() }
}

fn lid(c: &Compositor, closed: bool) {
    c.ok(Request::Synthesize { events: vec![Synthetic::Lid { closed }] });
}

/// Frames an output has drawn or tried to (with nothing changed) so far.
fn redraws(c: &Compositor, name: &str) -> u64 {
    let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!("no stats") };
    let o = stats.outputs.iter().find(|o| o.name == name).unwrap_or_else(|| panic!("no stats for {name}"));
    o.frames_rendered + o.frames_skipped
}

#[test]
fn closing_the_lid_turns_the_laptops_panel_off_until_it_opens() {
    let c = Compositor::start(laptop());
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    c.wait_windows("on the panel", |ws| ws[0].output.as_deref() == Some("eDP-1"));
    let original = output(&c, "eDP-1");
    assert!(output(&c, "eDP-1").powered && output(&c, "HEADLESS-2").powered);

    lid(&c, true);
    let panel = output(&c, "eDP-1");
    assert!(!panel.powered, "the panel stayed on behind the lid: {panel:?}");
    assert!(!panel.enabled, "the closed panel stayed in the layout");
    assert!(output(&c, "HEADLESS-2").powered, "a monitor went off with the lid");
    c.wait_windows("the focused Desk moved to the external display", |ws| {
        ws[0].focused && ws[0].output.as_deref() == Some("HEADLESS-2")
    });
    c.settle();
    assert_eq!(c.screenshot().width, 1280, "only the external display remains in the layout");
    // Turning the displays on doesn't light the panel behind it.
    c.ok(Request::PowerOffMonitors);
    assert!(!output(&c, "HEADLESS-2").powered);
    c.ok(Request::PowerOnMonitors);
    assert!(output(&c, "HEADLESS-2").powered);
    assert!(!output(&c, "eDP-1").powered, "powering the displays on lit the panel behind the lid");

    lid(&c, false);
    assert!(output(&c, "eDP-1").powered, "the panel stayed off with the lid open");
    assert_eq!(output(&c, "eDP-1").logical, original.logical);
    c.wait_windows("the Desk returned to the panel", |ws| ws[0].id == w.id && ws[0].output.as_deref() == Some("eDP-1"));
    c.settle();
    assert!(redraws(&c, "eDP-1") > 0, "the panel didn't draw once on again");
}

#[test]
fn docking_with_the_lid_closed_removes_the_panel_and_undocking_restores_it() {
    let c = Compositor::start(laptop());
    c.ok(config("HEADLESS-2", Some(false)));
    lid(&c, true);
    assert!(output(&c, "eDP-1").enabled, "alone, keep the dark panel's layout");
    assert!(!output(&c, "eDP-1").powered);
    c.ok(config("HEADLESS-2", Some(true)));
    c.settle();
    assert!(!output(&c, "eDP-1").enabled);
    c.ok(config("HEADLESS-2", Some(false)));
    c.settle();
    assert!(output(&c, "eDP-1").enabled);
    assert!(!output(&c, "eDP-1").powered, "still closed");
    lid(&c, false);
    assert!(output(&c, "eDP-1").powered);
}

#[test]
fn opening_the_lid_preserves_an_explicitly_disabled_panel() {
    let c = Compositor::start(laptop());
    // Already off before closing.
    c.ok(config("eDP-1", Some(false)));
    lid(&c, true);
    lid(&c, false);
    assert!(!output(&c, "eDP-1").enabled);
    // Turned off explicitly while the lid policy had it disabled.
    c.ok(config("eDP-1", Some(true)));
    lid(&c, true);
    c.ok(config("eDP-1", Some(false)));
    lid(&c, false);
    assert!(!output(&c, "eDP-1").enabled);
}

#[test]
fn opening_the_lid_restores_the_live_panel_configuration() {
    let c = Compositor::start(laptop());
    c.ok(Request::OutputConfig {
        name: "eDP-1".into(),
        mode: Some("1600x1000@60".into()),
        scale: Some(1.25),
        position: Some([-1280, 100]),
        transform: None,
        enabled: None,
    });
    let before = output(&c, "eDP-1");
    for _ in 0..3 {
        lid(&c, true);
        assert!(!output(&c, "eDP-1").enabled);
        lid(&c, false);
        let after = output(&c, "eDP-1");
        assert_eq!(after.logical, before.logical);
        assert_eq!(after.current_mode, before.current_mode);
        assert_eq!(after.scale, before.scale);
    }
}

#[test]
fn a_panel_another_display_mirrors_stays_on_with_the_lid_closed() {
    let c = Compositor::start(laptop());
    c.ok(Request::MirrorOutput { name: "HEADLESS-2".into(), source: Some("eDP-1".into()) });
    lid(&c, true);
    assert!(output(&c, "eDP-1").powered, "the mirror's picture went dark with the panel");
    // Mirroring no more, it goes dark.
    c.ok(Request::MirrorOutput { name: "HEADLESS-2".into(), source: None });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while output(&c, "eDP-1").powered {
        assert!(std::time::Instant::now() < deadline, "the panel stayed on behind the lid");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(output(&c, "HEADLESS-2").powered);
}
