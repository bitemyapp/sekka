//! Who holds idleness off, and for how long (docs/keep-awake.md,
//! docs/ipc.md). The compositor says which windows and processes have an
//! idle inhibitor (`idle-inhibitors`) and tells subscribers when that
//! changes. An app's inhibitor counts until the session is locked: a
//! locked screen goes dark on its timer whatever plays behind it. Keep
//! Awake's hold, asked for over IPC (`keep-awake`), counts locked or not,
//! and ends with its connection.
//!
//! The settings count idleness in minutes, so it's brought on here by
//! `synthesize`'s `idle`: the idle clock is put back and read at once, as
//! the compositor's own timer reads it every five seconds.

mod common;

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use aizome_ipc::{Client, Event, IdleHolder, IdleInhibitorInfo, Request, Response, Synthetic};
use common::desktop::wait;
use common::*;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_compositor, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1, zwp_idle_inhibitor_v1};

/// The lock after a minute, the displays off after two.
const SETTINGS: &str =
    "[appearance]\neffects = \"austere\"\n[idle]\nlock_after_minutes = 1\nmonitors_off_after_minutes = 2\n";

/// Nothing here looks at a picture: drawn in software, the compositor
/// starts as fast on a host whose GPU is busy.
fn start() -> Compositor {
    Compositor::start(Options { settings: SETTINGS, renderer: Some("pixman"), ..Default::default() })
}

fn inhibitors(c: &Compositor) -> Vec<IdleInhibitorInfo> {
    match c.request(Request::IdleInhibitors) {
        Response::IdleInhibitors { inhibitors } => inhibitors,
        other => panic!("{other:?}"),
    }
}

fn displays_on(c: &Compositor) -> bool {
    match c.request(Request::Outputs) {
        Response::Outputs { outputs } => outputs.iter().all(|o| o.powered),
        other => panic!("{other:?}"),
    }
}

/// Nobody touches anything for `minutes`. The compositor has read its
/// idle clock by the time this returns.
fn left_alone_for(c: &Compositor, minutes: u32) {
    c.ok(Request::Synthesize { events: vec![Synthetic::Idle { seconds: minutes * 60 }] });
}

/// What a subscriber hears.
struct Listener(Arc<Mutex<Vec<Event>>>);

impl Listener {
    /// Subscribed by the time this returns.
    fn start(c: &Compositor) -> Listener {
        let events = Client::connect_to(&c.ipc).expect("IPC connect").events().expect("subscribe");
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = heard.clone();
        std::thread::spawn(move || {
            for event in events {
                let Ok(event) = event else { return };
                sink.lock().unwrap().push(event);
            }
        });
        Listener(heard)
    }

    /// Each answer `idle-inhibitors-changed` has given, in order.
    fn holders(&self) -> Vec<Vec<IdleInhibitorInfo>> {
        let heard = self.0.lock().unwrap();
        heard
            .iter()
            .filter_map(|e| match e {
                Event::IdleInhibitorsChanged { inhibitors } => Some(inhibitors.clone()),
                _ => None,
            })
            .collect()
    }

    /// The kinds of `idle` heard (`lock`, `monitors-off`), in order.
    fn idle(&self) -> Vec<String> {
        let heard = self.0.lock().unwrap();
        heard
            .iter()
            .filter_map(|e| match e {
                Event::Idle { kind } => Some(kind.clone()),
                _ => None,
            })
            .collect()
    }

    fn lock_states(&self) -> Vec<bool> {
        let heard = self.0.lock().unwrap();
        heard
            .iter()
            .filter_map(|e| match e {
                Event::LockStateChanged { locked } => Some(*locked),
                _ => None,
            })
            .collect()
    }
}

/// A client with an idle inhibitor on a surface it never shows, as the
/// shell holds one for the apps that ask the session bus.
struct Bare {
    socket: UnixStream,
    conn: Connection,
    queue: EventQueue<Nothing>,
    _surface: wl_surface::WlSurface,
    _inhibitor: zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1,
}

struct Nothing;

