//! Power meters, for an activity monitor's energy figures: the processor
//! package's energy counters (RAPL, `/sys/class/powercap/intel-rapl:N`,
//! which AMD's processors use too) and the battery's draw while it
//! discharges.
//!
//! Linux measures power per device, not per process. RAPL's counters are
//! readable only by root on current kernels (they can leak secrets when
//! sampled quickly, CVE-2020-8694): for anyone else [`rapl_counters`]
//! returns `None`, and [`meter_counters`] asks Aizome's power meter
//! (`aizome-power-meter`, root's, started by its socket) for them instead.
//! [`split`] shares a meter's power between processes by their use of the
//! device.

use super::{read_attr, read_u64};
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where Aizome's power meter answers.
pub const METER_SOCKET: &str = "/run/aizome/power.sock";

/// One RAPL package domain's counter, in microjoules, and where it wraps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnergyCounter {
    pub name: String,
    pub microjoules: u64,
    pub range: u64,
}

/// The package domains' counters as read at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaplReading {
    /// When they were read, in microseconds of `CLOCK_BOOTTIME` (see
    /// [`boottime_us`]).
    pub at: u64,
    pub counters: Vec<EnergyCounter>,
}

/// Now, in microseconds of `CLOCK_BOOTTIME`. Unlike `Instant`
/// (`CLOCK_MONOTONIC`), it counts on while the computer sleeps, as RAPL's
/// counters do, and every process shares it, so Aizome's power meter can
/// say when it read them.
pub fn boottime_us() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a valid timespec to write into.
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec as u64 * 1_000_000 + ts.tv_nsec as u64 / 1_000
}

/// The package domains under `/sys/class/powercap`: `intel-rapl:0`, not
/// their sub-domains, and only those named `package…`. A laptop's platform
/// domain (`psys`, beside `package-0` on Intel's) measures the whole
/// computer, the package included, so it is no package's.
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
            n.starts_with("intel-rapl:")
                && n.matches(':').count() == 1
                && read_attr(e.path().join("name")).is_some_and(|name| name.starts_with("package"))
        })
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// The counters of `domains` read now, or `None` if any can't be read (not
/// root).
pub fn rapl_counters(domains: &[PathBuf]) -> Option<RaplReading> {
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
    let at = boottime_us();
    counters.filter(|c| !c.is_empty()).map(|counters| RaplReading { at, counters })
}

/// The package domains' counters from Aizome's power meter, or `None` if
/// it isn't there (not an Aizome system) or has none (a VM). It reads them
/// at most once a second, answering in between with its last reading.
pub fn meter_counters() -> Option<RaplReading> {
    meter_counters_at(Path::new(METER_SOCKET))
}

pub(crate) fn meter_counters_at(socket: &Path) -> Option<RaplReading> {
    let stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    let mut text = String::new();
    stream.take(4096).read_to_string(&mut text).ok()?;
    parse_counters(&text, boottime_us())
}

/// The power meter's answer: a "NAME ENERGY_UJ RANGE_UJ READ_US" line per
/// package domain, `READ_US` being when it read them (microseconds of
/// `CLOCK_BOOTTIME`), or `None` without a counter. An answer without
/// `READ_US`, as Tatami's meter gives, counts as read when it was
/// `received`.
pub fn parse_counters(text: &str, received: u64) -> Option<RaplReading> {
    let mut at = None;
    let counters: Vec<EnergyCounter> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let counter = EnergyCounter {
                name: fields.next()?.to_owned(),
                microjoules: fields.next()?.parse().ok()?,
                range: fields.next()?.parse().ok()?,
            };
            at = at.or_else(|| fields.next()?.parse().ok());
            Some(counter)
        })
        .collect();
    if counters.is_empty() { None } else { Some(RaplReading { at: at.unwrap_or(received), counters }) }
}

