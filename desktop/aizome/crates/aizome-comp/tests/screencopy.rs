//! wlr-screencopy as screen recorders, xdg-desktop-portal-wlr and grim
//! use it: wl_shm copies of a whole output and of a region under pixman
//! and GLES; `copy_with_damage` answering at once for a stream's first
//! frame, then only once something in its region changes; and, under
//! GLES on a render node, the linux-dmabuf buffers that keep a screencast
//! on the GPU.

mod common;

use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{zwp_linux_buffer_params_v1, zwp_linux_dmabuf_v1};
use wayland_protocols_wlr::screencopy::v1::client::{zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1};

use zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1 as Frame;

/// What a frame said.
#[derive(Default)]
struct Heard {
    shm: Option<(wl_shm::Format, u32, u32, u32)>,
    dmabuf: Option<(u32, u32, u32)>,
    buffer_done: bool,
    damage: Vec<(u32, u32, u32, u32)>,
    ready: bool,
    failed: bool,
}

struct Copier {
    conn: Connection,
    queue: EventQueue<Heard>,
    qh: QueueHandle<Heard>,
    heard: Heard,
    shm: wl_shm::WlShm,
    output: wl_output::WlOutput,
    manager: zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    dmabuf: Option<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
    /// The wl_shm buffer a copy is going into.
    target: Option<ShmTarget>,
}

/// A wl_shm buffer a copy goes into: its pool, the buffer, the memfd and
/// width, height and stride.
type ShmTarget = (wl_shm_pool::WlShmPool, wl_buffer::WlBuffer, std::fs::File, (u32, u32, u32));

/// A copied frame: its pixels in the client's byte order (BGRX), rows
/// packed.
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

impl Copier {
    fn new(c: &Compositor) -> Copier {
        let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
        let (globals, mut queue) = registry_queue_init::<Heard>(&conn).unwrap();
        let qh = queue.handle();
        let shm = globals.bind(&qh, 1..=1, ()).unwrap();
        let output = globals.bind(&qh, 1..=4, ()).unwrap();
        let manager = globals.bind(&qh, 3..=3, ()).expect("wlr-screencopy 3");
        let dmabuf = globals.bind(&qh, 3..=4, ()).ok();
        let mut heard = Heard::default();
        queue.roundtrip(&mut heard).unwrap();
        Copier { conn, queue, qh, heard, shm, output, manager, dmabuf, target: None }
    }

    /// Asks for a frame and waits for the buffers it offers.
    fn frame(&mut self, region: Option<(i32, i32, i32, i32)>) -> Frame {
        self.heard = Heard::default();
        let frame = match region {
            None => self.manager.capture_output(0, &self.output, &self.qh, ()),
            Some((x, y, w, h)) => self.manager.capture_output_region(0, &self.output, x, y, w, h, &self.qh, ()),
        };
        assert!(self.wait(Duration::from_secs(5), |h| h.buffer_done || h.failed), "no buffer_done");
        assert!(!self.heard.failed, "the frame failed");
        frame
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

    /// Sends a copy into a new wl_shm buffer (kept until `finish_shm`).
    fn start_shm(&mut self, frame: &Frame, with_damage: bool) {
        let (format, w, h, stride) = self.heard.shm.expect("a wl_shm offer");
        assert_eq!(format, wl_shm::Format::Xrgb8888);
        let len = (stride * h) as usize;
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-copy".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        file.write_all(&vec![0u8; len]).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), len as i32, &self.qh, ());
        let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, format, &self.qh, ());
        if with_damage {
            frame.copy_with_damage(&buffer);
        } else {
            frame.copy(&buffer);
        }
        self.target = Some((pool, buffer, file, (w, h, stride)));
    }

    /// Waits for the copy; `None` if nothing came in `within` (the buffer
    /// stays, the copy still pending).
    fn finish_shm(&mut self, within: Duration) -> Option<Picture> {
        let got = self.wait(within, |h| h.ready || h.failed);
        assert!(!self.heard.failed, "the copy failed");
        if !got {
            return None;
        }
        let (pool, buffer, file, (w, h, stride)) = self.target.take().expect("a copy sent");
        pool.destroy();
        buffer.destroy();
        let len = (stride * h) as usize;
        // Safety: the client's own memfd, `len` bytes.
        let map =
            unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_SHARED, file.as_raw_fd(), 0) };
        assert_ne!(map, libc::MAP_FAILED);
        // Safety: mapped above.
        let bytes = unsafe { std::slice::from_raw_parts(map as *const u8, len) };
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            bgra.extend_from_slice(&bytes[(y * stride) as usize..(y * stride + w * 4) as usize]);
        }
        // Safety: unmapping what we mapped.
        unsafe { libc::munmap(map, len) };
        Some(Picture { w, h, bgra })
    }

    fn copy_shm(&mut self, frame: &Frame, with_damage: bool, within: Duration) -> Option<Picture> {
        self.start_shm(frame, with_damage);
        self.finish_shm(within)
    }
}

fn start(renderer: &str) -> Compositor {
    Compositor::start(Options { outputs: &["1280x800@1"], renderer: Some(renderer), ..Default::default() })
}

const BLUE: u32 = 0x3366cc;
const ORANGE: u32 = 0xdd7722;

