//! Test harness: a headless compositor in a child process, IPC to it,
//! screenshots as pixels, and Wayland test clients (solid-colour
//! toplevels) on threads of their own.

#![allow(dead_code)]

pub mod desktop;
pub mod layer;
pub mod text;

use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aizome_ipc::{Client, Request, Response, Synthetic, WindowInfo};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::wp::text_input::zv3::client::{zwp_text_input_manager_v3, zwp_text_input_v3};
use wayland_protocols::xdg::decoration::zv1::client::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use wayland_protocols::xdg::foreign::zv2::client::{zxdg_exported_v2, zxdg_exporter_v2};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct Compositor {
    child: Child,
    pub dir: PathBuf,
    pub socket: String,
    pub ipc: PathBuf,
    pub log: PathBuf,
}

pub struct Options<'a> {
    pub outputs: &'a [&'a str],
    pub renderer: Option<&'a str>,
    /// settings.toml contents.
    pub settings: &'a str,
    /// The compositor to run (default: this package's `aizome`).
    pub exe: Option<&'a Path>,
    /// The logic library to load (default: the one beside the binary).
    pub logic: Option<&'a Path>,
    /// More arguments and environment for the compositor.
    pub args: &'a [&'a str],
    pub env: &'a [(&'a str, &'a str)],
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            outputs: &["1280x800@1"],
            renderer: None,
            settings: "[appearance]\neffects = \"austere\"\n",
            exe: None,
            logic: None,
            args: &[],
            env: &[],
        }
    }
}

impl Compositor {
    pub fn start(opts: Options<'_>) -> Compositor {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = PathBuf::from(format!("/tmp/aizome-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config/aizome")).unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(dir.join("config/aizome/settings.toml"), opts.settings).unwrap();
        let socket = format!("aztest-{}-{n}", std::process::id());
        let log = dir.join("aizome.log");
        let mut cmd = Command::new(opts.exe.unwrap_or(Path::new(env!("CARGO_BIN_EXE_aizome"))));
        cmd.args(["--backend", "headless", "--socket", &socket, "--no-session", "--no-xwayland"]);
        if !opts.args.contains(&"--shell") {
            cmd.arg("--no-shell");
        }
        cmd.args(opts.args.iter().filter(|a| **a != "--shell"));
        if let Some(l) = opts.logic {
            cmd.arg("--logic").arg(l);
        }
        if let Some(exe) = opts.exe {
            // Its own core before the one `cargo test` built (cargo's
            // LD_LIBRARY_PATH beats the binary's rpath, and the tests'
            // dev-dependencies can resolve the core's dependencies with
            // other features, so its symbols differ).
            let resolved = exe.canonicalize().expect("resolve the compositor binary");
            let dir = resolved.parent().unwrap();
            let mut paths = vec![dir.to_path_buf(), dir.join("deps"), dir.join("../lib/aizome")];
            paths.extend(std::env::split_paths(&std::env::var_os("LD_LIBRARY_PATH").unwrap_or_default()));
            cmd.env("LD_LIBRARY_PATH", std::env::join_paths(paths).unwrap());
        }
        cmd.envs(opts.env.iter().copied());
        for o in opts.outputs {
            cmd.args(["--headless-output", o]);
        }
        if let Some(r) = opts.renderer {
            cmd.args(["--renderer", r]);
        }
        cmd.env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .env_remove("DISPLAY")
            .env_remove("AIZOME_RENDERER")
            .env("XDG_RUNTIME_DIR", &dir)
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("XDG_CACHE_HOME", dir.join("cache"))
            .env("HOME", dir.join("home"))
            .env("AIZOME_LOG", "info,smithay=warn")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap());
        let child = cmd.spawn().expect("start aizome");
        let ipc = dir.join(format!("aizome.{socket}.sock"));
        let c = Compositor { child, dir, socket, ipc, log };
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Ok(mut client) = Client::connect_to(&c.ipc)
                && client.request(&Request::Version).is_ok()
            {
                return c;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the compositor didn't come up:\n{}", c.log_text());
    }

    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    pub fn request(&self, req: Request) -> Response {
        let mut c = Client::connect_to(&self.ipc).expect("IPC connect");
        c.stream().set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        c.request(&req).expect("IPC request")
    }

