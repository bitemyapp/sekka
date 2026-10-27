//! What Sidestep's fast paths ask of the compositor: wl_shm in ABGR and
//! XBGR, shm buffers released once their pixels are uploaded (GLES), and
//! drag and drop with an icon surface that moves by wl_surface.offset and
//! data source actions (version 3).

mod common;

use std::io::{Read, Write};
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use aizome_ipc::{Request, Response, Synthetic};
use common::*;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_data_device, wl_data_device_manager, wl_data_offer, wl_data_source, wl_pointer,
    wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use wl_data_device_manager::DndAction;

/// A bare client: one toplevel, its pointer and data device, and what it
/// heard.
#[derive(Default)]
struct Probe {
    formats: Vec<wl_shm::Format>,
    configured: bool,
    button_serial: Option<u32>,
    // As a drag target.
    offer: Option<wl_data_offer::WlDataOffer>,
    offer_mimes: Vec<String>,
    entered: Option<u32>,
    source_actions: Option<DndAction>,
    dropped: bool,
    // As a drag source.
    target_mime: Option<Option<String>>,
    chosen_action: Option<DndAction>,
    send_requests: Vec<(String, OwnedFd)>,
    drop_performed: bool,
    finished: bool,
}

struct Client {
    conn: Connection,
    queue: EventQueue<Probe>,
    qh: QueueHandle<Probe>,
    globals: GlobalList,
    state: Probe,
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    seat: wl_seat::WlSeat,
    surface: wl_surface::WlSurface,
    _toplevel: (xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel),
}

impl Client {
    fn new(c: &Compositor, app_id: &str) -> Client {
        let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
        let (globals, mut queue) = registry_queue_init::<Probe>(&conn).unwrap();
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 5..=6, ()).unwrap();
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
        let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 5..=9, ()).unwrap();
        seat.get_pointer(&qh, ());
        let surface = compositor.create_surface(&qh, ());
        let xs = wm.get_xdg_surface(&surface, &qh, ());
        let tl = xs.get_toplevel(&qh, ());
        tl.set_app_id(app_id.into());
        surface.commit();
        let mut state = Probe::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !state.configured {
            queue.roundtrip(&mut state).unwrap();
            assert!(Instant::now() < deadline, "no configure");
        }
        let mut client =
            Client { conn, queue, qh, globals, state, compositor, shm, seat, surface, _toplevel: (xs, tl) };
        let b = client.buffer(300, 200, 0x2266cc, None);
        client.surface.attach(Some(&b), 0, 0);
        client.surface.commit();
        client.roundtrip();
        c.wait_window(app_id);
        client
    }

    fn roundtrip(&mut self) {
        self.queue.roundtrip(&mut self.state).unwrap();
    }

    /// Dispatches until `f` holds.
    fn wait(&mut self, what: &str, f: impl Fn(&Probe) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !f(&self.state) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: target {:?}, action {:?}, source actions {:?}, entered {:?}",
                self.state.target_mime,
                self.state.chosen_action,
                self.state.source_actions,
                self.state.entered
            );
            self.roundtrip();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A solid ARGB8888 buffer; `released` turns true when the compositor
    /// releases it.
    fn buffer(&self, w: i32, h: i32, color: u32, released: Option<Arc<AtomicBool>>) -> wl_buffer::WlBuffer {
        self.buffer_in(wl_shm::Format::Argb8888, w, h, color, None, released)
    }

    /// A buffer in a format: `color` (0xRRGGBB) everywhere, or only in
    /// `patch` (x, y, w, h) over 0x808080.
    fn buffer_in(
        &self,
        format: wl_shm::Format,
        w: i32,
        h: i32,
        color: u32,
        patch: Option<(i32, i32, i32, i32)>,
        released: Option<Arc<AtomicBool>>,
    ) -> wl_buffer::WlBuffer {
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-probe".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        let bytes = |c: u32| -> [u8; 4] {
            let (r, g, b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
            match format {
                // Little-endian words: ARGB is B, G, R, A in memory; ABGR is R, G, B, A.
                wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => [r, g, b, 0xff],
                _ => [b, g, r, 0xff],
            }
        };
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let inside = patch.is_none_or(|(px, py, pw, ph)| x >= px && x < px + pw && y >= py && y < py + ph);
                data.extend_from_slice(&bytes(if inside { color } else { 0x808080 }));
            }
        }
        file.write_all(&data).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), w * h * 4, &self.qh, ());
        let b = pool.create_buffer(0, w, h, w * 4, format, &self.qh, released.unwrap_or_default());
        pool.destroy();
        b
    }
}

fn synth(c: &Compositor, events: Vec<Synthetic>) {
    c.ok(Request::Synthesize { events });
}

