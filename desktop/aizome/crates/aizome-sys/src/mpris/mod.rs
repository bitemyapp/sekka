//! Media players, through MPRIS (the Media Player Remote Interfacing
//! Specification, version 2.2): what is playing, for the menu bar's media
//! widget, and the media keys.
//!
//! [`Mpris`] follows every player on the session bus, each of which owns a
//! name starting with `org.mpris.MediaPlayer2.` and serves
//! `/org/mpris/MediaPlayer2`. One thread follows them all (with a second
//! that only takes the signals off zbus's queues, as the other services'
//! followers do):
//! it lists the names at start, hears players come and go from the bus's
//! `NameOwnerChanged`, and matches each `PropertiesChanged` and `Seeked`
//! signal to its player by the sender's unique name.
//!
//! For each [`Player`] the snapshot has its identity (`Identity`,
//! `DesktopEntry`), the `org.mpris.MediaPlayer2.Player` properties and the
//! current track's [`Metadata`]. Players don't signal their position as it
//! moves: it is read when the playback status, the track or the rate
//! changes, reset by `Seeked`, and [`Player::position`] estimates it from
//! there while playing. [`Mpris::refresh_position`] reads it again, for
//! when a position slider appears.
//!
//! [`MprisState::active`] is the player for the menu bar and the media
//! keys: one that is playing (the latest to start, when several are), else
//! the one that played last, else the one that changed last. The `media_*`
//! actions act on it.
//!
//! Players send odd data: integers of another width, an artist as a string
//! rather than a list, missing keys, unknown status strings. Everything is
//! read leniently; a player that misbehaves only shows less. A player that
//! hangs delays the others' updates by the query timeout (2 seconds) when
//! it appears or changes state, since the following thread reads players
//! one at a time.
//!
//! Needs a session bus; without one, [`Mpris::available`] is false and
//! there are no players.

mod player;

pub use player::{LoopStatus, Metadata, PlaybackStatus, Player};

use crate::bus::{self, Bus, Opts, Props, PropsExt, value_i64};
use crate::error::{Error, Result};
use crate::service::{Model, Stop, Task, Wake, lock, service_handle};
use futures_lite::StreamExt;
use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::message::Message;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// The prefix of every player's bus name.
pub const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const IFACE_ROOT: &str = "org.mpris.MediaPlayer2";
const IFACE_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const IFACE_PROPS: &str = "org.freedesktop.DBus.Properties";
const DBUS: &str = "org.freedesktop.DBus";

const OWNER_RULE: &str = "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',\
                          member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'";
const PROPS_RULE: &str = "type='signal',interface='org.freedesktop.DBus.Properties',\
                          member='PropertiesChanged',path='/org/mpris/MediaPlayer2'";
const SEEKED_RULE: &str = "type='signal',interface='org.mpris.MediaPlayer2.Player',\
                           member='Seeked',path='/org/mpris/MediaPlayer2'";

/// Commands: players answer at once or not at all, though raising a window
/// may take a moment.
const ACTION: Opts = Opts { timeout: Duration::from_secs(5), interactive: false };

/// The media players on the session bus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MprisState {
    /// Every player, by identity, then bus name.
    pub players: Vec<Player>,
    /// The bus name of the player for the menu bar and the media keys.
    pub active: Option<String>,
}

impl MprisState {
    /// The player with this bus name.
    pub fn player(&self, bus_name: &str) -> Option<&Player> {
        self.players.iter().find(|p| p.bus_name == bus_name)
    }

    /// The player for the menu bar and the media keys.
    pub fn active_player(&self) -> Option<&Player> {
        self.player(self.active.as_deref()?)
    }

    /// Whether anything is playing.
    pub fn playing(&self) -> bool {
        self.players.iter().any(Player::is_playing)
    }
}

/// What the following thread knows of one player.
struct Tracked {
    owner: String,
    root: Props,
    player: Props,
    /// The position last read, in microseconds, and when.
    position_us: i64,
    position_at: Instant,
    played: u64,
    changed: u64,
    /// The position must be read again (the status, track or rate moved).
    stale_position: bool,
}

