// SPDX-License-Identifier: MIT OR Apache-2.0
//! Window, workspace and monitor commands after Omarchy's omarchy-hyprland-*
//! scripts. Settings changed at runtime go through `hyprctl eval`; the ones
//! Omarchy keeps across sessions are remembered under
//! ~/.local/state/tatami/toggles and applied again by `tatami restore`
//! when the next session starts (and after the reloads below). The monitors'
//! state is also kept for the session where the configuration loads it, so
//! reloading the configuration leaves the monitors as they are.
use crate::{hypr, util};
use serde_json::Value;
use std::{fs, path::PathBuf};

const GAPS: &str = "window-no-gaps";
const ASPECT: &str = "single-window-aspect-ratio";

fn toggles() -> PathBuf {
    util::state_dir().join("toggles")
}

fn flag(name: &str) -> PathBuf {
    toggles().join(name)
}

/// Lua applied for a remembered toggle (default/hypr/toggles/*.lua).
pub fn toggle_lua(name: &str) -> Option<&'static str> {
    Some(match name {
        GAPS => {
            "hl.config({ general = { gaps_out = 0, gaps_in = 0, border_size = 0 }, decoration = { rounding = 0 } })"
        }
        ASPECT => "hl.config({ layout = { single_window_aspect_ratio = { 1, 1 } } })",
        _ => return None,
    })
}

/// Flip a remembered toggle: on applies it, off reloads the configuration
/// and applies whatever else is still remembered.
fn flip(name: &str) -> bool {
    let path = flag(name);
    let on = !path.exists();
    if on {
        let _ = fs::create_dir_all(toggles());
        let _ = fs::write(&path, "");
        if let Some(lua) = toggle_lua(name) {
            hypr::eval(lua);
        }
    } else {
        let _ = fs::remove_file(&path);
        if let Reload::Pinned = reload_hyprland() {
            util::notify("\u{f0450}", "Back to normal at the next login", "");
        }
    }
    on
}

pub fn gaps() {
    flip(GAPS);
}

pub fn aspect() {
    if flip(ASPECT) {
        util::notify("\u{f0b2}", "Enable single-window square aspect ratio", "");
    } else {
        util::notify("\u{f0b2}", "Disable single-window square aspect ratio", "");
    }
}

fn layouts() -> PathBuf {
    util::state_dir().join("workspace-layouts")
}

pub fn layout_lua(workspace: i64, layout: &str) -> String {
    format!("hl.workspace_rule({{ workspace = \"{workspace}\", layout = \"{layout}\" }})")
}

/// Super+L: switch the active workspace between dwindle and scrolling.
pub fn workspace_layout() {
    let Ok(workspace) = hypr::json("activeworkspace") else {
        return;
    };
    let Some(id) = workspace["id"].as_i64() else {
        return;
    };
    let layout = match workspace["tiledLayout"].as_str() {
        Some("dwindle") => "scrolling",
        _ => "dwindle",
    };
    let _ = fs::create_dir_all(layouts());
    let _ = fs::write(layouts().join(id.to_string()), layout);
    hypr::eval(&layout_lua(id, layout));
    util::notify(
        "\u{f10ac}",
        &format!("Workspace layout set to {layout}"),
        "",
    );
}

/// Apply remembered toggles, workspace layouts and a disabled touchpad.
pub fn restore() {
    for name in [GAPS, ASPECT] {
        if flag(name).exists()
            && let Some(lua) = toggle_lua(name)
        {
            hypr::eval(lua);
        }
    }
    if flag(TOUCHPAD).exists() {
        set_touchpads(false);
    }
    for entry in fs::read_dir(layouts()).into_iter().flatten().flatten() {
        let Some(id) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
            continue;
        };
        let layout = fs::read_to_string(entry.path()).unwrap_or_default();
        if matches!(
            layout.trim(),
            "dwindle" | "scrolling" | "master" | "monocle"
        ) {
            hypr::eval(&layout_lua(id, layout.trim()));
        }
    }
    // Display settings are in the configuration (displays.rs); scales kept
    // before it are brought in once.
    crate::displays::migrate(&scales());
    remember_monitors(&switched_off());
}

