//! What Chromium and Electron ask of the compositor when they open a
//! window, in their order: an xdg-toplevel-icon set on the toplevel and
//! destroyed before its buffer (smithay's own implementation disconnected
//! them for that), xdg-decoration either way, and xdg-activation with a
//! token of their own. And that xdg-toplevel-icon still refuses what the
//! protocol forbids.

mod common;

use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use common::*;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::xdg::activation::v1::client::{xdg_activation_token_v1, xdg_activation_v1};
use wayland_protocols::xdg::decoration::zv1::client::{zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_protocols::xdg::toplevel_icon::v1::client::{xdg_toplevel_icon_manager_v1, xdg_toplevel_icon_v1};

use zxdg_toplevel_decoration_v1::Mode;

#[derive(Default)]
struct Heard {
    configure: Option<u32>,
    decoration: Option<WEnum<Mode>>,
    token: Option<String>,
    icon_manager_done: bool,
}

struct Chromium {
    conn: Connection,
    queue: EventQueue<Heard>,
    qh: QueueHandle<Heard>,
    globals: GlobalList,
    heard: Heard,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    wm: xdg_wm_base::XdgWmBase,
    icons: xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1,
}

impl Chromium {
    fn connect(c: &Compositor) -> Chromium {
        let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
        let (globals, queue) = registry_queue_init::<Heard>(&conn).unwrap();
        let qh = queue.handle();
        let compositor = globals.bind(&qh, 4..=6, ()).unwrap();
        let shm = globals.bind(&qh, 1..=1, ()).unwrap();
        let wm = globals.bind(&qh, 1..=6, ()).unwrap();
        let icons = globals.bind(&qh, 1..=1, ()).expect("xdg_toplevel_icon_manager_v1 offered");
        let mut client = Chromium { conn, queue, qh, globals, heard: Heard::default(), compositor, shm, wm, icons };
        client.roundtrip().unwrap();
        assert!(client.heard.icon_manager_done, "the icon manager's sizes end with done");
        client
    }

    fn roundtrip(&mut self) -> Result<usize, wayland_client::DispatchError> {
        self.queue.roundtrip(&mut self.heard)
    }

    /// Dispatches until `f` holds; a protocol error fails the test.
    fn wait(&mut self, what: &str, f: impl Fn(&Heard) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f(&self.heard) {
            if let Err(e) = self.roundtrip() {
                panic!("disconnected waiting for {what}: {e} ({:?})", self.conn.protocol_error());
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A solid ARGB8888 shm buffer.
    fn buffer(&self, w: i32, h: i32, color: u32) -> wl_buffer::WlBuffer {
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-chromium".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        let px = [color as u8, (color >> 8) as u8, (color >> 16) as u8, 0xff];
        file.write_all(&px.repeat((w * h) as usize)).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), w * h * 4, &self.qh, ());
        let b = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &self.qh, ());
        pool.destroy();
        b
    }

    /// The protocol error the compositor ended the connection with.
    fn expect_error(&mut self, what: &str) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.roundtrip().is_ok() {
            assert!(Instant::now() < deadline, "no protocol error for {what}");
        }
        let e = self.conn.protocol_error().unwrap_or_else(|| panic!("{what}: disconnected without an error"));
        assert_eq!(e.object_interface, "xdg_toplevel_icon_v1", "{what}: {e:?}");
        e.code
    }
}

