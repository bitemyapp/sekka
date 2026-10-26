//! Content updates that wait for the display: wp_fifo_v1 barriers and
//! wp_commit_timing_v1 timestamps. A Vulkan swapchain presenting with
//! vsync (FIFO) commits every frame with wait_barrier and set_barrier, so
//! a compositor that never clears barriers freezes it after one frame.
//!
//! Under pixman a buffer is released when the next commit replaces it,
//! so a buffer's release says when the commit after it applied.

mod common;

use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use aizome_ipc::Request;
use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::commit_timing::v1::client::{wp_commit_timer_v1, wp_commit_timing_manager_v1};
use wayland_protocols::wp::fifo::v1::client::{wp_fifo_manager_v1, wp_fifo_v1};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

/// The headless output's refresh (60 Hz).
const REFRESH: Duration = Duration::from_micros(16_667);

#[derive(Default)]
struct Probe {
    configured: bool,
    /// When each buffer (by the order it was made) was released.
    released: Vec<Option<Instant>>,
}

struct Client {
    conn: Connection,
    queue: EventQueue<Probe>,
    qh: QueueHandle<Probe>,
    state: Probe,
    shm: wl_shm::WlShm,
    surface: wl_surface::WlSurface,
    fifo: wp_fifo_v1::WpFifoV1,
    timer: wp_commit_timer_v1::WpCommitTimerV1,
    _toplevel: (xdg_surface::XdgSurface, xdg_toplevel::XdgToplevel),
}

