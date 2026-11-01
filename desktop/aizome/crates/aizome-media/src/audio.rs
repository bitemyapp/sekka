//! Decoded audio on its way out: a ring of samples between the demuxer,
//! which decodes ahead in bursts, and the output's callback, which takes
//! what each graph cycle needs, and the output itself (PipeWire, or a
//! clock-paced null output where there's no sound server).
//!
//! The ring is single-producer, single-consumer and lock-free on the
//! consumer's side, which runs on PipeWire's real-time thread. It keeps a
//! second behind what was taken: pausing stops the stream, drops what
//! the graph still held, and steps back to the sample being heard, so
//! play resumes exactly there however large the buffers are (and the
//! buffers can be large: few wakeups is what saves power). Each run of
//! samples after a flush is a segment, which says what media time its
//! first sample is and at what speed it plays, so the callback can say
//! what's being heard.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::clock::{Anchor, Clock, monotonic_now};

/// The audio the ring carries: f32, interleaved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub rate: u32,
    pub channels: u32,
}

/// Seconds of audio the ring holds, and keeps behind the consumer.
const CAPACITY_SECS: u64 = 4;
const HISTORY_SECS: u64 = 1;

pub struct Ring {
    data: Box<[UnsafeCell<f32>]>,
    pub format: Format,
    /// Frames.
    cap: u64,
    history: u64,
    /// Frames written and taken, counting up from 0 for the ring's life.
    write: AtomicU64,
    read: AtomicU64,
    /// The consumer moves `read` here before taking (a flush while it
    /// runs).
    skip_to: AtomicU64,
    segments: Mutex<Vec<Segment>>,
    /// The consumer is in its callback.
    pub busy: AtomicBool,
    /// The consumer takes nothing and leaves the clock alone (paused: a
    /// cycle that runs after the stream stopped plays silence).
    pub halted: AtomicBool,
    /// The producer wrote everything (the end of the stream).
    pub ended: AtomicBool,
}

// SAFETY: the producer writes only frames in [write, read + cap - history)
// and the consumer reads only frames in [read, write), so no sample is
// written and read at once; indices are published with release/acquire.
unsafe impl Sync for Ring {}
unsafe impl Send for Ring {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    start: u64,
    pts: f64,
    speed: f64,
}

impl Ring {
    pub fn new(format: Format) -> Ring {
        let rate = u64::from(format.rate.max(1));
        let cap = rate * CAPACITY_SECS;
        let len = (cap * u64::from(format.channels.max(1))) as usize;
        Ring {
            data: (0..len).map(|_| UnsafeCell::new(0.0)).collect(),
            format,
            cap,
            history: rate * HISTORY_SECS,
            write: AtomicU64::new(0),
            read: AtomicU64::new(0),
            skip_to: AtomicU64::new(0),
            segments: Mutex::new(vec![Segment { start: 0, pts: 0.0, speed: 1.0 }]),
            busy: AtomicBool::new(false),
            halted: AtomicBool::new(false),
            ended: AtomicBool::new(false),
        }
    }

    fn channels(&self) -> usize {
        self.format.channels.max(1) as usize
    }