    /// A request that must succeed.
    pub fn ok(&self, req: Request) {
        if let Response::Error { message } = self.request(req.clone()) {
            panic!("{req:?}: {message}")
        }
    }

    pub fn windows(&self) -> Vec<WindowInfo> {
        match self.request(Request::Windows) {
            Response::Windows { windows } => windows,
            r => panic!("{r:?}"),
        }
    }

    pub fn window(&self, app_id: &str) -> Option<WindowInfo> {
        self.windows().into_iter().find(|w| w.app_id == app_id)
    }

    /// Waits for a condition on the window list.
    pub fn wait_windows(&self, what: &str, f: impl Fn(&[WindowInfo]) -> bool) -> Vec<WindowInfo> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let w = self.windows();
            if f(&w) {
                return w;
            }
            if Instant::now() > deadline {
                panic!("timed out waiting for {what}: {w:#?}\n{}", self.log_text());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    pub fn wait_window(&self, app_id: &str) -> WindowInfo {
        let ws = self.wait_windows(app_id, |ws| ws.iter().any(|w| w.app_id == app_id));
        ws.into_iter().find(|w| w.app_id == app_id).unwrap()
    }

    pub fn screenshot(&self) -> Image {
        let path = self.dir.join(format!("shot-{}.png", COUNTER.fetch_add(1, Ordering::SeqCst)));
        match self.request(Request::Screenshot {
            output: None,
            path: Some(path.to_string_lossy().into()),
            window: None,
            area: None,
            clipboard: false,
        }) {
            Response::Screenshot { .. } => {}
            r => panic!("{r:?}"),
        }
        Image::load(&path)
    }

    /// Lets frames settle: a few frame times.
    pub fn settle(&self) {
        std::thread::sleep(Duration::from_millis(120));
    }

    pub fn client(&self, spec: Spec) -> TestClient {
        TestClient::spawn(&self.dir.join(&self.socket), spec)
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        } else {
            eprintln!("compositor log and screenshots kept in {}", self.dir.display());
        }
    }
}

/// Presses a key and lets go of it, in one batch.
pub fn tap(c: &Compositor, key: &str) {
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::Key { key: key.into(), pressed: true },
            Synthetic::Key { key: key.into(), pressed: false },
        ],
    });
}

/// The middle of a window, in global points.
pub fn centre(w: &WindowInfo) -> (f64, f64) {
    ((w.geometry.x + w.geometry.w / 2) as f64, (w.geometry.y + w.geometry.h / 2) as f64)
}

/// A buffer of one opaque colour (0xRRGGBB), in shared memory.
pub fn solid_buffer<S>(shm: &wl_shm::WlShm, (w, h): (i32, i32), color: u32, qh: &QueueHandle<S>) -> wl_buffer::WlBuffer
where
    S: Dispatch<wl_shm_pool::WlShmPool, ()> + Dispatch<wl_buffer::WlBuffer, ()> + 'static,
{
    let len = (w * h * 4) as usize;
    // Safety: memfd_create with a static name.
    let fd = unsafe { libc::memfd_create(c"aizome-test".as_ptr(), 0) };
    assert!(fd >= 0);
    // Safety: we own the new fd.
    let mut file = std::fs::File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    // Copied in doubling runs: fast enough, unoptimised, for a fullscreen
    // client drawing every frame.
    let data = [color as u8, (color >> 8) as u8, (color >> 16) as u8, 0xff].repeat((w * h) as usize);
    file.write_all(&data).unwrap();
    let pool = shm.create_pool(file.as_fd(), len as i32, qh, ());
    let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, qh, ());
    pool.destroy();
    buffer
}

