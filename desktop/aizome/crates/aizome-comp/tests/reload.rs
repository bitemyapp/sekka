//! Reloading the compositor's logic under running clients: a test
//! client, foot and (where installed) Electron stay connected while the
//! logic is swapped for a variant built from the same sources (the focus
//! colour drawn pure red), a panic in the variant rolls it back, and a
//! session that starts without any logic shows its windows and takes a
//! logic in later. And restarting the shell: the apps stay, a kit app
//! finds the new shell, and notifications (on a D-Bus of the test's own)
//! carry over.
//!
//! The variant is built here, in a target directory of its own
//! (`target/reload-test`, kept between runs), with the compositor it is
//! loaded into, so both have the same core whatever `cargo test` was asked
//! to build.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use aizome_ipc::{Request, Response, Stats, WindowInfo};
use common::*;

struct Build {
    aizome: PathBuf,
    base: PathBuf,
    red: PathBuf,
}

/// The compositor and two logic libraries, as built from this checkout:
/// as it is, and with `AIZOME_LOGIC_VARIANT=red`.
fn build() -> &'static Build {
    static BUILD: OnceLock<Build> = OnceLock::new();
    BUILD.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let base_target =
            std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| workspace.join("target"));
        let target = base_target.join("reload-test");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| env!("CARGO").to_string());
        let run = |variant: Option<&str>| {
            let mut cmd = Command::new(&cargo);
            cmd.args(["build", "-p", "aizome-comp", "--manifest-path"])
                .arg(workspace.join("Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .env_remove("AIZOME_LOGIC_VARIANT");
            if let Some(v) = variant {
                cmd.env("AIZOME_LOGIC_VARIANT", v);
            }
            let status = cmd.status().expect("run cargo");
            assert!(status.success(), "building the compositor (variant {variant:?}) failed");
        };
        let logic = target.join("debug/libaizome_comp_logic.so");
        let base = target.join("logic-base.so");
        let red = target.join("logic-red.so");
        let abi = |p: &Path| {
            let bytes = std::fs::read(p).unwrap();
            let mark = b"AIZOME-LOGIC-ABI:";
            let at = bytes.windows(mark.len()).position(|w| w == mark).expect("the mark");
            bytes[at..at + mark.len() + 64].to_vec()
        };
        // Sidestep is a checkout beside this one that others may move on
        // meanwhile: then the three builds see different cores, and it's
        // built again.
        for _ in 0..3 {
            run(None);
            std::fs::copy(&logic, &base).unwrap();
            run(Some("red"));
            std::fs::copy(&logic, &red).unwrap();
            // Put the plain one back for the next run's first build.
            run(None);
            if abi(&base) == abi(&red) && abi(&red) == abi(&logic) {
                break;
            }
            eprintln!("the core changed while building (Sidestep moved?); again");
        }
        Build { aizome: target.join("debug/aizome"), base, red }
    })
}

fn stats(c: &Compositor) -> Stats {
    match c.request(Request::Stats { reset: false }) {
        Response::Stats { stats } => stats,
        r => panic!("{r:?}"),
    }
}

/// What must survive a reload: every window's place, group, Desk and
/// state, and which has focus.
fn desktop(ws: &[WindowInfo]) -> Vec<String> {
    let mut v: Vec<String> = ws
        .iter()
        .map(|w| {
            format!(
                "{} {} desk={:?} group={:?} members={:?} active={} focused={} min={} hidden={} fill={} {:?}",
                w.id,
                w.app_id,
                w.desk,
                w.group,
                w.group_members,
                w.group_active,
                w.focused,
                w.minimised,
                w.hidden,
                w.fill,
                (w.geometry.x, w.geometry.y, w.geometry.w, w.geometry.h)
            )
        })
        .collect();
    v.sort();
    v
}

/// Pixels of exactly the variant's red.
fn red_pixels(img: &Image) -> usize {
    img.rgba.chunks(4).filter(|p| p[0] >= 0xfa && p[1] <= 0x05 && p[2] <= 0x05).count()
}

struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Proc {
    fn running(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}

/// A program as a client of the test compositor.
fn spawn(c: &Compositor, argv: &[&str]) -> Proc {
    let child = Command::new(argv[0])
        .args(&argv[1..])
        .env_remove("DISPLAY")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env("WAYLAND_DISPLAY", &c.socket)
        .env("XDG_RUNTIME_DIR", &c.dir)
        .env("HOME", c.dir.join("home"))
        .env("XDG_CONFIG_HOME", c.dir.join("home/.config"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(
            std::fs::File::create(
                c.dir.join(format!("{}.log", Path::new(argv[0]).file_name().unwrap().to_string_lossy())),
            )
            .unwrap(),
        )
        .spawn()
        .unwrap_or_else(|e| panic!("{}: {e}", argv[0]));
    Proc(child)
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

const ELECTRON: &str = "/nix/store/nwn5gamww3fr51mwbrg7a0l9azrl3c9n-electron-44.5.1/bin/electron";

/// A small Electron app: one 400 × 300 window.
fn electron(c: &Compositor) -> Option<Proc> {
    let bin = std::env::var("AIZOME_TEST_ELECTRON").ok().map(PathBuf::from).unwrap_or_else(|| ELECTRON.into());
    if !bin.is_file() {
        eprintln!("no Electron at {}; testing without it", bin.display());
        return None;
    }
    let app = c.dir.join("electron-app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("package.json"), r#"{"name":"aizome-reload-test","main":"main.js"}"#).unwrap();
    std::fs::write(
        app.join("main.js"),
        r#"const { app, BrowserWindow } = require('electron');
app.whenReady().then(() => {
  const w = new BrowserWindow({ width: 400, height: 300, title: 'Electron under reload' });
  w.loadURL('data:text/html,<body style="background:%23884488">Electron</body>');
});"#,
    )
    .unwrap();
    let bin = bin.to_string_lossy().into_owned();
    let app = app.to_string_lossy().into_owned();
    Some(spawn(
        c,
        &[
            &bin,
            "--ozone-platform=wayland",
            "--enable-features=UseOzonePlatform",
            "--no-sandbox",
            "--disable-gpu",
            &app,
        ],
    ))
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn reloading_the_logic_keeps_every_client_and_the_desktop() {
    let b = build();
    let c = Compositor::start(Options {
        exe: Some(&b.aizome),
        logic: Some(&b.base),
        settings: "",
        outputs: &["1600x1000@1"],
        ..Default::default()
    });
    let base_build = stats(&c).logic.build;
    assert!(!base_build.is_empty(), "a logic is loaded:\n{}", c.log_text());

    // A test client in three windows, foot, and Electron.
    let a = c.client(Spec::new("org.example.a", 0x2266cc).size(420, 300));
    let a2 = c.client(Spec::new("org.example.a2", 0x22cc66).size(420, 300));
    let d = c.client(Spec::new("org.example.d", 0xcc6622).size(300, 200));
    let wa = c.wait_window("org.example.a");
    let wa2 = c.wait_window("org.example.a2");
    let wd = c.wait_window("org.example.d");
    let foot_bin = which("foot");
    let mut foot = foot_bin.as_ref().map(|f| spawn(&c, &[&f.to_string_lossy(), "--app-id", "foot", "sh"]));
    if foot.is_some() {
        c.wait_windows("foot's window", |ws| ws.iter().any(|w| w.app_id == "foot"));
    } else {
        eprintln!("foot isn't installed; testing without it");
    }
    let mut electron = electron(&c);
    let electron_pid = electron.as_ref().map(|e| e.0.id() as i32);
    if let Some(pid) = electron_pid {
        let deadline = Instant::now() + Duration::from_secs(40);
        while !c.windows().iter().any(|w| w.pid == Some(pid)) {
            assert!(Instant::now() < deadline, "Electron's window didn't open:\n{}", c.log_text());
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // A desktop to keep: a tab group, a window on Desk 2, one filled,
    // focus on the group's showing member.
    c.ok(Request::Stack { a: wa.id, b: wa2.id });
    c.ok(Request::MoveToDesk { id: Some(wd.id), desk: aizome_ipc::DeskSel::Index(2), follow: false });
    c.ok(Request::Focus { id: wa2.id });
    c.settle();
    let before = desktop(&c.windows());
    let desks_before = c.request(Request::Desks);
    let shot_a = c.screenshot();
    keep(&shot_a, "reload-before");
    assert_eq!(red_pixels(&shot_a), 0, "the plain logic draws no pure red");

    // Swap in the variant.
    let report = match c.request(Request::Reload {
        logic: true,
        shell: false,
        from: Some(b.red.to_string_lossy().into()),
        shell_from: None,
        check: false,
    }) {
        Response::Reload { report } => report,
        r => panic!("{r:?}"),
    };
    assert_eq!(report.logic.action, "reloaded", "{report:?}\n{}", c.log_text());
    c.settle();
    let s = stats(&c);
    assert_ne!(s.logic.build, base_build, "the variant is another build");
    assert_eq!(s.logic.reloads, 1);
    assert_eq!(desktop(&c.windows()), before, "windows, groups, Desks and focus survive the reload");
    assert_eq!(c.request(Request::Desks), desks_before);
    for (cl, name) in [(&a, "a"), (&a2, "a2"), (&d, "d")] {
        assert!(cl.connected(), "test client {name} is still connected");
    }
    if let Some(f) = &mut foot {
        assert!(f.running(), "foot is still running");
    }
    if let Some(e) = &mut electron {
        assert!(e.running(), "Electron is still running");
    }
    let shot_b = c.screenshot();
    keep(&shot_b, "reload-variant");
    assert!(red_pixels(&shot_b) > 200, "the variant draws the focused tab red: {} red pixels", red_pixels(&shot_b));

    // The variant fails: back to the logic before it, desktop intact.
    match c.request(Request::Action { action: "test-panic".into() }) {
        Response::Error { .. } => {}
        r => panic!("a panicking logic answers with an error, not {r:?}"),
    }
    wait_for("the rollback", 10, || {
        let s = stats(&c);
        s.logic.rollbacks == 1 && s.logic.healthy && s.logic.build == base_build
    });
    let s = stats(&c);
    assert!(s.logic.last_error.as_deref().is_some_and(|e| e.contains("panics on purpose")), "{:?}", s.logic);
    c.settle();
    assert_eq!(desktop(&c.windows()), before, "the rollback keeps the desktop");
    let shot_c = c.screenshot();
    keep(&shot_c, "reload-rolled-back");
    assert_eq!(red_pixels(&shot_c), 0, "the plain logic draws again");
    for (cl, name) in [(&a, "a"), (&a2, "a2"), (&d, "d")] {
        assert!(cl.connected(), "test client {name} survived the rollback");
    }
    if let Some(f) = &mut foot {
        assert!(f.running(), "foot survived the rollback");
    }
    if let Some(e) = &mut electron {
        assert!(e.running(), "Electron survived the rollback");
        assert!(c.windows().iter().any(|w| w.pid == electron_pid), "Electron's window is still there");
    }

    // The windows still take input from the reloaded logic: focus moves.
    c.ok(Request::Focus { id: wd.id });
    let ws = c.windows();
    assert!(ws.iter().find(|w| w.id == wd.id).unwrap().focused);
    assert_eq!(ws.iter().find(|w| w.id == wd.id).unwrap().desk, Some(2));
}

#[test]
fn without_a_logic_windows_still_show_and_a_reload_brings_it() {
    let b = build();
    let missing = Path::new("/nonexistent/libaizome_comp_logic.so");
    let c =
        Compositor::start(Options { exe: Some(&b.aizome), logic: Some(missing), settings: "", ..Default::default() });
    assert!(stats(&c).logic.build.is_empty(), "no logic is loaded");
    let a = c.client(Spec::new("org.example.bare", 0x33aa77).size(300, 200));
    let w = c.wait_window("org.example.bare");
    assert!(w.focused, "the recovery path focuses a new window");
    c.settle();
    let img = c.screenshot();
    let (x, y) = (w.geometry.x + 150, w.geometry.y + 100);
    assert_px(&img, x, y, rgb(0x33aa77), "the window, drawn without a logic");

    match c.request(Request::Reload {
        logic: true,
        shell: false,
        from: Some(b.base.to_string_lossy().into()),
        shell_from: None,
        check: false,
    }) {
        Response::Reload { report } => assert_eq!(report.logic.action, "reloaded", "{report:?}"),
        r => panic!("{r:?}"),
    }
    let ws = c.windows();
    let after = ws.iter().find(|x| x.app_id == "org.example.bare").expect("the logic took the window in");
    assert_eq!((after.id, after.geometry.x, after.geometry.y), (w.id, w.geometry.x, w.geometry.y), "where it was");
    assert!(a.connected());
    c.settle();
    // Now with its tab and frame.
    let img = c.screenshot();
    assert_px(&img, x, y, rgb(0x33aa77), "the window");
    assert!(
        !near(img.px(w.geometry.x + 2, w.geometry.y - 12), img.px(w.geometry.x + 2, w.geometry.y - 60)),
        "a tab above it"
    );
}

#[test]
fn a_library_built_against_another_core_is_refused() {
    let b = build();
    let c = Compositor::start(Options { exe: Some(&b.aizome), logic: Some(&b.base), ..Default::default() });
    // A copy whose ABI mark says another core.
    let mut bytes = std::fs::read(&b.base).unwrap();
    let mark = b"AIZOME-LOGIC-ABI:";
    let at = bytes.windows(mark.len()).position(|w| w == mark).expect("the mark") + mark.len();
    bytes[at] = if bytes[at] == b'0' { b'1' } else { b'0' };
    let other = c.dir.join("other-core.so");
    std::fs::write(&other, &bytes).unwrap();
    let report = match c.request(Request::Reload {
        logic: true,
        shell: false,
        from: Some(other.to_string_lossy().into()),
        shell_from: None,
        check: true,
    }) {
        Response::Reload { report } => report,
        r => panic!("{r:?}"),
    };
    assert_eq!(report.logic.action, "refused", "{report:?}");
    let report = match c.request(Request::Reload {
        logic: true,
        shell: false,
        from: Some(other.to_string_lossy().into()),
        shell_from: None,
        check: false,
    }) {
        Response::Reload { report } => report,
        r => panic!("{r:?}"),
    };
    assert_eq!(report.logic.action, "refused", "{report:?}");
    assert!(report.logic.detail.contains("another compositor core"), "{report:?}");
    assert_eq!(report.core.action, "same", "the installed core is the running one: {report:?}");
    let s = stats(&c);
    assert!(s.logic.healthy, "the running logic stays");
    assert_eq!(s.logic.reloads, 0);
}

/// The shell and a kit app, built from this checkout into the same
/// target directory as [`build`].
struct ShellBuild {
    shell: PathBuf,
    app: PathBuf,
}

fn shell_build() -> &'static ShellBuild {
    static BUILD: OnceLock<ShellBuild> = OnceLock::new();
    BUILD.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let base_target =
            std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| workspace.join("target"));
        let target = base_target.join("reload-test");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| env!("CARGO").to_string());
        let status = Command::new(&cargo)
            .args(["build", "-p", "aizome-shell", "-p", "aizome-tokei", "--manifest-path"])
            .arg(workspace.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target)
            .status()
            .expect("run cargo");
        assert!(status.success(), "building the shell failed");
        ShellBuild { shell: target.join("debug/aizome-shell"), app: target.join("debug/aizome-tokei") }
    })
}

/// A D-Bus daemon of the test's own, with no services to start: the
/// session bus (and the system bus, so the shell's logind, polkit and
/// NetworkManager parts reach nothing of the machine's).
struct Bus {
    _daemon: Proc,
    dir: PathBuf,
    address: String,
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn private_bus() -> Option<Bus> {
    use std::io::BufRead;
    let daemon = which("dbus-daemon")?;
    let dir = PathBuf::from(format!("/tmp/aizome-test-{}-bus", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let conf = dir.join("bus.conf");
    std::fs::write(
        &conf,
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
    let mut child = Command::new(daemon)
        .arg(format!("--config-file={}", conf.display()))
        .args(["--nofork", "--nopidfile", "--print-address=1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).ok()?;
    let address = line.trim().to_string();
    assert!(!address.is_empty(), "dbus-daemon printed no address");
    Some(Bus { _daemon: Proc(child), dir, address })
}

/// The test's connection to that bus.
fn bus_connection(address: &str) -> zbus::blocking::Connection {
    zbus::blocking::connection::Builder::address(address).unwrap().build().unwrap()
}

/// Posts a notification as an app would; its id.
fn notify(bus: &zbus::blocking::Connection, summary: &str) -> u32 {
    let hints: std::collections::HashMap<&str, zbus::zvariant::Value> = Default::default();
    let reply = bus
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("reload-test", 0u32, "", summary, "From the reload test", Vec::<&str>::new(), hints, -1i32),
        )
        .unwrap_or_else(|e| panic!("Notify({summary}): {e}"));
    reply.body().deserialize::<u32>().unwrap()
}

/// The pid of whoever holds the notifications' bus name.
fn notifications_owner(bus: &zbus::blocking::Connection) -> Option<u32> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).ok()?;
    let name = zbus::names::BusName::try_from("org.freedesktop.Notifications").ok()?;
    dbus.get_connection_unix_process_id(name).ok()
}

/// What the shell says it shows (`aizome-shell msg state`).
fn shell_state(shell: &Path, c: &Compositor) -> Option<serde_json::Value> {
    let out = Command::new(shell)
        .args(["msg", "state"])
        .env("AIZOME_SHELL_SOCKET", shell_socket(c))
        .env("XDG_RUNTIME_DIR", &c.dir)
        .env("WAYLAND_DISPLAY", &c.socket)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

fn shell_socket(c: &Compositor) -> PathBuf {
    c.dir.join(format!("aizome-shell.{}.sock", c.socket))
}

fn strings(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[test]
fn restarting_the_shell_keeps_apps_and_notifications() {
    let Some(bus) = private_bus() else {
        eprintln!("no dbus-daemon; skipping the shell restart test");
        return;
    };
    let sb = shell_build();
    let shell_bin = sb.shell.to_string_lossy().into_owned();
    let env = [
        ("AIZOME_SHELL_BIN", shell_bin.as_str()),
        ("DBUS_SESSION_BUS_ADDRESS", bus.address.as_str()),
        ("DBUS_SYSTEM_BUS_ADDRESS", bus.address.as_str()),
        ("AIZOME_LOG", "info,smithay=warn"),
    ];
    let c = Compositor::start(Options { args: &["--shell"], env: &env, settings: "", ..Default::default() });
    let conn = bus_connection(&bus.address);

    // The shell is up: its surfaces drawn, the notifications' name held.
    let mut first = None;
    wait_for("the shell", 60, || {
        first = shell_state(&sb.shell, &c).filter(|s| !strings(s, "surfaces").is_empty());
        first.is_some() && notifications_owner(&conn).is_some()
    });
    let first = first.unwrap();
    let old_pid = first["pid"].as_u64().unwrap() as u32;
    assert_eq!(notifications_owner(&conn), Some(old_pid));
    // The shell lists its surfaces as soon as one draws, which can be
    // before the bar's exclusive zone is in: wait for the bar's area to be
    // taken from the top (read too early, it failed this test now and then).
    let mut outputs_before = c.request(Request::Outputs);
    wait_for("the bar's exclusive zone", 60, || {
        outputs_before = c.request(Request::Outputs);
        matches!(&outputs_before, Response::Outputs { outputs }
            if outputs.first().is_some_and(|o| o.usable.y > o.logical.y))
    });

    // Apps: a test client, foot, and a kit app talking to the shell.
    let a = c.client(Spec::new("org.example.a", 0x2266cc).size(420, 300));
    c.wait_window("org.example.a");
    let mut foot = which("foot").map(|f| spawn(&c, &[&f.to_string_lossy(), "--app-id", "foot", "sh"]));
    if foot.is_some() {
        c.wait_windows("foot's window", |ws| ws.iter().any(|w| w.app_id == "foot"));
    }
    let mut app = {
        let child = Command::new(&sb.app)
            .env_remove("DISPLAY")
            .env("WAYLAND_DISPLAY", &c.socket)
            .env("XDG_RUNTIME_DIR", &c.dir)
            .env("HOME", c.dir.join("home"))
            .env("XDG_CONFIG_HOME", c.dir.join("config"))
            .env("AIZOME_SOCKET", &c.ipc)
            .env("AIZOME_SHELL_SOCKET", shell_socket(&c))
            .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
            .env("DBUS_SYSTEM_BUS_ADDRESS", &bus.address)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(c.dir.join("tokei.log")).unwrap())
            .spawn()
            .expect("start the kit app");
        Proc(child)
    };
    let app_id = "org.aizome.Tokei";
    wait_for("the kit app to say hello to the shell", 60, || {
        shell_state(&sb.shell, &c).is_some_and(|s| strings(&s, "apps").iter().any(|a| a == app_id))
    });
    c.wait_windows("the kit app's window", |ws| ws.iter().any(|w| w.app_id == app_id));
    c.settle();
    let windows_before = desktop(&c.windows());

    let n1 = notify(&conn, "Before the restart");
    wait_for("the shell to keep it", 10, || {
        shell_state(&sb.shell, &c).is_some_and(|s| strings(&s, "notes").contains(&"Before the restart".into()))
    });

    // Where the bar and the desktop show (not a window, not a banner).
    let before = c.screenshot();
    keep(&before, "shell-before-restart");
    let points = shell_points(&c, &before);
    let colours: Vec<_> = points.iter().map(|&(x, y)| before.px(x, y)).collect();

    // Restart it.
    let report = match c.request(Request::Reload {
        logic: false,
        shell: false,
        from: None,
        shell_from: Some(shell_bin.clone()),
        check: false,
    }) {
        Response::Reload { report } => report,
        r => panic!("{r:?}"),
    };
    assert_eq!(report.shell.action, "restarted", "{report:?}\n{}", c.log_text());
    assert!(report.shell.detail.contains(&shell_bin), "the shell asked for: {report:?}");
    assert_eq!(report.logic.action, "skipped", "{report:?}");
    // One comes in while the two shells overlap.
    let n2 = notify(&conn, "During the restart");
    // The bar and the desktop never leave the screen: every frame through
    // the overlap shows one shell's or the other's.
    let mut frames = 0;
    while !c.log_text().contains("shell restart finished") {
        let img = c.screenshot();
        for (&(x, y), &want) in points.iter().zip(&colours) {
            if !near(img.px(x, y), want) {
                keep(&img, "shell-during-restart");
                panic!("at ({x}, {y}) the shell's {want:?} became {:?} mid-restart", img.px(x, y));
            }
        }
        frames += 1;
        assert!(frames < 400, "the restart didn't finish:\n{}", c.log_text());
    }
    eprintln!("{frames} screenshots through the restart, the bar and desktop at {points:?} in each");

    let mut new_pid = 0;
    wait_for("the new shell to take over", 30, || {
        let Some(owner) = notifications_owner(&conn) else { return false };
        new_pid = owner;
        owner != old_pid && !pid_alive(old_pid)
    });
    let mut state = None;
    wait_for("the new shell to hold every notification", 20, || {
        state = shell_state(&sb.shell, &c);
        state.as_ref().is_some_and(|s| {
            let notes = strings(s, "notes");
            s["pid"].as_u64() == Some(new_pid as u64)
                && notes.contains(&"Before the restart".into())
                && notes.contains(&"During the restart".into())
                && !strings(s, "surfaces").is_empty()
        })
    });
    let n3 = notify(&conn, "After the restart");
    assert!(n3 > n1 && n3 > n2, "ids go on: {n1} {n2} {n3}");
    wait_for("the new shell to keep a new one", 10, || {
        shell_state(&sb.shell, &c).is_some_and(|s| strings(&s, "notes").contains(&"After the restart".into()))
    });

    // The apps never noticed; the kit app found the new shell.
    assert!(a.connected(), "the test client is still connected");
    if let Some(f) = &mut foot {
        assert!(f.running(), "foot is still running");
    }
    assert!(app.running(), "the kit app is still running");
    wait_for("the kit app to say hello to the new shell", 10, || {
        shell_state(&sb.shell, &c).is_some_and(|s| strings(&s, "apps").iter().any(|a| a == app_id))
    });
    c.settle();
    assert_eq!(desktop(&c.windows()), windows_before, "windows keep their places");
    assert_eq!(c.request(Request::Outputs), outputs_before, "the bar's area is the same");
    let log = c.log_text();
    assert!(log.contains("shell restart finished"), "{log}");
}

/// Points where the shell's bar and desktop show: the bar's top row, in
/// the middle, and a spot of the desktop's icons (the desktop's own
/// colour is also the compositor's backdrop, so that wouldn't tell).
fn shell_points(c: &Compositor, img: &Image) -> Vec<(i32, i32)> {
    let Response::Outputs { outputs } = c.request(Request::Outputs) else { panic!("no outputs") };
    let o = &outputs[0];
    let (l, u) = (&o.logical, &o.usable);
    assert!(u.y > l.y, "the bar keeps the top of the screen: {o:?}");
    let ws = c.windows();
    let covered = |x: i32, y: i32| {
        ws.iter().any(|w| {
            let g = &w.geometry;
            // With room for decorations.
            x >= g.x - 60 && x <= g.x + g.w + 60 && y >= g.y - 60 && y <= g.y + g.h + 60
        })
    };
    let backdrop = img.px(l.x + l.w - 2, l.y + l.h - 2);
    let mut desk = None;
    'search: for y in u.y + 4..u.y + 160 {
        for x in u.x + 4..u.x + 160 {
            let p = img.px(x, y);
            let even = (-1..=1).all(|dy| (-1..=1).all(|dx| img.px(x + dx, y + dy) == p));
            if even && !near(p, backdrop) && !covered(x, y) {
                desk = Some((x, y));
                break 'search;
            }
        }
    }
    let desk = desk.expect("the desktop's icons");
    let bar = (l.x + l.w / 2, l.y + 1);
    assert!(!near(img.px(bar.0, bar.1), backdrop), "the bar shows");
    vec![bar, desk]
}
