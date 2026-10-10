// SPDX-License-Identifier: MIT OR Apache-2.0
//! Queries and dispatches through hyprctl. Hyprland 0.56 evaluates
//! `hyprctl dispatch` arguments as Lua dispatcher expressions.
use crate::util::{Result, output, run};
use serde_json::Value;

pub fn json(what: &str) -> Result<Value> {
    json_args(&[what])
}

pub fn json_args(what: &[&str]) -> Result<Value> {
    let mut args = vec!["-j"];
    args.extend_from_slice(what);
    serde_json::from_str(&output("hyprctl", &args)?)
        .map_err(|error| format!("hyprctl {}: {error}", what.join(" ")))
}

pub fn dispatch(expression: &str) -> bool {
    run("hyprctl", &["dispatch", expression])
}

/// Run Lua in the compositor. With a Lua configuration Hyprland 0.56 has no
/// `hyprctl keyword`; `eval` changes settings until the next reload.
pub fn eval(code: &str) -> bool {
    output("hyprctl", &["eval", code]).is_ok_and(|reply| reply.trim() == "ok")
}

/// The configuration file in Hyprland's command line (`-c` or `--config`).
pub fn config_arg(args: &[String]) -> Option<&str> {
    args.windows(2)
        .find(|pair| pair[0] == "-c" || pair[0] == "--config")
        .map(|pair| pair[1].as_str())
}

/// Whether a reload would read a configuration pinned in the Nix store.
/// Hyprland resolves the path of its configuration once, when it starts,
/// and re-reads that file on every reload: started on the /etc symlink, it
/// keeps reading the configuration the session began with, whatever the
/// system has since installed. Anything unknown counts as pinned.
pub fn config_pinned() -> bool {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
    let pid = json("instances").ok().and_then(|instances| {
        instances.as_array()?.iter().find(|instance| {
            signature
                .as_deref()
                .is_none_or(|signature| instance["instance"] == signature)
        })?["pid"]
            .as_i64()
    });
    let Some(raw) = pid.and_then(|pid| std::fs::read(format!("/proc/{pid}/cmdline")).ok()) else {
        return true;
    };
    let args: Vec<String> = raw
        .split(|byte| *byte == 0)
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    config_arg(&args)
        .and_then(|config| std::fs::canonicalize(config).ok())
        .is_none_or(|config| config.starts_with("/nix/store"))
}

/// A window address as `hyprctl` prints it, safe to embed in Lua.
pub fn address(window: &Value) -> Option<&str> {
    window["address"]
        .as_str()
        .filter(|address| is_address(address))
}

