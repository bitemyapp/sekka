//! `aizome-awake`: Keep Awake (see the library's docs).
//!
//!     aizome-awake on | off | toggle   turn it on or off; kept for the next login
//!     aizome-awake status              {"enabled": …, "protected": …}
//!     aizome-awake requested           succeeds when it's on (the unit's ExecCondition)
//!     aizome-awake hold                the service: take the locks and keep them

use std::fs::{self, File, OpenOptions};
use std::os::fd::{AsRawFd, OwnedFd};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use aizome_awake::{Inhibitor, KeepAwakeHold, UNIT, WHAT, WHO, WHY, protected_by, requested, setting, state_dir};

type Result<T> = std::result::Result<T, String>;

const LOGIN1: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let result = match arg.as_str() {
        "on" => set(true),
        "off" => set(false),
        "toggle" => set(!requested()),
        "status" => status(),
        "requested" => {
            if requested() {
                Ok(())
            } else {
                Err("Keep Awake is off".into())
            }
        }
        "hold" => hold(),
        _ => Err("usage: aizome-awake on|off|toggle|status".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aizome-awake: {e}");
            ExitCode::FAILURE
        }
    }
}

fn uid() -> u32 {
    // Safety: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

// ---- logind ------------------------------------------------------------------

fn system_bus() -> Result<zbus::blocking::Connection> {
    zbus::blocking::Connection::system().map_err(|e| format!("the system bus: {e}"))
}

fn inhibitors(bus: &zbus::blocking::Connection) -> Result<Vec<Inhibitor>> {
    let reply = bus
        .call_method(Some(LOGIN1), MANAGER_PATH, Some(MANAGER), "ListInhibitors", &())
        .map_err(|e| format!("asking logind for its inhibitors: {e}"))?;
    reply.body().deserialize().map_err(|e| format!("reading logind's inhibitors: {e}"))
}

/// Takes the block inhibitor; logind keeps it while the descriptor is open.
fn inhibit(bus: &zbus::blocking::Connection) -> Result<OwnedFd> {
    let reply = bus
        .call_method(Some(LOGIN1), MANAGER_PATH, Some(MANAGER), "Inhibit", &(WHAT, WHO, WHY, "block"))
        .map_err(|e| format!("logind refused the sleep inhibitor: {e}"))?;
    let fd: zbus::zvariant::OwnedFd =
        reply.body().deserialize().map_err(|e| format!("reading the inhibitor from logind: {e}"))?;
    Ok(fd.into())
}

fn protected() -> Result<bool> {
    Ok(protected_by(&inhibitors(&system_bus()?)?, uid(), None))
}

// ---- on, off, status ---------------------------------------------------------

/// One change at a time (two clicks in the menu bar, or the palette and a
/// terminal at once).
fn edit_lock() -> Result<File> {
    fs::create_dir_all(state_dir()).map_err(|e| format!("{}: {e}", state_dir().display()))?;
    let path = state_dir().join("keep-awake.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // Safety: the descriptor is open for the lock's lifetime, the file's.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(format!("locking {}: {}", path.display(), std::io::Error::last_os_error()));
    }
    Ok(file)
}

fn save(on: bool) -> Result<()> {
    let path = setting();
    if on {
        let new = path.with_extension("new");
        fs::write(&new, "on\n").and_then(|()| fs::rename(&new, &path))
    } else {
        match fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn set(on: bool) -> Result<()> {
    let _lock = edit_lock()?;
    save(on)?;
    let verb = if on { "start" } else { "stop" };
    let ok = Command::new("systemctl")
        .args(["--user", verb, UNIT])
        .status()
        .map_err(|e| format!("systemctl: {e}"))?
        .success();
    if !ok {
        return Err(format!("couldn't {verb} Keep Awake; see `systemctl --user status {UNIT}`"));
    }
    // Started, the service is ready only once logind lists its locks;
    // stopped, logind drops them as its descriptor closes. Either way,
    // believe logind, not systemd.
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if protected()? == on {
            break;
        }
        if Instant::now() > until {
            if !on {
                return Err("Keep Awake stopped, but logind still lists its sleep inhibitor".into());
            }
            // A start whose condition failed succeeds too, and leaves
            // nothing running.
            let running = Command::new("systemctl")
                .args(["--user", "--quiet", "is-active", UNIT])
                .status()
                .is_ok_and(|s| s.success());
            return Err(if running {
                "Keep Awake is running, but logind doesn't list its sleep inhibitor".into()
            } else {
                format!("Keep Awake's service didn't start; see `systemctl --user status {UNIT}`")
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("Keep Awake is {}", if on { "on" } else { "off" });
    Ok(())
}

fn status() -> Result<()> {
    println!("{}", serde_json::json!({ "enabled": requested(), "protected": protected()? }));
    Ok(())
}

// ---- the service -------------------------------------------------------------

/// Takes logind's locks and the compositor's hold on idleness and keeps
/// them, checking every second that logind still lists the locks (a
/// restarted logind forgets them) and that the compositor is still there.
/// Losing either ends this with an error, and systemd starts it again,
/// taking them afresh.
fn hold() -> Result<()> {
    let bus = system_bus()?;
    let _lock = inhibit(&bus)?;
    let me = std::process::id();
    if !protected_by(&inhibitors(&bus)?, uid(), Some(me)) {
        return Err("logind doesn't list the inhibitor it gave".into());
    }
    let mut idle = KeepAwakeHold::take()?;
    if !idle.asked() {
        // An older Aizome, or another desktop: held as an app would.
        eprintln!("aizome-awake: the compositor doesn't know keep-awake; holding a Wayland idle inhibitor instead");
    }
    sd_notify("READY=1\nSTATUS=Sleep, idle and lid-close sleep are blocked");
    loop {
        idle.wait(Duration::from_secs(1))?;
        if !protected_by(&inhibitors(&bus)?, uid(), Some(me)) {
            return Err("logind no longer lists the inhibitor; taking it again".into());
        }
    }
}

/// Tells systemd (`Type=notify`) without libsystemd.
fn sd_notify(message: &str) {
    use std::os::unix::net::UnixDatagram;
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else { return };
    let Ok(socket) = UnixDatagram::unbound() else { return };
    let bytes = path.as_encoded_bytes();
    let _ = if let Some(name) = bytes.strip_prefix(b"@") {
        use std::os::linux::net::SocketAddrExt;
        std::os::unix::net::SocketAddr::from_abstract_name(name)
            .and_then(|addr| socket.send_to_addr(message.as_bytes(), &addr))
    } else {
        socket.send_to(message.as_bytes(), &path)
    };
}