/// One round of a test client's loop: sends what it queued, reads what the
/// compositor sends within 20 ms, and dispatches it. False once the
/// connection is gone.
pub fn dispatch_round<S>(conn: &Connection, queue: &mut EventQueue<S>, state: &mut S) -> bool {
    let _ = conn.flush();
    if let Some(guard) = queue.prepare_read() {
        let fd = guard.connection_fd();
        let mut pfd = libc::pollfd { fd: std::os::fd::AsRawFd::as_raw_fd(&fd), events: libc::POLLIN, revents: 0 };
        // Safety: one valid pollfd.
        if unsafe { libc::poll(&mut pfd, 1, 20) } > 0 {
            if guard.read().is_err() {
                return false;
            }
        } else {
            drop(guard);
        }
    }
    queue.dispatch_pending(state).is_ok()
}

/// A screenshot's pixels.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn load(path: &Path) -> Image {
        let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::GrayscaleAlpha => {
                buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
            }
            png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
            png::ColorType::Indexed => unreachable!("expanded"),
        };
        Image { width: info.width, height: info.height, rgba }
    }

    pub fn px(&self, x: i32, y: i32) -> [u8; 3] {
        assert!(
            x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height,
            "({x}, {y}) is outside {}x{}",
            self.width,
            self.height
        );
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    pub fn save(&self, path: &Path) {
        let f = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&self.rgba).unwrap();
    }
}

pub fn rgb(hex: u32) -> [u8; 3] {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

/// Close enough (rounding in blending).
pub fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() <= 2)
}

#[track_caller]
pub fn assert_px(img: &Image, x: i32, y: i32, want: [u8; 3], what: &str) {
    let got = img.px(x, y);
    assert!(
        near(got, want),
        "{what} at ({x}, {y}): got #{:02x}{:02x}{:02x}, want #{:02x}{:02x}{:02x}",
        got[0],
        got[1],
        got[2],
        want[0],
        want[1],
        want[2]
    );
}

// ---- test clients ----------------------------------------------------------

#[derive(Clone)]
pub struct Spec {
    pub app_id: String,
    pub title: String,
    /// The solid colour it draws, 0xRRGGBB.
    pub color: u32,
    pub size: (i32, i32),
    /// Ask for server-side decorations (otherwise it never binds
    /// xdg-decoration, as GTK4 doesn't).
    pub ssd: bool,
    /// xdg_toplevel.set_min_size, if any.
    pub min_size: Option<(i32, i32)>,
    /// xdg_toplevel.set_max_size, if any.
    pub max_size: Option<(i32, i32)>,
    /// Delay each configure response to exercise commits of obsolete sizes.
    pub configure_delay: Duration,
    /// Request fullscreen before the first surface commit, optionally on an output.
    pub fullscreen: bool,
    pub fullscreen_output: Option<usize>,
    /// Draw whenever a frame callback arrives.
    pub animated: bool,
    /// Hold the first buffer until `Cmd::Map`, even after acknowledging configure.
    pub defer_mapping: bool,
    /// Take text from input methods over text-input-v3 as foot does: enabled
    /// while the window has the keys, a commit applied only by a `done`
    /// whose serial counts its own commits.
    pub text_input: bool,
}

