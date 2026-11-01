//! A game's sound: resampled from the core's rate to 48 kHz and played
//! through a PipeWire stream of short cycles (about 11 ms), so a jump
//! sounds when it's pressed.
//!
//! The session paces frames by the clock; the sound card's clock drifts
//! from it. So the resampling ratio leans a little (at most half a per
//! cent, which no ear hears) toward keeping about 60 ms buffered: never
//! running dry, never falling behind.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use pipewire as pw;
use pw::spa;

pub const RATE: u32 = 48_000;
/// Frames kept buffered.
pub const TARGET: usize = (RATE as usize) * 60 / 1000;
/// The cycle asked of the graph.
const QUANTUM: u32 = 512;

/// Stereo samples between the session and PipeWire's thread: one writer,
/// one reader, no locks.
pub struct Ring {
    buf: Box<[AtomicU32]>,
    read: AtomicUsize,
    write: AtomicUsize,
}

impl Ring {
    /// Room for `frames` stereo frames.
    pub fn new(frames: usize) -> Ring {
        Ring {
            buf: (0..frames * 2).map(|_| AtomicU32::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    /// Stereo frames waiting.
    pub fn frames(&self) -> usize {
        // The reader's first: it never passes the writer, so read before
        // write can't be ahead of it.
        let r = self.read.load(Ordering::Acquire);
        (self.write.load(Ordering::Acquire) - r) / 2
    }

    /// Add interleaved samples, as many as fit: how many did.
    pub fn push(&self, samples: &[f32]) -> usize {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        let room = self.buf.len() - (w - r);
        let n = samples.len().min(room) & !1;
        for (i, s) in samples[..n].iter().enumerate() {
            self.buf[(w + i) % self.buf.len()].store(s.to_bits(), Ordering::Relaxed);
        }
        self.write.store(w + n, Ordering::Release);
        n
    }

    /// Take interleaved samples into `out`, silence for what's missing:
    /// how many were real.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let r = self.read.load(Ordering::Relaxed);
        let w = self.write.load(Ordering::Acquire);
        let n = out.len().min(w - r) & !1;
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o = f32::from_bits(self.buf[(r + i) % self.buf.len()].load(Ordering::Relaxed));
        }
        out[n..].fill(0.0);
        self.read.store(r + n, Ordering::Release);
        n
    }

    /// Drop what's waiting (pausing).
    pub fn clear(&self) {
        let w = self.write.load(Ordering::Acquire);
        self.read.store(w, Ordering::Release);
    }
}

/// Linear resampling of stereo 16-bit samples to [`RATE`].
#[derive(Debug)]
pub struct Resampler {
    /// Input frames per output frame.
    step: f64,
    pos: f64,
    prev: [f32; 2],
}

impl Resampler {
    pub fn new(from: f64) -> Resampler {
        Resampler { step: from / f64::from(RATE), pos: 0.0, prev: [0.0; 2] }
    }

    pub fn set_rate(&mut self, from: f64) {
        self.step = from / f64::from(RATE);
    }

    /// Resample `input` (interleaved), with the rate leaned by `lean`
    /// (1.0 for none; more makes more output), appending to `out`.
    pub fn run(&mut self, input: &[i16], lean: f64, out: &mut Vec<f32>) {
        let frames = input.len() / 2;
        if frames == 0 || self.step <= 0.0 {
            return;
        }
        let step = self.step / lean;
        let at = |i: isize, c: usize, prev: &[f32; 2]| -> f32 {
            if i < 0 { prev[c] } else { f32::from(input[i as usize * 2 + c]) / 32768.0 }
        };
        // `pos` counts from the frame before this input (the last one of
        // the previous call), so interpolation spans calls.
        while self.pos < frames as f64 {
            let i = self.pos.floor() as isize - 1;
            let t = (self.pos - self.pos.floor()) as f32;
            for c in 0..2 {
                let a = at(i, c, &self.prev);
                let b = at(i + 1, c, &self.prev);
                out.push(a + (b - a) * t);
            }
            self.pos += step;
        }
        self.pos -= frames as f64;
        self.prev = [at(frames as isize - 1, 0, &self.prev), at(frames as isize - 1, 1, &self.prev)];
    }
}

/// How much to lean the rate with `buffered` frames waiting.
pub fn lean(buffered: usize) -> f64 {
    let off = (TARGET as f64 - buffered as f64) / TARGET as f64;
    1.0 + (off * 0.005).clamp(-0.005, 0.005)
}

enum Cmd {
    Active(bool),
    Volume(f32),
    Quit,
}

/// Where the sound goes.
pub struct Speaker {
    pub ring: Arc<Ring>,
    tx: Option<pw::channel::Sender<Cmd>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Speaker {
    /// Nowhere (tests, and no PipeWire): what's pushed is dropped.
    pub fn silent() -> Speaker {
        Speaker { ring: Arc::new(Ring::new(RATE as usize / 2)), tx: None, thread: None }
    }

    /// A PipeWire stream named for `media`.
    pub fn connect(media: &str) -> Result<Speaker, String> {
        let ring = Arc::new(Ring::new(RATE as usize / 2));
        let (tx, rx) = pw::channel::channel::<Cmd>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let r = ring.clone();
        let media = media.to_owned();
        let thread = std::thread::Builder::new()
            .name("chanoma-sound".into())
            .spawn(move || run(rx, r, media, ready_tx))
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Speaker { ring, tx: Some(tx), thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("PipeWire's thread stopped".into()),
        }
    }

    /// Add resampled sound; with no stream it's dropped.
    pub fn push(&self, samples: &[f32]) {
        if self.tx.is_some() {
            self.ring.push(samples);
        }
    }

    pub fn buffered(&self) -> usize {
        if self.tx.is_some() { self.ring.frames() } else { TARGET }
    }

    pub fn set_active(&self, on: bool) {
        if !on {
            self.ring.clear();
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Active(on));
        }
    }

