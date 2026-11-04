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
use std::num::NonZeroU32;
use std::ops::Add;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::clock::{Anchor, Clock, MonotonicNs, monotonic_now};

/// The audio the ring carries: f32, interleaved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    rate: NonZeroU32,
    channels: Channels,
}

/// A channel count supported by both the mixer and the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channels(u8);

impl Channels {
    pub const fn new(count: u32) -> Option<Self> {
        if count >= 1 && count <= 8 { Some(Self(count as u8)) } else { None }
    }
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

impl Format {
    pub const fn new(rate: NonZeroU32, channels: Channels) -> Self {
        Self { rate, channels }
    }
    /// Frames per second.
    pub const fn rate(self) -> u32 {
        self.rate.get()
    }
    pub const fn channels(self) -> Channels {
        self.channels
    }
    /// How long `frames` play, in seconds.
    pub fn secs(self, frames: Frames) -> f64 {
        frames.0 as f64 / f64::from(self.rate())
    }
    /// The whole frames nearest `secs` seconds: none for less than none.
    pub fn frames_in(self, secs: f64) -> Frames {
        Frames((secs * f64::from(self.rate())).round().max(0.0) as u64)
    }
}

/// A number of frames, a frame being a sample for each channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frames(u64);

impl Frames {
    pub const ZERO: Frames = Frames(0);
    pub const fn new(frames: u64) -> Self {
        Self(frames)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    /// The interleaved samples in this many frames.
    pub const fn samples(self, channels: Channels) -> usize {
        self.0 as usize * channels.0 as usize
    }
    /// The whole frames in `samples` interleaved samples.
    pub const fn in_samples(samples: usize, channels: Channels) -> Frames {
        Frames((samples / channels.0 as usize) as u64)
    }
    pub const fn saturating_sub(self, other: Frames) -> Frames {
        Frames(self.0.saturating_sub(other.0))
    }
}

impl Add for Frames {
    type Output = Frames;
    fn add(self, other: Frames) -> Frames {
        Frames(self.0 + other.0)
    }
}

/// Where a frame is in a ring's stream, counting from 0 for the ring's
/// life. Only a ring makes these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameIndex(u64);

impl FrameIndex {
    /// The frames from `earlier` to this one: none if it isn't earlier.
    pub const fn since(self, earlier: FrameIndex) -> Frames {
        Frames(self.0.saturating_sub(earlier.0))
    }
    /// `frames` earlier, or the first.
    pub const fn saturating_sub(self, frames: Frames) -> FrameIndex {
        FrameIndex(self.0.saturating_sub(frames.0))
    }
}

impl Add<Frames> for FrameIndex {
    type Output = FrameIndex;
    fn add(self, frames: Frames) -> FrameIndex {
        FrameIndex(self.0 + frames.0)
    }
}

/// A [`FrameIndex`] shared between threads.
struct AtomicIndex(AtomicU64);

impl AtomicIndex {
    const fn new(index: FrameIndex) -> Self {
        Self(AtomicU64::new(index.0))
    }
    fn load(&self, order: Ordering) -> FrameIndex {
        FrameIndex(self.0.load(order))
    }
    fn store(&self, index: FrameIndex, order: Ordering) {
        self.0.store(index.0, order);
    }
    fn fetch_max(&self, index: FrameIndex, order: Ordering) -> FrameIndex {
        FrameIndex(self.0.fetch_max(index.0, order))
    }
}

/// The sole writer. Sharing the ring exposes observations, never write access.
///
/// ```compile_fail
/// use aizome_media::audio::Producer;
/// fn write(p: &Producer) { p.push(&[0.0]); }
/// ```
pub struct Producer {
    ring: Arc<Ring>,
}

impl std::ops::Deref for Producer {
    type Target = Ring;
    fn deref(&self) -> &Ring {
        &self.ring
    }
}

impl Producer {
    pub fn new(format: Format) -> Self {
        Self { ring: Arc::new(Ring::new(format)) }
    }
    pub fn shared(&self) -> Arc<Ring> {
        self.ring.clone()
    }
    /// Write interleaved samples, as many whole frames as fit: how many.
    pub fn push(&mut self, samples: &[f32]) -> Frames {
        self.ring.push(samples)
    }
    /// The consumer acknowledges the skip at its next cycle or stopped flush.
    pub fn flush(&mut self, pts: f64, speed: f64) {
        self.ring.flush(pts, speed);
    }
    pub fn finish(&mut self) {
        self.ring.ended.store(true, Ordering::Release);
    }
}

// Both halves retain the lease. A replacement output cannot start until the
// old callback AND its controller have gone away.
struct Lease {
    ring: Arc<Ring>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.ring.leased.store(false, Ordering::Release);
    }
}

/// The sole reader, moved into the audio callback; intentionally not Clone.
pub struct Consumer {
    lease: Arc<Lease>,
}
/// The sole pause/resume controller, kept off the real-time thread.
pub struct Playback {
    lease: Arc<Lease>,
}
/// Proof that the callback cannot touch the samples or advance the read index.
/// ```compile_fail
/// use aizome_media::audio::Playback;
/// fn invalid(p: &mut Playback) {
///     let mut stopped = p.pause();
///     p.resume();
///     stopped.rewind_to(0);
/// }
/// ```
pub struct Stopped<'a> {
    playback: &'a mut Playback,
}

