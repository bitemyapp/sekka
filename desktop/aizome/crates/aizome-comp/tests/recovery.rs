//! The recovery overlay (Ctrl+Alt+Del) and the input it rescues: it opens
//! over a full-screen client that inhibits shortcuts, force quits an app's
//! process, offers only safe actions over the lock screen, sees an app
//! that stops answering pings, and resets stuck input. Also the drag that
//! used to stick: a grab ends on its button's release whatever came in
//! between, and a slow client can't start one late.

mod common;

use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aizome_ipc::{InputState, RecoveryState, Request, Response, Synthetic};
use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1, zwp_keyboard_shortcuts_inhibitor_v1,
};
use wayland_protocols::wp::pointer_constraints::zv1::client::{zwp_locked_pointer_v1, zwp_pointer_constraints_v1};
use wayland_protocols::xdg::decoration::zv1::client::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

// ---- helpers ------------------------------------------------------------------

fn synth(c: &Compositor, events: Vec<Synthetic>) {
    c.ok(Request::Synthesize { events });
}

fn chord(c: &Compositor, keys: &str) {
    synth(c, vec![Synthetic::Chord { keys: keys.into() }]);
}

fn motion(c: &Compositor, x: i32, y: i32) {
    synth(c, vec![Synthetic::PointerMotion { x: x as f64, y: y as f64 }]);
}

fn button(c: &Compositor, button: &str, pressed: bool) {
    synth(c, vec![Synthetic::PointerButton { button: button.into(), pressed }]);
}

fn recovery(c: &Compositor) -> RecoveryState {
    match c.request(Request::RecoveryState) {
        Response::Recovery { state } => state,
        r => panic!("{r:?}"),
    }
}

fn input(c: &Compositor) -> InputState {
    match c.request(Request::InputState) {
        Response::InputState { state } => state,
        r => panic!("{r:?}"),
    }
}

