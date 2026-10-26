//! ext-image-copy-capture as xdg-desktop-portal-wlr 0.8 uses it for
//! browsers' screen and window sharing: a session per output or window
//! (through an ext-foreign-toplevel-list handle), its buffer constraints,
//! a first frame at once with full damage, then a frame per change for as
//! long as the stream runs (the portal's wlr-screencopy path froze after
//! the first), into wl_shm under pixman and GLES and into dmabufs under
//! GLES. Sessions follow their sources: new constraints on a resize (a
//! frame waiting with an old buffer fails), stopped when the window
//! closes.

mod common;

use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, event_created_child};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1, ext_foreign_toplevel_list_v1,
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1, ext_image_capture_source_v1,
    ext_output_image_capture_source_manager_v1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1, ext_image_copy_capture_manager_v1, ext_image_copy_capture_session_v1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1};

use ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1 as Toplevel;
use ext_image_copy_capture_frame_v1::{ExtImageCopyCaptureFrameV1 as Frame, FailureReason};
use ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1 as Session;

/// What the compositor said: a session's constraints, the frame's
/// answer, and the toplevels.
#[derive(Default)]
struct Heard {
    size: Option<(u32, u32)>,
    shm: Vec<wl_shm::Format>,
    device: Option<u64>,
    dmabuf: Vec<(u32, Vec<u64>)>,
    /// `done` events: constraint batches.
    constraints: u32,
    stopped: bool,
    transform: Option<wl_output::Transform>,
    damage: Vec<(i32, i32, i32, i32)>,
    presented: bool,
    ready: bool,
    failed: Option<FailureReason>,
    toplevels: Vec<(Toplevel, String)>,
    /// Frames answered (ready or failed).
    answered: Vec<wayland_client::backend::ObjectId>,
}

impl Heard {
    fn forget_constraints(&mut self) {
        (self.size, self.shm, self.device, self.dmabuf) = (None, Vec::new(), None, Vec::new());
    }

    fn forget_frame(&mut self) {
        (self.transform, self.damage, self.presented, self.ready, self.failed) = (None, Vec::new(), false, false, None);
    }
}

struct Capturer {
    conn: Connection,
    queue: EventQueue<Heard>,
    qh: QueueHandle<Heard>,
    heard: Heard,
    shm: wl_shm::WlShm,
    output: wl_output::WlOutput,
    manager: ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    outputs: ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
    toplevels: ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    _list: ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
    dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    /// The wl_shm buffer a capture is going into.
    target: Option<ShmTarget>,
}

/// A wl_shm buffer: its pool, the buffer, the memfd, and width and height.
type ShmTarget = (wl_shm_pool::WlShmPool, wl_buffer::WlBuffer, std::fs::File, (u32, u32));

/// A captured frame, in the client's byte order (BGRX), rows packed.
struct Picture {
    w: u32,
    h: u32,
    bgra: Vec<u8>,
}

impl Picture {
    fn px(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]]
    }
}

impl Capturer {
    fn new(c: &Compositor) -> Capturer {
        let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
        let (globals, mut queue) = registry_queue_init::<Heard>(&conn).unwrap();
        let qh = queue.handle();
        let shm = globals.bind(&qh, 1..=1, ()).unwrap();
        let output = globals.bind(&qh, 1..=4, ()).unwrap();
        let manager = globals.bind(&qh, 1..=1, ()).expect("ext-image-copy-capture");
        let outputs = globals.bind(&qh, 1..=1, ()).expect("output capture sources");
        let toplevels = globals.bind(&qh, 1..=1, ()).expect("toplevel capture sources");
        let list = globals.bind(&qh, 1..=1, ()).expect("ext-foreign-toplevel-list");
        let dmabuf = globals.bind(&qh, 3..=4, ()).ok();
        let mut heard = Heard::default();
        queue.roundtrip(&mut heard).unwrap();
        Capturer { conn, queue, qh, heard, shm, output, manager, outputs, toplevels, _list: list, dmabuf, target: None }
    }

