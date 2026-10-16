// SPDX-License-Identifier: MIT OR Apache-2.0
//! The scale X11 applications draw at. Tatami turns Hyprland's XWayland
//! scaling off (`force_zero_scaling` in hyprland.lua), so X11 windows are
//! drawn at the display's own resolution: sharp, but at 1x unless the
//! application scales itself. Many take their scale from the `Xft.dpi`
//! resource, 96 for each unit of scale (SDL, and so Steam; Chromium and
//! Electron; Qt; GTK's text), and without it they are drawn small on a
//! HiDPI display. X11 has one scale for every screen: the main display's.
use crate::{display_recovery, util};
use serde_json::Value;
use std::{
    io::Write,
    process::{Command, Stdio},
};

/// The main display, as the Displays window takes it: the kept one, else
/// the one showing workspace 1, else the first one on. A mirror is not.
pub fn main_display<'a>(all: &'a Value, kept: Option<&str>) -> Option<&'a Value> {
    let usable: Vec<&Value> = all
        .as_array()?
        .iter()
        .filter(|m| display_recovery::lit(m))
        .filter(|m| m["mirrorOf"].as_str().is_none_or(|mirror| mirror == "none"))
        .collect();
    kept.and_then(|name| usable.iter().find(|m| m["name"] == name))
        .or_else(|| usable.iter().find(|m| m["activeWorkspace"]["id"] == 1))
        .or_else(|| usable.first())
        .copied()
}

/// `Xft.dpi` for a scale: 96 for each unit, rounded as X11 applications
/// read it.
pub fn dpi(scale: f64) -> Option<u32> {
    (scale.is_finite() && scale > 0.0).then(|| (scale * 96.0).round() as u32)
}

/// The `Xft.dpi` the main display's scale asks for, while a display is on.
pub fn wanted(all: &Value, kept: Option<&str>) -> Option<u32> {
    dpi(main_display(all, kept)?["scale"].as_f64()?)
}

/// XWayland's display: this session's, or the service manager's when
/// Hyprland exported it after this helper started.
pub fn display() -> Option<String> {
    let from_manager = || {
        util::output("systemctl", &["--user", "show-environment"])
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("DISPLAY=").map(str::to_owned))
    };
    std::env::var("DISPLAY")
        .ok()
        .or_else(from_manager)
        .filter(|display| !display.is_empty())
}

/// Set `Xft.dpi` on XWayland's display, keeping its other resources.
/// Applications read it as they start.
pub fn set_dpi(display: &str, dpi: u32) -> util::Result<()> {
    // -nocpp: the line is taken as it is, without the C preprocessor.
    let mut child = Command::new("xrdb")
        .args(["-nocpp", "-merge"])
        .env("DISPLAY", display)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("xrdb: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("Xft.dpi:\t{dpi}\n").as_bytes())
            .map_err(|error| format!("xrdb: {error}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|error| format!("xrdb: {error}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "xrdb exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn monitor(name: &str, scale: f64, workspace: i64, mirror: &str, disabled: bool) -> Value {
        json!({"name": name, "disabled": disabled, "width": 3840, "height": 2160,
               "scale": scale, "mirrorOf": mirror, "activeWorkspace": {"id": workspace}})
    }

    #[test]
    fn dpi_is_96_for_each_unit_of_scale() {
        assert_eq!(dpi(1.0), Some(96));
        assert_eq!(dpi(1.25), Some(120));
        assert_eq!(dpi(1.5), Some(144));
        assert_eq!(dpi(2.0), Some(192));
        // Hyprland reports fractional scales as single-precision floats.
        assert_eq!(dpi(f64::from(1.6_f32)), Some(154));
        assert_eq!(dpi(0.0), None);
        assert_eq!(dpi(f64::NAN), None);
    }

    #[test]
    fn the_main_display_sets_the_scale() {
        let all = json!([
            monitor("eDP-1", 2.0, 1, "none", false),
            monitor("DP-1", 1.0, 2, "none", false),
            monitor("DP-2", 1.5, 3, "0", false),
            monitor("HDMI-A-1", 3.0, -1, "none", true),
        ]);
        // As kept in the Displays window.
        assert_eq!(wanted(&all, Some("DP-1")), Some(96));
        assert_eq!(wanted(&all, Some("eDP-1")), Some(192));
        // A kept main display that is off, mirroring or gone: the one
        // showing workspace 1.
        assert_eq!(wanted(&all, Some("HDMI-A-1")), Some(192));
        assert_eq!(wanted(&all, Some("DP-2")), Some(192));
        assert_eq!(wanted(&all, Some("DP-9")), Some(192));
        assert_eq!(wanted(&all, None), Some(192));
    }

    #[test]
    fn without_workspace_1_the_first_display_on_is_main() {
        let all = json!([
            monitor("DP-2", 1.5, 1, "1", false),
            monitor("DP-1", 1.25, 2, "none", false),
            monitor("eDP-1", 2.0, 3, "none", false),
        ]);
        assert_eq!(
            main_display(&all, None).map(|m| &m["name"]),
            Some(&json!("DP-1"))
        );
        assert_eq!(wanted(&all, None), Some(120));
        assert_eq!(
            wanted(&json!([monitor("eDP-1", 2.0, 1, "none", true)]), None),
            None
        );
        assert_eq!(wanted(&json!([]), None), None);
    }
}
