//! The video thread: decodes the demuxer's packets a few frames ahead,
//! and shows each frame at its time on the clock through the window's
//! video sink.
//!
//! **Frames as they are.** A hardware decoder's frame (VA-API) is a GPU
//! surface; it's exported once as a dmabuf (FFmpeg's DRM PRIME mapping,
//! which also waits for the decoder to finish writing it) and shown as is
//! — the compositor imports each of the decoder's surfaces once and can
//! scan the video out on a plane. The frame is kept until the compositor
//! releases it, so the decoder never writes a surface on screen. A
//! software decoder's frame is converted once, by swscale, into one of a
//! few shared-memory buffers the compositor shows; without dmabufs the
//! compositor will take (a software renderer, or a format it can't
//! import), a hardware frame is downloaded and converted the same way.
//!
//! **Timing.** Frames go a few milliseconds before they're due, carrying
//! the time they're for (wp_commit_timing_v1), half a refresh early so
//! each lands on the refresh nearest it. A frame already late by half
//! its duration is dropped; when they keep coming late, the decoder
//! skips frames nothing refers to.
//!
//! **Out of sight.** A frame the compositor didn't show is discarded; a
//! run of them means the window is out of sight (minimised, on another
//! Desk, covered), as does the window saying so. The thread then stops
//! decoding, and the demuxer drops the video's packets: audio plays on,
//! and video costs nothing. Once a second it shows its last frame again;
//! when that shows, or the window says it's back, the demuxer goes back
//! to the keyframe before now and the thread decodes up to the clock,
//! showing nothing before.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use sidestep_ui::video::{Dmabuf, DmabufPlane, ShmBuffer, Shown, VideoBuffer, VideoCaps, VideoFrame, VideoSink};

use crate::av::{self, Decoder, Frame, Packet, ff};
use crate::clock::{Clock, monotonic_now};

/// Frames decoded ahead.
const AHEAD: usize = 4;
/// Packets queued for the decoder before the demuxer stops reading for
/// it, and below which it reads again.
pub const PACKETS_HIGH: usize = 96;
pub const PACKETS_LOW: usize = 32;
/// How long before its time a frame goes to the compositor.
const LEAD_NS: u64 = 12_000_000;
/// A run of frames discarded this long (and this many) is the window out
/// of sight.
const HIDDEN_RUN: u32 = 8;
const HIDDEN_NS: u64 = 400_000_000;
/// How often an out-of-sight video shows its last frame again, to see.
const PROBE: Duration = Duration::from_secs(1);

/// What the demuxer and the controller tell the video thread.
pub enum Cmd {
    /// A stream to decode, from the start or a seek.
    Start { decoder: Decoder, info: StreamInfo },
    /// After a seek: drop what's decoded and queued; show nothing before
    /// `from`; show the first frame at once if `poster`.
    Flush { generation: u64, from: f64, poster: bool },
    /// Show the next frame, paused.
    Step,
    /// The picture showing, as straight RGBA at its display size, for a
    /// copy or a file: none if nothing shows.
    Snapshot(Box<dyn FnOnce(Option<Picture>) + Send>),
    /// No video any more.
    Stop,
}

/// A picture taken from the video: straight RGBA, `width` × `height`.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What the video thread needs to know of its stream.
#[derive(Clone, Copy, Debug)]
pub struct StreamInfo {
    /// Seconds a frame lasts, nominally.
    pub frame_duration: f64,
    /// Pixel aspect (sample aspect ratio), 1 for square pixels.
    pub pixel_aspect: f64,
}

/// Shared with the demuxer and the controller.
pub struct Shared {
    pub q: Mutex<Queue>,
    pub cv: Condvar,
    /// Out of sight: the demuxer drops video packets.
    pub suspended: AtomicBool,
    /// The window says it's out of sight.
    pub window_hidden: AtomicBool,
    /// The video thread wants the demuxer to go back for it (back in
    /// sight).
    pub resync: AtomicBool,
    /// Every frame of the stream has been shown (or dropped).
    pub drained: AtomicBool,
    /// The GPU decoder failed before its first frame: the player opens the
    /// stream again in software.
    pub hw_failed: AtomicBool,
    /// The generation of the latest flush whose first frame showed.
    pub shown_generation: AtomicU64,
    /// The time of the last frame shown, in media seconds (f64 bits).
    pub last_pts: AtomicU64,
    pub stats: Stats,
    /// Wakes the demuxer when the packets run low.
    pub wake_demuxer: Arc<crate::player::Wake>,
}

