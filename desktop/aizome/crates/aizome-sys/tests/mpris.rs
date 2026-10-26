//! Media players against fake MPRIS players on a private bus, and a
//! read-only look at this session's real players.

mod common;

use aizome_sys::mpris::{LoopStatus, Mpris, PlaybackStatus};
use aizome_sys::{Error, Wake};
use common::{eventually, path};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const PATH: &str = "/org/mpris/MediaPlayer2";
const VLC: &str = "org.mpris.MediaPlayer2.vlc";
const SPOTIFY: &str = "org.mpris.MediaPlayer2.spotify";
const ODD: &str = "org.mpris.MediaPlayer2.odd.instance7";

fn owned<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    OwnedValue::try_from(value.into()).expect("no fds")
}

/// What the tests can see of a fake player from outside.
#[derive(Default)]
struct Calls {
    raised: AtomicBool,
    quit: AtomicBool,
    methods: Mutex<Vec<String>>,
}

struct Root {
    identity: String,
    calls: Arc<Calls>,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.calls.raised.store(true, Ordering::SeqCst);
    }
    fn quit(&self) {
        self.calls.quit.store(true, Ordering::SeqCst);
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        self.identity.to_lowercase()
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["file".into()]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec!["audio/flac".into()]
    }
}

/// A well-behaved player with a three-track playlist.
struct Player {
    status: String,
    track: usize,
    position: i64,
    volume: f64,
    shuffle: bool,
    loop_status: String,
    calls: Arc<Calls>,
}

fn track_id(track: usize) -> OwnedObjectPath {
    path(&format!("/org/aizome/test/track/{track}"))
}

impl Player {
    fn new(status: &str, calls: Arc<Calls>) -> Player {
        Player {
            status: status.into(),
            track: 0,
            position: 0,
            volume: 0.5,
            shuffle: false,
            loop_status: "None".into(),
            calls,
        }
    }

    fn log(&self, method: &str) {
        self.calls.methods.lock().unwrap().push(method.to_owned());
    }

    async fn set_status(&mut self, status: &str, emitter: &SignalEmitter<'_>) -> fdo::Result<()> {
        self.status = status.into();
        self.playback_status_changed(emitter).await?;
        Ok(())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn play_pause(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("PlayPause");
        let next = if self.status == "Playing" { "Paused" } else { "Playing" };
        self.set_status(next, &emitter).await
    }
    async fn play(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("Play");
        self.set_status("Playing", &emitter).await
    }
    async fn pause(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("Pause");
        self.set_status("Paused", &emitter).await
    }
    async fn stop(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("Stop");
        self.position = 0;
        self.set_status("Stopped", &emitter).await
    }
    async fn next(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("Next");
        self.track = (self.track + 1) % 3;
        self.position = 0;
        self.metadata_changed(&emitter).await?;
        Ok(())
    }
    async fn previous(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("Previous");
        self.track = (self.track + 2) % 3;
        self.position = 0;
        self.metadata_changed(&emitter).await?;
        Ok(())
    }
    async fn seek(&mut self, offset: i64, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.log("Seek");
        self.position = (self.position + offset).max(0);
        Player::seeked(&emitter, self.position).await?;
        Ok(())
    }
    async fn set_position(
        &mut self,
        track: OwnedObjectPath,
        position: i64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.log("SetPosition");
        if track == track_id(self.track) {
            self.position = position;
            Player::seeked(&emitter, position).await?;
        }
        Ok(())
    }
    fn open_uri(&self, _uri: String) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.status.clone()
    }
    #[zbus(property)]
    fn loop_status(&self) -> String {
        self.loop_status.clone()
    }
    #[zbus(property)]
    fn set_loop_status(&mut self, status: String) {
        self.loop_status = status;
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.shuffle
    }
    #[zbus(property)]
    fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let titles = ["So What", "Freddie Freeloader", "Blue in Green"];
        HashMap::from([
            ("mpris:trackid".to_owned(), owned(track_id(self.track))),
            ("xesam:title".to_owned(), owned(titles[self.track])),
            ("xesam:artist".to_owned(), owned(vec!["Miles Davis"])),
            ("xesam:album".to_owned(), owned("Kind of Blue")),
            ("mpris:artUrl".to_owned(), owned("file:///tmp/kind-of-blue.jpg")),
            ("mpris:length".to_owned(), owned(300_000_000i64)),
        ])
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.volume
    }
    #[zbus(property)]
    fn set_volume(&mut self, volume: f64) {
        self.volume = volume;
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.position
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// Serve a player under `name`; dropping the connection is the player
/// quitting.
fn serve(bus: &common::TestBus, name: &str, identity: &str, status: &str) -> (zbus::blocking::Connection, Arc<Calls>) {
    let calls = Arc::new(Calls::default());
    let conn = bus
        .builder()
        .name(name.to_owned())
        .unwrap()
        .serve_at(PATH, Root { identity: identity.into(), calls: calls.clone() })
        .unwrap()
        .serve_at(PATH, Player::new(status, calls.clone()))
        .unwrap()
        .build()
        .unwrap();
    (conn, calls)
}

/// A player that sends odd values: a status in lower case, the artist as a
/// string, the length as text, the title as a number, a volume that's a
/// word, and a live stream that can't pause.
struct OddPlayer {
    metadata: OwnedValue,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl OddPlayer {
    fn play_pause(&self) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported("can't pause a stream".into()))
    }
    fn stop(&self) {}
    #[zbus(property)]
    fn playback_status(&self) -> String {
        "playing".into()
    }
    #[zbus(property)]
    fn metadata(&self) -> OwnedValue {
        self.metadata.clone()
    }
    #[zbus(property)]
    fn volume(&self) -> String {
        "loud".into()
    }
    #[zbus(property)]
    fn loop_status(&self) -> String {
        "Sometimes".into()
    }
    #[zbus(property)]
    fn position(&self) -> u64 {
        7_000_000
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        false
    }
}

