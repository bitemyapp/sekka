//! A drive's SMART attributes, as people read them: ATA drives' numbered
//! attributes (normalised value, worst, threshold, and the raw value in its
//! unit) and NVMe controllers' health log (spare, wear, data written,
//! temperatures, errors).

use crate::bus::{value_i64, value_u64};
use std::collections::HashMap;
use zbus::zvariant::{OwnedValue, Value};

/// One SMART attribute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SmartAttribute {
    /// The ATA attribute's number (NVMe's log has none).
    pub id: Option<u8>,
    /// "Reallocated sector count", "Available spare".
    pub name: String,
    /// The value for people: "12 sectors", "41 °C", "4.5 TB", "100 %".
    pub value: String,
    /// ATA's normalised value, worst and threshold (higher is better; at
    /// or under the threshold, the attribute fails).
    pub normalized: Option<i32>,
    pub worst: Option<i32>,
    pub threshold: Option<i32>,
    /// Failing now (under its threshold, or past NVMe's spare threshold).
    pub failing: bool,
    /// Failing predicts the drive's failure (ATA's pre-fail flag), rather
    /// than only its age.
    pub pre_fail: bool,
}

/// What `org.freedesktop.UDisks2.Drive.Ata.SmartGetAttributes` returns for
/// each attribute: id, name, flags, value, worst, threshold, pretty value,
/// its unit, and an expansion dictionary.
pub(crate) type AtaRow = (u8, String, u16, i32, i32, i32, i64, i32, HashMap<String, OwnedValue>);

/// ATA attributes, in the drive's order.
pub(crate) fn ata(rows: Vec<AtaRow>) -> Vec<SmartAttribute> {
    rows.into_iter()
        .map(|(id, name, flags, value, worst, threshold, pretty, unit, _)| {
            let valid = |n: i32| (0..=255).contains(&n).then_some(n);
            let (value_n, worst_n, threshold_n) = (valid(value), valid(worst), valid(threshold));
            SmartAttribute {
                id: Some(id),
                name: ata_name(&name),
                value: ata_pretty(pretty, unit),
                normalized: value_n,
                worst: worst_n,
                threshold: threshold_n,
                failing: matches!((value_n, threshold_n), (Some(v), Some(t)) if t > 0 && v <= t),
                pre_fail: flags & 0x0001 != 0,
            }
        })
        .collect()
}

