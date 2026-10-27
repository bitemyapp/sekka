//! Temperatures from hwmon: `/sys/class/hwmon/hwmon*`.
//!
//! Each hwmon directory is one chip (`name`: `coretemp`, `k10temp`,
//! `nvme`, `acpitz`, `amdgpu`, `iwlwifi_1`…) with sensors `tempN_input` in
//! millidegrees Celsius, and optionally `tempN_label`, `tempN_max` and
//! `tempN_crit`. A sensor whose input can't be read (thinkpad_acpi's
//! missing GPU sensor answers `ENXIO`) is left out, as is one reading
//! exactly 0, which drivers such as thinkpad_acpi report for slots with no
//! sensor; so are readings outside −60…250 °C. Limits outside 0…250 °C
//! (nvme reports 65261.85 °C for "none") are dropped.
//!
//! [`pick_cpu_temperature`] chooses the one sensor to show as "the CPU
//! temperature".

use super::read_attr;
use std::path::Path;

const HWMON: &str = "/sys/class/hwmon";

/// One temperature sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct Temperature {
    /// The chip's driver name (`coretemp`, `k10temp`, `nvme`, `acpitz`).
    pub chip: String,
    /// The hwmon directory, such as `hwmon6`, which tells apart chips with
    /// the same name.
    pub hwmon: String,
    /// The device the chip belongs to (the name of the hwmon's `device`
    /// link, such as `nvme0` or `coretemp.0`), when it has one.
    pub device: Option<String>,
    /// The sensor's number: `N` in `tempN_input`.
    pub index: u32,
    /// The driver's label (`Package id 0`, `Core 4`, `Tctl`, `Composite`),
    /// if it gives one.
    pub label: Option<String>,
    /// The temperature now, in °C.
    pub celsius: f32,
    /// The temperature the driver calls high, in °C.
    pub max: Option<f32>,
    /// The temperature the driver calls critical, in °C.
    pub critical: Option<f32>,
}

impl Temperature {
    /// A name for the UI: "coretemp Package id 0", "acpitz temp1".
    pub fn display_name(&self) -> String {
        match &self.label {
            Some(label) => format!("{} {label}", self.chip),
            None => format!("{} temp{}", self.chip, self.index),
        }
    }
}

/// Every readable temperature sensor, by hwmon number and then sensor
/// number.
pub fn temperatures() -> Vec<Temperature> {
    temperatures_in(Path::new(HWMON))
}

/// The CPU temperature: the sensor [`pick_cpu_temperature`] picks from
/// [`temperatures`].
pub fn cpu_temperature() -> Option<Temperature> {
    pick_cpu_temperature(&temperatures()).cloned()
}

/// Which sensor to show as "the CPU temperature", in order of preference:
/// Intel's `coretemp` package sensor (`Package id 0`); AMD's `k10temp` or
/// `zenpower` die sensor (`Tdie`, else `Tctl`, which some CPUs offset);
/// the `cpu_thermal` or `soc_thermal` zone of ARM boards; and as a last
/// resort the ACPI thermal zone (`acpitz`), which on many laptops follows
/// the CPU only loosely.
pub fn pick_cpu_temperature(sensors: &[Temperature]) -> Option<&Temperature> {
    let labelled = |chip: &'static str, label: &'static str| {
        move |t: &&Temperature| t.chip == chip && t.label.as_deref() == Some(label)
    };
    let chip = |chip: &'static str| move |t: &&Temperature| t.chip == chip;
    let coretemp_package =
        |t: &&Temperature| t.chip == "coretemp" && t.label.as_deref().is_some_and(|l| l.starts_with("Package id"));
    sensors
        .iter()
        .find(labelled("coretemp", "Package id 0"))
        .or_else(|| sensors.iter().find(coretemp_package))
        .or_else(|| sensors.iter().find(labelled("k10temp", "Tdie")))
        .or_else(|| sensors.iter().find(labelled("k10temp", "Tctl")))
        .or_else(|| sensors.iter().find(labelled("zenpower", "Tdie")))
        .or_else(|| sensors.iter().find(labelled("zenpower", "Tctl")))
        .or_else(|| sensors.iter().find(chip("k10temp")))
        .or_else(|| sensors.iter().find(chip("cpu_thermal")))
        .or_else(|| sensors.iter().find(chip("soc_thermal")))
        .or_else(|| sensors.iter().find(chip("acpitz")))
}