    /// Frames written and not yet taken (or skipped).
    pub fn buffered(&self) -> u64 {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire).max(self.skip_to.load(Ordering::Acquire));
        w.saturating_sub(r)
    }

    /// Seconds buffered.
    pub fn buffered_secs(&self) -> f64 {
        self.buffered() as f64 / f64::from(self.format.rate.max(1))
    }

    /// Frames the producer may write now.
    pub fn space(&self) -> u64 {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        (self.cap - self.history).saturating_sub(w - r)
    }

    /// Write interleaved samples, as many whole frames as fit: how many.
    pub fn push(&self, samples: &[f32]) -> usize {
        let ch = self.channels();
        let frames = ((samples.len() / ch) as u64).min(self.space());
        let w = self.write.load(Ordering::Relaxed);
        for f in 0..frames {
            let slot = ((w + f) % self.cap) as usize * ch;
            for c in 0..ch {
                // SAFETY: slot is the producer's (see `Sync`).
                unsafe { *self.data[slot + c].get() = samples[f as usize * ch + c] };
            }
        }
        self.write.store(w + frames, Ordering::Release);
        frames as usize
    }

    /// Take up to `out.len()` samples (whole frames) into `out`: how many
    /// frames, and the frame number of the first.
    pub fn pop(&self, out: &mut [f32]) -> (usize, u64) {
        let ch = self.channels();
        let skip = self.skip_to.load(Ordering::Acquire);
        let mut r = self.read.load(Ordering::Relaxed);
        if r < skip {
            r = skip;
        }
        let w = self.write.load(Ordering::Acquire);
        let frames = ((out.len() / ch) as u64).min(w.saturating_sub(r));
        for f in 0..frames {
            let slot = ((r + f) % self.cap) as usize * ch;
            for c in 0..ch {
                // SAFETY: slot is the consumer's (see `Sync`).
                out[f as usize * ch + c] = unsafe { *self.data[slot + c].get() };
            }
        }
        self.read.store(r + frames, Ordering::Release);
        (frames as usize, r)
    }

    /// The next frame to be taken.
    pub fn read_index(&self) -> u64 {
        self.read.load(Ordering::Acquire).max(self.skip_to.load(Ordering::Acquire))
    }

    pub fn write_index(&self) -> u64 {
        self.write.load(Ordering::Acquire)
    }

    /// Drop what's buffered: what's written next starts at media time
    /// `pts`, played at `speed`. With the consumer stopped (`idle`), the
    /// space is free at once; else at its next take.
    pub fn flush(&self, pts: f64, speed: f64, idle: bool) {
        let w = self.write.load(Ordering::Relaxed);
        self.skip_to.store(w, Ordering::Release);
        if idle {
            self.read.store(w, Ordering::Release);
        }
        self.ended.store(false, Ordering::Release);
        let mut segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
        segs.retain(|s| s.start < w);
        segs.push(Segment { start: w, pts, speed });
        let keep = segs.len().saturating_sub(4);
        segs.drain(..keep);
    }

    /// The media time of frame `index`, if the ring still knows it (the
    /// consumer doesn't wait for the lock: none if it's held).
    pub fn pts_of(&self, index: u64, wait: bool) -> Option<f64> {
        let segs = if wait {
            self.segments.lock().unwrap_or_else(|e| e.into_inner())
        } else {
            self.segments.try_lock().ok()?
        };
        let s = segs.iter().rev().find(|s| s.start <= index)?;
        Some(s.pts + (index - s.start) as f64 / f64::from(self.format.rate.max(1)) * s.speed)
    }

    /// The speed frame `index` plays at.
    pub fn speed_of(&self, index: u64) -> f64 {
        let segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
        segs.iter().rev().find(|s| s.start <= index).map_or(1.0, |s| s.speed)
    }

    /// Step the consumer back to frame `index` (the consumer stopped):
    /// the frames from there play again. Only within the history and the
    /// current segment; returns where it went.
    pub fn rewind_to(&self, index: u64) -> u64 {
        let r = self.read_index();
        let start = {
            let segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
            segs.iter().rev().find(|s| s.start <= r).map_or(0, |s| s.start)
        };
        let to = index.clamp(r.saturating_sub(self.history).max(start), r);
        self.read.store(to, Ordering::Release);
        self.skip_to.store(to.min(self.skip_to.load(Ordering::Relaxed)), Ordering::Release);
        to
    }

    /// The frame being heard at media time `pos`, in the current segment.
    pub fn index_of(&self, pos: f64) -> Option<u64> {
        let r = self.read_index();
        let segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
        let s = segs.iter().rev().find(|s| s.start <= r)?;
        let frames = ((pos - s.pts) / s.speed * f64::from(self.format.rate)).round();
        Some(s.start + frames.max(0.0) as u64)
    }
}