    /// A session on a source, once its first constraints are in.
    fn session(&mut self, source: &ext_image_capture_source_v1::ExtImageCaptureSourceV1, cursor: bool) -> Session {
        use ext_image_copy_capture_manager_v1::Options;
        self.heard.forget_constraints();
        let done = self.heard.constraints;
        let options = if cursor { Options::PaintCursors } else { Options::empty() };
        let session = self.manager.create_session(source, options, &self.qh, ());
        assert!(self.wait(Duration::from_secs(5), |h| h.constraints > done || h.stopped), "no constraints");
        assert!(!self.heard.stopped, "the session stopped at once");
        session
    }

    fn output_session(&mut self) -> Session {
        let source = self.outputs.create_source(&self.output, &self.qh, ());
        self.session(&source, false)
    }

    fn window_session(&mut self, app_id: &str) -> Session {
        assert!(
            self.wait(Duration::from_secs(5), |h| h.toplevels.iter().any(|(_, a)| a == app_id)),
            "no toplevel handle for {app_id}"
        );
        let handle = self.heard.toplevels.iter().find(|(_, a)| a == app_id).unwrap().0.clone();
        let source = self.toplevels.create_source(&handle, &self.qh, ());
        self.session(&source, false)
    }

    /// Dispatches until `done` or the time is up.
    fn wait(&mut self, within: Duration, done: impl Fn(&Heard) -> bool) -> bool {
        let deadline = Instant::now() + within;
        loop {
            self.queue.dispatch_pending(&mut self.heard).unwrap();
            if done(&self.heard) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            self.conn.flush().unwrap();
            if let Some(guard) = self.queue.prepare_read() {
                let fd = guard.connection_fd();
                let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                // Safety: one valid pollfd.
                if unsafe { libc::poll(&mut pfd, 1, left.as_millis().min(100) as i32) } > 0 {
                    guard.read().unwrap();
                }
            }
        }
    }

    /// A frame capturing into a new wl_shm buffer of the session's size,
    /// all of it damaged (the buffer is kept until `finish`).
    fn start(&mut self, session: &Session) -> Frame {
        let (w, h) = self.heard.size.expect("a buffer size");
        assert!(self.heard.shm.contains(&wl_shm::Format::Xrgb8888), "XRGB8888 on offer: {:?}", self.heard.shm);
        let len = (w * 4 * h) as usize;
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-capture".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        file.write_all(&vec![0u8; len]).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), len as i32, &self.qh, ());
        let buffer = pool.create_buffer(0, w as i32, h as i32, (w * 4) as i32, wl_shm::Format::Xrgb8888, &self.qh, ());
        self.heard.forget_frame();
        let frame = session.create_frame(&self.qh, ());
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, w as i32, h as i32);
        frame.capture();
        self.target = Some((pool, buffer, file, (w, h)));
        frame
    }

    /// Waits for the frame: its picture, or `None` if nothing came in
    /// `within` (the frame still waits).
    fn finish(&mut self, frame: &Frame, within: Duration) -> Option<Picture> {
        let got = self.wait(within, |h| h.ready || h.failed.is_some());
        assert_eq!(self.heard.failed, None, "the frame failed");
        if !got {
            return None;
        }
        frame.destroy();
        assert_eq!(self.heard.transform, Some(wl_output::Transform::Normal));
        assert!(self.heard.presented, "no presentation time");
        let (pool, buffer, file, (w, h)) = self.target.take().expect("a capture sent");
        pool.destroy();
        buffer.destroy();
        let len = (w * 4 * h) as usize;
        // Safety: the client's own memfd, `len` bytes.
        let map =
            unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_SHARED, file.as_raw_fd(), 0) };
        assert_ne!(map, libc::MAP_FAILED);
        // Safety: mapped above.
        let bgra = unsafe { std::slice::from_raw_parts(map as *const u8, len) }.to_vec();
        // Safety: unmapping what we mapped.
        unsafe { libc::munmap(map, len) };
        Some(Picture { w, h, bgra })
    }

    /// Waits for a frame to fail, for `why`.
    fn fails(&mut self, frame: &Frame, why: FailureReason) {
        assert!(self.wait(Duration::from_secs(5), |h| h.ready || h.failed.is_some()), "no answer");
        assert_eq!(self.heard.failed, Some(why));
        frame.destroy();
        self.target = None;
    }
}