/// Whether an integrated card's power meter is the processor package's.
/// An AMD APU's Radeon reports the whole package's power (amdgpu's `PPT`),
/// its processor cores included: it is all one die. Ryzen 7000 and 9000
/// desktop processors (Raphael and Granite Ridge, and the laptop
/// processors made from them) put their Radeon on the I/O die beside
/// separate core dies, and its `PPT` measures the graphics alone: a tenth
/// of a watt with every core busy.
pub fn meters_package(gpu: &crate::sysinfo::Gpu, integrated: bool) -> bool {
    integrated
        && gpu.vendor_id == Some(0x1002)
        && gpu.driver.as_deref() == Some("amdgpu")
        && !matches!(gpu.device_id, Some(0x164e | 0x13c0))
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
        let before = boottime_us();
        let reading = rapl_counters(&domains).unwrap();
        assert_eq!(reading.counters[0].microjoules, 1_000_000);
        assert!(reading.at >= before, "read now");
        assert_eq!(battery_draw_at(&tree.path("supply")), (Some(8.5), true));
    }

    #[test]
    fn a_laptops_platform_domain_is_not_the_package() {
        // The X1 Carbon Gen 14's domains (the counters made up): package-0
        // with core and dram beneath it; psys, the whole computer's power,
        // the package's included; and the MMIO interface's package-0 and
        // dram again. The control types have no name or counter.
        let tree = TestTree::new("psys");
        tree.dir("powercap/intel-rapl")
            .file("powercap/intel-rapl:0/name", "package-0\n")
            .file("powercap/intel-rapl:0/energy_uj", "100\n")
            .file("powercap/intel-rapl:0:0/name", "core\n")
            .file("powercap/intel-rapl:0:0/energy_uj", "60\n")
            .file("powercap/intel-rapl:0:1/name", "dram\n")
            .file("powercap/intel-rapl:0:1/energy_uj", "10\n")
            .file("powercap/intel-rapl:1/name", "psys\n")
            .file("powercap/intel-rapl:1/energy_uj", "200\n")
            .dir("powercap/intel-rapl-mmio")
            .file("powercap/intel-rapl-mmio:0/name", "package-0\n")
            .file("powercap/intel-rapl-mmio:0/energy_uj", "100\n")
            .file("powercap/intel-rapl-mmio:0:0/name", "dram\n");
        let domains = rapl_domains_at(&tree.path("powercap"));
        assert_eq!(domains, [tree.path("powercap/intel-rapl:0")]);
        let reading = rapl_counters(&domains).unwrap();
        assert_eq!(reading.counters, [EnergyCounter { name: "package-0".into(), microjoules: 100, range: 0 }]);
    }

    #[test]
    fn the_power_meters_answer() {
        let reading =
            parse_counters("package-0 65482528387 65532610987 7250000\npackage-1 42 0 7250000\nnot a counter\n", 9);
        assert_eq!(
            reading,
            Some(RaplReading {
                at: 7_250_000,
                counters: vec![
                    EnergyCounter { name: "package-0".into(), microjoules: 65_482_528_387, range: 65_532_610_987 },
                    EnergyCounter { name: "package-1".into(), microjoules: 42, range: 0 },
                ],
            }),
            "read when the meter says"
        );
        assert_eq!(
            parse_counters("package-0 42 0\n", 9).map(|r| r.at),
            Some(9),
            "without saying when (Tatami's format): read when received"
        );
        assert_eq!(parse_counters("", 9), None, "a VM's meter has no counters");
        let tree = TestTree::new("meter");
        assert_eq!(meter_counters_at(&tree.path("power.sock")), None, "no meter here");
        // A meter that answers.
        let socket = tree.path("answering.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let meter = std::thread::spawn(move || {
            use std::io::Write;
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"package-0 123 456 789\n").unwrap();
        });
        let reading = meter_counters_at(&socket).unwrap();
        meter.join().unwrap();
        assert_eq!((reading.at, reading.counters[0].microjoules), (789, 123));
    }

    #[test]
    fn only_an_apus_radeon_meters_the_package() {
        let card = |vendor, device, driver: &str| crate::sysinfo::Gpu {
            card: "card1".into(),
            name: String::new(),
            vendor_id: Some(vendor),
            device_id: Some(device),
            vendor_name: None,
            device_name: None,
            driver: Some(driver.into()),
            vram: None,
            boot_vga: false,
            bus_address: None,
        };
        let radeon = |device| card(0x1002, device, "amdgpu");
        // Phoenix and Strix Point: one die.
        assert!(meters_package(&radeon(0x15bf), true));
        assert!(meters_package(&radeon(0x150e), true));
        // Raphael's and Granite Ridge's Radeon, beside the core dies.
        assert!(!meters_package(&radeon(0x164e), true));
        assert!(!meters_package(&radeon(0x13c0), true));
        assert!(!meters_package(&radeon(0x744c), false), "a discrete card meters itself");
        assert!(!meters_package(&card(0x8086, 0x7d55, "xe"), true));
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