/// What the output callback does each cycle: fill `out` (interleaved,
/// whole frames) from the ring, silence after what there is, and anchor
/// the clock to what's heard. `latency` is the frames between the next
/// frame written and the speaker; `now` the cycle's time. Returns the
/// frames of audio (not silence) written.
pub fn fill(ring: &Ring, clock: &Clock, out: &mut [f32], latency: u64, now: u64) -> usize {
    // Busy, then halted: whoever halts the ring and then waits while it's
    // busy sees this cycle, or this cycle sees the halt (sequentially
    // consistent, as neither may pass the other).
    ring.busy.store(true, Ordering::SeqCst);
    if ring.halted.load(Ordering::SeqCst) {
        out.fill(0.0);
        ring.busy.store(false, Ordering::Release);
        return 0;
    }
    let (frames, first) = ring.pop(out);
    let ch = ring.format.channels.max(1) as usize;
    out[frames * ch..].fill(0.0);
    let rate = f64::from(ring.format.rate.max(1));
    if let Some(pts) = ring.pts_of(first, false) {
        let speed = ring.speed_of_quick(first);
        // What's heard now is `latency` frames before the first written;
        // run dry, the clock stops there.
        let pos = pts - latency as f64 / rate * speed;
        clock.set(Anchor { pos, at: now, rate: if frames > 0 { speed } else { 0.0 } }, false);
    }
    ring.busy.store(false, Ordering::Release);
    frames
}

impl Ring {
    /// `speed_of` without waiting for the lock (the callback's): 1 if
    /// it's held.
    fn speed_of_quick(&self, index: u64) -> f64 {
        match self.segments.try_lock() {
            Ok(segs) => segs.iter().rev().find(|s| s.start <= index).map_or(1.0, |s| s.speed),
            Err(_) => 1.0,
        }
    }
}

/// Where decoded audio goes.
pub trait Output: Send {
    /// Preferred PipeWire node name for the next stream; None follows the
    /// session default. Outputs without device selection may ignore it.
    fn set_device(&mut self, _device: Option<&str>) {}
    /// Start a stream of `format` taking from `ring`, stopped: the clock
    /// follows it once it plays.
    fn open(&mut self, ring: Arc<Ring>, clock: Arc<Clock>, media: &str) -> Result<(), String>;
    /// Play or stop taking. Stopping drops what the graph holds and steps
    /// the ring back to what was being heard, which plays first on the
    /// next start; the clock stops there.
    fn set_playing(&mut self, playing: bool);
    /// Drop what the graph holds (a seek; the ring was flushed).
    fn flush(&mut self);
    /// Volume 0–1 (perceptual) and mute.
    fn set_volume(&mut self, volume: f32, muted: bool);
    /// How the next stream opened is used: how long its cycles may be
    /// (seconds), and its role ("Movie", "Music").
    fn set_stream(&mut self, latency: f64, role: &'static str) {
        let _ = (latency, role);
    }
    /// End the stream.
    fn close(&mut self);
    /// What it is, for the info window.
    fn describe(&self) -> String;
}