/// A blue window, and a point inside it.
fn window(c: &Compositor) -> (TestClient, (i32, i32)) {
    let client = c.client(Spec::new("org.test.copy", BLUE).size(300, 200));
    let w = c.wait_window("org.test.copy");
    c.settle();
    (client, (w.geometry.x + 150, w.geometry.y + 100))
}

fn whole_and_region(renderer: &str) {
    let c = start(renderer);
    let (_client, (x, y)) = window(&c);
    let mut copier = Copier::new(&c);
    let frame = copier.frame(None);
    let pic = copier.copy_shm(&frame, false, Duration::from_secs(5)).expect("a whole-output copy");
    assert_eq!((pic.w, pic.h), (1280, 800));
    assert!(
        near(pic.px(x as u32, y as u32), rgb(BLUE)),
        "{renderer}: the window at {x},{y}: {:?}",
        pic.px(x as u32, y as u32)
    );
    assert!(!near(pic.px(5, 795), rgb(BLUE)), "{renderer}: the desktop isn't the window");
    // A region straddling the window's left edge: desktop, then window.
    let frame = copier.frame(Some((x - 160, y - 10, 40, 20)));
    let pic = copier.copy_shm(&frame, false, Duration::from_secs(5)).expect("a region copy");
    assert_eq!((pic.w, pic.h), (40, 20));
    assert!(near(pic.px(35, 10), rgb(BLUE)), "{renderer}: the region's right end is the window");
    assert!(!near(pic.px(2, 10), rgb(BLUE)), "{renderer}: the region's left end is the desktop");
}

#[test]
fn shm_copies_under_pixman() {
    whole_and_region("pixman");
}

#[test]
fn shm_copies_under_gles() {
    whole_and_region("gles");
}

#[test]
fn copies_with_damage_wait_for_a_change() {
    let c = start("pixman");
    let (client, (x, y)) = window(&c);
    let mut copier = Copier::new(&c);
    let region = Some((x - 50, y - 50, 100, 100));
    // A stream's first frame comes at once, all of it damaged.
    let frame = copier.frame(region);
    let pic = copier.copy_shm(&frame, true, Duration::from_secs(5)).expect("the first frame at once");
    assert!(near(pic.px(50, 50), rgb(BLUE)));
    assert_eq!(copier.heard.damage, vec![(0, 0, 100, 100)]);
    // Then nothing until the window changes, however the pointer moves.
    let frame = copier.frame(region);
    copier.start_shm(&frame, true);
    c.ok(aizome_ipc::Request::Synthesize { events: vec![aizome_ipc::Synthetic::PointerMotion { x: 5.0, y: 5.0 }] });
    assert!(copier.finish_shm(Duration::from_millis(400)).is_none(), "a frame without a change");
    client.send(Cmd::Color(ORANGE));
    let pic = copier.finish_shm(Duration::from_secs(5)).expect("a frame after the change");
    assert!(near(pic.px(50, 50), rgb(ORANGE)), "{:?}", pic.px(50, 50));
    assert!(!copier.heard.damage.is_empty());
}

/// The GLES renderer's dmabufs, when the test machine has a render node:
/// offered, rendered into on the GPU, and upright.
#[test]
fn dmabuf_copies_under_gles() {
    use aizome_comp_core::smithay::reexports::gbm;
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
    let (_client, (x, y)) = window(&c);
    let mut copier = Copier::new(&c);
    let frame = copier.frame(None);
    let (fourcc, w, h) = copier.heard.dmabuf.expect("a dmabuf offer under GLES");
    assert_eq!((w, h), (1280, 800));
    let file = std::fs::OpenOptions::new().read(true).write(true).open(&node).unwrap();
    let device = gbm::Device::new(file).unwrap();
    let format = gbm::Format::try_from(fourcc).unwrap();
    let bo = device
        .create_buffer_object::<()>(w, h, format, gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR)
        .unwrap();
    let params = copier.dmabuf.as_ref().expect("linux-dmabuf").create_params(&copier.qh, ());
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
        params.create_immed(w as i32, h as i32, fourcc, zwp_linux_buffer_params_v1::Flags::empty(), &copier.qh, ());
    frame.copy(&buffer);
    assert!(copier.wait(Duration::from_secs(5), |h| h.ready || h.failed));
    assert!(!copier.heard.failed, "the dmabuf copy failed:\n{}", c.log_text());
    let (at, below) = bo
        .map(0, 0, w, h, |m| {
            let s = m.stride();
            let px = |x: i32, y: i32| {
                let i = (y as u32 * s + x as u32 * 4) as usize;
                let b = m.buffer();
                [b[i + 2], b[i + 1], b[i]]
            };
            // Inside the window, and below it (upright: not mirrored).
            (px(x, y), px(x, y + 150))
        })
        .unwrap();
    assert!(near(at, rgb(BLUE)), "the window in the dmabuf: {at:?}");
    assert!(!near(below, rgb(BLUE)), "below the window: {below:?}");
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &Frame,
        event: zwlr_screencopy_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_screencopy_frame_v1::Event;
        match event {
            Event::Buffer { format: WEnum::Value(f), width, height, stride } => {
                h.shm = Some((f, width, height, stride))
            }
            Event::LinuxDmabuf { format, width, height } => h.dmabuf = Some((format, width, height)),
            Event::BufferDone => h.buffer_done = true,
            Event::Damage { x, y, width, height } => h.damage.push((x, y, width, height)),
            Event::Ready { .. } => h.ready = true,
            Event::Failed => h.failed = true,
            _ => {}
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
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1
);
