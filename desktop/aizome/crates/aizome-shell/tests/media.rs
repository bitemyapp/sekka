//! The sound panel's media row follows the player: its track and state as
//! they change, and ⏮ ⏯ ⏭ acting on it, against a fake MPRIS player on a
//! private bus.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aizome_shell::sys::Sys;
use aizome_shell::systray::panels::{Media, MediaVerb};
use aizome_sys::Wake;
use aizome_sys::mpris::Mpris;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const TITLES: [&str; 2] = ["First Light", "Second Wind"];

/// A `dbus-daemon` of our own that activates nothing, killed when dropped.
struct PrivateBus {
    child: Child,
    address: String,
    dir: std::path::PathBuf,
}

impl PrivateBus {
    fn start() -> Option<PrivateBus> {
        let dir = std::env::temp_dir().join(format!("aizome-shell-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                dir.display()
            ),
        )
        .ok()?;
        let mut child = match Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                eprintln!("skipping: no dbus-daemon ({e})");
                return None;
            }
        };
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        Some(PrivateBus { child, address: line.trim().to_owned(), dir })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Fake Player".into()
    }
}

struct Player {
    playing: bool,
    track: usize,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn play_pause(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.playing = !self.playing;
        self.playback_status_changed(&emitter).await?;
        Ok(())
    }
    async fn next(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.track = (self.track + 1) % TITLES.len();
        self.metadata_changed(&emitter).await?;
        Ok(())
    }
    async fn previous(&mut self, #[zbus(signal_emitter)] emitter: SignalEmitter<'_>) -> fdo::Result<()> {
        self.track = (self.track + TITLES.len() - 1) % TITLES.len();
        self.metadata_changed(&emitter).await?;
        Ok(())
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        if self.playing { "Playing" } else { "Paused" }.into()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let id = OwnedObjectPath::try_from(format!("/test/track/{}", self.track)).unwrap();
        let owned = |v: Value<'_>| OwnedValue::try_from(v).unwrap();
        HashMap::from([
            ("mpris:trackid".to_owned(), owned(Value::from(id))),
            ("xesam:title".to_owned(), owned(Value::from(TITLES[self.track]))),
            ("xesam:artist".to_owned(), owned(Value::from(vec!["The Testers"]))),
        ])
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
}

fn eventually(what: &str, mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(5), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_media_row_follows_the_player_and_its_buttons() {
    let Some(bus) = PrivateBus::start() else { return };
    let _player = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.mpris.MediaPlayer2.fake")
        .unwrap()
        .serve_at("/org/mpris/MediaPlayer2", Root)
        .unwrap()
        .serve_at("/org/mpris/MediaPlayer2", Player { playing: true, track: 0 })
        .unwrap()
        .build()
        .unwrap();
    let sys = Sys {
        mpris: Some(Mpris::with_bus(aizome_sys::Bus::at(&bus.address).unwrap(), Wake::none())),
        ..Sys::default()
    };
    let media = |title: &str, playing: bool| Media { title: title.into(), artist: "The Testers".into(), playing };

    eventually("the player", || sys.media() == Some(media(TITLES[0], true)));
    // The row changes with the player, without the panel being opened again.
    sys.media_control(MediaVerb::Next);
    eventually("the next track", || sys.media() == Some(media(TITLES[1], true)));
    sys.media_control(MediaVerb::PlayPause);
    eventually("the pause", || sys.media() == Some(media(TITLES[1], false)));
    sys.media_control(MediaVerb::Previous);
    eventually("the previous track", || sys.media() == Some(media(TITLES[0], false)));
}