impl Spec {
    pub fn new(app_id: &str, color: u32) -> Spec {
        Spec {
            app_id: app_id.into(),
            title: app_id.into(),
            color,
            size: (400, 300),
            ssd: true,
            min_size: None,
            max_size: None,
            configure_delay: Duration::ZERO,
            fullscreen: false,
            fullscreen_output: None,
            animated: false,
            defer_mapping: false,
            text_input: false,
        }
    }
    pub fn text_input(mut self) -> Spec {
        self.text_input = true;
        self
    }
    pub fn animated(mut self) -> Spec {
        self.animated = true;
        self
    }
    pub fn defer_mapping(mut self) -> Spec {
        self.defer_mapping = true;
        self
    }
    pub fn fullscreen(mut self, output: Option<usize>) -> Spec {
        self.fullscreen = true;
        self.fullscreen_output = output;
        self
    }
    pub fn min_size(mut self, w: i32, h: i32) -> Spec {
        self.min_size = Some((w, h));
        self
    }
    pub fn max_size(mut self, w: i32, h: i32) -> Spec {
        self.max_size = Some((w, h));
        self
    }
    pub fn size(mut self, w: i32, h: i32) -> Spec {
        self.size = (w, h);
        self
    }
    pub fn csd(mut self) -> Spec {
        self.ssd = false;
        self
    }
    pub fn title(mut self, t: &str) -> Spec {
        self.title = t.into();
        self
    }
    pub fn configure_delay(mut self, delay: Duration) -> Spec {
        self.configure_delay = delay;
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub configures: u32,
    pub frames: u32,
    pub bounds: Option<(i32, i32)>,
    pub size: (i32, i32),
    pub activated: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    /// Told it's suspended (xdg-shell 6): it needn't draw.
    pub suspended: bool,
    pub close_requested: bool,
    pub decoration_server_side: Option<bool>,
    /// Keys pressed (evdev codes) with the modifiers held then.
    pub keys: Vec<(u32, u32)>,
    pub mods: u32,
    pub keyboard_focus: bool,
    /// Vertical scrolling received, summed.
    pub scrolled: f64,
    /// Buttons: (code, pressed).
    pub buttons: Vec<(u32, bool)>,
    /// Text input: enabled (its `enter` came), the text input methods
    /// committed and it applied, and how many `done`s it dropped for a
    /// serial not its own.
    pub ime_enabled: bool,
    pub ime_text: String,
    pub ime_dropped: u32,
    /// The handle `Cmd::Export` got.
    pub exported: Option<String>,
}

pub enum Cmd {
    Map,
    /// Request a new state, then submit the first buffer using the previously
    /// acknowledged configure, before receiving the response to this request.
    FullscreenThenMap(bool, Option<usize>),
    Title(String),
    Color(u32),
    Fullscreen(bool, Option<usize>),
    /// Opens a dialog: a 200×150 toplevel of this title and colour, with
    /// the window as its parent.
    Dialog(String, u32),
    /// Exports the window (xdg-foreign v2), as an app does for a portal
    /// dialog: its handle comes in `Seen::exported`.
    Export,
    Quit,
}

pub struct TestClient {
    tx: Sender<Cmd>,
    pub seen: Arc<Mutex<Seen>>,
    join: Option<JoinHandle<()>>,
}

impl TestClient {
    fn spawn(socket: &Path, spec: Spec) -> TestClient {
        let stream = UnixStream::connect(socket).expect("connect to the compositor");
        let (tx, rx) = channel();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let seen2 = seen.clone();
        let join = std::thread::spawn(move || run_client(stream, spec, rx, seen2));
        TestClient { tx, seen, join: Some(join) }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    pub fn seen(&self) -> Seen {
        self.seen.lock().unwrap().clone()
    }

    /// Still connected: its thread ends when the compositor drops it.
    pub fn connected(&self) -> bool {
        self.join.as_ref().is_some_and(|j| !j.is_finished())
    }

    pub fn wait_seen(&self, what: &str, f: impl Fn(&Seen) -> bool) -> Seen {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = self.seen();
            if f(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {s:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

struct App {
    spec: Spec,
    seen: Arc<Mutex<Seen>>,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    surface: Option<wl_surface::WlSurface>,
    xdg_surface: Option<xdg_surface::XdgSurface>,
    toplevel: Option<xdg_toplevel::XdgToplevel>,
    dialog: Option<(wl_surface::WlSurface, xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel)>,
    exported: Option<zxdg_exported_v2::ZxdgExportedV2>,
    pending_size: Option<(i32, i32)>,
    size: (i32, i32),
    needs_draw: bool,
    configured: bool,
    quit: bool,
    /// Text input: commits made (what a `done` must say), and the string
    /// committed since the last `done`.
    ti_serial: u32,
    ti_commit: Option<String>,
}

fn run_client(stream: UnixStream, spec: Spec, rx: Receiver<Cmd>, seen: Arc<Mutex<Seen>>) {
    let conn = Connection::from_socket(stream).expect("wayland connection");
    let (globals, mut queue): (_, EventQueue<App>) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();
    let outputs: Vec<wl_output::WlOutput> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == "wl_output")
            .map(|g| globals.registry().bind(g.name, g.version.min(4), &qh, ()))
            .collect()
    });
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).expect("wl_compositor");
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).expect("xdg_wm_base");
    let seat: Option<wl_seat::WlSeat> = globals.bind(&qh, 5..=9, ()).ok();
    let _keyboard = seat.as_ref().map(|s| s.get_keyboard(&qh, ()));
    let _pointer = seat.as_ref().map(|s| s.get_pointer(&qh, ()));
    let deco: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1> =
        if spec.ssd { globals.bind(&qh, 1..=1, ()).ok() } else { None };
    let mut app = App {
        size: spec.size,
        spec: spec.clone(),
        seen,
        compositor,
        shm,
        surface: None,
        xdg_surface: None,
        toplevel: None,
        dialog: None,
        exported: None,
        pending_size: None,
        needs_draw: false,
        configured: false,
        quit: false,
        ti_serial: 0,
        ti_commit: None,
    };
    let _text_input = (spec.text_input)
        .then(|| globals.bind::<zwp_text_input_manager_v3::ZwpTextInputManagerV3, _, _>(&qh, 1..=1, ()).ok())
        .flatten()
        .zip(seat.as_ref())
        .map(|(m, s)| m.get_text_input(s, &qh, ()));
    let surface = app.compositor.create_surface(&qh, ());
    let xs = wm.get_xdg_surface(&surface, &qh, ());
    let tl = xs.get_toplevel(&qh, ());
    tl.set_app_id(spec.app_id.clone());
    tl.set_title(spec.title.clone());
    if let Some((w, h)) = spec.min_size {
        tl.set_min_size(w, h);
    }
    if let Some((w, h)) = spec.max_size {
        tl.set_max_size(w, h);
    }
    if let Some(d) = &deco {
        let td = d.get_toplevel_decoration(&tl, &qh, ());
        td.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
    }
    if spec.fullscreen {
        tl.set_fullscreen(spec.fullscreen_output.and_then(|i| outputs.get(i)));
    }
    surface.commit();
    app.surface = Some(surface);
    app.xdg_surface = Some(xs);
    app.toplevel = Some(tl);

