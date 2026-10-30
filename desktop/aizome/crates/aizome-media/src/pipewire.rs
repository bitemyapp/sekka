//! Audio through PipeWire: a playback stream on a thread of its own,
//! taking the ring in its process callback on PipeWire's real-time
//! thread.
//!
//! The stream asks the graph for a long cycle (`node.latency`): with
//! nothing else playing, PipeWire then wakes the device, its own daemon
//! and this process a few times a second for music rather than fifty,
//! which is most of what playing audio costs when the screen is off.
//! Long cycles cost nothing in responsiveness here: pausing drops what
//! the graph holds and steps the ring back (see `audio`), and seeking
//! flushes; only volume changes wait for the next cycle. The stream says
//! its role (Movie or Music), its application and what's playing, so
//! the desktop's volume controls show it; its volume is a stream volume
//! PipeWire applies.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use pipewire as pw;
use pw::spa;

use crate::audio::{Output, Ring, amplitude, fill};
use crate::av::ChannelLayout;
use crate::clock::Clock;

/// What the stream is for: how long its cycles may be, and its role.
#[derive(Clone, Debug)]
pub struct StreamKind {
    /// Seconds a graph cycle may be (the stream's `node.latency`).
    pub latency: f64,
    /// `media.role`: "Movie" or "Music".
    pub role: &'static str,
    pub app_name: String,
    pub app_id: String,
}

enum Cmd {
    Open { ring: Arc<Ring>, clock: Arc<Clock>, media: String, kind: StreamKind, reply: mpsc::Sender<Result<(), String>> },
    Playing(bool, mpsc::Sender<()>),
    Flush,
    Volume(f32, bool),
    Close(mpsc::Sender<()>),
    Quit,
}

/// PipeWire playback. Connecting fails where there's no PipeWire.
pub struct PipeWireOutput {
    tx: pw::channel::Sender<Cmd>,
    thread: Option<std::thread::JoinHandle<()>>,
    kind: StreamKind,
    describe: Arc<Mutex<String>>,
}

impl PipeWireOutput {
    /// Connect to PipeWire (on a thread of its own): an error where it
    /// can't.
    pub fn connect(kind: StreamKind) -> Result<PipeWireOutput, String> {
        let (tx, rx) = pw::channel::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let describe = Arc::new(Mutex::new(String::from("PipeWire")));
        let d = describe.clone();
        let thread = std::thread::Builder::new()
            .name("aizome-media-audio".into())
            .spawn(move || run(rx, ready_tx, d))
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(PipeWireOutput { tx, thread: Some(thread), kind, describe }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("PipeWire's thread stopped".into()),
        }
    }

}

impl Output for PipeWireOutput {
    fn set_stream(&mut self, latency: f64, role: &'static str) {
        self.kind.latency = latency;
        self.kind.role = role;
    }

    fn open(&mut self, ring: Arc<Ring>, clock: Arc<Clock>, media: &str) -> Result<(), String> {
        let (reply, rx) = mpsc::channel();
        let cmd = Cmd::Open { ring, clock, media: media.to_owned(), kind: self.kind.clone(), reply };
        if self.tx.send(cmd).is_err() {
            return Err("PipeWire's thread stopped".into());
        }
        rx.recv().unwrap_or_else(|_| Err("PipeWire's thread stopped".into()))
    }

