//! What the snapshot says about each player, read leniently from its
//! `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` properties:
//! [`Player`], its track's [`Metadata`], [`PlaybackStatus`] and
//! [`LoopStatus`], the position estimate and the active-player choice.

use crate::bus::{Props, PropsExt, unwrap_variant, value_dict, value_f64, value_i64, value_str, value_strings};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use zbus::zvariant::Value;

/// The track id MPRIS uses for "no track".
pub(crate) const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// Whether a player is playing (`PlaybackStatus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    /// Stopped, and what a player that sends nothing or nonsense reads as.
    #[default]
    Stopped,
}

impl PlaybackStatus {
    /// From the property's value (`Playing`, `Paused`, `Stopped`), ignoring
    /// case; anything else is [`Stopped`](PlaybackStatus::Stopped).
    pub fn parse(status: &str) -> PlaybackStatus {
        let status = status.trim();
        if status.eq_ignore_ascii_case("Playing") {
            PlaybackStatus::Playing
        } else if status.eq_ignore_ascii_case("Paused") {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Stopped
        }
    }

    /// The property's value.
    pub fn as_str(self) -> &'static str {
        match self {
            PlaybackStatus::Playing => "Playing",
            PlaybackStatus::Paused => "Paused",
            PlaybackStatus::Stopped => "Stopped",
        }
    }
}

/// What repeats (`LoopStatus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LoopStatus {
    /// Play to the end and stop.
    #[default]
    None,
    /// Repeat the current track.
    Track,
    /// Repeat the playlist.
    Playlist,
}

impl LoopStatus {
    /// From the property's value (`None`, `Track`, `Playlist`), ignoring
    /// case; `None` (the Rust one) for anything else.
    pub fn parse(status: &str) -> Option<LoopStatus> {
        let status = status.trim();
        [LoopStatus::None, LoopStatus::Track, LoopStatus::Playlist]
            .into_iter()
            .find(|s| status.eq_ignore_ascii_case(s.as_str()))
    }

    /// The property's value.
    pub fn as_str(self) -> &'static str {
        match self {
            LoopStatus::None => "None",
            LoopStatus::Track => "Track",
            LoopStatus::Playlist => "Playlist",
        }
    }

    /// The next state for a repeat button: none, playlist, track, none.
    pub fn cycle(self) -> LoopStatus {
        match self {
            LoopStatus::None => LoopStatus::Playlist,
            LoopStatus::Playlist => LoopStatus::Track,
            LoopStatus::Track => LoopStatus::None,
        }
    }
}

/// The current track (`Metadata`), from the keys the MPRIS and xesam
/// specifications define. Missing keys and values of an unexpected type
/// read as empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    /// `mpris:trackid`, needed by [`set_position`](super::Mpris::set_position);
    /// empty when there is no track (or the player sent MPRIS's `NoTrack`).
    pub track_id: String,
    /// `xesam:title`.
    pub title: String,
    /// `xesam:artist` (a list; some players send a single string).
    pub artists: Vec<String>,
    /// `xesam:album`.
    pub album: String,
    /// `xesam:albumArtist`.
    pub album_artists: Vec<String>,
    /// `mpris:artUrl`: the cover, usually a `file://` or `https://` URL.
    pub art_url: String,
    /// `mpris:length`; `None` when unknown (or zero, as live streams say).
    pub length: Option<Duration>,
    /// `xesam:url`: the media's location.
    pub url: String,
}

impl Metadata {
    /// Read the `Metadata` property's `a{sv}`; anything that isn't a
    /// dictionary reads as no metadata.
    pub(crate) fn from_value(value: Option<&Value<'static>>) -> Metadata {
        let Some(value) = value else { return Metadata::default() };
        let dict = value_dict(value);
        let get = |key: &str| dict.get(key).copied();
        Metadata {
            track_id: get("mpris:trackid")
                .and_then(value_str)
                .filter(|id| !id.is_empty() && id != NO_TRACK)
                .unwrap_or_default(),
            title: get("xesam:title").map(text).unwrap_or_default(),
            artists: get("xesam:artist").map(texts).unwrap_or_default(),
            album: get("xesam:album").map(text).unwrap_or_default(),
            album_artists: get("xesam:albumArtist").map(texts).unwrap_or_default(),
            art_url: get("mpris:artUrl").map(text).unwrap_or_default(),
            length: get("mpris:length").and_then(length),
            url: get("xesam:url").map(text).unwrap_or_default(),
        }
    }

