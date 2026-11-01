//! A player served on a private bus, asked things as the shell asks them:
//! what it says of itself and of what plays, and what reaches the player.
//! Skips if `dbus-daemon` isn't there.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aizome_mpris::{Action, Identity, Mpris, State};
use zbus::blocking::Proxy;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedValue, Str};

/// A `dbus-daemon` of our own that activates nothing, killed when dropped.
struct TestBus {
    child: Child,
    address: String,
    dir: std::path::PathBuf,
}

impl TestBus {
    fn start() -> Option<TestBus> {
        let dir = std::env::temp_dir().join(format!("aizome-mpris-bus-{}", std::process::id()));
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
                eprintln!("skipping: no dbus-daemon ({e}); try `nix shell nixpkgs#dbus`");
                return None;
            }
        };
        let mut line = String::new();
        BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        let address = line.trim().to_owned();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(TestBus { child, address, dir })
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(t) = f() {
            return t;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

const BUS: &str = "org.mpris.MediaPlayer2.aizome_test";
const PATH: &str = "/org/mpris/MediaPlayer2";

#[test]
fn a_player_says_what_plays_and_hears_only_what_it_takes() {
    let Some(bus) = TestBus::start() else { return };
    // The player's session bus is the test's (the one test in this binary).
    unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &bus.address) };
    // As Chanoma is: not to be quit or asked to open anything, at one speed.
    let id = Identity {
        bus: "aizome_test",
        name: "Test Player",
        desktop_entry: "org.aizome.Test",
        uri_schemes: &[],
        mime_types: &[],
        can_quit: false,
        can_raise: true,
        rates: (1.0, 1.0),
    };
    let heard: Arc<Mutex<Vec<Action>>> = Arc::default();
    let h = heard.clone();
    let mpris = Mpris::start(id, move |a| h.lock().unwrap().push(a), || 12.5);
    mpris.publish(State {
        item: 1,
        title: "Episode 1".into(),
        status: "Playing",
        duration: 600.0,
        can_next: true,
        can_seek: true,
        ..State::default()
    });

    let conn = zbus::blocking::connection::Builder::address(bus.address.as_str()).unwrap().build().unwrap();
    let proxy = |iface: &'static str| -> Proxy<'_> {
        zbus::blocking::proxy::Builder::new(&conn)
            .destination(BUS)
            .unwrap()
            .path(PATH)
            .unwrap()
            .interface(iface)
            .unwrap()
            .cache_properties(CacheProperties::No)
            .build()
            .unwrap()
    };
    let root = proxy("org.mpris.MediaPlayer2");
    let player = proxy("org.mpris.MediaPlayer2.Player");
    let name: String = eventually("the player on the bus", || root.get_property("Identity").ok());
    assert_eq!(name, "Test Player");
    assert_eq!(root.get_property::<String>("DesktopEntry").unwrap(), "org.aizome.Test");
    assert!(!root.get_property::<bool>("CanQuit").unwrap());
    assert!(root.get_property::<Vec<String>>("SupportedUriSchemes").unwrap().is_empty());
    assert_eq!(player.get_property::<String>("PlaybackStatus").unwrap(), "Playing");
    assert_eq!(player.get_property::<i64>("Position").unwrap(), 12_500_000);
    assert_eq!(player.get_property::<f64>("MaximumRate").unwrap(), 1.0);
    let meta: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap();
    let title: Str = meta["xesam:title"].clone().try_into().unwrap();
    assert_eq!(title.as_str(), "Episode 1");
    let track: ObjectPath = meta["mpris:trackid"].clone().try_into().unwrap();
    assert_eq!(track.as_str(), "/org/aizome/Test/item/1");

    // What it takes reaches it; what it doesn't, doesn't.
    player.call_method("PlayPause", &()).unwrap();
    player.call_method("Next", &()).unwrap();
    player.call_method("Previous", &()).unwrap();
    root.call_method("Quit", &()).unwrap();
    player.call_method("OpenUri", &("file:///elsewhere.mkv",)).unwrap();
    player.call_method("Seek", &(10_000_000_i64,)).unwrap();
    player
        .call_method("SetPosition", &(ObjectPath::try_from("/org/aizome/Test/item/1").unwrap(), 30_000_000_i64))
        .unwrap();
    player
        .call_method("SetPosition", &(ObjectPath::try_from("/org/aizome/Test/item/2").unwrap(), 40_000_000_i64))
        .unwrap();
    root.call_method("Raise", &()).unwrap();
    let all = eventually("the raise", || {
        let a = heard.lock().unwrap().clone();
        a.contains(&Action::Raise).then_some(a)
    });
    assert_eq!(all, [Action::Toggle, Action::Next, Action::Seek(22.5), Action::Seek(30.0), Action::Raise]);

    // A change shows.
    mpris.publish(State { item: 1, title: "Episode 1".into(), status: "Paused", duration: 600.0, ..State::default() });
    eventually("paused", || (player.get_property::<String>("PlaybackStatus").ok()? == "Paused").then_some(()));
}
