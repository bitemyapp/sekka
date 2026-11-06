//! The portal on a private session bus, as the frontend calls it, with a
//! stand-in chooser (a shell script) in place of the window: what the
//! chooser is handed, each of its answers as the frontend gets them, and
//! closing a request.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use aizome_portal::request::Request;
use aizome_portal::service::{CANCELLED, ENDED, FileChooser, SUCCESS};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

type Options = HashMap<String, OwnedValue>;

/// A `dbus-daemon` of our own that activates nothing. Its configuration
/// is ours too: a build sandbox has no /etc/dbus-1 for `--session`.
struct Bus {
    daemon: Child,
    address: String,
    dir: PathBuf,
}

impl Bus {
    fn new() -> Bus {
        // One folder per bus, as the tests run at once. Unix socket paths
        // are short (108 bytes), so a long `$TMPDIR` gives way to `/tmp`.
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let mut base = std::env::temp_dir();
        if base.as_os_str().len() > 40 && Path::new("/tmp").is_dir() {
            base = PathBuf::from("/tmp");
        }
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("portal-bus-{}-{n}", std::process::id()));
        // Left by an earlier run that had this process ID.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
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
        .unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
        assert!(!address.trim().is_empty(), "dbus-daemon did not start");
        Bus { daemon, address: address.trim().to_owned(), dir }
    }

    fn connect(&self) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(self.address.as_str()).unwrap().build().unwrap()
    }

    /// The portal, with `script` as its chooser.
    fn serve(&self, script: &str) -> zbus::blocking::Connection {
        let script = script.to_owned();
        let chooser = FileChooser::new(Arc::new(move || {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&script);
            c
        }));
        zbus::blocking::connection::Builder::address(self.address.as_str())
            .unwrap()
            .serve_at(aizome_portal::OBJECT_PATH, chooser)
            .unwrap()
            .name(aizome_portal::BUS_NAME)
            .unwrap()
            .build()
            .unwrap()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("portal-service-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn call(
    client: &zbus::blocking::Connection,
    method: &str,
    handle: &str,
    title: &str,
    options: Options,
) -> zbus::Result<(u32, Options)> {
    let proxy = zbus::blocking::Proxy::new(
        client,
        aizome_portal::BUS_NAME,
        aizome_portal::OBJECT_PATH,
        "org.freedesktop.impl.portal.FileChooser",
    )?;
    let handle = OwnedObjectPath::try_from(handle).unwrap();
    proxy.call(method, &(handle, "org.example.App", "wayland:exported-7", title, options))
}

fn options(pairs: Vec<(&str, Value<'static>)>) -> Options {
    pairs.into_iter().map(|(k, v)| (k.to_owned(), OwnedValue::try_from(v).unwrap())).collect()
}

#[test]
fn hands_the_request_over_and_returns_the_answer() {
    let bus = Bus::new();
    let dir = scratch("answer");
    let script = format!(
        "cat > {0}/request.json; printf '%s' '{{\"uris\":[\"file:///tmp/a%20b.png\"],\"choices\":[[\"q\",\"high\"]],\"current_filter\":null,\"writable\":true}}'",
        dir.display()
    );
    let _portal = bus.serve(&script);
    let client = bus.connect();
    let (response, results) = call(
        &client,
        "OpenFile",
        "/org/freedesktop/portal/desktop/request/1_1/t1",
        "Import",
        options(vec![("multiple", Value::from(true)), ("accept_label", Value::from("_Import"))]),
    )
    .unwrap();
    assert_eq!(response, SUCCESS);
    let uris: Vec<String> = Value::from(results["uris"].try_clone().unwrap()).try_into().unwrap();
    assert_eq!(uris, ["file:///tmp/a%20b.png"]);
    let choices: Vec<(String, String)> = Value::from(results["choices"].try_clone().unwrap()).try_into().unwrap();
    assert_eq!(choices, [("q".to_owned(), "high".to_owned())]);
    let writable: bool = Value::from(results["writable"].try_clone().unwrap()).try_into().unwrap();
    assert!(writable);
    // What the chooser read.
    let request: Request = serde_json::from_slice(&std::fs::read(dir.join("request.json")).unwrap()).unwrap();
    assert_eq!(request.app_id, "org.example.App");
    assert_eq!(request.wayland_parent(), Some("exported-7"));
    assert_eq!(request.title, "Import");
    assert_eq!(request.accept_label.as_deref(), Some("Import"));
    assert!(request.multiple);
}

#[test]
fn a_cancelled_chooser_and_a_failed_one() {
    let bus = Bus::new();
    let _portal = bus.serve("cat > /dev/null; echo null");
    let client = bus.connect();
    let (response, results) =
        call(&client, "SaveFile", "/org/freedesktop/portal/desktop/request/1_1/t2", "", Options::new()).unwrap();
    assert_eq!(response, CANCELLED);
    assert!(results.is_empty());
    drop(_portal);

    let _portal = bus.serve("cat > /dev/null; echo '{not json'; exit 0");
    let (response, _) =
        call(&client, "SaveFiles", "/org/freedesktop/portal/desktop/request/1_1/t3", "", Options::new()).unwrap();
    assert_eq!(response, ENDED);
    drop(_portal);

    let _portal = bus.serve("exit 4");
    let (response, _) =
        call(&client, "OpenFile", "/org/freedesktop/portal/desktop/request/1_1/t4", "", Options::new()).unwrap();
    assert_eq!(response, ENDED);
}

#[test]
fn closing_the_request_ends_the_chooser() {
    let bus = Bus::new();
    let dir = scratch("close");
    // A chooser that never answers, saying it started.
    let script = format!("cat > /dev/null; touch {0}/started; sleep 60", dir.display());
    let _portal = bus.serve(&script);
    let handle = "/org/freedesktop/portal/desktop/request/1_1/t5";
    let address = bus.address.clone();
    let asked = std::thread::spawn(move || {
        let client = zbus::blocking::connection::Builder::address(address.as_str()).unwrap().build().unwrap();
        call(&client, "OpenFile", handle, "", Options::new())
    });
    let until = Instant::now() + Duration::from_secs(10);
    while !dir.join("started").exists() {
        assert!(Instant::now() < until, "the chooser didn't start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let closed = Instant::now();
    let client = bus.connect();
    let request =
        zbus::blocking::Proxy::new(&client, aizome_portal::BUS_NAME, handle, "org.freedesktop.impl.portal.Request")
            .unwrap();
    let () = request.call("Close", &()).unwrap();
    let (response, _) = asked.join().unwrap().unwrap();
    assert_eq!(response, ENDED);
    assert!(closed.elapsed() < Duration::from_secs(5));
    // The request is gone from the bus with its call.
    let gone: zbus::Result<()> = request.call("Close", &());
    assert!(gone.is_err());
}