#[derive(Default)]
pub struct Queue {
    pub packets: VecDeque<Packet>,
    /// The demuxer has given the last packet.
    pub eof: bool,
    pub cmds: VecDeque<Cmd>,
    pub generation: u64,
    /// Something the video thread may be waiting for happened (packets,
    /// the last of them, the clock starting) since it last waited: its
    /// next wait doesn't sleep through it.
    pub woken: bool,
}

/// What the video thread did, for the info window.
#[derive(Default)]
pub struct Stats {
    pub shown: AtomicU64,
    pub dropped: AtomicU64,
    pub decoded: AtomicU64,
    /// Frames the compositor showed and discarded.
    pub presented: AtomicU64,
    pub discarded: AtomicU64,
    /// How frames reach the compositor: 0 none yet, 1 dmabuf, 2 shared
    /// memory (decoded in software), 3 shared memory (downloaded from the
    /// GPU).
    pub path: AtomicU32,
    /// The display's refresh, nanoseconds.
    pub refresh: AtomicU32,
    /// Times decoding stopped because the window was out of sight.
    pub suspensions: AtomicU64,
    /// The last frame was decoded on the GPU.
    pub gpu: AtomicBool,
}

impl Shared {
    pub fn new(wake_demuxer: Arc<crate::player::Wake>) -> Arc<Shared> {
        Arc::new(Shared {
            q: Mutex::new(Queue::default()),
            cv: Condvar::new(),
            suspended: AtomicBool::new(false),
            window_hidden: AtomicBool::new(false),
            resync: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            hw_failed: AtomicBool::new(false),
            shown_generation: AtomicU64::new(u64::MAX),
            last_pts: AtomicU64::new(0f64.to_bits()),
            stats: Stats::default(),
            wake_demuxer,
        })
    }

    pub fn send(&self, cmd: Cmd) {
        self.q.lock().unwrap_or_else(|e| e.into_inner()).cmds.push_back(cmd);
        self.cv.notify_all();
    }

    pub fn poke(&self) {
        self.q.lock().unwrap_or_else(|e| e.into_inner()).woken = true;
        self.cv.notify_all();
    }

    /// The video thread's wait, for a command, a poke or `timeout`. A poke
    /// that came while it was busy (decoding, between looking for work and
    /// waiting) counts: it looks again rather than sleep, so the last
    /// packets, or the clock starting, aren't slept through.
    fn wait(&self, timeout: Option<Duration>) {
        let mut q = self.q.lock().unwrap_or_else(|e| e.into_inner());
        if q.cmds.is_empty() && !q.woken {
            q = match timeout {
                Some(t) => self.cv.wait_timeout(q, t).unwrap_or_else(|e| e.into_inner()).0,
                None => self.cv.wait(q).unwrap_or_else(|e| e.into_inner()),
            };
        }
        // It looks at everything again now.
        q.woken = false;
    }
}

/// How frames reached the screen, from the compositor's answers.
#[derive(Default)]
struct Sight {
    presented: AtomicU64,
    discarded_run: AtomicU32,
    last_presented: AtomicU64,
}

/// A software frame's buffers: a memfd of a few frames, mapped here.
struct ShmPool {
    map: *mut u8,
    size: usize,
    width: u32,
    height: u32,
    stride: u32,
    slots: Vec<(Arc<VideoBuffer>, Arc<AtomicBool>)>,
}

// SAFETY: the mapping is written by the video thread alone, into slots
// the compositor has released.
unsafe impl Send for ShmPool {}

impl Drop for ShmPool {
    fn drop(&mut self) {
        // SAFETY: mapped by `ShmPool::new`, this size.
        unsafe { libc::munmap(self.map.cast(), self.size) };
    }
}

impl ShmPool {
    const SLOTS: usize = 4;

