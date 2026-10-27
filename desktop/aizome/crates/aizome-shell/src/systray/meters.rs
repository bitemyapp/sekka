//! The CPU and memory meters: samples taken off the shell's thread every
//! two seconds, turned into whole-point bar heights so a meter repaints
//! only when a bar visibly moves.

use aizome_sys::sysinfo::{CpuSampler, CpuUsage, memory};

/// The meters' track height, in points.
pub const TRACK: f64 = 14.0;
/// At most this many CPU bars; more cores share bars (each the busiest of
/// its share).
pub const MAX_BARS: usize = 8;
/// A core at or above this is hot (amber).
pub const HOT: f32 = 60.0;

/// One reading of both meters.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// Each bar's load, 0–100.
    pub cores: Vec<f32>,
    /// Memory in use, 0–100.
    pub memory: f32,
    pub total: f32,
}

impl Reading {
    /// The bars' heights in whole points (at least 1 for a live core, as
    /// the boards draw idle cores).
    pub fn bar_heights(&self) -> Vec<u8> {
        self.cores.iter().map(|l| height(*l)).collect()
    }

    pub fn memory_height(&self) -> u8 {
        height(self.memory)
    }

    pub fn hot(&self) -> Vec<bool> {
        self.cores.iter().map(|l| *l >= HOT).collect()
    }
}

fn height(load: f32) -> u8 {
    ((load.clamp(0.0, 100.0) / 100.0 * TRACK as f32).round() as u8).clamp(1, TRACK as u8)
}

/// Loads per bar from per-core loads: cores split as evenly as they go
/// into at most [`MAX_BARS`] bars.
pub fn bars(cores: &[f32]) -> Vec<f32> {
    if cores.len() <= MAX_BARS {
        return cores.to_vec();
    }
    let n = MAX_BARS;
    (0..n)
        .map(|b| {
            let lo = b * cores.len() / n;
            let hi = (b + 1) * cores.len() / n;
            cores[lo..hi].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

/// Takes readings; lives on a background thread.
pub struct Sampler {
    cpu: CpuSampler,
}

impl Default for Sampler {
    fn default() -> Self {
        Sampler::new()
    }
}

impl Sampler {
    pub fn new() -> Sampler {
        let mut cpu = CpuSampler::new();
        // The first sample has nothing to compare with.
        let _ = cpu.sample();
        Sampler { cpu }
    }

    pub fn read(&mut self) -> Reading {
        let usage: CpuUsage = self.cpu.sample();
        let per_core: Vec<f32> = usage.cores.iter().map(|c| c.percent).collect();
        let memory = memory().map(|m| m.used_percent()).unwrap_or(0.0);
        Reading { cores: bars(&per_core), memory, total: usage.total }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_cores_share_bars() {
        let cores: Vec<f32> = (0..32).map(|i| if i == 13 { 90.0 } else { 5.0 }).collect();
        let b = bars(&cores);
        assert_eq!(b.len(), MAX_BARS);
        assert_eq!(b[3], 90.0, "the busiest core of a share shows");
        assert_eq!(bars(&[10.0, 20.0]), [10.0, 20.0]);
    }

    #[test]
    fn heights_are_whole_points_and_never_vanish() {
        let r = Reading { cores: vec![0.0, 50.0, 100.0, 71.0], memory: 38.0, total: 30.0 };
        assert_eq!(r.bar_heights(), [1, 7, 14, 10]);
        assert_eq!(r.memory_height(), 5);
        assert_eq!(r.hot(), [false, false, true, true]);
    }

    #[test]
    fn the_sampler_reads_this_machine() {
        let mut s = Sampler::new();
        let r = s.read();
        assert!(!r.cores.is_empty());
        assert!(r.memory > 0.0 && r.memory <= 100.0);
    }
}
