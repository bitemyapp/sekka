// SPDX-License-Identifier: MIT OR Apache-2.0
//! Display settings: what the Displays window (`tatami-displays`) and
//! Super+/ choose, kept where every session finds them, and applied to the
//! running one.
//!
//! - $XDG_STATE_HOME/tatami/displays.json, the settings of each display;
//! - $XDG_STATE_HOME/tatami/displays.lua, the `hl.*` calls written from
//!   them, which config/hypr/hyprland.lua loads at login and on every
//!   reload, before the session's monitor state (window.rs), so a display
//!   switched off for the session stays off.
//!
//! A display is recognized by its description (make, model and serial;
//! Hyprland's `desc:` selector), so its settings follow it to another port
//! or dock, or by its connector when another display has the same
//! description. Turning a display off is for the session only, as with
//! Super+Ctrl+Delete: a display off at every login would leave none on once
//! the others are unplugged.
use crate::{hypr, util, window};
use serde_json::{Map, Value, json};
use std::{fs, io::Read, path::PathBuf};

/// Color management presets Hyprland 0.56 accepts (helpers/CMType.cpp).
const COLOR_MODES: [&str; 9] = [
    "auto", "srgb", "wide", "edid", "hdr", "hdredid", "dcip3", "dp3", "adobe",
];

/// One display's settings: a monitor rule (config/shared/monitor/Parser.cpp
/// in Hyprland), plus whether it is on. Optional settings left unset keep
/// Hyprland's defaults (`misc:vrr` for variable refresh rate).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The connector, such as DP-1.
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// "WIDTHxHEIGHT@HZ" or "preferred".
    pub mode: String,
    /// "XxY" in logical pixels, or "auto".
    pub position: String,
    pub scale: f64,
    /// wl_output transform: 0-3 rotate by 90° steps, 4-7 also flip.
    pub transform: i64,
    /// The connector of the display this one shows.
    pub mirror: Option<String>,
    /// 0 off, 1 on, 2 for full-screen windows, 3 for full-screen games and
    /// video.
    pub vrr: Option<i64>,
    pub bitdepth: Option<i64>,
    pub cm: Option<String>,
    pub sdr_brightness: Option<f64>,
    pub sdr_saturation: Option<f64>,
}

fn valid_mode(mode: &str) -> bool {
    if mode == "preferred" {
        return true;
    }
    let Some((size, refresh)) = mode.split_once('@') else {
        return false;
    };
    let Some((width, height)) = size.split_once('x') else {
        return false;
    };
    width.parse::<u32>().is_ok_and(|w| w > 0)
        && height.parse::<u32>().is_ok_and(|h| h > 0)
        && refresh.parse::<f64>().is_ok_and(|r| r > 0.0 && r < 1000.0)
}

fn valid_position(position: &str) -> bool {
    position == "auto"
        || position
            .split_once('x')
            .is_some_and(|(x, y)| x.parse::<i32>().is_ok() && y.parse::<i32>().is_ok())
}

/// A number written for Lua: integers without a fraction, others as Rust
/// prints them (shortest round trip).
fn lua_number(value: f64) -> String {
    format!("{value}")
}

