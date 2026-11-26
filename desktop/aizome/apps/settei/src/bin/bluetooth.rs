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

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use aizome_sys::bluetooth::privacy::{self, Choice, Pairings, Running, Why};

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

/// `contents` at `path`, whole, with `mode` from the start: a file of this
/// write's own beside it, renamed over the old (two writers at once each
/// rename their own).
fn write(path: &Path, contents: &str, mode: u32) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("no folder for {}", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let new = dir.join(format!(".{name}.{}.{n}.new", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&new)
            .map_err(|e| format!("{}: {e}", new.display()))?;
        // One left by a crash, made with another mode.
        file.set_permissions(std::fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
        file.write_all(contents.as_bytes()).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&new, path).map_err(|e| format!("{}: {e}", path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    result
}

fn words(on: bool, why: Why) -> String {
    let why = match why {
        Why::System => "main.conf sets it",
        Why::Chosen => "chosen in Settings",
        Why::NoPairings => "nothing is paired over LE",
        Why::AllHaveKey => "every device paired over LE has the key",
        Why::EarlierPairings => "a device paired over LE may not have the key",
        Why::Unprepared => {
            return "LE privacy as main.conf says: it couldn't be read, so bluetoothd reads it as it is".into();
        }
    };
    format!("LE privacy {}: {why}", if on { "on" } else { "off" })
}

fn prepare() -> Result<(), String> {
    let source = std::fs::canonicalize(privacy::MAIN_CONF).map(|p| p.display().to_string()).unwrap_or_default();
    let choice = Choice::read_from(Path::new(privacy::CHOICE));
    let storage = Path::new(privacy::STORAGE);
    let before = Pairings::read_from(Path::new(privacy::PAIRINGS));
    let main_conf = match privacy::read_main_conf(Path::new(privacy::MAIN_CONF)) {
        Ok(text) => text,
        Err(e) => {
            // bluetoothd reads the system's (no copy to bind over it), and
            // what it does with privacy isn't known: pairings made while it
            // runs count as without the key.
            let _ = std::fs::remove_file(privacy::CONFIG);
            let mut pairings = privacy::decide(choice, "", storage, before.as_ref()).pairings;
            pairings.privacy = None;
            let _ = write(Path::new(privacy::PAIRINGS), &pairings.to_toml(), 0o600);
            let running = Running { privacy: false, why: Why::Unprepared, choice, main_conf: source };
            let _ = write(Path::new(privacy::RUNNING), &running.to_toml(), 0o644);
            return Err(format!("{e}: {}", words(false, Why::Unprepared)));
        }
    };
    let decision = privacy::decide(choice, &main_conf, storage, before.as_ref());
    let (on, why) = (decision.privacy, decision.why);
    let config = if why == Why::System { main_conf } else { privacy::render(&main_conf, on) };
    // Root's alone: the system's may hold a secret (a CSIS SIRK, say).
    write(Path::new(privacy::CONFIG), &config, 0o600)?;
    write(Path::new(privacy::PAIRINGS), &decision.pairings.to_toml(), 0o600)?;
    let running = Running { privacy: on, why, choice, main_conf: source };
    write(Path::new(privacy::RUNNING), &running.to_toml(), 0o644)?;
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
            write(path, &format!("{}\n", choice.word()), 0o644)
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::write;

    #[test]
    fn writes_at_once_each_land_with_their_mode() {
        let dir = std::env::temp_dir().join(format!("aizome-bluetooth-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("main.conf");
        std::thread::scope(|s| {
            for i in 0..8 {
                let path = &path;
                s.spawn(move || {
                    for _ in 0..20 {
                        write(path, &format!("{i}\n"), 0o600).unwrap();
                    }
                });
            }
        });
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(std::fs::read_to_string(&path).unwrap().trim().parse::<u32>().is_ok());
        // Nothing left beside it.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
