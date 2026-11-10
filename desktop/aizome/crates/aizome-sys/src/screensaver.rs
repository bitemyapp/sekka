//! Apps that keep the screen awake by asking the session bus:
//! `org.freedesktop.ScreenSaver`, which VLC calls, Firefox calls first,
//! Chromium and Chrome call beside their Wayland inhibitor (and alone, as
//! X11 clients), and the GTK portal calls for
//! `org.freedesktop.portal.Inhibit`. A Wayland video player asks the
//! compositor instead (`zwp_idle_inhibit_manager_v1`: Utsushi, mpv); these
//! ask whoever has this name, and with nobody there they play on while the
//! screen locks.
//!
//! [`ScreenSaver::start`] serves the interface at the specification's
//! path and at KDE's `/ScreenSaver`, which VLC calls, and takes the name.
//! `Inhibit` gives a cookie for a hold and `UnInhibit` takes it back. A
//! hold is its connection's: only the connection that took it lets it go,
//! and when the connection leaves the bus (the app quit, or crashed) its
//! holds go with it. The [`snapshot`](ScreenSaver::snapshot) lists the
//! holds in force, so whoever serves this (the shell) holds idleness off
//! while there are any. If another service takes the name, the holds are
//! forgotten: apps would give their cookies back to that one.
//!
//! A hold is recorded before checking whether its caller still owns its
//! unique bus name. A disconnect already consumed by the watcher is then
//! caught by the check; one arriving later removes the recorded hold. No
//! history of departed connections is needed, even for delayed calls.
//!
//! Cookies begin somewhere different each time the service starts. An app
//! keeps its cookies when the shell starts again and gives them back to
//! the new service, where they must name none of its holds. A connection
//! may have 256 holds at once, and an app's name and reason are kept, and
//! logged, to their first 200 characters.
//!
//! The other methods of the interface (`GetActive`, `Lock`,
//! `SimulateUserActivity`…) aren't served: callers hear
//! `UnknownMethod`, as from a screen saver without them.
//!
//! Without a session bus, or when another service has the name and won't
//! give it up (or isn't ours to replace, [`Takeover`]),
//! [`ScreenSaver::available`] is false and there are no holds.

use crate::bus::{self, Bus, Opts, Takeover};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Wake, lock, service_handle};
use futures_lite::StreamExt;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};

/// The bus name, and the two paths apps call it at.
pub const NAME: &str = "org.freedesktop.ScreenSaver";
pub const PATH: &str = "/org/freedesktop/ScreenSaver";
pub const KDE_PATH: &str = "/ScreenSaver";
const DBUS: &str = "org.freedesktop.DBus";

/// Names changing hands: connections leaving the bus among them, and this
/// name going to another service.
const OWNER_RULE: &str = "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',\
                          member='NameOwnerChanged'";

/// How many holds one connection may have at once. A player has one or
/// two; the GTK portal holds for every app that asks it, on its one
/// connection. Past this a caller is refused rather than let fill the
/// table, which is copied for the shell on every change.
const MAX_HOLDS_PER_PEER: usize = 256;

/// What's kept, and logged, of an app's name and of its reason.
const MAX_TEXT: usize = 200;

/// What `Inhibit` gives for a hold and `UnInhibit` takes back. Never 0,
/// which callers keep for "no hold".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cookie(NonZeroU32);