impl Settings {
    /// Settings as the Displays window sends them or the store keeps them,
    /// checked, since they become Lua the compositor runs.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        let name = text("name").filter(|name| hypr::safe_name(name));
        let name = name.ok_or("a display without a valid name")?;
        let mode = text("mode").unwrap_or_else(|| "preferred".to_owned());
        if !valid_mode(&mode) {
            return Err(format!("{name}: invalid mode {mode:?}"));
        }
        let position = text("position").unwrap_or_else(|| "auto".to_owned());
        if !valid_position(&position) {
            return Err(format!("{name}: invalid position {position:?}"));
        }
        let scale = value["scale"].as_f64().unwrap_or(1.0);
        if !(0.25..=10.0).contains(&scale) {
            return Err(format!("{name}: invalid scale {scale}"));
        }
        let transform = value["transform"].as_i64().unwrap_or(0);
        if !(0..=7).contains(&transform) {
            return Err(format!("{name}: invalid transform {transform}"));
        }
        let mirror = text("mirror").filter(|mirror| !mirror.is_empty());
        if let Some(mirror) = &mirror
            && (!hypr::safe_name(mirror) || *mirror == name)
        {
            return Err(format!("{name}: invalid mirror {mirror:?}"));
        }
        let vrr = value["vrr"].as_i64();
        if vrr.is_some_and(|vrr| !(0..=3).contains(&vrr)) {
            return Err(format!("{name}: invalid variable refresh rate"));
        }
        let bitdepth = value["bitdepth"].as_i64();
        if bitdepth.is_some_and(|depth| depth != 8 && depth != 10) {
            return Err(format!("{name}: invalid bit depth"));
        }
        let cm = text("cm");
        if let Some(cm) = &cm
            && !COLOR_MODES.contains(&cm.as_str())
        {
            return Err(format!("{name}: invalid color mode {cm:?}"));
        }
        let level = |key: &str| -> Result<Option<f64>, String> {
            match value[key].as_f64() {
                Some(level) if !(0.0..=10.0).contains(&level) => {
                    Err(format!("{name}: invalid {key}"))
                }
                level => Ok(level),
            }
        };
        Ok(Self {
            description: text("description").unwrap_or_default(),
            enabled: value["enabled"].as_bool().unwrap_or(true),
            sdr_brightness: level("sdrbrightness")?,
            sdr_saturation: level("sdrsaturation")?,
            name,
            mode,
            position,
            scale,
            transform,
            mirror,
            vrr,
            bitdepth,
            cm,
        })
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "name": self.name,
            "description": self.description,
            "mode": self.mode,
            "position": self.position,
            "scale": self.scale,
            "transform": self.transform,
        });
        let object = value.as_object_mut().expect("an object");
        if let Some(mirror) = &self.mirror {
            object.insert("mirror".into(), json!(mirror));
        }
        for (key, number) in [("vrr", self.vrr), ("bitdepth", self.bitdepth)] {
            if let Some(number) = number {
                object.insert(key.into(), json!(number));
            }
        }
        if let Some(cm) = &self.cm {
            object.insert("cm".into(), json!(cm));
        }
        for (key, level) in [
            ("sdrbrightness", self.sdr_brightness),
            ("sdrsaturation", self.sdr_saturation),
        ] {
            if let Some(level) = level {
                object.insert(key.into(), json!(level));
            }
        }
        value
    }

    /// The display as Hyprland has it now (`hyprctl -j monitors all`), for
    /// one that has no settings yet. Its position stays automatic.
    pub fn from_monitor(monitor: &Value) -> Option<Self> {
        let name = monitor["name"].as_str().filter(|n| hypr::safe_name(n))?;
        let width = monitor["width"].as_i64()?;
        let height = monitor["height"].as_i64()?;
        let refresh = monitor["refreshRate"].as_f64()?;
        Some(Self {
            name: name.to_owned(),
            description: monitor["description"].as_str().unwrap_or("").to_owned(),
            enabled: monitor["disabled"] != true,
            mode: format!("{width}x{height}@{refresh}"),
            position: "auto".to_owned(),
            scale: monitor["scale"].as_f64().unwrap_or(1.0),
            transform: monitor["transform"].as_i64().unwrap_or(0),
            mirror: None,
            vrr: None,
            bitdepth: None,
            cm: None,
            sdr_brightness: None,
            sdr_saturation: None,
        })
    }

    /// The monitor rule, for `output` (a selector: a connector or
    /// "desc:…").
    pub fn rule(&self, output: &str) -> String {
        let mut fields = vec![
            format!("output = {}", window::lua_string(output)),
            format!("mode = \"{}\"", self.mode),
            format!("position = \"{}\"", self.position),
            format!("scale = {}", lua_number(self.scale)),
            format!("transform = {}", self.transform),
        ];
        if let Some(mirror) = &self.mirror {
            fields.push(format!("mirror = \"{mirror}\""));
        }
        if let Some(vrr) = self.vrr {
            fields.push(format!("vrr = {vrr}"));
        }
        if let Some(bitdepth) = self.bitdepth {
            fields.push(format!("bitdepth = {bitdepth}"));
        }
        if let Some(cm) = &self.cm {
            fields.push(format!("cm = \"{cm}\""));
        }
        if let Some(level) = self.sdr_brightness {
            fields.push(format!("sdrbrightness = {}", lua_number(level)));
        }
        if let Some(level) = self.sdr_saturation {
            fields.push(format!("sdrsaturation = {}", lua_number(level)));
        }
        format!("hl.monitor({{ {} }})", fields.join(", "))
    }
}