impl Tracked {
    fn status(&self) -> PlaybackStatus {
        PlaybackStatus::parse(&self.player.str("PlaybackStatus"))
    }

    /// Carry the position estimate up to `now` under the current state,
    /// before the state changes.
    fn rebase(&mut self, now: Instant) {
        let playing = self.status() == PlaybackStatus::Playing;
        let length = player::length_of(&self.player);
        self.position_us =
            player::estimate_us(self.position_us, self.position_at, now, playing, player::rate(&self.player), length);
        self.position_at = now;
    }

    /// Change the player properties (`touched` says which), keeping the
    /// position and the active-player counters right.
    fn change_player(&mut self, seq: u64, touched: impl Fn(&str) -> bool, change: impl FnOnce(&mut Props)) {
        let now = Instant::now();
        let was = self.status();
        self.rebase(now);
        change(&mut self.player);
        if let Some(us) = self.player.get("Position").and_then(|v| value_i64(v)).filter(|_| touched("Position")) {
            // Read with the rest, or announced (MPRIS doesn't ask players
            // to, but some do).
            self.position_us = us;
            self.position_at = now;
            self.stale_position = false;
        } else if touched("PlaybackStatus") || touched("Metadata") || touched("Rate") {
            self.stale_position = true;
        }
        let is = self.status();
        if was != is && (was == PlaybackStatus::Playing || is == PlaybackStatus::Playing) {
            self.played = seq;
        }
        self.changed = seq;
    }

    fn snapshot(&self, bus_name: &str) -> Player {
        Player::from_props(
            bus_name,
            &self.owner,
            &self.root,
            &self.player,
            self.position_us,
            self.position_at,
            self.played,
            self.changed,
        )
    }
}

/// Every player followed, by bus name.
#[derive(Default)]
struct Players {
    map: BTreeMap<String, Tracked>,
    /// Counts changes, to order them for the active-player choice.
    seq: u64,
}

impl Players {
    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn build(&self) -> MprisState {
        let mut players: Vec<Player> = self.map.iter().map(|(name, t)| t.snapshot(name)).collect();
        let active = player::choose_active(&players);
        players.sort_by(|a, b| {
            a.identity.to_lowercase().cmp(&b.identity.to_lowercase()).then_with(|| a.bus_name.cmp(&b.bus_name))
        });
        MprisState { players, active }
    }
}

/// Media players: their state, actions on them, and the media keys.
pub struct Mpris {
    bus: Option<Bus>,
    model: Arc<Model<MprisState>>,
    players: Arc<Mutex<Players>>,
    stop: Stop,
}

service_handle!(Mpris, MprisState);

impl Mpris {
    /// Follow the players on the session bus.
    pub fn new(wake: Wake) -> Mpris {
        match Bus::session() {
            Ok(bus) => Mpris::with_bus(bus, wake),
            Err(error) => {
                log::info!("aizome-sys: no session bus for media players: {error}");
                Mpris {
                    bus: None,
                    model: Arc::new(Model::new(MprisState::default(), wake)),
                    players: Arc::default(),
                    stop: Stop::default(),
                }
            }
        }
    }

    /// Follow the players on `bus`.
    pub fn with_bus(bus: Bus, wake: Wake) -> Mpris {
        let model = Arc::new(Model::new(MprisState::default(), wake));
        let players: Arc<Mutex<Players>> = Arc::default();
        let stop = Stop::default();
        follow_players(bus.clone(), players.clone(), model.clone(), stop.clone());
        Mpris { bus: Some(bus), model, players, stop }
    }

    /// Whether the session bus is there (players come and go on it).
    pub fn available(&self) -> bool {
        self.bus.is_some()
    }