fn renderer(c: &Compositor) -> String {
    match c.request(Request::Stats { reset: false }) {
        Response::Stats { stats } => stats.renderer,
        r => panic!("{r:?}"),
    }
}

fn shm_formats_and_release(renderer_name: &str) {
    let c = Compositor::start(Options { renderer: Some(renderer_name), ..Default::default() });
    let mut a = Client::new(&c, "org.example.probe");
    assert!(a.state.formats.contains(&wl_shm::Format::Abgr8888), "ABGR8888 offered: {:?}", a.state.formats);
    assert!(a.state.formats.contains(&wl_shm::Format::Xbgr8888), "XBGR8888 offered: {:?}", a.state.formats);
    // A new frame in a buffer of its own, and no commit after it.
    let released = Arc::new(AtomicBool::new(false));
    let b = a.buffer(300, 200, 0xcc6622, Some(released.clone()));
    a.surface.attach(Some(&b), 0, 0);
    a.surface.damage_buffer(0, 0, 300, 200);
    a.surface.commit();
    let gles = renderer(&c) == "gles";
    if gles {
        // Its pixels are in a texture: it comes back without waiting for
        // the next commit.
        a.wait("the buffer released", |_| released.load(Ordering::SeqCst));
        c.settle();
        let w = c.window("org.example.probe").unwrap();
        let img = c.screenshot();
        assert_px(&img, w.geometry.x + 150, w.geometry.y + 100, rgb(0xcc6622), "the released buffer's pixels shown");
    } else {
        // Pixman reads the client's memory while drawing: kept.
        c.settle();
        a.roundtrip();
        assert!(!released.load(Ordering::SeqCst), "pixman keeps the buffer until the next commit");
    }
}

#[test]
fn shm_buffers_in_abgr_and_released_after_upload_gles() {
    shm_formats_and_release("gles");
}

#[test]
fn shm_buffers_in_abgr_and_kept_while_drawn_pixman() {
    shm_formats_and_release("pixman");
}

#[test]
fn drag_and_drop_with_an_icon_and_actions() {
    let c = Compositor::start(Options::default());
    let mut a = Client::new(&c, "org.example.source");
    let mut b = Client::new(&c, "org.example.target");
    let ida = c.window("org.example.source").unwrap().id;
    let idb = c.window("org.example.target").unwrap().id;
    c.ok(Request::Move { id: Some(ida), x: 100, y: 200 });
    c.ok(Request::Move { id: Some(idb), x: 700, y: 200 });
    c.settle();
    let ddm_a: wl_data_device_manager::WlDataDeviceManager = a.globals.bind(&a.qh, 3..=3, ()).unwrap();
    let ddm_b: wl_data_device_manager::WlDataDeviceManager = b.globals.bind(&b.qh, 3..=3, ()).unwrap();
    let dev_a = ddm_a.get_data_device(&a.seat, &a.qh, ());
    let _dev_b = ddm_b.get_data_device(&b.seat, &b.qh, ());
    a.roundtrip();
    b.roundtrip();

    // Press on the source window.
    synth(
        &c,
        vec![
            Synthetic::PointerMotion { x: 250.0, y: 300.0 },
            Synthetic::PointerButton { button: "left".into(), pressed: true },
        ],
    );
    a.wait("the press", |s| s.button_serial.is_some());
    let serial = a.state.button_serial.unwrap();
    let source = ddm_a.create_data_source(&a.qh, ());
    source.offer("text/plain;charset=utf-8".into());
    source.set_actions(DndAction::Copy | DndAction::Move);
    let icon = a.compositor.create_surface(&a.qh, ());
    dev_a.start_drag(Some(&source), &a.surface, Some(&icon), serial);
    // The icon: 20×20 red, 10 right and 12 below the pointer.
    let ib = a.buffer(20, 20, 0xee1111, None);
    icon.attach(Some(&ib), 0, 0);
    icon.offset(10, 12);
    icon.commit();
    a.roundtrip();

    // Over the target.
    synth(&c, vec![Synthetic::PointerMotion { x: 800.0, y: 300.0 }]);
    b.wait("the drag entering", |s| s.entered.is_some() && s.offer.is_some());
    assert!(b.state.offer_mimes.iter().any(|m| m == "text/plain;charset=utf-8"), "{:?}", b.state.offer_mimes);
    let offer = b.state.offer.clone().unwrap();
    offer.accept(b.state.entered.unwrap(), Some("text/plain;charset=utf-8".into()));
    offer.set_actions(DndAction::Copy | DndAction::Move, DndAction::Move);
    b.roundtrip();
    // (smithay doesn't pass wl_data_source.target on; the action it does.)
    a.wait("the target's choice of action", |s| s.chosen_action == Some(DndAction::Move));
    b.wait("the action offered", |s| s.source_actions.is_some());
    c.settle();
    let img = c.screenshot();
    keep(&img, "protocols-drag-icon");
    assert_px(&img, 820, 322, rgb(0xee1111), "the drag icon at its offset from the pointer");

    // Drop: the target reads the data and finishes.
    synth(&c, vec![Synthetic::PointerButton { button: "left".into(), pressed: false }]);
    b.wait("the drop", |s| s.dropped);
    let mut fds = [0; 2];
    // Safety: a pipe of two fresh fds.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    // Safety: both fds are ours.
    let (read_end, write_end) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    offer.receive("text/plain;charset=utf-8".into(), write_end.as_fd());
    drop(write_end);
    b.roundtrip();
    a.wait("the send", |s| !s.send_requests.is_empty());
    let (mime, fd) = a.state.send_requests.remove(0);
    assert_eq!(mime, "text/plain;charset=utf-8");
    std::fs::File::from(fd).write_all(b"hello").unwrap();
    let mut got = String::new();
    std::fs::File::from(read_end).read_to_string(&mut got).unwrap();
    assert_eq!(got, "hello");
    offer.finish();
    b.roundtrip();
    a.wait("drop performed and finished", |s| s.drop_performed && s.finished);
    let _ = a.conn.flush();
}