impl Playback {
    pub fn pause(&mut self) -> Stopped<'_> {
        let ring = &self.lease.ring;
        ring.halted.store(true, Ordering::SeqCst);
        while ring.busy.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        Stopped { playback: self }
    }
    pub fn resume(&mut self) {
        self.lease.ring.halted.store(false, Ordering::SeqCst);
    }
}
impl Stopped<'_> {
    pub fn rewind_to(&mut self, index: FrameIndex) -> FrameIndex {
        self.playback.lease.ring.rewind_to(index)
    }
    pub fn discard_pending(&mut self) {
        let ring = &self.playback.lease.ring;
        let skip = ring.skip_to.load(Ordering::Acquire);
        ring.read.store(skip.max(ring.read.load(Ordering::Relaxed)), Ordering::Release);
        ring.read_high.fetch_max(skip, Ordering::Release);
    }
}

/// Seconds of audio the ring holds, and keeps behind the consumer.
const CAPACITY_SECS: u64 = 4;
const HISTORY_SECS: u64 = 1;

pub struct Ring {
    data: Box<[UnsafeCell<f32>]>,
    format: Format,
    leased: AtomicBool,
    read_high: AtomicIndex,
    cap: Frames,
    history: Frames,
    /// The next frame written, and taken.
    write: AtomicIndex,
    read: AtomicIndex,
    /// The consumer moves `read` here before taking (a flush while it
    /// runs).
    skip_to: AtomicIndex,
    segments: Mutex<Vec<Segment>>,
    /// The consumer is in its callback.
    busy: AtomicBool,
    /// The consumer takes nothing and leaves the clock alone (paused: a
    /// cycle that runs after the stream stopped plays silence).
    halted: AtomicBool,
    /// The producer wrote everything (the end of the stream).
    ended: AtomicBool,
}

// SAFETY: the producer writes only frames in [write, read + cap - history)
// and the consumer reads only frames in [read, write), so no sample is
// written and read at once; indices are published with release/acquire.
// Producer and Consumer are unique, with mutable operations. Playback can
// change read only after halting the callback and waiting for it to finish.
// Rewinds never go behind read_high - history, even after repeated pauses.
unsafe impl Sync for Ring {}
unsafe impl Send for Ring {}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    start: FrameIndex,
    pts: f64,
    speed: f64,
}

impl Ring {
    fn new(format: Format) -> Ring {
        let rate = u64::from(format.rate());
        let cap = Frames(rate * CAPACITY_SECS);
        let start = FrameIndex::default();
        Ring {
            data: (0..cap.samples(format.channels())).map(|_| UnsafeCell::new(0.0)).collect(),
            format,
            leased: AtomicBool::new(false),
            read_high: AtomicIndex::new(start),
            cap,
            history: Frames(rate * HISTORY_SECS),
            write: AtomicIndex::new(start),
            read: AtomicIndex::new(start),
            skip_to: AtomicIndex::new(start),
            segments: Mutex::new(vec![Segment { start, pts: 0.0, speed: 1.0 }]),
            busy: AtomicBool::new(false),
            halted: AtomicBool::new(true),
            ended: AtomicBool::new(false),
        }
    }

    pub fn format(&self) -> Format {
        self.format
    }