/// Close windows politely so applications can save state, then focus the
/// first workspace.
pub fn close_all() {
    if let Ok(clients) = hypr::json("clients") {
        for address in hypr::window_addresses(&clients) {
            hypr::dispatch(&format!(
                "hl.dsp.window.close({{ window = \"address:{address}\" }})"
            ));
        }
    }
    hypr::dispatch("hl.dsp.focus({ workspace = \"1\" })");
}

/// Super+Ctrl+F: full screen inside the window's tile.
pub fn tiled_fullscreen() {
    let client = hypr::json("activewindow")
        .ok()
        .and_then(|window| window["fullscreenClient"].as_i64())
        .unwrap_or(0);
    let target = if client == 2 { 0 } else { 2 };
    hypr::dispatch(&format!(
        "hl.dsp.window.fullscreen_state({{ internal = 0, client = {target} }})"
    ));
}

/// Super+Backspace: make the active window opaque, or translucent again.
pub fn transparency() {
    if let Ok(window) = hypr::json("activewindow")
        && let Some(address) = hypr::address(&window)
    {
        hypr::dispatch(&format!(
            "hl.dsp.window.set_prop({{ window = \"address:{address}\", prop = \"opaque\", value = \"toggle\" }})"
        ));
    }
}

/// Dispatches for Super+O: pop the window out (floating, centered, pinned
/// and on top at 1300x900), or put a popped window back.
pub fn pop_steps(address: &str, pinned: bool) -> Vec<String> {
    let window = format!("window = \"address:{address}\"");
    let float = format!("hl.dsp.window.float({{ {window}, action = \"toggle\" }})");
    let pin = format!("hl.dsp.window.pin({{ {window} }})");
    if pinned {
        vec![
            pin,
            float,
            format!("hl.dsp.window.tag({{ {window}, tag = \"-pop\" }})"),
        ]
    } else {
        vec![
            float,
            format!("hl.dsp.window.resize({{ {window}, x = 1300, y = 900 }})"),
            format!("hl.dsp.window.center({{ {window} }})"),
            pin,
            format!("hl.dsp.window.alter_zorder({{ {window}, mode = \"top\" }})"),
            format!("hl.dsp.window.tag({{ {window}, tag = \"+pop\" }})"),
        ]
    }
}

pub fn pop() {
    if let Ok(window) = hypr::json("activewindow")
        && let Some(address) = hypr::address(&window)
    {
        for step in pop_steps(address, window["pinned"] == true) {
            hypr::dispatch(&step);
        }
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The nearest scale at or above `scale` that divides the mode into whole
/// logical pixels in Hyprland's 1/120 steps.
pub fn clean_scale(scale: f64, width: i64, height: i64) -> f64 {
    let whole = gcd(width * 120, height * 120).max(1);
    let mut steps = ((scale * 120.0 + 0.5) as i64).clamp(1, whole);
    while whole % steps != 0 {
        steps += 1;
    }
    steps as f64 / 120.0
}

/// Omarchy's scale presets, stepping from the one nearest the current scale.
pub fn next_scale(current: f64, width: i64, height: i64, up: bool) -> f64 {
    const PRESETS: [f64; 6] = [1.0, 1.25, 1.6, 2.0, 3.0, 4.0];
    // (effective, preset, distance): presets that land on the same effective
    // scale collapse into the one closest to it.
    let mut steps: Vec<(f64, f64, f64)> = vec![];
    for preset in PRESETS {
        let effective = clean_scale(preset, width, height);
        let distance = (preset - effective).abs();
        match steps.iter_mut().find(|s| (s.0 - effective).abs() < 1e-8) {
            Some(step) if distance < step.2 => *step = (effective, preset, distance),
            Some(_) => {}
            None => steps.push((effective, preset, distance)),
        }
    }
    let mut best = 0;
    for (index, step) in steps.iter().enumerate() {
        if (current - step.0).abs() < (current - steps[best].0).abs() {
            best = index;
        }
    }
    let index = if up {
        (best + 1).min(steps.len() - 1)
    } else {
        best.saturating_sub(1)
    };
    steps[index].1
}

/// Scales kept per connector before the display settings (displays.rs).
fn scales() -> PathBuf {
    toggles().join("monitor-scale")
}

/// This session's files: the configuration Hyprland starts with and the
/// monitor state it loads. In the runtime directory, so they last as long
/// as the session.
pub fn session_dir() -> Option<PathBuf> {
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| PathBuf::from(dir).join("tatami"))
}

/// The monitor state, which the configuration loads after the display
/// settings (config/hypr/hyprland.lua), and which reloads write first.
/// Hyprland turns a monitor on or off on a reload only when its rule says
/// otherwise than the monitor is, and of the rules naming a monitor the
/// last one loaded wins, so with the state a reload leaves every monitor on
/// or off as it was. Turning one on or off under running applications is
/// what ended Claude Desktop (Electron).
const MONITORS_LUA: &str = "monitors.lua";
const MONITORS_OFF: &str = "monitors-off";

/// The monitors switched off in this session: by Tatami when it turns one
/// off, and as they are just before every reload. Never read back from
/// Hyprland just after a change, which it may not show yet.
fn switched_off() -> Vec<String> {
    session_dir()
        .and_then(|dir| fs::read_to_string(dir.join(MONITORS_OFF)).ok())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|name| hypr::safe_name(name))
        .map(str::to_owned)
        .collect()
}