    /// The artists, joined for one line of text.
    pub fn artist(&self) -> String {
        self.artists.join(", ")
    }

    /// Whether the player said anything about a track.
    pub fn is_empty(&self) -> bool {
        *self == Metadata::default()
    }
}

/// A string, or a list of strings joined (some players send the title as
/// a list); anything else reads as empty.
fn text(value: &Value<'_>) -> String {
    match unwrap_variant(value) {
        Value::Array(_) => value_strings(value).join(", "),
        other => value_str(other).unwrap_or_default(),
    }
}

/// A list of strings, or a single string as a list of one.
fn texts(value: &Value<'_>) -> Vec<String> {
    let mut texts = match unwrap_variant(value) {
        Value::Array(_) => value_strings(value),
        other => value_str(other).into_iter().collect(),
    };
    texts.retain(|t| !t.trim().is_empty());
    texts
}

/// `mpris:length` in microseconds: any integer type, a float, or a number
/// in a string. Zero and negative lengths mean unknown.
fn length(value: &Value<'_>) -> Option<Duration> {
    let micros = value_i64(value).or_else(|| {
        let text = value_str(value)?;
        let text = text.trim();
        text.parse::<i64>().ok().or_else(|| text.parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f as i64))
    })?;
    u64::try_from(micros).ok().filter(|&us| us > 0).map(Duration::from_micros)
}

/// A media player on the bus.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    /// Its bus name, `org.mpris.MediaPlayer2.…`: what the actions take.
    pub bus_name: String,
    /// The unique name that owns it (`:1.42`).
    pub owner: String,
    /// `Identity`, the name to show; the bus name's last part when the
    /// player gives none.
    pub identity: String,
    /// `DesktopEntry`: the desktop file's name without `.desktop`, for the
    /// app's icon. Empty when the player doesn't say.
    pub desktop_entry: String,
    /// It can bring its window to the front ([`raise`](super::Mpris::raise)).
    pub can_raise: bool,
    /// It can be asked to quit ([`quit`](super::Mpris::quit)).
    pub can_quit: bool,
    /// `PlaybackStatus`.
    pub status: PlaybackStatus,
    /// `None` when the player doesn't support repeating.
    pub loop_status: Option<LoopStatus>,
    /// `None` when the player doesn't support shuffling.
    pub shuffle: Option<bool>,
    /// 0.0–1.0 (players may go above); `None` when the player has no volume.
    pub volume: Option<f64>,
    /// Playback speed, 1.0 normally.
    pub rate: f64,
    /// Whether the player takes commands at all. When it's false, all the
    /// `can_*` below are false too.
    pub can_control: bool,
    /// What the controls may do (`CanPlay`, `CanPause`, `CanGoNext`,
    /// `CanGoPrevious`, `CanSeek`): grey out the buttons that are false.
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    /// The current track.
    pub metadata: Metadata,
    /// The position as last read or announced (`Seeked`).
    pub last_position: Duration,
    /// When `last_position` was read.
    pub last_position_at: Instant,
    /// When the player last started or stopped playing (a change counter;
    /// 0 if never seen playing).
    pub(crate) played: u64,
    /// When the player last changed (a change counter).
    pub(crate) changed: u64,
}

