// SPDX-License-Identifier: MIT OR Apache-2.0
//! Which displays should be on, after macOS: closing the lid with another
//! display connected turns the built-in display off (closed-display mode)
//! and opening it turns the display back on; a display the user turns off
//! stays off whenever the same displays are connected again, for the rest
//! of the session; and a connected screen is never left without one on.
//! Decided here from Hyprland's monitors, the lid and this session's
//! record; display_recovery.rs carries the decision out.
use crate::{displays, hypr, window};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

pub fn builtin(name: &str) -> bool {
    ["eDP-", "LVDS-", "DSI-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// A connector of a real screen. FALLBACK, headless and nested outputs
/// cannot make a black physical screen usable.
pub fn physical_name(name: &str) -> bool {
    hypr::safe_name(name)
        && ["eDP-", "LVDS-", "DSI-", "DP-", "HDMI-", "DVI-", "VGA-"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn physical(monitor: &Value) -> bool {
    monitor["name"].as_str().is_some_and(physical_name)
}

/// Monitors without the field count as on: never act on partial data.
fn on(monitor: &Value) -> bool {
    monitor["disabled"] != true
}

/// The connected screens in `hyprctl -j monitors all`.
pub fn screens(all: &Value) -> Vec<&Value> {
    all.as_array()
        .into_iter()
        .flatten()
        .filter(|monitor| physical(monitor))
        .collect()
}

fn name(monitor: &Value) -> &str {
    monitor["name"].as_str().unwrap_or("")
}

/// The display's identity, as its settings are kept (displays::selector):
/// its description, so it is recognized on another port or dock.
pub fn selector(monitor: &Value, all: &Value) -> String {
    let live: Vec<displays::Settings> = screens(all).into_iter().filter_map(identity).collect();
    identity(monitor)
        .map(|display| displays::selector(&display, &live))
        .unwrap_or_else(|| name(monitor).to_owned())
}

/// The name and description only: a monitor off since login reports no mode.
fn identity(monitor: &Value) -> Option<displays::Settings> {
    let mut monitor = monitor.clone();
    for (key, value) in [("width", 1), ("height", 1)] {
        if !monitor[key].is_i64() {
            monitor[key] = value.into();
        }
    }
    if !monitor["refreshRate"].is_number() {
        monitor["refreshRate"] = 60.0.into();
    }
    displays::Settings::from_monitor(&monitor)
}

/// The set of connected displays, as a key: sorted identities.
pub fn arrangement(all: &Value) -> Vec<String> {
    let mut set: Vec<String> = screens(all)
        .into_iter()
        .map(|monitor| selector(monitor, all))
        .collect();
    set.sort();
    set
}

fn key(set: &[String]) -> String {
    serde_json::to_string(set).unwrap_or_default()
}

/// This session's record, in the runtime directory: it ends with the
/// session, so a new login starts with every connected display on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// The connected displays when last reconciled (`arrangement`).
    #[serde(default)]
    pub displays: Option<Vec<String>>,
    /// Built-in displays turned off because the lid closed, by connector.
    #[serde(default)]
    pub lid_off: Vec<String>,
    /// The lid when last reconciled.
    #[serde(default)]
    pub lid_closed: Option<bool>,
    /// Displays the user turned off, by the displays connected then.
    #[serde(default)]
    pub off: BTreeMap<String, Vec<String>>,
}

fn session_file() -> Option<PathBuf> {
    window::session_dir().map(|dir| dir.join("displays-session.json"))
}

/// The record, or one started from the displays the session turned off
/// before it was kept (an update applied in a running session).
pub fn load(all: &Value) -> Session {
    if let Some(session) = session_file()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        return session;
    }
    let off = window::switched_off();
    let mut session = Session::default();
    // Only a choice while another screen is on: a display left off with
    // nothing else on was lost, not chosen.
    if !screens(all).into_iter().any(on) {
        return session;
    }
    let chosen: Vec<String> = screens(all)
        .into_iter()
        .filter(|monitor| {
            !on(monitor)
                && off
                    .iter()
                    .any(|name| name == monitor["name"].as_str().unwrap_or(""))
        })
        .map(|monitor| selector(monitor, all))
        .collect();
    if !chosen.is_empty() {
        session.off.insert(key(&arrangement(all)), chosen);
    }
    session
}

pub fn save(session: &Session) -> bool {
    let Some(path) = session_file() else {
        return false;
    };
    let partial = path.with_extension("partial");
    let text = serde_json::to_string_pretty(session).unwrap_or_default() + "\n";
    path.parent()
        .is_some_and(|dir| fs::create_dir_all(dir).is_ok())
        && fs::write(&partial, text).is_ok()
        && fs::rename(&partial, &path).is_ok()
}

pub fn forget() {
    if let Some(path) = session_file() {
        let _ = fs::remove_file(path);
    }
}

/// The user turned displays on or off (the Displays window, Super+Ctrl+
/// Delete): kept for the displays connected now.
pub fn choose(session: &mut Session, all: &Value, on: &[String], off: &[String]) {
    let set = arrangement(all);
    let entry = session.off.entry(key(&set)).or_default();
    for monitor in screens(all) {
        let id = selector(monitor, all);
        let connector = name(monitor);
        if off.iter().any(|name| name == connector) && !entry.contains(&id) {
            entry.push(id);
        } else if on.iter().any(|name| name == connector) {
            entry.retain(|chosen| *chosen != id);
            session.lid_off.retain(|name| name != connector);
        }
    }
    entry.sort();
    session.off.retain(|_, chosen| !chosen.is_empty());
    session.displays = Some(set);
}

/// What to change now.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// Connectors to turn on, in order of preference.
    pub enable: Vec<String>,
    /// Connectors to turn off once another screen is confirmed on.
    pub disable: Vec<String>,
    /// Connectors that must show their own picture: what they mirror goes.
    pub unmirror: Vec<String>,
    /// Built-in displays to blank or wake as the lid closes or opens with
    /// nothing else connected.
    pub sleep: Vec<String>,
    pub wake: Vec<String>,
    /// Turned on because no other screen was: the last resort, which may
    /// need the preferred mode.
    pub fallback: Option<String>,
    pub session: Session,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.enable.is_empty()
            && self.disable.is_empty()
            && self.unmirror.is_empty()
            && self.sleep.is_empty()
            && self.wake.is_empty()
    }
}

