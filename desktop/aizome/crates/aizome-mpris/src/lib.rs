//! The desktop's media controls (MPRIS 2), from a player's side: a player
//! ([`Identity`]) as `org.mpris.MediaPlayer2.<bus>`, for the menu bar's
//! media widget and the media keys (both the shell's: `aizome-sys`'s
//! `mpris` follows the players), and anything else that speaks it.
//! Utsushi and Chanoma are players.
//!
//! D-Bus runs on a thread of its own; the player publishes its [`State`]
//! when it changes (never as the position moves: MPRIS readers estimate
//! it, and `Seeked` says when it jumps), and hears what's asked of it as
//! [`Action`]s.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

use zbus::blocking::Connection;
use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";
const INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

/// Who the player is, as MPRIS says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    /// The last part of its bus name: `org.mpris.MediaPlayer2.<bus>`.
    pub bus: &'static str,
    /// Its name, as people know it ("Utsushi").
    pub name: &'static str,
    /// Its desktop entry, without `.desktop` ("org.aizome.Utsushi").
    pub desktop_entry: &'static str,
    /// What it may be asked to open (`OpenUri`): none, nothing.
    pub uri_schemes: &'static [&'static str],
    pub mime_types: &'static [&'static str],
    /// Whether it may be asked to quit, or to come forward.
    pub can_quit: bool,
    pub can_raise: bool,
    /// The slowest and fastest it plays (1 and 1: as recorded only).
    pub rates: (f64, f64),
}