    fn new(width: u32, height: u32) -> Option<ShmPool> {
        let stride = width * 4;
        let frame = stride as usize * height as usize;
        let size = frame * Self::SLOTS;
        // SAFETY: a new anonymous file, sized and mapped here.
        unsafe {
            let fd = libc::memfd_create(c"aizome-video".as_ptr(), libc::MFD_CLOEXEC);
            if fd < 0 {
                return None;
            }
            let owned = <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd);
            if libc::ftruncate(fd, size as libc::off_t) != 0 {
                return None;
            }
            let map = libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            if map == libc::MAP_FAILED {
                return None;
            }
            let mut slots = Vec::with_capacity(Self::SLOTS);
            for i in 0..Self::SLOTS {
                let dup = std::os::fd::AsFd::as_fd(&owned).try_clone_to_owned().ok()?;
                let buffer = Arc::new(VideoBuffer::Shm(ShmBuffer {
                    fd: dup,
                    size,
                    offset: (frame * i) as u32,
                    width,
                    height,
                    stride,
                    // wl_shm's XRGB8888.
                    format: 1,
                }));
                slots.push((buffer, Arc::new(AtomicBool::new(false))));
            }
            Some(ShmPool { map: map.cast(), size, width, height, stride, slots })
        }
    }

    /// A slot the compositor isn't reading, marked busy until its guard
    /// drops.
    fn take(&self) -> Option<(usize, Arc<VideoBuffer>, SlotGuard)> {
        self.slots.iter().enumerate().find_map(|(i, (b, busy))| {
            busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .ok()
                .map(|_| (i, b.clone(), SlotGuard(busy.clone())))
        })
    }
}

struct SlotGuard(Arc<AtomicBool>);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Converts software frames to XRGB8888.
struct Scaler {
    ctx: *mut ff::SwsContext,
}

// SAFETY: used by the video thread alone.
unsafe impl Send for Scaler {}

impl Scaler {
    fn new() -> Scaler {
        // SAFETY: allocation; configured from the frames at each scale.
        let ctx = unsafe { ff::sws_alloc_context() };
        if !ctx.is_null() {
            // SAFETY: a fresh context's public fields.
            unsafe {
                (*ctx).flags = ff::SwsFlags::SWS_BILINEAR as u32;
                (*ctx).threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(8)) as i32;
            }
        }
        Scaler { ctx }
    }

    /// Convert `src` into `dst`'s memory, `width` × `height` XRGB8888 at
    /// `stride`.
    fn convert(&mut self, src: &Frame, dst: *mut u8, width: u32, height: u32, stride: u32) -> bool {
        if self.ctx.is_null() {
            return false;
        }
        let out = Frame::new();
        // SAFETY: an empty frame pointing into the slot's memory (not
        // reference-counted: sws writes it, nobody frees it).
        unsafe {
            let p = out.0;
            let o = &mut *p;
            o.format = ff::AVPixelFormat::AV_PIX_FMT_BGR0 as i32;
            o.width = width as i32;
            o.height = height as i32;
            o.data[0] = dst;
            o.linesize[0] = stride as i32;
            o.colorspace = ff::AVColorSpace::AVCOL_SPC_RGB;
            o.color_range = ff::AVColorRange::AVCOL_RANGE_JPEG;
            let r = ff::sws_scale_frame(self.ctx, p, src.0);
            o.data[0] = std::ptr::null_mut();
            r >= 0
        }
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: allocated by sws_alloc_context.
        unsafe { ff::sws_free_context(&mut self.ctx) };
    }
}

/// DRM fourccs FFmpeg's layers come in.
const R8: u32 = sidestep_ui::video::fourcc(b"R8  ");
const GR88: u32 = sidestep_ui::video::fourcc(b"GR88");
const R16: u32 = sidestep_ui::video::fourcc(b"R16 ");
const GR1616: u32 = sidestep_ui::video::fourcc(b"GR32");