/// The connector a monitor mirrors (hyprctl reports its ID), if any.
fn mirrored<'a>(monitor: &Value, all: &'a Value) -> Option<&'a str> {
    let id = monitor["mirrorOf"]
        .as_str()
        .filter(|id| *id != "none" && !id.is_empty())?;
    all.as_array()?
        .iter()
        .find(|other| {
            other["id"]
                .as_i64()
                .is_some_and(|other| other.to_string() == id)
                || other["name"] == id
        })
        .and_then(|other| other["name"].as_str())
}

pub fn plan(all: &Value, lid_closed: bool, session: &Session, main: Option<&str>) -> Plan {
    let screens = screens(all);
    let set = arrangement(all);
    let chosen: Vec<&str> = session
        .off
        .get(&key(&set))
        .into_iter()
        .flatten()
        .filter_map(|id| {
            screens
                .iter()
                .find(|monitor| selector(monitor, all) == *id)
                .map(|monitor| name(monitor))
        })
        .collect();
    let externals: Vec<&str> = screens
        .iter()
        .map(|monitor| name(monitor))
        .filter(|name| !builtin(name))
        .collect();
    let is_on = |connector: &str| screens.iter().any(|m| name(m) == connector && on(m));
    let closed_display = lid_closed && !externals.is_empty();
    let mut enable: Vec<String> = vec![];
    let mut disable: BTreeSet<String> = BTreeSet::new();
    let mut sleep = vec![];
    let mut wake = vec![];

    // Docked or undocked: the displays the user turned off for these.
    if session.displays.as_ref() != Some(&set) {
        disable.extend(
            chosen
                .iter()
                .filter(|name| is_on(name))
                .map(|name| (*name).to_owned()),
        );
    }
    let mut lid_off: Vec<String> = session
        .lid_off
        .iter()
        .filter(|connector| screens.iter().any(|m| name(m) == connector.as_str()))
        .cloned()
        .collect();
    if closed_display {
        for monitor in &screens {
            let connector = name(monitor);
            if builtin(connector) && on(monitor) {
                disable.insert(connector.to_owned());
                if !lid_off.iter().any(|off| off == connector) {
                    lid_off.push(connector.to_owned());
                }
            }
        }
    } else {
        // Open, or with nothing else to show on: what the lid turned off
        // comes back, unless the user had turned it off too.
        for connector in lid_off.drain(..) {
            if !chosen.contains(&connector.as_str()) && !is_on(&connector) {
                enable.push(connector);
            }
        }
    }
    let opened = session.lid_closed == Some(true) && !lid_closed;
    let closed = session.lid_closed != Some(true) && lid_closed;

    // At least one connected screen on, preferring the built-in display
    // unless its lid is closed.
    let any_on = screens.iter().any(|m| {
        let connector = name(m);
        (is_on(connector) && !disable.contains(connector)) || enable.iter().any(|e| e == connector)
    });
    let mut fallback = None;
    if !screens.is_empty() && !any_on {
        let mut candidates: Vec<&str> = screens.iter().map(|m| name(m)).collect();
        candidates.sort_by_key(|connector| {
            let external = !builtin(connector);
            (
                if lid_closed { !external } else { external },
                main != Some(*connector),
                connector.to_string(),
            )
        });
        let candidate = candidates[0].to_owned();
        disable.remove(&candidate);
        lid_off.retain(|connector| *connector != candidate);
        if !is_on(&candidate) {
            enable.push(candidate.clone());
            fallback = Some(candidate);
        }
    }
    let stays_on = |connector: &str| {
        (is_on(connector) && !disable.contains(connector)) || enable.iter().any(|e| e == connector)
    };

    // With nothing else connected, the panel is blanked as the lid closes
    // (Keep Awake can hold off sleep), or as it is turned on behind the
    // closed lid (undocked with the lid closed): once, so a key still wakes it.
    if lid_closed && externals.is_empty() {
        for monitor in &screens {
            let connector = name(monitor);
            if builtin(connector)
                && stays_on(connector)
                && (closed || enable.iter().any(|e| e == connector))
                && monitor["dpmsStatus"] != false
            {
                sleep.push(connector.to_owned());
            }
        }
    }
    // Opening the lid wakes the screens, as a key press does.
    if opened {
        wake.extend(
            screens
                .iter()
                .map(|m| name(m))
                .filter(|connector| stays_on(connector))
                .map(str::to_owned),
        );
    }

    // Any output can be mirrored (a headless one, for streaming): only one
    // going off here ends the mirror.
    let source_stays = |source: &str| {
        if physical_name(source) {
            stays_on(source)
        } else {
            all.as_array()
                .into_iter()
                .flatten()
                .any(|m| name(m) == source && on(m))
        }
    };
    let unmirror = screens
        .iter()
        .filter(|monitor| stays_on(name(monitor)))
        .filter(|monitor| {
            mirrored(monitor, all).is_some_and(|source| !source_stays(source))
                || (monitor["mirrorOf"]
                    .as_str()
                    .is_some_and(|m| m != "none" && !m.is_empty())
                    && mirrored(monitor, all).is_none())
        })
        .map(|monitor| name(monitor).to_owned())
        .collect();

    Plan {
        enable,
        disable: disable.into_iter().collect(),
        unmirror,
        sleep,
        wake,
        fallback,
        session: Session {
            displays: Some(set),
            lid_off,
            lid_closed: Some(lid_closed),
            off: session.off.clone(),
        },
    }
}