const BLUE: u32 = 0x3366cc;
const ORANGE: u32 = 0xdd7722;
const GREEN: u32 = 0x33aa55;

fn start(renderer: &str) -> Compositor {
    Compositor::start(Options { outputs: &["1280x800@1"], renderer: Some(renderer), ..Default::default() })
}

/// A blue window, and its content rectangle.
fn window(c: &Compositor, app_id: &str) -> (TestClient, aizome_ipc::Rect) {
    let client = c.client(Spec::new(app_id, BLUE).size(300, 200));
    let w = c.wait_window(app_id);
    c.settle();
    (client, w.geometry)
}

/// An output's stream: the first frame at once and all damaged, then one
/// per change (and none while nothing changes), with damage around the
/// change.
fn output_stream(renderer: &str) {
    let c = start(renderer);
    let (client, r) = window(&c, "org.test.capture");
    let (x, y) = ((r.x + 150) as u32, (r.y + 100) as u32);
    let mut cap = Capturer::new(&c);
    let session = cap.output_session();
    assert_eq!(cap.heard.size, Some((1280, 800)));
    assert!(cap.heard.shm.contains(&wl_shm::Format::Argb8888));
    if renderer == "pixman" {
        assert_eq!(cap.heard.device, None, "pixman offers no dmabufs");
    }
    let frame = cap.start(&session);
    let pic = cap.finish(&frame, Duration::from_secs(5)).expect("the first frame at once");
    assert_eq!((pic.w, pic.h), (1280, 800));
    assert!(near(pic.px(x, y), rgb(BLUE)), "{renderer}: the window: {:?}", pic.px(x, y));
    assert!(!near(pic.px(5, 795), rgb(BLUE)), "{renderer}: the desktop isn't the window");
    assert_eq!(cap.heard.damage, vec![(0, 0, 1280, 800)]);
    // Nothing while nothing changes, however the pointer moves (it isn't
    // painted).
    let mut waiting = Some(cap.start(&session));
    c.ok(aizome_ipc::Request::Synthesize { events: vec![aizome_ipc::Synthetic::PointerMotion { x: 5.0, y: 5.0 }] });
    assert!(
        cap.finish(waiting.as_ref().unwrap(), Duration::from_millis(400)).is_none(),
        "{renderer}: a frame without a change"
    );
    // Then a frame for every change, for as long as it streams.
    for (i, color) in [ORANGE, GREEN, BLUE, ORANGE, GREEN].into_iter().enumerate() {
        let frame = if i == 0 { waiting.take().unwrap() } else { cap.start(&session) };
        client.send(Cmd::Color(color));
        let pic = cap.finish(&frame, Duration::from_secs(5)).unwrap_or_else(|| panic!("{renderer}: frame {i}"));
        assert!(near(pic.px(x, y), rgb(color)), "{renderer}: frame {i}: {:?}", pic.px(x, y));
        // Damage around the window, not the whole output.
        let damage = &cap.heard.damage;
        let covers = |&(dx, dy, dw, dh): &(i32, i32, i32, i32)| {
            dx <= x as i32 && (x as i32) < dx + dw && dy <= y as i32 && (y as i32) < dy + dh
        };
        assert!(damage.iter().any(covers), "{renderer}: frame {i}: damage {damage:?}");
        assert!(!damage.contains(&(0, 0, 1280, 800)), "{renderer}: frame {i}: all damaged");
    }
}