impl Identity {
    /// The object path naming item `item`: under its desktop entry's name
    /// (`/org/aizome/Utsushi/item/3`).
    fn item_path(&self, item: u64) -> String {
        format!("/{}/item/{item}", self.desktop_entry.replace('.', "/"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    /// To this position, seconds.
    Seek(f64),
    Volume(f64),
    Rate(f64),
    Loop(String),
    Raise,
    Quit,
    Open(String),
}

/// What's playing, as MPRIS says it.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// A number for the item, changing when another plays.
    pub item: u64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub url: Option<String>,
    pub art: Option<String>,
    /// "Playing", "Paused" or "Stopped".
    pub status: &'static str,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub rate: f64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    /// "None", "Track" or "Playlist".
    pub loop_status: &'static str,
    /// Changes when the position jumps (a seek).
    pub seeks: u64,
}

impl Default for State {
    fn default() -> State {
        State {
            item: 0,
            title: String::new(),
            artist: None,
            album: None,
            url: None,
            art: None,
            status: "Stopped",
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            rate: 1.0,
            can_next: false,
            can_previous: false,
            can_seek: false,
            loop_status: "None",
            seeks: 0,
        }
    }
}

type Notify = Arc<dyn Fn(Action) + Send + Sync>;

struct Root {
    id: Identity,
    notify: Notify,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        if self.id.can_raise {
            (self.notify)(Action::Raise);
        }
    }
    fn quit(&self) {
        if self.id.can_quit {
            (self.notify)(Action::Quit);
        }
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        self.id.can_quit
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        self.id.can_raise
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        self.id.name
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        self.id.desktop_entry
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        self.id.uri_schemes.iter().map(|s| (*s).to_owned()).collect()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        self.id.mime_types.iter().map(|s| (*s).to_owned()).collect()
    }
}

struct Transport {
    id: Identity,
    state: Arc<Mutex<State>>,
    notify: Notify,
    /// Where playback is now: the player's clock, read when asked.
    now: Now,
}

type Now = Arc<dyn Fn() -> f64 + Send + Sync>;

impl Transport {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Transport {
    fn next(&self) {
        if self.state().can_next {
            (self.notify)(Action::Next);
        }
    }
    fn previous(&self) {
        if self.state().can_previous {
            (self.notify)(Action::Previous);
        }
    }
    fn pause(&self) {
        (self.notify)(Action::Pause);
    }
    fn play_pause(&self) {
        (self.notify)(Action::Toggle);
    }
    fn stop(&self) {
        (self.notify)(Action::Stop);
    }
    fn play(&self) {
        (self.notify)(Action::Play);
    }
    fn seek(&self, offset: i64) {
        let s = self.state();
        if !s.can_seek {
            return;
        }
        let to = ((self.now)() + offset as f64 / 1e6).max(0.0);
        if to >= s.duration && s.can_next {
            (self.notify)(Action::Next);
        } else {
            (self.notify)(Action::Seek(to.min(s.duration)));
        }
    }
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) {
        let s = self.state();
        if s.can_seek
            && track_id.as_str() == self.id.item_path(s.item)
            && position >= 0
            && position as f64 / 1e6 <= s.duration
        {
            (self.notify)(Action::Seek(position as f64 / 1e6));
        }
    }
    fn open_uri(&self, uri: &str) {
        if self.id.uri_schemes.iter().any(|s| uri.starts_with(&format!("{s}:"))) {
            (self.notify)(Action::Open(uri.to_owned()));
        }
    }
    #[zbus(property)]
    fn playback_status(&self) -> &str {
        self.state().status
    }
    #[zbus(property)]
    fn loop_status(&self) -> &str {
        self.state().loop_status
    }
    #[zbus(property)]
    fn set_loop_status(&self, value: &str) -> zbus::fdo::Result<()> {
        match value {
            "None" | "Track" | "Playlist" => {
                (self.notify)(Action::Loop(value.into()));
                Ok(())
            }
            _ => Err(zbus::fdo::Error::InvalidArgs("Loop status is None, Track or Playlist".into())),
        }
    }
    #[zbus(signal)]
    async fn seeked(emitter: &zbus::object_server::SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.state().rate
    }
    #[zbus(property)]
    fn set_rate(&self, rate: f64) {
        if rate == 0.0 {
            (self.notify)(Action::Pause);
        } else if rate.is_finite() {
            (self.notify)(Action::Rate(rate.clamp(self.id.rates.0, self.id.rates.1)));
        }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.id, &self.state())
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state().volume
    }
    #[zbus(property)]
    fn set_volume(&self, volume: f64) {
        if volume.is_finite() {
            (self.notify)(Action::Volume(volume.clamp(0.0, 1.0)));
        }
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        let at = if self.state().status == "Stopped" { 0.0 } else { (self.now)() };
        (at * 1e6) as i64
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        self.id.rates.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        self.id.rates.1
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.state().can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.state().can_previous
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        !self.state().title.is_empty()
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.state().status != "Stopped"
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.state().can_seek
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn metadata(id: &Identity, s: &State) -> HashMap<String, OwnedValue> {
    if s.title.is_empty() {
        return HashMap::new();
    }
    let mut m: HashMap<String, OwnedValue> = HashMap::new();
    if let Ok(p) = ObjectPath::try_from(id.item_path(s.item)) {
        m.insert("mpris:trackid".into(), p.into());
    }
    m.insert("mpris:length".into(), ((s.duration * 1e6) as i64).into());
    m.insert("xesam:title".into(), Str::from(s.title.clone()).into());
    if let Some(a) = &s.artist
        && let Ok(v) = Value::from(vec![a.clone()]).try_into()
    {
        m.insert("xesam:artist".into(), v);
    }
    if let Some(a) = &s.album {
        m.insert("xesam:album".into(), Str::from(a.clone()).into());
    }
    if let Some(u) = &s.url {
        m.insert("xesam:url".into(), Str::from(u.clone()).into());
    }
    if let Some(a) = &s.art {
        m.insert("mpris:artUrl".into(), Str::from(a.clone()).into());
    }
    m
}

/// A file (or an address) as the URL MPRIS wants for what plays and its
/// picture (`xesam:url`, `mpris:artUrl`): an address as it was given, a
/// file as a `file://` URL escaped as GLib escapes it (spaces, `#` and `%`
/// included).
pub fn url_of(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    if !bytes.starts_with(b"/") && path.to_string_lossy().contains("://") {
        return path.to_string_lossy().into_owned();
    }
    let mut s = String::from("file://");
    for &b in bytes {
        if b.is_ascii_alphanumeric() || b"!$&'()*+,-./:=@_~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The MPRIS service: publish states, hear actions.
pub struct Mpris {
    updates: mpsc::Sender<State>,
}

impl Mpris {
    /// Serve `id` on the session bus (on a thread of its own), calling
    /// `notify` with what's asked, and `now` for the position. Without a
    /// bus it says so in the log.
    pub fn start(
        id: Identity,
        notify: impl Fn(Action) + Send + Sync + 'static,
        now: impl Fn() -> f64 + Send + Sync + 'static,
    ) -> Mpris {
        let (updates, rx) = mpsc::channel::<State>();
        let notify: Notify = Arc::new(notify);
        let now: Now = Arc::new(now);
        let _ = std::thread::Builder::new().name(format!("{}-mpris", id.bus)).spawn(move || {
            // The first state goes up before the name is taken: clients
            // read everything at once when it appears.
            let Ok(first) = rx.recv() else { return };
            let state = Arc::new(Mutex::new(first));
            let conn = match connect(id.clone(), state.clone(), notify, now) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("no desktop media controls: {e}");
                    return;
                }
            };
            while let Ok(mut newest) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    newest = newer;
                }
                let old = std::mem::replace(&mut *state.lock().unwrap_or_else(|e| e.into_inner()), newest.clone());
                publish_changes(&id, &conn, &old, &newest);
            }
        });
        Mpris { updates }
    }

