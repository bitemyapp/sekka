//! Panel Self Refresh, in the battery menu: a laptop's panel keeping its
//! last picture on screen by itself while nothing changes, so the graphics
//! and the link to the panel can rest (PSR, and Panel Replay on newer
//! panels). It saves battery, but on some panels it costs smoothness or
//! flashes a stray line now and then (the Samsung OLED of a ThinkPad X1
//! Carbon Gen 14 on Linux 7.3 does both), so Aizome keeps it off unless
//! it's turned on.
//!
//! Intel's drivers (xe, i915) say whether the panel can, and take the
//! setting, in debugfs, which only root reaches. [`HELPER`], run through
//! pkexec under its own polkit action (no password for whoever is at the
//! computer), keeps the choice in [`CHOICE`] and applies it; a boot service
//! applies it again at each start. What it found is in [`STATUS`], which
//! the menu reads. A computer whose panel can't (or isn't Intel's, or has
//! no panel) has no status, and the menu has no switch.

use crate::error::Error;
use crate::service::{Task, Wake};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The choice, `on` or `off` (off when missing).
pub const CHOICE: &str = "/var/lib/aizome/panel-refresh";
/// What the helper found, for the menu.
pub const STATUS: &str = "/run/aizome/panel-refresh.toml";
/// The helper, run as root.
pub const HELPER: &str = "aizome-panel-refresh";

/// The panel's self refresh, as the helper last left it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// The panel can refresh itself.
    pub available: bool,
    pub on: bool,
}

impl Status {
    pub fn read_from(path: &Path) -> Option<Status> {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok())
    }
}

/// The panel's self refresh, if this computer's panel can.
pub fn status() -> Option<Status> {
    Status::read_from(Path::new(STATUS)).filter(|s| s.available)
}

fn helper() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../libexec").join(HELPER), dir.join(HELPER)]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

/// Turn it on or off, through the helper, off this thread.
pub fn set(wake: &Wake, on: bool) -> Task<()> {
    let Some(helper) = helper() else {
        return Task::ready(Err(Error::Unavailable(format!("{HELPER} isn't installed"))));
    };
    Task::spawn("panel-refresh", wake, move || {
        // On NixOS only the wrapper is setuid.
        let wrapper = Path::new("/run/wrappers/bin/pkexec");
        let pkexec = if wrapper.exists() { wrapper } else { Path::new("pkexec") };
        let out = std::process::Command::new(pkexec)
            .arg("--disable-internal-agent")
            .arg(helper)
            .arg(if on { "on" } else { "off" })
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| Error::Unavailable(format!("pkexec: {e}")))?;
        match out.status.code() {
            Some(0) => Ok(()),
            Some(126) => Err(Error::Cancelled),
            Some(127) => Err(Error::NotAuthorized("Not allowed to change Panel Self Refresh".into())),
            _ => Err(Error::Failed(String::from_utf8_lossy(&out.stderr).trim().to_owned())),
        }
    })
}

/// The helper's side: finding the driver's files and setting them.
pub mod helper {
    use super::Status;
    use std::path::{Path, PathBuf};

    /// The driver's PSR debug file for the built-in panel under `debugfs`
    /// (`/sys/kernel/debug`), if that panel can refresh itself.
    pub fn find(debugfs: &Path) -> Option<PathBuf> {
        let mut gpus: Vec<PathBuf> = std::fs::read_dir(debugfs.join("dri")).ok()?.flatten().map(|e| e.path()).collect();
        gpus.sort();
        gpus.into_iter().map(|g| g.join("i915_edp_psr_debug")).find(|debug| {
            let status = debug.with_file_name("i915_edp_psr_status");
            debug.is_file()
                && std::fs::read_to_string(status).is_ok_and(|s| {
                    s.lines().any(|l| {
                        l.starts_with("Sink support:") && (l.contains("PSR = yes") || l.contains("Panel Replay = yes"))
                    })
                })
        })
    }

