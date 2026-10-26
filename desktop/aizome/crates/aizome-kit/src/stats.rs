//! What frames cost: event handling, the program's update, view and
//! layout, painting, and how much of the window each frame redrew.

use std::collections::VecDeque;
use std::time::Duration;

const KEEP: usize = 1024;

/// Recent samples of one measure, in microseconds.
#[derive(Clone, Debug, Default)]
pub struct Samples(VecDeque<u32>);

impl Samples {
    pub fn push(&mut self, d: Duration) {
        if self.0.len() == KEEP {
            self.0.pop_front();
        }
        self.0.push_back(d.as_micros().min(u128::from(u32::MAX)) as u32);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `p`th percentile (0–100), in microseconds.
    pub fn percentile(&self, p: f64) -> u32 {
        if self.0.is_empty() {
            return 0;
        }
        let mut v: Vec<u32> = self.0.iter().copied().collect();
        v.sort_unstable();
        let i = ((p / 100.0) * (v.len() - 1) as f64).round() as usize;
        v[i.min(v.len() - 1)]
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub event: Samples,
    pub update: Samples,
    pub view_layout: Samples,
    pub paint: Samples,
    /// Damage rectangles per build, and the share of the window redrawn
    /// (per mille).
    pub damage_rects: Vec<usize>,
    pub damage_share: Vec<u32>,
}

impl Stats {
    pub(crate) fn event(&mut self, d: Duration) {
        self.event.push(d);
    }

    pub(crate) fn update(&mut self, d: Duration) {
        self.update.push(d);
    }

    pub(crate) fn view_layout(&mut self, d: Duration) {
        self.view_layout.push(d);
    }

    pub(crate) fn paint(&mut self, d: Duration) {
        self.paint.push(d);
    }

    pub(crate) fn damage(&mut self, rects: usize, area: f64, window: f64) {
        if self.damage_rects.len() >= KEEP {
            self.damage_rects.remove(0);
            self.damage_share.remove(0);
        }
        self.damage_rects.push(rects);
        self.damage_share.push(if window > 0.0 { ((area / window) * 1000.0).min(1000.0) as u32 } else { 0 });
    }

    /// p50 and p99 of each measure.
    pub fn summary(&self) -> String {
        let line = |name: &str, s: &Samples| {
            format!("{name}: p50 {}µs p99 {}µs (n={})", s.percentile(50.0), s.percentile(99.0), s.len())
        };
        [
            line("event", &self.event),
            line("update", &self.update),
            line("view+layout", &self.view_layout),
            line("paint (record)", &self.paint),
        ]
        .join("\n")
    }

    pub fn clear(&mut self) {
        *self = Stats::default();
    }
}