// ---- dispatch ---------------------------------------------------------------

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
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
        impl Dispatch<$t, ()> for Probe {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    wl_seat::WlSeat,
    xdg_toplevel::XdgToplevel,
    wl_data_device_manager::WlDataDeviceManager,
    wayland_client::protocol::wl_callback::WlCallback
);

impl Dispatch<wl_shm::WlShm, ()> for Probe {
    fn event(p: &mut Self, _: &wl_shm::WlShm, event: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_shm::Event::Format { format: WEnum::Value(f) } = event {
            p.formats.push(f);
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for Probe {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        released: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            released.store(true, Ordering::SeqCst);
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Probe {
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

impl Dispatch<xdg_surface::XdgSurface, ()> for Probe {
    fn event(
        p: &mut Self,
        xs: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xs.ack_configure(serial);
            p.configured = true;
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for Probe {
    fn event(
        p: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_pointer::Event::Button { serial, state: WEnum::Value(wl_pointer::ButtonState::Pressed), .. } = event {
            p.button_serial = Some(serial);
        }
    }
}

impl Dispatch<wl_data_device::WlDataDevice, ()> for Probe {
    fn event(
        p: &mut Self,
        _: &wl_data_device::WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::DataOffer { id } => {
                p.offer_mimes.clear();
                p.offer = Some(id);
            }
            wl_data_device::Event::Enter { serial, id, .. } => {
                p.entered = Some(serial);
                if id.is_some() {
                    p.offer = id;
                }
            }
            wl_data_device::Event::Drop => p.dropped = true,
            _ => {}
        }
    }

    wayland_client::event_created_child!(Probe, wl_data_device::WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (wl_data_offer::WlDataOffer, ()),
    ]);
}

impl Dispatch<wl_data_offer::WlDataOffer, ()> for Probe {
    fn event(
        p: &mut Self,
        _: &wl_data_offer::WlDataOffer,
        event: wl_data_offer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_offer::Event::Offer { mime_type } => p.offer_mimes.push(mime_type),
            wl_data_offer::Event::SourceActions { source_actions: WEnum::Value(a) } => p.source_actions = Some(a),
            _ => {}
        }
    }
}

impl Dispatch<wl_data_source::WlDataSource, ()> for Probe {
    fn event(
        p: &mut Self,
        _: &wl_data_source::WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Target { mime_type } => p.target_mime = Some(mime_type),
            wl_data_source::Event::Action { dnd_action: WEnum::Value(a) } => p.chosen_action = Some(a),
            wl_data_source::Event::Send { mime_type, fd } => p.send_requests.push((mime_type, fd)),
            wl_data_source::Event::DndDropPerformed => p.drop_performed = true,
            wl_data_source::Event::DndFinished => p.finished = true,
            _ => {}
        }
    }
}

// ---- wlr-gamma-control --------------------------------------------------------

use wayland_client::protocol::wl_output;
use wayland_protocols_wlr::gamma_control::v1::client::{zwlr_gamma_control_manager_v1, zwlr_gamma_control_v1};

#[derive(Default)]
struct Gamma {
    size: Option<u32>,
    failed: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Gamma {
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

impl Dispatch<wl_output::WlOutput, ()> for Gamma {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1, ()> for Gamma {
    fn event(
        _: &mut Self,
        _: &zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
        _: zwlr_gamma_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_gamma_control_v1::ZwlrGammaControlV1, ()> for Gamma {
    fn event(
        g: &mut Self,
        _: &zwlr_gamma_control_v1::ZwlrGammaControlV1,
        event: zwlr_gamma_control_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => g.size = Some(size),
            zwlr_gamma_control_v1::Event::Failed => g.failed = true,
            _ => {}
        }
    }
}

#[test]
fn gamma_control_is_offered_and_refused_where_there_is_no_gamma() {
    let c = Compositor::start(Options::default());
    let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
    let (globals, mut queue) = registry_queue_init::<Gamma>(&conn).unwrap();
    let qh = queue.handle();
    let manager: zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1 =
        globals.bind(&qh, 1..=1, ()).expect("the gamma control global");
    let output: wl_output::WlOutput = globals.bind(&qh, 1..=4, ()).unwrap();
    let _control = manager.get_gamma_control(&output, &qh, ());
    let mut g = Gamma::default();
    queue.roundtrip(&mut g).unwrap();
    // Headless outputs have no CRTC to take ramps.
    assert!(g.failed && g.size.is_none());
}

// ---- formats, first frame and damaged frames ----------------------------------

/// Frames in a sequence of formats: the first fills the window, each later
/// one damages only a patch; the screenshot must show each in its colours.
fn frames_keep_their_colours(renderer_name: &str) {
    use wl_shm::Format::{Abgr8888, Argb8888, Xbgr8888, Xrgb8888};
    let c = Compositor::start(Options { renderer: Some(renderer_name), ..Default::default() });
    let mut a = Client::new(&c, "org.example.formats");
    let w = c.window("org.example.formats").unwrap();
    let (x0, y0) = (w.geometry.x, w.geometry.y);
    // (format, colour, patch: x, y, width, height)
    type Patch = Option<(i32, i32, i32, i32)>;
    let steps: [(wl_shm::Format, u32, Patch); 6] = [
        (Xbgr8888, 0xd02010, None),
        (Xbgr8888, 0x1040e0, Some((20, 20, 60, 40))),
        (Abgr8888, 0x20b040, Some((100, 20, 60, 40))),
        (Argb8888, 0xe0a020, Some((180, 20, 60, 40))),
        (Abgr8888, 0x9030c0, Some((20, 100, 60, 40))),
        (Xrgb8888, 0x30c0c0, Some((100, 100, 60, 40))),
    ];
    let mut drawn: Vec<(u32, Patch)> = Vec::new();
    for (format, color, patch) in steps {
        // Every buffer holds the whole picture so far (as a client's would).
        let b = {
            // Draw the history into one buffer of this format.
            let (w, h) = (300, 200);
            let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-formats".as_ptr(), 0)) };
            let mut file = std::fs::File::from(fd);
            let mut layers = drawn.clone();
            layers.push((color, patch));
            let mut data = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let mut c = 0x808080;
                    for (lc, lp) in &layers {
                        if lp.is_none_or(|(px, py, pw, ph)| x >= px && x < px + pw && y >= py && y < py + ph) {
                            c = *lc;
                        }
                    }
                    let (r, g, b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
                    data.extend_from_slice(&match format {
                        Abgr8888 | Xbgr8888 => [r, g, b, 0xff],
                        _ => [b, g, r, 0xff],
                    });
                }
            }
            file.write_all(&data).unwrap();
            let pool = a.shm.create_pool(file.as_fd(), w * h * 4, &a.qh, ());
            let buf = pool.create_buffer(0, w, h, w * 4, format, &a.qh, Arc::default());
            pool.destroy();
            buf
        };
        a.surface.attach(Some(&b), 0, 0);
        match patch {
            Some((x, y, w, h)) => a.surface.damage_buffer(x, y, w, h),
            None => a.surface.damage_buffer(0, 0, 300, 200),
        }
        a.surface.commit();
        a.roundtrip();
        c.settle();
        drawn.push((color, patch));
        let img = c.screenshot();
        for (color, patch) in &drawn {
            let (px, py) = match patch {
                Some((x, y, w, h)) => (x + w / 2, y + h / 2),
                None => (270, 180),
            };
            assert_px(
                &img,
                x0 + px,
                y0 + py,
                rgb(*color),
                &format!("{color:06x} after a {format:?} frame ({renderer_name})"),
            );
        }
    }
}

#[test]
fn every_shm_format_keeps_its_colours_gles() {
    frames_keep_their_colours("gles");
}

#[test]
fn every_shm_format_keeps_its_colours_pixman() {
    frames_keep_their_colours("pixman");
}
