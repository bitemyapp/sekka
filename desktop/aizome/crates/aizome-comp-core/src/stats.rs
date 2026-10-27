//! Frame times per output: a rolling window of the last frames' CPU (and,
//! where measurable, GPU) times, read as percentiles over IPC.

use std::collections::VecDeque;
use std::time::Duration;

use aizome_ipc::{OutputStats, Percentiles};

/// How many frames the percentiles cover.
pub const WINDOW: usize = 4096;

#[derive(Debug, Default)]
pub struct FrameStats {
    cpu: VecDeque<u32>,
    gpu: VecDeque<u32>,
    pub rendered: u64,
    pub skipped: u64,
}

impl FrameStats {
    /// A frame that drew something: its CPU time (collect, render, submit)
    /// and its GPU time if known.
    pub fn record(&mut self, cpu: Duration, gpu: Option<Duration>) {
        self.rendered += 1;
        push(&mut self.cpu, cpu);
        if let Some(g) = gpu {
            push(&mut self.gpu, g);
        }
    }

    /// A redraw that found nothing damaged.
    pub fn skip(&mut self) {
        self.skipped += 1;
    }

    pub fn reset(&mut self) {
        *self = FrameStats::default();
    }

    pub fn report(&self, name: &str) -> OutputStats {
        OutputStats {
            name: name.to_string(),
            frames_rendered: self.rendered,
            frames_skipped: self.skipped,
            cpu_us: percentiles(&self.cpu),
            gpu_us: (!self.gpu.is_empty()).then(|| percentiles(&self.gpu)),
            samples: self.cpu.len() as u64,
        }
    }
}

fn push(q: &mut VecDeque<u32>, d: Duration) {
    if q.len() == WINDOW {
        q.pop_front();
    }
    q.push_back(d.as_micros().min(u32::MAX as u128) as u32);
}

/// Nearest-rank percentiles, in microseconds.
pub fn percentiles(q: &VecDeque<u32>) -> Percentiles {
    if q.is_empty() {
        return Percentiles::default();
    }
    let mut v: Vec<u32> = q.iter().copied().collect();
    v.sort_unstable();
    let at = |p: f64| {
        let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
        v[rank.clamp(1, v.len()) - 1] as f64
    };
    let mean = v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64;
    Percentiles { p50: at(50.0), p95: at(95.0), p99: at(99.0), max: *v.last().unwrap() as f64, mean }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_by_nearest_rank() {
        let mut s = FrameStats::default();
        for i in 1..=100 {
            s.record(Duration::from_micros(i), None);
        }
        s.skip();
        let r = s.report("x");
        assert_eq!(r.frames_rendered, 100);
        assert_eq!(r.frames_skipped, 1);
        assert_eq!(r.cpu_us.p50, 50.0);
        assert_eq!(r.cpu_us.p95, 95.0);
        assert_eq!(r.cpu_us.p99, 99.0);
        assert_eq!(r.cpu_us.max, 100.0);
        assert!(r.gpu_us.is_none());
    }

    #[test]
    fn the_window_rolls() {
        let mut s = FrameStats::default();
        for _ in 0..WINDOW {
            s.record(Duration::from_micros(1000), None);
        }
        for _ in 0..WINDOW {
            s.record(Duration::from_micros(10), None);
        }
        assert_eq!(s.report("x").cpu_us.max, 10.0);
    }
}