    /// Run `work` against `player` on a thread; a player that isn't on the
    /// bus fails at once with [`Error::NotFound`].
    fn player_task<T: Send + 'static>(
        &self,
        player: &str,
        work: impl FnOnce(&Bus, &str) -> Result<T> + Send + 'static,
    ) -> Task<T> {
        if self.bus.is_some() && self.snapshot().player(player).is_none() {
            return Task::ready(Err(Error::NotFound(format!("no media player {player}"))));
        }
        let name = player.to_owned();
        bus::spawn(&self.bus, "media players", self.model.wake(), move |bus| work(bus, &name))
    }

    fn method(&self, player: &str, iface: &'static str, method: &'static str) -> Task<()> {
        self.player_task(player, move |bus, name| bus.call_with(ACTION, name, PATH, iface, method, &()))
    }

    fn set_property(&self, player: &str, property: &'static str, value: Value<'static>) -> Task<()> {
        self.player_task(player, move |bus, name| {
            bus.call_with(ACTION, name, PATH, IFACE_PROPS, "Set", &(IFACE_PLAYER, property, value))
        })
    }

    /// Play if paused, pause if playing.
    pub fn play_pause(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_PLAYER, "PlayPause")
    }

    /// Start or resume playback.
    pub fn play(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_PLAYER, "Play")
    }

    /// Pause playback.
    pub fn pause(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_PLAYER, "Pause")
    }

    /// Stop playback.
    pub fn stop(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_PLAYER, "Stop")
    }

    /// The next track.
    pub fn next(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_PLAYER, "Next")
    }

    /// The previous track (or the start of this one, as players decide).
    pub fn previous(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_PLAYER, "Previous")
    }

    /// Move forward (or back, when negative) by `offset_us` microseconds.
    pub fn seek(&self, player: &str, offset_us: i64) -> Task<()> {
        self.player_task(player, move |bus, name| {
            bus.call_with(ACTION, name, PATH, IFACE_PLAYER, "Seek", &(offset_us,))
        })
    }

    /// Go to `position` in the track `track_id` ([`Metadata::track_id`]);
    /// players ignore it when that track is no longer the current one.
    pub fn set_position(&self, player: &str, track_id: &str, position: Duration) -> Task<()> {
        let track = match OwnedObjectPath::try_from(track_id.to_owned()) {
            Ok(track) if track.as_str() != player::NO_TRACK => track,
            _ => return Task::ready(Err(Error::InvalidArgument(format!("not a track: {track_id:?}")))),
        };
        let position = i64::try_from(position.as_micros()).unwrap_or(i64::MAX);
        self.player_task(player, move |bus, name| {
            bus.call_with(ACTION, name, PATH, IFACE_PLAYER, "SetPosition", &(track, position))
        })
    }

    /// Set the volume (0.0 is silent, 1.0 full; negative values are 0).
    pub fn set_volume(&self, player: &str, volume: f64) -> Task<()> {
        if !volume.is_finite() {
            return Task::ready(Err(Error::InvalidArgument(format!("not a volume: {volume}"))));
        }
        self.set_property(player, "Volume", Value::from(volume.max(0.0)))
    }

    /// Turn shuffling on or off.
    pub fn set_shuffle(&self, player: &str, shuffle: bool) -> Task<()> {
        self.set_property(player, "Shuffle", Value::from(shuffle))
    }

    /// Set what repeats.
    pub fn set_loop_status(&self, player: &str, status: LoopStatus) -> Task<()> {
        self.set_property(player, "LoopStatus", Value::from(status.as_str()))
    }

    /// Bring the player's window to the front.
    pub fn raise(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_ROOT, "Raise")
    }

    /// Ask the player to quit.
    pub fn quit(&self, player: &str) -> Task<()> {
        self.method(player, IFACE_ROOT, "Quit")
    }

    /// Read the player's position again (as the menu opens, say), update
    /// the estimate in the snapshot, and return it.
    pub fn refresh_position(&self, player: &str) -> Task<Duration> {
        let (players, model) = (self.players.clone(), self.model.clone());
        self.player_task(player, move |bus, name| {
            let owner = lock(&players).map.get(name).map(|t| t.owner.clone());
            let owner = owner.ok_or_else(|| Error::NotFound(format!("no media player {name}")))?;
            let us = read_position(bus, &owner)?;
            let mut players = lock(&players);
            if let Some(tracked) = players.map.get_mut(name).filter(|t| t.owner == owner) {
                tracked.position_us = us;
                tracked.position_at = Instant::now();
                tracked.stale_position = false;
            }
            model.set_if_changed(players.build());
            Ok(Duration::from_micros(u64::try_from(us).unwrap_or(0)))
        })
    }

    fn active(&self) -> Result<Player> {
        self.snapshot().active_player().cloned().ok_or_else(|| Error::NotFound("no media player".into()))
    }

    /// The play/pause key: play or pause the active player (stop it, if
    /// it's playing something that can't pause, such as a live stream).
    pub fn media_play_pause(&self) -> Task<()> {
        match self.active() {
            Err(error) => Task::ready(Err(error)),
            Ok(p) if p.is_playing() && !p.can_pause => self.stop(&p.bus_name),
            Ok(p) => self.play_pause(&p.bus_name),
        }
    }

    /// The next-track key.
    pub fn media_next(&self) -> Task<()> {
        self.active().map_or_else(|e| Task::ready(Err(e)), |p| self.next(&p.bus_name))
    }

    /// The previous-track key.
    pub fn media_previous(&self) -> Task<()> {
        self.active().map_or_else(|e| Task::ready(Err(e)), |p| self.previous(&p.bus_name))
    }

    /// The stop key.
    pub fn media_stop(&self) -> Task<()> {
        self.active().map_or_else(|e| Task::ready(Err(e)), |p| self.stop(&p.bus_name))
    }

    /// The fast-forward and rewind keys: seek the active player by
    /// `offset_us` microseconds.
    pub fn media_seek(&self, offset_us: i64) -> Task<()> {
        self.active().map_or_else(|e| Task::ready(Err(e)), |p| self.seek(&p.bus_name, offset_us))
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        self.stop.stop();
    }
}

