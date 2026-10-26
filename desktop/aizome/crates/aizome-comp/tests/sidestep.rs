//! What Aizome's shell and apps (sidestep-ui programs) need of the
//! compositor, checked with a sidestep-ui client (examples/sidestep_client):
//! the required globals, server-side decorations, the app id, frame
//! callbacks on the content subsurface, layer-shell exclusive zones and the
//! session lock.

mod common;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aizome_ipc::{Request, Response};
use common::*;

const GREEN: u32 = 0x26a269;
const MAGENTA: u32 = 0xc0268a;
const BAR: u32 = 0x1d2b53;
const LOCK: u32 = 0x7e2553;

struct Program(Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn client_binary() -> PathBuf {
    // target/<profile>/deps/sidestep-… → target/<profile>/examples/
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().join("examples").join("sidestep_client")
}

fn run(c: &Compositor, mode: &str) -> Program {
    let bin = client_binary();
    assert!(bin.exists(), "build the example first: {}", bin.display());
    let child = Command::new(bin)
        .arg(mode)
        .env("WAYLAND_DISPLAY", &c.socket)
        .env("XDG_RUNTIME_DIR", &c.dir)
        .env_remove("DISPLAY")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(c.dir.join(format!("client-{mode}.log"))).unwrap())
        .spawn()
        .unwrap();
    Program(child)
}

fn wait_px(c: &Compositor, x: i32, y: i32, want: u32, what: &str) -> Image {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let img = c.screenshot();
        if near(img.px(x, y), rgb(want)) {
            return img;
        }
        if Instant::now() > deadline {
            let p = c.dir.join("failed.png");
            img.save(&p);
            panic!("{what}: ({x}, {y}) is {:?}, not #{want:06x} (see {})\n{}", img.px(x, y), p.display(), c.log_text());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_sidestep_window_is_decorated_by_the_compositor_and_animates() {
    let c = Compositor::start(Options::default());
    let _p = run(&c, "toplevel");
    let w = c.wait_window("org.aizome.test-client");
    assert!(!w.client_side_decorations, "it asked for and got server-side decorations");
    assert!(w.kit_app, "Aizome's own apps keep ❖");
    assert_eq!(w.title, "Sidestep test");
    let g = w.geometry;
    assert_eq!((g.w, g.h), (320, 200));
    // Content from the geometry's top left: no client decorations.
    wait_px(&c, g.x + 1, g.y + 1, GREEN, "the content's top left");
    wait_px(&c, g.x + g.w - 2, g.y + g.h - 2, GREEN, "the content's bottom right");
    // The second frame comes only through frame callbacks on the content
    // subsurface.
    wait_px(&c, g.x + 100, g.y + 100, MAGENTA, "redrawn after a frame callback");
    // And the tab is ours.
    let img = c.screenshot();
    assert_px(&img, g.x + 2, g.y - 14, rgb(0xe0af68), "Aizome's tab");
}

#[test]
fn a_panel_reserves_its_exclusive_zone() {
    let c = Compositor::start(Options::default());
    let _p = run(&c, "panel");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let Response::Outputs { outputs } = c.request(Request::Outputs) else { panic!() };
        if outputs[0].usable.y == 30 {
            assert_eq!(outputs[0].usable.h, 800 - 30);
            break;
        }
        assert!(Instant::now() < deadline, "no exclusive zone: {outputs:?}\n{}", c.log_text());
        std::thread::sleep(Duration::from_millis(50));
    }
    // The panel spans the output, 30pt tall, configured to the real width.
    wait_px(&c, 5, 5, BAR, "the panel's left");
    wait_px(&c, 1275, 25, BAR, "the panel's right");
    let img = c.screenshot();
    assert_px(&img, 640, 31, rgb(0x16161e), "under the panel");
    // Windows open below it, tab and all.
    let _a = c.client(Spec::new("test.a", 0xcc2222).size(600, 700));
    let w = c.wait_window("test.a");
    assert!(w.geometry.y - 25 >= 30, "the tab clears the panel: {:?}", w.geometry);
    // Fill stays out of it too.
    c.ok(Request::Fill { id: Some(w.id), state: Some(true) });
    let ws = c.wait_windows("filled", |ws| ws[0].fill);
    assert_eq!(ws[0].geometry.y, 30 + 25);
}

#[test]
fn a_see_through_surface_shows_what_is_under_it() {
    // A layer surface with a transparent background (a menu's shadow
    // corners, the Full effects' frost) shows the desktop where it draws
    // nothing, and blends where it draws translucently.
    let c = Compositor::start(Options::default());
    let _p = run(&c, "glass");
    wait_px(&c, 150, 150, BAR, "the solid half");
    let img = c.screenshot();
    assert_px(&img, 250, 120, rgb(0x16161e), "the desktop through the undrawn part");
    let (bar, desk) = (rgb(BAR), rgb(0x16161e));
    let half: [u8; 3] = std::array::from_fn(|i| ((bar[i] as u16 + desk[i] as u16) / 2) as u8);
    assert!(near(img.px(250, 180), half), "half the bar over the desktop: {:?}, not {half:?}", img.px(250, 180));
}

#[test]
fn a_sidestep_lock_screen_covers_everything_and_holds() {
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let p = run(&c, "lock");
    // Locked: the lock surface covers everything.
    let g = w.geometry;
    wait_px(&c, g.x + 10, g.y + 10, LOCK, "the lock surface over the window");
    wait_px(&c, 2, 2, LOCK, "the lock surface over the desktop");
    let saw_lock = events.by_ref().any(|e| matches!(e, Ok(aizome_ipc::Event::LockStateChanged { locked: true })));
    assert!(saw_lock);
    // A locker that dies leaves the session locked: nothing shows.
    drop(p);
    std::thread::sleep(Duration::from_millis(300));
    c.settle();
    let img = c.screenshot();
    assert_px(&img, g.x + 10, g.y + 10, [0, 0, 0], "still locked, blank");
}

#[test]
fn the_session_locks_and_unlocks() {
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("test.a", 0xcc2222));
    let w = c.wait_window("test.a");
    let lock = LockClient::spawn(&c, LOCK);
    let g = w.geometry;
    wait_px(&c, g.x + 10, g.y + 10, LOCK, "the lock surface");
    assert_eq!(*lock.locked.lock().unwrap(), Some(true), "locked was sent");
    // Clicks and keys go to the lock surface, not the window.
    c.ok(Request::Synthesize {
        events: vec![
            aizome_ipc::Synthetic::PointerMotion { x: g.x as f64 + 10.0, y: g.y as f64 + 10.0 },
            aizome_ipc::Synthetic::Click { button: "left".into() },
        ],
    });
    lock.unlock();
    wait_px(&c, g.x + 10, g.y + 10, 0xcc2222, "the window shows again");
}

