//! The playback clock: where in the media we are now.
//!
//! It is an anchor (a media time at an instant of CLOCK_MONOTONIC, the
//! clock frames are presented by) and a rate (the speed, or none while
//! stopped), read without locks by the interface and the video thread.
//! While audio plays, the audio callback anchors it to the sample that
//! the device plays at its graph cycle's time, so pictures follow what
//! is heard; without audio (or while it isn't running) the player
//! anchors it to the monotonic clock.

use std::sync::atomic::{AtomicU64, Ordering};

pub use sidestep_ui::video::monotonic_now;

/// The clock, a seqlock of three words, and who to tell when it starts.
#[derive(Default)]
pub struct Clock {
    seq: AtomicU64,
    pos: AtomicU64,
    at: AtomicU64,
    rate: AtomicU64,
    on_start: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>>,
}

/// A reading of the clock's anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    /// Media seconds at `at`.
    pub pos: f64,
    /// CLOCK_MONOTONIC nanoseconds.
    pub at: u64,
    /// Media seconds per second: 0 while stopped.
    pub rate: f64,
}

impl Anchor {
    /// Media seconds at `now` (monotonic nanoseconds).
    pub fn at_time(&self, now: u64) -> f64 {
        if self.rate == 0.0 {
            return self.pos;
        }
        self.pos + (now as f64 - self.at as f64) / 1e9 * self.rate
    }

    /// The monotonic time media time `pos` comes, if the clock runs.
    pub fn time_of(&self, pos: f64) -> Option<u64> {
        (self.rate > 0.0).then(|| (self.at as f64 + (pos - self.pos) / self.rate * 1e9).max(0.0) as u64)
    }
}

impl Clock {
    pub fn new() -> Clock {
        Clock::default()
    }

    /// Call `f` whenever the clock starts running (from stopped): once set.
    pub fn on_start(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.on_start.set(Box::new(f));
    }

    /// Set the anchor. `wait`: wait for another writer; else give up if
    /// one is writing (the audio callback, which mustn't wait).
    pub fn set(&self, a: Anchor, wait: bool) -> bool {
        loop {
            let s = self.seq.load(Ordering::Relaxed);
            if s & 1 == 0
                && self.seq.compare_exchange_weak(s, s + 1, Ordering::Acquire, Ordering::Relaxed).is_ok()
            {
                self.pos.store(a.pos.to_bits(), Ordering::Relaxed);
                self.at.store(a.at, Ordering::Relaxed);
                let was = f64::from_bits(self.rate.swap(a.rate.to_bits(), Ordering::Relaxed));
                self.seq.store(s + 2, Ordering::Release);
                // Waiting for it to run (the video, before the audio's first
                // cycle): it runs now.
                if was == 0.0 && a.rate > 0.0
                    && let Some(f) = self.on_start.get()
                {
                    f();
                }
                return true;
            }
            if !wait {
                return false;
            }
            std::hint::spin_loop();
        }
    }

    pub fn anchor(&self) -> Anchor {
        loop {
            let s = self.seq.load(Ordering::Acquire);
            if s & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let a = Anchor {
                pos: f64::from_bits(self.pos.load(Ordering::Relaxed)),
                at: self.at.load(Ordering::Relaxed),
                rate: f64::from_bits(self.rate.load(Ordering::Relaxed)),
            };
            std::sync::atomic::fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s {
                return a;
            }
        }
    }

    /// Media seconds now.
    pub fn now(&self) -> f64 {
        self.anchor().at_time(monotonic_now())
    }

    /// Stop at `pos`.
    pub fn stop_at(&self, pos: f64) {
        self.set(Anchor { pos, at: monotonic_now(), rate: 0.0 }, true);
    }

    /// Run from `pos` now at `rate`.
    pub fn run_from(&self, pos: f64, rate: f64) {
        self.set(Anchor { pos, at: monotonic_now(), rate }, true);
    }

    pub fn running(&self) -> bool {
        self.anchor().rate > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_clock_stays_and_a_running_one_moves() {
        let c = Clock::new();
        c.stop_at(12.5);
        assert_eq!(c.now(), 12.5);
        assert!(!c.running());
        let a = Anchor { pos: 10.0, at: 1_000_000_000, rate: 2.0 };
        assert_eq!(a.at_time(1_500_000_000), 11.0);
        assert_eq!(a.time_of(11.0), Some(1_500_000_000));
        c.set(a, true);
        assert!(c.running());
        assert_eq!(c.anchor(), a);
        assert_eq!(Anchor { rate: 0.0, ..a }.time_of(11.0), None);
    }

    #[test]
    fn readers_never_see_half_an_anchor() {
        let c = std::sync::Arc::new(Clock::new());
        let w = c.clone();
        let writer = std::thread::spawn(move || {
            for i in 0..20_000u64 {
                let x = i as f64;
                w.set(Anchor { pos: x, at: i, rate: x }, true);
            }
        });
        for _ in 0..20_000 {
            let a = c.anchor();
            assert_eq!(a.pos, a.at as f64);
            assert_eq!(a.rate, a.pos);
        }
        writer.join().expect("writer");
    }
}
