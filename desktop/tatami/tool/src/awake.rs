// SPDX-License-Identifier: MIT OR Apache-2.0
//! Keep Awake is a real logind block inhibitor, independent of the bar and
//! applications. Never infer protection from an idle daemon being absent.
use crate::util::{self, Result};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::time::Duration;

pub const UNIT: &str = "tatami-keep-awake.service";
const WHO: &str = "Tatami Keep Awake";
const REQUIRED: [&str; 3] = ["sleep", "idle", "handle-lid-switch"];

fn requested() -> bool {
    util::state_dir().join("keep-awake").is_file()
}

fn inhibitors() -> Result<serde_json::Value> {
    let text = util::output(
        "busctl",
        &[
            "--system",
            "--timeout=3",
            "--json=short",
            "call",
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "ListInhibitors",
        ],
    )?;
    serde_json::from_str(&text).map_err(|e| format!("Reading sleep inhibitors: {e}"))
}

/// logind's a(ssssuu) records: what, who, why, mode, uid, pid.
fn protected_by(reply: &serde_json::Value, uid: u32, pid: Option<u32>) -> Result<bool> {
    let rows = reply["data"][0]
        .as_array()
        .ok_or("Unexpected sleep inhibitor reply")?;
    Ok(rows.iter().any(|row| {
        row[1] == WHO
            && row[3] == "block"
            && row[4].as_u64() == Some(uid as u64)
            && pid.is_none_or(|pid| row[5].as_u64() == Some(pid as u64))
            && row[0].as_str().is_some_and(|what| {
                REQUIRED
                    .iter()
                    .all(|item| what.split(':').any(|s| s == *item))
            })
    }))
}

fn protected(pid: Option<u32>) -> Result<bool> {
    // SAFETY: geteuid has no preconditions.
    protected_by(&inhibitors()?, unsafe { libc::geteuid() }, pid)
}

fn edit_lock() -> Result<File> {
    fs::create_dir_all(util::state_dir()).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(util::state_dir().join("keep-awake.lock"))
        .map_err(|e| e.to_string())?;
    // SAFETY: the file descriptor stays open for the lifetime of the guard.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(file)
}