fn odd_metadata() -> OwnedValue {
    let map: HashMap<&str, Value<'_>> = HashMap::from([
        ("mpris:trackid", Value::from("/not/an/object/path/but/a/string")),
        ("xesam:title", Value::from(12i32)),
        ("xesam:artist", Value::from("Radio Host")),
        ("mpris:length", Value::from("not a number")),
        ("xesam:url", Value::from("https://radio.example/stream")),
    ]);
    owned(map)
}

#[test]
fn follows_two_players_and_chooses_the_active_one() {
    let bus = test_bus!();
    let (_vlc, _) = serve(&bus, VLC, "VLC media player", "Paused");
    let mpris = Mpris::with_bus(bus.bus(), Wake::none());
    assert!(mpris.available());
    assert!(eventually(|| mpris.snapshot().players.len() == 1));
    let (_spotify, _) = serve(&bus, SPOTIFY, "Spotify", "Stopped");
    assert!(eventually(|| mpris.snapshot().players.len() == 2));

    let state = mpris.snapshot();
    let names: Vec<&str> = state.players.iter().map(|p| p.identity.as_str()).collect();
    assert_eq!(names, ["Spotify", "VLC media player"], "sorted by name");
    let vlc = state.player(VLC).unwrap();
    assert_eq!(vlc.status, PlaybackStatus::Paused);
    assert_eq!(vlc.desktop_entry, "vlc media player");
    assert_eq!(vlc.metadata.title, "So What");
    assert_eq!(vlc.metadata.artists, ["Miles Davis"]);
    assert_eq!(vlc.metadata.album, "Kind of Blue");
    assert_eq!(vlc.metadata.track_id, "/org/aizome/test/track/0");
    assert_eq!(vlc.metadata.art_url, "file:///tmp/kind-of-blue.jpg");
    assert_eq!(vlc.metadata.length, Some(Duration::from_secs(300)));
    assert_eq!((vlc.volume, vlc.shuffle, vlc.loop_status), (Some(0.5), Some(false), Some(LoopStatus::None)));
    assert!(vlc.can_control && vlc.can_play && vlc.can_pause && vlc.can_seek && vlc.can_go_next && vlc.can_raise);
    assert!(vlc.owner.starts_with(':'));
    // Neither has played: the one that changed last (appeared last).
    assert_eq!(state.active.as_deref(), Some(SPOTIFY));

    mpris.play_pause(VLC).wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().is_playing()));
    assert_eq!(mpris.snapshot().active.as_deref(), Some(VLC), "playing wins");

    mpris.play(SPOTIFY).wait().unwrap();
    assert!(eventually(|| mpris.snapshot().active.as_deref() == Some(SPOTIFY)), "the latest to start");
    assert!(mpris.snapshot().player(VLC).unwrap().is_playing());

    mpris.pause(SPOTIFY).wait().unwrap();
    assert!(eventually(|| mpris.snapshot().active.as_deref() == Some(VLC)), "the one still playing");

    mpris.pause(VLC).wait().unwrap();
    assert!(eventually(|| !mpris.snapshot().playing()));
    assert_eq!(mpris.snapshot().active.as_deref(), Some(VLC), "the one that played last");

    // The media keys act on the active player.
    mpris.media_play_pause().wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().is_playing()));
    assert_eq!(mpris.snapshot().player(SPOTIFY).unwrap().status, PlaybackStatus::Paused);
    mpris.media_next().wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().metadata.title == "Freddie Freeloader"));
    mpris.media_previous().wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().metadata.title == "So What"));
    mpris.media_stop().wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().status == PlaybackStatus::Stopped));
}