/// An output with no sound server: takes the ring at its rate on a thread
/// of its own, in 50 ms cycles, as a device would, and drops the samples.
/// For tests, and for playing where nothing can be heard.
pub struct NullOutput {
    run: Option<Arc<NullRun>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct NullRun {
    ring: Arc<Ring>,
    clock: Arc<Clock>,
    playing: AtomicBool,
    stop: AtomicBool,
    cv: std::sync::Condvar,
    m: Mutex<()>,
}

impl NullOutput {
    pub fn new() -> NullOutput {
        NullOutput { run: None, thread: None }
    }
}

impl Default for NullOutput {
    fn default() -> NullOutput {
        NullOutput::new()
    }
}

/// The null output's cycle and the latency it says it has.
const NULL_CYCLE_MS: u64 = 50;

impl Output for NullOutput {
    fn open(&mut self, ring: Arc<Ring>, clock: Arc<Clock>, _media: &str) -> Result<(), String> {
        self.close();
        let run = Arc::new(NullRun {
            ring,
            clock,
            playing: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            cv: std::sync::Condvar::new(),
            m: Mutex::new(()),
        });
        let r = run.clone();
        self.thread = Some(
            std::thread::Builder::new()
                .name("aizome-media-null-audio".into())
                .spawn(move || {
                    let fmt = r.ring.format;
                    let frames = (u64::from(fmt.rate) * NULL_CYCLE_MS / 1000) as usize;
                    let mut buf = vec![0.0f32; frames * fmt.channels.max(1) as usize];
                    let cycle = std::time::Duration::from_millis(NULL_CYCLE_MS);
                    let mut next = std::time::Instant::now();
                    while !r.stop.load(Ordering::Acquire) {
                        if !r.playing.load(Ordering::Acquire) {
                            let g = r.m.lock().unwrap_or_else(|e| e.into_inner());
                            let _ = r.cv.wait_timeout(g, std::time::Duration::from_millis(200));
                            next = std::time::Instant::now();
                            continue;
                        }
                        fill(&r.ring, &r.clock, &mut buf, 0, monotonic_now());
                        next += cycle;
                        let now = std::time::Instant::now();
                        if next > now {
                            std::thread::sleep(next - now);
                        } else {
                            next = now;
                        }
                    }
                })
                .map_err(|e| e.to_string())?,
        );
        self.run = Some(run);
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) {
        let Some(r) = &self.run else { return };
        if !playing && r.playing.load(Ordering::Acquire) {
            r.playing.store(false, Ordering::Release);
            // A cycle that already saw it playing takes nothing now, and
            // leaves the clock alone: stopped below, it stays stopped.
            r.ring.halted.store(true, Ordering::SeqCst);
            while r.ring.busy.load(Ordering::SeqCst) {
                std::hint::spin_loop();
            }
            // Nothing is held past the ring: the clock stops at what's next.
            let i = r.ring.read_index();
            if let Some(pos) = r.ring.pts_of(i, true) {
                r.clock.stop_at(pos);
            }
            return;
        }
        if playing {
            // Whatever halted the ring (a pause, or the output this one
            // stands in for) is over.
            r.ring.halted.store(false, Ordering::SeqCst);
        }
        r.playing.store(playing, Ordering::Release);
        r.cv.notify_all();
    }

    fn flush(&mut self) {}

    fn set_volume(&mut self, _volume: f32, _muted: bool) {}