fn is_player_name(name: &str) -> bool {
    name.strip_prefix(BUS_PREFIX).is_some_and(|rest| !rest.is_empty())
}

fn read_position(bus: &Bus, owner: &str) -> Result<i64> {
    let value: OwnedValue = bus.get(owner, PATH, IFACE_PLAYER, "Position")?;
    value_i64(&value).ok_or_else(|| Error::Failed("the player's position is not a number".into()))
}

type Stream = Pin<Box<dyn futures_lite::Stream<Item = zbus::Result<Message>> + Send>>;

async fn subscribe(conn: &zbus::Connection, rule: &str) -> Result<zbus::MessageStream> {
    let rule = zbus::MatchRule::try_from(rule)?;
    Ok(zbus::MessageStream::for_match_rule(rule, conn, Some(1024)).await?)
}

/// One stream of players' names changing owner and of their signals.
async fn subscribe_all(conn: &zbus::Connection) -> Result<Stream> {
    let mut stream: Stream = Box::pin(subscribe(conn, OWNER_RULE).await?);
    for rule in [PROPS_RULE, SEEKED_RULE] {
        let more = subscribe(conn, rule).await?;
        stream = Box::pin(futures_lite::stream::or(stream, more));
    }
    Ok(stream)
}

/// Follow every player, as [`bus::follow`] follows one daemon, with two
/// threads: one only moves the signals from zbus's queues into a channel,
/// so zbus's socket reader never waits on a full queue (which would hold up
/// the replies the other thread waits for); the other reads the players
/// and applies the signals. It ends soon after `stop` is set, or when the
/// connection closes (then the players are gone).
fn follow_players(bus: Bus, players: Arc<Mutex<Players>>, model: Arc<Model<MprisState>>, stop: Stop) {
    let (messages, inbox) = mpsc::channel::<Message>();
    let (ready, subscribed) = mpsc::channel::<Result<()>>();
    let (drain_bus, drain_stop) = (bus.clone(), stop.clone());
    let drained = std::thread::Builder::new().name("aizome-sys mpris signals".into()).spawn(move || {
        zbus::block_on(async move {
            let mut stream = match subscribe_all(drain_bus.connection()).await {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            let _ = ready.send(Ok(()));
            while let Some(msg) = stream.next().await {
                let Ok(msg) = msg else { continue };
                if drain_stop.stopped() || messages.send(msg).is_err() {
                    return;
                }
            }
        })
    });
    if let Err(error) = drained {
        log::warn!("aizome-sys: could not start a thread for media players: {error}");
        return;
    }
    let followed = std::thread::Builder::new().name("aizome-sys mpris".into()).spawn(move || {
        match subscribed.recv() {
            Ok(Ok(())) => {
                if let Err(error) = follow(&bus, &players, &model, &stop, &inbox) {
                    log::warn!("aizome-sys: stopped following media players: {error}");
                }
            }
            Ok(Err(error)) => log::warn!("aizome-sys: can't follow media players: {error}"),
            Err(_) => {}
        }
        if !stop.stopped() {
            let mut players = lock(&players);
            players.map.clear();
            model.set_if_changed(players.build());
        }
    });
    if let Err(error) = followed {
        log::warn!("aizome-sys: could not start a thread for media players: {error}");
    }
}

/// Read every player, then apply the signals as they come, until `stop`
/// is set or the signals end.
fn follow(
    bus: &Bus,
    players: &Mutex<Players>,
    model: &Model<MprisState>,
    stop: &Stop,
    inbox: &mpsc::Receiver<Message>,
) -> Result<()> {
    // Subscribed: now list the players, so none is missed.
    let names: Vec<String> = bus.call(DBUS, "/org/freedesktop/DBus", DBUS, "ListNames", &())?;
    for name in names.iter().filter(|n| is_player_name(n)) {
        if let Some(owner) = bus.owner(name) {
            appeared(bus, players, name, &owner);
        }
    }
    publish(bus, players, model);

    loop {
        let msg = match inbox.recv_timeout(Duration::from_millis(250)) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) if stop.stopped() => return Ok(()),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        if stop.stopped() {
            return Ok(());
        }
        dispatch(bus, players, &msg);
        // Take whatever else is already queued before publishing.
        while let Ok(msg) = inbox.try_recv() {
            dispatch(bus, players, &msg);
        }
        publish(bus, players, model);
    }
}