    pub fn set_volume(&self, v: f32) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Volume(v));
        }
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Cmd::Quit);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    rx: pw::channel::Receiver<Cmd>,
    ring: Arc<Ring>,
    media: String,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) {
    pw::init();
    let setup = || -> Result<(pw::main_loop::MainLoopRc, pw::core::CoreRc), pw::Error> {
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        Ok((mainloop, core))
    };
    let (mainloop, core) = match setup() {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(format!("PipeWire: {e}")));
            return;
        }
    };
    let stream = match open(&core, ring, &media) {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let ml = mainloop.clone();
    let s = stream.0.clone();
    let _attached = rx.attach(mainloop.loop_(), move |cmd| match cmd {
        Cmd::Active(on) => {
            if !on {
                let _ = s.flush(false);
            }
            let _ = s.set_active(on);
        }
        Cmd::Volume(v) => {
            let amp = v.clamp(0.0, 1.0).powi(3);
            let _ = s.set_control(spa::sys::SPA_PROP_channelVolumes, &[amp, amp]);
        }
        Cmd::Quit => {
            let _ = s.set_active(false);
            let _ = s.disconnect();
            ml.quit();
        }
    });
    mainloop.run();
    drop(stream);
}

fn open(
    core: &pw::core::CoreRc,
    ring: Arc<Ring>,
    media: &str,
) -> Result<(pw::stream::StreamRc, pw::stream::StreamListener<()>), String> {
    let latency = format!("{QUANTUM}/{RATE}");
    let props = pw::properties::properties! {
        "media.type" => "Audio",
        "media.category" => "Playback",
        "media.role" => "Game",
        "media.name" => media,
        "application.name" => "Chanoma",
        "application.id" => crate::APP_ID,
        "application.icon-name" => crate::APP_ID,
        "node.name" => "chanoma-game",
        "node.latency" => latency.as_str(),
        "audio.channels" => "2",
    };
    let stream = pw::stream::StreamRc::new(core.clone(), "Chanoma", props).map_err(|e| e.to_string())?;
    let stride = 2 * std::mem::size_of::<f32>();
    let listener = stream
        .add_local_listener::<()>()
        .process(move |stream, _| {
            // SAFETY: a buffer dequeued is ours until queued; its first
            // data's memory is mapped (MAP_BUFFERS), maxsize bytes.
            unsafe {
                let b = stream.dequeue_raw_buffer();
                if b.is_null() {
                    return;
                }
                let buf = (*b).buffer;
                let d = &mut *(*buf).datas;
                if d.data.is_null() {
                    stream.queue_raw_buffer(b);
                    return;
                }
                let room = d.maxsize as usize / stride;
                let want = if (*b).requested > 0 { ((*b).requested as usize).min(room) } else { room };
                let out = std::slice::from_raw_parts_mut(d.data as *mut f32, want * 2);
                ring.pop(out);
                let chunk = &mut *d.chunk;
                chunk.offset = 0;
                chunk.stride = stride as i32;
                chunk.size = (want * stride) as u32;
                (*b).size = want as u64;
                stream.queue_raw_buffer(b);
            }
        })
        .register()
        .map_err(|e| e.to_string())?;
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(RATE);
    info.set_channels(2);
    let mut position = [spa::sys::SPA_AUDIO_CHANNEL_UNKNOWN; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    info.set_position(position);
    let bytes: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(spa::pod::Object {
            type_: spa::sys::SPA_TYPE_OBJECT_Format,
            id: spa::sys::SPA_PARAM_EnumFormat,
            properties: info.into(),
        }),
    )
    .map_err(|e| format!("{e:?}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&bytes).ok_or("an audio format PipeWire won't read")?];
    stream
        .connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|e| e.to_string())?;
    Ok((stream, listener))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_order_and_pads_with_silence() {
        let r = Ring::new(4);
        assert_eq!(r.push(&[1.0, 2.0, 3.0, 4.0]), 4);
        assert_eq!(r.frames(), 2);
        let mut out = [9.0; 6];
        assert_eq!(r.pop(&mut out), 4);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 0.0, 0.0]);
        // It wraps.
        assert_eq!(r.push(&[5.0; 8]), 8);
        assert_eq!(r.push(&[6.0; 2]), 0, "full");
        r.clear();
        assert_eq!(r.frames(), 0);
    }

    #[test]
    fn resampling_makes_the_rates_number_of_frames() {
        // A second of a Super Famicom's 32,040 Hz becomes a second at 48 kHz.
        let mut rs = Resampler::new(32_040.0);
        let mut out = Vec::new();
        let input = vec![1000i16; 32_040 * 2];
        for chunk in input.chunks(534 * 2) {
            rs.run(chunk, 1.0, &mut out);
        }
        let frames = out.len() / 2;
        assert!((47_990..=48_010).contains(&frames), "{frames}");
        // A steady level stays steady across calls.
        assert!(out[200..].iter().all(|s| (s - 1000.0 / 32768.0).abs() < 1e-6));
    }

    #[test]
    fn the_rate_leans_toward_the_target_but_barely() {
        assert_eq!(lean(TARGET), 1.0);
        assert!(lean(0) > 1.0 && lean(0) <= 1.005);
        assert!(lean(TARGET * 10) >= 0.995 && lean(TARGET * 10) < 1.0);
    }
}