/// Export a VA-API frame's surface as one dmabuf in the format the
/// compositor takes (NV12 or P010, from FFmpeg's one layer or its two:
/// luma and chroma).
fn export(frame: &Frame) -> Option<Dmabuf> {
    let f = frame.get();
    if f.hw_frames_ctx.is_null() {
        return None;
    }
    // SAFETY: a hardware frame's frames context; a mapped frame is
    // freed here, its descriptor read before.
    unsafe {
        let hw = &*((*f.hw_frames_ctx).data as *const ff::AVHWFramesContext);
        let mut mapped = Frame::new();
        mapped.get_mut().format = ff::AVPixelFormat::AV_PIX_FMT_DRM_PRIME as i32;
        if ff::av_hwframe_map(mapped.0, frame.0, ff::AV_HWFRAME_MAP_READ as i32) < 0 {
            return None;
        }
        let desc = &*(mapped.get().data[0] as *const ff::AVDRMFrameDescriptor);
        let layers = &desc.layers[..desc.nb_layers.clamp(0, 4) as usize];
        let fourcc = match layers.iter().map(|l| l.format).collect::<Vec<_>>().as_slice() {
            [single] => *single,
            [R8, GR88] => sidestep_ui::video::NV12,
            [R16, GR1616] => sidestep_ui::video::P010,
            _ => return None,
        };
        let mut planes = Vec::new();
        let mut modifier = None;
        for l in layers {
            for p in &l.planes[..l.nb_planes.clamp(0, 4) as usize] {
                let obj = desc.objects.get(p.object_index as usize)?;
                if modifier.is_some_and(|m| m != obj.format_modifier) {
                    return None;
                }
                modifier = Some(obj.format_modifier);
                let fd = std::os::fd::BorrowedFd::borrow_raw(obj.fd).try_clone_to_owned().ok()?;
                planes.push(DmabufPlane { fd, offset: p.offset as u32, stride: p.pitch as u32 });
            }
        }
        Some(Dmabuf {
            width: hw.width as u32,
            height: hw.height as u32,
            fourcc,
            modifier: modifier?,
            planes,
        })
    }
}

/// Wait for a hardware frame's surface to be written (FFmpeg's mapping
/// syncs it), without exporting what's known.
fn sync_surface(frame: &Frame) -> bool {
    let mut mapped = Frame::new();
    mapped.get_mut().format = ff::AVPixelFormat::AV_PIX_FMT_DRM_PRIME as i32;
    // SAFETY: valid frames; the mapping is dropped at once.
    unsafe { ff::av_hwframe_map(mapped.0, frame.0, ff::AV_HWFRAME_MAP_READ as i32) >= 0 }
}

/// How the thread shows its frames.
enum Path {
    Unknown,
    Dmabuf,
    Shm,
}

struct State {
    shared: Arc<Shared>,
    sink: VideoSink,
    caps: VideoCaps,
    clock: Arc<Clock>,
    decoder: Option<Decoder>,
    info: StreamInfo,
    frames: VecDeque<(f64, Frame)>,
    /// Show nothing earlier than this (a precise seek, or catching up).
    from: f64,
    poster: bool,
    generation: u64,
    steps: u32,
    /// The decoder has been given the last packet.
    draining: bool,
    path: Path,
    /// Exported surfaces, by frames context and surface.
    surfaces: HashMap<(usize, usize), Arc<VideoBuffer>>,
    frames_ctx: usize,
    pool: Option<ShmPool>,
    scaler: Scaler,
    sight: Arc<Sight>,
    /// The last frame shown (another reference to the decoder's), kept to
    /// show again.
    last: Option<Frame>,
    suspended: bool,
    /// Suspended because the window said it was out of sight (not seen).
    by_window: bool,
    late_run: u32,
    skipping: bool,
    /// Decoding errors since the stream started with no frame yet.
    failures: u32,
    any_frame: bool,
}