    /// Acquire the unique output pair. Closing an output releases its lease,
    /// so fallback can reuse buffered samples without a second reader.
    pub fn consumer(self: &Arc<Self>) -> Result<(Consumer, Playback), String> {
        self.leased
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| "audio ring already has an output".to_owned())?;
        self.halted.store(true, Ordering::SeqCst);
        let lease = Arc::new(Lease { ring: self.clone() });
        Ok((Consumer { lease: lease.clone() }, Playback { lease }))
    }

    /// Frames written and not yet taken (or skipped).
    pub fn buffered(&self) -> Frames {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Acquire).max(self.skip_to.load(Ordering::Acquire));
        w.since(r)
    }

    /// Seconds buffered.
    pub fn buffered_secs(&self) -> f64 {
        self.format.secs(self.buffered())
    }

    /// Frames the producer may write now.
    pub fn space(&self) -> Frames {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        self.cap.saturating_sub(self.history).saturating_sub(w.since(r))
    }

    /// Write interleaved samples, as many whole frames as fit: how many.
    fn push(&self, samples: &[f32]) -> Frames {
        let channels = self.format.channels();
        let ch = channels.get() as usize;
        let frames = Frames::in_samples(samples.len(), channels).min(self.space());
        let w = self.write.load(Ordering::Relaxed);
        for f in 0..frames.0 {
            let slot = ((w.0 + f) % self.cap.0) as usize * ch;
            for c in 0..ch {
                // SAFETY: slot is the producer's (see `Sync`).
                unsafe { *self.data[slot + c].get() = samples[f as usize * ch + c] };
            }
        }
        self.write.store(w + frames, Ordering::Release);
        frames
    }

    /// Take up to `out.len()` samples (whole frames) into `out`: how many
    /// frames, and where the first was.
    fn pop(&self, out: &mut [f32]) -> (Frames, FrameIndex) {
        let channels = self.format.channels();
        let ch = channels.get() as usize;
        let r = self.read.load(Ordering::Relaxed).max(self.skip_to.load(Ordering::Acquire));
        let w = self.write.load(Ordering::Acquire);
        let frames = Frames::in_samples(out.len(), channels).min(w.since(r));
        for f in 0..frames.0 {
            let slot = ((r.0 + f) % self.cap.0) as usize * ch;
            for c in 0..ch {
                // SAFETY: slot is the consumer's (see `Sync`).
                out[f as usize * ch + c] = unsafe { *self.data[slot + c].get() };
            }
        }
        self.read_high.store(self.read_high.load(Ordering::Relaxed).max(r + frames), Ordering::Release);
        self.read.store(r + frames, Ordering::Release);
        (frames, r)
    }

    /// The next frame to be taken.
    pub fn read_index(&self) -> FrameIndex {
        self.read.load(Ordering::Acquire).max(self.skip_to.load(Ordering::Acquire))
    }

    pub fn write_index(&self) -> FrameIndex {
        self.write.load(Ordering::Acquire)
    }

    /// Drop what's buffered: what's written next starts at media time
    /// `pts`, played at `speed`; acknowledged at the next take.
    fn flush(&self, pts: f64, speed: f64) {
        let w = self.write.load(Ordering::Relaxed);
        self.skip_to.store(w, Ordering::Release);
        self.ended.store(false, Ordering::Release);
        let mut segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
        segs.retain(|s| s.start < w);
        segs.push(Segment { start: w, pts, speed });
        let keep = segs.len().saturating_sub(4);
        segs.drain(..keep);
    }

    /// The media time of frame `index`, if the ring still knows it (the
    /// consumer doesn't wait for the lock: none if it's held).
    pub fn pts_of(&self, index: FrameIndex, wait: bool) -> Option<f64> {
        let segs = if wait {
            self.segments.lock().unwrap_or_else(|e| e.into_inner())
        } else {
            self.segments.try_lock().ok()?
        };
        let s = segs.iter().rev().find(|s| s.start <= index)?;
        Some(s.pts + self.format.secs(index.since(s.start)) * s.speed)
    }

    /// The speed frame `index` plays at.
    pub fn speed_of(&self, index: FrameIndex) -> f64 {
        let segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
        segs.iter().rev().find(|s| s.start <= index).map_or(1.0, |s| s.speed)
    }

    /// Step the consumer back to frame `index` (the consumer stopped):
    /// the frames from there play again. Only within the history and the
    /// current segment; returns where it went.
    fn rewind_to(&self, index: FrameIndex) -> FrameIndex {
        let r = self.read_index();
        let start = {
            let segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
            segs.iter().rev().find(|s| s.start <= r).map_or(FrameIndex::default(), |s| s.start)
        };
        let earliest = self.read_high.load(Ordering::Acquire).saturating_sub(self.history).max(start).min(r);
        let to = index.clamp(earliest, r);
        self.read.store(to, Ordering::Release);
        self.skip_to.store(to.min(self.skip_to.load(Ordering::Relaxed)), Ordering::Release);
        to
    }

    /// The frame being heard at media time `pos`, in the current segment.
    pub fn index_of(&self, pos: f64) -> Option<FrameIndex> {
        let r = self.read_index();
        let segs = self.segments.lock().unwrap_or_else(|e| e.into_inner());
        let s = segs.iter().rev().find(|s| s.start <= r)?;
        Some(s.start + self.format.frames_in((pos - s.pts) / s.speed))
    }
}