/// The frosted panel's face (`sidestep_client frost`), at 90% alpha.
const FACE: u32 = 0x24283b;

#[test]
fn frosted_shell_surfaces_blur_what_is_behind_them_under_gles_in_full() {
    let c = Compositor::start(Options {
        renderer: Some("gles"),
        settings: "[appearance]\neffects = \"full\"\n",
        ..Options::default()
    });
    if let Response::Stats { stats } = c.request(Request::Stats { reset: false })
        && stats.renderer != "gles"
    {
        eprintln!("no GLES here ({}): nothing to blur", stats.renderer);
        return;
    }
    // A red window whose left edge runs under the panel (200–500 × 120–520)
    // and on below it.
    let _a = c.client(Spec::new("test.a", 0xcc2222).size(300, 400));
    let w = c.wait_window("test.a");
    c.ok(Request::Move { id: Some(w.id), x: 350, y: 250 });
    let _p = run(&c, "frost");
    let face = rgb(FACE);
    let desk = rgb(0x16161e);
    let over_desk: [u8; 3] = std::array::from_fn(|i| (face[i] as f32 * 0.9 + desk[i] as f32 * 0.1).round() as u8);
    wait_px(&c, 260, 160, u32::from_be_bytes([0, over_desk[0], over_desk[1], over_desk[2]]), "the panel");
    c.settle();
    let full = c.screenshot();
    keep(&full, "frost-full-gles");
    // The same in Lean, where nothing is blurred.
    std::fs::write(c.dir.join("config/aizome/settings.toml"), "[appearance]\neffects = \"lean\"\n").unwrap();
    c.ok(Request::ReloadConfig);
    std::thread::sleep(Duration::from_millis(300));
    c.settle();
    let lean = c.screenshot();
    let diff = |x: i32, y: i32| -> i32 {
        let (a, b) = (full.px(x, y), lean.px(x, y));
        (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).max().unwrap()
    };
    // Across the window's edge under the face, the blur softens the step a
    // tenth of the window shows with.
    let edge = 350 - 1;
    assert!(
        diff(edge - 3, 380) > 1 || diff(edge + 3, 380) > 1,
        "a blurred edge: {:?} vs {:?}",
        full.px(edge + 3, 380),
        lean.px(edge + 3, 380)
    );
    // Far from any edge, under the face, the blur of a flat colour is it.
    assert!(diff(260, 160) <= 2, "flat backdrop: {:?} vs {:?}", full.px(260, 160), lean.px(260, 160));
    assert!(diff(450, 400) <= 2, "flat window: {:?} vs {:?}", full.px(450, 400), lean.px(450, 400));
    // The hard shadow and everything off the panel are as they were.
    assert_eq!(full.px(501, 380), lean.px(501, 380), "the shadow, beside the face");
    // Nor does the blur spill past the face: the edge below the panel is
    // as sharp as ever.
    for x in [edge - 3, edge, edge + 3] {
        assert_eq!(full.px(x, 540), lean.px(x, 540), "the window's edge below the panel, at {x}");
    }
    assert_eq!(full.px(600, 380), lean.px(600, 380), "the window, off the panel");
    assert_eq!(full.px(100, 100), lean.px(100, 100), "the desktop, off the panel");
}
