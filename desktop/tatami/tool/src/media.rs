// SPDX-License-Identifier: MIT OR Apache-2.0
//! Volume, microphone, brightness and media keys with SwayOSD feedback on the
//! focused monitor.
use crate::{hypr, util};

fn osd(args: &[&str]) {
    let mut full: Vec<String> = vec![];
    if let Some(monitor) = hypr::json("monitors")
        .ok()
        .and_then(|monitors| hypr::focused_monitor(&monitors))
    {
        full.extend(["--monitor".into(), monitor]);
    }
    full.extend(args.iter().map(|arg| arg.to_string()));
    let full: Vec<&str> = full.iter().map(String::as_str).collect();
    util::run("swayosd-client", &full);
}

/// raise | lower | mute-toggle | +N | -N
pub fn volume(change: &str) {
    osd(&["--output-volume", change]);
}

pub fn media(action: &str) {
    osd(&["--playerctl", action]);
}

pub fn mic_mute() {
    util::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]);
    let muted = util::output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"])
        .is_ok_and(|state| state.contains("[MUTED]"));
    let (message, icon) = if muted {
        ("Microphone muted", "microphone-sensitivity-muted-symbolic")
    } else {
        ("Microphone on", "audio-input-microphone-symbolic")
    };
    osd(&["--custom-message", message, "--custom-icon", icon]);
}

/// The most likely internal panel among /sys/class/backlight entries.
pub fn backlight_device(mut names: Vec<String>) -> Option<String> {
    names.sort();
    for prefix in ["amdgpu_bl", "intel_backlight", "acpi_video"] {
        if let Some(name) = names.iter().find(|name| name.starts_with(prefix)) {
            return Some(name.clone());
        }
    }
    names.into_iter().next()
}

/// Non-uniform steps as in Omarchy: 1% at or below 5%, else 5%, expressed
/// as an absolute target so rounding never makes the OSD jump unevenly.
pub fn brightness_target(current: u32, step: &str) -> String {
    match step {
        "+5%" => format!("{}%", (current + if current < 5 { 1 } else { 5 }).min(100)),
        "5%-" => format!(
            "{}%",
            current
                .saturating_sub(if current <= 5 { 1 } else { 5 })
                .max(1)
        ),
        other => other.to_owned(),
    }
}

/// The percentage field of `brightnessctl -m`: `device,class,raw,NN%,max`.
pub fn machine_percent(line: &str) -> Option<u32> {
    line.trim()
        .split(',')
        .nth(3)?
        .trim_end_matches('%')
        .parse()
        .ok()
}

pub fn brightness(step: &str) {
    match step {
        "on" => return hypr::dpms(true),
        "off" => return hypr::dpms(false),
        _ => {}
    }
    let names = std::fs::read_dir("/sys/class/backlight")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let Some(device) = backlight_device(names) else {
        util::notify("\u{f0379}", "This display has no adjustable brightness", "");
        return;
    };
    let current = util::output("brightnessctl", &["-d", &device, "-m"])
        .ok()
        .and_then(|line| machine_percent(&line))
        .unwrap_or(50);
    let target = brightness_target(current, step);
    util::run("brightnessctl", &["-d", &device, "set", &target]);
    let now = util::output("brightnessctl", &["-d", &device, "-m"])
        .ok()
        .and_then(|line| machine_percent(&line))
        .unwrap_or(current);
    let progress = format!("{:.2}", (f64::from(now) / 100.0).max(0.01));
    osd(&[
        "--custom-icon",
        "display-brightness-symbolic",
        "--custom-progress",
        &progress,
        "--custom-progress-text",
        &format!("{now}%"),
    ]);
}

/// The next keyboard backlight level for up, down or cycle (Fn+Space style).
pub fn keyboard_level(current: u32, max: u32, step: &str) -> u32 {
    match step {
        "up" => (current + 1).min(max),
        "down" => current.saturating_sub(1),
        _ if current >= max => 0,
        _ => current + 1,
    }
}

/// Keyboard backlight keys, with the level on the on-screen display.
pub fn keyboard_brightness(step: &str) {
    let mut names: Vec<String> = std::fs::read_dir("/sys/class/leds")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.contains("kbd_backlight"))
        .collect();
    names.sort();
    let Some(device) = names.first() else {
        return;
    };
    let number = |what: &str| {
        util::output("brightnessctl", &["-d", device, what])
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
    };
    let (Some(current), Some(max)) = (number("get"), number("max")) else {
        return;
    };
    let level = keyboard_level(current, max, step);
    util::run("brightnessctl", &["-d", device, "set", &level.to_string()]);
    let progress = format!(
        "{:.2}",
        (f64::from(level) / f64::from(max.max(1))).max(0.01)
    );
    osd(&[
        "--custom-icon",
        "keyboard-brightness-symbolic",
        "--custom-progress",
        &progress,
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefers_internal_panels() {
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            backlight_device(names(&["acpi_video0", "intel_backlight"])).as_deref(),
            Some("intel_backlight")
        );
        assert_eq!(
            backlight_device(names(&["nvidia_0", "amdgpu_bl1"])).as_deref(),
            Some("amdgpu_bl1")
        );
        assert_eq!(
            backlight_device(names(&["ddcci3"])).as_deref(),
            Some("ddcci3")
        );
        assert_eq!(backlight_device(vec![]), None);
    }
    #[test]
    fn brightness_steps_are_fine_near_the_bottom() {
        assert_eq!(brightness_target(3, "+5%"), "4%");
        assert_eq!(brightness_target(50, "+5%"), "55%");
        assert_eq!(brightness_target(98, "+5%"), "100%");
        assert_eq!(brightness_target(5, "5%-"), "4%");
        assert_eq!(brightness_target(1, "5%-"), "1%");
        assert_eq!(brightness_target(40, "5%-"), "35%");
        assert_eq!(brightness_target(40, "100%"), "100%");
        assert_eq!(
            machine_percent("intel_backlight,backlight,9600,50%,19200"),
            Some(50)
        );
        assert_eq!(machine_percent("bad"), None);
    }
    #[test]
    fn keyboard_levels_step_and_cycle() {
        assert_eq!(keyboard_level(1, 2, "up"), 2);
        assert_eq!(keyboard_level(2, 2, "up"), 2);
        assert_eq!(keyboard_level(0, 2, "down"), 0);
        assert_eq!(keyboard_level(2, 2, "cycle"), 0);
        assert_eq!(keyboard_level(0, 2, "cycle"), 1);
    }
}