#[test]
fn output_stream_under_pixman() {
    output_stream("pixman");
}

#[test]
fn output_stream_under_gles() {
    output_stream("gles");
}

/// A window's stream: its content alone, at its size; new constraints
/// when it's resized (a frame waiting with the old buffer fails, the next
/// comes at once); stopped when it closes.
#[test]
fn window_stream_follows_the_window() {
    let c = start("pixman");
    let (client, r) = window(&c, "org.test.shared");
    // Another window over part of it isn't in its frames.
    let _other = c.client(Spec::new("org.test.other", GREEN).size(200, 150));
    c.wait_window("org.test.other");
    c.settle();
    let mut cap = Capturer::new(&c);
    let session = cap.window_session("org.test.shared");
    assert_eq!(cap.heard.size, Some((r.w as u32, r.h as u32)));
    let frame = cap.start(&session);
    let pic = cap.finish(&frame, Duration::from_secs(5)).expect("the window's first frame");
    for (x, y) in [(0, 0), (r.w as u32 - 1, r.h as u32 - 1), (r.w as u32 / 2, r.h as u32 / 2)] {
        assert!(near(pic.px(x, y), rgb(BLUE)), "the window's content at {x},{y}: {:?}", pic.px(x, y));
    }
    assert_eq!(cap.heard.damage, vec![(0, 0, r.w, r.h)]);
    let frame = cap.start(&session);
    client.send(Cmd::Color(ORANGE));
    let pic = cap.finish(&frame, Duration::from_secs(5)).expect("a frame after the change");
    assert!(near(pic.px(10, 10), rgb(ORANGE)), "{:?}", pic.px(10, 10));

    // A resize: new constraints, and the waiting frame fails.
    let frame = cap.start(&session);
    let done = cap.heard.constraints;
    cap.heard.forget_constraints();
    let id = c.window("org.test.shared").unwrap().id;
    let rect = aizome_ipc::Rect { x: r.x, y: r.y, w: 400, h: 260 };
    c.ok(aizome_ipc::Request::Resize { id: Some(id), rect });
    cap.fails(&frame, FailureReason::BufferConstraints);
    assert!(cap.wait(Duration::from_secs(5), |h| h.constraints > done), "no new constraints");
    assert_eq!(cap.heard.size, Some((400, 260)));
    let frame = cap.start(&session);
    let pic = cap.finish(&frame, Duration::from_secs(5)).expect("the resized window's frame at once");
    assert_eq!((pic.w, pic.h), (400, 260));
    assert!(near(pic.px(390, 250), rgb(ORANGE)), "{:?}", pic.px(390, 250));

    // Closed: the session stops and a frame waiting fails.
    let frame = cap.start(&session);
    c.ok(aizome_ipc::Request::Close { id: Some(id) });
    cap.fails(&frame, FailureReason::Stopped);
    assert!(cap.wait(Duration::from_secs(5), |h| h.stopped), "the session didn't stop");
    drop(client);
}

/// The portal makes a frame before destroying its last when its stream
/// pauses and resumes: the newer frame takes over (no protocol error),
/// and only it is answered.
#[test]
fn a_second_frame_replaces_the_first() {
    let c = start("pixman");
    let (client, r) = window(&c, "org.test.capture");
    let mut cap = Capturer::new(&c);
    let session = cap.output_session();
    let frame = cap.start(&session);
    cap.finish(&frame, Duration::from_secs(5)).expect("the first frame");
    let first = cap.start(&session);
    let second = cap.start(&session);
    client.send(Cmd::Color(ORANGE));
    let pic = cap.finish(&second, Duration::from_secs(5)).expect("the second frame");
    let (x, y) = ((r.x + 150) as u32, (r.y + 100) as u32);
    assert!(near(pic.px(x, y), rgb(ORANGE)));
    cap.wait(Duration::from_millis(200), |_| false);
    assert!(!cap.heard.answered.contains(&first.id()), "the replaced frame was answered");
    first.destroy();
    // Still connected and streaming.
    let frame = cap.start(&session);
    client.send(Cmd::Color(BLUE));
    cap.finish(&frame, Duration::from_secs(5)).expect("a frame after");
}

