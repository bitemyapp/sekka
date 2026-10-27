//! A media player to try the sound panel's media controls with: it serves
//! MPRIS as `org.mpris.MediaPlayer2.aizome_test` on the session bus and
//! plays its "tracks", steady tones, with `pw-play`.
//!
//! ```text
//! cargo run -p aizome-sys --example test_player [-- --target SINK]
//! ```
//!
//! Play, Pause, PlayPause, Next, Previous and Stop work, and each change is
//! signalled as a player's would be. `--target` names the sink to play into
//! (by default PipeWire's default). Run it against a private PipeWire and a
//! private session bus: it plays sound.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

const NAME: &str = "org.mpris.MediaPlayer2.aizome_test";
const PATH: &str = "/org/mpris/MediaPlayer2";
const RATE: u32 = 16_000;
const SECONDS: u32 = 300;

/// The playlist: title and pitch.
const TRACKS: [(&str, f64); 3] = [("Tone in A", 440.0), ("Tone in E", 659.25), ("Tone in C", 523.25)];
const ARTIST: &str = "Aizome Test Signals";

fn owned<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    OwnedValue::try_from(value.into()).expect("no file descriptors")
}

/// A mono 16-bit WAV of a sine at `hz`, at a quarter of full scale.
fn tone(hz: f64) -> Vec<u8> {
    let samples = RATE * SECONDS;
    let data = samples * 2;
    let mut wav = Vec::with_capacity(44 + data as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data.to_le_bytes());
    for n in 0..samples {
        let t = f64::from(n) / f64::from(RATE);
        let s = (t * hz * std::f64::consts::TAU).sin() * 0.25 * f64::from(i16::MAX);
        wav.extend_from_slice(&(s as i16).to_le_bytes());
    }
    wav
}

/// What plays: one `pw-play` at a time.
struct Sound {
    files: Vec<std::path::PathBuf>,
    target: Option<String>,
    child: Option<Child>,
}

impl Sound {
    fn play(&mut self, track: usize) {
        self.stop();
        let mut cmd = Command::new("pw-play");
        if let Some(t) = &self.target {
            cmd.arg("--target").arg(t);
        }
        cmd.arg("--media-role").arg("Music").arg(&self.files[track]);
        match cmd.stdin(Stdio::null()).spawn() {
            Ok(child) => self.child = Some(child),
            Err(e) => eprintln!("pw-play: {e}"),
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}
    fn quit(&self) {
        std::process::exit(0);
    }
    #[zbus(property)]
    fn identity(&self) -> String {
        "Aizome Test Player".into()
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
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
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    playing: bool,
    stopped: bool,
    track: usize,
    sound: Arc<Mutex<Sound>>,
}

impl Player {
    fn sound(&self) -> std::sync::MutexGuard<'_, Sound> {
        self.sound.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn set(&mut self, playing: bool, stopped: bool, emitter: &SignalEmitter<'_>) -> fdo::Result<()> {
        self.playing = playing;
        self.stopped = stopped;
        if playing {
            let track = self.track;
            self.sound().play(track);
        } else {
            self.sound().stop();
        }
        println!("{} {}", self.playback_status(), TRACKS[self.track].0);
        self.playback_status_changed(emitter).await?;
        Ok(())
    }

    async fn go(&mut self, by: usize, emitter: &SignalEmitter<'_>) -> fdo::Result<()> {
        self.track = (self.track + by) % TRACKS.len();
        self.metadata_changed(emitter).await?;
        let playing = self.playing;
        self.set(playing, !playing && self.stopped, emitter).await
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn play_pause(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        let playing = !self.playing;
        self.set(playing, false, &emitter).await
    }
    async fn play(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.set(true, false, &emitter).await
    }
    async fn pause(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.set(false, false, &emitter).await
    }
    async fn stop(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.set(false, true, &emitter).await
    }
    async fn next(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.go(1, &emitter).await
    }
    async fn previous(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.go(TRACKS.len() - 1, &emitter).await
    }
    fn seek(&self, _offset: i64) {}
    fn set_position(&self, _track: ObjectPath<'_>, _position: i64) {}
    fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> String {
        match (self.playing, self.stopped) {
            (true, _) => "Playing",
            (false, true) => "Stopped",
            (false, false) => "Paused",
        }
        .into()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let id = OwnedObjectPath::try_from(format!("/org/aizome/test_player/track/{}", self.track))
            .expect("a valid object path");
        HashMap::from([
            ("mpris:trackid".to_owned(), owned(id)),
            ("xesam:title".to_owned(), owned(TRACKS[self.track].0)),
            ("xesam:artist".to_owned(), owned(vec![ARTIST])),
            ("xesam:album".to_owned(), owned("Pitch Pipe")),
            ("mpris:length".to_owned(), owned(i64::from(SECONDS) * 1_000_000)),
        ])
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        1.0
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        0
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
        false
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target = args.iter().position(|a| a == "--target").and_then(|i| args.get(i + 1)).cloned();
    let dir = std::env::temp_dir().join(format!("aizome-test-player-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut files = Vec::new();
    for (i, (_, hz)) in TRACKS.iter().enumerate() {
        let file = dir.join(format!("track{i}.wav"));
        std::fs::write(&file, tone(*hz))?;
        files.push(file);
    }
    let sound = Arc::new(Mutex::new(Sound { files, target, child: None }));
    sound.lock().unwrap_or_else(|e| e.into_inner()).play(0);
    let _conn = zbus::blocking::connection::Builder::session()?
        .name(NAME)?
        .serve_at(PATH, Root)?
        .serve_at(PATH, Player { playing: true, stopped: false, track: 0, sound: sound.clone() })?
        .build()?;
    println!("serving {NAME}: Playing {}", TRACKS[0].0);
    loop {
        std::thread::park();
    }
}