/// The selector a display's settings are kept under: its description, or
/// its connector when another display among `all` has the same one.
pub fn selector(display: &Settings, all: &[Settings]) -> String {
    let shared = all
        .iter()
        .filter(|other| other.description == display.description)
        .count()
        > 1;
    if display.description.trim().is_empty() || shared {
        display.name.clone()
    } else {
        format!("desc:{}", display.description)
    }
}

fn state_file(name: &str) -> PathBuf {
    util::state_dir().join(name)
}

/// The kept settings: {"main": connector, "displays": {selector: settings}}.
pub fn load_store() -> Value {
    fs::read_to_string(state_file("displays.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "displays": {} }))
}

/// The Lua the configuration loads: a rule for every kept display, and the
/// main display, where the cursor starts and workspace 1 opens.
pub fn store_lua(store: &Value) -> String {
    let mut lua = String::from(
        "-- Display settings from the Displays window and Super+/, written by\n\
         -- `tatami displays` (tool/src/displays.rs). Changes made here are replaced.\n",
    );
    let mut displays: Vec<(&String, &Value)> = store["displays"]
        .as_object()
        .into_iter()
        .flatten()
        .collect();
    displays.sort_by_key(|(key, _)| key.as_str());
    for (key, value) in displays {
        // Kept settings are checked again: the file is the user's to edit.
        if let Ok(settings) = Settings::from_json(value) {
            lua.push_str(&settings.rule(key));
            lua.push('\n');
        }
    }
    if let Some(main) = main_lua(store["main"].as_str()) {
        lua.push_str(&main);
    }
    lua
}

fn main_lua(main: Option<&str>) -> Option<String> {
    let main = main.filter(|main| hypr::safe_name(main))?;
    Some(format!(
        "hl.config({{ cursor = {{ default_monitor = \"{main}\" }} }})\n\
         hl.workspace_rule({{ workspace = \"1\", monitor = \"{main}\", default = true }})\n"
    ))
}

/// Write a file whole or not at all: Hyprland may read it at any reload.
fn write_atomically(path: &PathBuf, contents: &str) -> bool {
    let partial = path.with_extension("partial");
    path.parent()
        .is_some_and(|dir| fs::create_dir_all(dir).is_ok())
        && fs::write(&partial, contents).is_ok()
        && fs::rename(&partial, path).is_ok()
}

fn save_store(store: &Value) -> bool {
    let json = serde_json::to_string_pretty(store).unwrap_or_default();
    write_atomically(&state_file("displays.json"), &(json + "\n"))
        && write_atomically(&state_file("displays.lua"), &store_lua(store))
}

/// A change from the Displays window: every display's settings, the main
/// display, and the displays to change now.
pub struct Change {
    pub displays: Vec<Settings>,
    pub main: Option<String>,
    pub apply: Vec<String>,
}

pub fn parse_change(value: &Value) -> Result<Change, String> {
    let displays = value["displays"]
        .as_array()
        .ok_or("no displays")?
        .iter()
        .map(Settings::from_json)
        .collect::<Result<Vec<_>, _>>()?;
    if !displays.iter().any(|display| display.enabled) {
        return Err("at least one display has to stay on".into());
    }
    for display in &displays {
        if let Some(mirror) = &display.mirror {
            let source = displays.iter().find(|other| other.name == *mirror);
            if !source.is_some_and(|source| source.enabled && source.mirror.is_none()) {
                return Err(format!(
                    "{}: {mirror} is not a display it can mirror",
                    display.name
                ));
            }
        }
    }
    let main = value["main"]
        .as_str()
        .filter(|main| displays.iter().any(|display| display.name == *main))
        .map(str::to_owned);
    let apply = value["apply"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|name| displays.iter().any(|display| display.name == *name))
        .map(str::to_owned)
        .collect();
    Ok(Change {
        displays,
        main,
        apply,
    })
}

/// The store with a change's settings kept: those of the displays that are
/// on (a display turned off keeps what it had), and the main display.
pub fn updated_store(store: &Value, change: &Change) -> Value {
    let mut store = store.clone();
    if !store["displays"].is_object() {
        store["displays"] = json!({});
    }
    let kept = store["displays"].as_object_mut().expect("an object");
    for display in change.displays.iter().filter(|display| display.enabled) {
        let key = selector(display, &change.displays);
        // Kept under its connector before, when its description was shared.
        if key != display.name {
            kept.remove(&display.name);
        }
        kept.insert(key, display.to_json());
    }
    if let Some(main) = &change.main {
        store["main"] = json!(main);
    }
    store
}

/// The Lua that applies a change now: the displays it names, by connector,
/// in one evaluation, so displays moved together never overlap on the way.
pub fn change_lua(change: &Change, main_changed: bool) -> String {
    let mut lua = String::new();
    for display in change
        .displays
        .iter()
        .filter(|display| change.apply.contains(&display.name))
    {
        if display.enabled {
            lua.push_str(&display.rule(&display.name));
        } else {
            lua.push_str(&format!(
                "hl.monitor({{ output = \"{}\", disabled = true }})",
                display.name
            ));
        }
        lua.push('\n');
    }
    if main_changed && let Some(main) = main_lua(change.main.as_deref()) {
        lua.push_str(&main);
    }
    lua
}

/// `tatami displays apply [--dry-run]`: a change from the Displays window,
/// as JSON on standard input. Kept, and applied to the session; with
/// --dry-run, printed instead.
pub fn apply_command(dry_run: bool) -> Result<(), String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&input).map_err(|error| error.to_string())?;
    let change = parse_change(&value)?;
    let store = load_store();
    let updated = updated_store(&store, &change);
    let main_changed = updated["main"] != store["main"];
    let lua = change_lua(&change, main_changed);
    if dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&updated).unwrap_or_default()
        );
        print!("{}", store_lua(&updated));
        print!("-- applied now:\n{lua}");
        return Ok(());
    }
    if !save_store(&updated) {
        return Err("cannot write the display settings".into());
    }
    let off: Vec<String> = change
        .displays
        .iter()
        .filter(|display| !display.enabled)
        .map(|display| display.name.clone())
        .collect();
    let on: Vec<String> = change
        .displays
        .iter()
        .filter(|display| display.enabled)
        .map(|display| display.name.clone())
        .collect();
    window::switch_off(&off, &on);
    if lua.is_empty() || hypr::eval(&lua) {
        Ok(())
    } else {
        Err("Hyprland did not take the display settings".into())
    }
}

