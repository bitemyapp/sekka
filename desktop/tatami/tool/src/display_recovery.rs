// SPDX-License-Identifier: MIT OR Apache-2.0
//! Keeps the displays as display_policy.rs decides, through hotplug, the
//! lid, reloads and missed events: closed-display mode, the displays the
//! user turned off for each set of connected displays, and a connected
//! screen always on. Also brings back floating windows Hyprland leaves
//! outside every screen when a monitor goes: it moves them by the change
//! in monitor position only (WorkspacePlacementController.cpp), which can
//! put them beyond the remaining screens. And keeps the scale X11
//! applications draw at to the main display's (xwayland.rs).
use crate::{display_policy, displays, hypr, lid, util, window, xwayland};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, ErrorKind},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::Path,
    time::{Duration, Instant},
};

/// Serialize the settings window, shortcuts, reloads and the watcher.
pub fn lock() -> util::Result<File> {
    let dir = window::session_dir().ok_or("No desktop runtime directory")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("displays.lock"))
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        // SAFETY: the open file owns the descriptor until the guard is dropped.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != ErrorKind::WouldBlock {
            return Err(error.to_string());
        }
        if started.elapsed() >= Duration::from_secs(5) {
            return Err("Another display change is still in progress; please try again".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(file)
}

pub(crate) fn monitors() -> util::Result<Value> {
    let all = hypr::request_json("j/monitors all")?;
    if !all.is_array() {
        return Err("Invalid monitor list from Hyprland".into());
    }
    Ok(all)
}

pub(crate) fn evaluate(lua: &str) -> util::Result<()> {
    let reply = hypr::request(&format!("/eval {lua}"))?;
    if reply.trim() == "ok" {
        Ok(())
    } else {
        Err(format!(
            "Hyprland rejected the display settings: {}",
            reply.trim()
        ))
    }
}

fn find<'a>(all: &'a Value, name: &str) -> Option<&'a Value> {
    all.as_array()?.iter().find(|m| m["name"] == name)
}

pub(crate) fn lit(monitor: &Value) -> bool {
    monitor["disabled"] == false
        && monitor["width"].as_i64().is_some_and(|w| w > 0)
        && monitor["height"].as_i64().is_some_and(|h| h > 0)
}