/// Under GLES on a render node: the device and the modifiers on offer,
/// and frames rendered into a dmabuf on the GPU, upright, one per change.
#[test]
fn dmabuf_stream_under_gles() {
    use aizome_comp_core::smithay::reexports::gbm;
    use std::os::unix::fs::MetadataExt;
    let Some(node) = std::fs::read_dir("/dev/dri")
        .ok()
        .and_then(|d| d.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains("renderD")).min())
    else {
        eprintln!("no render node: skipping");
        return;
    };
    let c = start("gles");
    if !c.log_text().contains("rendering with GLES") {
        eprintln!("GLES didn't start here: skipping");
        return;
    }
    let (client, r) = window(&c, "org.test.capture");
    let (x, y) = (r.x + 150, r.y + 100);
    let mut cap = Capturer::new(&c);
    let session = cap.output_session();
    let device = cap.heard.device.expect("a dmabuf device under GLES");
    assert_eq!(device, std::fs::metadata(&node).unwrap().rdev(), "the render node's dev_t");
    const XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
    let modifiers = cap.heard.dmabuf.iter().find(|(f, _)| *f == XRGB8888).map(|(_, m)| m.clone());
    let modifiers = modifiers.expect("XRGB8888 dmabufs on offer");
    if !modifiers.contains(&0) {
        eprintln!("no linear XRGB8888 here ({modifiers:x?}): skipping");
        return;
    }
    let (w, h) = cap.heard.size.unwrap();
    let file = std::fs::OpenOptions::new().read(true).write(true).open(&node).unwrap();
    let gbm = gbm::Device::new(file).unwrap();
    let bo = gbm
        .create_buffer_object::<()>(
            w,
            h,
            gbm::Format::Xrgb8888,
            gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR,
        )
        .unwrap();
    let params = cap.dmabuf.as_ref().expect("linux-dmabuf").create_params(&cap.qh, ());
    let modifier: u64 = bo.modifier().into();
    params.add(
        bo.fd_for_plane(0).unwrap().as_fd(),
        0,
        bo.offset(0),
        bo.stride_for_plane(0),
        (modifier >> 32) as u32,
        modifier as u32,
    );
    let buffer =
        params.create_immed(w as i32, h as i32, XRGB8888, zwp_linux_buffer_params_v1::Flags::empty(), &cap.qh, ());
    let capture = |cap: &mut Capturer, within: Duration| -> Option<([u8; 3], [u8; 3])> {
        cap.heard.forget_frame();
        let frame = session.create_frame(&cap.qh, ());
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, w as i32, h as i32);
        frame.capture();
        if !cap.wait(within, |h| h.ready || h.failed.is_some()) {
            frame.destroy();
            return None;
        }
        assert_eq!(cap.heard.failed, None, "the dmabuf capture failed:\n{}", c.log_text());
        frame.destroy();
        Some(
            bo.map(0, 0, w, h, |m| {
                let s = m.stride();
                let px = |x: i32, y: i32| {
                    let i = (y as u32 * s + x as u32 * 4) as usize;
                    let b = m.buffer();
                    [b[i + 2], b[i + 1], b[i]]
                };
                // Inside the window, and below it (upright: not mirrored).
                (px(x, y), px(x, y + 150))
            })
            .unwrap(),
        )
    };
    let (at, below) = capture(&mut cap, Duration::from_secs(5)).expect("the first frame at once");
    assert!(near(at, rgb(BLUE)), "the window in the dmabuf: {at:?}");
    assert!(!near(below, rgb(BLUE)), "below the window: {below:?}");
    assert!(capture(&mut cap, Duration::from_millis(300)).is_none(), "a frame without a change");
    for color in [ORANGE, GREEN, BLUE] {
        client.send(Cmd::Color(color));
        let (at, _) = capture(&mut cap, Duration::from_secs(5)).expect("a frame after a change");
        assert!(near(at, rgb(color)), "{at:?}");
    }
}