    while !app.quit {
        for cmd in rx.try_iter() {
            match cmd {
                Cmd::Map => app.spec.defer_mapping = false,
                Cmd::Title(t) => {
                    app.toplevel.as_ref().unwrap().set_title(t);
                }
                Cmd::Color(c) => {
                    app.spec.color = c;
                    app.needs_draw = true;
                }
                Cmd::Fullscreen(on, output) | Cmd::FullscreenThenMap(on, output) => {
                    let tl = app.toplevel.as_ref().unwrap();
                    if on {
                        tl.set_fullscreen(output.and_then(|i| outputs.get(i)));
                    } else {
                        tl.unset_fullscreen();
                    }
                    if matches!(cmd, Cmd::FullscreenThenMap(..)) {
                        app.spec.defer_mapping = false;
                        app.draw(&qh);
                    }
                }
                Cmd::Dialog(title, color) => {
                    let s = app.compositor.create_surface(&qh, ());
                    let xs = wm.get_xdg_surface(&s, &qh, DialogColor(color));
                    let tl = xs.get_toplevel(&qh, DialogColor(color));
                    tl.set_parent(app.toplevel.as_ref());
                    tl.set_app_id(app.spec.app_id.clone());
                    tl.set_title(title);
                    s.commit();
                    app.dialog = Some((s, xs, tl));
                }
                Cmd::Export => {
                    let exporter: zxdg_exporter_v2::ZxdgExporterV2 =
                        globals.bind(&qh, 1..=1, ()).expect("zxdg_exporter_v2");
                    app.exported = Some(exporter.export_toplevel(app.surface.as_ref().unwrap(), &qh, ()));
                }
                Cmd::Quit => app.quit = true,
            }
        }
        if app.needs_draw && app.configured {
            app.draw(&qh);
        }
        if !dispatch_round(&conn, &mut queue, &mut app) {
            break;
        }
    }
    if let Some((s, x, t)) = app.dialog.take() {
        t.destroy();
        x.destroy();
        s.destroy();
    }
    if let Some(t) = app.toplevel.take() {
        t.destroy();
    }
    if let Some(x) = app.xdg_surface.take() {
        x.destroy();
    }
    if let Some(s) = app.surface.take() {
        s.destroy();
    }
    let _ = conn.flush();
}

impl App {
    fn draw(&mut self, qh: &QueueHandle<App>) {
        if self.spec.defer_mapping {
            return;
        }
        self.needs_draw = false;
        let s = self.surface.clone().unwrap();
        if self.spec.animated {
            s.frame(qh, ());
        }
        self.paint(qh, &s, self.size, self.spec.color);
    }