fn is_address(address: &str) -> bool {
    address.len() > 2
        && address.starts_with("0x")
        && address[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Output names such as eDP-1 or DP-3, safe to embed in Lua.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub fn dpms(on: bool) {
    dispatch(if on {
        "hl.dsp.dpms({ action = \"on\" })"
    } else {
        "hl.dsp.dpms({ action = \"off\" })"
    });
}

/// Name of the monitor with keyboard focus, for on-screen displays.
pub fn focused_monitor(monitors: &Value) -> Option<String> {
    monitors
        .as_array()?
        .iter()
        .find(|monitor| monitor["focused"] == true)?["name"]
        .as_str()
        .map(str::to_owned)
}

pub fn active_window_pid(window: &Value) -> Option<i32> {
    window["pid"]
        .as_i64()
        .and_then(|pid| i32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
}

pub fn window_addresses(clients: &Value) -> Vec<String> {
    clients
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|client| client["address"].as_str())
        .filter(|address| is_address(address))
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}
impl Rect {
    /// slurp's geometry syntax.
    pub fn slurp(self) -> String {
        format!("{},{} {}x{}", self.x, self.y, self.w, self.h)
    }
    pub fn parse(text: &str) -> Option<Self> {
        let (position, size) = text.trim().split_once(' ')?;
        let (x, y) = position.split_once(',')?;
        let (w, h) = size.split_once('x')?;
        Some(Self {
            x: x.parse().ok()?,
            y: y.parse().ok()?,
            w: w.parse().ok()?,
            h: h.parse().ok()?,
        })
    }
    pub fn contains(self, x: i64, y: i64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Logical monitor geometry, accounting for scale and 90°/270° transforms.
pub fn monitor_rect(monitor: &Value) -> Option<Rect> {
    let scale = monitor["scale"].as_f64().filter(|s| *s > 0.0)?;
    let w = (monitor["width"].as_f64()? / scale).floor() as i64;
    let h = (monitor["height"].as_f64()? / scale).floor() as i64;
    let rotated = matches!(monitor["transform"].as_i64(), Some(1 | 3 | 5 | 7));
    Some(Rect {
        x: monitor["x"].as_i64()?,
        y: monitor["y"].as_i64()?,
        w: if rotated { h } else { w },
        h: if rotated { w } else { h },
    })
}

pub fn focused_monitor_rect(monitors: &Value) -> Option<Rect> {
    monitors
        .as_array()?
        .iter()
        .find(|monitor| monitor["focused"] == true)
        .and_then(monitor_rect)
}

/// Monitors and windows on the focused monitor's active workspace: the
/// candidate rectangles for a "smart" screenshot selection.
pub fn workspace_rects(monitors: &Value, clients: &Value) -> Vec<Rect> {
    let Some(workspace) = monitors.as_array().and_then(|monitors| {
        monitors
            .iter()
            .find(|monitor| monitor["focused"] == true)
            .and_then(|monitor| monitor["activeWorkspace"]["id"].as_i64())
    }) else {
        return vec![];
    };
    let screens = monitors
        .as_array()
        .into_iter()
        .flatten()
        .filter(|monitor| monitor["activeWorkspace"]["id"].as_i64() == Some(workspace))
        .filter_map(monitor_rect);
    let windows = clients
        .as_array()
        .into_iter()
        .flatten()
        .filter(|client| client["workspace"]["id"].as_i64() == Some(workspace))
        .filter_map(|client| {
            Some(Rect {
                x: client["at"][0].as_i64()?,
                y: client["at"][1].as_i64()?,
                w: client["size"][0].as_i64()?,
                h: client["size"][1].as_i64()?,
            })
        });
    screens.chain(windows).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn monitors() -> Value {
        json!([
            {"name": "eDP-1", "focused": true, "x": 0, "y": 0, "width": 2880, "height": 1800,
             "scale": 2.0, "transform": 0, "activeWorkspace": {"id": 3}},
            {"name": "DP-2", "focused": false, "x": 1440, "y": 0, "width": 1080, "height": 1920,
             "scale": 1.0, "transform": 1, "activeWorkspace": {"id": 3}},
            {"name": "DP-3", "focused": false, "x": 0, "y": 900, "width": 1920, "height": 1080,
             "scale": 1.0, "transform": 0, "activeWorkspace": {"id": 4}}
        ])
    }
    #[test]
    fn focused_monitor_and_scaled_geometry() {
        let monitors = monitors();
        assert_eq!(focused_monitor(&monitors).as_deref(), Some("eDP-1"));
        assert_eq!(
            focused_monitor_rect(&monitors),
            Some(Rect {
                x: 0,
                y: 0,
                w: 1440,
                h: 900
            })
        );
        assert_eq!(
            monitor_rect(&monitors[1]),
            Some(Rect {
                x: 1440,
                y: 0,
                w: 1920,
                h: 1080
            })
        );
        assert_eq!(focused_monitor(&json!([])), None);
    }
    #[test]
    fn workspace_rectangles_include_only_the_active_workspace() {
        let clients = json!([
            {"address": "0x1a", "at": [10, 20], "size": [300, 200], "workspace": {"id": 3}},
            {"address": "0x2b", "at": [0, 0], "size": [5, 5], "workspace": {"id": 4}},
            {"address": "not-hex", "at": [0, 0], "size": [5, 5], "workspace": {"id": 3}}
        ]);
        let rects = workspace_rects(&monitors(), &clients);
        assert_eq!(rects.len(), 4);
        assert!(rects.contains(&Rect {
            x: 10,
            y: 20,
            w: 300,
            h: 200
        }));
        assert_eq!(window_addresses(&clients), ["0x1a", "0x2b"]);
    }
    #[test]
    fn slurp_geometry_round_trips() {
        let rect = Rect {
            x: 4,
            y: 5,
            w: 600,
            h: 338,
        };
        assert_eq!(Rect::parse(&rect.slurp()), Some(rect));
        assert_eq!(Rect::parse("garbage"), None);
        assert!(rect.contains(4, 5) && !rect.contains(604, 5));
    }
    #[test]
    fn configuration_is_read_from_the_command_line() {
        let args = |line: &str| line.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            config_arg(&args(
                "Hyprland --watchdog-fd 4 --config /etc/xdg/tatami/hypr/hyprland.lua"
            )),
            Some("/etc/xdg/tatami/hypr/hyprland.lua")
        );
        assert_eq!(
            config_arg(&args("Hyprland -c /run/user/1000/tatami/hyprland.lua")),
            Some("/run/user/1000/tatami/hyprland.lua")
        );
        assert_eq!(config_arg(&args("Hyprland --config")), None);
        assert_eq!(config_arg(&args("Hyprland")), None);
    }
    #[test]
    fn active_window_pid_rejects_missing_values() {
        assert_eq!(active_window_pid(&json!({"pid": 1234})), Some(1234));
        assert_eq!(active_window_pid(&json!({"pid": -1})), None);
        assert_eq!(active_window_pid(&json!({})), None);
    }
}