/// Run the video thread until `Cmd::Stop`.
pub fn run(shared: Arc<Shared>, sink: VideoSink, caps: VideoCaps, clock: Arc<Clock>) {
    let mut s = State {
        shared,
        sink,
        caps,
        clock,
        decoder: None,
        info: StreamInfo { frame_duration: 1.0 / 30.0, pixel_aspect: 1.0 },
        frames: VecDeque::new(),
        from: f64::NEG_INFINITY,
        poster: true,
        generation: 0,
        steps: 0,
        draining: false,
        path: Path::Unknown,
        surfaces: HashMap::new(),
        frames_ctx: 0,
        pool: None,
        scaler: Scaler::new(),
        sight: Arc::new(Sight::default()),
        last: None,
        suspended: false,
        by_window: false,
        late_run: 0,
        skipping: false,
        failures: 0,
        any_frame: false,
    };
    loop {
        if !s.commands() {
            break;
        }
        if s.decoder.is_none() {
            s.wait(None);
            continue;
        }
        if s.check_sight() {
            continue;
        }
        s.decode_ahead();
        let Some(&(pts, _)) = s.frames.front() else {
            let done = s.draining && s.shared.q.lock().unwrap_or_else(|e| e.into_inner()).packets.is_empty();
            if done {
                s.shared.drained.store(true, Ordering::Release);
            }
            s.wait(None);
            continue;
        };
        if s.poster || s.steps > 0 {
            s.poster = false;
            s.steps = s.steps.saturating_sub(1);
            let (pts, frame) = s.frames.pop_front().expect("a frame");
            s.present(pts, frame, None);
            continue;
        }
        let anchor = s.clock.anchor();
        let Some(due) = anchor.time_of(pts) else {
            // Stopped (paused, or not yet started): wait to be told.
            s.wait(None);
            continue;
        };
        let now = monotonic_now();
        let half = (s.info.frame_duration / anchor.rate.max(0.01) * 0.5e9) as u64;
        if due + half < now {
            s.frames.pop_front();
            s.shared.stats.dropped.fetch_add(1, Ordering::Relaxed);
            s.late(true);
            continue;
        }
        if due > now + LEAD_NS {
            s.wait(Some(Duration::from_nanos(due - now - LEAD_NS)));
            continue;
        }
        s.late(false);
        let refresh = u64::from(s.shared.stats.refresh.load(Ordering::Relaxed));
        let at = due.saturating_sub(refresh / 2);
        let (pts, frame) = s.frames.pop_front().expect("a frame");
        s.present(pts, frame, Some(at));
    }
    s.sink.clear();
}

impl State {
    /// Take the commands: false to stop.
    fn commands(&mut self) -> bool {
        let cmds: Vec<Cmd> = self.shared.q.lock().unwrap_or_else(|e| e.into_inner()).cmds.drain(..).collect();
        for cmd in cmds {
            match cmd {
                Cmd::Start { decoder, info } => {
                    self.decoder = Some(decoder);
                    self.last = None;
                    self.info = info;
                    self.frames.clear();
                    self.surfaces.clear();
                    self.frames_ctx = 0;
                    self.path = Path::Unknown;
                    self.draining = false;
                    self.poster = true;
                    self.from = f64::NEG_INFINITY;
                    self.suspended = false;
                    self.shared.suspended.store(false, Ordering::Release);
                    self.shared.drained.store(false, Ordering::Release);
                    self.failures = 0;
                    self.any_frame = false;
                }
                Cmd::Flush { generation, from, poster } => {
                    if let Some(d) = &mut self.decoder {
                        d.flush();
                    }
                    self.frames.clear();
                    self.generation = generation;
                    self.from = from;
                    self.poster = poster;
                    self.draining = false;
                    self.steps = 0;
                    self.late_run = 0;
                    self.shared.drained.store(false, Ordering::Release);
                }
                Cmd::Step => self.steps += 1,
                Cmd::Snapshot(reply) => reply(self.snapshot()),
                Cmd::Stop => return false,
            }
        }
        true
    }

    /// Woken by packets, commands and the clock starting; a decoder short
    /// of frames waits only for packets.
    fn wait(&self, timeout: Option<Duration>) {
        self.shared.wait(timeout);
    }

