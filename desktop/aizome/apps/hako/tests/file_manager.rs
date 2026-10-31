//! Real D-Bus calls to the shipped executable on a private bus. A socket
//! stands in for an already-open Hako, and a script on PATH for a Hako
//! started anew; no host desktop or bus is touched.

#![cfg(feature = "gui")]

use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hako::file_manager::{BUS_NAME, FileManager, MAX_WINDOWS, OBJECT_PATH, connect};
use hako::model::uri::file_uri;
use hako::ui::{Request, instance};
use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::fdo::RequestNameFlags;

/// A folder of the test's own, removed when dropped. Unix socket paths
/// are short (108 bytes), so a long `$TMPDIR` gives way to `/tmp`.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let mut base = std::env::temp_dir();
        if base.as_os_str().len() > 40 && Path::new("/tmp").is_dir() {
            base = PathBuf::from("/tmp");
        }
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("hako-fm-{}-{n}", std::process::id()));
        // Left by an earlier run that had this process ID.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A `dbus-daemon` of our own that activates nothing. Its configuration
/// is ours too: a build sandbox has no /etc/dbus-1.
struct Bus {
    _daemon: Process,
    address: String,
    _dir: Scratch,
}

impl Bus {
    fn new() -> Self {
        let dir = Scratch::new();
        let config = dir.0.join("bus.conf");
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
                dir.0.display()
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
        Self { _daemon: Process(daemon), address: address.trim().into(), _dir: dir }
    }

    fn builder(&self) -> Builder<'_> {
        Builder::address(self.address.as_str()).unwrap()
    }

    fn client(&self) -> Connection {
        self.builder().build().unwrap()
    }
}