    /// Fills a surface with one colour, opaque, and commits it.
    fn paint(&self, qh: &QueueHandle<App>, s: &wl_surface::WlSurface, (w, h): (i32, i32), c: u32) {
        let buffer = solid_buffer(&self.shm, (w, h), c, qh);
        let opaque = self.compositor.create_region(qh, ());
        opaque.add(0, 0, w, h);
        s.set_opaque_region(Some(&opaque));
        opaque.destroy();
        s.attach(Some(&buffer), 0, 0);
        s.damage_buffer(0, 0, w, h);
        s.commit();
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
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
        impl Dispatch<$t, ()> for App {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_output::WlOutput,
    wayland_client::protocol::wl_region::WlRegion,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    wl_seat::WlSeat,
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zxdg_exporter_v2::ZxdgExporterV2
);

impl Dispatch<zxdg_exported_v2::ZxdgExportedV2, ()> for App {
    fn event(
        app: &mut Self,
        _: &zxdg_exported_v2::ZxdgExportedV2,
        event: zxdg_exported_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_exported_v2::Event::Handle { handle } = event {
            app.seen.lock().unwrap().exported = Some(handle);
        }
    }
}

impl Dispatch<zwp_text_input_v3::ZwpTextInputV3, ()> for App {
    fn event(
        app: &mut Self,
        ti: &zwp_text_input_v3::ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwp_text_input_v3::{ContentHint, ContentPurpose, Event};
        match event {
            // As foot: enable on enter, with a content type and cursor.
            Event::Enter { .. } => {
                ti.enable();
                ti.set_content_type(ContentHint::None, ContentPurpose::Terminal);
                ti.set_cursor_rectangle(0, 0, 8, 16);
                ti.commit();
                app.ti_serial += 1;
                app.seen.lock().unwrap().ime_enabled = true;
            }
            Event::Leave { .. } => {
                ti.disable();
                ti.commit();
                app.ti_serial += 1;
                app.seen.lock().unwrap().ime_enabled = false;
            }
            Event::CommitString { text } => app.ti_commit = text,
            Event::Done { serial } => {
                let commit = app.ti_commit.take();
                let mut seen = app.seen.lock().unwrap();
                // foot's ime.c: "IME serial mismatch" drops the lot.
                if serial != app.ti_serial {
                    seen.ime_dropped += 1;
                } else if let Some(t) = commit {
                    seen.ime_text.push_str(&t);
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        app.seen.lock().unwrap().frames += 1;
        app.needs_draw = true;
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut seen = app.seen.lock().unwrap();
        match event {
            wl_keyboard::Event::Enter { .. } => seen.keyboard_focus = true,
            wl_keyboard::Event::Leave { .. } => seen.keyboard_focus = false,
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => seen.mods = mods_depressed,
            wl_keyboard::Event::Key { key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } => {
                let m = seen.mods;
                seen.keys.push((key, m));
            }
            _ => {}
        }
    }
}

impl Dispatch<wayland_client::protocol::wl_pointer::WlPointer, ()> for App {
    fn event(
        app: &mut Self,
        _: &wayland_client::protocol::wl_pointer::WlPointer,
        event: wayland_client::protocol::wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_pointer;
        match event {
            wl_pointer::Event::Axis { axis: WEnum::Value(wl_pointer::Axis::VerticalScroll), value, .. } => {
                app.seen.lock().unwrap().scrolled += value;
            }
            wl_pointer::Event::Button { button, state: WEnum::Value(state), .. } => {
                app.seen.lock().unwrap().buttons.push((button, state == wl_pointer::ButtonState::Pressed));
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for App {
    fn event(
        _: &mut Self,
        buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            buffer.destroy();
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        app: &mut Self,
        xs: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            std::thread::sleep(app.spec.configure_delay);
            xs.ack_configure(serial);
            if let Some(s) = app.pending_size.take()
                && s.0 > 0
                && s.1 > 0
                && s != app.size
            {
                app.size = s;
                app.needs_draw = true;
            }
            if !app.configured {
                app.configured = true;
                app.needs_draw = true;
            }
            {
                let mut seen = app.seen.lock().unwrap();
                seen.configures += 1;
                seen.size = app.size;
            }
            if app.needs_draw {
                app.draw(qh);
            } else {
                app.surface.as_ref().unwrap().commit();
            }
        }
    }
}

/// A dialog's surface and toplevel, by the colour it draws.
#[derive(Clone, Copy)]
struct DialogColor(u32);

impl Dispatch<xdg_surface::XdgSurface, DialogColor> for App {
    fn event(
        app: &mut Self,
        xs: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        color: &DialogColor,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xs.ack_configure(serial);
            if let Some((s, ..)) = &app.dialog {
                app.paint(qh, s, (200, 150), color.0);
            }
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, DialogColor> for App {
    fn event(
        _: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        _: xdg_toplevel::Event,
        _: &DialogColor,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(
        app: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::ConfigureBounds { width, height } => {
                app.seen.lock().unwrap().bounds = Some((width, height));
            }
            xdg_toplevel::Event::Configure { width, height, states } => {
                app.pending_size = Some((width, height));
                let states: Vec<u32> =
                    states.as_chunks::<4>().0.iter().map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
                let mut seen = app.seen.lock().unwrap();
                seen.activated = states.contains(&(xdg_toplevel::State::Activated as u32));
                seen.maximized = states.contains(&(xdg_toplevel::State::Maximized as u32));
                seen.fullscreen = states.contains(&(xdg_toplevel::State::Fullscreen as u32));
                seen.suspended = states.contains(&(xdg_toplevel::State::Suspended as u32));
            }
            xdg_toplevel::Event::Close => {
                app.seen.lock().unwrap().close_requested = true;
                app.quit = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            app.seen.lock().unwrap().decoration_server_side =
                Some(matches!(mode, WEnum::Value(zxdg_toplevel_decoration_v1::Mode::ServerSide)));
        }
    }
}

// ---- a session-lock client ---------------------------------------------------

use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1,
};

/// Locks the session with a solid-colour lock surface on each output
/// advertised when it starts, until dropped or unlocked.
pub struct LockClient {
    tx: Sender<Cmd>,
    pub locked: Arc<Mutex<Option<bool>>>,
    join: Option<JoinHandle<()>>,
}

impl LockClient {
    pub fn spawn(c: &Compositor, color: u32) -> LockClient {
        let stream = UnixStream::connect(c.dir.join(&c.socket)).unwrap();
        let (tx, rx) = channel();
        let locked = Arc::new(Mutex::new(None));
        let l2 = locked.clone();
        let join = std::thread::spawn(move || run_lock(stream, color, rx, l2));
        LockClient { tx, locked, join: Some(join) }
    }

    /// Unlocks (unlock_and_destroy) and lets the thread end.
    pub fn unlock(mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for LockClient {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

struct Locker {
    color: u32,
    shm: wl_shm::WlShm,
    locked: Arc<Mutex<Option<bool>>>,
}

fn run_lock(stream: UnixStream, color: u32, rx: Receiver<Cmd>, locked: Arc<Mutex<Option<bool>>>) {
    let conn = Connection::from_socket(stream).unwrap();
    let (globals, mut queue): (_, EventQueue<Locker>) = registry_queue_init(&conn).unwrap();
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 4..=6, ()).unwrap();
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
    let outputs: Vec<wl_output::WlOutput> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == "wl_output")
            .map(|g| globals.registry().bind(g.name, g.version.min(4), &qh, ()))
            .collect()
    });
    let manager: ext_session_lock_manager_v1::ExtSessionLockManagerV1 = globals.bind(&qh, 1..=1, ()).unwrap();
    let lock = manager.lock(&qh, ());
    let mut state = Locker { color, shm, locked };
    // Each lock surface carries the wl_surface its configures fill.
    let _lock_surfaces: Vec<_> = outputs
        .iter()
        .map(|output| {
            let surface = compositor.create_surface(&qh, ());
            lock.get_lock_surface(&surface, output, &qh, surface.clone())
        })
        .collect();
    let mut quit = false;
    while !quit {
        if rx.try_iter().any(|c| matches!(c, Cmd::Quit)) {
            quit = true;
        }
        if !dispatch_round(&conn, &mut queue, &mut state) {
            break;
        }
    }
    if *state.locked.lock().unwrap() == Some(true) {
        lock.unlock_and_destroy();
    } else {
        lock.destroy();
    }
    let _ = conn.flush();
    let _ = conn.roundtrip();
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Locker {
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

macro_rules! ignore_locker {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Locker {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_locker!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    wl_output::WlOutput,
    wl_buffer::WlBuffer,
    ext_session_lock_manager_v1::ExtSessionLockManagerV1
);

impl Dispatch<ext_session_lock_v1::ExtSessionLockV1, ()> for Locker {
    fn event(
        s: &mut Self,
        _: &ext_session_lock_v1::ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_session_lock_v1::Event::Locked => *s.locked.lock().unwrap() = Some(true),
            ext_session_lock_v1::Event::Finished => *s.locked.lock().unwrap() = Some(false),
            _ => {}
        }
    }
}

impl Dispatch<ext_session_lock_surface_v1::ExtSessionLockSurfaceV1, wl_surface::WlSurface> for Locker {
    fn event(
        s: &mut Self,
        ls: &ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure { serial, width, height } = event {
            ls.ack_configure(serial);
            let (w, h) = (width as i32, height as i32);
            let buffer = solid_buffer(&s.shm, (w, h), s.color, qh);
            surface.attach(Some(&buffer), 0, 0);
            surface.damage_buffer(0, 0, w, h);
            surface.commit();
        }
    }
}

/// Keeps a screenshot for people to look at, in the target's tmp dir.
pub fn keep(img: &Image, name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("aizome-shots");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.png"));
    img.save(&p);
    p
}
