//! Optional end-to-end UI check with the real shell and installed apps.
//! See docs/ipc.md for the command. No connection to the login session's
//! display, configuration, browser profile or D-Bus is used.

mod common;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aizome_ipc::{Request, Synthetic};
use common::*;

struct Program(Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(c: &Compositor, bin: &str) -> Command {
    let mut cmd = Command::new(bin);
    cmd.env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", &c.socket)
        .env("XDG_RUNTIME_DIR", &c.dir)
        .env("XDG_CONFIG_HOME", c.dir.join("config"))
        .env("XDG_STATE_HOME", c.dir.join("state"))
        .env("XDG_DATA_HOME", c.dir.join("data"))
        .env("XDG_CACHE_HOME", c.dir.join("cache"))
        .env("HOME", c.dir.join("home"))
        .env("AIZOME_SOCKET", &c.ipc)
        .env("AIZOME_SHELL_SOCKET", c.dir.join(format!("aizome-shell.{}.sock", c.socket)))
        .env("AIZOME_SESSION", "0")
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}/no-bus", c.dir.display()))
        .env("DBUS_SYSTEM_BUS_ADDRESS", format!("unix:path={}/no-bus", c.dir.display()));
    cmd
}

fn program(c: &Compositor, bin: &str, args: &[&str], name: &str) -> Program {
    Program(
        command(c, bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(c.dir.join(format!("{name}.log"))).unwrap())
            .spawn()
            .unwrap(),
    )
}

fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn shell_state(c: &Compositor, bin: &str) -> Option<serde_json::Value> {
    let out = command(c, bin).args(["msg", "state"]).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| serde_json::from_slice(&out.stdout).ok()).flatten()
}

fn shell_msg(c: &Compositor, bin: &str, message: serde_json::Value) {
    assert!(command(c, bin).args(["msg", &message.to_string()]).status().unwrap().success());
}

fn key(c: &Compositor, keys: &str) {
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] });
}

#[test]
#[ignore = "requires AIZOME_LAYOUT_TEST_SHELL and AIZOME_LAYOUT_TEST_CHROME; see docs/ipc.md"]
fn real_launcher_chrome_and_calculator_keep_the_deskbar_clear() {
    let shell = std::env::var("AIZOME_LAYOUT_TEST_SHELL").expect("shell binary");
    let chrome = std::env::var("AIZOME_LAYOUT_TEST_CHROME").expect("Chrome binary");
    assert!(Path::new(&shell).is_file() && Path::new(&chrome).is_file());
    let c = Compositor::start(Options {
        outputs: &["2880x1800@2"],
        // Ensure launched programs also cannot inherit the login session's bus.
        env: &[
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent-aizome-layout-bus"),
            ("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent-aizome-layout-bus"),
        ],
        ..Default::default()
    });
    let _shell = program(&c, &shell, &[], "shell");
    wait("the shell's Deskbar", || {
        shell_state(&c, &shell)
            .is_some_and(|s| s["surfaces"].as_array().is_some_and(|ss| ss.iter().any(|v| v == "Deskbar")))
    });
    c.settle();
    let profile = format!("--user-data-dir={}/chrome", c.dir.display());
    let _chrome = program(
        &c,
        &chrome,
        &[
            &profile,
            "--ozone-platform=wayland",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-background-mode",
            "--disable-gpu",
            "--window-size=1800,1200",
            "about:blank",
        ],
        "chrome",
    );
    let windows = c.wait_windows("Chrome", |ws| !ws.is_empty());
    let browser = windows[0].id;
    c.wait_windows("Chrome's default fits", |ws| {
        ws.iter().find(|w| w.id == browser).is_some_and(|w| w.geometry.x + w.geometry.w <= 1208)
    });
    let _tab = c.client(Spec::new("layout-tab", 0x6688aa).title("Layout regression"));
    let tab = c.wait_window("layout-tab");
    c.ok(Request::Stack { a: browser, b: tab.id });
    c.ok(Request::Fill { id: Some(browser), state: Some(true) });
    c.ok(Request::Focus { id: browser });
    c.settle();
    let before = c.windows().into_iter().find(|w| w.id == browser).unwrap().geometry;
    keep(&c.screenshot(), "layout-desktop-before");

    // Use the actual launcher and its keyboard path, as in the recording.
    shell_msg(&c, &shell, serde_json::json!({"request":"palette", "scope":"apps", "query":"Soroban"}));
    wait("the launcher", || shell_state(&c, &shell).is_some_and(|s| s["palette"] == "Soroban"));
    c.settle();
    key(&c, "return");
    c.wait_windows("Calculator launched", |ws| ws.iter().any(|w| w.app_id == "org.aizome.Soroban"));
    wait("launcher closed", || shell_state(&c, &shell).is_some_and(|s| s["palette"].is_null()));
    c.settle();
    for w in c.windows().into_iter().filter(|w| w.id == browser || w.id == tab.id) {
        assert_eq!(w.geometry, before, "launch changed {}", w.app_id);
        assert!(w.geometry.x + w.geometry.w <= 1208);
    }
    keep(&c.screenshot(), "layout-desktop-after-launch");
    // Repeat with keyboard open/escape, and toggle the Deskbar while grouped.
    key(&c, "super+space");
    wait("palette reopened", || shell_state(&c, &shell).is_some_and(|s| !s["palette"].is_null()));
    key(&c, "escape");
    wait("palette dismissed", || shell_state(&c, &shell).is_some_and(|s| s["palette"].is_null()));
    shell_msg(&c, &shell, serde_json::json!({"request":"deskbar", "collapsed":true}));
    c.wait_windows("Deskbar collapsed", |ws| {
        ws.iter().any(|w| w.id == browser && w.geometry.w == if w.client_side_decorations { 1440 } else { 1438 })
    });
    shell_msg(&c, &shell, serde_json::json!({"request":"deskbar", "collapsed":false}));
    c.wait_windows("Deskbar restored", |ws| ws.iter().any(|w| w.id == browser && w.geometry == before));
    c.ok(Request::Focus { id: tab.id });
    c.settle();
    assert_eq!(c.window("layout-tab").unwrap().geometry, before);
    keep(&c.screenshot(), "layout-desktop-after-toggle");
    for w in c.windows() {
        c.ok(Request::Close { id: Some(w.id) });
    }
}
