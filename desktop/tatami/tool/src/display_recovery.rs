// SPDX-License-Identifier: MIT OR Apache-2.0
//! Keep an undocked laptop usable even when its panel was turned off.
use crate::{displays, hypr, util, window};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, ErrorKind},
    os::{fd::AsRawFd, unix::net::UnixStream},
    time::{Duration, Instant},
};

/// Serialize the settings window, shortcuts, reloads and hotplug recovery.
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
        if started.elapsed() >= Duration::from_secs(3) {
            return Err("Another display change is still in progress; please try again".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(file)
}

fn builtin(name: &str) -> bool {
    ["eDP-", "LVDS-", "DSI-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn physical(monitor: &Value) -> bool {
    monitor["name"].as_str().is_some_and(|name| {
        hypr::safe_name(name)
            && ["eDP-", "LVDS-", "DSI-", "DP-", "HDMI-", "DVI-", "VGA-"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
    })
}

/// FALLBACK and virtual outputs cannot make a black physical screen usable.
/// DPMS is deliberately ignored: normal idle blanking must stay asleep.
fn target(all: &Value) -> Option<&Value> {
    let monitors = all.as_array()?;
    if monitors
        .iter()
        .any(|m| physical(m) && m["disabled"] != true)
    {
        return None;
    }
    let mut disabled = monitors
        .iter()
        .filter(|m| physical(m) && m["disabled"] == true);
    disabled
        .clone()
        .find(|m| builtin(m["name"].as_str().unwrap_or("")))
        .or_else(|| disabled.next())
}

/// An unresponsive compositor must not wedge recovery forever. Coreutils is
/// part of the NixOS system PATH; timeout also terminates a stuck hyprctl.
fn request(args: &[&str]) -> util::Result<String> {
    let mut bounded = vec!["--kill-after=1s", "2s", "hyprctl"];
    bounded.extend_from_slice(args);
    util::output("timeout", &bounded)
}

fn monitors() -> util::Result<Value> {
    let all: Value =
        serde_json::from_str(&request(&["-j", "monitors", "all"])?).map_err(|e| e.to_string())?;
    if !all.is_array() {
        return Err("Invalid monitor list from Hyprland".into());
    }
    Ok(all)
}

pub(crate) fn evaluate(rule: &str) -> util::Result<()> {
    let reply = request(&["eval", rule])?;
    if reply.trim() == "ok" {
        Ok(())
    } else {
        Err(format!("Hyprland rejected the display settings: {reply}"))
    }
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
                    m["disabled"] == false
                        && mirror_matches
                        && m["width"].as_i64().is_some_and(|w| w > 0)
                        && m["height"].as_i64().is_some_and(|h| h > 0)
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
        if let Some(monitor) = all.as_array().unwrap().iter().find(|m| {
            m["name"] == name
                && m["disabled"] == false
                && m["width"].as_i64().is_some_and(|w| w > 0)
                && m["height"].as_i64().is_some_and(|h| h > 0)
        }) {
            return Ok(monitor.clone());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{name} did not turn on"))
}

pub fn recover() -> util::Result<bool> {
    let _guard = lock()?;
    let all = monitors()?;
    let Some(monitor) = target(&all) else {
        return Ok(false);
    };
    let name = monitor["name"].as_str().unwrap();
    let store = displays::load_store();
    let mut settings = displays::kept_for(&store, monitor, &all)
        .map(|(_, s)| s)
        .or_else(|| displays::Settings::from_monitor(monitor))
        .ok_or("Cannot read the remaining display's settings")?;
    settings.enabled = true;
    settings.mirror = None;
    settings.position = "auto".into();
    if !window::switch_off(&[], &[name.to_owned()]) {
        return Err("Cannot clear the display's disabled session state".into());
    }
    let first = evaluate(&settings.rule(name)).and_then(|_| wait_enabled(name));
    let restored = match first {
        Ok(restored) => restored,
        Err(_) => {
            // A saved mode, transform or color format can become unusable
            // after a dock changes. Fall back to the panel's preferred mode.
            evaluate(&format!(
                "hl.monitor({{ output = {}, disabled = false, mirror = \"\", mode = \"preferred\", position = \"auto\", scale = \"auto\", transform = 0, bitdepth = 8, cm = \"srgb\", vrr = 0 }})",
                window::lua_string(name)
            ))?;
            let restored = wait_enabled(name)?;
            settings = displays::Settings::from_monitor(&restored)
                .ok_or("Cannot read recovered display")?;
            restored
        }
    };
    // Keep the recovered state through reloads too, including clearing a
    // stale mirror target. Other displays and the preferred main stay saved.
    let mut connected: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["name"] != name)
        .filter_map(displays::Settings::from_monitor)
        .map(|mut s| {
            s.enabled = false;
            s
        })
        .collect();
    // Include the other descriptions for selector disambiguation, without
    // rewriting those displays' preferences.
    connected.push(settings);
    let change = displays::Change {
        displays: connected,
        main: None,
        apply: vec![name.to_owned()],
    };
    if !displays::save_store(&displays::updated_store(&store, &change)) {
        return Err("Display restored, but its settings could not be saved".into());
    }
    if restored["dpmsStatus"] == false {
        request(&["dispatch", "hl.dsp.dpms({ action = \"on\" })"])?;
    }
    eprintln!("tatami: restored {name} because no physical display was enabled");
    Ok(true)
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
    let mut last_check = Instant::now() - Duration::from_secs(2);
    let mut last_error = None;
    let mut changed = true;
    loop {
        if changed || last_check.elapsed() >= Duration::from_secs(1) {
            let error = recover().err();
            if error != last_error
                && let Some(error) = &error
            {
                eprintln!("tatami: display recovery: {error}");
            }
            last_error = error;
            last_check = Instant::now();
        }
        // Periodic reconciliation also covers events emitted before startup,
        // partial socket reads, and delayed hardware state after hotplug.
        let mut line = String::new();
        changed = match reader.read_line(&mut line) {
            Ok(0) => return Err("Hyprland event connection closed".into()),
            Ok(_) => matches!(
                line.split(">>").next(),
                Some(
                    "monitoradded"
                        | "monitoraddedv2"
                        | "monitorremoved"
                        | "monitorremovedv2"
                        | "configreloaded"
                )
            ),
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
    fn restores_panel_after_last_external_display_is_removed() {
        let all = json!([{"name":"eDP-1","disabled":true}, {"name":"FALLBACK","disabled":false}]);
        assert_eq!(target(&all).unwrap()["name"], "eDP-1");
    }
    #[test]
    fn leaves_docked_and_normally_blanked_displays_alone() {
        for dpms in [true, false] {
            assert!(target(&json!([{"name":"eDP-1","disabled":true}, {"name":"DP-1","disabled":false,"dpmsStatus":dpms}])).is_none());
        }
    }
    #[test]
    fn prefers_builtin_but_recovers_external_only_machines_too() {
        let all = json!([{"name":"HDMI-A-1","disabled":true}, {"name":"eDP-1","disabled":true}]);
        assert_eq!(target(&all).unwrap()["name"], "eDP-1");
        assert_eq!(
            target(&json!([{"name":"HDMI-A-1","disabled":true}])).unwrap()["name"],
            "HDMI-A-1"
        );
    }
    #[test]
    fn does_not_treat_virtual_outputs_or_missing_data_as_physical_displays() {
        assert!(target(&json!([{"name":"HEADLESS-1","disabled":true}, {"name":"FALLBACK","disabled":false}])).is_none());
        assert!(target(&json!([{"name":"eDP-1","disabled":true}, {"name":"DP-1"}])).is_none());
        assert!(target(&json!([])).is_none());
    }
}