impl Client {
    /// A mapped toplevel with a fifo object and a commit timer; its first
    /// buffer is buffer 0.
    fn new(c: &Compositor, app_id: &str) -> Client {
        let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
        let (globals, queue) = registry_queue_init::<Probe>(&conn).unwrap();
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 5..=6, ()).unwrap();
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).unwrap();
        let wm: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=6, ()).unwrap();
        let fifos: wp_fifo_manager_v1::WpFifoManagerV1 = globals.bind(&qh, 1..=1, ()).expect("wp_fifo_manager_v1");
        let timing: wp_commit_timing_manager_v1::WpCommitTimingManagerV1 =
            globals.bind(&qh, 1..=1, ()).expect("wp_commit_timing_manager_v1");
        let surface = compositor.create_surface(&qh, ());
        let fifo = fifos.get_fifo(&surface, &qh, ());
        let timer = timing.get_timer(&surface, &qh, ());
        let xs = wm.get_xdg_surface(&surface, &qh, ());
        let tl = xs.get_toplevel(&qh, ());
        tl.set_app_id(app_id.into());
        surface.commit();
        let mut client =
            Client { conn, queue, qh, state: Probe::default(), shm, surface, fifo, timer, _toplevel: (xs, tl) };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !client.state.configured {
            client.roundtrip();
            assert!(Instant::now() < deadline, "no configure");
        }
        client.present(false, None);
        client.roundtrip();
        c.wait_window(app_id);
        client
    }

    fn roundtrip(&mut self) {
        self.queue.roundtrip(&mut self.state).unwrap();
    }

    /// Commits a new buffer as a swapchain would: with `fifo`, waiting for
    /// the barrier the commit before set and setting one; with `at`, not
    /// to be shown before then (CLOCK_MONOTONIC). Returns its number.
    fn present(&mut self, fifo: bool, at: Option<Duration>) -> usize {
        let n = self.state.released.len();
        self.state.released.push(None);
        let (w, h) = (300, 200);
        // Safety: memfd_create with a static name.
        let fd = unsafe { OwnedFd::from_raw_fd(libc::memfd_create(c"aizome-pacing".as_ptr(), 0)) };
        let mut file = std::fs::File::from(fd);
        let shade = (n as u8).wrapping_mul(37);
        file.write_all(&[shade, 0x66, 0x22, 0xff].repeat((w * h) as usize)).unwrap();
        let pool = self.shm.create_pool(file.as_fd(), w * h * 4, &self.qh, ());
        let buffer = pool.create_buffer(0, w, h, w * 4, wl_shm::Format::Argb8888, &self.qh, n);
        pool.destroy();
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.damage_buffer(0, 0, w, h);
        if fifo {
            self.fifo.wait_barrier();
            self.fifo.set_barrier();
        }
        if let Some(t) = at {
            let secs = t.as_secs();
            self.timer.set_timestamp((secs >> 32) as u32, secs as u32, t.subsec_nanos());
        }
        self.surface.commit();
        self.conn.flush().unwrap();
        n
    }

    /// When buffer `n` was released (the commit after it applied).
    fn wait_released(&mut self, n: usize, within: Duration) -> Instant {
        let deadline = Instant::now() + within;
        loop {
            if let Some(t) = self.state.released[n] {
                return t;
            }
            assert!(
                Instant::now() < deadline,
                "buffer {n} wasn't released within {within:?}: the commit after it never applied ({} of {} released)",
                self.state.released.iter().filter(|r| r.is_some()).count(),
                self.state.released.len()
            );
            self.roundtrip();
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// The compositor's presentation clock.
fn monotonic() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // Safety: a valid timespec to fill.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

fn start() -> Compositor {
    Compositor::start(Options { renderer: Some("pixman"), ..Default::default() })
}

/// A swapchain presenting with vsync, frames queued ahead: every commit
/// applies, one a refresh.
#[test]
fn fifo_commits_apply_one_a_refresh() {
    let c = start();
    let mut a = Client::new(&c, "fifo-visible");
    let frames: Vec<usize> = (0..20).map(|_| a.present(true, None)).collect();
    // The first fifo commit waits for nothing; each one after it waits
    // for a frame that showed the one before.
    let second = a.wait_released(frames[0], Duration::from_secs(5));
    let last = a.wait_released(frames[18], Duration::from_secs(5));
    let cycles = 18;
    assert!(
        last - second >= REFRESH * cycles * 3 / 4,
        "{cycles} barriers cleared in {:?}: faster than one a refresh",
        last - second
    );
}

/// A window nobody sees (minimised) has no frames to clear its barriers;
/// they clear about once a second instead, so it still makes progress.
#[test]
fn fifo_commits_of_a_hidden_window_still_apply() {
    let c = start();
    let mut a = Client::new(&c, "fifo-hidden");
    let w = c.wait_window("fifo-hidden");
    c.ok(Request::Minimise { id: Some(w.id) });
    c.settle();
    let frames: Vec<usize> = (0..3).map(|_| a.present(true, None)).collect();
    let second = a.wait_released(frames[0], Duration::from_secs(5));
    let third = a.wait_released(frames[1], Duration::from_secs(5));
    assert!(
        third - second >= Duration::from_millis(500),
        "a hidden window's barrier cleared after {:?}: it should wait about a second",
        third - second
    );
}

/// An already-armed backstop must keep checking after the output stops
/// drawing, even if its last frame was less than a second ago.
#[test]
fn fifo_commits_keep_progressing_after_display_power_off() {
    let c = start();
    let mut a = Client::new(&c, "fifo-power-off");
    let frames: Vec<usize> = (0..20).map(|_| a.present(true, None)).collect();
    a.wait_released(frames[18], Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(50));
    c.ok(Request::PowerOffMonitors);
    let frames: Vec<usize> = (0..3).map(|_| a.present(true, None)).collect();
    a.wait_released(frames[1], Duration::from_secs(4));
}

/// The first backstop tick finds no current barrier: the commit that
/// installs it is still waiting on a timestamp. Later FIFO commits must
/// still advance once that timestamp passes, without more client requests.
#[test]
fn hidden_fifo_commits_resume_after_a_future_timestamp() {
    let c = start();
    let mut a = Client::new(&c, "fifo-hidden-future");
    let w = c.wait_window("fifo-hidden-future");
    c.ok(Request::Minimise { id: Some(w.id) });
    c.settle();
    let at = monotonic() + Duration::from_millis(1500);
    let first = a.present(true, Some(at));
    a.present(true, Some(at));
    a.present(true, Some(at));
    a.wait_released(first + 1, Duration::from_secs(5));
}

/// wait_barrier need not set a replacement barrier. Once smithay consumes
/// that wait, the pending cached state no longer contains the barrier;
/// until its timestamp passes, the current state doesn't contain it either.
#[test]
fn hidden_fifo_wait_without_a_new_barrier_survives_a_future_timestamp() {
    let c = start();
    let mut a = Client::new(&c, "fifo-hidden-wait-only");
    let w = c.wait_window("fifo-hidden-wait-only");
    c.ok(Request::Minimise { id: Some(w.id) });
    c.settle();
    let at = monotonic() + Duration::from_millis(1500);
    let first = a.present(true, Some(at));
    a.fifo.wait_barrier();
    a.present(false, Some(at));
    a.wait_released(first, Duration::from_secs(4));
}

/// Real swapchains combine both constraints. Releasing the timestamp must
/// preserve FIFO progress and its refresh pacing for the queued frames.
#[test]
fn visible_fifo_commits_with_timestamps_keep_drawing() {
    let c = start();
    let mut a = Client::new(&c, "fifo-timed");
    let at = monotonic() + Duration::from_millis(100);
    let frames: Vec<usize> = (0..20).map(|n| a.present(true, Some(at + REFRESH * n))).collect();
    let second = a.wait_released(frames[0], Duration::from_secs(5));
    let last = a.wait_released(frames[18], Duration::from_secs(5));
    assert!(last - second >= REFRESH * 18 * 3 / 4, "timestamped FIFO commits ran faster than the refresh rate");
}

/// A timestamped commit applies when its time comes, not before, even
/// with nothing else drawing.
#[test]
fn timed_commits_apply_at_their_time() {
    let c = start();
    let mut a = Client::new(&c, "timed");
    c.settle();
    applies_at_its_time(&mut a);
}

/// A minimised window's timestamped commit applies at its time too.
#[test]
fn timed_commits_of_a_hidden_window_apply_at_their_time() {
    let c = start();
    let mut a = Client::new(&c, "timed-hidden");
    let w = c.wait_window("timed-hidden");
    c.ok(Request::Minimise { id: Some(w.id) });
    c.settle();
    applies_at_its_time(&mut a);
}

fn applies_at_its_time(a: &mut Client) {
    let wait = Duration::from_millis(300);
    let sent = Instant::now();
    let shown = a.present(false, Some(monotonic() + wait));
    let after = a.wait_released(shown - 1, Duration::from_secs(5)) - sent;
    assert!(
        after >= wait - REFRESH && after < wait + Duration::from_millis(200),
        "a commit for {wait:?} ahead applied after {after:?}"
    );
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
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
    xdg_toplevel::XdgToplevel,
    wp_fifo_manager_v1::WpFifoManagerV1,
    wp_fifo_v1::WpFifoV1,
    wp_commit_timing_manager_v1::WpCommitTimingManagerV1,
    wp_commit_timer_v1::WpCommitTimerV1
);

impl Dispatch<wl_buffer::WlBuffer, usize> for Probe {
    fn event(
        p: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        n: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            p.released[*n].get_or_insert_with(Instant::now);
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