/// What the output callback does each cycle: fill `out` (interleaved,
/// whole frames) from the ring, silence after what there is, and anchor
/// the clock to what's heard. `latency` is the frames between the next
/// frame written and the speaker; `now` the cycle's time. Returns the
/// frames of audio (not silence) written.
pub fn fill(consumer: &mut Consumer, clock: &Clock, out: &mut [f32], latency: Frames, now: MonotonicNs) -> Frames {
    let ring = &consumer.lease.ring;
    // Busy, then halted: whoever halts the ring and then waits while it's
    // busy sees this cycle, or this cycle sees the halt (sequentially
    // consistent, as neither may pass the other).
    ring.busy.store(true, Ordering::SeqCst);
    struct ActiveRead<'a>(&'a AtomicBool);
    impl Drop for ActiveRead<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let _active = ActiveRead(&ring.busy);
    if ring.halted.load(Ordering::SeqCst) {
        out.fill(0.0);
        return Frames::ZERO;
    }
    let (frames, first) = ring.pop(out);
    out[frames.samples(ring.format.channels())..].fill(0.0);
    if let Some(pts) = ring.pts_of(first, false) {
        let speed = ring.speed_of_quick(first);
        // What's heard now is `latency` frames before the first written;
        // run dry, the clock stops there.
        let pos = pts - ring.format.secs(latency) * speed;
        clock.set(Anchor { pos, at: now, rate: if frames > Frames::ZERO { speed } else { 0.0 } }, false);
    }
    frames
}

impl Ring {
    /// `speed_of` without waiting for the lock (the callback's): 1 if
    /// it's held.
    fn speed_of_quick(&self, index: FrameIndex) -> f64 {
        match self.segments.try_lock() {
            Ok(segs) => segs.iter().rev().find(|s| s.start <= index).map_or(1.0, |s| s.speed),
            Err(_) => 1.0,
        }
    }
}

/// Where decoded audio goes.
pub trait Output: Send {
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
    playback: Option<Playback>,
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
        NullOutput { run: None, thread: None, playback: None }
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
        let (mut consumer, playback) = ring.consumer()?;
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
                    let frames = Frames(u64::from(fmt.rate()) * NULL_CYCLE_MS / 1000);
                    let mut buf = vec![0.0f32; frames.samples(fmt.channels())];
                    let cycle = std::time::Duration::from_millis(NULL_CYCLE_MS);
                    let mut next = std::time::Instant::now();
                    while !r.stop.load(Ordering::Acquire) {
                        if !r.playing.load(Ordering::Acquire) {
                            let g = r.m.lock().unwrap_or_else(|e| e.into_inner());
                            let _ = r.cv.wait_timeout(g, std::time::Duration::from_millis(200));
                            next = std::time::Instant::now();
                            continue;
                        }
                        fill(&mut consumer, &r.clock, &mut buf, Frames::ZERO, monotonic_now());
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
        self.playback = Some(playback);
        self.run = Some(run);
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) {
        let Some(r) = &self.run else { return };
        if !playing && r.playing.load(Ordering::Acquire) {
            r.playing.store(false, Ordering::Release);
            // A cycle that already saw it playing takes nothing now, and
            // leaves the clock alone: stopped below, it stays stopped.
            self.playback.as_mut().unwrap().pause();
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
            self.playback.as_mut().unwrap().resume();
        }
        r.playing.store(playing, Ordering::Release);
        r.cv.notify_all();
    }

    fn flush(&mut self) {
        if let Some(p) = &mut self.playback {
            p.pause().discard_pending();
            if self.run.as_ref().is_some_and(|r| r.playing.load(Ordering::Acquire)) {
                p.resume();
            }
        }
    }