impl Bare {
    fn hold(c: &Compositor) -> Bare {
        let stream = UnixStream::connect(c.dir.join(&c.socket)).unwrap();
        let socket = stream.try_clone().unwrap();
        let conn = Connection::from_socket(stream).unwrap();
        let (globals, mut queue) = registry_queue_init::<Nothing>(&conn).unwrap();
        let qh = queue.handle();
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=6, ()).unwrap();
        let manager: zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1 =
            globals.bind(&qh, 1..=1, ()).expect("zwp_idle_inhibit_manager_v1");
        let surface = compositor.create_surface(&qh, ());
        let inhibitor = manager.create_inhibitor(&surface, &qh, ());
        queue.roundtrip(&mut Nothing).unwrap();
        Bare { socket, conn, queue, _surface: surface, _inhibitor: inhibitor }
    }

    /// Dies without destroying anything, as a killed process does.
    fn die(self) {
        self.socket.shutdown(Shutdown::Both).unwrap();
        drop((self.conn, self.queue));
    }
}

/// This process: the test's Wayland clients and IPC connections are its.
fn me() -> Option<i32> {
    Some(std::process::id() as i32)
}

#[test]
fn the_compositor_says_who_holds_idleness_off_and_when_that_changes() {
    let c = start();
    let heard = Listener::start(&c);
    assert!(inhibitors(&c).is_empty(), "a hold before anything asked");

    // A player's window: playing, it takes an inhibitor on it.
    let player = c.client(Spec::new("org.example.Player", 0x2255aa).title("A Film"));
    let window = c.wait_window("org.example.Player");
    player.send(Cmd::InhibitIdle(true));
    let playing = |title: &str| IdleInhibitorInfo {
        kind: IdleHolder::App,
        window: Some(window.id),
        app_id: Some("org.example.Player".into()),
        title: Some(title.into()),
        pid: me(),
    };
    wait("the player's hold", || inhibitors(&c) == [playing("A Film")]);
    wait("word of it", || heard.holders().last() == Some(&vec![playing("A Film")]));

    // A surface that's no window's is named by its process alone, after
    // the player: holds are listed in the order they began.
    let bare = Bare::hold(&c);
    let unshown = IdleInhibitorInfo { kind: IdleHolder::App, pid: me(), ..Default::default() };
    wait("the bare surface's hold", || inhibitors(&c) == [playing("A Film"), unshown.clone()]);
    wait("word of it", || heard.holders().last() == Some(&vec![playing("A Film"), unshown.clone()]));

    // The next film: asked, the compositor says the new title at once,
    // and subscribers hear it when the idle timer next looks.
    player.send(Cmd::Title("Another Film".into()));
    wait("the new title", || inhibitors(&c) == [playing("Another Film"), unshown.clone()]);
    wait("word of the new title", || heard.holders().last() == Some(&vec![playing("Another Film"), unshown.clone()]));

    // Paused, the player lets go; playing again, it holds again, now
    // after the bare surface.
    player.send(Cmd::InhibitIdle(false));
    wait("the player let go", || inhibitors(&c) == [unshown.clone()]);
    wait("word of it", || heard.holders().last() == Some(&vec![unshown.clone()]));
    player.send(Cmd::InhibitIdle(true));
    wait("the player holds again", || inhibitors(&c) == [unshown.clone(), playing("Another Film")]);

    // Its window closes with the inhibitor still on it, and the other
    // client is killed: neither is left holding.
    drop(player);
    wait("the closed window's hold gone", || inhibitors(&c) == [unshown.clone()]);
    bare.die();
    wait("the killed client's hold gone", || inhibitors(&c).is_empty());
    wait("word that nobody holds", || heard.holders().last() == Some(&Vec::new()));
    // Nothing was said twice over.
    let said = heard.holders();
    assert!(said.windows(2).all(|pair| pair[0] != pair[1]), "an unchanged answer was sent again: {said:#?}");
}