impl Cookie {
    /// The cookie an app sent back, unless it's the 0 no hold has.
    pub fn new(raw: u32) -> Option<Cookie> {
        NonZeroU32::new(raw).map(Cookie)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// A connection on the bus, by its unique name (`:1.42`): who holds.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Peer(String);

impl Peer {
    /// The connection named `name`, when that is a unique name. A
    /// well-known name isn't a connection: it can change hands.
    pub fn new(name: &str) -> Option<Peer> {
        name.starts_with(':').then(|| Peer(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One hold: who took it, and what they said of themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub cookie: Cookie,
    pub peer: Peer,
    /// The app's name for itself ("vlc").
    pub app: String,
    /// Why ("Playing some media.").
    pub reason: String,
}

/// The holds in force, in the order of their cookies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenSaverState {
    pub holds: Vec<Hold>,
}

impl ScreenSaverState {
    /// Whether any app holds the screen awake.
    pub fn held(&self) -> bool {
        !self.holds.is_empty()
    }
}

/// Why a hold wasn't given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The connection has as many holds as one may.
    TooMany,
}

/// The holds, by cookie. A cookie is given once: the next goes on from
/// the last, past any still held.
#[derive(Debug, Default)]
pub struct Holds {
    last: u32,
    held: BTreeMap<Cookie, Hold>,
}

impl Holds {
    /// No holds, and cookies that go on from `seed`. An app keeps the
    /// cookies a service gave it and may give them back to the next one
    /// (the shell started again while a video played), so each service
    /// starts somewhere of its own: an old cookie then names no new hold.
    pub fn starting_after(seed: u32) -> Holds {
        Holds { last: seed, ..Holds::default() }
    }

    /// `peer` takes a hold, and gets it as it's kept.
    pub fn take(&mut self, peer: Peer, app: &str, reason: &str) -> Result<Hold, Refused> {
        if self.held.values().filter(|h| h.peer == peer).count() >= MAX_HOLDS_PER_PEER {
            return Err(Refused::TooMany);
        }
        let cookie = loop {
            self.last = self.last.wrapping_add(1);
            if let Some(c) = Cookie::new(self.last).filter(|c| !self.held.contains_key(c)) {
                break c;
            }
        };
        let hold = Hold { cookie, peer, app: kept(app), reason: kept(reason) };
        self.held.insert(cookie, hold.clone());
        Ok(hold)
    }

    /// `peer` lets go of the hold it took as `cookie`. Another
    /// connection's cookie, or one already given back, changes nothing.
    pub fn release(&mut self, peer: &Peer, cookie: Cookie) -> Option<Hold> {
        if self.held.get(&cookie).is_some_and(|h| h.peer == *peer) { self.held.remove(&cookie) } else { None }
    }

    /// `peer` left the bus: its holds go with it. The served interface
    /// checks the bus for callers whose disconnect preceded their call.
    pub fn peer_left(&mut self, peer: Peer) -> Vec<Hold> {
        let cookies: Vec<Cookie> = self.held.values().filter(|h| h.peer == peer).map(|h| h.cookie).collect();
        cookies.iter().filter_map(|c| self.held.remove(c)).collect()
    }

    /// Nobody can give a cookie back any more: no hold stays. Cookies
    /// still go on from the last.
    pub fn forget_all(&mut self) {
        self.held.clear();
    }

    /// The holds in force.
    pub fn state(&self) -> ScreenSaverState {
        ScreenSaverState { holds: self.held.values().cloned().collect() }
    }
}

/// An app's words, no longer than is worth keeping.
fn kept(text: &str) -> String {
    text.chars().take(MAX_TEXT).collect()
}

/// Where a service's cookies begin: somewhere else each time one starts.
fn seed() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    // The standard library's randomly keyed hasher, with nothing to hash.
    std::collections::hash_map::RandomState::new().build_hasher().finish() as u32
}

/// The holds and what the shell reads of them, changed together.
#[derive(Clone)]
struct Table {
    holds: Arc<Mutex<Holds>>,
    model: Arc<Model<ScreenSaverState>>,
}

impl Table {
    /// Change the holds and publish them, in the order the changes
    /// were made.
    fn change<R>(&self, change: impl FnOnce(&mut Holds) -> R) -> R {
        let mut holds = lock(&self.holds);
        let result = change(&mut holds);
        self.model.set_if_changed(holds.state());
        result
    }
}

/// The served interface.
struct Server(Table);

fn caller(header: &zbus::message::Header<'_>) -> fdo::Result<Peer> {
    let sender = header.sender().and_then(|s| Peer::new(s.as_str()));
    sender.ok_or_else(|| fdo::Error::Failed("a call without its sender".into()))
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl Server {
    async fn inhibit(
        &self,
        application_name: String,
        reason_for_inhibit: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<u32> {
        let peer = caller(&header)?;
        let taken = self.0.change(|holds| holds.take(peer, &application_name, &reason_for_inhibit));
        let hold = taken.map_err(|refused| match refused {
            Refused::TooMany => fdo::Error::LimitsExceeded(format!(
                "this connection has {MAX_HOLDS_PER_PEER} holds already: give one back with UnInhibit"
            )),
        })?;
        // Register first, then check: checking before registration would
        // let a disconnect between the two leave an orphaned hold. Unique
        // names are never reused. A failed/timed-out check fails closed.
        let bus = Bus::from_connection(connection.clone());
        let error = match bus
            .call_async::<_, bool>(
                Opts::QUERY,
                DBUS,
                "/org/freedesktop/DBus",
                DBUS,
                "NameHasOwner",
                &(hold.peer.as_str(),),
            )
            .await
        {
            Ok(true) => None,
            Ok(false) => Some("the caller has left the bus".to_owned()),
            Err(error) => Some(format!("could not check the caller: {error}")),
        };
        if let Some(error) = error {
            self.0.change(|holds| holds.release(&hold.peer, hold.cookie));
            return Err(fdo::Error::Failed(error));
        }
        log::info!("aizome-sys: {:?} ({}) keeps the screen awake: {:?}", hold.app, hold.peer.as_str(), hold.reason);
        Ok(hold.cookie.get())
    }

    fn un_inhibit(&self, cookie: u32, #[zbus(header)] header: zbus::message::Header<'_>) -> fdo::Result<()> {
        let peer = caller(&header)?;
        let released = Cookie::new(cookie).and_then(|cookie| self.0.change(|holds| holds.release(&peer, cookie)));
        if let Some(hold) = released {
            log::info!("aizome-sys: {:?} ({}) lets the screen sleep", hold.app, peer.as_str());
        }
        Ok(())
    }
}

/// Apps' holds on the screen; dropping it stops serving them.
pub struct ScreenSaver {
    bus: Option<Bus>,
    model: Arc<Model<ScreenSaverState>>,
    stop: Stop,
    /// The objects are ours to remove, the name ours to release.
    serving: Vec<&'static str>,
    named: bool,
}

service_handle!(ScreenSaver, ScreenSaverState);

impl ScreenSaver {
    /// Serve the interface on the session bus; without one, or when the
    /// name can't be had, there are no holds.
    pub fn new(wake: Wake, takeover: Takeover) -> ScreenSaver {
        let started = Bus::session().and_then(|bus| ScreenSaver::start(&bus, wake.clone(), takeover));
        started.unwrap_or_else(|error| {
            log::info!("aizome-sys: apps can't keep the screen awake over the session bus: {error}");
            ScreenSaver::none(wake)
        })
    }

    fn none(wake: Wake) -> ScreenSaver {
        let model = Arc::new(Model::new(ScreenSaverState::default(), wake));
        ScreenSaver { bus: None, model, stop: Stop::default(), serving: Vec::new(), named: false }
    }

    /// Serve `org.freedesktop.ScreenSaver` on `bus` and take its name,
    /// from a service that allows it (a previous shell's) as `takeover`
    /// permits, allowing the next to take it in turn. Fails with
    /// [`Error::Unavailable`] when another service keeps it.
    pub fn start(bus: &Bus, wake: Wake, takeover: Takeover) -> Result<ScreenSaver> {
        let mut saver = ScreenSaver::none(wake);
        saver.bus = Some(bus.clone());
        let holds = Arc::new(Mutex::new(Holds::starting_after(seed())));
        let table = Table { holds, model: saver.model.clone() };
        let conn = bus.connection().clone();

        // Follow first, so no connection's leaving is missed.
        follow_owners(bus.clone(), table.clone(), saver.stop.clone())?;

        for path in [PATH, KDE_PATH] {
            if !zbus::block_on(conn.object_server().at(path, Server(table.clone())))? {
                return Err(Error::Unavailable("this connection already serves a screen saver".into()));
            }
            saver.serving.push(path);
            bus::wait_until_serving(bus, path)?;
        }
        let mut flags = RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement;
        let owner = if matches!(takeover, Takeover::From(_)) { bus.owner_pid(NAME) } else { None };
        if takeover.replaces(owner) {
            flags |= RequestNameFlags::ReplaceExisting;
        }
        match zbus::block_on(conn.request_name_with_flags(NAME, flags)) {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
                saver.named = true;
                Ok(saver)
            }
            Ok(RequestNameReply::InQueue | RequestNameReply::Exists) | Err(zbus::Error::NameTaken) => {
                Err(Error::Unavailable(format!("another service has {NAME}")))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Whether apps can take holds with it.
    pub fn available(&self) -> bool {
        self.named
    }
}

impl Drop for ScreenSaver {
    fn drop(&mut self) {
        self.stop.stop();
        let Some(bus) = &self.bus else { return };
        let conn = bus.connection().clone();
        if self.named {
            let _ = zbus::block_on(conn.release_name(NAME));
        }
        for path in self.serving.drain(..) {
            let _ = zbus::block_on(conn.object_server().remove::<Server, _>(path));
        }
    }
}

/// Follow names changing hands, on a thread: a connection that leaves
/// lets go of its holds, and when the name goes to another service (or
/// the bus goes) none is left. Returns once the signals are subscribed,
/// so no connection's leaving after is missed.
fn follow_owners(bus: Bus, table: Table, stop: Stop) -> Result<()> {
    let (ready, subscribed) = mpsc::channel::<Result<()>>();
    std::thread::Builder::new()
        .name("aizome-sys screen saver".into())
        .spawn(move || {
            zbus::block_on(async move {
                let subscribe = async {
                    let rule = zbus::MatchRule::try_from(OWNER_RULE)?;
                    zbus::MessageStream::for_match_rule(rule, bus.connection(), Some(1024)).await
                };
                let mut stream = match subscribe.await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = ready.send(Err(error.into()));
                        return;
                    }
                };
                let _ = ready.send(Ok(()));
                let me = bus.unique_name();
                while let Some(msg) = stream.next().await {
                    if stop.stopped() {
                        return;
                    }
                    let Ok(msg) = msg else { continue };
                    // Only the bus says who left.
                    if msg.header().sender().is_none_or(|s| s.as_str() != DBUS) {
                        continue;
                    }
                    let Ok((name, _old, new)) = msg.body().deserialize::<(String, String, String)>() else { continue };
                    if !new.is_empty() {
                        if name == NAME && Some(&new) != me.as_ref() {
                            table.change(Holds::forget_all);
                        }
                    } else if let Some(peer) = Peer::new(&name) {
                        for hold in table.change(|holds| holds.peer_left(peer)) {
                            log::info!(
                                "aizome-sys: {:?} ({}) left the bus holding the screen awake",
                                hold.app,
                                hold.peer.as_str()
                            );
                        }
                    }
                }
                // No hold outlasts the bus it was asked for on.
                table.change(Holds::forget_all);
            })
        })
        .map_err(|error| Error::Failed(format!("could not start a thread: {error}")))?;
    subscribed.recv().map_err(|_| Error::Failed("the bus's signals stopped".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_bus::{TestBus, eventually};

    #[test]
    fn a_delayed_call_cannot_outlive_its_caller_after_connection_churn() {
        let bus = TestBus::start().expect("a private dbus-daemon");
        let connection = bus.bus();
        let saver = ScreenSaver::start(&connection, Wake::none(), Takeover::Any).unwrap();
        let sentinel = bus.builder().build().unwrap();
        sentinel.call_method(Some(NAME), KDE_PATH, Some(NAME), "Inhibit", &("sentinel", "barrier")).unwrap();

        let iface = zbus::block_on(connection.connection().object_server().interface::<_, Server>(PATH)).unwrap();
        // Pause only method dispatch, leaving the independent owner watcher
        // free to consume disconnects while the call waits.
        let guard = zbus::block_on(iface.get_mut());
        let app = bus.builder().build().unwrap();
        let sender = app.unique_name().unwrap().to_owned();
        let call = zbus::Message::method_call(PATH, "Inhibit")
            .unwrap()
            .destination(NAME)
            .unwrap()
            .interface(NAME)
            .unwrap()
            .build(&("departed caller", "delayed call"))
            .unwrap();
        app.send(&call).unwrap();
        drop(app);
        let daemon = zbus::blocking::fdo::DBusProxy::new(&sentinel).unwrap();
        assert!(eventually(|| !daemon.name_has_owner(sender.clone().into()).unwrap()));
        // More than the former 256-entry disconnect history. Check that each
        // disconnect has reached the daemon before dropping the sentinel.
        for _ in 0..512 {
            let other = bus.builder().build().unwrap();
            let name = other.unique_name().unwrap().to_owned();
            drop(other);
            assert!(eventually(|| !daemon.name_has_owner(name.clone().into()).unwrap()));
        }
        drop(daemon);
        drop(sentinel);
        // Its hold disappearing proves the owner watcher consumed all the
        // earlier disconnect signals, including the departed caller's.
        assert!(eventually(|| !saver.snapshot().held()));
        let before = saver.generation();
        drop(guard);
        assert!(eventually(|| saver.generation() > before), "the queued call never ran");
        // The handler holds an interface read lock through its async work.
        // Taking the write lock waits for it to finish, without a sleep.
        let _finished = zbus::block_on(iface.get_mut());
        assert!(!saver.snapshot().held(), "a departed caller left a hold: {:?}", saver.snapshot().holds);
    }

    #[test]
    fn a_failed_caller_check_releases_the_registered_hold() {
        let bus = TestBus::start().expect("a private dbus-daemon");
        let connection = bus.bus();
        let call = zbus::Message::method_call(PATH, "Inhibit")
            .unwrap()
            .sender(connection.unique_name().unwrap())
            .unwrap()
            .build(&())
            .unwrap();
        let table = Table {
            holds: Arc::new(Mutex::new(Holds::default())),
            model: Arc::new(Model::new(ScreenSaverState::default(), Wake::none())),
        };
        // No owner watcher in this test: the failed query itself must clean
        // up, rather than relying on a disconnect signal to do it later.
        zbus::block_on(connection.connection().clone().close()).unwrap();
        let result = zbus::block_on(Server(table.clone()).inhibit(
            "player".into(),
            "video".into(),
            call.header(),
            connection.connection(),
        ));
        assert!(result.is_err(), "an unverified caller was accepted");
        assert!(!table.model.get().held(), "a failed check left a hold");
    }

    fn peer(name: &str) -> Peer {
        Peer::new(name).expect("a unique name")
    }

    /// A hold for the connection `who`, by its cookie.
    fn take(holds: &mut Holds, who: &str, app: &str, reason: &str) -> Cookie {
        holds.take(peer(who), app, reason).expect("a hold").cookie
    }

    #[test]
    fn handles_go_between_threads() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<ScreenSaver>();
        send_sync::<ScreenSaverState>();
    }

    #[test]
    fn without_a_bus_there_is_nothing() {
        let saver = ScreenSaver::none(Wake::none());
        assert!(!saver.available());
        assert!(!saver.snapshot().held());
    }

    #[test]
    fn only_a_connection_holds() {
        assert!(Peer::new(":1.42").is_some());
        assert!(Peer::new("org.videolan.VLC").is_none(), "a well-known name can change hands");
        assert!(Peer::new("").is_none());
        assert!(Cookie::new(0).is_none(), "0 is no hold");
    }

    #[test]
    fn a_hold_lasts_until_its_cookie_comes_back() {
        let mut holds = Holds::default();
        let vlc = take(&mut holds, ":1.7", "vlc", "Playing some media.");
        assert_eq!(
            holds.state().holds,
            [Hold { cookie: vlc, peer: peer(":1.7"), app: "vlc".into(), reason: "Playing some media.".into() }]
        );
        assert!(holds.state().held());
        assert_eq!(holds.release(&peer(":1.7"), vlc).map(|h| h.app), Some("vlc".into()));
        assert!(!holds.state().held());
        assert_eq!(holds.release(&peer(":1.7"), vlc), None, "given back once");
    }

    #[test]
    fn each_hold_is_its_own() {
        let mut holds = Holds::default();
        let first = take(&mut holds, ":1.7", "a", "");
        let second = take(&mut holds, ":1.7", "a", "");
        let other = take(&mut holds, ":1.9", "b", "");
        assert!(first != second && second != other && first != other);
        holds.release(&peer(":1.7"), first);
        assert!(holds.state().held(), "the same app's second hold and another's remain");
        holds.release(&peer(":1.7"), second);
        holds.release(&peer(":1.9"), other);
        assert!(!holds.state().held());
    }

    #[test]
    fn another_connections_cookie_lets_nothing_go() {
        let mut holds = Holds::default();
        let cookie = take(&mut holds, ":1.7", "player", "");
        assert_eq!(holds.release(&peer(":1.8"), cookie), None);
        assert!(holds.state().held());
    }

    #[test]
    fn a_connection_that_leaves_takes_its_holds_with_it() {
        let mut holds = Holds::default();
        take(&mut holds, ":1.7", "crashed", "one");
        take(&mut holds, ":1.7", "crashed", "two");
        let kept = take(&mut holds, ":1.9", "playing", "");
        assert_eq!(holds.peer_left(peer(":1.7")).len(), 2);
        assert_eq!(holds.state().holds.iter().map(|h| h.cookie).collect::<Vec<_>>(), [kept]);
        assert!(holds.peer_left(peer(":1.7")).is_empty());
    }

    #[test]
    fn a_cookie_is_not_given_again_while_held_nor_ever_zero() {
        let mut holds = Holds::starting_after(u32::MAX - 1);
        assert_eq!(take(&mut holds, ":1.7", "a", "").get(), u32::MAX);
        let one = Cookie::new(1).unwrap();
        // Round the end: past 0, and past the one still held.
        holds.held.insert(one, Hold { cookie: one, peer: peer(":1.7"), app: "a".into(), reason: String::new() });
        assert_eq!(take(&mut holds, ":1.7", "a", "").get(), 2);
    }

    #[test]
    fn forgotten_holds_do_not_lend_their_cookies_to_new_ones() {
        let mut holds = Holds::default();
        let old = take(&mut holds, ":1.7", "player", "");
        holds.forget_all();
        assert!(!holds.state().held());
        let new = take(&mut holds, ":1.9", "another", "");
        assert_ne!(new, old, "the first player's late UnInhibit must not match");
        assert_eq!(holds.release(&peer(":1.7"), old), None);
        assert!(holds.state().held());
    }

    #[test]
    fn a_cookie_from_the_service_before_names_no_hold_of_the_next() {
        // The shell started again while Firefox played: it still has the
        // two cookies the old shell's service gave it.
        let mut old = Holds::starting_after(40);
        let audio = take(&mut old, ":1.7", "firefox", "Playing audio");
        let video = take(&mut old, ":1.7", "firefox", "Playing video");
        let mut new = Holds::starting_after(9000);
        // The video pauses and plays again, then the sound stops.
        assert_eq!(new.release(&peer(":1.7"), video), None);
        let again = take(&mut new, ":1.7", "firefox", "Playing video");
        assert_eq!(new.release(&peer(":1.7"), audio), None, "an old cookie let a new hold go");
        assert_eq!(new.state().holds.iter().map(|h| h.cookie).collect::<Vec<_>>(), [again]);
        // Starting from the same place it would: what the seed is for.
        let mut same = Holds::starting_after(40);
        take(&mut same, ":1.7", "firefox", "Playing video");
        assert!(same.release(&peer(":1.7"), audio).is_some());
    }

    #[test]
    fn each_start_has_a_seed_of_its_own() {
        let seeds: std::collections::BTreeSet<u32> = (0..16).map(|_| seed()).collect();
        assert!(seeds.len() > 1, "every service would start at {seeds:?}");
    }

    #[test]
    fn one_connection_cannot_fill_the_table() {
        let mut holds = Holds::default();
        let first = take(&mut holds, ":1.7", "greedy", "");
        for _ in 1..MAX_HOLDS_PER_PEER {
            take(&mut holds, ":1.7", "greedy", "");
        }
        assert_eq!(holds.take(peer(":1.7"), "greedy", ""), Err(Refused::TooMany));
        assert_eq!(holds.state().holds.len(), MAX_HOLDS_PER_PEER);
        // Another connection isn't kept out by it, and giving one back
        // makes room.
        assert!(holds.take(peer(":1.9"), "player", "").is_ok());
        holds.release(&peer(":1.7"), first);
        assert!(holds.take(peer(":1.7"), "greedy", "").is_ok());
    }

    #[test]
    fn long_words_are_cut_where_they_are_kept() {
        let mut holds = Holds::default();
        let hold = holds.take(peer(":1.7"), &"a".repeat(5000), &"é".repeat(5000)).unwrap();
        assert_eq!(hold.app.chars().count(), MAX_TEXT);
        assert_eq!(hold.reason, "é".repeat(MAX_TEXT));
        assert_eq!(holds.state().holds, [hold]);
    }
}