    /// Whether a display has Intel's driver (xe or i915), whose files
    /// [`find`] looks for.
    pub fn intel_display() -> bool {
        std::fs::read_dir("/sys/class/drm").is_ok_and(|cards| {
            cards.flatten().any(|c| {
                std::fs::read_link(c.path().join("device/driver"))
                    .is_ok_and(|d| d.file_name().is_some_and(|n| n == "xe" || n == "i915"))
            })
        })
    }

    /// The driver's setting for a choice: its own default, or off (no PSR
    /// and no Panel Replay).
    pub fn value(on: bool) -> &'static str {
        if on { "0" } else { "1" }
    }

    /// The choice kept in `path`: off unless it says `on`.
    pub fn choice(path: &Path) -> bool {
        std::fs::read_to_string(path).is_ok_and(|t| t.trim() == "on")
    }

    /// Apply `on` (or the kept choice, for `None`) and say what came of it
    /// in `status`; keep a new choice in `choice`.
    pub fn run(debugfs: &Path, choice: &Path, status: &Path, on: Option<bool>) -> std::io::Result<Status> {
        if let Some(on) = on {
            write(choice, if on { "on\n" } else { "off\n" })?;
        }
        let on = on.unwrap_or_else(|| self::choice(choice));
        let found = match find(debugfs) {
            Some(debug) => {
                std::fs::write(&debug, value(on))?;
                Status { available: true, on }
            }
            None => Status::default(),
        };
        write(status, &toml::to_string(&found).map_err(std::io::Error::other)?)?;
        Ok(found)
    }

    fn write(path: &Path, text: &str) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("new");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::helper::*;
    use super::*;

    fn debugfs(name: &str, sink: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("aizome-panel-refresh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let gpu = root.join("dri/0000:00:02.0");
        std::fs::create_dir_all(&gpu).unwrap();
        std::fs::write(gpu.join("i915_edp_psr_debug"), "0\n").unwrap();
        std::fs::write(gpu.join("i915_edp_psr_status"), format!("Sink support: {sink}\nPSR mode: disabled\n")).unwrap();
        // The render node's directory, which has neither.
        std::fs::create_dir_all(root.join("dri/128")).unwrap();
        root
    }

    #[test]
    fn a_panel_that_can_is_kept_off_until_turned_on() {
        let root = debugfs("replay", "PSR = yes [0x04] (Early Transport), Panel Replay = yes");
        let debug = root.join("dri/0000:00:02.0/i915_edp_psr_debug");
        assert_eq!(find(&root), Some(debug.clone()));
        let (choice, status) = (root.join("state/panel-refresh"), root.join("run/panel-refresh.toml"));
        // At boot with no choice yet: off.
        assert_eq!(run(&root, &choice, &status, None).unwrap(), Status { available: true, on: false });
        assert_eq!(std::fs::read_to_string(&debug).unwrap(), "1");
        assert_eq!(Status::read_from(&status), Some(Status { available: true, on: false }));
        // Turned on in the menu, then the next boot keeps it.
        run(&root, &choice, &status, Some(true)).unwrap();
        assert_eq!(std::fs::read_to_string(&debug).unwrap(), "0");
        std::fs::write(&debug, "0\n").unwrap();
        assert_eq!(run(&root, &choice, &status, None).unwrap(), Status { available: true, on: true });
        assert_eq!(std::fs::read_to_string(&choice).unwrap(), "on\n");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_switch_where_the_panel_cant() {
        let root = debugfs("none", "PSR = no, Panel Replay = no");
        assert_eq!(find(&root), None);
        let (choice, status) = (root.join("choice"), root.join("status.toml"));
        assert_eq!(run(&root, &choice, &status, Some(true)).unwrap(), Status::default());
        assert_eq!(std::fs::read_to_string(root.join("dri/0000:00:02.0/i915_edp_psr_debug")).unwrap(), "0\n");
        assert_eq!(Status::read_from(&status).filter(|s| s.available), None);
        assert_eq!(find(Path::new("/nonexistent")), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