/// An IPC acknowledgement only accepts the rule. Check the resulting state
/// before the settings window tells its caller that enabling succeeded.
pub(crate) fn verify(change: &displays::Change) -> util::Result<()> {
    if change.apply.is_empty() {
        return Ok(());
    }
    let started = Instant::now();
    loop {
        let all = monitors()?;
        let all = all.as_array().unwrap();
        if change
            .displays
            .iter()
            .filter(|s| change.apply.contains(&s.name))
            .all(|s| {
                all.iter().find(|m| m["name"] == s.name).is_some_and(|m| {
                    if !s.enabled {
                        return m["disabled"] == true;
                    }
                    let mirror = s
                        .mirror
                        .as_ref()
                        .and_then(|name| all.iter().find(|other| other["name"] == *name));
                    let mirror_matches = match mirror {
                        Some(source) => {
                            m["mirrorOf"].as_str()
                                == source["id"].as_i64().map(|id| id.to_string()).as_deref()
                        }
                        None => s.mirror.is_none() && m["mirrorOf"] == "none",
                    };
                    lit(m) && mirror_matches
                })
            })
        {
            return Ok(());
        }
        if started.elapsed() >= Duration::from_secs(2) {
            return Err("Hyprland did not apply the requested display state".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_enabled(name: &str) -> util::Result<Value> {
    for _ in 0..10 {
        let all = monitors()?;
        if let Some(monitor) = find(&all, name).filter(|m| lit(m)) {
            return Ok(monitor.clone());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{name} did not turn on"))
}

fn wait_unmirrored(names: &[&String]) -> util::Result<()> {
    for _ in 0..20 {
        let all = monitors()?;
        if names
            .iter()
            .all(|name| find(&all, name).is_none_or(|m| m["mirrorOf"] == "none"))
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "{} did not stop mirroring",
        names
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn wait_disabled(names: &[String]) -> util::Result<()> {
    for _ in 0..20 {
        let all = monitors()?;
        if names
            .iter()
            .all(|name| find(&all, name).is_none_or(|m| m["disabled"] == true))
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{} did not turn off", names.join(", ")))
}

/// The preferred mode, when kept settings cannot be shown: a saved mode,
/// transform or color format can become unusable after a dock changes.
fn preferred_rule(name: &str) -> String {
    format!(
        "hl.monitor({{ output = {}, disabled = false, mirror = \"\", mode = \"preferred\", position = \"auto\", scale = \"auto\", transform = 0, bitdepth = 8, cm = \"srgb\", vrr = 0 }})",
        window::lua_string(name)
    )
}

/// The settings to show a display with: as kept, or as it is.
fn settings_for(store: &Value, monitor: &Value, all: &Value) -> Option<displays::Settings> {
    displays::kept_for(store, monitor, all)
        .map(|(_, settings)| settings)
        .or_else(|| displays::Settings::from_monitor(monitor).filter(|s| !s.mode.starts_with("0x")))
}

/// Turn a display on with its kept settings, then its preferred mode. It
/// mirrors what it mirrored only while that stays on.
fn turn_on(
    name: &str,
    all: &Value,
    store: &Value,
    mirror_ok: &dyn Fn(&str) -> bool,
) -> util::Result<displays::Settings> {
    let monitor = find(all, name).ok_or_else(|| format!("{name} is not connected"))?;
    let settings = settings_for(store, monitor, all).map(|mut settings| {
        settings.enabled = true;
        if settings
            .mirror
            .as_deref()
            .is_some_and(|source| !mirror_ok(source))
        {
            settings.mirror = None;
        }
        settings
    });
    if let Some(settings) = settings
        && evaluate(&settings.rule(name))
            .and_then(|_| wait_enabled(name))
            .is_ok()
    {
        return Ok(settings);
    }
    evaluate(&preferred_rule(name))?;
    let restored = wait_enabled(name)?;
    displays::Settings::from_monitor(&restored).ok_or_else(|| format!("Cannot read {name}"))
}

/// Some screen must stay usable: each connected one with its preferred
/// mode, the built-in display first unless its lid is closed.
fn last_resort(all: &Value, lid_closed: bool) -> util::Result<String> {
    let mut candidates: Vec<&str> = display_policy::screens(all)
        .into_iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    candidates.sort_by_key(|name| {
        (
            display_policy::builtin(name) == lid_closed,
            name.to_string(),
        )
    });
    let mut last = Err("No display could be turned on".to_owned());
    for name in candidates {
        last = evaluate(&preferred_rule(name))
            .and_then(|_| wait_enabled(name).map(|_| name.to_owned()));
        if last.is_ok() {
            break;
        }
    }
    last
}

/// Carry out a plan, never turning a display off before another is on.
fn carry_out(
    all: &Value,
    store: &Value,
    plan: &display_policy::Plan,
    lid_closed: bool,
) -> util::Result<()> {
    let mut errors = vec![];
    let mut turned_on = vec![];
    let staying = |name: &str| {
        !plan.disable.iter().any(|off| off == name)
            && find(all, name)
                .is_some_and(|m| m["disabled"] != true || plan.enable.iter().any(|on| on == name))
    };
    for name in &plan.enable {
        let fallback = plan.fallback.as_deref() == Some(name.as_str());
        let mirror_ok = |source: &str| !fallback && staying(source);
        match turn_on(name, all, store, &mirror_ok) {
            Ok(settings) => {
                turned_on.push(name.clone());
                // Recovered without the settings window: kept that way
                // through reloads, without a mirror of a display now gone,
                // and where it was arranged.
                if fallback {
                    save_recovered(store, all, settings);
                }
            }
            Err(error) => errors.push(format!("{name}: {error}")),
        }
    }
    let now = monitors()?;
    let mut on: Vec<String> = display_policy::screens(&now)
        .into_iter()
        .filter(|m| lit(m))
        .filter_map(|m| m["name"].as_str().map(str::to_owned))
        .collect();
    if on.is_empty() && !display_policy::screens(&now).is_empty() {
        let name = last_resort(&now, lid_closed)?;
        turned_on.push(name.clone());
        on.push(name);
    }
    // Mirrors show their own picture first, confirmed, then what they
    // mirrored goes: Hyprland would otherwise end each mirror while
    // iterating them (Monitor.cpp onDisconnect).
    let unmirror: Vec<&String> = plan
        .unmirror
        .iter()
        .filter(|name| find(&now, name).is_some_and(lit))
        .collect();
    if !unmirror.is_empty() {
        let lua: String = unmirror
            .iter()
            .filter_map(|name| {
                let monitor = find(&now, name)?;
                let mut settings = settings_for(store, monitor, &now)?;
                settings.enabled = true;
                settings.mirror = None;
                Some(settings.rule(name) + "\n")
            })
            .collect();
        if let Err(error) = evaluate(&lua).and_then(|_| wait_unmirrored(&unmirror)) {
            errors.push(error);
        }
    }
    let mut turned_off = vec![];
    if !plan.disable.is_empty() {
        let remaining = on.iter().any(|name| !plan.disable.contains(name));
        if !remaining {
            errors.push(format!(
                "{} stays on: no other display is on",
                plan.disable.join(", ")
            ));
        } else {
            let lua: String = plan
                .disable
                .iter()
                .map(|name| {
                    format!(
                        "hl.monitor({{ output = {}, disabled = true }})\n",
                        window::lua_string(name)
                    )
                })
                .collect();
            match evaluate(&lua).and_then(|_| wait_disabled(&plan.disable)) {
                Ok(()) => turned_off.extend(plan.disable.iter().cloned()),
                Err(error) => errors.push(error),
            }
        }
    }
    for (names, action) in [(&plan.sleep, "off"), (&plan.wake, "on")] {
        for name in names {
            let reply = hypr::request(&format!(
                "/dispatch hl.dsp.dpms({{ action = \"{action}\", monitor = {} }})",
                window::lua_string(name)
            ));
            if !reply.as_ref().is_ok_and(|reply| reply.trim() == "ok") {
                errors.push(format!("{name}: could not turn the screen {action}"));
            }
        }
    }
    if (!turned_on.is_empty() || !turned_off.is_empty())
        && !window::switch_off(&turned_off, &turned_on)
    {
        errors.push("cannot save the displays' session state".into());
    }
    for name in &turned_on {
        eprintln!("tatami: turned {name} on");
    }
    for name in &turned_off {
        eprintln!("tatami: turned {name} off");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Keep a recovered display's settings without a mirror of a display that
/// went, so a reload leaves it showing its own picture.
fn save_recovered(store: &Value, all: &Value, settings: displays::Settings) {
    let mut connected: Vec<_> = display_policy::screens(all)
        .into_iter()
        .filter(|m| m["name"] != settings.name.as_str())
        .filter_map(display_policy_identity)
        .collect();
    let name = settings.name.clone();
    connected.push(settings);
    let change = displays::Change {
        displays: connected,
        main: None,
        apply: vec![name],
    };
    if !displays::save_store(&displays::updated_store(store, &change)) {
        eprintln!("tatami: display recovered, but its settings could not be saved");
    }
}

/// Other displays, for selector disambiguation only: left as kept.
fn display_policy_identity(monitor: &Value) -> Option<displays::Settings> {
    let mut settings = displays::Settings::from_monitor(monitor)?;
    settings.enabled = false;
    Some(settings)
}

/// Bring the displays to what the policy decides, once. Returns whether
/// anything was changed.
pub fn reconcile(lid_closed: bool) -> util::Result<bool> {
    let _guard = lock()?;
    let all = monitors()?;
    let store = displays::load_store();
    let session = display_policy::load(&all);
    let plan = display_policy::plan(&all, lid_closed, &session, store["main"].as_str());
    if plan.is_empty() {
        if plan.session != session && !display_policy::save(&plan.session) {
            return Err("cannot save the displays' session state".into());
        }
        return Ok(false);
    }
    // Kept only once carried out, so a failure is retried.
    carry_out(&all, &store, &plan, lid_closed)?;
    if !display_policy::save(&plan.session) {
        return Err("cannot save the displays' session state".into());
    }
    Ok(true)
}

/// `tatami displays recover`: one reconciliation with the lid as it is.
pub fn recover() -> util::Result<bool> {
    reconcile(lid::closed().unwrap_or(false))
}

/// At login, before Hyprland starts: with the lid closed and another screen
/// connected, the built-in display starts off rather than lighting behind
/// the lid and taking workspaces for a moment.
pub fn prepare_session() {
    if lid::closed() != Some(true) {
        return;
    }
    let outputs = lid::connected_outputs(Path::new("/sys/class/drm"));
    let builtins: Vec<String> = outputs
        .iter()
        .filter(|name| display_policy::physical_name(name) && display_policy::builtin(name))
        .cloned()
        .collect();
    let external = outputs
        .iter()
        .any(|name| display_policy::physical_name(name) && !display_policy::builtin(name));
    if builtins.is_empty() || !external {
        return;
    }
    let session = display_policy::Session {
        lid_off: builtins.clone(),
        lid_closed: Some(true),
        ..Default::default()
    };
    if !display_policy::save(&session) || !window::switch_off(&builtins, &[]) {
        display_policy::forget();
        window::forget_monitors();
    }
}

fn overlaps(a: hypr::Rect, b: hypr::Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Floating windows that Hyprland moved off their own screen (or every
/// screen), by address. A window the user drags between screens moves to
/// the screen it is dropped on, so none is meant to be there.
pub fn lost_windows(monitors: &Value, clients: &Value) -> Vec<String> {
    let screens: Vec<(i64, hypr::Rect)> = monitors
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| lit(m))
        .filter_map(|m| Some((m["id"].as_i64().unwrap_or(-1), hypr::monitor_rect(m)?)))
        .collect();
    if screens.is_empty() {
        return vec![];
    }
    clients
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["mapped"] == true && c["hidden"] != true && c["floating"] == true)
        // Special workspaces (scratchpads) follow the monitor showing them.
        .filter(|c| c["workspace"]["id"].as_i64().is_some_and(|id| id > 0))
        .filter_map(|c| {
            let window = hypr::Rect {
                x: c["at"][0].as_i64()?,
                y: c["at"][1].as_i64()?,
                w: c["size"][0].as_i64()?.max(1),
                h: c["size"][1].as_i64()?.max(1),
            };
            let own = screens
                .iter()
                .find(|(id, _)| c["monitor"].as_i64() == Some(*id));
            let visible = match own {
                Some((_, screen)) => overlaps(window, *screen),
                None => screens.iter().any(|(_, screen)| overlaps(window, *screen)),
            };
            (!visible)
                .then(|| hypr::address(c).map(str::to_owned))
                .flatten()
        })
        .collect()
}

/// Center lost floating windows on their own screen.
pub fn rescue_windows() -> util::Result<usize> {
    let monitors = monitors()?;
    let clients = hypr::request_json("j/clients")?;
    let lost = lost_windows(&monitors, &clients);
    if lost.is_empty() {
        return Ok(0);
    }
    let lua: String = lost
        .iter()
        .map(|address| {
            format!("hl.dispatch(hl.dsp.window.center({{ window = \"address:{address}\" }}))\n")
        })
        .collect();
    // A window closed meanwhile is only a notice, not a failure.
    let reply = hypr::request(&format!("/eval {lua}"))?;
    if reply.trim_start().starts_with("error:") {
        return Err(reply.trim().to_owned());
    }
    eprintln!("tatami: brought {} window(s) back on screen", lost.len());
    Ok(lost.len())
}

/// Where the screens are: a change means windows may have been left behind.
fn layout(all: &Value) -> String {
    let mut screens: Vec<String> = all
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| lit(m))
        .map(|m| {
            format!(
                "{} {} {} {} {} {} {}",
                m["name"], m["x"], m["y"], m["width"], m["height"], m["scale"], m["transform"]
            )
        })
        .collect();
    screens.sort();
    screens.join("\n")
}

/// The lid as Hyprland's lid bindings report it, with logind's state polled
/// for what they miss: a switch named otherwise, a change while the watcher
/// restarted. A poll changes it only when logind's answer changes, so a
/// machine whose lid only Hyprland sees keeps the bindings' report.
struct Lid {
    closed: bool,
    polled: Option<bool>,
    last_poll: Instant,
}

impl Lid {
    fn new() -> Self {
        let polled = lid::closed();
        Self {
            closed: polled.unwrap_or(false),
            polled,
            last_poll: Instant::now(),
        }
    }

    /// Poll now, or when due: whether the lid changed.
    fn poll(&mut self, now: bool) -> bool {
        if !now && self.last_poll.elapsed() < Duration::from_secs(2) {
            return false;
        }
        self.last_poll = Instant::now();
        let polled = lid::closed();
        if polled == self.polled {
            return false;
        }
        self.polled = polled;
        match polled {
            Some(closed) if closed != self.closed => {
                self.closed = closed;
                true
            }
            _ => false,
        }
    }
}

/// XWayland's `Xft.dpi`, kept at the main display's scale (xwayland.rs):
/// set when that scale or the main display changes, and tried again a few
/// seconds after a failure, such as XWayland not running yet.
struct XScale {
    display: Option<String>,
    set: Option<u32>,
    last_error: Option<String>,
    next_try: Instant,
}

impl XScale {
    fn new() -> Self {
        Self {
            display: xwayland::display(),
            set: None,
            last_error: None,
            next_try: Instant::now(),
        }
    }

    fn keep(&mut self, all: &Value) {
        let Some(dpi) = xwayland::wanted(all, displays::load_store()["main"].as_str()) else {
            return;
        };
        if self.set == Some(dpi) || Instant::now() < self.next_try {
            return;
        }
        if self.display.is_none() {
            self.display = xwayland::display();
        }
        let result = match &self.display {
            Some(display) => xwayland::set_dpi(display, dpi),
            None => Err("no XWayland display".to_owned()),
        };
        let error = result.err();
        if error != self.last_error
            && let Some(error) = &error
        {
            eprintln!("tatami: XWayland scale: {error}");
        }
        self.set = error.is_none().then_some(dpi);
        self.next_try = Instant::now() + Duration::from_secs(if error.is_some() { 5 } else { 0 });
        self.last_error = error;
    }
}

/// The lid bindings' socket events (config/hypr/hyprland.lua).
fn lid_event(line: &str) -> Option<bool> {
    match line.trim_end().strip_prefix("custom>>")? {
        "tatami-lid-closed" => Some(true),
        "tatami-lid-open" => Some(false),
        _ => None,
    }
}

pub fn watch() -> util::Result<()> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "No desktop runtime directory")?;
    let signature =
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE").map_err(|_| "No Hyprland session")?;
    let stream = UnixStream::connect(format!("{runtime}/hypr/{signature}/.socket2.sock"))
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut lid = Lid::new();
    let mut last_check = Instant::now() - Duration::from_secs(2);
    let mut last_error = None;
    let mut changed = true;
    let mut screens = None;
    let mut rescue_until = Instant::now();
    let mut scale = XScale::new();
    loop {
        if changed || last_check.elapsed() >= Duration::from_secs(1) {
            lid.poll(changed);
            let error = reconcile(lid.closed).err();
            if error != last_error
                && let Some(error) = &error
            {
                eprintln!("tatami: display recovery: {error}");
            }
            last_error = error;
            // Windows are rescued for a few seconds after the screens change,
            // as Hyprland settles: never otherwise, so an application may
            // place its own windows where it likes.
            if let Ok(all) = monitors() {
                let now = layout(&all);
                if screens.as_ref() != Some(&now) {
                    if screens.is_some() {
                        rescue_until = Instant::now() + Duration::from_secs(3);
                    }
                    screens = Some(now);
                }
                scale.keep(&all);
            }
            if Instant::now() < rescue_until
                && let Err(error) = rescue_windows()
            {
                eprintln!("tatami: window rescue: {error}");
            }
            last_check = Instant::now();
        }
        // Periodic reconciliation also covers events emitted before startup,
        // partial socket reads, and delayed hardware state after hotplug.
        let mut line = String::new();
        changed = match reader.read_line(&mut line) {
            Ok(0) => return Err("Hyprland event connection closed".into()),
            Ok(_) => {
                if let Some(closed) = lid_event(&line) {
                    lid.closed = closed;
                    true
                } else {
                    matches!(
                        line.split(">>").next(),
                        Some(
                            "monitoradded"
                                | "monitoraddedv2"
                                | "monitorremoved"
                                | "monitorremovedv2"
                                | "configreloaded"
                        )
                    )
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                false
            }
            Err(e) => return Err(e.to_string()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn lid_bindings_are_read_from_the_event_socket() {
        assert_eq!(lid_event("custom>>tatami-lid-closed\n"), Some(true));
        assert_eq!(lid_event("custom>>tatami-lid-open"), Some(false));
        assert_eq!(lid_event("custom>>other"), None);
        assert_eq!(lid_event("monitoradded>>DP-1"), None);
    }
    #[test]
    fn only_floating_windows_outside_every_screen_are_lost() {
        let monitors = json!([
            {"id": 0, "name": "eDP-1", "disabled": false, "x": 0, "y": 0, "width": 2880, "height": 1800, "scale": 2.0, "transform": 0},
            {"id": 1, "name": "DP-1", "disabled": false, "x": 1440, "y": 0, "width": 3840, "height": 2160, "scale": 2.0, "transform": 0},
            {"id": 2, "name": "HDMI-A-1", "disabled": true, "x": -4000, "y": 0, "width": 0, "height": 0, "scale": 1.0, "transform": 0}
        ]);
        let client = |address: &str, x: i64, floating: bool, workspace: i64, monitor: i64| {
            json!({"address": address, "mapped": true, "hidden": false, "floating": floating,
                   "workspace": {"id": workspace}, "monitor": monitor, "at": [x, 60], "size": [880, 800]})
        };
        let clients = json!([
            // Moved off every screen when a monitor went (the Displays window).
            client("0x1", -1720, true, 3, 0),
            client("0x2", 200, true, 3, 0),
            // Partly on its screen: where the user put it.
            client("0x3", -400, true, 3, 0),
            client("0x4", -1720, false, 3, 0),
            client("0x5", -1720, true, -98, 0),
            client("0x6", 3400, true, 2, 1),
            client("0x7", 3300, true, 2, 1),
            // Over the panel but belonging to the external display: placed by
            // a replug before the display had a position.
            client("0x8", 100, true, 2, 1),
            // Its screen is gone: anywhere visible will do.
            client("0x9", 100, true, 4, 7),
        ]);
        assert_eq!(lost_windows(&monitors, &clients), ["0x1", "0x6", "0x8"]);
        assert!(lost_windows(&json!([]), &clients).is_empty());
    }
}