#[test]
fn tracks_seeks_and_settings() {
    let bus = test_bus!();
    let (_vlc, calls) = serve(&bus, VLC, "VLC media player", "Playing");
    let mpris = Mpris::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| mpris.snapshot().player(VLC).is_some()));
    assert!(mpris.snapshot().player(VLC).unwrap().is_playing());

    // Seeked resets the position; the estimate moves on while playing.
    mpris.seek(VLC, 10_000_000).wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().last_position == Duration::from_secs(10)));
    let player = mpris.snapshot().player(VLC).unwrap().clone();
    let later = player.last_position_at + Duration::from_secs(3);
    assert_eq!(player.position_at(later), Duration::from_secs(13));
    assert!(player.position() >= Duration::from_secs(10));

    // SetPosition needs the current track.
    let track = player.metadata.track_id.clone();
    mpris.set_position(VLC, &track, Duration::from_secs(42)).wait().unwrap();
    assert!(eventually(|| mpris.snapshot().player(VLC).unwrap().last_position == Duration::from_secs(42)));

    // A new track: its position is read again (the fake starts it at 0).
    mpris.next(VLC).wait().unwrap();
    assert!(eventually(|| {
        let state = mpris.snapshot();
        let p = state.player(VLC).unwrap();
        p.metadata.title == "Freddie Freeloader" && p.last_position < Duration::from_secs(1)
    }));

    // Pausing reads where it stopped; the estimate then stands still.
    mpris.seek(VLC, 5_000_000).wait().unwrap();
    mpris.pause(VLC).wait().unwrap();
    assert!(eventually(|| {
        let state = mpris.snapshot();
        let p = state.player(VLC).unwrap();
        p.status == PlaybackStatus::Paused && p.last_position == Duration::from_secs(5)
    }));
    let paused = mpris.snapshot().player(VLC).unwrap().clone();
    assert_eq!(paused.position_at(paused.last_position_at + Duration::from_secs(60)), Duration::from_secs(5));
    assert_eq!(mpris.refresh_position(VLC).wait(), Ok(Duration::from_secs(5)));

    mpris.set_volume(VLC, 0.25).wait().unwrap();
    mpris.set_shuffle(VLC, true).wait().unwrap();
    mpris.set_loop_status(VLC, LoopStatus::Playlist).wait().unwrap();
    assert!(eventually(|| {
        let state = mpris.snapshot();
        let p = state.player(VLC).unwrap();
        p.volume == Some(0.25) && p.shuffle == Some(true) && p.loop_status == Some(LoopStatus::Playlist)
    }));

    mpris.raise(VLC).wait().unwrap();
    assert!(calls.raised.load(Ordering::SeqCst));
    let methods = calls.methods.lock().unwrap().clone();
    assert_eq!(methods, ["Seek", "SetPosition", "Next", "Seek", "Pause"]);
}