    fn set_volume(&mut self, _volume: f32, _muted: bool) {}

    fn close(&mut self) {
        if let Some(r) = self.run.take() {
            r.stop.store(true, Ordering::Release);
            r.cv.notify_all();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.playback = None;
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

    fn ring() -> Arc<Ring> {
        Arc::new(Ring::new(Format::new(NonZeroU32::new(100).unwrap(), Channels::new(2).unwrap())))
    }

    #[test]
    fn what_goes_in_comes_out_in_order() {
        let r = ring();
        let samples: Vec<f32> = (0..20).map(|i| i as f32).collect();
        assert_eq!(r.push(&samples), Frames(10));
        assert_eq!(r.buffered(), Frames(10));
        let mut out = [0.0; 8];
        assert_eq!(r.pop(&mut out), (Frames(4), FrameIndex(0)));
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(r.buffered(), Frames(6));
    }

    #[test]
    fn the_producer_leaves_the_history_alone() {
        let r = ring();
        // 4 seconds of room, one kept behind the consumer: 3 free.
        assert_eq!(r.space(), Frames(300));
        let big = vec![1.0; 2 * 400];
        assert_eq!(r.push(&big), Frames(300));
        let mut out = vec![0.0; 2 * 100];
        r.pop(&mut out);
        assert_eq!(r.space(), Frames(100));
    }

    #[test]
    fn media_times_follow_segments() {
        let r = ring();
        r.flush(10.0, 1.0);
        {
            let (_, mut control) = r.consumer().unwrap();
            control.pause().discard_pending();
        }
        r.push(&vec![0.0; 2 * 50]);
        assert_eq!(r.pts_of(FrameIndex(0), true), Some(10.0));
        assert_eq!(r.pts_of(FrameIndex(25), true), Some(10.25));
        r.flush(30.0, 2.0);
        {
            let (_, mut control) = r.consumer().unwrap();
            control.pause().discard_pending();
        }
        assert_eq!(r.read_index(), FrameIndex(50));
        r.push(&[0.0; 2 * 10]);
        assert_eq!(r.pts_of(FrameIndex(60), true), Some(30.2));
        assert_eq!(r.index_of(30.2), Some(FrameIndex(60)));
    }

    #[test]
    fn pausing_steps_back_to_what_was_heard() {
        let r = ring();
        r.flush(0.0, 1.0);
        {
            let (_, mut control) = r.consumer().unwrap();
            control.pause().discard_pending();
        }
        r.push(&vec![0.5; 2 * 200]);
        let mut out = vec![0.0; 2 * 150];
        r.pop(&mut out);
        // 150 taken; 40 of them still in the graph when it stopped.
        assert_eq!(r.rewind_to(FrameIndex(110)), FrameIndex(110));
        assert_eq!(r.buffered(), Frames(90));
        // Never further back than the history, nor before the segment.
        assert_eq!(r.rewind_to(FrameIndex(0)), FrameIndex(50));
        r.flush(5.0, 1.0);
        {
            let (_, mut control) = r.consumer().unwrap();
            control.pause().discard_pending();
        }
        assert_eq!(r.rewind_to(FrameIndex(0)), FrameIndex(200));
    }

    #[test]
    fn a_flush_while_the_consumer_runs_skips_at_its_next_take() {
        let r = ring();
        r.push(&vec![1.0; 2 * 50]);
        r.flush(3.0, 1.0);
        assert_eq!(r.buffered(), Frames::ZERO);
        r.push(&[2.0, 2.0]);
        let mut out = vec![0.0; 2 * 10];
        let (n, first) = r.pop(&mut out);
        assert_eq!((n, first), (Frames(1), FrameIndex(50)));
        assert_eq!(out[0], 2.0);
    }

    #[test]
    fn the_callback_anchors_the_clock_to_what_is_heard() {
        let r = ring();
        let c = Clock::new();
        r.flush(2.0, 1.0);
        {
            let (_, mut control) = r.consumer().unwrap();
            control.pause().discard_pending();
        }
        r.push(&vec![0.0; 2 * 100]);
        let mut out = vec![1.0; 2 * 30];
        let (mut consumer, mut control) = r.consumer().unwrap();
        control.resume();
        // 20 frames between the first written and the speaker.
        assert_eq!(fill(&mut consumer, &c, &mut out, Frames(20), MonotonicNs::from_nanos(1_000)), Frames(30));
        let a = c.anchor();
        assert!((a.pos - 1.8).abs() < 1e-9, "{a:?}");
        assert_eq!((a.at, a.rate), (MonotonicNs::from_nanos(1_000), 1.0));
        // Run dry: silence, and the clock stops.
        r.pop(&mut vec![0.0; 2 * 100]);
        let mut out = vec![1.0; 2 * 10];
        assert_eq!(fill(&mut consumer, &c, &mut out, Frames::ZERO, MonotonicNs::from_nanos(2_000)), Frames::ZERO);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(c.anchor().rate, 0.0);
    }

    #[test]
    fn the_null_output_plays_on_a_halted_ring_and_pausing_stops_its_clock() {
        // As when the player falls back to it from a sink that took
        // nothing: that output halted the ring as it stopped.
        let r = Arc::new(Ring::new(Format::new(NonZeroU32::new(1000).unwrap(), Channels::new(2).unwrap())));
        let c = Arc::new(Clock::new());
        r.flush(0.0, 1.0);
        {
            let (_, mut control) = r.consumer().unwrap();
            control.pause().discard_pending();
        }
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
    fn an_output_lease_is_unique_until_both_halves_are_dropped() {
        let p = Producer::new(ring().format());
        let shared = p.shared();
        let (reader, control) = shared.consumer().unwrap();
        assert!(shared.consumer().is_err());
        drop(reader);
        assert!(shared.consumer().is_err());
        drop(control);
        assert!(shared.consumer().is_ok());
    }

    #[test]
    fn typed_endpoints_keep_samples_ordered_across_threads_and_wraparound() {
        let format = Format::new(NonZeroU32::new(64).unwrap(), Channels::new(1).unwrap());
        let mut producer = Producer::new(format);
        let (mut consumer, mut control) = producer.shared().consumer().unwrap();
        control.resume();
        let writer = std::thread::spawn(move || {
            for i in 1..=20_000 {
                while producer.push(&[i as f32]) == Frames::ZERO {
                    std::thread::yield_now();
                }
            }
        });
        let clock = Clock::new();
        let mut expected = 1;
        let mut samples = [0.0; 31];
        while expected <= 20_000 {
            let count = fill(&mut consumer, &clock, &mut samples, Frames::ZERO, MonotonicNs::default());
            for sample in &samples[..count.samples(format.channels())] {
                assert_eq!(*sample, expected as f32);
                expected += 1;
            }
            if count == Frames::ZERO {
                std::thread::yield_now();
            }
        }
        writer.join().unwrap();
    }

    #[test]
    fn stopped_rewinds_cannot_walk_back_beyond_reserved_history() {
        let mut producer = Producer::new(ring().format());
        let shared = producer.shared();
        let (mut consumer, mut control) = shared.consumer().unwrap();
        producer.push(&vec![0.5; 400]);
        control.resume();
        assert_eq!(
            fill(&mut consumer, &Clock::new(), &mut [0.0; 300], Frames::ZERO, MonotonicNs::default()),
            Frames(150)
        );
        let mut stopped = control.pause();
        assert_eq!(stopped.rewind_to(FrameIndex(110)), FrameIndex(110));
        assert_eq!(stopped.rewind_to(FrameIndex(0)), FrameIndex(50));
        assert_eq!(stopped.rewind_to(FrameIndex(0)), FrameIndex(50));
        producer.flush(10.0, 1.0);
        stopped.discard_pending();
        assert_eq!(shared.buffered(), Frames::ZERO);
        producer.push(&[0.75, 0.75]);
        control.resume();
        let mut out = [0.0; 2];
        assert_eq!(fill(&mut consumer, &Clock::new(), &mut out, Frames::ZERO, MonotonicNs::default()), Frames(1));
        assert_eq!(out, [0.75, 0.75]);
    }

    #[test]
    fn formats_reject_unsupported_channel_counts() {
        assert!(Channels::new(0).is_none());
        assert!(Channels::new(9).is_none());
        assert_eq!(Channels::new(8).unwrap().get(), 8);
    }

    #[test]
    fn volume_is_perceptual() {
        assert_eq!(amplitude(1.0), 1.0);
        assert_eq!(amplitude(0.5), 0.125);
        assert_eq!(amplitude(2.0), 1.0);
    }
}