    fn close(&mut self) {
        if let Some(r) = self.run.take() {
            r.stop.store(true, Ordering::Release);
            r.cv.notify_all();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn describe(&self) -> String {
        "No audio output".into()
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.close();
    }
}

/// Volume as the slider shows it (0–1, perceptual) as an amplitude: the
/// cube, as PipeWire's and PulseAudio's own sliders map it.
pub fn amplitude(volume: f32) -> f32 {
    let v = volume.clamp(0.0, 1.0);
    v * v * v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring() -> Ring {
        Ring::new(Format { rate: 100, channels: 2 })
    }

    #[test]
    fn what_goes_in_comes_out_in_order() {
        let r = ring();
        let samples: Vec<f32> = (0..20).map(|i| i as f32).collect();
        assert_eq!(r.push(&samples), 10);
        assert_eq!(r.buffered(), 10);
        let mut out = [0.0; 8];
        assert_eq!(r.pop(&mut out), (4, 0));
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(r.buffered(), 6);
    }

    #[test]
    fn the_producer_leaves_the_history_alone() {
        let r = ring();
        // 4 seconds of room, one kept behind the consumer: 3 free.
        assert_eq!(r.space(), 300);
        let big = vec![1.0; 2 * 400];
        assert_eq!(r.push(&big), 300);
        let mut out = vec![0.0; 2 * 100];
        r.pop(&mut out);
        assert_eq!(r.space(), 100);
    }

    #[test]
    fn media_times_follow_segments() {
        let r = ring();
        r.flush(10.0, 1.0, true);
        r.push(&vec![0.0; 2 * 50]);
        assert_eq!(r.pts_of(0, true), Some(10.0));
        assert_eq!(r.pts_of(25, true), Some(10.25));
        r.flush(30.0, 2.0, true);
        assert_eq!(r.read_index(), 50);
        r.push(&[0.0; 2 * 10]);
        assert_eq!(r.pts_of(60, true), Some(30.2));
        assert_eq!(r.index_of(30.2), Some(60));
    }

    #[test]
    fn pausing_steps_back_to_what_was_heard() {
        let r = ring();
        r.flush(0.0, 1.0, true);
        r.push(&vec![0.5; 2 * 200]);
        let mut out = vec![0.0; 2 * 150];
        r.pop(&mut out);
        // 150 taken; 40 of them still in the graph when it stopped.
        assert_eq!(r.rewind_to(110), 110);
        assert_eq!(r.buffered(), 90);
        // Never further back than the history, nor before the segment.
        assert_eq!(r.rewind_to(0), 10);
        r.flush(5.0, 1.0, true);
        assert_eq!(r.rewind_to(0), 200);
    }

    #[test]
    fn a_flush_while_the_consumer_runs_skips_at_its_next_take() {
        let r = ring();
        r.push(&vec![1.0; 2 * 50]);
        r.flush(3.0, 1.0, false);
        assert_eq!(r.buffered(), 0);
        r.push(&[2.0, 2.0]);
        let mut out = vec![0.0; 2 * 10];
        let (n, first) = r.pop(&mut out);
        assert_eq!((n, first), (1, 50));
        assert_eq!(out[0], 2.0);
    }

    #[test]
    fn the_callback_anchors_the_clock_to_what_is_heard() {
        let r = ring();
        let c = Clock::new();
        r.flush(2.0, 1.0, true);
        r.push(&vec![0.0; 2 * 100]);
        let mut out = vec![1.0; 2 * 30];
        // 20 frames between the first written and the speaker.
        assert_eq!(fill(&r, &c, &mut out, 20, 1_000), 30);
        let a = c.anchor();
        assert!((a.pos - 1.8).abs() < 1e-9, "{a:?}");
        assert_eq!((a.at, a.rate), (1_000, 1.0));
        // Run dry: silence, and the clock stops.
        r.pop(&mut vec![0.0; 2 * 100]);
        let mut out = vec![1.0; 2 * 10];
        assert_eq!(fill(&r, &c, &mut out, 0, 2_000), 0);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(c.anchor().rate, 0.0);
    }

    #[test]
    fn the_null_output_plays_on_a_halted_ring_and_pausing_stops_its_clock() {
        // As when the player falls back to it from a sink that took
        // nothing: that output halted the ring as it stopped.
        let r = Arc::new(Ring::new(Format { rate: 1000, channels: 2 }));
        let c = Arc::new(Clock::new());
        r.flush(0.0, 1.0, true);
        r.push(&vec![0.0; 2 * 3000]);
        r.halted.store(true, Ordering::SeqCst);
        let mut n = NullOutput::new();
        n.open(r.clone(), c.clone(), "test").expect("the null output");
        n.set_playing(true);
        let start = std::time::Instant::now();
        while !c.running() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5), "the clock never ran");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        n.set_playing(false);
        let at = c.now();
        assert!(!c.running());
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(c.now(), at, "paused, the clock stays");
    }

    #[test]
    fn volume_is_perceptual() {
        assert_eq!(amplitude(1.0), 1.0);
        assert_eq!(amplitude(0.5), 0.125);
        assert_eq!(amplitude(2.0), 1.0);
    }
}