impl Player {
    /// Build a player from its two interfaces' properties.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_props(
        bus_name: &str,
        owner: &str,
        root: &Props,
        player: &Props,
        position_us: i64,
        position_at: Instant,
        played: u64,
        changed: u64,
    ) -> Player {
        // CanControl is required; a player that leaves it out is trusted.
        let can_control = player.opt_bool("CanControl").unwrap_or(true);
        let can = |key: &str| can_control && player.bool(key);
        Player {
            bus_name: bus_name.to_owned(),
            owner: owner.to_owned(),
            identity: root
                .opt_str("Identity")
                .map(|i| i.trim().to_owned())
                .filter(|i| !i.is_empty())
                .unwrap_or_else(|| fallback_identity(bus_name)),
            desktop_entry: root.str("DesktopEntry").trim().trim_end_matches(".desktop").to_owned(),
            can_raise: root.bool("CanRaise"),
            can_quit: root.bool("CanQuit"),
            status: PlaybackStatus::parse(&player.str("PlaybackStatus")),
            loop_status: player.opt_str("LoopStatus").and_then(|s| LoopStatus::parse(&s)),
            shuffle: player.opt_bool("Shuffle"),
            volume: player.value("Volume").and_then(value_f64).filter(|v| v.is_finite()).map(|v| v.max(0.0)),
            rate: rate(player),
            can_control,
            can_play: can("CanPlay"),
            can_pause: can("CanPause"),
            can_go_next: can("CanGoNext"),
            can_go_previous: can("CanGoPrevious"),
            can_seek: can("CanSeek"),
            metadata: Metadata::from_value(player.value("Metadata")),
            last_position: Duration::from_micros(u64::try_from(position_us).unwrap_or(0)),
            last_position_at: position_at,
            played,
            changed,
        }
    }

    /// Whether it is playing.
    pub fn is_playing(&self) -> bool {
        self.status == PlaybackStatus::Playing
    }

    /// The position now: the last one read, plus the time since then at
    /// the playback rate while playing, up to the track's length.
    pub fn position(&self) -> Duration {
        self.position_at(Instant::now())
    }

    /// The position estimated for `now`.
    pub fn position_at(&self, now: Instant) -> Duration {
        let base = i64::try_from(self.last_position.as_micros()).unwrap_or(i64::MAX);
        let us = estimate_us(base, self.last_position_at, now, self.is_playing(), self.rate, self.metadata.length);
        Duration::from_micros(u64::try_from(us).unwrap_or(0))
    }
}

/// `Rate`, 1.0 when missing or not a number.
pub(crate) fn rate(player: &Props) -> f64 {
    player.value("Rate").and_then(value_f64).filter(|r| r.is_finite()).unwrap_or(1.0)
}

/// The position in microseconds at `now`, from `base_us` read at `read_at`.
pub(crate) fn estimate_us(
    base_us: i64,
    read_at: Instant,
    now: Instant,
    playing: bool,
    rate: f64,
    length: Option<Duration>,
) -> i64 {
    let base = base_us.max(0);
    let mut us = base;
    if playing {
        let elapsed = now.saturating_duration_since(read_at).as_secs_f64() * 1e6;
        let moved = elapsed * if rate.is_finite() { rate } else { 1.0 };
        // `as` saturates for floats out of range.
        us = (base as f64 + moved).max(0.0) as i64;
    }
    match length.and_then(|l| i64::try_from(l.as_micros()).ok()) {
        Some(length) if us > length => length,
        _ => us,
    }
}

/// A name for a player that gives no `Identity`: its bus name's first
/// part after the prefix (`org.mpris.MediaPlayer2.vlc.instance42` → `vlc`).
pub(crate) fn fallback_identity(bus_name: &str) -> String {
    let rest = bus_name.strip_prefix(super::BUS_PREFIX).unwrap_or(bus_name);
    rest.split('.').next().filter(|s| !s.is_empty()).unwrap_or(rest).to_owned()
}

/// The player for the menu bar and the media keys: one that's playing
/// (the latest to start, when several are), else the one that stopped
/// playing last, else the one that changed last.
pub(crate) fn choose_active(players: &[Player]) -> Option<String> {
    players.iter().max_by_key(|p| (p.is_playing(), p.played, p.changed)).map(|p| p.bus_name.clone())
}