fn save(enabled: bool) -> Result<()> {
    let path = util::state_dir().join("keep-awake");
    if enabled {
        let temporary = path.with_extension("new");
        fs::write(&temporary, "enabled\n")
            .and_then(|_| fs::rename(temporary, path))
            .map_err(|e| e.to_string())
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn set(enabled: bool) -> Result<()> {
    save(enabled)?;
    if !util::run(
        "systemctl",
        &["--user", if enabled { "start" } else { "stop" }, UNIT],
    ) {
        return Err(format!(
            "Could not {} Keep Awake. Check systemctl --user status {UNIT}",
            if enabled { "start" } else { "stop" }
        ));
    }
    if protected(None)? != enabled {
        return Err("Could not verify Keep Awake with the system sleep manager".into());
    }
    // Keep hypridle running for explicit lock/sleep hooks. It respects our
    // idle inhibitor. This also repairs the predecessor toggle's stopped unit.
    if !util::run("systemctl", &["--user", "start", "tatami-hypridle.service"]) {
        return Err("Keep Awake changed, but the idle-locking service could not be started".into());
    }
    util::notify(
        "\u{f0176}",
        if enabled {
            "Keep Awake is on"
        } else {
            "Keep Awake is off"
        },
        if enabled {
            "System sleep, idle locking and lid-close sleep are blocked until you turn it off."
        } else {
            "Normal idle locking and sleep behavior restored."
        },
    );
    Ok(())
}

/// Started only inside systemd-inhibit, after it acquires the lock. The
/// parent's logind record is checked before readiness and continuously after
/// it; losing logind forces a fresh acquisition through systemd's restart.
fn hold() -> Result<()> {
    // SAFETY: getppid has no preconditions.
    let owner = unsafe { libc::getppid() } as u32;
    if !protected(Some(owner))? {
        return Err("The sleep inhibitor was not acquired".into());
    }
    util::signal_waybar(crate::toggle::IDLE_SIGNAL);
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if !protected(Some(owner))? {
            return Err("Keep Awake lost its sleep inhibitor; reacquiring it".into());
        }
    }
}

/// ExecStartPost gets the inhibitor's PID from systemd. A start job succeeds
/// only once that exact process holds all locks; TimeoutStartSec bounds this.
fn verify() -> Result<()> {
    let owner: u32 = std::env::var("MAINPID")
        .map_err(|_| "Missing Keep Awake service PID")?
        .parse()
        .map_err(|_| "Invalid Keep Awake service PID")?;
    if owner == 0 {
        return Err("Missing Keep Awake service PID".into());
    }
    loop {
        if protected(Some(owner))? {
            return Ok(());
        }
        if !std::path::Path::new(&format!("/proc/{owner}")).exists() {
            return Err(
                "The Keep Awake inhibitor process exited before acquiring its locks".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn command(arg: &str) -> Result<()> {
    match arg {
        "hold" => hold(),
        "verify" => verify(),
        "requested" => {
            if requested() {
                Ok(())
            } else {
                Err("Keep Awake is off".into())
            }
        }
        "status" => {
            println!(
                "{}",
                serde_json::json!({"enabled": requested(), "protected": protected(None)?})
            );
            Ok(())
        }
        "on" | "off" | "toggle" => {
            let _guard = edit_lock()?;
            let enabled = match arg {
                "on" => true,
                "off" => false,
                _ => !requested(),
            };
            let result = set(enabled);
            if let Err(error) = &result {
                util::notify("\u{f071}", "Keep Awake needs attention", error);
            }
            util::signal_waybar(crate::toggle::IDLE_SIGNAL);
            result
        }
        _ => Err("Usage: tatami awake on|off|toggle|status".into()),
    }
}

pub fn indicator() -> String {
    indicator_for(requested(), protected(None))
}

fn indicator_for(enabled: bool, protection: Result<bool>) -> String {
    let (class, tooltip) = match protection {
        Ok(true) => (
            "active",
            "Keep Awake is on: sleep, idle and lid-close sleep are blocked. Click to turn off.",
        ),
        Ok(false) if !enabled => (
            "inactive",
            "Keep Awake is off. Click to prevent sleep, idle locking and lid-close sleep.",
        ),
        _ => (
            "warning",
            "Keep Awake is not verified. Sleep may occur. Check the Keep Awake service or run tatami awake on.",
        ),
    };
    serde_json::json!({"text": "\u{f0176}", "class": class, "tooltip": tooltip}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reply(what: &str, mode: &str, uid: u32, pid: u32) -> serde_json::Value {
        serde_json::json!({"type":"a(ssssuu)","data":[[[what, WHO, "work", mode, uid, pid]]]})
    }
    #[test]
    fn protection_requires_all_three_block_locks_for_the_right_owner() {
        assert!(
            protected_by(
                &reply("idle:handle-lid-switch:sleep", "block", 1000, 42),
                1000,
                Some(42)
            )
            .unwrap()
        );
        for r in [
            reply("sleep:idle", "block", 1000, 42),
            reply("sleep:idle:handle-lid-switch", "delay", 1000, 42),
            reply("sleep:idle:handle-lid-switch", "block", 1001, 42),
            reply("sleep:idle:handle-lid-switch", "block", 1000, 43),
        ] {
            assert!(!protected_by(&r, 1000, Some(42)).unwrap());
        }
        assert!(protected_by(&serde_json::json!({}), 1000, None).is_err());
    }
    #[test]
    fn indicator_never_confuses_requested_with_protected() {
        for (enabled, protection, class) in [
            (false, Ok(false), "inactive"),
            (true, Ok(true), "active"),
            (true, Ok(false), "warning"),
            (true, Err("offline".into()), "warning"),
        ] {
            let json: serde_json::Value =
                serde_json::from_str(&indicator_for(enabled, protection)).unwrap();
            assert_eq!(json["class"], class);
            assert!(!json["tooltip"].as_str().unwrap().contains('&'));
        }
    }
}
