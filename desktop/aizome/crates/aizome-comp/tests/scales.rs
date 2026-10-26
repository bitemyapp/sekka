//! A window keeps hearing its output's scale: when the output's scale
//! changes under it, the client is told the new one through
//! wp_fractional_scale (preferred_scale) and wl_surface 6
//! (preferred_buffer_scale), so it redraws sharp rather than being
//! stretched. (They were told once, when the window was made: a 4K
//! display switched to 2× left every open app blurred.)

mod common;

use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use aizome_ipc::{Request, Response};
use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

#[derive(Default)]
struct Heard {
    configure: Option<u32>,
    /// wp_fractional_scale's preferred scale, in 120ths.
    fractional: Option<u32>,
    /// wl_surface's preferred_buffer_scale.
    buffer_scale: Option<i32>,
}

struct Client {
    conn: Connection,
    queue: EventQueue<Heard>,
    qh: QueueHandle<Heard>,
    heard: Heard,
    shm: wl_shm::WlShm,
}

impl Client {
    fn roundtrip(&mut self) {
        if let Err(e) = self.queue.roundtrip(&mut self.heard) {
            panic!("disconnected: {e} ({:?})", self.conn.protocol_error());
        }
    }

    fn wait(&mut self, what: &str, f: impl Fn(&Heard) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f(&self.heard) {
            self.roundtrip();
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what} (fractional {:?}, buffer scale {:?})",
                self.heard.fractional,
                self.heard.buffer_scale
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn buffer(&self, w: i32, h: i32) -> wl_buffer::WlBuffer {
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-scales".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        file.write_all(&[0x40, 0x60, 0x80, 0xff].repeat((w * h) as usize)).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), w * h * 4, &self.qh, ());
        let b = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &self.qh, ());
        pool.destroy();
        b
    }
}

fn set_scale(c: &Compositor, scale: f64) {
    let r = c.request(Request::OutputConfig {
        name: "HEADLESS-1".into(),
        mode: None,
        scale: Some(scale),
        position: None,
        transform: None,
        enabled: None,
    });
    assert!(!matches!(r, Response::Error { .. }), "{r:?}");
}

#[test]
fn an_open_window_hears_its_outputs_new_scale() {
    let c = Compositor::start(Options { outputs: &["1920x1080@1"], ..Default::default() });
    let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
    let (globals, queue) = registry_queue_init::<Heard>(&conn).unwrap();
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 6..=6, ()).expect("wl_compositor 6");
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
    let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
    let fractional: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1 =
        globals.bind(&qh, 1..=1, ()).expect("wp_fractional_scale_manager_v1");
    let mut a = Client { conn, queue, qh, heard: Heard::default(), shm };

    let surface = compositor.create_surface(&a.qh, ());
    let _fs = fractional.get_fractional_scale(&surface, &a.qh, ());
    let xs = wm.get_xdg_surface(&surface, &a.qh, ());
    let tl = xs.get_toplevel(&a.qh, ());
    tl.set_app_id("org.example.scales".into());
    surface.commit();
    a.wait("the first configure", |h| h.configure.is_some());
    xs.ack_configure(a.heard.configure.unwrap());
    let frame = a.buffer(400, 300);
    surface.attach(Some(&frame), 0, 0);
    surface.damage_buffer(0, 0, 400, 300);
    surface.commit();
    a.roundtrip();
    c.wait_window("org.example.scales");
    a.wait("scale 1", |h| h.fractional == Some(120));

    // The display goes to 2×, as Settings does it: the open window hears.
    set_scale(&c, 2.0);
    a.wait("scale 2", |h| h.fractional == Some(240) && h.buffer_scale == Some(2));

    // A fractional scale: 1.5 is 180/120, and buffers at 2 are sharpest.
    set_scale(&c, 1.5);
    a.wait("scale 1.5", |h| h.fractional == Some(180) && h.buffer_scale == Some(2));

    // And back.
    set_scale(&c, 1.0);
    a.wait("scale 1 again", |h| h.fractional == Some(120) && h.buffer_scale == Some(1));
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
    wl_buffer::WlBuffer,
    xdg_toplevel::XdgToplevel,
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1
);

impl Dispatch<wl_surface::WlSurface, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &wl_surface::WlSurface,
        event: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_surface::Event::PreferredBufferScale { factor } = event {
            h.buffer_scale = Some(factor);
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            h.fractional = Some(scale);
        }
    }
}

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
