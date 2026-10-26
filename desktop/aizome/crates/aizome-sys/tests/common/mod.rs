//! A private D-Bus daemon per test, so fake daemons (NetworkManager, BlueZ,
//! UPower, UDisks2…) can be served without touching the real buses.

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A `dbus-daemon` of our own, killed when dropped. Its configuration
/// allows everything and activates nothing, so no real service can start
/// on it.
pub struct TestBus {
    child: Child,
    pub address: String,
    dir: PathBuf,
}

impl TestBus {
    /// Start a daemon, or `None` (the test then skips) when `dbus-daemon`
    /// isn't installed.
    pub fn start() -> Option<TestBus> {
        let dir = std::env::temp_dir().join(format!(
            "aizome-sys-bus-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
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
            Ok(child) => child,
            Err(error) => {
                eprintln!("skipping: no dbus-daemon ({error}); try `nix shell nixpkgs#dbus`");
                return None;
            }
        };
        let stdout = child.stdout.take()?;
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).ok()?;
        let address = line.trim().to_owned();
        if address.is_empty() {
            let _ = child.kill();
            return None;
        }
        Some(TestBus { child, address, dir })
    }

    /// A zbus connection builder for this bus.
    pub fn builder(&self) -> zbus::blocking::connection::Builder<'static> {
        zbus::blocking::connection::Builder::address(self.address.as_str()).expect("bus address")
    }

    /// The bus, for the services under test.
    pub fn bus(&self) -> aizome_sys::Bus {
        aizome_sys::Bus::at(&self.address).expect("connect to the test bus")
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Wait up to 5 seconds for `condition`; true if it came.
pub fn eventually(mut condition: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

/// Start a test bus or return from the test.
#[macro_export]
macro_rules! test_bus {
    () => {
        match common::TestBus::start() {
            Some(bus) => bus,
            None => return,
        }
    };
}

/// An object path.
pub fn path(p: &str) -> zbus::zvariant::OwnedObjectPath {
    zbus::zvariant::OwnedObjectPath::try_from(p.to_owned()).expect("object path")
}