/// Read every hwmon chip under `root` (normally `/sys/class/hwmon`).
pub(crate) fn temperatures_in(root: &Path) -> Vec<Temperature> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut chips: Vec<(u32, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let number = name.strip_prefix("hwmon")?.parse().ok()?;
            Some((number, name))
        })
        .collect();
    chips.sort();
    let mut sensors = Vec::new();
    for (_, hwmon) in chips {
        sensors.extend(chip_temperatures(&root.join(&hwmon), &hwmon));
    }
    sensors
}

/// The temperature sensors of one hwmon directory.
fn chip_temperatures(dir: &Path, hwmon: &str) -> Vec<Temperature> {
    // Old drivers kept the name on the device instead.
    let Some(chip) = read_attr(dir.join("name")).or_else(|| read_attr(dir.join("device/name"))) else {
        return Vec::new();
    };
    let device = std::fs::read_link(dir.join("device"))
        .ok()
        .and_then(|target| Some(target.file_name()?.to_string_lossy().into_owned()));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut indexes: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.strip_prefix("temp")?.strip_suffix("_input")?.parse().ok()
        })
        .collect();
    indexes.sort_unstable();

    let mut sensors = Vec::new();
    for index in indexes {
        let attribute = |what: &str| read_attr(dir.join(format!("temp{index}_{what}")));
        let Some(celsius) = attribute("input").and_then(|value| millidegrees(&value)) else {
            continue;
        };
        if celsius == 0.0 || !(-60.0..=250.0).contains(&celsius) {
            continue;
        }
        let limit =
            |what: &str| attribute(what).and_then(|value| millidegrees(&value)).filter(|c| *c > 0.0 && *c <= 250.0);
        sensors.push(Temperature {
            chip: chip.clone(),
            hwmon: hwmon.to_owned(),
            device: device.clone(),
            index,
            label: attribute("label"),
            celsius,
            max: limit("max"),
            critical: limit("crit"),
        });
    }
    sensors
}

