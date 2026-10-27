//! Real desktop clients isolated from the login session. Optional tests
//! supply executable paths; every process uses the test compositor's home,
//! sockets, browser profile and deliberately disconnected D-Bus addresses.
use super::Compositor;
use aizome_ipc::{Request, Response, Synthetic};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Program(pub Child);

impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn command(c: &Compositor, bin: &str) -> Command {
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

pub fn program(c: &Compositor, bin: &str, args: &[&str], name: &str) -> Program {
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

pub fn wait(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn shell_state(c: &Compositor, bin: &str) -> Option<serde_json::Value> {
    let out = command(c, bin).args(["msg", "state"]).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| serde_json::from_slice(&out.stdout).ok()).flatten()
}

pub fn shell_msg(c: &Compositor, bin: &str, message: serde_json::Value) {
    assert!(command(c, bin).args(["msg", &message.to_string()]).status().unwrap().success());
}

pub fn key(c: &Compositor, keys: &str) {
    c.ok(Request::Synthesize { events: vec![Synthetic::Chord { keys: keys.into() }] });
}

/// Shell state can report an open palette before its first surface commit.
/// In these isolated sessions the palette is the only keyboard-taking layer;
/// wait for focus to leave the app before sending input intended for it.
pub fn wait_layer_keyboard_focus(c: &Compositor) {
    wait("shell layer keyboard focus", || {
        matches!(c.request(Request::InputState), Response::InputState { state }
            if state.keyboard_focus.as_deref() == Some("a surface"))
    });
}
