//! Real D-Bus calls to the shipped executable on a private bus. A socket
//! stands in for an already-open Hako; no host desktop or bus is touched.

#![cfg(feature = "gui")]

use std::io::{BufRead, BufReader, Read};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hako::file_manager::{BUS_NAME, OBJECT_PATH, connect};
use hako::model::uri::file_uri;
use hako::ui::{Request, instance};
use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::fdo::RequestNameFlags;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Bus {
    _daemon: Process,
    address: String,
}

impl Bus {
    fn new() -> Self {
        let mut daemon = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().unwrap()).read_line(&mut address).unwrap();
        Self { _daemon: Process(daemon), address: address.trim().into() }
    }

    fn builder(&self) -> Builder<'_> {
        Builder::address(self.address.as_str()).unwrap()
    }

    fn client(&self) -> Connection {
        self.builder().build().unwrap()
    }
}

fn call(client: &Connection, method: &str, uris: &[String]) -> zbus::Result<()> {
    Proxy::new(client, BUS_NAME, OBJECT_PATH, BUS_NAME)?.call(method, &(uris, "test-startup-id"))
}

fn received(listener: &UnixListener) -> Vec<Request> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut text = String::new();
            stream.read_to_string(&mut text).unwrap();
            return instance::decode(&text);
        }
        assert!(Instant::now() < deadline, "Hako did not receive the request");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn browser_calls_reach_hakos_existing_instance() {
    let bus = Bus::new();
    let root = std::env::temp_dir().join(format!("hako-file-manager-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let socket = root.join("aizome-app.hako.test.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut service = Process(
        Command::new(env!("CARGO_BIN_EXE_aizome-hako"))
            .arg("--file-manager-service")
            .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
            .env("XDG_RUNTIME_DIR", &root)
            .env("HOME", &root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("WAYLAND_DISPLAY", "test")
            .spawn()
            .unwrap(),
    );
    let client = bus.client();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !dbus.name_has_owner(BUS_NAME.try_into().unwrap()).unwrap() {
        assert!(service.0.try_wait().unwrap().is_none(), "service exited");
        assert!(Instant::now() < deadline, "service did not claim FileManager1");
        std::thread::sleep(Duration::from_millis(10));
    }
    // A folder must be selected in its parent by ShowItems, but opened
    // directly by ShowFolders. Include URI escaping and shell metacharacters.
    let folder = root.join("Downloads");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("naïve #1%\n$(literal).pdf");
    call(&client, "ShowItems", &[file_uri(&file), file_uri(&folder)]).unwrap();
    assert_eq!(received(&listener), vec![Request::Reveal(file.clone()), Request::Reveal(folder.clone())]);
    call(&client, "ShowFolders", &[file_uri(&folder)]).unwrap();
    assert_eq!(received(&listener), vec![Request::Open(folder)]);

    // Reject a whole mixed batch; empty calls do not open a home window.
    for bad in ["https://example.org/file", "file://remote/path", "file:///tmp/a%00b"] {
        assert!(call(&client, "ShowItems", &[file_uri(&file), bad.into()]).is_err());
    }
    call(&client, "ShowItems", &[]).unwrap();
    assert!(call(&client, "ShowItemProperties", &[file_uri(&file)]).is_err());
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    drop(service);
    drop(listener);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn session_ownership_is_exclusive_and_released_on_exit() {
    let bus = Bus::new();
    let old = bus.builder().name(BUS_NAME).unwrap().allow_name_replacements(true).build().unwrap();
    let service = connect(bus.builder(), PathBuf::from("/does-not-exist/hako")).unwrap();
    let client = bus.client();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&client).unwrap();
    assert_eq!(
        dbus.get_name_owner(BUS_NAME.try_into().unwrap()).unwrap().as_str(),
        service.unique_name().unwrap().as_str()
    );
    // A later file manager cannot displace the Aizome provider.
    assert!(
        client
            .request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting)
            .is_err()
    );
    // Failure to launch is reported to the caller, allowing its fallback.
    assert!(call(&client, "ShowItems", &["file:///tmp/download".into()]).is_err());
    drop(old);
    drop(service);
    client.request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into()).unwrap();
    // Do not silently queue if a non-replaceable owner was there first.
    assert!(connect(bus.builder(), PathBuf::from("/does-not-exist/hako")).is_err());
}