fn wait<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(t) = f() {
            return t;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Where to grab a window by its tab (the tab sits above the content's
/// top-left corner).
fn tab_of(w: &aizome_ipc::WindowInfo) -> (i32, i32) {
    (w.geometry.x + 24, w.geometry.y - 11)
}

// ---- tests ----------------------------------------------------------------------

#[test]
fn ctrl_alt_del_opens_over_a_full_screen_client_that_inhibits_shortcuts() {
    let c = Compositor::start(Options::default());
    let game = Probe::spawn(&c, ProbeSpec::new("org.example.Game", 0xff00ff).inhibit());
    let w = c.wait_window("org.example.Game");
    c.ok(Request::Fullscreen { id: Some(w.id), state: Some(true) });
    game.wait("full screen, with the keys and shortcuts inhibited", |s| {
        s.fullscreen && s.keyboard_focus && s.inhibited
    });
    // Shortcuts go to the game: ❖Tab is its Tab, not the switcher.
    chord(&c, "super+tab");
    game.wait("Tab, inhibited", |s| s.keys.contains(&15));

    // Ctrl+Alt+Del isn't inhibited.
    chord(&c, "ctrl+alt+delete");
    let st = recovery(&c);
    assert!(st.open && !st.locked, "{st:?}");
    assert_eq!(st.selected.as_deref(), Some("org.example.Game"));
    assert!(st.apps.iter().any(|a| a.app_id == "org.example.Game" && a.pids == vec![std::process::id() as i32]));
    game.wait("the keyboard leaving the game", |s| !s.keyboard_focus);
    let keys = game.seen().keys.len();

    // Drawn above the full-screen surface: the title strip on top, and
    // instead of the game the desktop's colour (nothing below is drawn).
    let panel = st.panel.expect("the panel's place");
    c.settle();
    let img = c.screenshot();
    keep(&img, "recovery-over-fullscreen");
    assert_px(&img, panel.x + panel.w / 2, panel.y + 6, rgb(0x0b0c12), "the title strip");
    assert_px(&img, 20, 20, rgb(0x16161e), "the backdrop over the game");

    // Keys are the overlay's while it shows.
    tap(&c, "down");
    tap(&c, "z");
    c.settle();
    assert_eq!(game.seen().keys.len(), keys, "keys reached the game under the overlay");

    // Escape closes it and the game has the keys again.
    tap(&c, "escape");
    assert!(!recovery(&c).open);
    game.wait("the keyboard back", |s| s.keyboard_focus);
    tap(&c, "b");
    game.wait("B in the game", |s| s.keys.contains(&48));
}

#[test]
fn ctrl_alt_del_toggles_and_a_held_key_is_let_go() {
    let c = Compositor::start(Options::default());
    let app = Probe::spawn(&c, ProbeSpec::new("org.example.Editor", 0x2266aa));
    c.wait_window("org.example.Editor");
    app.wait("keyboard focus", |s| s.keyboard_focus);
    // A key held down when the overlay opens: the app hears leave, so
    // it doesn't repeat it for ever.
    synth(&c, vec![Synthetic::Key { key: "j".into(), pressed: true }]);
    app.wait("J", |s| s.keys.contains(&36));
    chord(&c, "ctrl+alt+delete");
    app.wait("leave", |s| !s.keyboard_focus);
    synth(&c, vec![Synthetic::Key { key: "j".into(), pressed: false }]);
    chord(&c, "ctrl+alt+delete");
    assert!(!recovery(&c).open);
    app.wait("the keys back", |s| s.keyboard_focus);
    assert!(input(&c).pressed_keys.is_empty(), "{:?}", input(&c));
}

#[test]
fn force_quit_kills_the_apps_process() {
    let c = Compositor::start(Options::default());
    let mut child = spawn_child_client(&c, "org.example.Victim");
    let w = c.wait_window("org.example.Victim");
    assert_eq!(w.pid, Some(child.0.id() as i32), "the window is the child's");
    chord(&c, "ctrl+alt+delete");
    assert_eq!(recovery(&c).selected.as_deref(), Some("org.example.Victim"));
    tap(&c, "delete");
    let status = wait("the child to die", 10, || child.0.try_wait().ok().flatten());
    assert_eq!(status.signal(), Some(libc::SIGKILL), "{status:?}");
    c.wait_windows("its window gone", |ws| ws.is_empty());
    let st = recovery(&c);
    assert!(st.open);
    assert!(st.message.as_deref().is_some_and(|m| m.starts_with("Force quit")), "{st:?}");
    // Its row goes with its last window, not at the overlay's next tick.
    assert!(!st.apps.iter().any(|a| a.app_id == "org.example.Victim"), "{st:?}");
    // The compositor itself, and the test running it, are fine.
    assert!(matches!(c.request(Request::Version), Response::Version { .. }));
}

#[test]
fn force_quit_never_kills_the_compositors_parent() {
    // These test clients are threads of this test, the compositor's
    // parent: refused.
    let c = Compositor::start(Options::default());
    let _app = Probe::spawn(&c, ProbeSpec::new("org.example.Mine", 0x22aa66));
    c.wait_window("org.example.Mine");
    match c.request(Request::RecoveryAction { action: "force-quit".into(), app_id: Some("org.example.Mine".into()) }) {
        Response::Message { .. } => {}
        r => panic!("{r:?}"),
    }
    let st = wait("the answer", 5, || {
        let st = recovery(&c);
        st.message.clone().filter(|m| m.starts_with("Couldn't")).map(|_| st)
    });
    assert!(st.message.unwrap().contains("Aizome's own"));
    assert!(c.window("org.example.Mine").is_some());
}

#[test]
fn a_frozen_app_shows_as_not_responding() {
    let c = Compositor::start(Options::default());
    let _ok = Probe::spawn(&c, ProbeSpec::new("org.example.Fine", 0x22aa66));
    let frozen = Probe::spawn(&c, ProbeSpec::new("org.example.Frozen", 0xaa2266));
    c.wait_windows("both", |ws| ws.len() == 2);
    frozen.send(PCmd::Freeze);
    chord(&c, "ctrl+alt+delete");
    let st = wait("Not responding", 10, || {
        let st = recovery(&c);
        st.apps.iter().any(|a| a.app_id == "org.example.Frozen" && a.not_responding).then_some(st)
    });
    // In trouble: listed first.
    assert_eq!(st.apps[0].app_id, "org.example.Frozen");
    assert!(st.apps.iter().any(|a| a.app_id == "org.example.Fine" && !a.not_responding));
    c.settle();
    keep(&c.screenshot(), "recovery-not-responding");
    frozen.send(PCmd::Thaw);
    wait("answering again", 10, || {
        recovery(&c).apps.iter().any(|a| a.app_id == "org.example.Frozen" && !a.not_responding).then_some(())
    });
}

#[test]
fn only_safe_actions_over_the_lock_screen() {
    let c = Compositor::start(Options::default());
    let _app = Probe::spawn(&c, ProbeSpec::new("org.example.Private", 0x22aa66));
    c.wait_window("org.example.Private");
    let lock = LockClient::spawn(&c, 0x3a0b2c);
    wait("locked", 10, || (*lock.locked.lock().unwrap() == Some(true)).then_some(()));
    chord(&c, "ctrl+alt+delete");
    let st = recovery(&c);
    assert!(st.open && st.locked, "{st:?}");
    assert!(st.apps.is_empty(), "apps (and their titles) aren't shown over the lock");
    assert_eq!(st.actions, ["reset-input", "restart-shell", "text-console"]);
    let refused =
        c.request(Request::RecoveryAction { action: "force-quit".into(), app_id: Some("org.example.Private".into()) });
    assert!(matches!(refused, Response::Error { .. }), "{refused:?}");
    // Delete does nothing here.
    tap(&c, "delete");
    c.settle();
    assert!(c.window("org.example.Private").is_some());
    let panel = recovery(&c).panel.unwrap();
    let img = c.screenshot();
    keep(&img, "recovery-locked");
    assert_px(&img, panel.x + panel.w / 2, panel.y + 6, rgb(0x0b0c12), "the title strip over the lock");
    tap(&c, "escape");
    assert!(!recovery(&c).open);
    lock.unlock();
}

#[test]
fn reset_input_ends_a_stuck_grab_and_frees_a_locked_pointer() {
    let c = Compositor::start(Options::default());
    let app = Probe::spawn(&c, ProbeSpec::new("org.example.Stuck", 0x3366ff));
    let w = c.wait_window("org.example.Stuck");
    let (cx, cy) = (w.geometry.x + w.geometry.w / 2, w.geometry.y + w.geometry.h / 2);

    // A button held in the app (its implicit grab) and a key held.
    motion(&c, cx, cy);
    button(&c, "left", true);
    synth(&c, vec![Synthetic::Key { key: "k".into(), pressed: true }]);
    let st = input(&c);
    assert!(st.pointer_grab.as_deref().is_some_and(|g| g.contains("org.example.Stuck")), "{st:?}");
    assert_eq!(st.pressed_buttons, [0x110]);
    assert_eq!(st.pressed_keys, [37]);
    let r = c.request(Request::RecoveryAction { action: "reset-input".into(), app_id: None });
    assert!(matches!(&r, Response::Message { message } if message.contains("pointer grab")), "{r:?}");
    let st = input(&c);
    assert_eq!(st.pointer_grab, None, "{st:?}");
    assert!(st.pressed_buttons.is_empty() && st.pressed_keys.is_empty(), "{st:?}");
    // The real release comes later and changes nothing.
    button(&c, "left", false);
    synth(&c, vec![Synthetic::Key { key: "k".into(), pressed: false }]);

    // A locked pointer: free after a reset.
    app.send(PCmd::LockPointer);
    motion(&c, cx + 5, cy + 5);
    app.wait("locked", |s| s.locked);
    assert_eq!(input(&c).pointer_constraint.as_deref(), Some("locked"));
    chord(&c, "ctrl+alt+delete");
    // Free while the overlay shows.
    app.wait("unlocked under the overlay", |s| !s.locked);
    tap(&c, "r");
    app.wait("unlocked", |s| !s.locked);
    assert!(!recovery(&c).open, "Reset Input closes the overlay");
    // Moving over the app doesn't lock it again until it is clicked.
    motion(&c, cx + 20, cy + 20);
    motion(&c, cx + 30, cy + 25);
    let st = input(&c);
    assert_eq!(st.pointer_constraint, None, "{st:?}");
    assert!(!app.seen().locked);
    synth(&c, vec![Synthetic::Click { button: "left".into() }]);
    motion(&c, cx + 31, cy + 25);
    app.wait("locked again after a click", |s| s.locked);
    c.request(Request::RecoveryAction { action: "reset-input".into(), app_id: None });
    app.wait("unlocked", |s| !s.locked);

    // A move held on the tab ends too.
    let (tx, ty) = tab_of(&w);
    motion(&c, tx, ty);
    button(&c, "left", true);
    motion(&c, tx + 40, ty + 30);
    assert!(input(&c).pointer_grab.is_some());
    c.request(Request::RecoveryAction { action: "reset-input".into(), app_id: None });
    let moved = c.window("org.example.Stuck").unwrap().geometry;
    motion(&c, tx + 200, ty + 200);
    assert_eq!(c.window("org.example.Stuck").unwrap().geometry, moved, "the window still followed the pointer");
    assert_eq!(input(&c).pointer_grab, None);
    button(&c, "left", false);
}

/// The drag that stuck: a button pressed in a window whose release an
/// overlay took (here the Desks overview; a client's window menu, the
/// switcher or a screenshot area do the same) left the seat holding it,
/// so no grab ended on release any more: the next drag's window followed
/// the pointer, and clicks went to the first app.
#[test]
fn a_drag_ends_on_release_after_an_overlay_took_a_release() {
    let c = Compositor::start(Options::default());
    let app = Probe::spawn(&c, ProbeSpec::new("org.aizome.Settei", 0x2a2e44));
    let other = Probe::spawn(&c, ProbeSpec::new("org.example.Other", 0x22aa66));
    c.wait_windows("both", |ws| ws.len() == 2);
    let w = c.window("org.aizome.Settei").unwrap();
    c.ok(Request::Focus { id: w.id });
    let (cx, cy) = (w.geometry.x + w.geometry.w / 2, w.geometry.y + w.geometry.h / 2);

    motion(&c, cx, cy);
    button(&c, "right", true);
    c.ok(Request::Overview { state: Some(true) });
    button(&c, "right", false);
    c.ok(Request::Overview { state: Some(false) });
    let st = input(&c);
    assert!(st.pressed_buttons.is_empty() && st.pointer_grab.is_none(), "the seat still holds a button: {st:?}");
    app.wait("its release", |s| s.buttons.last() == Some(&(0x111, false)));

    // Drag it by the tab: it follows while held and stops on release.
    let before = c.window("org.aizome.Settei").unwrap().geometry;
    let (tx, ty) = tab_of(&w);
    motion(&c, tx, ty);
    button(&c, "left", true);
    for i in 1..=6 {
        motion(&c, tx + 20 * i, ty + 10 * i);
    }
    let moved = c.window("org.aizome.Settei").unwrap().geometry;
    assert_eq!((moved.x, moved.y), (before.x + 120, before.y + 60), "the tab didn't drag");
    button(&c, "left", false);
    motion(&c, tx + 400, ty + 300);
    assert_eq!(c.window("org.aizome.Settei").unwrap().geometry, moved, "it followed after the release");
    assert_eq!(input(&c).pointer_grab, None);

    // And a click goes where it's made.
    let o = c.window("org.example.Other").unwrap();
    motion(&c, o.geometry.x + 10, o.geometry.y + 10);
    synth(&c, vec![Synthetic::Click { button: "left".into() }]);
    other.wait("the click", |s| s.buttons.contains(&(0x110, true)));
    c.wait_windows("Other focused", |ws| ws.iter().any(|w| w.app_id == "org.example.Other" && w.focused));
}

#[test]
fn a_frozen_apps_window_drags_and_lets_go() {
    let c = Compositor::start(Options::default());
    let app = Probe::spawn(&c, ProbeSpec::new("org.example.Busy", 0x2a2e44));
    let w = c.wait_window("org.example.Busy");
    app.send(PCmd::Freeze);
    std::thread::sleep(Duration::from_millis(100));
    let (tx, ty) = tab_of(&w);
    motion(&c, tx, ty);
    button(&c, "left", true);
    motion(&c, tx + 50, ty + 40);
    button(&c, "left", false);
    let moved = c.window("org.example.Busy").unwrap().geometry;
    assert_eq!((moved.x, moved.y), (w.geometry.x + 50, w.geometry.y + 40));
    motion(&c, tx + 300, ty + 300);
    assert_eq!(c.window("org.example.Busy").unwrap().geometry, moved);
    assert_eq!(input(&c).pointer_grab, None);
    app.send(PCmd::Thaw);
}

/// A client whose UI was busy (Settings waiting on D-Bus) asks to move its
/// window for a press long since released: no grab starts.
#[test]
fn a_late_move_request_starts_no_grab() {
    let c = Compositor::start(Options::default());
    let app = Probe::spawn(&c, ProbeSpec::new("org.example.Slow", 0x2a2e44).csd());
    let w = c.wait_window("org.example.Slow");
    app.send(PCmd::MoveAfterRelease);
    app.wait("armed", |s| s.armed);
    motion(&c, w.geometry.x + 30, w.geometry.y + 10);
    synth(&c, vec![Synthetic::Click { button: "left".into() }]);
    app.wait("its late move", |s| s.moved_with.is_some());
    c.settle();
    assert_eq!(input(&c).pointer_grab, None, "a late move grabbed the pointer");
    motion(&c, w.geometry.x + 300, w.geometry.y + 200);
    assert_eq!(c.window("org.example.Slow").unwrap().geometry, w.geometry);
}

#[test]
fn ipc_names_unknown_actions() {
    let c = Compositor::start(Options::default());
    let r = c.request(Request::RecoveryAction { action: "explode".into(), app_id: None });
    assert!(matches!(r, Response::Error { .. }));
    let r = c.request(Request::RecoveryAction { action: "reload-compositor".into(), app_id: None });
    // The overlay's "Reload Aizome" reloads the compositor's logic in place.
    assert!(matches!(&r, Response::Message { message } if message.contains("logic: reloaded")), "{r:?}");
    let Response::Stats { stats } = c.request(Request::Stats { reset: false }) else { panic!("no stats") };
    assert_eq!(stats.logic.reloads, 1, "{:?}", stats.logic);
    c.ok(Request::Recovery { state: Some(true) });
    assert!(recovery(&c).open);
    c.ok(Request::Recovery { state: None });
    assert!(!recovery(&c).open);
}

// ---- the app Force Quit kills, in a process of its own ------------------------------

struct ChildApp(Child);

impl Drop for ChildApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_child_client(c: &Compositor, app_id: &str) -> ChildApp {
    let exe = std::env::current_exe().unwrap();
    let child = Command::new(exe)
        .args(["--exact", "child_client", "--ignored", "--nocapture", "--test-threads=1"])
        .env("AIZOME_RECOVERY_CHILD", c.dir.join(&c.socket))
        .env("AIZOME_RECOVERY_APP", app_id)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    ChildApp(child)
}

/// Run by `force_quit_kills_the_apps_process` as the app it kills.
#[test]
#[ignore = "a helper: force_quit_kills_the_apps_process runs it in a process of its own"]
fn child_client() {
    let (Ok(socket), Ok(app)) = (std::env::var("AIZOME_RECOVERY_CHILD"), std::env::var("AIZOME_RECOVERY_APP")) else {
        return;
    };
    let _p = Probe::spawn_at(Path::new(&socket), ProbeSpec::new(&app, 0x00cc44));
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

// ---- a test client with what these tests need ---------------------------------------

#[derive(Clone)]
struct ProbeSpec {
    app_id: String,
    color: u32,
    size: (i32, i32),
    inhibit: bool,
    ssd: bool,
}

impl ProbeSpec {
    fn new(app_id: &str, color: u32) -> ProbeSpec {
        ProbeSpec { app_id: app_id.into(), color, size: (400, 300), inhibit: false, ssd: true }
    }
    fn inhibit(mut self) -> ProbeSpec {
        self.inhibit = true;
        self
    }
    fn csd(mut self) -> ProbeSpec {
        self.ssd = false;
        self
    }
}

#[derive(Clone, Debug, Default)]
struct PSeen {
    keyboard_focus: bool,
    /// Keys pressed (evdev).
    keys: Vec<u32>,
    /// Buttons: (code, pressed).
    buttons: Vec<(u32, bool)>,
    locked: bool,
    inhibited: bool,
    fullscreen: bool,
    moved_with: Option<u32>,
    /// It has taken MoveAfterRelease.
    armed: bool,
}

enum PCmd {
    /// Stop reading the connection (no pongs, no events), as a client
    /// whose thread is blocked.
    Freeze,
    Thaw,
    LockPointer,
    /// After the next press is released, ask to move with its serial.
    MoveAfterRelease,
    Quit,
}

struct Probe {
    tx: Sender<PCmd>,
    seen: Arc<Mutex<PSeen>>,
    join: Option<JoinHandle<()>>,
}

impl Probe {
    fn spawn(c: &Compositor, spec: ProbeSpec) -> Probe {
        Probe::spawn_at(&c.dir.join(&c.socket), spec)
    }

    fn spawn_at(socket: &Path, spec: ProbeSpec) -> Probe {
        let stream = UnixStream::connect(socket).expect("connect");
        let (tx, rx) = channel();
        let seen = Arc::new(Mutex::new(PSeen::default()));
        let s2 = seen.clone();
        let join = std::thread::spawn(move || run_probe(stream, spec, rx, s2));
        Probe { tx, seen, join: Some(join) }
    }

    fn send(&self, c: PCmd) {
        let _ = self.tx.send(c);
    }

    fn seen(&self) -> PSeen {
        self.seen.lock().unwrap().clone()
    }

    fn wait(&self, what: &str, f: impl Fn(&PSeen) -> bool) -> PSeen {
        wait(what, 10, || {
            let s = self.seen();
            f(&s).then_some(s)
        })
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = self.tx.send(PCmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

struct PApp {
    spec: ProbeSpec,
    seen: Arc<Mutex<PSeen>>,
    shm: wl_shm::WlShm,
    surface: wl_surface::WlSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    seat: wl_seat::WlSeat,
    pointer: Option<wl_pointer::WlPointer>,
    constraints: Option<zwp_pointer_constraints_v1::ZwpPointerConstraintsV1>,
    configured: bool,
    size: (i32, i32),
    move_after_release: bool,
    pressed_serial: Option<u32>,
    quit: bool,
}

fn run_probe(stream: UnixStream, spec: ProbeSpec, rx: Receiver<PCmd>, seen: Arc<Mutex<PSeen>>) {
    let conn = Connection::from_socket(stream).expect("connection");
    let (globals, mut queue): (_, EventQueue<PApp>) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).unwrap();
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 5..=9, ()).unwrap();
    let _keyboard = seat.get_keyboard(&qh, ());
    let pointer = seat.get_pointer(&qh, ());
    let deco: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1> =
        if spec.ssd { globals.bind(&qh, 1..=1, ()).ok() } else { None };
    let inhibit_mgr: Option<zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1> =
        globals.bind(&qh, 1..=1, ()).ok();
    let constraints: Option<zwp_pointer_constraints_v1::ZwpPointerConstraintsV1> = globals.bind(&qh, 1..=1, ()).ok();
    let surface = compositor.create_surface(&qh, ());
    let xs = wm.get_xdg_surface(&surface, &qh, ());
    let toplevel = xs.get_toplevel(&qh, ());
    toplevel.set_app_id(spec.app_id.clone());
    toplevel.set_title(spec.app_id.clone());
    if let Some(d) = &deco {
        d.get_toplevel_decoration(&toplevel, &qh, ()).set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
    }
    if spec.inhibit
        && let Some(m) = &inhibit_mgr
    {
        m.inhibit_shortcuts(&surface, &seat, &qh, ());
    }
    surface.commit();
    let mut app = PApp {
        size: spec.size,
        spec,
        seen,
        shm,
        surface,
        toplevel,
        seat,
        pointer: Some(pointer),
        constraints,
        configured: false,
        move_after_release: false,
        pressed_serial: None,
        quit: false,
    };
    let mut frozen = false;
    while !app.quit {
        for cmd in rx.try_iter() {
            match cmd {
                PCmd::Freeze => frozen = true,
                PCmd::Thaw => frozen = false,
                PCmd::LockPointer => {
                    if let (Some(c), Some(p)) = (&app.constraints, &app.pointer) {
                        c.lock_pointer(
                            &app.surface,
                            p,
                            None,
                            zwp_pointer_constraints_v1::Lifetime::Persistent,
                            &qh,
                            (),
                        );
                        app.surface.commit();
                    }
                }
                PCmd::MoveAfterRelease => {
                    app.move_after_release = true;
                    app.seen.lock().unwrap().armed = true;
                }
                PCmd::Quit => app.quit = true,
            }
        }
        if frozen {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let _ = conn.flush();
        if let Some(guard) = queue.prepare_read() {
            let fd = guard.connection_fd();
            let mut pfd = libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&fd), events: libc::POLLIN, revents: 0 };
            // Safety: one valid pollfd.
            if unsafe { libc::poll(&mut pfd, 1, 20) } > 0 {
                if guard.read().is_err() {
                    break;
                }
            } else {
                drop(guard);
            }
        }
        if queue.dispatch_pending(&mut app).is_err() {
            break;
        }
    }
    app.toplevel.destroy();
    xs.destroy();
    app.surface.destroy();
    let _ = conn.flush();
}

impl PApp {
    fn draw(&mut self, qh: &QueueHandle<PApp>) {
        let (w, h) = self.size;
        let len = (w * h * 4) as usize;
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-probe".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        let c = self.spec.color;
        let px = [c as u8, (c >> 8) as u8, (c >> 16) as u8, 0xff];
        let data: Vec<u8> = std::iter::repeat_n(px, (w * h) as usize).flatten().collect();
        std::io::Write::write_all(&mut file, &data).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
        pool.destroy();
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.damage_buffer(0, 0, w, h);
        self.surface.commit();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for PApp {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for PApp {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    wl_seat::WlSeat,
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
    zwp_pointer_constraints_v1::ZwpPointerConstraintsV1
);

impl Dispatch<wl_buffer::WlBuffer, ()> for PApp {
    fn event(
        _: &mut Self,
        b: &wl_buffer::WlBuffer,
        e: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = e {
            b.destroy();
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for PApp {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        e: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = e {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for PApp {
    fn event(
        app: &mut Self,
        xs: &xdg_surface::XdgSurface,
        e: xdg_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = e {
            xs.ack_configure(serial);
            app.configured = true;
            app.draw(qh);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for PApp {
    fn event(
        app: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        e: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            xdg_toplevel::Event::Configure { width, height, states } => {
                if width > 0 && height > 0 {
                    app.size = (width, height);
                }
                let states: Vec<u32> =
                    states.as_chunks::<4>().0.iter().map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
                app.seen.lock().unwrap().fullscreen = states.contains(&(xdg_toplevel::State::Fullscreen as u32));
            }
            xdg_toplevel::Event::Close => app.quit = true,
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for PApp {
    fn event(
        app: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        e: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut seen = app.seen.lock().unwrap();
        match e {
            wl_keyboard::Event::Enter { .. } => seen.keyboard_focus = true,
            wl_keyboard::Event::Leave { .. } => seen.keyboard_focus = false,
            wl_keyboard::Event::Key { key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } => {
                seen.keys.push(key)
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for PApp {
    fn event(
        app: &mut Self,
        _: &wl_pointer::WlPointer,
        e: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_pointer::Event::Button { serial, button, state: WEnum::Value(state), .. } = e {
            let pressed = state == wl_pointer::ButtonState::Pressed;
            app.seen.lock().unwrap().buttons.push((button, pressed));
            if pressed {
                app.pressed_serial = Some(serial);
            } else if app.move_after_release
                && let Some(s) = app.pressed_serial.take()
            {
                // As a client whose UI thread was busy: the press is
                // long released when it gets round to it.
                app.move_after_release = false;
                app.toplevel._move(&app.seat, s);
                app.seen.lock().unwrap().moved_with = Some(s);
            }
        }
    }
}

impl Dispatch<zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1, ()> for PApp {
    fn event(
        app: &mut Self,
        _: &zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1,
        e: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let active = matches!(e, zwp_keyboard_shortcuts_inhibitor_v1::Event::Active);
        app.seen.lock().unwrap().inhibited = active;
    }
}

impl Dispatch<zwp_locked_pointer_v1::ZwpLockedPointerV1, ()> for PApp {
    fn event(
        app: &mut Self,
        _: &zwp_locked_pointer_v1::ZwpLockedPointerV1,
        e: zwp_locked_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let locked = matches!(e, zwp_locked_pointer_v1::Event::Locked);
        app.seen.lock().unwrap().locked = locked;
    }
}
