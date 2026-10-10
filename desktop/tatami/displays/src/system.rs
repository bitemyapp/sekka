// SPDX-License-Identifier: MIT OR Apache-2.0
//! What the window reads and changes outside itself: Hyprland's displays
//! (`hyprctl`), the settings the `tatami` helper keeps and applies, the
//! built-in display's backlight, and Night Shift (hyprsunset, through the
//! helper).
use crate::model::Display;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Command, Stdio},
};

fn output(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{program} exited with {}", out.status))
    }
}

pub fn displays() -> Result<Vec<Display>, String> {
    let text = output("hyprctl", &["-j", "monitors", "all"])?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Display::from_json)
        .collect())
}

/// Hyprland's variable refresh rate for displays without a setting of their
/// own (`misc:vrr`).
pub fn default_vrr() -> i64 {
    output("hyprctl", &["-j", "getoption", "misc:vrr"])
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["int"].as_i64())
        .unwrap_or(0)
}

fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|dir| dir.starts_with('/'))
        .unwrap_or_else(|| format!("{}/.local/state", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("tatami")
}

/// The settings the helper keeps (tool/src/displays.rs).
pub fn kept() -> Value {
    std::fs::read_to_string(state_dir().join("displays.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({ "displays": {} }))
}

/// With TATAMI_DISPLAYS_DRY_RUN set, changes are printed, not made.
pub fn dry_run() -> bool {
    std::env::var_os("TATAMI_DISPLAYS_DRY_RUN").is_some()
}

/// Keep and apply a change (`tatami displays apply`).
pub fn apply(change: &Value) -> Result<(), String> {
    let mut args = vec!["displays", "apply"];
    if dry_run() {
        eprintln!("change: {change}");
        args.push("--dry-run");
    }
    let mut child = Command::new("tatami")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(if dry_run() {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("tatami: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(change.to_string().as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let out = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&out.stderr);
        let message = message.trim().trim_start_matches("tatami: ");
        Err(if message.is_empty() {
            "The display settings could not be applied".to_owned()
        } else {
            message.to_owned()
        })
    }
}

/// The built-in display's backlight, as the brightness keys choose it.
pub fn backlight() -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    for prefix in ["amdgpu_bl", "intel_backlight", "acpi_video"] {
        if let Some(name) = names.iter().find(|name| name.starts_with(prefix)) {
            return Some(name.clone());
        }
    }
    names.into_iter().next()
}

/// The backlight's brightness, in percent.
pub fn brightness(device: &str) -> Option<f64> {
    let read = |file: &str| {
        std::fs::read_to_string(format!("/sys/class/backlight/{device}/{file}"))
            .ok()?
            .trim()
            .parse::<f64>()
            .ok()
    };
    let max = read("max_brightness").filter(|max| *max > 0.0)?;
    Some(read("brightness")? * 100.0 / max)
}

pub fn set_brightness(device: &str, percent: f64) {
    if dry_run() {
        eprintln!("brightness {device} {percent:.0}%");
        return;
    }
    let target = format!("{:.0}%", percent.clamp(1.0, 100.0));
    let _ = Command::new("brightnessctl")
        .args(["-q", "-d", device, "set", &target])
        .stdin(Stdio::null())
        .status();
}

/// Night Shift: Some(temperature) when the screen is tinted, and the night
/// temperature that turning it on uses.
pub fn night_shift() -> (Option<u32>, u32) {
    let status = output("tatami", &["nightlight", "status"]).unwrap_or_default();
    let mut words = status.split_whitespace();
    match words.next() {
        Some("off") => (
            None,
            words.next().and_then(|t| t.parse().ok()).unwrap_or(4000),
        ),
        Some(kelvin) => {
            let kelvin = kelvin.parse().ok();
            (kelvin, kelvin.unwrap_or(4000))
        }
        None => (None, 4000),
    }
}

/// "on", "off", or a temperature in kelvin.
pub fn set_night_shift(request: &str) {
    if dry_run() {
        eprintln!("nightlight {request}");
        return;
    }
    // hyprsunset takes a moment to start: off the main loop.
    let request = request.to_owned();
    std::thread::spawn(move || {
        let _ = Command::new("tatami")
            .args(["nightlight", &request])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

/// Call `changed` whenever Hyprland reports displays coming, going or its
/// configuration reloaded, from its event socket.
pub fn watch(changed: async_channel::Sender<()>) {
    let (Ok(runtime), Ok(signature)) = (
        std::env::var("XDG_RUNTIME_DIR"),
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE"),
    ) else {
        return;
    };
    let path = format!("{runtime}/hypr/{signature}/.socket2.sock");
    std::thread::spawn(move || {
        let Ok(stream) = UnixStream::connect(&path) else {
            return;
        };
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let event = line.split(">>").next().unwrap_or("");
            if matches!(
                event,
                "monitoradded"
                    | "monitoraddedv2"
                    | "monitorremoved"
                    | "monitorremovedv2"
                    | "configreloaded"
            ) && changed.send_blocking(()).is_err()
            {
                return;
            }
        }
    });
}
