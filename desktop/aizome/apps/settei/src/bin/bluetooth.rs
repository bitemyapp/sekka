//! `aizome-bluetooth`: bluetoothd's LE privacy (`aizome_sys::bluetooth::privacy`
//! says what it is and why it's chosen as it is), as root.
//!
//! - `prepare`: what bluetoothd starts with. systemd runs it just before
//!   bluetoothd, every time (bluetooth.service's ExecStartPre), and
//!   bluetoothd reads the main.conf it writes in the system's place.
//!   Through pkexec it's refused: what it writes is what bluetoothd read.
//! - `set automatic|on|off`: the person's choice, for bluetoothd's next
//!   start. Settings runs it through pkexec under its own polkit action.
//! - `restart`: restart bluetoothd, so that it starts with what's chosen.
//!   Settings runs it likewise, when the person says to.

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use aizome_sys::bluetooth::privacy::{self, Choice, Running, Why};

fn main() {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        fail("this runs as root: by systemd, or through pkexec");
    }
    let through_pkexec = std::env::var_os("PKEXEC_UID").is_some();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["prepare"] if through_pkexec => Err("bluetoothd's start prepares it".into()),
        ["prepare"] => prepare(),
        ["set", choice] => match Choice::parse(choice) {
            Some(choice) => set(choice),
            None => Err(format!("no choice called {choice}")),
        },
        ["restart"] => restart(),
        _ => Err("usage: aizome-bluetooth prepare | set automatic|on|off | restart".into()),
    };
    if let Err(e) = result {
        fail(&e);
    }
}

fn fail(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(1);
}

/// `contents` at `path`, whole: a new file renamed over the old, readable
/// by all.
fn write(path: &Path, contents: &str) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let new = path.with_extension("new");
    let mut file = File::create(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    file.write_all(contents.as_bytes()).map_err(|e| e.to_string())?;
    file.set_permissions(std::fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&new, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn words(on: bool, why: Why) -> String {
    let why = match why {
        Why::System => "main.conf sets it",
        Why::Chosen => "chosen in Settings",
        Why::NoPairings => "nothing is paired over LE",
        Why::AlreadyOn => "it was on before, so devices paired since have the key",
        Why::EarlierPairings => "devices were paired over LE while it was off",
    };
    format!("LE privacy {}: {why}", if on { "on" } else { "off" })
}

fn prepare() -> Result<(), String> {
    // None: BlueZ's defaults, and privacy goes in as it would.
    let main_conf = std::fs::read_to_string(privacy::MAIN_CONF).unwrap_or_default();
    let source = std::fs::canonicalize(privacy::MAIN_CONF).map(|p| p.display().to_string()).unwrap_or_default();
    let choice = Choice::read_from(Path::new(privacy::CHOICE));
    let (on, why) = privacy::decide(choice, &main_conf, Path::new(privacy::STORAGE));
    let config = if why == Why::System { main_conf } else { privacy::render(&main_conf, on) };
    write(Path::new(privacy::CONFIG), &config)?;
    let running = Running { privacy: on, why, choice, main_conf: source };
    write(Path::new(privacy::RUNNING), &running.to_toml())?;
    eprintln!("{}", words(on, why));
    Ok(())
}

fn set(choice: Choice) -> Result<(), String> {
    let path = Path::new(privacy::CHOICE);
    match choice {
        Choice::Automatic => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
            _ => Ok(()),
        },
        _ => {
            let dir = path.parent().ok_or("no folder for the choice")?;
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
            write(path, &format!("{}\n", choice.word()))
        }
    }
}

/// systemctl, from the system's folders only (pkexec's PATH is root's,
/// but nothing here relies on it).
fn systemctl() -> Result<PathBuf, String> {
    ["/run/current-system/sw/bin", "/usr/bin", "/bin"]
        .iter()
        .map(|d| Path::new(d).join("systemctl"))
        .find(|p| p.is_file())
        .ok_or_else(|| "systemctl isn't installed".to_owned())
}

fn restart() -> Result<(), String> {
    let out = Command::new(systemctl()?)
        .args(["restart", "bluetooth.service"])
        .output()
        .map_err(|e| format!("systemctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if why.is_empty() { format!("restarting bluetooth.service failed ({})", out.status) } else { why })
    }
}
