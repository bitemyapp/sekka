//! A headless compositor must not publish its display to the login session.
//! Run the real binary, with private directories, disconnected buses and
//! recording stand-ins for the commands that would mutate shared state.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use aizome_ipc::{Client, Request, Response};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    dir: PathBuf,
    child: Option<Child>,
}

impl Fixture {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("aizome-session-test-{}-{n}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        for subdir in ["bin", "home", "config", "state", "data", "cache"] {
            fs::create_dir(dir.join(subdir)).unwrap();
        }
        Self { dir, child: None }
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.dir.join("bin").join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn wait_for(&mut self, what: &str, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !predicate() {
            assert!(
                self.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                "compositor exited while waiting for {what}: {}",
                self.log()
            );
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {}", self.log());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> String {
        fs::read_to_string(self.dir.join("compositor.log")).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if std::thread::panicking() {
            eprintln!("session fixture kept in {}\n{}", self.dir.display(), self.log());
        } else {
            fs::remove_dir_all(&self.dir).unwrap();
        }
    }
}

fn check_headless(invocation: bool, notify: bool, no_session: bool) {
    let mut f = Fixture::new();
    let commands = f.dir.join("commands");
    // Never invoke the host's manager or activation-environment tools,
    // including when this test runs against the broken implementation.
    for name in ["systemctl", "dbus-update-activation-environment"] {
        f.script(name, "printf '%s\\n' \"$0\" \"$@\" >> \"$COMMAND_LOG\"");
    }
    let probe = "printf '%s\\n' \"$WAYLAND_DISPLAY\" \"$AIZOME_SOCKET\" \"$AIZOME_SHELL_SOCKET\" \
                 \"$XDG_CURRENT_DESKTOP\" \"$AIZOME_SESSION\" > \"$1.tmp\"\nmv \"$1.tmp\" \"$1\"";
    f.script("probe", probe);
    f.script("aizome-shell", "exec probe \"$SHELL_ENV\"");
    let shell_env = f.dir.join("shell-env");
    let command_env = f.dir.join("command-env");
    let path = std::env::join_paths(
        std::iter::once(f.dir.join("bin")).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let notify_path = f.dir.join("notify");
    let notifications = UnixDatagram::bind(&notify_path).unwrap();
    notifications.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_aizome"));
    cmd.args([
        "--backend",
        "headless",
        "--renderer",
        "pixman",
        "--headless-output",
        "640x480",
        "--socket",
        "test-display",
        "--no-xwayland",
    ])
    .env("PATH", path)
    .env("COMMAND_LOG", &commands)
    .env("SHELL_ENV", &shell_env)
    .env("AIZOME_SHELL_BIN", f.dir.join("bin/aizome-shell"))
    .env("AIZOME_LOG", "warn")
    .env("HOME", f.dir.join("home"))
    .env("XDG_RUNTIME_DIR", &f.dir)
    .env("XDG_CONFIG_HOME", f.dir.join("config"))
    .env("XDG_STATE_HOME", f.dir.join("state"))
    .env("XDG_DATA_HOME", f.dir.join("data"))
    .env("XDG_CACHE_HOME", f.dir.join("cache"))
    .env("XDG_CONFIG_DIRS", f.dir.join("config"))
    .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}/absent-session-bus", f.dir.display()))
    .env("DBUS_SYSTEM_BUS_ADDRESS", format!("unix:path={}/absent-system-bus", f.dir.display()))
    // Model launching from an existing login session. These values
    // must be replaced for children, but never exported globally.
    .env("WAYLAND_DISPLAY", "login-display")
    .env("AIZOME_SOCKET", "/absent/login-ipc")
    .env("AIZOME_SHELL_SOCKET", "/absent/login-shell")
    .env("AIZOME_SESSION", "1")
    .env("XDG_CURRENT_DESKTOP", "OtherDesktop")
    .env_remove("WAYLAND_SOCKET")
    .env_remove("DISPLAY")
    .env_remove("INVOCATION_ID")
    .env_remove("NOTIFY_SOCKET")
    .env_remove("WATCHDOG_USEC")
    .env_remove("WATCHDOG_PID")
    .env_remove("AIZOME_LOGIC")
    .env_remove("AIZOME_RENDERER")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(fs::File::create(f.dir.join("compositor.log")).unwrap());
    if invocation {
        cmd.env("INVOCATION_ID", "00000000000000000000000000000001");
    }
    if notify {
        cmd.env("NOTIFY_SOCKET", &notify_path);
    }
    if no_session {
        cmd.arg("--no-session");
    }
    cmd.args(["--", "probe"]).arg(&command_env);
    f.child = Some(cmd.spawn().unwrap());
    let ipc = f.dir.join("aizome.test-display.sock");
    f.wait_for("the compositor event loop", || {
        Client::connect_to(&ipc).is_ok_and(|mut c| {
            c.stream().set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            c.request(&Request::Version).is_ok()
        })
    });
    f.wait_for("the shell and command environments", || shell_env.exists() && command_env.exists());
    let expected = format!(
        "test-display\n{}\n{}\nAizome\n0\n",
        ipc.display(),
        f.dir.join("aizome-shell.test-display.sock").display()
    );
    assert_eq!(fs::read_to_string(shell_env).unwrap(), expected);
    assert_eq!(fs::read_to_string(command_env).unwrap(), expected);
    assert!(
        !commands.exists(),
        "headless startup touched shared session state:\n{}",
        fs::read_to_string(&commands).unwrap_or_default()
    );

    let mut buf = [0; 256];
    if notify && !no_session {
        let n = notifications.recv(&mut buf).expect("a headless Type=notify service still reports readiness");
        assert_eq!(&buf[..n], b"READY=1\nSTATUS=Aizome is running");
    } else {
        let error = notifications.recv(&mut buf).expect_err("no readiness notification expected");
        assert!(matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut));
    }

    let mut client = Client::connect_to(&ipc).unwrap();
    client.stream().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert!(!matches!(client.request(&Request::Quit).unwrap(), Response::Error { .. }));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = f.child.as_mut().unwrap().try_wait().unwrap() {
            assert!(status.success(), "{}", f.log());
            break;
        }
        assert!(Instant::now() < deadline, "compositor did not quit: {}", f.log());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!commands.exists(), "headless shutdown touched shared session state");
}

#[test]
fn headless_without_no_session_does_not_publish_to_dbus() {
    check_headless(false, false, false);
}

#[test]
fn headless_systemd_invocation_does_not_publish_or_start_desktop_services() {
    check_headless(true, false, false);
}

#[test]
fn headless_notify_service_reports_readiness_without_taking_over_the_session() {
    check_headless(false, true, false);
}

#[test]
fn explicit_no_session_still_suppresses_publication_and_readiness() {
    check_headless(true, true, true);
}