/// A player's name is owned by `owner`: read it whole.
fn appeared(bus: &Bus, players: &Mutex<Players>, name: &str, owner: &str) {
    // Read without holding the lock: a slow player mustn't block actions.
    let root = bus.get_all(owner, PATH, IFACE_ROOT).unwrap_or_else(|error| {
        log::info!("aizome-sys: reading media player {name}: {error}");
        Props::new()
    });
    let player = bus.get_all(owner, PATH, IFACE_PLAYER).unwrap_or_default();
    let now = Instant::now();
    let mut players = lock(players);
    let seq = players.next_seq();
    let playing = PlaybackStatus::parse(&player.str("PlaybackStatus")) == PlaybackStatus::Playing;
    players.map.insert(
        name.to_owned(),
        Tracked {
            owner: owner.to_owned(),
            position_us: player.i64("Position"),
            position_at: now,
            played: if playing { seq } else { 0 },
            changed: seq,
            stale_position: false,
            root,
            player,
        },
    );
}

/// Apply one signal.
fn dispatch(bus: &Bus, players: &Mutex<Players>, msg: &Message) {
    let sender = msg.header().sender().map(|s| s.to_string()).unwrap_or_default();
    let (iface, member) = bus::member(msg);
    match (iface.as_str(), member.as_str()) {
        (DBUS, "NameOwnerChanged") if sender == DBUS => {
            let Ok((name, old, new)) = msg.body().deserialize::<(String, String, String)>() else { return };
            if !is_player_name(&name) {
                return;
            }
            if !old.is_empty() || new.is_empty() {
                lock(players).map.remove(&name);
            }
            if !new.is_empty() {
                appeared(bus, players, &name, &new);
            }
        }
        (IFACE_PROPS, "PropertiesChanged") => {
            let Ok((changed_iface, changed, invalidated)) = msg.body().deserialize::<(String, Props, Vec<String>)>()
            else {
                return;
            };
            if changed_iface != IFACE_ROOT && changed_iface != IFACE_PLAYER {
                return;
            }
            let mut refetch = Vec::new();
            {
                let mut players = lock(players);
                let seq = players.next_seq();
                for (name, tracked) in players.map.iter_mut().filter(|(_, t)| t.owner == sender) {
                    let apply = |props: &mut Props| {
                        props.extend(changed.iter().map(|(k, v)| (k.clone(), v.clone())));
                        for key in &invalidated {
                            props.remove(key);
                        }
                    };
                    if changed_iface == IFACE_ROOT {
                        apply(&mut tracked.root);
                        tracked.changed = seq;
                    } else {
                        let touched = |key: &str| changed.contains_key(key) || invalidated.iter().any(|k| k == key);
                        tracked.change_player(seq, touched, apply);
                    }
                    if !invalidated.is_empty() {
                        refetch.push(name.clone());
                    }
                }
            }
            // Properties invalidated without values: read the interface again.
            for name in refetch {
                let Ok(props) = bus.get_all(&sender, PATH, &changed_iface) else { continue };
                let mut players = lock(players);
                let seq = players.next_seq();
                if let Some(tracked) = players.map.get_mut(&name).filter(|t| t.owner == sender) {
                    if changed_iface == IFACE_ROOT {
                        tracked.root = props;
                        tracked.changed = seq;
                    } else {
                        tracked.change_player(seq, |_| true, |p| *p = props);
                    }
                }
            }
        }
        (IFACE_PLAYER, "Seeked") => {
            let position = seeked_position(msg);
            let now = Instant::now();
            let mut players = lock(players);
            let seq = players.next_seq();
            for tracked in players.map.values_mut().filter(|t| t.owner == sender) {
                match position {
                    Some(us) => {
                        tracked.position_us = us;
                        tracked.position_at = now;
                        tracked.stale_position = false;
                    }
                    None => tracked.stale_position = true,
                }
                tracked.changed = seq;
            }
        }
        _ => {}
    }
}