/// The shipped executable serving FileManager1 on `bus`, in a session of
/// its own under `root`. Its PATH starts with a stand-in `aizome-hako`
/// that records how it was started, so no real Hako is ever opened.
fn service(bus: &Bus, root: &Path, environment: &[(&str, &str)]) -> Process {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let hako = bin.join("aizome-hako");
    let record = root.join("launched");
    std::fs::write(
        &hako,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"${{XDG_ACTIVATION_TOKEN-unset}}\" \"${{DESKTOP_STARTUP_ID-unset}}\" \"$@\" > '{0}.tmp'\nmv '{0}.tmp' '{0}'\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hako, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())))
            .unwrap();
    let mut service = Process(
        Command::new(env!("CARGO_BIN_EXE_aizome-hako"))
            .arg("--file-manager-service")
            .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
            .env("XDG_RUNTIME_DIR", root)
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("WAYLAND_DISPLAY", "test")
            .env("PATH", path)
            .envs(environment.iter().copied())
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
    service
}

/// What the stand-in `aizome-hako` was started with: its activation
/// token, its startup ID and its arguments.
fn launched(root: &Path) -> Vec<String> {
    let record = root.join("launched");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !record.exists() {
        assert!(Instant::now() < deadline, "Hako was not started");
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = std::fs::read_to_string(&record).unwrap();
    std::fs::remove_file(&record).unwrap();
    text.lines().map(str::to_owned).collect()
}

fn call(client: &Connection, method: &str, uris: &[String], startup_id: &str) -> zbus::Result<()> {
    Proxy::new(client, BUS_NAME, OBJECT_PATH, BUS_NAME)?.call(method, &(uris, startup_id))
}

/// One handoff to the stand-in Hako: its requests and its token.
fn received(listener: &UnixListener) -> (Vec<Request>, Option<String>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut text = String::new();
            stream.read_to_string(&mut text).unwrap();
            return (instance::decode(&text), instance::token(&text));
        }
        assert!(Instant::now() < deadline, "Hako did not receive the request");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn no_owner(dbus: &zbus::blocking::fdo::DBusProxy<'_>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while dbus.name_has_owner(BUS_NAME.try_into().unwrap()).unwrap() {
        assert!(Instant::now() < deadline, "FileManager1 was not released");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn browser_calls_reach_hakos_existing_instance() {
    let bus = Bus::new();
    let root = Scratch::new();
    let listener = UnixListener::bind(root.0.join("aizome-app.hako.test.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _service = service(&bus, &root.0, &[]);
    let client = bus.client();
    // A folder must be selected in its parent by ShowItems, but opened
    // directly by ShowFolders. Include URI escaping and shell metacharacters.
    let folder = root.0.join("Downloads");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("naïve #1%\n$(literal).pdf");
    call(&client, "ShowItems", &[file_uri(&file), file_uri(&folder)], "browser-token").unwrap();
    assert_eq!(
        received(&listener),
        (vec![Request::Reveal(file.clone()), Request::Reveal(folder.clone())], Some("browser-token".into()))
    );
    call(&client, "ShowFolders", &[file_uri(&folder)], "").unwrap();
    assert_eq!(received(&listener), (vec![Request::Open(folder.clone())], None));

    // Reject a whole mixed batch, and more windows than a call may open.
    // Empty calls do not open a home window.
    for bad in ["https://example.org/file", "file://remote/path", "file:///tmp/a%00b"] {
        assert!(call(&client, "ShowItems", &[file_uri(&file), bad.into()], "").is_err());
    }
    let many: Vec<String> = (0..=MAX_WINDOWS).map(|i| file_uri(&root.0.join(format!("{i}/file")))).collect();
    assert!(call(&client, "ShowItems", &many, "").is_err());
    assert!(call(&client, "ShowItems", &many[..MAX_WINDOWS], "").is_ok());
    assert_eq!(received(&listener).0.len(), MAX_WINDOWS);
    call(&client, "ShowItems", &[], "").unwrap();
    assert!(call(&client, "ShowItemProperties", &[file_uri(&file)], "").is_err());
    // Each call answers only after its handoff, so anything the calls above
    // had sent would arrive before this one.
    call(&client, "ShowFolders", &[file_uri(&root.0)], "").unwrap();
    assert_eq!(received(&listener).0, vec![Request::Open(root.0.clone())]);
    assert!(!root.0.join("launched").exists(), "a running Hako was started again");
}

#[test]
fn with_no_hako_running_one_starts_with_the_callers_token() {
    let bus = Bus::new();
    let root = Scratch::new();
    // Tokens left in the service's own environment are no one's to use.
    let _service = service(&bus, &root.0, &[("XDG_ACTIVATION_TOKEN", "stale"), ("DESKTOP_STARTUP_ID", "stale")]);
    let client = bus.client();
    let file = root.0.join("a b\nc.pdf");
    call(&client, "ShowItems", &[file_uri(&file)], "browser-token").unwrap();
    assert_eq!(launched(&root.0), ["browser-token", "browser-token", "--select", &file_uri(&file)]);
    call(&client, "ShowFolders", &[file_uri(&root.0)], "").unwrap();
    assert_eq!(launched(&root.0), ["unset", "unset", &file_uri(&root.0)]);
}

#[test]
fn session_ownership_is_exclusive_and_released_on_exit() {
    let bus = Bus::new();
    let root = Scratch::new();
    let manager = || FileManager::new(root.0.join("no-hako.sock"), PathBuf::from("/does-not-exist/hako"));
    let old = bus.builder().name(BUS_NAME).unwrap().allow_name_replacements(true).build().unwrap();
    let service = connect(bus.builder(), manager()).unwrap();
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
    assert!(call(&client, "ShowItems", &["file:///tmp/download".into()], "").is_err());
    drop(old);
    drop(service);
    // The bus sees a connection close only some time after it is dropped.
    no_owner(&dbus);
    client.request_name_with_flags(BUS_NAME, RequestNameFlags::DoNotQueue.into()).unwrap();
    // Do not silently queue if a non-replaceable owner was there first.
    assert!(matches!(connect(bus.builder(), manager()), Err(zbus::Error::NameTaken)));
}
