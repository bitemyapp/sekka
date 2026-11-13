use crate::bluetooth::{Record, Snapshot};
use serde::Serialize;
use std::collections::VecDeque;

pub fn strongest_first(records: &mut [Record]) {
    records.sort_by(|a, b| b.rssi_dbm.cmp(&a.rssi_dbm).then(a.address.cmp(&b.address)).then(a.path.cmp(&b.path)));
}

#[derive(Clone, Debug, Serialize)]
pub struct Sample {
    pub at_unix_ms: u64,
    pub rssi_dbm: i16,
    pub report_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Tracker {
    pub record: Record,
    pub present: bool,
    pub baseline_dbm: Option<i16>,
    pub samples: VecDeque<Sample>,
}

impl Tracker {
    pub fn new(record: Record) -> Self {
        let mut tracker = Self { record, present: true, baseline_dbm: None, samples: VecDeque::new() };
        tracker.sample();
        tracker
    }

    pub fn update(&mut self, snapshot: &Snapshot) {
        if let Some(record) = snapshot.records.iter().find(|r| r.path == self.record.path) {
            self.record = record.clone();
            self.present = true;
            self.sample();
        } else {
            // Never jump to a different TV after a disappearance or private
            // address rotation, even when its signal/IP looks similar.
            self.present = false;
        }
    }

    fn sample(&mut self) {
        let (Some(rssi), Some(at)) = (self.record.rssi_dbm, self.record.last_report_unix_ms) else { return };
        if self.samples.back().is_some_and(|s| s.report_count == self.record.report_count) {
            return;
        }
        self.baseline_dbm.get_or_insert(rssi);
        self.samples.push_back(Sample { at_unix_ms: at, rssi_dbm: rssi, report_count: self.record.report_count });
        while self.samples.len() > 120 {
            self.samples.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, rssi: Option<i16>) -> Record {
        Record { path: id.into(), address: id.into(), rssi_dbm: rssi, ..Record::default() }
    }

    #[test]
    fn strength_sort_is_descending_with_stable_ties_and_unknown_last() {
        let mut rows =
            vec![record("missing", None), record("weak", Some(-91)), record("b", Some(-40)), record("a", Some(-40))];
        strongest_first(&mut rows);
        assert_eq!(rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["a", "b", "weak", "missing"]);
    }

    #[test]
    fn tracking_survives_reordering_and_loss_without_switching_devices() {
        let mut target = record("target", Some(-40));
        let mut tracker = Tracker::new(target.clone());
        assert!(tracker.samples.is_empty(), "cached RSSI is not a fresh observation");
        target.last_report_unix_ms = Some(1000);
        target.report_count = 1;
        let mut snapshot =
            Snapshot { records: vec![record("other", Some(-30)), target.clone()], ..Snapshot::default() };
        tracker.update(&snapshot);
        tracker.update(&snapshot);
        assert_eq!(tracker.samples.len(), 1, "polling must not duplicate samples");
        target.rssi_dbm = Some(-80);
        target.report_count = 2;
        target.last_report_unix_ms = Some(2000);
        snapshot.records = vec![target, record("other", Some(-10))];
        tracker.update(&snapshot);
        assert_eq!(tracker.record.path, "target");
        assert_eq!(tracker.record.rssi_dbm, Some(-80));
        assert_eq!(tracker.baseline_dbm, Some(-40));
        snapshot.records = vec![record("rotated-or-unrelated", Some(-80))];
        tracker.update(&snapshot);
        assert!(!tracker.present);
        assert_eq!(tracker.record.path, "target");
        assert_eq!(tracker.samples.len(), 2);
    }
}
