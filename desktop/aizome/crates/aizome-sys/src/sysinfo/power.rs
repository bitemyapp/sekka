//! Power meters, for an activity monitor's energy figures: the processor
//! package's energy counters (RAPL, `/sys/class/powercap/intel-rapl:N`,
//! which AMD's processors use too) and the battery's draw while it
//! discharges.
//!
//! Linux measures power per device, not per process. RAPL's counters are
//! readable only by root on current kernels (they can leak secrets when
//! sampled quickly, CVE-2020-8694); without a privileged helper,
//! [`rapl_counters`] returns `None` and only the battery is measured, while
//! it discharges. [`split`] shares a meter's power between processes by
//! their use of the device.

use super::{read_attr, read_u64};
use std::path::{Path, PathBuf};

/// One RAPL package domain's counter, in microjoules, and where it wraps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnergyCounter {
    pub name: String,
    pub microjoules: u64,
    pub range: u64,
}

/// The package domains under `/sys/class/powercap` (`intel-rapl:0`, not
/// their sub-domains).
pub fn rapl_domains() -> Vec<PathBuf> {
    rapl_domains_at(Path::new("/sys/class/powercap"))
}

pub(crate) fn rapl_domains_at(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("intel-rapl:") && n.matches(':').count() == 1
        })
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// The counters of `domains`, or `None` if any can't be read (not root).
pub fn rapl_counters(domains: &[PathBuf]) -> Option<Vec<EnergyCounter>> {
    let counters: Option<Vec<EnergyCounter>> = domains
        .iter()
        .map(|d| {
            Some(EnergyCounter {
                name: read_attr(d.join("name")).unwrap_or_default(),
                microjoules: read_u64(d.join("energy_uj"))?,
                range: read_u64(d.join("max_energy_range_uj")).unwrap_or(0),
            })
        })
        .collect();
    counters.filter(|c| !c.is_empty())
}

/// Joules between two readings of a counter that wraps at `range`.
pub fn joules(before: &EnergyCounter, after: &EnergyCounter) -> f64 {
    let delta = if after.microjoules >= before.microjoules {
        after.microjoules - before.microjoules
    } else if after.range > 0 {
        after.range - before.microjoules + after.microjoules
    } else {
        0
    };
    delta as f64 / 1e6
}

/// What the batteries give now, in watts, while they discharge (`None`
/// when none does, or none reports it), and whether any discharges.
pub fn battery_draw() -> (Option<f64>, bool) {
    battery_draw_at(Path::new("/sys/class/power_supply"))
}

pub(crate) fn battery_draw_at(root: &Path) -> (Option<f64>, bool) {
    let mut draw: Option<f64> = None;
    let mut discharging = false;
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let p = e.path();
        if read_attr(p.join("type")).as_deref() != Some("Battery")
            || read_attr(p.join("status")).as_deref() != Some("Discharging")
        {
            continue;
        }
        discharging = true;
        let at = |n: &str| read_u64(p.join(n));
        let watts = at("power_now")
            .map(|uw| uw as f64 / 1e6)
            .or_else(|| Some(at("current_now")? as f64 * at("voltage_now")? as f64 / 1e12));
        if let Some(w) = watts {
            *draw.get_or_insert(0.0) += w;
        }
    }
    (draw, discharging)
}

/// The lowest power a meter has shown: its idle floor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Floor(Option<f64>);

impl Floor {
    pub fn update(&mut self, watts: f64) -> f64 {
        let floor = self.0.map_or(watts, |f| f.min(watts)).max(0.0);
        self.0 = Some(floor);
        floor
    }
}

/// Split a meter's `power` between users of its device: each is held to
/// use the power above the idle `floor` in proportion to its share of the
/// busy time, plus the floor in proportion to its share of the device's
/// capacity (`shares` are fractions of capacity, 0 to 1). What is left is
/// the device idling.
pub fn split(power: f64, floor: f64, shares: &[f64]) -> Vec<f64> {
    let power = power.max(0.0);
    let floor = floor.clamp(0.0, power);
    let dynamic = power - floor;
    let used: f64 = shares.iter().map(|s| s.max(0.0)).sum();
    shares
        .iter()
        .map(|s| {
            let s = s.max(0.0);
            if used <= 0.0 { 0.0 } else { dynamic * s / used + floor * s / used.max(1.0) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    #[test]
    fn counters_wrap() {
        let before = EnergyCounter { name: "package-0".into(), microjoules: 262_143_000_000, range: 262_143_328_850 };
        let after = EnergyCounter { microjoules: 671_150, ..before.clone() };
        assert!((joules(&before, &after) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn domains_and_batteries() {
        let tree = TestTree::new("power");
        tree.file("powercap/intel-rapl:0/name", "package-0\n")
            .file("powercap/intel-rapl:0/energy_uj", "1000000\n")
            .dir("powercap/intel-rapl:0:0")
            .file("supply/BAT0/type", "Battery\n")
            .file("supply/BAT0/status", "Discharging\n")
            .file("supply/BAT0/power_now", "8500000\n")
            .file("supply/AC/type", "Mains\n");
        let domains = rapl_domains_at(&tree.path("powercap"));
        assert_eq!(domains.len(), 1);
        assert_eq!(rapl_counters(&domains).unwrap()[0].microjoules, 1_000_000);
        assert_eq!(battery_draw_at(&tree.path("supply")), (Some(8.5), true));
    }

    #[test]
    fn power_is_shared_by_use() {
        let parts = split(30.0, 10.0, &[0.5, 0.1]);
        assert!((parts[0] - (20.0 * 0.5 / 0.6 + 10.0 * 0.5)).abs() < 1e-9);
        assert_eq!(split(30.0, 10.0, &[0.0, 0.0]), vec![0.0, 0.0]);
        assert!(split(30.0, 10.0, &[0.9, 0.9]).iter().sum::<f64>() <= 30.0 + 1e-9);
        let mut f = Floor::default();
        assert_eq!((f.update(20.0), f.update(25.0), f.update(12.0)), (20.0, 20.0, 12.0));
    }
}
