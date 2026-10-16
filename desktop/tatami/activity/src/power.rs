// SPDX-License-Identifier: MIT OR Apache-2.0
//! Power: the meters there are, and each process's estimated share.
//!
//! Linux measures power per device, not per process. The processor
//! package's energy counter (RAPL, /sys/class/powercap) is readable only by
//! root, since it can leak secrets when sampled quickly (CVE-2020-8694), so
//! Tatami's power meter service (`tatami power-meter`) reads it for the
//! window at most once a second. Graphics cards have their own meters (NVML,
//! hwmon), and a laptop's battery reports what the whole computer draws.
//!
//! A process is held to use the part of a meter's power above that
//! device's idle floor in proportion to its share of the device's busy
//! time, plus the idle floor in proportion to the share of the device's
//! capacity it used. What is left is the device idling. The floor is the
//! lowest power the meter has shown while the window has been open.
use crate::procfs;
use std::{
    io::Read,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

/// Where Tatami's power meter service listens.
pub const SOCKET: &str = "/run/tatami/power.sock";

/// One RAPL package domain's counter, in microjoules, and where it wraps.
#[derive(Clone, Debug, PartialEq)]
pub struct Counter {
    pub name: String,
    pub energy: u64,
    pub range: u64,
}

/// The service's answer: one "name energy range" line per package domain.
pub fn parse_counters(text: &str) -> Vec<Counter> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(Counter {
                name: fields.next()?.to_owned(),
                energy: fields.next()?.parse().ok()?,
                range: fields.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// The package domains under /sys/class/powercap: intel-rapl:N, which
/// AMD's processors use too.
pub fn rapl_domains() -> Vec<PathBuf> {
    let mut domains: Vec<PathBuf> = std::fs::read_dir("/sys/class/powercap")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("intel-rapl:") && name.matches(':').count() == 1
        })
        .map(|entry| entry.path())
        .collect();
    domains.sort();
    domains
}

pub fn read_direct(domains: &[PathBuf]) -> Option<Vec<Counter>> {
    let counters: Option<Vec<Counter>> = domains
        .iter()
        .map(|domain| {
            Some(Counter {
                name: procfs::read(domain.join("name"))
                    .map(|n| n.trim().to_owned())
                    .unwrap_or_default(),
                energy: procfs::number(domain.join("energy_uj"))?,
                range: procfs::number(domain.join("max_energy_range_uj")).unwrap_or(0),
            })
        })
        .collect();
    counters.filter(|counters| !counters.is_empty())
}

pub fn read_service() -> Option<Vec<Counter>> {
    let stream = UnixStream::connect(SOCKET).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok()?;
    let mut text = String::new();
    stream.take(4096).read_to_string(&mut text).ok()?;
    let counters = parse_counters(&text);
    (!counters.is_empty()).then_some(counters)
}

/// Joules between two readings of a counter that wraps at `range`.
pub fn joules(before: &Counter, after: &Counter) -> f64 {
    let delta = if after.energy >= before.energy {
        after.energy - before.energy
    } else if after.range > 0 {
        after.range - before.energy + after.energy
    } else {
        0
    };
    delta as f64 / 1e6
}

/// Watts the battery gives now, while it discharges.
pub fn battery() -> (Option<f64>, bool) {
    let mut draw: Option<f64> = None;
    let mut discharging = false;
    for entry in std::fs::read_dir("/sys/class/power_supply")
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let kind = procfs::read(path.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        let status = procfs::read(path.join("status")).unwrap_or_default();
        if status.trim() != "Discharging" {
            continue;
        }
        discharging = true;
        let at = |name: &str| procfs::number(Path::new(&path).join(name));
        let watts = at("power_now").map(|uw| uw as f64 / 1e6).or_else(|| {
            let (ua, uv) = (at("current_now")?, at("voltage_now")?);
            Some(ua as f64 * uv as f64 / 1e12)
        });
        if let Some(watts) = watts {
            *draw.get_or_insert(0.0) += watts;
        }
    }
    (draw, discharging)
}

/// The lowest power a meter has shown: its idle floor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Floor(Option<f64>);

impl Floor {
    pub fn update(&mut self, watts: f64) -> f64 {
        let floor = self.0.map_or(watts, |floor| floor.min(watts)).max(0.0);
        self.0 = Some(floor);
        floor
    }
}

/// Split a meter's power between users of its device. `shares` are each
/// one's fraction of the device's capacity used over the interval (0 to 1).
pub fn split(power: f64, floor: f64, shares: &[f64]) -> Vec<f64> {
    let power = power.max(0.0);
    let floor = floor.clamp(0.0, power);
    let dynamic = power - floor;
    let used: f64 = shares.iter().map(|share| share.max(0.0)).sum();
    shares
        .iter()
        .map(|share| {
            let share = share.max(0.0);
            if used <= 0.0 {
                0.0
            } else {
                dynamic * share / used + floor * share / used.max(1.0)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_lines_and_wrapping_counters() {
        let counters = parse_counters("package-0 123456789 262143328850\nbad line\n");
        assert_eq!(counters.len(), 1);
        assert_eq!(counters[0].name, "package-0");
        let before = Counter {
            name: "package-0".into(),
            energy: 262_143_000_000,
            range: 262_143_328_850,
        };
        let after = Counter {
            energy: 671_150,
            ..before.clone()
        };
        assert!((joules(&before, &after) - 1.0).abs() < 1e-6);
        let later = Counter {
            energy: 2_671_150,
            ..after.clone()
        };
        assert!((joules(&after, &later) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn power_is_shared_by_use() {
        // 30 W measured with a 10 W floor; one process used half the
        // processor and another a tenth.
        let parts = split(30.0, 10.0, &[0.5, 0.1]);
        assert!((parts[0] - (20.0 * 0.5 / 0.6 + 10.0 * 0.5)).abs() < 1e-9);
        assert!((parts[1] - (20.0 * 0.1 / 0.6 + 10.0 * 0.1)).abs() < 1e-9);
        // The dynamic part is fully shared; the floor's unused part is
        // idling.
        let total: f64 = parts.iter().sum();
        assert!((total - (20.0 + 10.0 * 0.6)).abs() < 1e-9);
        // Nothing used: nothing attributed.
        assert_eq!(split(30.0, 10.0, &[0.0, 0.0]), vec![0.0, 0.0]);
        // Overlapping shares never attribute more than was measured.
        let crowded: f64 = split(30.0, 10.0, &[0.9, 0.9]).iter().sum();
        assert!(crowded <= 30.0 + 1e-9);
        let mut floor = Floor::default();
        assert_eq!(floor.update(20.0), 20.0);
        assert_eq!(floor.update(25.0), 20.0);
        assert_eq!(floor.update(12.0), 12.0);
    }
}