/// A hwmon temperature (millidegrees Celsius) in °C.
fn millidegrees(text: &str) -> Option<f32> {
    let milli: i64 = text.trim().parse().ok()?;
    Some(milli as f32 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysinfo::TestTree;

    /// This machine's hwmon tree, trimmed: an nvme drive, a fan, the ACPI
    /// zone, coretemp and thinkpad_acpi (whose GPU sensor can't be read and
    /// whose empty slots read 0).
    fn fake_hwmon() -> TestTree {
        let tree = TestTree::new("hwmon");
        tree.file("hwmon0/name", "nvme\n")
            .link("hwmon0/device", "../../nvme0")
            .file("hwmon0/temp1_input", "42850\n")
            .file("hwmon0/temp1_label", "Composite\n")
            .file("hwmon0/temp1_max", "82850\n")
            .file("hwmon0/temp1_crit", "84850\n")
            .file("hwmon0/temp2_input", "45850\n")
            .file("hwmon0/temp2_label", "Sensor 1\n")
            .file("hwmon0/temp2_max", "65261850\n")
            .file("hwmon2/name", "acpi_fan\n")
            .file("hwmon2/fan1_input", "0\n")
            .file("hwmon4/name", "acpitz\n")
            .file("hwmon4/temp1_input", "38000\n")
            .file("hwmon6/name", "coretemp\n")
            .file("hwmon6/temp1_input", "70000\n")
            .file("hwmon6/temp1_label", "Package id 0\n")
            .file("hwmon6/temp1_max", "100000\n")
            .file("hwmon6/temp1_crit", "100000\n")
            .file("hwmon6/temp10_input", "50000\n")
            .file("hwmon6/temp10_label", "Core 8\n")
            .file("hwmon6/temp2_input", "36000\n")
            .file("hwmon6/temp2_label", "Core 0\n")
            .file("hwmon7/name", "thinkpad\n")
            .file("hwmon7/temp1_input", "38000\n")
            .file("hwmon7/temp1_label", "CPU\n")
            .file("hwmon7/temp2_label", "GPU\n")
            .dir("hwmon7/temp2_input")
            .file("hwmon7/temp3_input", "0\n")
            .file("hwmon10/name", "iwlwifi_1\n")
            .file("hwmon10/temp1_input", "46000\n")
            .file("hwmon11/temp1_input", "46000\n")
            .file("not-hwmon/name", "x\n");
        tree
    }

    #[test]
    fn hwmon_sensors_are_read_in_order_and_junk_dropped() {
        let tree = fake_hwmon();
        let sensors = temperatures_in(&tree.0);
        let names: Vec<String> = sensors.iter().map(Temperature::display_name).collect();
        assert_eq!(
            names,
            [
                "nvme Composite",
                "nvme Sensor 1",
                "acpitz temp1",
                "coretemp Package id 0",
                "coretemp Core 0",
                "coretemp Core 8",
                "thinkpad CPU",
                "iwlwifi_1 temp1",
            ]
        );
        let composite = &sensors[0];
        assert_eq!(composite.celsius, 42.85);
        assert_eq!(composite.max, Some(82.85));
        assert_eq!(composite.critical, Some(84.85));
        assert_eq!(composite.device.as_deref(), Some("nvme0"));
        assert_eq!(composite.hwmon, "hwmon0");
        assert_eq!(sensors[1].max, None, "65261.85 °C is not a limit");
        assert_eq!(sensors[5].index, 10);
    }

    #[test]
    fn the_cpu_temperature_prefers_the_package_sensor() {
        let tree = fake_hwmon();
        let sensors = temperatures_in(&tree.0);
        let cpu = pick_cpu_temperature(&sensors).unwrap();
        assert_eq!(cpu.chip, "coretemp");
        assert_eq!(cpu.celsius, 70.0);

        let sensor = |chip: &str, label: Option<&str>, celsius: f32| Temperature {
            chip: chip.into(),
            hwmon: "hwmon0".into(),
            device: None,
            index: 1,
            label: label.map(Into::into),
            celsius,
            max: None,
            critical: None,
        };
        let amd = [
            sensor("acpitz", None, 40.0),
            sensor("k10temp", Some("Tccd1"), 55.0),
            sensor("k10temp", Some("Tctl"), 60.0),
        ];
        assert_eq!(pick_cpu_temperature(&amd).unwrap().celsius, 60.0);
        let pi = [sensor("rp1_adc", None, 30.0), sensor("cpu_thermal", None, 48.0)];
        assert_eq!(pick_cpu_temperature(&pi).unwrap().celsius, 48.0);
        let fallback = [sensor("nvme", Some("Composite"), 30.0), sensor("acpitz", None, 41.0)];
        assert_eq!(pick_cpu_temperature(&fallback).unwrap().celsius, 41.0);
        assert_eq!(pick_cpu_temperature(&[sensor("nvme", None, 30.0)]), None);
    }

    #[test]
    fn millidegrees_parse() {
        assert_eq!(millidegrees("42850\n"), Some(42.85));
        assert_eq!(millidegrees("-5000"), Some(-5.0));
        assert_eq!(millidegrees(""), None);
        assert_eq!(millidegrees("hot"), None);
    }

    #[test]
    fn a_missing_hwmon_class_gives_nothing() {
        assert!(temperatures_in(Path::new("/nonexistent/hwmon")).is_empty());
    }

    #[test]
    #[ignore = "reads this machine"]
    fn live_temperatures_are_sane() {
        let sensors = temperatures();
        for t in &sensors {
            println!(
                "{:<8} {:<28} {:>6.1} °C  max {:?} crit {:?} device {:?}",
                t.hwmon,
                t.display_name(),
                t.celsius,
                t.max,
                t.critical,
                t.device
            );
        }
        let cpu = cpu_temperature();
        println!("CPU: {cpu:?}");
        assert!(sensors.iter().all(|t| (-60.0..=250.0).contains(&t.celsius)));
        if let Some(cpu) = cpu {
            assert!((10.0..=120.0).contains(&cpu.celsius));
        }
    }
}