/// A session that showed a frame is a share, which the shell hears of
/// (IPC `shares`, `shares-changed`) and can stop: the session stops and a
/// frame waiting fails, as when its source goes.
#[test]
fn shares_are_reported_and_can_be_stopped() {
    use aizome_ipc::{Event, Request, Response};
    let c = start("pixman");
    let (_client, _) = window(&c, "org.test.capture");
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut next_shares = || {
        events.by_ref().find_map(|e| match e {
            Ok(Event::SharesChanged { shares }) => Some(shares),
            _ => None,
        })
    };
    let mut cap = Capturer::new(&c);
    let session = cap.output_session();
    // Not a share until a frame showed.
    assert_eq!(c.request(Request::Shares), Response::Shares { shares: Vec::new() });
    let frame = cap.start(&session);
    cap.finish(&frame, Duration::from_secs(5)).expect("the first frame");
    let shares = next_shares().expect("shares-changed");
    assert_eq!(shares.len(), 1, "{shares:?}");
    assert_eq!((shares[0].output.as_deref(), shares[0].window), (Some("HEADLESS-1"), None));
    assert_eq!(c.request(Request::Shares), Response::Shares { shares: shares.clone() });
    // Stopped from the shell's indicator.
    let frame = cap.start(&session);
    c.ok(Request::StopShare { id: Some(shares[0].id) });
    cap.fails(&frame, FailureReason::Stopped);
    assert!(cap.wait(Duration::from_secs(5), |h| h.stopped), "the session didn't stop");
    assert_eq!(next_shares().expect("shares-changed"), Vec::new());
    assert!(matches!(c.request(Request::StopShare { id: None }), Response::Error { .. }));
}

impl Dispatch<Session, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &Session,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => h.size = Some((width, height)),
            Event::ShmFormat { format: WEnum::Value(f) } => h.shm.push(f),
            Event::DmabufDevice { device } => h.device = Some(u64::from_ne_bytes(device.try_into().unwrap())),
            Event::DmabufFormat { format, modifiers } => {
                h.dmabuf.push((format, modifiers.as_chunks::<8>().0.iter().map(|m| u64::from_ne_bytes(*m)).collect()))
            }
            Event::Done => h.constraints += 1,
            Event::Stopped => h.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<Frame, ()> for Heard {
    fn event(
        h: &mut Self,
        frame: &Frame,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Transform { transform: WEnum::Value(t) } => h.transform = Some(t),
            Event::Damage { x, y, width, height } => h.damage.push((x, y, width, height)),
            Event::PresentationTime { .. } => h.presented = true,
            Event::Ready => h.ready = true,
            Event::Failed { reason: WEnum::Value(r) } => h.failed = Some(r),
            _ => {}
        }
        if h.ready || h.failed.is_some() {
            h.answered.push(frame.id());
        }
    }
}

impl Dispatch<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, ()> for Heard {
    fn event(
        _: &mut Self,
        _: &ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        _: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(Heard, ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (Toplevel, ()),
    ]);
}

impl Dispatch<Toplevel, ()> for Heard {
    fn event(
        h: &mut Self,
        t: &Toplevel,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_handle_v1::Event::AppId { app_id } = event {
            h.toplevels.retain(|(o, _)| o.id() != t.id());
            h.toplevels.push((t.clone(), app_id));
        }
    }
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
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    wl_output::WlOutput,
    ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1
);