/// The metadata's length, for estimates made before a snapshot is built.
pub(crate) fn length_of(player: &Props) -> Option<Duration> {
    let value = player.value("Metadata")?;
    let dict: HashMap<String, &Value<'static>> = value_dict(value);
    dict.get("mpris:length").copied().and_then(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;
    use zbus::zvariant::{ObjectPath, OwnedValue};

    fn dict(entries: Vec<(&str, Value<'static>)>) -> Value<'static> {
        let map: HashMap<String, Value<'static>> = entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        Value::from(map)
    }

    fn props(entries: Vec<(&str, OwnedValue)>) -> Props {
        entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    #[test]
    fn metadata_reads_the_usual_keys() {
        let value = dict(vec![
            ("mpris:trackid", Value::from(ObjectPath::try_from("/org/example/track/7").unwrap())),
            ("xesam:title", Value::from("Blue in Green")),
            ("xesam:artist", Value::from(vec!["Miles Davis", "Bill Evans"])),
            ("xesam:album", Value::from("Kind of Blue")),
            ("mpris:artUrl", Value::from("file:///tmp/cover.jpg")),
            ("mpris:length", Value::from(337_000_000i64)),
            ("xesam:url", Value::from("file:///music/blue.flac")),
        ]);
        let m = Metadata::from_value(Some(&value));
        assert_eq!(m.track_id, "/org/example/track/7");
        assert_eq!(m.title, "Blue in Green");
        assert_eq!(m.artist(), "Miles Davis, Bill Evans");
        assert_eq!(m.album, "Kind of Blue");
        assert_eq!(m.art_url, "file:///tmp/cover.jpg");
        assert_eq!(m.length, Some(Duration::from_secs(337)));
        assert_eq!(m.url, "file:///music/blue.flac");
    }

    #[test]
    fn odd_metadata_reads_as_well_as_it_can() {
        // Artist as a string, length as u64, i32 and text, the title as a
        // number, the track id as a string, values in extra variants.
        let value = dict(vec![
            ("mpris:trackid", Value::from("not/a/path")),
            ("xesam:title", Value::from(42u32)),
            ("xesam:artist", Value::from("Solo Artist")),
            ("xesam:albumArtist", Value::Value(Box::new(Value::from(vec!["A", ""])))),
            ("mpris:length", Value::from(5_000_000u64)),
            ("xesam:album", Value::from(vec!["Disc", "One"])),
        ]);
        let m = Metadata::from_value(Some(&value));
        assert_eq!(m.track_id, "not/a/path");
        assert_eq!(m.title, "", "a number isn't a title");
        assert_eq!(m.artists, ["Solo Artist"]);
        assert_eq!(m.album_artists, ["A"]);
        assert_eq!(m.album, "Disc, One");
        assert_eq!(m.length, Some(Duration::from_secs(5)));

        for (raw, expected) in [
            (Value::from(3_000_000i32), Some(Duration::from_secs(3))),
            (Value::from("2000000"), Some(Duration::from_secs(2))),
            (Value::from(1.5e6f64), Some(Duration::from_millis(1500))),
            (Value::from(-1i64), None),
            (Value::from(0u64), None),
            (Value::from(u64::MAX), None),
            (Value::from(f64::NAN), None),
            (Value::from(true), None),
        ] {
            let m = Metadata::from_value(Some(&dict(vec![("mpris:length", raw.clone())])));
            assert_eq!(m.length, expected, "{raw:?}");
        }

        // Not a dictionary at all, or no track.
        assert!(Metadata::from_value(Some(&Value::from("nonsense"))).is_empty());
        assert!(Metadata::from_value(None).is_empty());
        let none = dict(vec![("mpris:trackid", Value::from(ObjectPath::try_from(NO_TRACK).unwrap()))]);
        assert!(Metadata::from_value(Some(&none)).is_empty());
    }

    #[test]
    fn players_read_leniently() {
        let root =
            props(vec![("Identity", owned("  ")), ("DesktopEntry", owned("vlc.desktop")), ("CanRaise", owned(1u8))]);
        let player = props(vec![
            ("PlaybackStatus", owned("playing")),
            ("LoopStatus", owned("Sometimes")),
            ("Shuffle", owned(1i32)),
            ("Volume", owned("loud")),
            ("Rate", owned(f64::NAN)),
            ("CanPlay", owned(true)),
            ("CanSeek", owned("yes")),
        ]);
        let p = Player::from_props(
            "org.mpris.MediaPlayer2.vlc.instance42",
            ":1.7",
            &root,
            &player,
            -5,
            Instant::now(),
            0,
            1,
        );
        assert_eq!(p.identity, "vlc");
        assert_eq!(p.desktop_entry, "vlc");
        assert!(p.can_raise && !p.can_quit);
        assert_eq!(p.status, PlaybackStatus::Playing);
        assert_eq!(p.loop_status, None);
        assert_eq!(p.shuffle, Some(true));
        assert_eq!(p.volume, None);
        assert_eq!(p.rate, 1.0);
        assert!(p.can_control && p.can_play && !p.can_seek && !p.can_pause);
        assert_eq!(p.last_position, Duration::ZERO);

        let locked = props(vec![("CanControl", owned(false)), ("CanPlay", owned(true))]);
        let p = Player::from_props("org.mpris.MediaPlayer2.x", ":1.8", &Props::new(), &locked, 0, Instant::now(), 0, 1);
        assert!(!p.can_control && !p.can_play, "CanControl false turns every can_* off");
    }

    #[test]
    fn positions_are_estimated_while_playing() {
        let start = Instant::now();
        let later = start + Duration::from_secs(10);
        assert_eq!(estimate_us(1_000_000, start, later, true, 1.0, None), 11_000_000);
        assert_eq!(estimate_us(1_000_000, start, later, false, 1.0, None), 1_000_000);
        assert_eq!(estimate_us(1_000_000, start, later, true, 2.0, None), 21_000_000);
        assert_eq!(estimate_us(1_000_000, start, later, true, 1.0, Some(Duration::from_secs(5))), 5_000_000);
        assert_eq!(estimate_us(1_000_000, start, later, true, -1.0, None), 0, "never below zero");
        assert_eq!(estimate_us(1_000_000, later, start, true, 1.0, None), 1_000_000, "a clock read before is no time");
        assert_eq!(estimate_us(i64::MAX, start, later, true, f64::MAX, None), i64::MAX);
    }

    #[test]
    fn the_active_player_is_the_one_playing_else_the_last_played_else_the_last_changed() {
        let player = |name: &str, status: PlaybackStatus, played: u64, changed: u64| {
            let mut p =
                Player::from_props(name, ":1.1", &Props::new(), &Props::new(), 0, Instant::now(), played, changed);
            p.status = status;
            p
        };
        use PlaybackStatus::*;
        assert_eq!(choose_active(&[]), None);
        let a = player("a", Paused, 0, 5);
        let b = player("b", Stopped, 0, 9);
        assert_eq!(choose_active(&[a.clone(), b.clone()]).as_deref(), Some("b"), "the last changed");
        let c = player("c", Paused, 3, 4);
        assert_eq!(choose_active(&[a.clone(), b.clone(), c.clone()]).as_deref(), Some("c"), "the last played");
        let d = player("d", Playing, 1, 2);
        assert_eq!(choose_active(&[a.clone(), b.clone(), c.clone(), d.clone()]).as_deref(), Some("d"), "playing wins");
        let e = player("e", Playing, 7, 8);
        assert_eq!(choose_active(&[d, e, a, b, c]).as_deref(), Some("e"), "the latest to start");
    }

    #[test]
    fn statuses_parse_and_cycle() {
        assert_eq!(PlaybackStatus::parse("Paused"), PlaybackStatus::Paused);
        assert_eq!(PlaybackStatus::parse(""), PlaybackStatus::Stopped);
        assert_eq!(LoopStatus::parse("playlist"), Some(LoopStatus::Playlist));
        assert_eq!(LoopStatus::None.cycle().cycle().cycle(), LoopStatus::None);
        assert_eq!(fallback_identity("org.mpris.MediaPlayer2.spotify"), "spotify");
    }
}