/// The kept settings for a monitor Hyprland lists, with the selector it is
/// kept under.
fn kept_for(store: &Value, monitor: &Value, all: &Value) -> Option<(String, Settings)> {
    let live: Vec<Settings> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Settings::from_monitor)
        .collect();
    let display = Settings::from_monitor(monitor)?;
    let key = selector(&display, &live);
    let kept = Settings::from_json(&store["displays"][&key]).ok()?;
    Some((
        key,
        Settings {
            name: display.name,
            ..kept
        },
    ))
}

/// The rule to turn a monitor back on with: its kept settings, or the
/// configuration's for every monitor.
pub fn rule_for(monitor: &Value, all: &Value) -> Option<String> {
    let name = monitor["name"].as_str().filter(|n| hypr::safe_name(n))?;
    Some(match kept_for(&load_store(), monitor, all) {
        Some((_, kept)) => kept.rule(name),
        None => format!(
            "hl.monitor({{ output = \"{name}\", mode = \"preferred\", position = \"auto\", scale = \"auto\" }})"
        ),
    })
}

/// Super+/: keep a new scale for a monitor, with its other settings as kept
/// or as they are, and apply it.
pub fn set_scale(monitor: &Value, all: &Value, scale: f64) -> bool {
    let Some(display) = Settings::from_monitor(monitor) else {
        return false;
    };
    let store = load_store();
    let mut settings = kept_for(&store, monitor, all)
        .map(|(_, kept)| kept)
        .unwrap_or(display);
    settings.scale = scale;
    settings.enabled = true;
    let live: Vec<Settings> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Settings::from_monitor)
        .collect();
    let mut store = store;
    store["displays"][selector(&settings, &live)] = settings.to_json();
    save_store(&store) && hypr::eval(&settings.rule(&settings.name))
}