    pub fn publish(&self, state: State) {
        let _ = self.updates.send(state);
    }
}

fn publish_changes(id: &Identity, conn: &Connection, old: &State, new: &State) {
    let mut changed: HashMap<&str, OwnedValue> = HashMap::new();
    if old.status != new.status {
        changed.insert("PlaybackStatus", Str::from(new.status).into());
        changed.insert("CanPause", (new.status != "Stopped").into());
    }
    if old.volume != new.volume {
        changed.insert("Volume", new.volume.into());
    }
    if old.rate != new.rate {
        changed.insert("Rate", new.rate.into());
    }
    if old.loop_status != new.loop_status {
        changed.insert("LoopStatus", Str::from(new.loop_status).into());
    }
    if (old.item, &old.title, &old.artist, &old.album, &old.art, old.duration)
        != (new.item, &new.title, &new.artist, &new.album, &new.art, new.duration)
    {
        if let Ok(v) = Value::from(metadata(id, new)).try_into() {
            changed.insert("Metadata", v);
        }
        changed.insert("CanPlay", (!new.title.is_empty()).into());
    }
    if old.can_next != new.can_next {
        changed.insert("CanGoNext", new.can_next.into());
    }
    if old.can_previous != new.can_previous {
        changed.insert("CanGoPrevious", new.can_previous.into());
    }
    if old.can_seek != new.can_seek {
        changed.insert("CanSeek", new.can_seek.into());
    }
    if !changed.is_empty() {
        let _ = conn.emit_signal(
            None::<&str>,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(INTERFACE, changed, Vec::<String>::new()),
        );
    }
    if old.seeks != new.seeks {
        let _ = conn.emit_signal(None::<&str>, PATH, INTERFACE, "Seeked", &((new.position * 1e6) as i64,));
    }
}

fn connect(id: Identity, state: Arc<Mutex<State>>, notify: Notify, now: Now) -> zbus::Result<Connection> {
    zbus::blocking::connection::Builder::session()?
        .name(format!("org.mpris.MediaPlayer2.{}", id.bus))?
        .serve_at(PATH, Root { id: id.clone(), notify: notify.clone() })?
        .serve_at(PATH, Transport { id, state, notify, now })?
        .build()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn what_plays_is_a_url_the_media_controls_can_read() {
        let film = PathBuf::from("/home/u/My Film #2 (100%).mkv");
        assert_eq!(url_of(&film), "file:///home/u/My%20Film%20%232%20(100%25).mkv");
        assert_eq!(url_of(&PathBuf::from("/m/\u{6620}.flac")), "file:///m/%E6%98%A0.flac");
        assert_eq!(url_of(&PathBuf::from("https://example.org/a.mp4")), "https://example.org/a.mp4");
    }

    #[test]
    fn items_are_named_under_the_players_desktop_entry() {
        let id = Identity {
            bus: "chanoma",
            name: "Chanoma",
            desktop_entry: "org.aizome.Chanoma",
            uri_schemes: &[],
            mime_types: &[],
            can_quit: false,
            can_raise: true,
            rates: (1.0, 1.0),
        };
        assert_eq!(id.item_path(3), "/org/aizome/Chanoma/item/3");
        let state = State { item: 3, title: "Episode 1".into(), duration: 2.0, ..State::default() };
        let m = metadata(&id, &state);
        assert_eq!(m.len(), 3, "the track, its length and its title: {m:?}");
        assert!(metadata(&id, &State::default()).is_empty(), "nothing plays: nothing to say");
    }
}
