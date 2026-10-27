//! An idle inhibitor holds off idleness even on a surface that's never
//! shown. Keep Awake (`aizome-awake hold`) relies on that: it has no
//! window, only a bare surface with an inhibitor. Watched through
//! ext-idle-notify, as idle daemons see it: with the inhibitor there's no
//! `idled`, and once it's gone idleness comes as usual. It's gone too when
//! its client goes without destroying it, as a killed holder does.

mod common;

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_registry, wl_seat, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1, ext_idle_notifier_v1};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1};

#[derive(Default)]
struct Heard {
    idled: u32,
    resumed: u32,
}

struct Client {
    conn: Connection,
    queue: EventQueue<Heard>,
    heard: Heard,
}

impl Client {
    fn roundtrip(&mut self) {
        if let Err(e) = self.queue.roundtrip(&mut self.heard) {
            panic!("disconnected: {e} ({:?})", self.conn.protocol_error());
        }
    }

    /// Runs the queue for `d`.
    fn run_for(&mut self, d: Duration) {
        let until = Instant::now() + d;
        while Instant::now() < until {
            self.roundtrip();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn an_unshown_surfaces_inhibitor_holds_off_idleness() {
    let c = Compositor::start(Options { outputs: &["1280x800@1"], ..Default::default() });
    let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
    let (globals, queue) = registry_queue_init::<Heard>(&conn).unwrap();
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ()).unwrap();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).unwrap();
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = globals.bind(&qh, 1..=1, ()).expect("ext_idle_notifier_v1");
    let inhibit: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
        globals.bind(&qh, 1..=1, ()).expect("zwp_idle_inhibit_manager_v1");
    let mut a = Client { conn, queue, heard: Heard::default() };

    // Keep Awake's way: a surface with no role and no buffer, and an
    // inhibitor on it.
    let surface = compositor.create_surface(&qh, ());
    let inhibitor = inhibit.create_inhibitor(&surface, &qh, ());
    a.roundtrip();

    // Idle after 300 ms without input; the headless session has none.
    let _note = notifier.get_idle_notification(300, &seat, &qh, ());
    a.run_for(Duration::from_millis(1500));
    assert_eq!(a.heard.idled, 0, "idle while an inhibitor was held");

    // Let go: idleness comes.
    inhibitor.destroy();
    let until = Instant::now() + Duration::from_secs(5);
    while a.heard.idled == 0 {
        assert!(Instant::now() < until, "never idle after the inhibitor went");
        a.run_for(Duration::from_millis(100));
    }

    // And taken again, idleness ends (resumed) and doesn't come back.
    let again = inhibit.create_inhibitor(&surface, &qh, ());
    a.run_for(Duration::from_millis(300));
    assert_eq!(a.heard.resumed, 1, "not resumed when the inhibitor came back");
    let idled = a.heard.idled;
    a.run_for(Duration::from_millis(1500));
    assert_eq!(a.heard.idled, idled, "idle again while an inhibitor was held");
    again.destroy();
}

#[test]
fn an_inhibitor_goes_with_its_client() {
    let c = Compositor::start(Options { outputs: &["1280x800@1"], ..Default::default() });

    // The holder, as Keep Awake's: a bare surface with an inhibitor.
    let stream = UnixStream::connect(c.dir.join(&c.socket)).unwrap();
    let socket = stream.try_clone().unwrap();
    let conn = Connection::from_socket(stream).unwrap();
    let (globals, queue) = registry_queue_init::<Heard>(&conn).unwrap();
    let qh = queue.handle();
    let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ()).unwrap();
    let inhibit: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
        globals.bind(&qh, 1..=1, ()).expect("zwp_idle_inhibit_manager_v1");
    let surface = compositor.create_surface(&qh, ());
    let _inhibitor = inhibit.create_inhibitor(&surface, &qh, ());
    let mut holder = Client { conn, queue, heard: Heard::default() };
    holder.roundtrip();

    // Another client watches for idleness, as an idle daemon would.
    let conn = Connection::from_socket(UnixStream::connect(c.dir.join(&c.socket)).unwrap()).unwrap();
    let (globals, queue) = registry_queue_init::<Heard>(&conn).unwrap();
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).unwrap();
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = globals.bind(&qh, 1..=1, ()).expect("ext_idle_notifier_v1");
    let _note = notifier.get_idle_notification(300, &seat, &qh, ());
    let mut watcher = Client { conn, queue, heard: Heard::default() };
    watcher.run_for(Duration::from_millis(1500));
    assert_eq!(watcher.heard.idled, 0, "idle while the holder held its inhibitor");

    // The holder dies without destroying anything (`kill -9`): idleness
    // comes at once, not never.
    socket.shutdown(Shutdown::Both).unwrap();
    drop(holder);
    let until = Instant::now() + Duration::from_secs(3);
    while watcher.heard.idled == 0 {
        assert!(Instant::now() < until, "never idle after the inhibitor's client went");
        watcher.run_for(Duration::from_millis(100));
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

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for Heard {
    fn event(
        h: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_idle_notification_v1::Event::Idled => h.idled += 1,
            ext_idle_notification_v1::Event::Resumed => h.resumed += 1,
            _ => {}
        }
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
    wl_surface::WlSurface,
    wl_seat::WlSeat,
    ext_idle_notifier_v1::ExtIdleNotifierV1,
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1,
    zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1
);