/// "reallocated-sector-count" → "Reallocated sector count".
fn ata_name(name: &str) -> String {
    let spaced = name.replace(['-', '_'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Unknown attribute".into(),
    }
}

/// libatasmart's pretty value in its unit: 0 unknown, 1 none, 2
/// milliseconds, 3 sectors, 4 millikelvin.
fn ata_pretty(pretty: i64, unit: i32) -> String {
    match unit {
        2 => {
            let hours = pretty / 3_600_000;
            if hours >= 48 {
                format!("{} days", hours / 24)
            } else if hours >= 1 {
                format!("{hours} h")
            } else {
                format!("{} min", pretty / 60_000)
            }
        }
        3 => format!("{pretty} sectors"),
        4 => format!("{:.0} °C", pretty as f64 / 1000.0 - 273.15),
        _ => pretty.to_string(),
    }
}

/// NVMe's health log (`org.freedesktop.UDisks2.NVMe.Controller.SmartGetAttributes`),
/// in a fixed, readable order.
pub(crate) fn nvme(log: &HashMap<String, OwnedValue>) -> Vec<SmartAttribute> {
    let get = |k: &str| log.get(k).and_then(|v| value_u64(v));
    let kelvin = |k: u64| format!("{:.0} °C", k as f64 - 273.15);
    let minutes = |m: u64| if m >= 120 { format!("{} h", m / 60) } else { format!("{m} min") };
    let mut out = Vec::new();
    let mut push = |name: &str, value: String, failing: bool| {
        out.push(SmartAttribute { name: name.into(), value, failing, ..Default::default() });
    };
    let spare = get("avail_spare");
    let threshold = get("spare_thresh");
    if let Some(s) = spare {
        let failing = threshold.is_some_and(|t| t > 0 && s < t);
        let value = match threshold {
            Some(t) => format!("{s} % (threshold {t} %)"),
            None => format!("{s} %"),
        };
        push("Available spare", value, failing);
    }
    if let Some(p) = get("percent_used") {
        push("Life used", format!("{p} %"), p >= 100);
    }
    if let Some(b) = get("total_data_written") {
        push("Data written", super::format_size(b), false);
    }
    if let Some(b) = get("total_data_read") {
        push("Data read", super::format_size(b), false);
    }
    if let Some(sensors) = log.get("temp_sensors") {
        let readings: Vec<u64> = match &**sensors {
            Value::Array(a) => a.iter().filter_map(value_u64).filter(|&k| k > 0).collect(),
            _ => Vec::new(),
        };
        for (i, k) in readings.iter().enumerate() {
            push(&format!("Temperature sensor {}", i + 1), kelvin(*k), false);
        }
    }
    if let Some(k) = get("wctemp").filter(|&k| k > 0) {
        push("Warning temperature", kelvin(k), false);
    }
    if let Some(k) = get("cctemp").filter(|&k| k > 0) {
        push("Critical temperature", kelvin(k), false);
    }
    if let Some(m) = get("warning_temp_time") {
        push("Time over the warning temperature", minutes(m), false);
    }
    if let Some(m) = get("critical_temp_time") {
        push("Time over the critical temperature", minutes(m), m > 0);
    }
    if let Some(m) = get("ctrl_busy_time") {
        push("Controller busy", minutes(m), false);
    }
    if let Some(n) = get("power_cycles") {
        push("Power cycles", n.to_string(), false);
    }
    if let Some(n) = get("unsafe_shutdowns") {
        push("Unsafe shutdowns", n.to_string(), false);
    }
    if let Some(n) = log.get("media_errors").and_then(|v| value_i64(v)) {
        push("Media and data integrity errors", n.to_string(), n > 0);
    }
    if let Some(n) = get("num_err_log_entries") {
        push("Error log entries", n.to_string(), false);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::owned;

    #[test]
    fn ata_attributes_read_plainly() {
        let rows = vec![
            (5u8, "reallocated-sector-count".to_owned(), 0x0003u16, 100, 100, 10, 12i64, 3, HashMap::new()),
            (194, "temperature-celsius-2".to_owned(), 0x0022, 64, 50, 0, 314_150, 4, HashMap::new()),
            (9, "power-on-hours".to_owned(), 0x0032, 90, 90, 0, 3_600_000 * 50, 2, HashMap::new()),
            (184, "end-to-end-error".to_owned(), 0x0033, 5, 5, 97, 3, 1, HashMap::new()),
        ];
        let a = ata(rows);
        assert_eq!(a[0].name, "Reallocated sector count");
        assert_eq!(a[0].value, "12 sectors");
        assert!(a[0].pre_fail && !a[0].failing);
        assert_eq!(a[1].value, "41 °C");
        assert_eq!(a[2].value, "2 days");
        assert!(a[3].failing, "under its threshold");
    }

    #[test]
    fn nvme_log_reads_plainly() {
        let mut log: HashMap<String, OwnedValue> = HashMap::new();
        log.insert("avail_spare".into(), owned(100u8));
        log.insert("spare_thresh".into(), owned(10u8));
        log.insert("percent_used".into(), owned(3u8));
        log.insert("total_data_written".into(), owned(4_502_664_704_000u64));
        log.insert("temp_sensors".into(), owned(vec![327u16, 322, 0]));
        log.insert("media_errors".into(), owned(0u64));
        let a = nvme(&log);
        let names: Vec<&str> = a.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Available spare",
                "Life used",
                "Data written",
                "Temperature sensor 1",
                "Temperature sensor 2",
                "Media and data integrity errors"
            ]
        );
        assert_eq!(a[2].value, "4.5 TB");
        assert_eq!(a[3].value, "54 °C");
        assert!(a.iter().all(|a| !a.failing));
    }
}