    /// Keep `AHEAD` frames decoded, while there are packets.
    fn decode_ahead(&mut self) {
        while self.frames.len() < AHEAD {
            let (pkt, eof, low) = {
                let mut q = self.shared.q.lock().unwrap_or_else(|e| e.into_inner());
                if !q.cmds.is_empty() {
                    return;
                }
                let p = q.packets.pop_front();
                (p, q.eof, q.packets.len() < PACKETS_LOW)
            };
            if low && !eof {
                self.shared.wake_demuxer.wake();
            }
            let Some(dec) = self.decoder.as_mut() else { return };
            match &pkt {
                Some(p) => {
                    if let Err(e) = dec.send(Some(p))
                        && !e.is_again()
                    {
                        log::debug!("video packet: {e}");
                        self.failures += 1;
                    }
                }
                None if eof && !self.draining => {
                    self.draining = true;
                    let _ = dec.send(None);
                }
                None => {}
            }
            let mut got = false;
            loop {
                let mut frame = Frame::new();
                match dec.receive(&mut frame) {
                    Ok(true) => {
                        got = true;
                        self.any_frame = true;
                        self.shared.stats.decoded.fetch_add(1, Ordering::Relaxed);
                        let tb = dec.time_base;
                        let Some(pts) = frame.time(tb) else { continue };
                        let dur = self.info.frame_duration;
                        if pts + dur * 0.5 < self.from {
                            continue;
                        }
                        self.frames.push_back((pts, frame));
                    }
                    Ok(false) => break,
                    Err(e) => {
                        log::debug!("video frame: {e}");
                        self.failures += 1;
                        break;
                    }
                }
            }
            // A GPU decoder that only fails (a codec the GPU lacks, with a
            // decoder that can't fall back by itself): software instead.
            if !self.any_frame && self.failures >= 4 && !dec.get().hw_device_ctx.is_null() {
                log::info!("the GPU can't decode this: decoding in software");
                self.failures = 0;
                self.shared.hw_failed.store(true, Ordering::Release);
                self.shared.wake_demuxer.wake();
                return;
            }
            if pkt.is_none() && !got {
                return;
            }
        }
    }

    /// Frames keep coming late: skip what nothing refers to until they
    /// don't.
    fn late(&mut self, late: bool) {
        self.late_run = if late { self.late_run + 1 } else { self.late_run.saturating_sub(1) };
        let skip = self.late_run >= 4;
        if skip != self.skipping
            && let Some(d) = &self.decoder
        {
            self.skipping = skip;
            // SAFETY: the decoder's own field, read at each packet.
            unsafe {
                (*d.ctx).skip_frame =
                    if skip { ff::AVDiscard::AVDISCARD_NONREF } else { ff::AVDiscard::AVDISCARD_DEFAULT };
            }
        }
    }