#[test]
fn a_player_that_quits_disappears() {
    let bus = test_bus!();
    let (vlc, calls) = serve(&bus, VLC, "VLC media player", "Playing");
    let (_spotify, _) = serve(&bus, SPOTIFY, "Spotify", "Paused");
    let mpris = Mpris::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| mpris.snapshot().players.len() == 2));
    assert_eq!(mpris.snapshot().active.as_deref(), Some(VLC));

    mpris.quit(VLC).wait().unwrap();
    assert!(calls.quit.load(Ordering::SeqCst));
    // The fake leaves the bus.
    drop(vlc);
    assert!(eventually(|| mpris.snapshot().players.len() == 1));
    assert_eq!(mpris.snapshot().active.as_deref(), Some(SPOTIFY));
    assert!(matches!(mpris.play(VLC).wait(), Err(Error::NotFound(_))));

    // A player coming back under the same name is followed again.
    let (_vlc, _) = serve(&bus, VLC, "VLC media player", "Stopped");
    assert!(eventually(|| mpris.snapshot().players.len() == 2));
    assert_eq!(mpris.snapshot().player(VLC).unwrap().status, PlaybackStatus::Stopped);
}

#[test]
fn odd_players_show_what_they_can() {
    let bus = test_bus!();
    let conn = bus
        .builder()
        .name(ODD)
        .unwrap()
        .serve_at(PATH, OddPlayer { metadata: odd_metadata() })
        .unwrap()
        .build()
        .unwrap();
    let mpris = Mpris::with_bus(bus.bus(), Wake::none());
    assert!(eventually(|| mpris.snapshot().players.len() == 1));
    let state = mpris.snapshot();
    let odd = state.player(ODD).unwrap();
    // No root interface: the name comes from the bus name.
    assert_eq!(odd.identity, "odd");
    assert_eq!(odd.status, PlaybackStatus::Playing);
    assert_eq!(odd.metadata.title, "");
    assert_eq!(odd.metadata.artists, ["Radio Host"]);
    assert_eq!(odd.metadata.length, None);
    assert_eq!(odd.metadata.url, "https://radio.example/stream");
    assert_eq!((odd.volume, odd.loop_status, odd.shuffle), (None, None, None));
    assert_eq!(odd.last_position, Duration::from_secs(7), "a u64 position");
    assert!(odd.can_play && !odd.can_pause && !odd.can_seek);
    assert_eq!(state.active.as_deref(), Some(ODD));

    // A playing stream that can't pause: the play/pause key stops it.
    mpris.media_play_pause().wait().unwrap();
    // Its own refusals come back as errors.
    assert!(mpris.play_pause(ODD).wait().is_err());

    // Signals with the wrong types: metadata that isn't a dictionary, a
    // Seeked position as u64.
    let changed: HashMap<&str, Value<'_>> =
        HashMap::from([("Metadata", Value::from("nonsense")), ("PlaybackStatus", Value::from(3u8))]);
    conn.emit_signal(
        None::<()>,
        PATH,
        "org.freedesktop.DBus.Properties",
        "PropertiesChanged",
        &("org.mpris.MediaPlayer2.Player", changed, Vec::<String>::new()),
    )
    .unwrap();
    conn.emit_signal(None::<()>, PATH, "org.mpris.MediaPlayer2.Player", "Seeked", &(9_000_000u64,)).unwrap();
    assert!(eventually(|| {
        let state = mpris.snapshot();
        let odd = state.player(ODD).unwrap();
        odd.metadata.is_empty() && odd.status == PlaybackStatus::Stopped && odd.last_position == Duration::from_secs(9)
    }));
    // A Seeked without a position makes the player read again.
    conn.emit_signal(None::<()>, PATH, "org.mpris.MediaPlayer2.Player", "Seeked", &("soon",)).unwrap();
    assert!(eventually(|| mpris.snapshot().player(ODD).unwrap().last_position == Duration::from_secs(7)));
}