#[test]
fn an_apps_hold_keeps_the_displays_on_until_the_session_is_locked() {
    let c = start();
    let heard = Listener::start(&c);
    let player = c.client(Spec::new("org.example.Player", 0x2255aa).title("A Film"));
    c.wait_window("org.example.Player");
    player.send(Cmd::InhibitIdle(true));
    wait("the player's hold", || inhibitors(&c).len() == 1);

    // With someone at the screen, as before: an hour of a film, and
    // nothing locks or goes dark.
    left_alone_for(&c, 60);
    assert!(displays_on(&c), "the displays went off under a playing video");
    assert_eq!(heard.idle(), Vec::<String>::new(), "idle under a playing video");

    // Locked by hand, the film playing on behind the lock.
    let lock = LockClient::spawn(&c, 0x101010);
    wait("locked", || *lock.locked.lock().unwrap() == Some(true));
    assert_eq!(inhibitors(&c).len(), 1, "the player holds on, locked");
    // The displays' own timer runs: on at one minute, off at two.
    left_alone_for(&c, 1);
    assert!(displays_on(&c), "the displays went off before their time");
    left_alone_for(&c, 2);
    assert!(!displays_on(&c), "a playing video kept a locked screen lit");
    wait("word that they went off", || heard.idle().iter().any(|kind| kind == "monitors-off"));

    // Someone comes back: a key lights them, and once the session is
    // unlocked the player holds them on again.
    tap(&c, "shift");
    wait("the displays lit again", || displays_on(&c));
    lock.unlock();
    wait("unlocked", || heard.lock_states().last() == Some(&false));
    left_alone_for(&c, 60);
    assert!(displays_on(&c), "unlocked again, the playing video didn't hold");

    // Paused, it lets idleness come as ever.
    player.send(Cmd::InhibitIdle(false));
    wait("the player let go", || inhibitors(&c).is_empty());
    left_alone_for(&c, 2);
    assert!(!displays_on(&c), "nothing held, and the displays stayed on");
}

#[test]
fn keep_awakes_hold_counts_locked_or_not_and_ends_with_its_connection() {
    let c = start();
    let heard = Listener::start(&c);

    // Keep Awake's service asks on a connection it keeps.
    let mut service = Client::connect_to(&c.ipc).expect("IPC connect");
    assert_eq!(service.request(&Request::KeepAwake).expect("keep-awake"), Response::Ok);
    let keep_awake = IdleInhibitorInfo { kind: IdleHolder::KeepAwake, pid: me(), ..Default::default() };
    assert_eq!(inhibitors(&c), std::slice::from_ref(&keep_awake));
    wait("word of it", || heard.holders().last() == Some(&vec![keep_awake.clone()]));
    // Asked again on the same connection, it's still one hold.
    assert_eq!(service.request(&Request::KeepAwake).expect("keep-awake"), Response::Ok);
    assert_eq!(inhibitors(&c), std::slice::from_ref(&keep_awake));

    left_alone_for(&c, 60);
    assert!(displays_on(&c), "the displays went off with Keep Awake on");

    // Locked by hand: the user's own switch still holds, where an app's
    // inhibitor doesn't.
    let lock = LockClient::spawn(&c, 0x101010);
    wait("locked", || *lock.locked.lock().unwrap() == Some(true));
    left_alone_for(&c, 60);
    assert!(displays_on(&c), "Keep Awake let a locked screen go dark");
    assert_eq!(heard.idle(), Vec::<String>::new(), "idle with Keep Awake on");

    // An app's hold beside it changes nothing, locked.
    let bare = Bare::hold(&c);
    wait("both hold", || inhibitors(&c).len() == 2);
    left_alone_for(&c, 60);
    assert!(displays_on(&c), "Keep Awake stopped holding beside an app's hold");

    // The service stops, or dies: its connection closes and the hold goes
    // with it. The app's is all that's left, and locked it doesn't count.
    drop(service);
    wait("Keep Awake's hold gone", || inhibitors(&c).iter().all(|i| i.kind == IdleHolder::App));
    wait("word of it", || heard.holders().last().is_some_and(|l| l.len() == 1 && l[0].kind == IdleHolder::App));
    left_alone_for(&c, 2);
    assert!(!displays_on(&c), "the displays stayed on after Keep Awake went");
    bare.die();
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Nothing {
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
        impl Dispatch<$t, ()> for Nothing {
            fn event(_: &mut Self, _: &$t, _: <$t as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    wl_compositor::WlCompositor,
    wl_surface::WlSurface,
    zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1,
    zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1
);