    /// Show a frame (at `at`, or now).
    fn present(&mut self, pts: f64, frame: Frame, at: Option<u64>) {
        let again = frame.share();
        let Some((buffer, crop, keep)) = self.prepare(frame) else {
            self.shared.stats.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let mut vf = VideoFrame::new(buffer.clone());
        vf.crop = crop;
        vf.at = at;
        let sight = self.sight.clone();
        let shared = self.shared.clone();
        vf.shown = Some(Box::new(move |shown| match shown {
            Shown::Presented { time, refresh } => {
                sight.presented.fetch_add(1, Ordering::Relaxed);
                sight.discarded_run.store(0, Ordering::Relaxed);
                sight.last_presented.store(time.max(1), Ordering::Relaxed);
                shared.stats.presented.fetch_add(1, Ordering::Relaxed);
                if refresh > 0 {
                    shared.stats.refresh.store(refresh, Ordering::Relaxed);
                }
            }
            Shown::Discarded => {
                sight.discarded_run.fetch_add(1, Ordering::Relaxed);
                shared.stats.discarded.fetch_add(1, Ordering::Relaxed);
            }
        }));
        vf.keep = Some(Box::new(keep));
        self.sink.show(vf);
        self.last = Some(again);
        self.shared.stats.shown.fetch_add(1, Ordering::Relaxed);
        self.shared.last_pts.store(pts.to_bits(), Ordering::Release);
        if self.shared.shown_generation.swap(self.generation, Ordering::AcqRel) != self.generation {
            self.shared.wake_demuxer.wake();
        }
    }

    /// A frame's buffer for the compositor, the picture's part of it, and
    /// what to keep while it's shown.
    fn prepare(&mut self, frame: Frame) -> Option<(Arc<VideoBuffer>, [f32; 4], Keep)> {
        let f = frame.get();
        let crop = [
            f.crop_left as f32,
            f.crop_top as f32,
            (f.width as usize - f.crop_left - f.crop_right) as f32,
            (f.height as usize - f.crop_top - f.crop_bottom) as f32,
        ];
        let hw = f.format == ff::AVPixelFormat::AV_PIX_FMT_VAAPI as i32;
        self.shared.stats.gpu.store(hw, Ordering::Relaxed);
        if hw && !matches!(self.path, Path::Shm) {
            // SAFETY: a hardware frame's frames context and surface id.
            let (ctx, surface) = unsafe { ((*f.hw_frames_ctx).data as usize, f.data[3] as usize) };
            if ctx != self.frames_ctx {
                self.frames_ctx = ctx;
                self.surfaces.clear();
            }
            if let Some(b) = self.surfaces.get(&(ctx, surface)).cloned() {
                return sync_surface(&frame).then_some((b, crop, Keep::Frame(frame)));
            }
            match export(&frame).filter(|d| self.caps.takes_dmabuf(d.fourcc, d.modifier)) {
                Some(d) => {
                    let b = Arc::new(VideoBuffer::Dmabuf(d));
                    self.surfaces.insert((ctx, surface), b.clone());
                    self.path = Path::Dmabuf;
                    self.shared.stats.path.store(1, Ordering::Relaxed);
                    return Some((b, crop, Keep::Frame(frame)));
                }
                None => {
                    log::info!("the compositor can't take this decoder's surfaces: copying frames");
                    self.path = Path::Shm;
                    self.surfaces.clear();
                }
            }
        }
        // Software: download a hardware frame first.
        let sw = if hw {
            let s = Frame::new();
            // SAFETY: valid frames.
            if unsafe { ff::av_hwframe_transfer_data(s.0, frame.0, 0) } < 0 {
                return None;
            }
            // SAFETY: both valid; copies times and colour.
            unsafe { ff::av_frame_copy_props(s.0, frame.0) };
            self.shared.stats.path.store(3, Ordering::Relaxed);
            s
        } else {
            self.shared.stats.path.store(2, Ordering::Relaxed);
            frame
        };
        let (w, h) = (crop[2] as u32, crop[3] as u32);
        if self.pool.as_ref().is_none_or(|p| (p.width, p.height) != (w, h)) {
            self.pool = ShmPool::new(w, h);
        }
        let pool = self.pool.as_ref()?;
        let (i, buffer, guard) = pool.take()?;
        let frame_bytes = pool.stride as usize * pool.height as usize;
        // SAFETY: slot `i` is ours until its guard drops.
        let dst = unsafe { pool.map.add(frame_bytes * i) };
        let (stride, height) = (pool.stride, pool.height);
        if !self.scaler.convert(&sw, dst, w, height, stride) {
            return None;
        }
        Some((buffer, [0.0, 0.0, w as f32, h as f32], Keep::Slot(guard)))
    }

    /// Notice the window going out of sight or coming back: true while
    /// out of sight (the loop goes round again, decoding nothing).
    fn check_sight(&mut self) -> bool {
        let told_hidden = self.shared.window_hidden.load(Ordering::Acquire);
        if !self.suspended {
            let run = self.sight.discarded_run.load(Ordering::Relaxed);
            let last = self.sight.last_presented.load(Ordering::Relaxed);
            let unseen = run >= HIDDEN_RUN && monotonic_now().saturating_sub(last) > HIDDEN_NS;
            if (told_hidden || unseen) && self.clock.running() && self.seekable() {
                self.suspend(told_hidden);
                return true;
            }
            return false;
        }
        if told_hidden {
            // Until the window says it's back (which wakes this thread).
            self.wait(Some(PROBE));
            return true;
        }
        if !self.by_window {
            // Seen out of sight: show the last frame again now and then,
            // until it shows.
            let seen = self.sight.presented.load(Ordering::Relaxed);
            if let Some((b, crop, keep)) = self.last.as_ref().map(Frame::share).and_then(|f| self.prepare(f)) {
                let mut vf = VideoFrame::new(b);
                vf.crop = crop;
                vf.keep = Some(Box::new(keep));
                let sight = self.sight.clone();
                vf.shown = Some(Box::new(move |s| {
                    if let Shown::Presented { time, .. } = s {
                        sight.presented.fetch_add(1, Ordering::Relaxed);
                        sight.last_presented.store(time.max(1), Ordering::Relaxed);
                    }
                }));
                self.sink.show(vf);
                self.wait(Some(PROBE));
                if self.sight.presented.load(Ordering::Relaxed) == seen {
                    return true;
                }
            }
        }
        // Back in sight: the demuxer goes back to the keyframe before now,
        // and its flush says from where to show.
        log::debug!("video back in sight: catching up");
        self.suspended = false;
        self.sight.discarded_run.store(0, Ordering::Relaxed);
        self.sight.last_presented.store(monotonic_now(), Ordering::Relaxed);
        self.shared.resync.store(true, Ordering::Release);
        self.shared.wake_demuxer.wake();
        self.wait(Some(PROBE));
        true
    }

    /// The last frame shown, downloaded if it's on the GPU and converted to
    /// RGBA at its display size (its pixel aspect applied).
    fn snapshot(&self) -> Option<Picture> {
        let frame = self.last.as_ref()?.share();
        let f = frame.get();
        let src = if f.format == ff::AVPixelFormat::AV_PIX_FMT_VAAPI as i32 {
            let s = Frame::new();
            // SAFETY: valid frames.
            if unsafe { ff::av_hwframe_transfer_data(s.0, frame.0, 0) } < 0 {
                return None;
            }
            // SAFETY: both valid; copies times and colour.
            unsafe { ff::av_frame_copy_props(s.0, frame.0) };
            s
        } else {
            frame
        };
        let (w, h) = (src.get().width, src.get().height);
        let width = ((f64::from(w) * self.info.pixel_aspect).round() as i32).max(1);
        let mut out = Frame::new();
        // SAFETY: a new RGBA frame with its own buffer, scaled into by a
        // context of its own, then read row by row.
        unsafe {
            let o = out.get_mut();
            o.format = ff::AVPixelFormat::AV_PIX_FMT_RGBA as i32;
            o.width = width;
            o.height = h;
            o.colorspace = ff::AVColorSpace::AVCOL_SPC_RGB;
            o.color_range = ff::AVColorRange::AVCOL_RANGE_JPEG;
            if ff::av_frame_get_buffer(out.0, 0) < 0 {
                return None;
            }
            let mut ctx = ff::sws_alloc_context();
            if ctx.is_null() {
                return None;
            }
            (*ctx).flags = ff::SwsFlags::SWS_BICUBIC as u32;
            let r = ff::sws_scale_frame(ctx, out.0, src.0);
            ff::sws_free_context(&mut ctx);
            if r < 0 {
                return None;
            }
            let o = out.get();
            let row = width as usize * 4;
            let mut rgba = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                rgba.extend_from_slice(std::slice::from_raw_parts(o.data[0].add(y * o.linesize[0] as usize), row));
            }
            Some(Picture { width: width as u32, height: h as u32, rgba })
        }
    }

