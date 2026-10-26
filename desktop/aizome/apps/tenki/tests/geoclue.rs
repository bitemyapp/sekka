//! Tenki's GeoClue client against a fake GeoClue on a private bus (its own
//! `dbus-daemon`, which can activate nothing): the position it gives, a
//! refusal, and no GeoClue at all. Skips if `dbus-daemon` isn't there.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use aizome_tenki::location::{LocateError, locate_on};
use zbus::zvariant::OwnedObjectPath;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A `dbus-daemon` of our own, killed when dropped.
struct TestBus {
    child: Child,
    address: String,
    dir: PathBuf,
}

impl TestBus {
    fn start() -> Option<TestBus> {
        let dir = std::env::temp_dir().join(format!(
            "tenki-bus-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
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
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping: no dbus-daemon ({e})");
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

    fn connect(&self) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .expect("an address")
            .build()
            .expect("connected")
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn path(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p.to_owned()).expect("a path")
}

const CLIENT: &str = "/org/freedesktop/GeoClue2/Client/1";
const LOCATION: &str = "/org/freedesktop/GeoClue2/Location/1";

struct Manager;

#[zbus::interface(name = "org.freedesktop.GeoClue2.Manager")]
impl Manager {
    fn get_client(&self) -> OwnedObjectPath {
        path(CLIENT)
    }
}

struct Client {
    deny: bool,
    started: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    desktop_id: String,
    accuracy: u32,
    running: bool,
}

#[zbus::interface(name = "org.freedesktop.GeoClue2.Client")]
impl Client {
    fn start(&mut self) -> zbus::fdo::Result<()> {
        if self.deny {
            return Err(zbus::fdo::Error::AccessDenied("'org.aizome.Tenki' disallowed, no agent".into()));
        }
        self.running = true;
        self.started.lock().unwrap().push(format!("{} {}", self.desktop_id, self.accuracy));
        Ok(())
    }

    fn stop(&mut self) {
        self.running = false;
    }

    #[zbus(property)]
    fn location(&self) -> OwnedObjectPath {
        path(if self.running { LOCATION } else { "/" })
    }

    #[zbus(property)]
    fn desktop_id(&self) -> String {
        self.desktop_id.clone()
    }

    #[zbus(property)]
    fn set_desktop_id(&mut self, id: String) {
        self.desktop_id = id;
    }

    #[zbus(property)]
    fn requested_accuracy_level(&self) -> u32 {
        self.accuracy
    }

    #[zbus(property)]
    fn set_requested_accuracy_level(&mut self, level: u32) {
        self.accuracy = level;
    }
}

struct Location;

#[zbus::interface(name = "org.freedesktop.GeoClue2.Location")]
impl Location {
    #[zbus(property)]
    fn latitude(&self) -> f64 {
        43.0731
    }

    #[zbus(property)]
    fn longitude(&self) -> f64 {
        -89.4012
    }

    #[zbus(property)]
    fn accuracy(&self) -> f64 {
        5000.0
    }

    #[zbus(property)]
    fn description(&self) -> String {
        String::new()
    }
}

fn serve(bus: &TestBus, deny: bool) -> (zbus::blocking::Connection, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let started = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let client = Client { deny, started: started.clone(), desktop_id: String::new(), accuracy: 0, running: false };
    let conn = zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.freedesktop.GeoClue2")
        .unwrap()
        .serve_at("/org/freedesktop/GeoClue2/Manager", Manager)
        .unwrap()
        .serve_at(CLIENT, client)
        .unwrap()
        .serve_at(LOCATION, Location)
        .unwrap()
        .build()
        .expect("the fake GeoClue on the bus");
    (conn, started)
}

#[test]
fn geoclue_gives_a_position_at_city_accuracy() {
    let Some(bus) = TestBus::start() else { return };
    let (_geoclue, started) = serve(&bus, false);
    let me = bus.connect();
    let found = locate_on(&me, "org.aizome.Tenki", Duration::from_secs(3)).expect("a position");
    assert_eq!((found.latitude, found.longitude), (43.0731, -89.4012));
    assert_eq!(found.accuracy, 5000.0);
    assert_eq!(found.description, None, "an empty description is none");
    // Asked as Tenki, for a city's accuracy (4).
    assert_eq!(started.lock().unwrap().as_slice(), ["org.aizome.Tenki 4"]);
}

#[test]
fn a_refusal_is_a_denial() {
    let Some(bus) = TestBus::start() else { return };
    let (_geoclue, _) = serve(&bus, true);
    let me = bus.connect();
    assert_eq!(locate_on(&me, "org.aizome.Tenki", Duration::from_secs(3)), Err(LocateError::Denied));
}

#[test]
fn no_geoclue_is_unavailable() {
    let Some(bus) = TestBus::start() else { return };
    let me = bus.connect();
    match locate_on(&me, "org.aizome.Tenki", Duration::from_secs(3)) {
        Err(LocateError::Unavailable(_)) => {}
        other => panic!("expected unavailable, got {other:?}"),
    }
}
