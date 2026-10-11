// SPDX-License-Identifier: MIT OR Apache-2.0
//! The laptop lid, for closed-display mode (display_recovery.rs). logind
//! reads the same switch device as Hyprland's lid bindings, so both agree;
//! ACPI's own report is the fallback when logind cannot be asked.
use crate::util;
use std::{fs, path::Path};

/// Whether the lid is closed: None when the machine reports no lid state.
pub fn closed() -> Option<bool> {
    logind().or_else(|| acpi(Path::new("/proc/acpi/button/lid")))
}

/// `tatami displays lid`: open, closed or unknown.
pub fn describe(closed: Option<bool>) -> &'static str {
    match closed {
        Some(true) => "closed",
        Some(false) => "open",
        None => "unknown",
    }
}

fn logind() -> Option<bool> {
    let reply = util::output(
        "busctl",
        &[
            "--system",
            "--timeout=1",
            "get-property",
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "LidClosed",
        ],
    )
    .ok()?;
    parse_property(&reply)
}

/// busctl's "b true" or "b false".
pub fn parse_property(reply: &str) -> Option<bool> {
    match reply.trim() {
        "b true" => Some(true),
        "b false" => Some(false),
        _ => None,
    }
}

/// /proc/acpi/button/lid/*/state: "state:      open". Closed if any lid
/// is, as logind treats several lid switches.
pub fn acpi(dir: &Path) -> Option<bool> {
    let states: Vec<bool> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path().join("state")).ok())
        .filter_map(|text| match text.split_whitespace().last()? {
            "closed" => Some(true),
            "open" => Some(false),
            _ => None,
        })
        .collect();
    (!states.is_empty()).then(|| states.contains(&true))
}

/// Connected displays from the kernel (/sys/class/drm/card0-eDP-1/status),
/// by connector name: what Hyprland will find when it starts.
pub fn connected_outputs(drm: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            fs::read_to_string(entry.path().join("status"))
                .is_ok_and(|status| status.trim() == "connected")
        })
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let (card, connector) = name.split_once('-')?;
            card.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .then(|| connector.to_owned())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logind_and_acpi_reports_are_read() {
        assert_eq!(parse_property("b true\n"), Some(true));
        assert_eq!(parse_property("b false"), Some(false));
        assert_eq!(parse_property("Failed to get property"), None);
        let dir = std::env::temp_dir().join(format!("tatami-lid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(acpi(&dir), None);
        fs::create_dir_all(dir.join("LID0")).unwrap();
        fs::write(dir.join("LID0/state"), "state:      open\n").unwrap();
        assert_eq!(acpi(&dir), Some(false));
        fs::create_dir_all(dir.join("LID1")).unwrap();
        fs::write(dir.join("LID1/state"), "state:      closed\n").unwrap();
        assert_eq!(acpi(&dir), Some(true));
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn connected_connectors_are_named_as_hyprland_names_them() {
        let dir = std::env::temp_dir().join(format!("tatami-drm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (name, status) in [
            ("card1-eDP-1", "connected"),
            ("card1-DP-1", "connected\n"),
            ("card1-HDMI-A-1", "disconnected"),
            ("card1", ""),
            ("renderD128", ""),
            ("version", ""),
        ] {
            fs::create_dir_all(dir.join(name)).unwrap();
            if !status.is_empty() {
                fs::write(dir.join(name).join("status"), status).unwrap();
            }
        }
        assert_eq!(connected_outputs(&dir), ["DP-1", "eDP-1"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