    fn seekable(&self) -> bool {
        self.shared.wake_demuxer.seekable.load(Ordering::Relaxed)
    }

    fn suspend(&mut self, by_window: bool) {
        log::debug!("video out of sight: decoding stops");
        self.suspended = true;
        self.by_window = by_window;
        self.shared.stats.suspensions.fetch_add(1, Ordering::Relaxed);
        self.shared.suspended.store(true, Ordering::Release);
        self.frames.clear();
        if let Some(d) = &mut self.decoder {
            d.flush();
        }
        let mut q = self.shared.q.lock().unwrap_or_else(|e| e.into_inner());
        q.packets.clear();
    }
}

/// What a frame on screen keeps, until the compositor releases it (held
/// for its drop).
enum Keep {
    /// The decoder's frame (its surface).
    Frame(#[allow(dead_code)] Frame),
    /// A shared-memory slot.
    Slot(#[allow(dead_code)] SlotGuard),
}

/// The frame duration of a stream, from its rates.
pub fn frame_duration(stream: &ff::AVStream) -> f64 {
    av::ratio(stream.avg_frame_rate)
        .or_else(|| av::ratio(stream.r_frame_rate))
        .filter(|r| *r > 1.0 && *r < 1000.0)
        .map_or(1.0 / 30.0, |r| 1.0 / r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poke_while_the_video_thread_is_busy_isnt_slept_through() {
        // Poked after it looked for work and before it waits (the last
        // packets came while it was decoding, say): it doesn't sleep.
        let shared = Shared::new(crate::player::Wake::new());
        shared.poke();
        let s = shared.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            s.wait(None);
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "the wait slept through the poke");
        // Seen once: the next wait sleeps until it's woken again.
        assert!(!shared.q.lock().unwrap().woken);
        let s = shared.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            s.wait(None);
            let _ = tx.send(());
        });
        std::thread::sleep(Duration::from_millis(50));
        shared.poke();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "a poke didn't wake the wait");
    }
}
