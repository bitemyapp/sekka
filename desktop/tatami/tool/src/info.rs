// SPDX-License-Identifier: MIT OR Apache-2.0
//! Time and battery notifications (Super+Ctrl+Alt+T and B), after Omarchy's
//! omarchy-notification-time and omarchy-battery-status.
use crate::util;
use std::{fs, path::Path};

pub fn time() {
    let now = util::output("date", &["+%A %H:%M  ·  %d %B %Y  ·  Week %V"]).unwrap_or_default();
    util::notify("\u{f017}", now.trim(), "");
}

/// A battery's state from /sys/class/power_supply (µW, µWh, µA, µAh, µV).
#[derive(Default, Debug, PartialEq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
    /// Watts drawn or charged.
    pub watts: f64,
    /// Energy now and when full, in watt-hours.
    pub energy: f64,
    pub full: f64,
}

fn read(dir: &Path, name: &str) -> Option<f64> {
    fs::read_to_string(dir.join(name)).ok()?.trim().parse().ok()
}

pub fn battery_from(dir: &Path) -> Option<Battery> {
    let percent = read(dir, "capacity")? as u32;
    let status = fs::read_to_string(dir.join("status")).unwrap_or_default();
    let volts = read(dir, "voltage_now").map(|v| v / 1e6);
    let watts = read(dir, "power_now")
        .map(|p| p / 1e6)
        .or_else(|| Some(read(dir, "current_now")? / 1e6 * volts?))
        .unwrap_or(0.0)
        .abs();
    let energy_of = |energy: &str, charge: &str| {
        read(dir, energy)
            .map(|e| e / 1e6)
            .or_else(|| Some(read(dir, charge)? / 1e6 * volts?))
            .unwrap_or(0.0)
    };
    Some(Battery {
        percent,
        charging: status.trim() == "Charging",
        watts,
        energy: energy_of("energy_now", "charge_now"),
        full: energy_of("energy_full", "charge_full"),
    })
}

fn duration(hours: f64) -> String {
    let minutes = (hours * 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

fn watts(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").unwrap_or(&text).to_owned()
}

/// Omarchy's battery line: "Battery 80%  ·  3h 5m left  ·  7.5W / 57Wh".
pub fn battery_line(battery: &Battery) -> String {
    let mut parts = vec![format!("Battery {}%", battery.percent)];
    if battery.watts > 0.2 {
        if battery.charging {
            parts.push(format!(
                "{} to full",
                duration((battery.full - battery.energy).max(0.0) / battery.watts)
            ));
        } else {
            parts.push(format!("{} left", duration(battery.energy / battery.watts)));
        }
    }
    parts.push(format!(
        "{}W / {}Wh",
        watts(battery.watts),
        battery.full.round()
    ));
    parts.join("  ·  ")
}

pub fn battery() {
    let found = fs::read_dir("/sys/class/power_supply")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            fs::read_to_string(entry.path().join("type")).is_ok_and(|t| t.trim() == "Battery")
        })
        .find_map(|entry| battery_from(&entry.path()));
    match found {
        Some(battery) => util::notify("\u{f0079}", &battery_line(&battery), ""),
        None => util::notify("\u{f0079}", "No battery found", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn battery_lines_read_like_tatami() {
        let discharging = Battery {
            percent: 80,
            charging: false,
            watts: 7.5,
            energy: 23.125,
            full: 57.0,
        };
        assert_eq!(
            battery_line(&discharging),
            "Battery 80%  ·  3h 5m left  ·  7.5W / 57Wh"
        );
        let charging = Battery {
            percent: 50,
            charging: true,
            watts: 30.0,
            energy: 28.5,
            full: 57.0,
        };
        assert_eq!(
            battery_line(&charging),
            "Battery 50%  ·  57m to full  ·  30W / 57Wh"
        );
        let idle = Battery {
            percent: 100,
            ..Default::default()
        };
        assert_eq!(battery_line(&idle), "Battery 100%  ·  0W / 0Wh");
    }
    #[test]
    fn battery_reads_charge_counters_too() {
        let dir = std::env::temp_dir().join(format!("tatami-bat-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for (name, value) in [
            ("capacity", "42"),
            ("status", "Discharging"),
            ("voltage_now", "12000000"),
            ("current_now", "500000"),
            ("charge_now", "2000000"),
            ("charge_full", "4000000"),
        ] {
            fs::write(dir.join(name), value).unwrap();
        }
        let battery = battery_from(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(battery.percent, 42);
        assert!(!battery.charging);
        assert!((battery.watts - 6.0).abs() < 1e-9);
        assert!((battery.energy - 24.0).abs() < 1e-9 && (battery.full - 48.0).abs() < 1e-9);
    }
}