/// Lua for the session's monitors: the switched-off ones off. The others get
/// no rule here, so their display settings, or the configuration's rule for
/// every monitor, apply.
pub fn monitor_state_lua(off: &[String]) -> String {
    off.iter()
        .filter(|name| hypr::safe_name(name))
        .map(|name| format!("hl.monitor({{ output = \"{name}\", disabled = true }})\n"))
        .collect()
}

/// Record the switched-off monitors and write the state the configuration
/// loads.
fn remember_monitors(off: &[String]) -> bool {
    let Some(dir) = session_dir() else {
        return false;
    };
    fs::create_dir_all(&dir).is_ok()
        && fs::write(dir.join(MONITORS_OFF), off.join("\n")).is_ok()
        && fs::write(dir.join(MONITORS_LUA), monitor_state_lua(off)).is_ok()
}

/// Displays turned off and on in the Displays window, for the session.
pub fn switch_off(off: &[String], on: &[String]) -> bool {
    let mut list = switched_off();
    list.retain(|name| !on.contains(name) && !off.contains(name));
    list.extend(off.iter().cloned());
    remember_monitors(&list)
}

pub enum Reload {
    Done,
    Failed,
    /// Hyprland would re-read a pinned configuration, which does not load the
    /// monitor state (hypr::config_pinned): not reloaded.
    Pinned,
}

/// Reload Hyprland's configuration without turning any monitor on or off,
/// then apply the remembered toggles it resets: the state written first has
/// the monitors that are off now off, and the others on.
pub fn reload_hyprland() -> Reload {
    // Scales kept before the display settings join them first, so the
    // configuration loads them rather than its automatic scale: now, or at
    // the next login when this session's configuration is pinned.
    crate::displays::migrate(&scales());
    if hypr::config_pinned() {
        return Reload::Pinned;
    }
    let Ok(all) = hypr::json_args(&["monitors", "all"]) else {
        return Reload::Failed;
    };
    if !remember_monitors(&disabled_monitors(&all)) || !util::run("hyprctl", &["reload"]) {
        return Reload::Failed;
    }
    restore();
    Reload::Done
}

/// A new session starts with every monitor on and nothing remembered.
pub fn forget_monitors() {
    if let Some(dir) = session_dir() {
        let _ = fs::remove_file(dir.join(MONITORS_LUA));
        let _ = fs::remove_file(dir.join(MONITORS_OFF));
    }
}

/// Super+/ and Super+Alt+/: step the focused monitor's scale, kept with its
/// display settings (displays.rs).
pub fn scale(up: bool) {
    let all = hypr::json_args(&["monitors", "all"]).unwrap_or_default();
    let Some(monitor) = all
        .as_array()
        .and_then(|list| list.iter().find(|m| m["focused"] == true))
    else {
        return;
    };
    let (Some(current), Some(width), Some(height)) = (
        monitor["scale"].as_f64(),
        monitor["width"].as_i64(),
        monitor["height"].as_i64(),
    ) else {
        return;
    };
    let target = clean_scale(next_scale(current, width, height, up), width, height);
    crate::displays::set_scale(monitor, &all, target);
}