/// Settings kept before the Displays window, as a scale per connector under
/// ~/.local/state/tatami/toggles/monitor-scale: brought into the store once,
/// at login, for the monitors connected then.
pub fn migrate(legacy: &PathBuf) {
    if state_file("displays.json").exists() || !legacy.is_dir() {
        return;
    }
    let all = hypr::json_args(&["monitors", "all"]).unwrap_or_default();
    let live: Vec<Settings> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Settings::from_monitor)
        .collect();
    let mut store = json!({ "displays": Map::new() });
    let mut lua = String::new();
    for display in &live {
        let scale = fs::read_to_string(legacy.join(&display.name))
            .ok()
            .and_then(|text| text.trim().parse::<f64>().ok())
            .filter(|scale| (1.0..=4.0).contains(scale));
        if let Some(scale) = scale {
            let settings = Settings {
                scale,
                ..display.clone()
            };
            store["displays"][selector(display, &live)] = settings.to_json();
            if settings.enabled {
                lua.push_str(&settings.rule(&settings.name));
                lua.push('\n');
            }
        }
    }
    if save_store(&store) && !lua.is_empty() {
        hypr::eval(&lua);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asus() -> Value {
        json!({
            "name": "DP-1",
            "description": "ASUSTek COMPUTER INC PG27UCDM T5LMAV009458",
            "width": 3840, "height": 2160, "refreshRate": 59.997,
            "scale": 2.0, "transform": 0, "disabled": false
        })
    }
    fn laptop() -> Value {
        json!({
            "name": "eDP-1",
            "description": "Samsung Display Corp. ATNA40HQ10-0  0x0000003F",
            "width": 2880, "height": 1800, "refreshRate": 120.0,
            "scale": 2.0, "transform": 0, "disabled": true
        })
    }
    #[test]
    fn settings_are_checked_before_they_become_lua() {
        let good = json!({"name": "DP-1", "mode": "3840x2160@240.00", "position": "-1920x0",
                          "scale": 2, "transform": 1, "vrr": 2, "bitdepth": 10, "cm": "hdr",
                          "sdrbrightness": 1.2, "mirror": "eDP-1"});
        let settings = Settings::from_json(&good).unwrap();
        assert_eq!(
            settings.rule("desc:ASUS \"PG\""),
            "hl.monitor({ output = \"desc:ASUS \\034PG\\034\", mode = \"3840x2160@240.00\", \
             position = \"-1920x0\", scale = 2, transform = 1, mirror = \"eDP-1\", vrr = 2, \
             bitdepth = 10, cm = \"hdr\", sdrbrightness = 1.2 })"
        );
        assert_eq!(
            Settings::from_json(&settings.to_json()).unwrap().mode,
            settings.mode
        );
        for bad in [
            json!({"name": "DP-1\")"}),
            json!({"name": "DP-1", "mode": "4k\")"}),
            json!({"name": "DP-1", "position": "0x0\")"}),
            json!({"name": "DP-1", "scale": 40}),
            json!({"name": "DP-1", "transform": 9}),
            json!({"name": "DP-1", "vrr": 4}),
            json!({"name": "DP-1", "bitdepth": 12}),
            json!({"name": "DP-1", "cm": "neon"}),
            json!({"name": "DP-1", "mirror": "DP-1"}),
            json!({"name": "DP-1", "sdrsaturation": -1}),
        ] {
            assert!(Settings::from_json(&bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn displays_are_kept_by_description_unless_it_is_shared() {
        let one = Settings::from_monitor(&asus()).unwrap();
        let two = Settings::from_monitor(&laptop()).unwrap();
        assert_eq!(
            selector(&one, &[one.clone(), two.clone()]),
            "desc:ASUSTek COMPUTER INC PG27UCDM T5LMAV009458"
        );
        let twin = Settings {
            name: "DP-2".into(),
            ..one.clone()
        };
        assert_eq!(selector(&one, &[one.clone(), twin.clone()]), "DP-1");
        let anonymous = Settings {
            description: " ".into(),
            ..one.clone()
        };
        assert_eq!(selector(&anonymous, &[anonymous.clone()]), "DP-1");
    }
    fn change(displays: Value) -> Result<Change, String> {
        parse_change(
            &json!({"displays": displays, "main": "DP-1", "apply": ["DP-1", "eDP-1", "HDMI-A-9"]}),
        )
    }
    #[test]
    fn changes_keep_one_display_on_and_mirror_only_what_can_be_mirrored() {
        assert!(change(json!([{"name": "DP-1", "enabled": false}])).is_err());
        assert!(
            change(json!([{"name": "DP-1"}, {"name": "eDP-1", "mirror": "HDMI-A-1"}])).is_err()
        );
        assert!(
            change(
                json!([{"name": "DP-1", "enabled": false}, {"name": "eDP-1", "mirror": "DP-1"}])
            )
            .is_err()
        );
        let ok = change(json!([{"name": "DP-1"}, {"name": "eDP-1", "mirror": "DP-1"}])).unwrap();
        assert_eq!(ok.main.as_deref(), Some("DP-1"));
        // Only displays in the change are applied.
        assert_eq!(ok.apply, ["DP-1", "eDP-1"]);
    }
    #[test]
    fn a_change_keeps_the_settings_of_displays_that_are_on() {
        let change = change(json!([
            {"name": "DP-1", "description": "ASUS", "mode": "3840x2160@240", "position": "0x0", "scale": 2},
            {"name": "eDP-1", "description": "Samsung", "enabled": false, "scale": 1.5}
        ]))
        .unwrap();
        let before =
            json!({"displays": {"desc:Samsung": {"name": "eDP-1", "scale": 2}}, "main": "eDP-1"});
        let store = updated_store(&before, &change);
        assert_eq!(store["displays"]["desc:ASUS"]["mode"], "3840x2160@240");
        // Off for the session: what it had is kept for when it is on again.
        assert_eq!(store["displays"]["desc:Samsung"]["scale"], 2);
        assert_eq!(store["main"], "DP-1");
        let lua = store_lua(&store);
        assert!(lua.contains("hl.monitor({ output = \"desc:ASUS\", mode = \"3840x2160@240\", position = \"0x0\", scale = 2, transform = 0 })"));
        assert!(lua.contains("default_monitor = \"DP-1\""));
        assert!(lua.contains(
            "hl.workspace_rule({ workspace = \"1\", monitor = \"DP-1\", default = true })"
        ));
        let now = change_lua(&change, true);
        assert!(now.starts_with("hl.monitor({ output = \"DP-1\", mode = \"3840x2160@240\""));
        assert!(now.contains("hl.monitor({ output = \"eDP-1\", disabled = true })"));
        assert!(now.contains("default_monitor"));
        assert!(!change_lua(&change, false).contains("default_monitor"));
    }
    #[test]
    fn kept_settings_that_fail_the_checks_are_left_out() {
        let store = json!({"displays": {
            "DP-1": {"name": "DP-1", "mode": "1x1@60\") os.exit(1) --"},
            "DP-2": {"name": "DP-2", "scale": 1}
        }, "main": "DP-2\")"});
        let lua = store_lua(&store);
        assert!(!lua.contains("os.exit"));
        assert!(lua.contains("output = \"DP-2\""));
        assert!(!lua.contains("default_monitor"));
    }
    #[test]
    fn monitors_without_settings_start_from_how_they_are() {
        let display = Settings::from_monitor(&asus()).unwrap();
        assert_eq!(display.mode, "3840x2160@59.997");
        assert_eq!(display.position, "auto");
        assert!(display.enabled && display.vrr.is_none());
        assert!(!Settings::from_monitor(&laptop()).unwrap().enabled);
    }
}