/// A player that takes its time to answer.
struct SlowRoot;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl SlowRoot {
    #[zbus(property)]
    async fn identity(&self) -> String {
        async_io::Timer::after(Duration::from_millis(500)).await;
        "Slow Player".into()
    }
}

#[test]
fn a_flood_of_signals_does_not_hold_up_replies() {
    let bus = test_bus!();
    let mpris = Mpris::with_bus(bus.bus(), Wake::none());
    std::thread::sleep(Duration::from_millis(100));
    let flood = bus.builder().build().unwrap();
    // While the slow player is being read, thousands of signals arrive on
    // the rules that hear every player; they mustn't block the reply.
    let _slow =
        bus.builder().name("org.mpris.MediaPlayer2.slow").unwrap().serve_at(PATH, SlowRoot).unwrap().build().unwrap();
    for position in 0..5000i64 {
        flood.emit_signal(None::<()>, PATH, "org.mpris.MediaPlayer2.Player", "Seeked", &(position,)).unwrap();
    }
    assert!(eventually(|| mpris.snapshot().players.len() == 1));
    assert_eq!(mpris.snapshot().players[0].identity, "Slow Player", "read in full, not timed out");
}

#[test]
fn without_players_the_keys_do_nothing() {
    let bus = test_bus!();
    let mpris = Mpris::with_bus(bus.bus(), Wake::none());
    std::thread::sleep(Duration::from_millis(200));
    assert!(mpris.available());
    assert!(mpris.snapshot().players.is_empty());
    assert_eq!(mpris.snapshot().active, None);
    assert!(matches!(mpris.media_play_pause().wait(), Err(Error::NotFound(_))));
    assert!(matches!(mpris.media_next().wait(), Err(Error::NotFound(_))));
    assert!(matches!(mpris.play(VLC).wait(), Err(Error::NotFound(_))));
}

#[test]
fn changes_wake_the_ui() {
    let bus = test_bus!();
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = woken.clone();
    let mpris = Mpris::with_bus(
        bus.bus(),
        Wake::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let seen = mpris.generation();
    let (_vlc, _) = serve(&bus, VLC, "VLC media player", "Paused");
    assert!(eventually(|| mpris.generation() > seen && mpris.snapshot().players.len() == 1));
    assert!(woken.load(Ordering::SeqCst) > 0);
}

#[test]
#[ignore = "reads the real session bus"]
fn lists_this_sessions_players() {
    let mpris = Mpris::new(Wake::none());
    assert!(mpris.available(), "no session bus");
    // Players appear as they're read; give them a moment.
    std::thread::sleep(Duration::from_millis(1500));
    let state = mpris.snapshot();
    println!("{} media player(s); active: {:?}", state.players.len(), state.active);
    for p in &state.players {
        println!(
            "  {} ({}) owner {} desktop={:?} {:?} rate={} volume={:?} shuffle={:?} loop={:?}",
            p.identity, p.bus_name, p.owner, p.desktop_entry, p.status, p.rate, p.volume, p.shuffle, p.loop_status
        );
        println!(
            "    can: control={} play={} pause={} next={} previous={} seek={} raise={} quit={}",
            p.can_control,
            p.can_play,
            p.can_pause,
            p.can_go_next,
            p.can_go_previous,
            p.can_seek,
            p.can_raise,
            p.can_quit
        );
        let m = &p.metadata;
        println!(
            "    track {:?}: {:?} by {:?} on {:?}, length {:?}, art {:?}, url {:?}",
            m.track_id,
            m.title,
            m.artist(),
            m.album,
            m.length,
            m.art_url,
            m.url
        );
        println!(
            "    position {:?} (read {:?}), estimated now {:?}",
            p.last_position,
            p.last_position_at.elapsed(),
            p.position()
        );
    }
}