/// The built-in panel among `hyprctl monitors all`.
pub fn internal_monitor(monitors: &Value) -> Option<&Value> {
    monitors.as_array()?.iter().find(|monitor| {
        monitor["name"]
            .as_str()
            .is_some_and(|name| ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p)))
    })
}

/// Super+Ctrl+Delete: turn the laptop display off while another display is
/// active, or back on.
pub fn laptop_display() {
    let all = crate::hypr::json_args(&["monitors", "all"]).unwrap_or_default();
    let Some(internal) = internal_monitor(&all) else {
        util::notify("\u{f0379}", "No laptop display found", "");
        return;
    };
    let Some(name) = internal["name"].as_str().filter(|n| hypr::safe_name(n)) else {
        return;
    };
    let mut off = switched_off();
    off.retain(|other| other != name);
    if internal["disabled"] == true {
        // Its display settings, or the configuration's rule for every
        // monitor, given by name: Hyprland compares rules by their settings,
        // so a later reload leaves it on.
        remember_monitors(&off);
        if let Some(rule) = crate::displays::rule_for(internal, &all) {
            hypr::eval(&rule);
        }
        restore();
        hypr::dpms(true);
        util::notify("\u{f0379}", "Laptop display enabled", "");
        return;
    }
    let others = all
        .as_array()
        .into_iter()
        .flatten()
        .any(|monitor| monitor["name"].as_str() != Some(name) && monitor["disabled"] != true);
    if !others {
        util::notify("\u{f0379}", "Can't disable the only active display", "");
        return;
    }
    off.push(name.to_owned());
    remember_monitors(&off);
    disable_monitor(name);
    util::notify("\u{f0379}", "Laptop display disabled", "");
}

/// Turn a monitor off until the next configuration reload.
fn disable_monitor(name: &str) -> bool {
    hypr::safe_name(name)
        && hypr::eval(&format!(
            "hl.monitor({{ output = \"{name}\", disabled = true }})"
        ))
}

/// Monitors that are off, from `hyprctl -j monitors all`, such as the
/// laptop display after Super+Ctrl+Delete.
fn disabled_monitors(all: &Value) -> Vec<String> {
    all.as_array()
        .into_iter()
        .flatten()
        .filter(|monitor| monitor["disabled"] == true)
        .filter_map(|monitor| monitor["name"].as_str())
        .filter(|name| hypr::safe_name(name))
        .map(str::to_owned)
        .collect()
}

/// A Lua string literal for any text: every byte outside [A-Za-z0-9 -_.:]
/// is written as a decimal escape, so device names are data, not code.
pub fn lua_string(text: &str) -> String {
    let mut out = String::from("\"");
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b" -_.:".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("\\{byte:03}"));
        }
    }
    out.push('"');
    out
}

const TOUCHPAD: &str = "touchpad-disabled";

fn touchpads() -> Vec<String> {
    hypr::json("devices")
        .ok()
        .and_then(|devices| devices["mice"].as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|mouse| mouse["name"].as_str().map(str::to_owned))
        .filter(|name| name.contains("touchpad"))
        .collect()
}

fn set_touchpads(enabled: bool) {
    for name in touchpads() {
        hypr::eval(&format!(
            "hl.device({{ name = {}, enabled = {enabled} }})",
            lua_string(&name)
        ));
    }
}

/// Touchpad keys: on, off or toggle, remembered for the next session.
pub fn touchpad(action: &str) {
    if touchpads().is_empty() {
        util::notify("\u{f07f8}", "No touchpad found", "");
        return;
    }
    let disable = match action {
        "on" => false,
        "off" => true,
        _ => !flag(TOUCHPAD).exists(),
    };
    set_touchpads(!disable);
    if disable {
        let _ = fs::create_dir_all(toggles());
        let _ = fs::write(flag(TOUCHPAD), "");
        util::notify("\u{f07f8}", "Touchpad disabled", "");
    } else {
        let _ = fs::remove_file(flag(TOUCHPAD));
        util::notify("\u{f07f8}", "Touchpad enabled", "");
    }
}

