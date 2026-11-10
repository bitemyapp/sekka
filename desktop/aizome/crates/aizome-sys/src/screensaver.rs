//! Apps that keep the screen awake by asking the session bus:
//! `org.freedesktop.ScreenSaver`, which VLC calls, Firefox calls first,
//! and the GTK portal calls for `org.freedesktop.portal.Inhibit`. A
//! Wayland video player asks the compositor instead
//! (`zwp_idle_inhibit_manager_v1`: Utsushi, mpv); these ask whoever has
//! this name, and with nobody there they play on while the screen locks.
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
//! The other methods of the interface (`GetActive`, `Lock`,
//! `SimulateUserActivity`…) aren't served: callers hear
//! `UnknownMethod`, as from a screen saver without them.
//!
//! Without a session bus, or when another service has the name and won't
//! give it up (or isn't ours to replace, [`Takeover`]),
//! [`ScreenSaver::available`] is false and there are no holds.

use crate::bus::{self, Bus, Takeover};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Wake, lock, service_handle};
use futures_lite::StreamExt;
use std::collections::{BTreeMap, VecDeque};
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

/// How many connections are remembered as having left. The bus never
/// gives a unique name twice, and a call is read within moments of its
/// connection's leaving, so the last few are enough.
const LEFT_KEPT: usize = 256;

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

/// The holds in force, oldest first.
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

/// The holds, by cookie. A cookie is given once: the next goes on from
/// the last, past any still held.
#[derive(Debug, Default)]
pub struct Holds {
    last: u32,
    held: BTreeMap<Cookie, Hold>,
    /// The connections last seen leaving. Calls and the bus's word of a
    /// connection's leaving are read apart, so an app that takes a hold
    /// and leaves at once may be seen leaving first.
    left: VecDeque<Peer>,
}

impl Holds {
    /// `peer` takes a hold; none for a connection that has already left,
    /// which nobody would ever let go of.
    pub fn take(&mut self, peer: Peer, app: &str, reason: &str) -> Option<Cookie> {
        if self.left.contains(&peer) {
            return None;
        }
        let cookie = loop {
            self.last = self.last.wrapping_add(1);
            if let Some(c) = Cookie::new(self.last).filter(|c| !self.held.contains_key(c)) {
                break c;
            }
        };
        self.held.insert(cookie, Hold { cookie, peer, app: app.to_owned(), reason: reason.to_owned() });
        Some(cookie)
    }

    /// `peer` lets go of the hold it took as `cookie`. Another
    /// connection's cookie, or one already given back, changes nothing.
    pub fn release(&mut self, peer: &Peer, cookie: Cookie) -> Option<Hold> {
        if self.held.get(&cookie).is_some_and(|h| h.peer == *peer) { self.held.remove(&cookie) } else { None }
    }

    /// `peer` left the bus: its holds go with it, and it takes no more.
    pub fn peer_left(&mut self, peer: Peer) -> Vec<Hold> {
        let cookies: Vec<Cookie> = self.held.values().filter(|h| h.peer == peer).map(|h| h.cookie).collect();
        if self.left.len() == LEFT_KEPT {
            self.left.pop_front();
        }
        self.left.push_back(peer);
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
    fn inhibit(
        &self,
        application_name: String,
        reason_for_inhibit: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<u32> {
        let peer = caller(&header)?;
        let taken = self.0.change(|holds| holds.take(peer.clone(), &application_name, &reason_for_inhibit));
        let cookie = taken.ok_or_else(|| fdo::Error::Failed("the caller has left the bus".into()))?;
        log::info!(
            "aizome-sys: {application_name:?} ({}) keeps the screen awake: {reason_for_inhibit:?}",
            peer.as_str()
        );
        Ok(cookie.get())
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
        let table = Table { holds: Arc::default(), model: saver.model.clone() };
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

    fn peer(name: &str) -> Peer {
        Peer::new(name).expect("a unique name")
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
        let vlc = holds.take(peer(":1.7"), "vlc", "Playing some media.").unwrap();
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
        let first = holds.take(peer(":1.7"), "a", "").unwrap();
        let second = holds.take(peer(":1.7"), "a", "").unwrap();
        let other = holds.take(peer(":1.9"), "b", "").unwrap();
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
        let cookie = holds.take(peer(":1.7"), "player", "").unwrap();
        assert_eq!(holds.release(&peer(":1.8"), cookie), None);
        assert!(holds.state().held());
    }

    #[test]
    fn a_connection_that_leaves_takes_its_holds_with_it() {
        let mut holds = Holds::default();
        holds.take(peer(":1.7"), "crashed", "one").unwrap();
        holds.take(peer(":1.7"), "crashed", "two").unwrap();
        let kept = holds.take(peer(":1.9"), "playing", "").unwrap();
        assert_eq!(holds.peer_left(peer(":1.7")).len(), 2);
        assert_eq!(holds.state().holds.iter().map(|h| h.cookie).collect::<Vec<_>>(), [kept]);
        assert!(holds.peer_left(peer(":1.7")).is_empty());
    }

    #[test]
    fn a_connection_seen_leaving_first_takes_no_hold() {
        let mut holds = Holds::default();
        // Its call is read after the bus's word that it left.
        holds.peer_left(peer(":1.7"));
        assert_eq!(holds.take(peer(":1.7"), "a script", "one call"), None);
        assert!(!holds.state().held());
        assert!(holds.take(peer(":1.8"), "a player", "").is_some(), "others still do");
        // Only the last few are remembered: enough, as no name comes twice.
        for n in 0..LEFT_KEPT {
            holds.peer_left(peer(&format!(":2.{n}")));
        }
        assert_eq!(holds.left.len(), LEFT_KEPT);
        assert!(!holds.left.contains(&peer(":1.7")));
    }

    #[test]
    fn a_cookie_is_not_given_again_while_held_nor_ever_zero() {
        let mut holds = Holds { last: u32::MAX - 1, ..Holds::default() };
        let last = holds.take(peer(":1.7"), "a", "").unwrap();
        assert_eq!(last.get(), u32::MAX);
        let one = Cookie::new(1).unwrap();
        // Round the end: past 0, and past the one still held.
        holds.held.insert(one, Hold { cookie: one, peer: peer(":1.7"), app: "a".into(), reason: String::new() });
        assert_eq!(holds.take(peer(":1.7"), "a", "").unwrap().get(), 2);
    }

    #[test]
    fn forgotten_holds_do_not_lend_their_cookies_to_new_ones() {
        let mut holds = Holds::default();
        let old = holds.take(peer(":1.7"), "player", "").unwrap();
        holds.forget_all();
        assert!(!holds.state().held());
        let new = holds.take(peer(":1.9"), "another", "").unwrap();
        assert_ne!(new, old, "the first player's late UnInhibit must not match");
        assert_eq!(holds.release(&peer(":1.7"), old), None);
        assert!(holds.state().held());
    }
}
