// SPDX-License-Identifier: MIT OR Apache-2.0
//! Locking, waking and leaving the session.
use crate::{CONFIG, hypr, util};
use std::{thread, time::Duration};

fn locked() -> bool {
    !util::pids_named(&["hyprlock", ".hyprlock-wrapped"]).is_empty()
}

/// Lock the session, then turn the displays off after a few seconds unless
/// `lock_only` (before suspend the displays turn off anyway).
pub fn lock(lock_only: bool) {
    if !locked()
        && let Some(me) = util::me()
    {
        // Wait for hyprlock in a separate process so this command returns
        // promptly to hypridle and key bindings.
        util::spawn(&me, &["lock-wait"]);
    }
    // Unlock with the primary layout, whatever was active when locking.
    util::run("hyprctl", &["switchxkblayout", "all", "0"]);
    if !lock_only {
        thread::sleep(Duration::from_secs(3));
        if locked() {
            hypr::dpms(false);
        }
    }
}

pub fn lock_wait() {
    util::run("hyprlock", &["-c", &format!("{CONFIG}/hypr/hyprlock.conf")]);
    wake(0);
}

pub fn wake(delay: u64) {
    thread::sleep(Duration::from_secs(delay));
    hypr::dpms(true);
}

pub enum Leave {
    Logout,
    Reboot,
    Shutdown,
}

pub fn leave(how: Leave) {
    crate::window::close_all();
    // Give browsers a moment to shut down cleanly.
    thread::sleep(Duration::from_secs(1));
    match how {
        Leave::Logout => util::run("uwsm", &["stop"]),
        Leave::Reboot => util::run("systemctl", &["reboot", "--no-wall"]),
        Leave::Shutdown => util::run("systemctl", &["poweroff", "--no-wall"]),
    };
}

/// Hibernation needs active swap and kernel support for suspend-to-disk.
pub fn can_hibernate(proc_swaps: &str, power_state: &str) -> bool {
    proc_swaps.lines().count() > 1 && power_state.split_whitespace().any(|s| s == "disk")
}

pub fn hibernation_available() -> bool {
    can_hibernate(
        &std::fs::read_to_string("/proc/swaps").unwrap_or_default(),
        &std::fs::read_to_string("/sys/power/state").unwrap_or_default(),
    )
}

pub fn hibernate() {
    if hibernation_available() {
        util::run("systemctl", &["hibernate"]);
    } else {
        util::notify("\u{f0901}", "Hibernation needs a swap partition", "");
    }
}

/// `Powered: yes` from `bluetoothctl show`.
pub fn bluetooth_powered(show: &str) -> Option<bool> {
    show.lines()
        .find_map(|line| line.trim().strip_prefix("Powered:"))
        .map(|value| value.trim() == "yes")
}

/// Right click on the bar's Bluetooth icon: switch the radio on or off.
/// Super+Ctrl+B, the bar's Bluetooth icon and Setup › Bluetooth: bluetui,
/// which pairs devices that need a passkey, after unblocking the radio.
/// Without an adapter it says so instead of opening an empty panel.
pub fn bluetooth() {
    let adapter = std::fs::read_dir("/sys/class/bluetooth")
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("hci"));
    if !adapter {
        util::notify("\u{f00b2}", "No Bluetooth adapter", "");
        return;
    }
    crate::menu::after_menu();
    util::run("rfkill", &["unblock", "bluetooth"]);
    crate::terminal::tui(&["bluetui".to_owned()], crate::terminal::Tui::Panel);
}

pub fn bluetooth_toggle() {
    if let Some(powered) = util::output("bluetoothctl", &["show"])
        .ok()
        .and_then(|show| bluetooth_powered(&show))
    {
        util::run(
            "bluetoothctl",
            &["power", if powered { "off" } else { "on" }],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hibernation_requires_swap_and_disk_state() {
        let swaps = "Filename Type Size Used Priority\n/dev/nvme0n1p3 partition 33554428 0 -2\n";
        assert!(can_hibernate(swaps, "freeze mem disk\n"));
        assert!(!can_hibernate(
            "Filename Type Size Used Priority\n",
            "freeze mem disk"
        ));
        assert!(!can_hibernate(swaps, "freeze mem"));
    }
    #[test]
    fn bluetooth_power_state() {
        let show = "Controller 00:11:22:33:44:55 (public)\n\tName: x\n\tPowered: yes\n\tDiscoverable: no\n";
        assert_eq!(bluetooth_powered(show), Some(true));
        assert_eq!(bluetooth_powered("\tPowered: no\n"), Some(false));
        assert_eq!(bluetooth_powered("No default controller available\n"), None);
    }
}