/// Whether a change turns on a built-in display the closed lid would turn
/// off again at once.
pub fn behind_closed_lid(all: &Value, lid_closed: bool, connector: &str) -> bool {
    lid_closed && builtin(connector) && screens(all).iter().any(|monitor| !builtin(name(monitor)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn monitor(name: &str, id: i64, disabled: bool) -> Value {
        json!({"name": name, "id": id, "description": format!("{name} panel"), "disabled": disabled,
               "width": 1920, "height": 1080, "refreshRate": 60.0, "mirrorOf": "none", "dpmsStatus": true})
    }
    fn docked(panel_on: bool) -> Value {
        json!([monitor("eDP-1", 0, !panel_on), monitor("DP-1", 1, false)])
    }
    fn settled(all: &Value, lid: bool) -> Session {
        plan(all, lid, &Session::default(), None).session
    }

    #[test]
    fn closing_the_lid_while_docked_turns_the_panel_off_and_opening_brings_it_back() {
        let all = docked(true);
        let session = settled(&all, false);
        let closed = plan(&all, true, &session, None);
        assert_eq!(closed.disable, ["eDP-1"]);
        assert!(closed.enable.is_empty() && closed.sleep.is_empty());
        assert_eq!(closed.session.lid_off, ["eDP-1"]);
        // Once off, nothing more to do while the lid stays closed.
        let off = docked(false);
        assert!(plan(&off, true, &closed.session, None).is_empty());
        let opened = plan(&off, false, &closed.session, None);
        assert_eq!(opened.enable, ["eDP-1"]);
        assert!(opened.disable.is_empty() && opened.session.lid_off.is_empty());
    }

    #[test]
    fn undocking_with_the_lid_closed_brings_the_panel_back_blanked() {
        let mut session = settled(&docked(false), true);
        session.lid_off = vec!["eDP-1".into()];
        let alone = json!([monitor("eDP-1", 0, true)]);
        let undocked = plan(&alone, true, &session, None);
        assert_eq!(undocked.enable, ["eDP-1"]);
        // Behind the closed lid until it opens, if Keep Awake holds off sleep.
        assert_eq!(undocked.sleep, ["eDP-1"]);
        let mut asleep = monitor("eDP-1", 0, false);
        asleep["dpmsStatus"] = false.into();
        let opened = plan(&json!([asleep]), false, &undocked.session, None);
        assert_eq!(opened.wake, ["eDP-1"]);
    }

    #[test]
    fn the_lid_alone_blanks_and_wakes_the_panel_only_on_its_edges() {
        let alone = json!([monitor("eDP-1", 0, false)]);
        let open = settled(&alone, false);
        let closed = plan(&alone, true, &open, None);
        assert_eq!(closed.sleep, ["eDP-1"]);
        assert!(closed.disable.is_empty());
        // Woken by a key while closed: left on.
        assert!(plan(&alone, true, &closed.session, None).is_empty());
        assert_eq!(plan(&alone, false, &closed.session, None).wake, ["eDP-1"]);
    }

    #[test]
    fn a_display_turned_off_stays_off_with_the_same_displays() {
        let all = docked(true);
        let mut session = settled(&all, false);
        choose(&mut session, &all, &[], &["eDP-1".into()]);
        // Chosen now: the change itself turns it off, not the plan.
        let off = docked(false);
        assert!(plan(&off, false, &session, None).is_empty());
        // Undocked: on, the only screen.
        let alone = json!([monitor("eDP-1", 0, true)]);
        let undocked = plan(&alone, false, &session, None);
        assert_eq!(undocked.enable, ["eDP-1"]);
        assert_eq!(undocked.fallback.as_deref(), Some("eDP-1"));
        // Docked again, the panel came back on: off again, as chosen.
        let redocked = plan(&docked(true), false, &undocked.session, None);
        assert_eq!(redocked.disable, ["eDP-1"]);
        // Chosen on again: forgotten.
        let mut session = redocked.session;
        choose(&mut session, &docked(false), &["eDP-1".into()], &[]);
        assert!(session.off.is_empty());
        assert!(
            plan(
                &docked(true),
                false,
                &settled(&json!([monitor("eDP-1", 0, false)]), false),
                None
            )
            .disable
            .is_empty()
        );
    }

    #[test]
    fn opening_the_lid_keeps_a_panel_the_user_turned_off() {
        let all = docked(true);
        let mut session = settled(&all, false);
        choose(&mut session, &all, &[], &["eDP-1".into()]);
        let closed = plan(&docked(false), true, &session, None);
        assert!(closed.disable.is_empty());
        let opened = plan(&docked(false), false, &closed.session, None);
        assert!(opened.enable.is_empty());
    }

    #[test]
    fn some_screen_always_stays_on() {
        // Closed lid, the only external turned off by the user: the
        // external comes back rather than a panel behind the lid.
        let all = json!([monitor("eDP-1", 0, false), monitor("DP-1", 1, true)]);
        let mut session = settled(&all, false);
        choose(&mut session, &all, &[], &["DP-1".into()]);
        let closed = plan(&all, true, &session, None);
        assert_eq!(closed.enable, ["DP-1"]);
        assert_eq!(closed.disable, ["eDP-1"]);
        // Nothing on after an undock: the panel, or the main display.
        let lost = json!([
            monitor("HDMI-A-1", 2, true),
            monitor("eDP-1", 0, true),
            monitor("FALLBACK", 9, false)
        ]);
        assert_eq!(
            plan(&lost, false, &Session::default(), None).enable,
            ["eDP-1"]
        );
        assert_eq!(
            plan(&lost, true, &Session::default(), None).enable,
            ["HDMI-A-1"]
        );
        let desk = json!([monitor("DP-2", 3, true), monitor("DP-1", 1, true)]);
        assert_eq!(
            plan(&desk, false, &Session::default(), Some("DP-2")).enable,
            ["DP-2"]
        );
        // Nothing physical, or partial data: nothing to do.
        assert!(
            plan(
                &json!([monitor("HEADLESS-1", 4, true)]),
                false,
                &Session::default(),
                None
            )
            .is_empty()
        );
        assert!(
            plan(
                &json!([{"name": "eDP-1", "disabled": true}, {"name": "DP-1"}]),
                false,
                &Session::default(),
                None
            )
            .enable
            .is_empty()
        );
    }

    #[test]
    fn displays_mirroring_a_display_that_goes_show_their_own_picture() {
        let mut external = monitor("DP-1", 1, false);
        external["mirrorOf"] = "0".into();
        let all = json!([monitor("eDP-1", 0, false), external]);
        let closed = plan(&all, true, &settled(&all, false), None);
        assert_eq!(closed.disable, ["eDP-1"]);
        assert_eq!(closed.unmirror, ["DP-1"]);
        // Mirroring a display no longer listed.
        let mut orphan = monitor("eDP-1", 0, false);
        orphan["mirrorOf"] = "7".into();
        assert_eq!(
            plan(&json!([orphan]), false, &Session::default(), None).unmirror,
            ["eDP-1"]
        );
    }

    #[test]
    fn displays_are_recognized_on_another_port() {
        let all = docked(true);
        let mut session = settled(&all, false);
        choose(&mut session, &all, &[], &["eDP-1".into()]);
        // Unplugged, then plugged into another port.
        let alone = json!([monitor("eDP-1", 0, true)]);
        let undocked = plan(&alone, false, &session, None);
        let mut moved = monitor("DP-3", 5, false);
        moved["description"] = "DP-1 panel".into();
        let elsewhere = json!([monitor("eDP-1", 0, false), moved]);
        assert_eq!(
            plan(&elsewhere, false, &undocked.session, None).disable,
            ["eDP-1"]
        );
        // The same displays all along: nothing to apply again.
        assert!(plan(&elsewhere, false, &session, None).disable.is_empty());
    }

    #[test]
    fn opening_the_lid_wakes_every_screen_and_mirrors_of_virtual_outputs_stay() {
        let all = docked(true);
        let closed = plan(&all, true, &settled(&all, false), None);
        let opened = plan(&docked(false), false, &closed.session, None);
        assert_eq!(opened.wake, ["eDP-1", "DP-1"]);
        // A screen mirroring a headless output (streaming) keeps doing so.
        let mut mirror = monitor("DP-1", 1, false);
        mirror["mirrorOf"] = "4".into();
        let streaming = json!([
            monitor("eDP-1", 0, false),
            mirror,
            monitor("HEADLESS-2", 4, false)
        ]);
        assert!(plan(&streaming, false, &settled(&streaming, false), None).is_empty());
    }

    #[test]
    fn closed_lid_changes_are_refused_only_while_docked() {
        assert!(behind_closed_lid(&docked(false), true, "eDP-1"));
        assert!(!behind_closed_lid(&docked(false), false, "eDP-1"));
        assert!(!behind_closed_lid(
            &json!([monitor("eDP-1", 0, false)]),
            true,
            "eDP-1"
        ));
        assert!(!behind_closed_lid(&docked(false), true, "DP-1"));
    }
}