    fn set_playing(&mut self, playing: bool) {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::Playing(playing, reply)).is_ok() {
            let _ = rx.recv();
        }
    }

    fn flush(&mut self) {
        let _ = self.tx.send(Cmd::Flush);
    }

    fn set_volume(&mut self, volume: f32, muted: bool) {
        let _ = self.tx.send(Cmd::Volume(volume, muted));
    }

    fn close(&mut self) {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::Close(reply)).is_ok() {
            let _ = rx.recv();
        }
    }

    fn describe(&self) -> String {
        self.describe.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Drop for PipeWireOutput {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The stream as it plays, and what its callback reads.
struct Playing {
    stream: pw::stream::StreamRc,
    _listener: pw::stream::StreamListener<()>,
    ring: Arc<Ring>,
    clock: Arc<Clock>,
    channels: u32,
    active: bool,
    volume: (f32, bool),
}

fn run(rx: pw::channel::Receiver<Cmd>, ready: mpsc::Sender<Result<(), String>>, describe: Arc<Mutex<String>>) {
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
    let _ = ready.send(Ok(()));
    let current: std::rc::Rc<std::cell::RefCell<Option<Playing>>> = Default::default();
    let ml = mainloop.clone();
    let cur = current.clone();
    let _attached = rx.attach(mainloop.loop_(), move |cmd| match cmd {
        Cmd::Open { ring, clock, media, kind, reply } => {
            close(&cur);
            let r = open(&core, ring, clock, &media, &kind);
            match r {
                Ok(p) => {
                    let f = p.ring.format;
                    *describe.lock().unwrap_or_else(|e| e.into_inner()) = format!(
                        "PipeWire, {} Hz, {} channel{}, {} ms cycles",
                        f.rate,
                        f.channels,
                        if f.channels == 1 { "" } else { "s" },
                        (kind.latency * 1000.0).round()
                    );
                    *cur.borrow_mut() = Some(p);
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
        }
        Cmd::Playing(on, reply) => {
            if let Some(p) = cur.borrow_mut().as_mut() {
                set_playing(p, on);
            }
            let _ = reply.send(());
        }
        Cmd::Flush => {
            if let Some(p) = cur.borrow().as_ref() {
                let _ = p.stream.flush(false);
            }
        }
        Cmd::Volume(v, muted) => {
            if let Some(p) = cur.borrow_mut().as_mut() {
                p.volume = (v, muted);
                apply_volume(p);
            }
        }
        Cmd::Close(reply) => {
            close(&cur);
            let _ = reply.send(());
        }
        Cmd::Quit => {
            close(&cur);
            ml.quit();
        }
    });
    mainloop.run();
}

fn close(cur: &std::rc::Rc<std::cell::RefCell<Option<Playing>>>) {
    if let Some(p) = cur.borrow_mut().take() {
        let _ = p.stream.set_active(false);
        let _ = p.stream.disconnect();
    }
}

/// Channel positions for `n` channels, as PipeWire names them: the layout
/// the player's resampler mixes them to ([`ChannelLayout::mixed`], FFmpeg's
/// usual one for `n`: 2.1 for three, 4.0 for four, 5.1(back) for six),
/// channel by channel, so each is placed where FFmpeg put it.
fn positions(n: u32) -> [u32; spa::sys::SPA_AUDIO_MAX_CHANNELS as usize] {
    use crate::av::ff::AVChannel::*;
    use spa::sys::*;
    let mut p = [SPA_AUDIO_CHANNEL_UNKNOWN; SPA_AUDIO_MAX_CHANNELS as usize];
    let n = n.clamp(1, SPA_AUDIO_MAX_CHANNELS);
    let layout = ChannelLayout::mixed(n as i32);
    for (i, slot) in p.iter_mut().enumerate().take(n as usize) {
        *slot = match layout.channel(i as u32) {
            // FFmpeg's mono is a centre channel; PipeWire's is MONO.
            AV_CHAN_FRONT_CENTER if n == 1 => SPA_AUDIO_CHANNEL_MONO,
            AV_CHAN_FRONT_LEFT => SPA_AUDIO_CHANNEL_FL,
            AV_CHAN_FRONT_RIGHT => SPA_AUDIO_CHANNEL_FR,
            AV_CHAN_FRONT_CENTER => SPA_AUDIO_CHANNEL_FC,
            AV_CHAN_LOW_FREQUENCY => SPA_AUDIO_CHANNEL_LFE,
            AV_CHAN_BACK_LEFT => SPA_AUDIO_CHANNEL_RL,
            AV_CHAN_BACK_RIGHT => SPA_AUDIO_CHANNEL_RR,
            AV_CHAN_FRONT_LEFT_OF_CENTER => SPA_AUDIO_CHANNEL_FLC,
            AV_CHAN_FRONT_RIGHT_OF_CENTER => SPA_AUDIO_CHANNEL_FRC,
            AV_CHAN_BACK_CENTER => SPA_AUDIO_CHANNEL_RC,
            AV_CHAN_SIDE_LEFT => SPA_AUDIO_CHANNEL_SL,
            AV_CHAN_SIDE_RIGHT => SPA_AUDIO_CHANNEL_SR,
            AV_CHAN_TOP_CENTER => SPA_AUDIO_CHANNEL_TC,
            AV_CHAN_TOP_FRONT_LEFT => SPA_AUDIO_CHANNEL_TFL,
            AV_CHAN_TOP_FRONT_CENTER => SPA_AUDIO_CHANNEL_TFC,
            AV_CHAN_TOP_FRONT_RIGHT => SPA_AUDIO_CHANNEL_TFR,
            AV_CHAN_TOP_BACK_LEFT => SPA_AUDIO_CHANNEL_TRL,
            AV_CHAN_TOP_BACK_CENTER => SPA_AUDIO_CHANNEL_TRC,
            AV_CHAN_TOP_BACK_RIGHT => SPA_AUDIO_CHANNEL_TRR,
            AV_CHAN_WIDE_LEFT => SPA_AUDIO_CHANNEL_FLW,
            AV_CHAN_WIDE_RIGHT => SPA_AUDIO_CHANNEL_FRW,
            AV_CHAN_LOW_FREQUENCY_2 => SPA_AUDIO_CHANNEL_LFE2,
            _ => SPA_AUDIO_CHANNEL_UNKNOWN,
        };
    }
    p
}

/// Frames in a cycle of `secs` at `rate`, as a power of two (the graph's
/// quanta are).
pub fn quantum(secs: f64, rate: u32) -> u32 {
    let frames = (secs * f64::from(rate)).max(64.0);
    (frames.log2().round().exp2() as u32).clamp(64, 8192)
}

fn open(core: &pw::core::CoreRc, ring: Arc<Ring>, clock: Arc<Clock>, media: &str, kind: &StreamKind) -> Result<Playing, String> {
    let fmt = ring.format;
    let channels = fmt.channels.clamp(1, 8);
    let latency = format!("{}/{}", quantum(kind.latency, fmt.rate), fmt.rate);
    let props = pw::properties::properties! {
        "media.type" => "Audio",
        "media.category" => "Playback",
        "media.role" => kind.role,
        "media.name" => media,
        "application.name" => kind.app_name.as_str(),
        "application.id" => kind.app_id.as_str(),
        "application.icon-name" => kind.app_id.as_str(),
        "node.name" => kind.app_name.to_lowercase().as_str(),
        "node.latency" => latency.as_str(),
        "audio.channels" => channels.to_string().as_str(),
    };
    let stream = pw::stream::StreamRc::new(core.clone(), &kind.app_name, props).map_err(|e| e.to_string())?;
    let r = ring.clone();
    let c = clock.clone();
    let stride = (channels as usize) * std::mem::size_of::<f32>();
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
                let out = std::slice::from_raw_parts_mut(d.data as *mut f32, want * channels as usize);
                let (latency, now) = match stream.time() {
                    Ok(t) => {
                        let rate = t.rate();
                        let delay = if rate.denom > 0 {
                            t.delay().max(0) as f64 * f64::from(rate.num) / f64::from(rate.denom)
                        } else {
                            0.0
                        };
                        let frames = delay * f64::from(fmt.rate) + (t.buffered() + t.queued()) as f64;
                        (frames as u64, t.now().max(0) as u64)
                    }
                    Err(_) => (0, crate::clock::monotonic_now()),
                };
                fill(&r, &c, out, latency, now);
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
    info.set_rate(fmt.rate);
    info.set_channels(channels);
    info.set_position(positions(channels));
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
                | pw::stream::StreamFlags::RT_PROCESS
                | pw::stream::StreamFlags::INACTIVE,
            &mut params,
        )
        .map_err(|e| e.to_string())?;
    let mut p = Playing { stream, _listener: listener, ring, clock, channels, active: false, volume: (1.0, false) };
    apply_volume(&mut p);
    Ok(p)
}

fn apply_volume(p: &mut Playing) {
    let (v, muted) = p.volume;
    let amp = amplitude(v);
    let volumes = vec![amp; p.channels as usize];
    let _ = p.stream.set_control(spa::sys::SPA_PROP_channelVolumes, &volumes);
    let _ = p.stream.set_control(spa::sys::SPA_PROP_mute, &[if muted { 1.0 } else { 0.0 }]);
}

fn set_playing(p: &mut Playing, on: bool) {
    if on == p.active {
        return;
    }
    p.active = on;
    if on {
        p.ring.halted.store(false, std::sync::atomic::Ordering::Release);
        let _ = p.stream.set_active(true);
        return;
    }
    // Stop, then step back to what was being heard: what the graph held
    // goes, and plays again from the ring.
    let heard = p.clock.now();
    p.ring.halted.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = p.stream.set_active(false);
    while p.ring.busy.load(std::sync::atomic::Ordering::SeqCst) {
        std::hint::spin_loop();
    }
    let _ = p.stream.flush(false);
    let to = p.ring.index_of(heard).map(|i| p.ring.rewind_to(i));
    let pos = to.and_then(|i| p.ring.pts_of(i, true)).unwrap_or(heard);
    p.clock.stop_at(pos);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_are_powers_of_two_within_the_graphs_limits() {
        assert_eq!(quantum(0.170, 48_000), 8192);
        assert_eq!(quantum(0.085, 48_000), 4096);
        assert_eq!(quantum(0.040, 48_000), 2048);
        assert_eq!(quantum(10.0, 48_000), 8192);
        assert_eq!(quantum(0.0, 48_000), 64);
    }

    #[test]
    fn channels_are_placed_where_ffmpeg_puts_them() {
        use spa::sys::*;
        const FL: u32 = SPA_AUDIO_CHANNEL_FL;
        const FR: u32 = SPA_AUDIO_CHANNEL_FR;
        const FC: u32 = SPA_AUDIO_CHANNEL_FC;
        const LFE: u32 = SPA_AUDIO_CHANNEL_LFE;
        const RL: u32 = SPA_AUDIO_CHANNEL_RL;
        const RR: u32 = SPA_AUDIO_CHANNEL_RR;
        const RC: u32 = SPA_AUDIO_CHANNEL_RC;
        const SL: u32 = SPA_AUDIO_CHANNEL_SL;
        const SR: u32 = SPA_AUDIO_CHANNEL_SR;
        // FFmpeg's default layouts, which the resampler mixes to.
        let want: [&[u32]; 8] = [
            &[SPA_AUDIO_CHANNEL_MONO],
            &[FL, FR],
            &[FL, FR, LFE],                     // 2.1
            &[FL, FR, FC, RC],                  // 4.0
            &[FL, FR, FC, RL, RR],              // 5.0(back)
            &[FL, FR, FC, LFE, RL, RR],         // 5.1(back)
            &[FL, FR, FC, LFE, RC, SL, SR],     // 6.1
            &[FL, FR, FC, LFE, RL, RR, SL, SR], // 7.1
        ];
        for (n, want) in (1..).zip(want) {
            let p = positions(n);
            assert_eq!(&p[..n as usize], want, "{n} channels");
            assert!(p[n as usize..].iter().all(|&c| c == SPA_AUDIO_CHANNEL_UNKNOWN), "{n} channels");
        }
    }
}