/// `Seeked`'s position: an `x`, or another integer from a sloppy player.
fn seeked_position(msg: &Message) -> Option<i64> {
    let body = msg.body();
    body.deserialize::<i64>()
        .ok()
        .or_else(|| body.deserialize::<u64>().ok().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| body.deserialize::<i32>().ok().map(i64::from))
        .or_else(|| body.deserialize::<u32>().ok().map(i64::from))
}

/// Read the positions that went stale, then publish the snapshot.
fn publish(bus: &Bus, players: &Mutex<Players>, model: &Model<MprisState>) {
    let stale: Vec<(String, String)> = lock(players)
        .map
        .iter_mut()
        .filter_map(|(name, t)| std::mem::take(&mut t.stale_position).then(|| (name.clone(), t.owner.clone())))
        .collect();
    for (name, owner) in stale {
        let Ok(us) = read_position(bus, &owner) else { continue };
        if let Some(tracked) = lock(players).map.get_mut(&name).filter(|t| t.owner == owner) {
            tracked.position_us = us;
            tracked.position_at = Instant::now();
        }
    }
    // Build and install under the lock, so a refresh can't publish older.
    let players = lock(players);
    model.set_if_changed(players.build());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_go_between_threads() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Mpris>();
        send_sync::<MprisState>();
    }

    #[test]
    fn player_names_have_the_prefix_and_more() {
        assert!(is_player_name("org.mpris.MediaPlayer2.vlc"));
        assert!(!is_player_name("org.mpris.MediaPlayer2."));
        assert!(!is_player_name("org.mpris.MediaPlayer2"));
        assert!(!is_player_name("org.freedesktop.Notifications"));
    }

    #[test]
    fn without_a_bus_actions_fail_cleanly() {
        let mpris = Mpris {
            bus: None,
            model: Arc::new(Model::new(MprisState::default(), Wake::none())),
            players: Arc::default(),
            stop: Stop::default(),
        };
        assert!(!mpris.available());
        assert!(matches!(mpris.play_pause("org.mpris.MediaPlayer2.x").wait(), Err(Error::Unavailable(_))));
        assert!(matches!(mpris.media_play_pause().wait(), Err(Error::NotFound(_))));
        assert!(matches!(mpris.set_position("x", "not a path", Duration::ZERO).wait(), Err(Error::InvalidArgument(_))));
        assert!(matches!(mpris.set_volume("x", f64::NAN).wait(), Err(Error::InvalidArgument(_))));
    }
}