/// Opens a window the way Chromium's Wayland backend does, with the
/// decoration mode it asks for (server-side for a normal title bar,
/// client-side for Electron's `titleBarStyle: "hidden"`).
fn opens_a_window_with_an_icon(mode: Mode) {
    let c = Compositor::start(Options { outputs: &["1440x900@2"], ..Default::default() });
    let mut a = Chromium::connect(&c);
    let deco: zxdg_decoration_manager_v1::ZxdgDecorationManagerV1 = a.globals.bind(&a.qh, 1..=1, ()).unwrap();
    let activation: xdg_activation_v1::XdgActivationV1 = a.globals.bind(&a.qh, 1..=1, ()).unwrap();

    let surface = a.compositor.create_surface(&a.qh, ());
    let xs = a.wm.get_xdg_surface(&surface, &a.qh, ());
    let tl = xs.get_toplevel(&a.qh, ());
    let td = deco.get_toplevel_decoration(&tl, &a.qh, ());
    tl.set_app_id("org.example.chromium".into());
    tl.set_title("Chromium".into());
    tl.set_min_size(300, 200);
    tl.set_max_size(0, 0);
    td.set_mode(mode);
    surface.commit();
    let token = activation.get_activation_token(&a.qh, ());
    token.commit();
    xs.set_window_geometry(0, 0, 400, 300);

    // The icon: set on the toplevel, then the icon goes, then its buffer.
    let icon = a.icons.create_icon(&a.qh, ());
    let pixels = a.buffer(64, 64, 0x3366cc);
    icon.add_buffer(&pixels, 1);
    a.icons.set_icon(&tl, Some(&icon));
    icon.destroy();
    pixels.destroy();

    a.wait("the first configure", |h| h.configure.is_some());
    let serial = a.heard.configure.unwrap();
    xs.ack_configure(serial);
    let frame = a.buffer(400, 300, 0xcc6622);
    surface.attach(Some(&frame), 0, 0);
    surface.damage_buffer(0, 0, 400, 300);
    surface.commit();
    a.wait("the activation token", |h| h.token.is_some());
    activation.activate(a.heard.token.clone().unwrap(), &surface);
    token.destroy();
    a.roundtrip().expect("still connected after the icon's buffer went");

    let w = c.wait_window("org.example.chromium");
    assert_eq!(a.heard.decoration, Some(WEnum::Value(mode)), "the decoration mode it asked for");
    assert_eq!(w.client_side_decorations, mode == Mode::ClientSide, "{w:?}");
    assert!(w.focused, "a new window has the focus: {w:?}");
    assert!(!w.urgent, "a focused window asking for itself isn't flagged: {w:?}");
    assert!(!c.log_text().contains("protocol error"), "{}", c.log_text());

    // Later icons work the same.
    let icon = a.icons.create_icon(&a.qh, ());
    let pixels = a.buffer(32, 32, 0x22aa44);
    icon.add_buffer(&pixels, 2);
    icon.set_name("org.example.chromium".into());
    a.icons.set_icon(&tl, Some(&icon));
    surface.commit();
    icon.destroy();
    pixels.destroy();
    a.icons.set_icon(&tl, None);
    surface.commit();
    a.roundtrip().expect("still connected after a second icon");
    assert!(c.window("org.example.chromium").is_some());
}

#[test]
fn chromium_opens_a_window_with_an_icon_and_server_side_decorations() {
    opens_a_window_with_an_icon(Mode::ServerSide);
}

#[test]
fn chromium_opens_a_window_with_an_icon_and_its_own_title_bar() {
    opens_a_window_with_an_icon(Mode::ClientSide);
}

#[test]
fn toplevel_icons_refuse_what_the_protocol_forbids() {
    use xdg_toplevel_icon_v1::Error;
    let c = Compositor::start(Options::default());
    let toplevel = |a: &Chromium| {
        let surface = a.compositor.create_surface(&a.qh, ());
        let xs = a.wm.get_xdg_surface(&surface, &a.qh, ());
        let tl = xs.get_toplevel(&a.qh, ());
        (surface, xs, tl)
    };

    // A buffer that isn't square.
    let mut a = Chromium::connect(&c);
    let icon = a.icons.create_icon(&a.qh, ());
    icon.add_buffer(&a.buffer(32, 16, 0), 1);
    assert_eq!(a.expect_error("a buffer that isn't square"), Error::InvalidBuffer as u32);

    // Changing an icon once it's set.
    let mut a = Chromium::connect(&c);
    let (_s, _xs, tl) = toplevel(&a);
    let icon = a.icons.create_icon(&a.qh, ());
    icon.add_buffer(&a.buffer(32, 32, 0), 1);
    a.icons.set_icon(&tl, Some(&icon));
    icon.add_buffer(&a.buffer(64, 64, 0), 1);
    assert_eq!(a.expect_error("adding to an icon that's set"), Error::Immutable as u32);

    // A buffer destroyed while its icon still needs it.
    let mut a = Chromium::connect(&c);
    let (_s, _xs, tl) = toplevel(&a);
    let icon = a.icons.create_icon(&a.qh, ());
    let pixels = a.buffer(32, 32, 0);
    icon.add_buffer(&pixels, 1);
    pixels.destroy();
    a.icons.set_icon(&tl, Some(&icon));
    assert_eq!(a.expect_error("a buffer gone before its icon"), Error::NoBuffer as u32);

    // The compositor says why it disconnected them.
    let said = |log: &str| log.matches("client disconnected for a protocol error: xdg_toplevel_icon_v1").count() == 3;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !said(&c.log_text()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(said(&c.log_text()), "{}", c.log_text());
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Heard {
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
        impl Dispatch<$t, ()> for Heard {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    wl_buffer::WlBuffer,
    xdg_toplevel::XdgToplevel,
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    xdg_activation_v1::XdgActivationV1,
    xdg_toplevel_icon_v1::XdgToplevelIconV1
);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Heard {
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

impl Dispatch<xdg_surface::XdgSurface, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            h.configure = Some(serial);
        }
    }
}

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            h.decoration = Some(mode);
        }
    }
}

impl Dispatch<xdg_activation_token_v1::XdgActivationTokenV1, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &xdg_activation_token_v1::XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_activation_token_v1::Event::Done { token } = event {
            h.token = Some(token);
        }
    }
}

impl Dispatch<xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1,
        event: xdg_toplevel_icon_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel_icon_manager_v1::Event::Done = event {
            h.icon_manager_done = true;
        }
    }
}