/// Super+Ctrl+Z zooms in one step; Super+Ctrl+Alt+Z resets.
pub fn zoom(action: &str) {
    let current = hypr::json_args(&["getoption", "cursor:zoom_factor"])
        .ok()
        .and_then(|option| option["float"].as_f64())
        .unwrap_or(1.0);
    let factor = if action == "reset" {
        1.0
    } else {
        current + 1.0
    };
    hypr::eval(&format!(
        "hl.config({{ cursor = {{ zoom_factor = {factor} }} }})"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn scales_divide_the_mode_evenly() {
        assert_eq!(clean_scale(1.0, 1920, 1080), 1.0);
        assert_eq!(clean_scale(2.0, 2880, 1800), 2.0);
        assert_eq!(clean_scale(1.6, 2880, 1800), 1.6);
        // 1.25 does not divide 2880x1800 into whole pixels; the next step does.
        let s = clean_scale(1.25, 2880, 1800);
        assert!(s >= 1.25 && (2880.0 / s).fract() == 0.0 && (1800.0 / s).fract() == 0.0);
    }
    #[test]
    fn scale_steps_follow_the_presets() {
        assert_eq!(next_scale(1.0, 1920, 1080, true), 1.25);
        assert_eq!(next_scale(2.0, 2880, 1800, true), 3.0);
        assert_eq!(next_scale(2.0, 2880, 1800, false), 1.6);
        assert_eq!(next_scale(1.0, 1920, 1080, false), 1.0);
        assert_eq!(next_scale(4.0, 3840, 2160, true), 4.0);
    }
    #[test]
    fn pop_out_and_back() {
        let out = pop_steps("0xabc", false);
        assert_eq!(out.len(), 6);
        assert!(out[0].contains("float") && out[3].contains("pin"));
        let back = pop_steps("0xabc", true);
        assert!(back[0].contains("pin") && back[2].contains("-pop"));
    }
    #[test]
    fn internal_panels_are_recognized() {
        let monitors = json!([{"name": "DP-3"}, {"name": "eDP-1"}]);
        assert_eq!(internal_monitor(&monitors).unwrap()["name"], "eDP-1");
        assert!(internal_monitor(&json!([{"name": "HDMI-A-1"}])).is_none());
        assert_eq!(
            layout_lua(3, "scrolling"),
            "hl.workspace_rule({ workspace = \"3\", layout = \"scrolling\" })"
        );
        assert!(toggle_lua(GAPS).unwrap().contains("gaps_out = 0"));
        assert!(toggle_lua("nope").is_none());
    }
    #[test]
    fn disabled_monitors_are_listed_by_safe_name() {
        let all = json!([
            {"name": "eDP-1", "disabled": true},
            {"name": "DP-1", "disabled": false},
            {"name": "x\"y", "disabled": true}
        ]);
        assert_eq!(disabled_monitors(&all), ["eDP-1"]);
        assert!(disabled_monitors(&json!([])).is_empty());
    }
    #[test]
    fn session_monitor_state_keeps_switched_off_displays_off() {
        let off = ["eDP-1".to_owned(), "x\"y".to_owned()];
        // Only the switched-off displays: the others keep their display
        // settings, which load before this state. Names stay data.
        assert_eq!(
            monitor_state_lua(&off),
            "hl.monitor({ output = \"eDP-1\", disabled = true })\n"
        );
        // Switched on again: no rule.
        assert!(monitor_state_lua(&[]).is_empty());
    }
    #[test]
    fn device_names_stay_data() {
        assert_eq!(
            lua_string("syna8016:00-06cb:ceb3-touchpad"),
            "\"syna8016:00-06cb:ceb3-touchpad\""
        );
        // Three-digit escapes, so a following digit is never read into one.
        assert_eq!(
            lua_string("a\") os.exit(1) --"),
            "\"a\\034\\041 os.exit\\0401\\041 --\""
        );
    }
}
