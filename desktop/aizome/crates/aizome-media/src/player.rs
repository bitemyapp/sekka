//! The player: one thread that demuxes, decodes audio and carries out
//! the interface's commands, the video thread (`video`), and the audio
//! output (`pipewire`, or `audio::NullOutput`).
//!
//! **In bursts.** The demuxer reads ahead until the audio ring holds two
//! seconds and the video has a few seconds of packets, then sleeps until
//! the ring is down to one or the video thread runs low: about once a
//! second while playing, where a player decoding as it goes wakes for
//! every packet. Paused, nothing runs.
//!
//! **Starting.** After opening or seeking, playing starts once the video
//! has shown its first frame and the ring has a few hundred milliseconds:
//! the audio output starts, and its callback starts the clock the video
//! follows. Without audio the player runs the clock itself.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;

use sidestep_ui::video::{VideoCaps, VideoSink};

use crate::audio::{Format, NullOutput, Output, Ring};
use crate::av::{self, Decoder, Frame, Input, Packet, ff};
use crate::clock::Clock;
use crate::subs::{self, Cue};
use crate::video;

/// The ring's fill: refilled below `LOW`, up to `HIGH` seconds.
const HIGH: f64 = 2.0;
const LOW: f64 = 1.0;
/// Audio buffered before playing starts.
const PREROLL: f64 = 0.3;
/// Compressed audio read ahead at most, in seconds.
const AUDIO_PACKETS_SECS: f64 = 30.0;

/// Wakes the player's thread: commands, and the threads it feeds.
pub struct Wake {
    flag: Mutex<bool>,
    cv: Condvar,
    /// The open media can seek (for the video thread's catching up).
    pub seekable: AtomicBool,
}

impl Wake {
    fn new() -> Arc<Wake> {
        Arc::new(Wake { flag: Mutex::new(false), cv: Condvar::new(), seekable: AtomicBool::new(false) })
    }

    pub fn wake(&self) {
        *self.flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }

    fn wait(&self, timeout: Option<Duration>) {
        let mut woken = self.flag.lock().unwrap_or_else(|e| e.into_inner());
        if !*woken {
            woken = match timeout {
                Some(t) => self.cv.wait_timeout(woken, t).unwrap_or_else(|e| e.into_inner()).0,
                None => self.cv.wait(woken).unwrap_or_else(|e| e.into_inner()),
            };
        }
        *woken = false;
    }
}

/// Where the player sends audio.
pub enum AudioChoice {
    /// PipeWire, falling back to no sound where it can't connect.
    PipeWire(crate::pipewire::StreamKind),
    /// Nowhere, paced as a device would be (tests).
    Null,
}

pub struct Options {
    /// Where video frames go: none plays audio only.
    pub sink: Option<VideoSink>,
    pub audio: AudioChoice,
    /// Decode video on the GPU where it can (VA-API).
    pub hardware: bool,
    /// What the compositor takes; none: asked of the toolkit (which knows
    /// once it's shown a window).
    pub caps: Option<VideoCaps>,
}

/// What's open.
#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub url: String,
    pub path: Option<PathBuf>,
    /// The title it carries, else the file's name without its extension.
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub duration: Option<f64>,
    pub chapters: Vec<Chapter>,
    pub video: Option<VideoInfo>,
    pub audio: Vec<Track>,
    pub subtitles: Vec<Track>,
    /// The cover picture it carries (JPEG or PNG bytes).
    pub cover: Option<Arc<Vec<u8>>>,
    pub seekable: bool,
    pub format: String,
    pub bit_rate: i64,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Its size as shown (pixel aspect applied).
    pub display: (f64, f64),
    pub fps: Option<f64>,
    /// High dynamic range (PQ or HLG), which shows as standard range.
    pub hdr: bool,
    pub bit_depth: u32,
}

/// An audio or subtitle track.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// Its stream in the media, or none for a file beside it.
    pub stream: Option<usize>,
    pub file: Option<PathBuf>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub codec: String,
    pub channels: Option<u32>,
    pub default: bool,
    pub forced: bool,
}

impl Track {
    /// "English · 5.1 · E-AC-3", "Commentary (English)", "Track 2".
    pub fn label(&self, n: usize) -> String {
        let lang = self.language.as_deref().map(language_name);
        let mut parts = Vec::new();
        match (&self.title, &lang) {
            (Some(t), Some(l)) if !t.contains(l.as_str()) => parts.push(format!("{t} ({l})")),
            (Some(t), _) => parts.push(t.clone()),
            (None, Some(l)) => parts.push(l.clone()),
            (None, None) => parts.push(format!("Track {n}")),
        }
        if let Some(c) = self.channels {
            parts.push(match c {
                1 => "Mono".into(),
                2 => "Stereo".into(),
                6 => "5.1".into(),
                8 => "7.1".into(),
                c => format!("{c} channels"),
            });
        }
        if self.forced {
            parts.push("Forced".into());
        }
        parts.join(" · ")
    }
}

/// A language's English name from its ISO 639 code, where it's common.
pub fn language_name(code: &str) -> String {
    let name = match code.to_ascii_lowercase().as_str() {
        "en" | "eng" => "English",
        "ja" | "jpn" => "Japanese",
        "fr" | "fre" | "fra" => "French",
        "de" | "ger" | "deu" => "German",
        "es" | "spa" => "Spanish",
        "it" | "ita" => "Italian",
        "pt" | "por" => "Portuguese",
        "ru" | "rus" => "Russian",
        "zh" | "chi" | "zho" => "Chinese",
        "ko" | "kor" => "Korean",
        "nl" | "dut" | "nld" => "Dutch",
        "sv" | "swe" => "Swedish",
        "no" | "nor" => "Norwegian",
        "da" | "dan" => "Danish",
        "fi" | "fin" => "Finnish",
        "pl" | "pol" => "Polish",
        "cs" | "cze" | "ces" => "Czech",
        "tr" | "tur" => "Turkish",
        "ar" | "ara" => "Arabic",
        "he" | "heb" => "Hebrew",
        "hi" | "hin" => "Hindi",
        "el" | "gre" | "ell" => "Greek",
        "hu" | "hun" => "Hungarian",
        "uk" | "ukr" => "Ukrainian",
        "vi" | "vie" => "Vietnamese",
        "th" | "tha" => "Thai",
        "id" | "ind" => "Indonesian",
        "und" => return "Unknown".into(),
        other => return other.to_owned(),
    };
    name.into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Paused,
    Playing,
    Ended,
}

/// How the media is being decoded and shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pipeline {
    /// "h264 on the GPU (VA-API)", "hevc in software, 8 threads".
    pub video_decoder: Option<String>,
    pub audio_decoder: Option<String>,
    pub audio_output: String,
}

/// What the player tells the interface.
#[derive(Clone, Debug)]
pub enum Event {
    Opened(Arc<MediaInfo>),
    /// It couldn't be opened or played.
    Failed(String),
    State(PlayState),
    /// A seek landed (its first frame shows, or its audio is ready).
    Seeked(f64),
    /// Cues of the subtitle track shown (embedded tracks' as they're
    /// read; a file's all at once).
    Cues(Vec<Cue>),
    /// Forget the cues (a seek, another track).
    ClearCues,
    Pipeline(Pipeline),
    /// The tracks playing changed.
    Selected { audio: Option<usize>, subtitles: Option<usize> },
}

enum Command {
    Open { url: String, start: f64, play: bool },
    Play,
    Pause,
    Seek { to: f64, precise: bool },
    Step(bool),
    Speed(f64),
    Volume(f32, bool),
    Audio(Option<usize>),
    Subtitles(Option<usize>),
    AddSubtitles(PathBuf),
    Visible(bool),
    Close,
    Quit,
}

/// A player. Commands return at once; what they do is heard as events.
pub struct Player {
    tx: mpsc::Sender<Command>,
    wake: Arc<Wake>,
    clock: Arc<Clock>,
    video: Arc<video::Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    pub fn new(options: Options, events: impl Fn(Event) + Send + Sync + 'static) -> Player {
        av::quiet();
        let (tx, rx) = mpsc::channel();
        let wake = Wake::new();
        let clock = Arc::new(Clock::new());
        let video = video::Shared::new(wake.clone());
        let v = video.clone();
        clock.on_start(move || v.poke());
        let ctl = Ctl::new(options, rx, wake.clone(), clock.clone(), video.clone(), Arc::new(events));
        let thread = std::thread::Builder::new()
            .name("aizome-media".into())
            .spawn(move || ctl.run())
            .expect("aizome-media: couldn't start the player's thread");
        Player { tx, wake, clock, video, thread: Some(thread) }
    }

    fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
        self.wake.wake();
    }

    /// Open a file (or URL) and play it from `start` seconds, or show its
    /// first frame paused.
    pub fn open(&self, url: impl Into<String>, start: f64, play: bool) {
        self.send(Command::Open { url: url.into(), start, play });
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Go to `to` seconds: exactly, or to the keyframe before (quick, for
    /// dragging along the timeline).
    pub fn seek(&self, to: f64, precise: bool) {
        self.send(Command::Seek { to, precise });
    }

    /// One frame forward or back (pausing).
    pub fn step(&self, forward: bool) {
        self.send(Command::Step(forward));
    }

    pub fn set_speed(&self, speed: f64) {
        self.send(Command::Speed(speed.clamp(0.25, 4.0)));
    }

    pub fn set_volume(&self, volume: f32, muted: bool) {
        self.send(Command::Volume(volume, muted));
    }

    /// Play audio track `i` of the media's (none: no sound).
    pub fn select_audio(&self, i: Option<usize>) {
        self.send(Command::Audio(i));
    }

    /// Show subtitle track `i` (none: none).
    pub fn select_subtitles(&self, i: Option<usize>) {
        self.send(Command::Subtitles(i));
    }

    /// Add a subtitle file and show it.
    pub fn add_subtitles(&self, path: PathBuf) {
        self.send(Command::AddSubtitles(path));
    }

    /// The picture showing (straight RGBA at its display size), to `reply`
    /// on the video thread: none without one.
    pub fn snapshot(&self, reply: impl FnOnce(Option<video::Picture>) + Send + 'static) {
        self.video.send(video::Cmd::Snapshot(Box::new(reply)));
    }

    /// Whether the window shows the video: out of sight, no video is
    /// decoded.
    pub fn set_visible(&self, visible: bool) {
        self.video.window_hidden.store(!visible, Ordering::Release);
        self.video.poke();
        self.send(Command::Visible(visible));
    }

    pub fn close(&self) {
        self.send(Command::Close);
    }

    /// Where playback is now, in seconds.
    pub fn position(&self) -> f64 {
        self.clock.now()
    }

    /// The clock itself, to read without asking.
    pub fn clock(&self) -> Arc<Clock> {
        self.clock.clone()
    }

    /// Counts of what the video thread did.
    pub fn stats(&self) -> Stats {
        let s = &self.video.stats;
        Stats {
            shown: s.shown.load(Ordering::Relaxed),
            dropped: s.dropped.load(Ordering::Relaxed),
            decoded: s.decoded.load(Ordering::Relaxed),
            presented: s.presented.load(Ordering::Relaxed),
            discarded: s.discarded.load(Ordering::Relaxed),
            path: match s.path.load(Ordering::Relaxed) {
                1 => FramePath::Dmabuf,
                2 => FramePath::Software,
                3 => FramePath::Downloaded,
                _ => FramePath::None,
            },
            refresh_hz: match s.refresh.load(Ordering::Relaxed) {
                0 => None,
                ns => Some(1e9 / f64::from(ns)),
            },
            suspensions: s.suspensions.load(Ordering::Relaxed),
            gpu: s.gpu.load(Ordering::Relaxed),
            suspended: self.video.suspended.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.send(Command::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// How frames reach the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePath {
    None,
    /// Decoded on the GPU and shown as they are.
    Dmabuf,
    /// Decoded in software, converted once into shared memory.
    Software,
    /// Decoded on the GPU, copied back (the compositor can't take them).
    Downloaded,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub shown: u64,
    pub dropped: u64,
    pub decoded: u64,
    pub presented: u64,
    pub discarded: u64,
    pub path: FramePath,
    pub refresh_hz: Option<f64>,
    /// Times video decoding stopped with the window out of sight, and
    /// whether it is stopped now.
    pub suspensions: u64,
    pub suspended: bool,
    /// The last frame was decoded on the GPU.
    pub gpu: bool,
}

/// Audio at a speed: libavfilter's `atempo`, which keeps the pitch.
struct Tempo {
    graph: *mut ff::AVFilterGraph,
    src: *mut ff::AVFilterContext,
    sink: *mut ff::AVFilterContext,
}

// SAFETY: used by the player's thread alone.
unsafe impl Send for Tempo {}

impl Tempo {
    fn new(speed: f64, f: &ff::AVFrame) -> Option<Tempo> {
        let mut layout = [0 as libc::c_char; 128];
        // SAFETY: describing the frame's layout into the buffer.
        unsafe { ff::av_channel_layout_describe(&f.ch_layout, layout.as_mut_ptr(), layout.len()) };
        // SAFETY: the buffer was written and ended above.
        let layout = unsafe { av::text(layout.as_ptr()) }.unwrap_or_else(|| format!("{}c", f.ch_layout.nb_channels));
        // SAFETY: a sample format's name.
        let fmt = unsafe { av::text(ff::av_get_sample_fmt_name(std::mem::transmute::<i32, ff::AVSampleFormat>(f.format))) }?;
        let args = std::ffi::CString::new(format!(
            "time_base=1/{r}:sample_rate={r}:sample_fmt={fmt}:channel_layout={layout}",
            r = f.sample_rate
        ))
        .ok()?;
        // atempo takes 0.5–100: two halve.
        let chain = if speed < 0.5 { format!("atempo=0.5,atempo={}", speed / 0.5) } else { format!("atempo={speed}") };
        let chain = std::ffi::CString::new(chain).ok()?;
        // SAFETY: building a graph; each failure frees it.
        unsafe {
            let graph = ff::avfilter_graph_alloc();
            let mut t = Tempo { graph, src: std::ptr::null_mut(), sink: std::ptr::null_mut() };
            let abuffer = ff::avfilter_get_by_name(c"abuffer".as_ptr());
            let abuffersink = ff::avfilter_get_by_name(c"abuffersink".as_ptr());
            if ff::avfilter_graph_create_filter(&mut t.src, abuffer, c"in".as_ptr(), args.as_ptr(), std::ptr::null_mut(), graph) < 0
                || ff::avfilter_graph_create_filter(&mut t.sink, abuffersink, c"out".as_ptr(), std::ptr::null(), std::ptr::null_mut(), graph)
                    < 0
            {
                return None;
            }
            let mut outputs = ff::avfilter_inout_alloc();
            let mut inputs = ff::avfilter_inout_alloc();
            (*outputs).name = ff::av_strdup(c"in".as_ptr());
            (*outputs).filter_ctx = t.src;
            (*inputs).name = ff::av_strdup(c"out".as_ptr());
            (*inputs).filter_ctx = t.sink;
            let r = ff::avfilter_graph_parse_ptr(graph, chain.as_ptr(), &mut inputs, &mut outputs, std::ptr::null_mut());
            ff::avfilter_inout_free(&mut inputs);
            ff::avfilter_inout_free(&mut outputs);
            if r < 0 || ff::avfilter_graph_config(graph, std::ptr::null_mut()) < 0 {
                return None;
            }
            Some(t)
        }
    }

    fn push(&mut self, frame: &Frame) -> bool {
        // AV_BUFFERSRC_FLAG_KEEP_REF (8): the frame stays ours.
        // SAFETY: a valid graph and frame.
        unsafe { ff::av_buffersrc_add_frame_flags(self.src, frame.0, 8) >= 0 }
    }

    fn pull(&mut self, out: &mut Frame) -> bool {
        out.unref();
        // SAFETY: a valid graph and frame.
        unsafe { ff::av_buffersink_get_frame(self.sink, out.0) >= 0 }
    }
}

impl Drop for Tempo {
    fn drop(&mut self) {
        // SAFETY: allocated by avfilter_graph_alloc; frees its filters.
        unsafe { ff::avfilter_graph_free(&mut self.graph) };
    }
}

/// Converts decoded audio to the ring's f32, interleaved.
struct Resampler {
    swr: *mut ff::SwrContext,
    from: (i32, i32, i32),
    channels: i32,
    rate: i32,
}

// SAFETY: used by the player's thread alone.
unsafe impl Send for Resampler {}

impl Resampler {
    fn new() -> Resampler {
        Resampler { swr: std::ptr::null_mut(), from: (-1, 0, 0), channels: 0, rate: 0 }
    }

    /// Append `frame`'s samples to `out`, as `channels` channels.
    fn convert(&mut self, frame: &Frame, channels: i32, out: &mut Vec<f32>) -> bool {
        let f = frame.get();
        let from = (f.format, f.sample_rate, f.ch_layout.nb_channels);
        // SAFETY: (re)configuring for the frame's format; converting its
        // samples into room for all of them.
        unsafe {
            if self.swr.is_null() || from != self.from || channels != self.channels {
                ff::swr_free(&mut self.swr);
                let mut out_layout: ff::AVChannelLayout = std::mem::zeroed();
                ff::av_channel_layout_default(&mut out_layout, channels);
                let r = ff::swr_alloc_set_opts2(
                    &mut self.swr,
                    &out_layout,
                    ff::AVSampleFormat::AV_SAMPLE_FMT_FLT,
                    f.sample_rate,
                    &f.ch_layout,
                    std::mem::transmute::<i32, ff::AVSampleFormat>(f.format),
                    f.sample_rate,
                    0,
                    std::ptr::null_mut(),
                );
                ff::av_channel_layout_uninit(&mut out_layout);
                if r < 0 || ff::swr_init(self.swr) < 0 {
                    ff::swr_free(&mut self.swr);
                    return false;
                }
                self.from = from;
                self.channels = channels;
                self.rate = f.sample_rate;
            }
            let room = ff::swr_get_out_samples(self.swr, f.nb_samples).max(f.nb_samples) as usize;
            let at = out.len();
            out.resize(at + room * channels as usize, 0.0);
            let dst = out.as_mut_ptr().add(at) as *mut u8;
            let n = ff::swr_convert(self.swr, &dst, room as i32, f.extended_data as *mut *const u8, f.nb_samples);
            out.truncate(at + n.max(0) as usize * channels as usize);
            n >= 0
        }
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        // SAFETY: allocated by swr_alloc_set_opts2, or null.
        unsafe { ff::swr_free(&mut self.swr) };
    }
}

/// The audio being played.
struct AudioPart {
    index: usize,
    decoder: Decoder,
    resampler: Resampler,
    tempo: Option<Tempo>,
    ring: Arc<Ring>,
    packets: VecDeque<Packet>,
    /// Samples converted but not yet in the ring.
    pending: Vec<f32>,
    /// The media time after the last sample converted.
    next_pts: f64,
    /// Drop audio before this (a precise seek, or packets read again).
    skip_until: f64,
    /// The next sample converted starts a new segment of the ring.
    rebase: bool,
    /// The decoder has been given the last packet.
    drained: bool,
}

struct VideoPart {
    index: usize,
    thread: std::thread::JoinHandle<()>,
}

struct SubPart {
    index: usize,
    decoder: Decoder,
    /// Drop cues before this (packets read again).
    skip_until: f64,
}

/// What's open, and how it's being played.
struct Session {
    input: Input,
    info: Arc<MediaInfo>,
    audio: Option<AudioPart>,
    video: Option<VideoPart>,
    subs: Option<SubPart>,
    /// Which of the media's audio and subtitle tracks play.
    audio_track: Option<usize>,
    sub_track: Option<usize>,
    eof: bool,
    /// Playing has started since the last open or seek.
    started: bool,
    /// The media time playing starts from (after the last seek).
    from: f64,
    generation: u64,
    ended: bool,
    /// A frame was stepped to: audio must catch up before playing.
    stepped: bool,
    seeked_said: bool,
}

struct Ctl {
    rx: mpsc::Receiver<Command>,
    wake: Arc<Wake>,
    clock: Arc<Clock>,
    video: Arc<video::Shared>,
    events: Arc<dyn Fn(Event) + Send + Sync>,
    sink: Option<VideoSink>,
    caps: Option<VideoCaps>,
    hardware: bool,
    hw_device: *mut ff::AVBufferRef,
    output: Box<dyn Output>,
    /// What plays the current item's audio when the output can't (it
    /// couldn't open, or nothing takes its sound).
    silent: Option<NullOutput>,
    output_name: String,
    /// How the open media is being played, as last told.
    pipeline: Pipeline,
    session: Option<Session>,
    want_play: bool,
    speed: f64,
    volume: (f32, bool),
    visible: bool,
    /// When playing last started: if the audio output takes nothing for
    /// a while after, nothing plays sound (no device), and the player
    /// plays on without it.
    started_at: Option<std::time::Instant>,
}

// SAFETY: the hardware device reference is used by this thread alone.
unsafe impl Send for Ctl {}

impl Ctl {
    fn new(
        options: Options,
        rx: mpsc::Receiver<Command>,
        wake: Arc<Wake>,
        clock: Arc<Clock>,
        video: Arc<video::Shared>,
        events: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> Ctl {
        let output: Box<dyn Output> = match options.audio {
            AudioChoice::PipeWire(kind) => match crate::pipewire::PipeWireOutput::connect(kind) {
                Ok(o) => Box::new(o),
                Err(e) => {
                    log::warn!("no sound: {e}");
                    Box::new(NullOutput::new())
                }
            },
            AudioChoice::Null => Box::new(NullOutput::new()),
        };
        let hardware = options.hardware && std::env::var("AIZOME_MEDIA_HWDEC").map_or(true, |v| v != "0");
        Ctl {
            rx,
            wake,
            clock,
            video,
            events,
            caps: options.caps,
            sink: options.sink,
            hardware,
            hw_device: std::ptr::null_mut(),
            output_name: output.describe(),
            output,
            silent: None,
            pipeline: Pipeline::default(),
            session: None,
            want_play: false,
            speed: 1.0,
            volume: (1.0, false),
            visible: true,
            started_at: None,
        }
    }

    fn emit(&self, e: Event) {
        (self.events)(e);
    }

    /// The output playing the current item's audio.
    fn out(&mut self) -> &mut dyn Output {
        match &mut self.silent {
            Some(n) => n,
            None => self.output.as_mut(),
        }
    }

    fn run(mut self) {
        // What the compositor takes, asked here, not on the interface's
        // thread: the toolkit knows once its render thread has heard.
        if self.caps.is_none() && self.sink.is_some() {
            self.caps = sidestep_ui::video::caps(Duration::from_secs(2));
        }
        loop {
            while let Ok(cmd) = self.rx.try_recv() {
                if !self.command(cmd) {
                    self.close();
                    // SAFETY: a device reference made here, or null.
                    unsafe { ff::av_buffer_unref(&mut self.hw_device) };
                    return;
                }
            }
            let timeout = self.work();
            self.wake.wait(timeout);
        }
    }

    /// Carry out a command: false to quit.
    fn command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Open { url, start, play } => {
                self.close();
                self.want_play = play;
                if let Err(e) = self.open(&url, start) {
                    self.close();
                    self.emit(Event::Failed(e));
                }
            }
            Command::Play => self.play(),
            Command::Pause => self.pause(),
            Command::Seek { to, precise } => self.seek(to, precise),
            Command::Step(forward) => self.step(forward),
            Command::Speed(s) => {
                self.speed = s;
                let pos = self.clock.now();
                if self.session.as_ref().is_some_and(|s| s.audio.is_some()) {
                    if let Some(a) = self.session.as_mut().and_then(|s| s.audio.as_mut()) {
                        a.tempo = None;
                    }
                    self.seek(pos, true);
                } else if self.clock.running() {
                    self.clock.run_from(pos, s);
                    self.video.poke();
                }
            }
            Command::Volume(v, m) => {
                self.volume = (v, m);
                self.out().set_volume(v, m);
            }
            Command::Audio(i) => self.select_audio(i),
            Command::Subtitles(i) => self.select_subtitles(i),
            Command::AddSubtitles(path) => self.add_subtitles(path),
            Command::Visible(v) => self.visible = v,
            Command::Close => self.close(),
            Command::Quit => return false,
        }
        true
    }

    /// The VA-API device on the compositor's GPU, made the first time a
    /// video plays.
    fn hw_device(&mut self) -> Option<*mut ff::AVBufferRef> {
        if !self.hardware || self.caps.as_ref().is_none_or(|c| c.dmabuf.is_empty()) {
            return None;
        }
        if self.hw_device.is_null() {
            let node = render_node(self.caps.as_ref().map_or(0, |c| c.main_device));
            let path = node.as_deref().and_then(|p| std::ffi::CString::new(p).ok());
            // SAFETY: creating a device; null leaves FFmpeg to pick.
            let r = unsafe {
                ff::av_hwdevice_ctx_create(
                    &mut self.hw_device,
                    ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                    path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
                    std::ptr::null_mut(),
                    0,
                )
            };
            if r < 0 {
                log::info!("no VA-API ({}): video decodes in software", av::Error(r));
                self.hardware = false;
                return None;
            }
        }
        Some(self.hw_device)
    }

    fn open(&mut self, url: &str, start: f64) -> Result<(), String> {
        let input = Input::open(url).map_err(|e| format!("Can't open it: {e}"))?;
        let path = url_path(url);
        let mut info = media_info(&input, url, path.clone());
        self.wake.seekable.store(info.seekable, Ordering::Release);
        // Tracks: the best audio; subtitles marked default or forced, else
        // a file beside the video.
        let best_audio = best_stream(&input, ff::AVMediaType::AVMEDIA_TYPE_AUDIO);
        let audio_track = info.audio.iter().position(|t| t.stream == best_audio).or(if info.audio.is_empty() {
            None
        } else {
            Some(0)
        });
        if let Some(p) = &path {
            for f in subs::beside(p) {
                info.subtitles.push(Track {
                    stream: None,
                    language: subs::file_language(&f, p),
                    title: None,
                    codec: f.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_uppercase(),
                    channels: None,
                    default: false,
                    forced: false,
                    file: Some(f),
                });
            }
        }
        let sub_track = info
            .subtitles
            .iter()
            .position(|t| t.forced)
            .or_else(|| info.subtitles.iter().position(|t| t.default))
            .or_else(|| info.subtitles.iter().position(|t| t.file.is_some()));
        let info = Arc::new(info);
        self.emit(Event::Opened(info.clone()));
        let mut session = Session {
            input,
            info: info.clone(),
            audio: None,
            video: None,
            subs: None,
            audio_track: None,
            sub_track: None,
            eof: false,
            started: false,
            from: 0.0,
            generation: 0,
            ended: false,
            stepped: false,
            seeked_said: false,
        };
        // Video.
        let mut pipeline = Pipeline { audio_output: self.output_name.clone(), ..Pipeline::default() };
        let vindex = best_stream(&session.input, ff::AVMediaType::AVMEDIA_TYPE_VIDEO);
        if let (Some(vi), Some(sink)) = (vindex, self.sink.clone()) {
            let hw = self.hw_device();
            let stream = session.input.stream(vi).ok_or("no video stream")?;
            let decoder = open_video(stream, hw).map_err(|e| format!("Can't decode its video: {e}"))?;
            pipeline.video_decoder = Some(describe_video(&decoder));
            let stream_info = video::StreamInfo {
                frame_duration: video::frame_duration(stream),
                pixel_aspect: av::ratio(stream.sample_aspect_ratio).unwrap_or(1.0),
            };
            {
                let mut q = self.video.q.lock().unwrap_or_else(|e| e.into_inner());
                q.packets.clear();
                q.eof = false;
                q.cmds.clear();
            }
            self.video.send(video::Cmd::Start { decoder, info: stream_info });
            let shared = self.video.clone();
            let caps = self.caps.clone().unwrap_or_default();
            let clock = self.clock.clone();
            let thread = std::thread::Builder::new()
                .name("aizome-media-video".into())
                .spawn(move || video::run(shared, sink, caps, clock))
                .map_err(|e| e.to_string())?;
            session.video = Some(VideoPart { index: vi, thread });
        }
        self.session = Some(session);
        if let Some(i) = audio_track {
            self.start_audio(i)?;
        }
        if let Some(a) = self.session.as_ref().and_then(|s| s.audio.as_ref()) {
            pipeline.audio_decoder = Some(a.decoder.codec_name());
        }
        pipeline.audio_output = self.out().describe();
        self.pipeline = pipeline.clone();
        self.emit(Event::Pipeline(pipeline));
        if let Some(i) = sub_track {
            self.start_subtitles(Some(i));
        }
        let (a, s) = self.session.as_ref().map_or((None, None), |s| (s.audio_track, s.sub_track));
        self.emit(Event::Selected { audio: a, subtitles: s });
        let start = if info.seekable && start > 0.0 { start } else { 0.0 };
        self.seek_to(start, true, true);
        Ok(())
    }

    /// Play audio track `i`: a decoder, a ring of its format, a stream.
    fn start_audio(&mut self, i: usize) -> Result<(), String> {
        let Some(s) = self.session.as_mut() else { return Ok(()) };
        let Some(index) = s.info.audio.get(i).and_then(|t| t.stream) else { return Ok(()) };
        let stream = s.input.stream(index).ok_or("no audio stream")?;
        let decoder = Decoder::open(stream, |c| c.thread_count = 1).map_err(|e| format!("Can't decode its audio: {e}"))?;
        // SAFETY: an audio stream's parameters.
        let (rate, channels) = unsafe { ((*stream.codecpar).sample_rate, (*stream.codecpar).ch_layout.nb_channels) };
        let format = Format { rate: rate.max(8000) as u32, channels: channels.clamp(1, 8) as u32 };
        let ring = Arc::new(Ring::new(format));
        let media = s.info.title.clone();
        s.audio = Some(AudioPart {
            index,
            decoder,
            resampler: Resampler::new(),
            tempo: None,
            ring: ring.clone(),
            packets: VecDeque::new(),
            pending: Vec::new(),
            next_pts: 0.0,
            skip_until: f64::NEG_INFINITY,
            rebase: true,
            drained: false,
        });
        s.audio_track = Some(i);
        let has_video = s.video.is_some();
        // Music plays in long cycles; a film's audio in shorter ones, so a
        // volume change is heard soon.
        if has_video {
            self.output.set_stream(0.040, "Movie");
        } else {
            self.output.set_stream(0.170, "Music");
        }
        self.silent = None;
        if let Err(e) = self.output.open(ring.clone(), self.clock.clone(), &media) {
            log::warn!("no sound: {e}");
            let mut null = NullOutput::new();
            null.open(ring, self.clock.clone(), &media)?;
            self.silent = Some(null);
        }
        let (v, m) = self.volume;
        self.out().set_volume(v, m);
        Ok(())
    }

    fn start_subtitles(&mut self, i: Option<usize>) {
        let Some(s) = self.session.as_mut() else { return };
        s.subs = None;
        s.sub_track = i;
        self.emit(Event::ClearCues);
        let Some(s) = self.session.as_mut() else { return };
        let Some(track) = i.and_then(|i| s.info.subtitles.get(i)).cloned() else { return };
        if let Some(index) = track.stream {
            if let Some(stream) = s.input.stream(index) {
                match Decoder::open(stream, |_| {}) {
                    Ok(decoder) => s.subs = Some(SubPart { index, decoder, skip_until: f64::NEG_INFINITY }),
                    Err(e) => log::warn!("subtitles: {e}"),
                }
            }
        } else if let Some(file) = &track.file {
            match subs::read_file(file) {
                Ok(mut cues) => {
                    subs::settle(&mut cues);
                    self.emit(Event::Cues(cues));
                }
                Err(e) => log::warn!("{}: {e}", file.display()),
            }
        }
    }

    fn select_audio(&mut self, i: Option<usize>) {
        let pos = self.clock.now();
        let Some(s) = self.session.as_mut() else { return };
        if s.audio_track == i {
            return;
        }
        s.audio = None;
        s.audio_track = None;
        self.output.close();
        self.silent = None;
        if let Some(i) = i
            && let Err(e) = self.start_audio(i)
        {
            log::warn!("{e}");
        }
        let (a, sub) = self.session.as_ref().map_or((None, None), |s| (s.audio_track, s.sub_track));
        self.emit(Event::Selected { audio: a, subtitles: sub });
        self.seek(pos, true);
    }

    fn select_subtitles(&mut self, i: Option<usize>) {
        let pos = self.clock.now();
        let Some(s) = self.session.as_ref() else { return };
        if s.sub_track == i {
            return;
        }
        let embedded = i.and_then(|i| s.info.subtitles.get(i)).is_some_and(|t| t.stream.is_some());
        self.start_subtitles(i);
        let (a, sub) = self.session.as_ref().map_or((None, None), |s| (s.audio_track, s.sub_track));
        self.emit(Event::Selected { audio: a, subtitles: sub });
        // An embedded track's cues for now were read and dropped: read again.
        if embedded {
            self.seek(pos, true);
        }
    }

    fn add_subtitles(&mut self, path: PathBuf) {
        let Some(s) = self.session.as_mut() else { return };
        let mut info = (*s.info).clone();
        info.subtitles.push(Track {
            stream: None,
            language: info.path.as_deref().and_then(|v| subs::file_language(&path, v)),
            title: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            codec: path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_uppercase(),
            channels: None,
            default: false,
            forced: false,
            file: Some(path),
        });
        let i = info.subtitles.len() - 1;
        s.info = Arc::new(info);
        let info = s.info.clone();
        self.emit(Event::Opened(info));
        self.select_subtitles(Some(i));
    }

    fn close(&mut self) {
        self.want_play = false;
        if let Some(s) = self.session.take() {
            self.out().set_playing(false);
            self.output.close();
            self.silent = None;
            if let Some(v) = s.video {
                self.video.send(video::Cmd::Stop);
                let _ = v.thread.join();
                let mut q = self.video.q.lock().unwrap_or_else(|e| e.into_inner());
                q.packets.clear();
                q.cmds.clear();
            }
        }
        self.clock.stop_at(0.0);
    }

    fn play(&mut self) {
        let Some(s) = self.session.as_ref() else { return };
        self.want_play = true;
        if s.ended {
            self.seek(0.0, true);
            return;
        }
        if s.stepped {
            // Audio is where the frame stepping started: catch it up.
            let pos = self.clock.now();
            self.seek(pos, true);
            return;
        }
        if s.started {
            self.resume();
        }
    }

    /// Start (or restart) what plays.
    fn resume(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        s.started = true;
        self.started_at = Some(std::time::Instant::now());
        if s.audio.is_some() {
            self.out().set_playing(true);
        } else {
            let pos = self.clock.now();
            self.clock.run_from(pos, self.speed);
        }
        self.video.poke();
        self.emit(Event::State(PlayState::Playing));
    }

    fn pause(&mut self) {
        self.want_play = false;
        let Some(s) = self.session.as_ref() else { return };
        if s.audio.is_some() {
            // The fields, not `out()`: the session stays borrowed.
            match &mut self.silent {
                Some(n) => n.set_playing(false),
                None => self.output.set_playing(false),
            }
        } else {
            let pos = self.clock.now();
            self.clock.stop_at(pos);
        }
        self.video.poke();
        if !s.ended {
            self.emit(Event::State(PlayState::Paused));
        }
    }

    fn seek(&mut self, to: f64, precise: bool) {
        if self.session.as_ref().is_none_or(|s| !s.info.seekable) {
            return;
        }
        self.seek_to(to, precise, false);
    }

    /// Go to `to`: everything flushed, read from the keyframe before, and
    /// played (if playing) once it's ready again.
    fn seek_to(&mut self, to: f64, precise: bool, opening: bool) {
        let duration = self.session.as_ref().and_then(|s| s.info.duration).unwrap_or(f64::MAX);
        let to = to.clamp(0.0, (duration - 0.05).max(0.0));
        self.out().set_playing(false);
        self.clock.stop_at(to);
        let speed = self.speed;
        let Some(s) = self.session.as_mut() else { return };
        if !opening || to > 0.0 {
            let start = s.input.start();
            if let Err(e) = s.input.seek(start + to, None) {
                log::warn!("seek: {e}");
            }
        }
        s.eof = false;
        s.ended = false;
        s.started = false;
        s.stepped = false;
        s.seeked_said = false;
        s.from = to;
        s.generation += 1;
        if let Some(a) = &mut s.audio {
            a.decoder.flush();
            a.packets.clear();
            a.pending.clear();
            a.tempo = None;
            a.drained = false;
            a.skip_until = if precise { to } else { f64::NEG_INFINITY };
            a.next_pts = to;
            a.rebase = true;
            a.ring.flush(to, speed, true);
        }
        match &mut self.silent {
            Some(n) => n.flush(),
            None => self.output.flush(),
        }
        if let Some(sp) = &mut s.subs {
            sp.decoder.flush();
            sp.skip_until = f64::NEG_INFINITY;
        }
        if s.video.is_some() {
            let mut q = self.video.q.lock().unwrap_or_else(|e| e.into_inner());
            q.packets.clear();
            q.eof = false;
            q.generation = s.generation;
            drop(q);
            self.video.suspended.store(false, Ordering::Release);
            self.video.send(video::Cmd::Flush {
                generation: s.generation,
                from: if precise { to } else { f64::NEG_INFINITY },
                poster: true,
            });
        }
        let embedded = s.subs.is_some();
        if embedded {
            self.emit(Event::ClearCues);
        }
    }

    fn step(&mut self, forward: bool) {
        if self.session.as_ref().is_none_or(|s| s.video.is_none()) {
            return;
        }
        if self.want_play {
            self.pause();
        }
        if forward {
            if let Some(s) = self.session.as_mut() {
                s.stepped = true;
            }
            self.video.send(video::Cmd::Step);
        } else {
            let pts = f64::from_bits(self.video.last_pts.load(Ordering::Acquire));
            let dur = self.session.as_ref().map_or(1.0 / 30.0, |s| {
                s.info.video.as_ref().and_then(|v| v.fps).map_or(1.0 / 30.0, |f| 1.0 / f)
            });
            self.seek(pts - dur, true);
            if let Some(s) = self.session.as_mut() {
                s.stepped = true;
            }
        }
    }

    /// The audio output takes nothing: play on through the null output,
    /// which keeps time as a device would.
    fn silent(&mut self) {
        log::warn!("nothing plays sound: playing on without it");
        let Some(ring) = self.session.as_ref().and_then(|s| s.audio.as_ref()).map(|a| a.ring.clone()) else { return };
        self.out().set_playing(false);
        self.output.close();
        let mut null = NullOutput::new();
        let media = self.session.as_ref().map(|s| s.info.title.clone()).unwrap_or_default();
        if null.open(ring, self.clock.clone(), &media).is_ok() {
            null.set_playing(true);
        }
        // For this item: the next opens the real output again.
        self.silent = Some(null);
        self.started_at = Some(std::time::Instant::now());
        self.pipeline.audio_output = self.out().describe();
        self.emit(Event::Pipeline(self.pipeline.clone()));
    }

    /// The GPU couldn't decode the video: open it again in software and go
    /// on from where the clock is.
    fn software_video(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let Some(index) = s.video.as_ref().map(|v| v.index) else { return };
        let Some(stream) = s.input.stream(index) else { return };
        let info = video::StreamInfo {
            frame_duration: video::frame_duration(stream),
            pixel_aspect: av::ratio(stream.sample_aspect_ratio).unwrap_or(1.0),
        };
        match open_video(stream, None) {
            Ok(decoder) => {
                self.pipeline.video_decoder = Some(decoder.codec_name());
                self.video.send(video::Cmd::Start { decoder, info });
                let pos = self.clock.now();
                let playing = self.want_play;
                self.seek_to(pos, true, false);
                self.want_play = playing;
                self.emit(Event::Pipeline(self.pipeline.clone()));
            }
            Err(e) => self.emit(Event::Failed(format!("Can't decode its video: {e}"))),
        }
    }

    /// Read, decode and start as needed: how long until there's more to
    /// do (none: until woken).
    fn work(&mut self) -> Option<Duration> {
        if self.video.hw_failed.swap(false, Ordering::AcqRel) {
            self.software_video();
        }
        let s = self.session.as_mut()?;
        // The video thread back in sight: go back for it.
        if self.video.resync.swap(false, Ordering::AcqRel) && s.video.is_some() && s.info.seekable {
            let pos = self.clock.now() + 0.3;
            resync_video(s, &self.video, pos);
        }
        let suspended = self.video.suspended.load(Ordering::Acquire);
        // Read while the audio or the video wants more.
        let mut packets_read = 0;
        while !s.eof {
            let audio_secs = s.audio.as_ref().map_or(0.0, |a| a.ring.buffered_secs() + packets_secs(a));
            let wants_audio = s.audio.as_ref().is_some_and(|_| audio_secs < HIGH + 1.0);
            let vq = if s.video.is_some() && !suspended {
                self.video.q.lock().unwrap_or_else(|e| e.into_inner()).packets.len()
            } else {
                usize::MAX
            };
            let wants_video = vq < video::PACKETS_HIGH;
            let audio_full = s.audio.as_ref().is_some_and(|a| packets_secs(a) > AUDIO_PACKETS_SECS);
            if !(wants_audio || wants_video) || audio_full || (vq != usize::MAX && vq > video::PACKETS_HIGH * 4) {
                break;
            }
            let mut pkt = Packet::new();
            match s.input.read(&mut pkt) {
                Ok(true) => {}
                Ok(false) => {
                    s.eof = true;
                    if s.video.is_some() {
                        self.video.q.lock().unwrap_or_else(|e| e.into_inner()).eof = true;
                        self.video.poke();
                    }
                    break;
                }
                Err(e) => {
                    log::warn!("reading: {e}");
                    s.eof = true;
                    self.video.q.lock().unwrap_or_else(|e| e.into_inner()).eof = true;
                    self.video.poke();
                    break;
                }
            }
            packets_read += 1;
            let index = pkt.get().stream_index as usize;
            if s.audio.as_ref().is_some_and(|a| a.index == index) {
                if let Some(a) = &mut s.audio {
                    a.packets.push_back(pkt);
                }
            } else if s.video.as_ref().is_some_and(|v| v.index == index) {
                if !suspended {
                    let mut q = self.video.q.lock().unwrap_or_else(|e| e.into_inner());
                    q.packets.push_back(pkt);
                    if q.packets.len() == 1 || q.packets.len().is_multiple_of(8) {
                        self.video.cv.notify_all();
                    }
                }
            } else if s.subs.as_ref().is_some_and(|p| p.index == index) {
                let frame = s.info.video.as_ref().map_or((0, 0), |v| (v.width, v.height));
                if let Some(sp) = &mut s.subs {
                    let cues: Vec<Cue> = subs::decode(&mut sp.decoder, &pkt, frame)
                        .into_iter()
                        .filter(|c| c.start >= sp.skip_until)
                        .collect();
                    if let Some(last) = cues.iter().map(|c| c.start).reduce(f64::max) {
                        sp.skip_until = sp.skip_until.max(last + 1e-6);
                    }
                    if !cues.is_empty() {
                        (self.events)(Event::Cues(cues));
                    }
                }
            }
        }
        if packets_read > 0 && s.video.is_some() {
            self.video.cv.notify_all();
        }
        // Decode audio into the ring.
        let speed = self.speed;
        if let Some(a) = &mut s.audio {
            decode_audio(a, s.eof, speed);
        }
        // Start, once ready.
        let video_ready = s.video.is_none() || self.video.shown_generation.load(Ordering::Acquire) == s.generation;
        let audio_ready = s.audio.as_ref().is_none_or(|a| a.ring.buffered_secs() >= PREROLL || (s.eof && a.packets.is_empty()));
        if video_ready && audio_ready && !s.seeked_said {
            s.seeked_said = true;
            let from = s.from;
            (self.events)(Event::Seeked(from));
            if !self.want_play && !s.started {
                (self.events)(Event::State(PlayState::Paused));
            }
        }
        let start_now = video_ready && audio_ready && self.want_play && !s.started;
        if start_now {
            self.resume();
        }
        // Playing, audio waiting, and the clock not running: nothing takes
        // the sound (no device). Play on without it.
        let stalled = self.want_play
            && self.session.as_ref().is_some_and(|s| s.started && s.audio.as_ref().is_some_and(|a| a.ring.buffered() > 0))
            && !self.clock.running()
            && self.started_at.is_some_and(|t| t.elapsed() > Duration::from_millis(1500));
        if stalled {
            self.silent();
        }
        let s = self.session.as_mut()?;
        // The end.
        if s.eof && !s.ended {
            let audio_done = s.audio.as_ref().is_none_or(|a| {
                a.packets.is_empty() && a.pending.is_empty() && a.drained && a.ring.buffered() == 0
            });
            let video_done = s.video.is_none() || self.video.drained.load(Ordering::Acquire) || suspended;
            if let Some(a) = &s.audio
                && a.packets.is_empty()
                && a.pending.is_empty()
            {
                a.ring.ended.store(true, Ordering::Release);
            }
            let end = s.info.duration.unwrap_or(0.0);
            let heard_all = s.audio.is_none() || !self.clock.running() || self.clock.now() >= end - 0.05;
            let past_video = s.audio.is_some() || self.clock.now() >= f64::from_bits(self.video.last_pts.load(Ordering::Acquire));
            if audio_done && video_done && heard_all && past_video && s.started {
                s.ended = true;
                self.want_play = false;
                self.out().set_playing(false);
                let at = self.clock.now();
                self.clock.stop_at(at);
                (self.events)(Event::State(PlayState::Ended));
                return None;
            }
            return Some(Duration::from_millis(50));
        }
        let s = self.session.as_ref()?;
        if !s.started && self.want_play {
            return Some(Duration::from_millis(20));
        }
        if !self.want_play || !s.started {
            return None;
        }
        // Playing: back when the ring is down to its low mark.
        match &s.audio {
            Some(a) => {
                let secs = (a.ring.buffered_secs() - LOW) / self.speed.max(0.1);
                Some(Duration::from_secs_f64(secs.clamp(0.02, LOW)))
            }
            None => None,
        }
    }
}

/// Seconds of compressed audio waiting.
fn packets_secs(a: &AudioPart) -> f64 {
    match (a.packets.front(), a.packets.back()) {
        (Some(f), Some(b)) => {
            let tb = a.decoder.time_base;
            match (av::seconds(f.get().pts, tb), av::seconds(b.get().pts, tb)) {
                (Some(x), Some(y)) => (y - x).max(0.0),
                _ => a.packets.len() as f64 * 0.02,
            }
        }
        _ => 0.0,
    }
}

/// Decode audio packets into the ring until it holds `HIGH` seconds.
fn decode_audio(a: &mut AudioPart, eof: bool, speed: f64) {
    let channels = a.ring.format.channels as i32;
    loop {
        if !a.pending.is_empty() {
            let pushed = a.ring.push(&a.pending);
            a.pending.drain(..pushed * channels as usize);
            if !a.pending.is_empty() {
                return;
            }
        }
        if a.ring.buffered_secs() >= HIGH {
            return;
        }
        let pkt = a.packets.pop_front();
        match &pkt {
            Some(p) => {
                if let Err(e) = a.decoder.send(Some(p))
                    && !e.is_again()
                {
                    log::debug!("audio packet: {e}");
                }
            }
            None if eof && !a.drained => {
                a.drained = true;
                let _ = a.decoder.send(None);
            }
            None => return,
        }
        let mut frame = Frame::new();
        while let Ok(true) = a.decoder.receive(&mut frame) {
            let f = frame.get();
            let rate = f64::from(f.sample_rate.max(1));
            let pts = frame.time(a.decoder.time_base).unwrap_or(a.next_pts);
            let dur = f64::from(f.nb_samples) / rate;
            if pts + dur <= a.skip_until {
                continue;
            }
            // Trim a frame that straddles where playing starts.
            let mut from_sample = 0;
            if pts < a.skip_until {
                from_sample = ((a.skip_until - pts) * rate).round() as i32;
            }
            let start = pts + f64::from(from_sample) / rate;
            if a.rebase {
                a.ring.flush(start, speed, false);
                a.rebase = false;
            }
            let src = if from_sample > 0 { trimmed(&frame, from_sample) } else { None };
            let src = src.as_ref().unwrap_or(&frame);
            if (speed - 1.0).abs() > 1e-3 {
                if a.tempo.is_none() {
                    a.tempo = Tempo::new(speed, src.get());
                }
                if let Some(t) = &mut a.tempo {
                    t.push(src);
                    let mut out = Frame::new();
                    while t.pull(&mut out) {
                        a.resampler.convert(&out, channels, &mut a.pending);
                    }
                }
            } else {
                a.resampler.convert(src, channels, &mut a.pending);
            }
            a.next_pts = pts + dur;
            a.skip_until = f64::NEG_INFINITY;
        }
        if pkt.is_none() {
            // Drained: everything converted.
            let pushed = a.ring.push(&a.pending);
            a.pending.drain(..pushed * channels as usize);
            return;
        }
    }
}

/// `frame` without its first `n` samples.
fn trimmed(frame: &Frame, n: i32) -> Option<Frame> {
    let f = frame.get();
    if n <= 0 || n >= f.nb_samples {
        return None;
    }
    let mut out = Frame::new();
    // SAFETY: a new frame of the same format, `nb_samples - n` long, its
    // samples copied from `n` on.
    unsafe {
        let o = out.get_mut();
        o.format = f.format;
        o.sample_rate = f.sample_rate;
        ff::av_channel_layout_copy(&mut o.ch_layout, &f.ch_layout);
        o.nb_samples = f.nb_samples - n;
        if ff::av_frame_get_buffer(out.0, 0) < 0 {
            return None;
        }
        if ff::av_samples_copy(
            (*out.0).extended_data,
            f.extended_data as *const *mut u8,
            0,
            n,
            f.nb_samples - n,
            f.ch_layout.nb_channels,
            std::mem::transmute::<i32, ff::AVSampleFormat>(f.format),
        ) < 0
        {
            return None;
        }
        (*out.0).pts = ff::AV_NOPTS_VALUE;
    }
    Some(out)
}

/// The video back in sight: read again from its keyframe before `pos`,
/// dropping what audio and subtitles already have, and show from `pos`.
fn resync_video(s: &mut Session, shared: &video::Shared, pos: f64) {
    let Some(v) = &s.video else { return };
    let start = s.input.start();
    if let Err(e) = s.input.seek(start + pos, Some(v.index)) {
        log::warn!("catching up: {e}");
        return;
    }
    s.eof = false;
    s.generation += 1;
    if let Some(a) = &mut s.audio {
        // What's decoded stays; what's read again before it goes.
        a.packets.clear();
        a.skip_until = a.next_pts;
    }
    let mut q = shared.q.lock().unwrap_or_else(|e| e.into_inner());
    q.packets.clear();
    q.eof = false;
    q.generation = s.generation;
    drop(q);
    shared.suspended.store(false, Ordering::Release);
    // Keep the generation shown: playing doesn't wait for this frame.
    shared.shown_generation.store(s.generation, Ordering::Release);
    shared.send(video::Cmd::Flush { generation: s.generation, from: pos, poster: false });
}

/// The media's best stream of a kind (never a cover picture).
fn best_stream(input: &Input, kind: ff::AVMediaType) -> Option<usize> {
    // SAFETY: a valid input; no decoder asked for.
    let i = unsafe { ff::av_find_best_stream(input.ctx, kind, -1, -1, std::ptr::null_mut(), 0) };
    let i = usize::try_from(i).ok()?;
    let s = input.stream(i)?;
    (s.disposition & ff::AV_DISPOSITION_ATTACHED_PIC == 0).then_some(i)
}

/// Open a video decoder: on the GPU through `hw` where the codec allows,
/// else in software on several threads.
fn open_video(stream: &ff::AVStream, hw: Option<*mut ff::AVBufferRef>) -> av::Result<Decoder> {
    Decoder::open_with(stream, hw.is_some(), |c| {
        match hw {
            Some(dev) => {
                // SAFETY: a new reference to the device for the context.
                c.hw_device_ctx = unsafe { ff::av_buffer_ref(dev) };
                c.get_format = Some(get_format);
                // Frames on screen and queued, held past the decoder's own.
                c.extra_hw_frames = 12;
                // One thread: the GPU decodes; more only wake more.
                c.thread_count = 1;
            }
            None => {
                c.thread_count = 0;
                c.thread_type = 3;
            }
        }
    })
}

/// FFmpeg's choice of pixel format: VA-API surfaces where offered.
unsafe extern "C" fn get_format(c: *mut ff::AVCodecContext, formats: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    // SAFETY: a list ended by NONE.
    unsafe {
        let mut p = formats;
        let mut first_sw = ff::AVPixelFormat::AV_PIX_FMT_NONE;
        while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE {
            if *p == ff::AVPixelFormat::AV_PIX_FMT_VAAPI && !(*c).hw_device_ctx.is_null() {
                return *p;
            }
            let desc = ff::av_pix_fmt_desc_get(*p);
            let hwaccel = !desc.is_null() && ((*desc).flags & (1 << 3)) != 0;
            if !hwaccel && first_sw == ff::AVPixelFormat::AV_PIX_FMT_NONE {
                first_sw = *p;
            }
            p = p.add(1);
        }
        first_sw
    }
}

fn describe_video(d: &Decoder) -> String {
    d.codec_name()
}

/// The DRM render node of device `dev` (dev_t), if it has one.
fn render_node(dev: u64) -> Option<String> {
    if dev == 0 {
        return None;
    }
    let sys = |d: u64| {
        let (maj, min) = (libc::major(d as libc::dev_t), libc::minor(d as libc::dev_t));
        std::fs::canonicalize(format!("/sys/dev/char/{maj}:{min}/device")).ok()
    };
    let want = sys(dev)?;
    let entries = std::fs::read_dir("/dev/dri").ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("renderD") {
            continue;
        }
        use std::os::unix::fs::MetadataExt;
        let Ok(m) = e.metadata() else { continue };
        if sys(m.rdev()).as_ref() == Some(&want) {
            return Some(e.path().to_string_lossy().into_owned());
        }
    }
    None
}

/// A local path from a URL or path.
fn url_path(url: &str) -> Option<PathBuf> {
    if let Some(rest) = url.strip_prefix("file://") {
        return Some(PathBuf::from(percent_decode(rest)));
    }
    (!url.contains("://")).then(|| PathBuf::from(url))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What the media says of itself.
fn media_info(input: &Input, url: &str, path: Option<PathBuf>) -> MediaInfo {
    let meta = input.metadata();
    let get = |keys: &[&str]| {
        keys.iter().find_map(|k| meta.iter().find(|(m, _)| m.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_owned()))
            .filter(|v| !v.is_empty())
    };
    let file_title = path
        .as_deref()
        .and_then(Path::file_stem)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(url).to_owned());
    let mut info = MediaInfo {
        url: url.to_owned(),
        size: path.as_deref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()),
        path,
        title: get(&["title"]).unwrap_or(file_title),
        artist: get(&["artist", "album_artist", "artists", "composer"]),
        album: get(&["album"]),
        year: get(&["date", "year", "creation_time"]).map(|d| d.chars().take(4).collect()),
        duration: input.duration(),
        chapters: input.chapters().into_iter().map(|(start, end, title)| Chapter { start, end, title }).collect(),
        seekable: input.seekable() && input.duration().is_some(),
        format: input.format_name(),
        bit_rate: input.bit_rate(),
        ..MediaInfo::default()
    };
    let start = input.start();
    for c in &mut info.chapters {
        c.start -= start;
        c.end -= start;
    }
    let best_video = best_stream(input, ff::AVMediaType::AVMEDIA_TYPE_VIDEO);
    for (i, &s) in input.streams().iter().enumerate() {
        // SAFETY: streams of an open input.
        let s = unsafe { &*s };
        let par = unsafe { &*s.codecpar };
        let smeta = unsafe { av::dictionary(s.metadata) };
        let sget = |k: &str| smeta.iter().find(|(m, _)| m.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
        let disposition = s.disposition;
        match par.codec_type {
            ff::AVMediaType::AVMEDIA_TYPE_VIDEO if disposition & ff::AV_DISPOSITION_ATTACHED_PIC != 0 => {
                if info.cover.is_none() && !s.attached_pic.data.is_null() && s.attached_pic.size > 0 {
                    // SAFETY: the attached picture's bytes.
                    let bytes = unsafe { std::slice::from_raw_parts(s.attached_pic.data, s.attached_pic.size as usize) };
                    info.cover = Some(Arc::new(bytes.to_vec()));
                }
            }
            ff::AVMediaType::AVMEDIA_TYPE_VIDEO if Some(i) == best_video => {
                let sar = av::ratio(if s.sample_aspect_ratio.num > 0 { s.sample_aspect_ratio } else { par.sample_aspect_ratio })
                    .filter(|r| *r > 0.0)
                    .unwrap_or(1.0);
                let (w, h) = (par.width.max(1) as u32, par.height.max(1) as u32);
                // SAFETY: a pixel format's description, if any.
                let depth = unsafe {
                    let d = ff::av_pix_fmt_desc_get(std::mem::transmute::<i32, ff::AVPixelFormat>(par.format));
                    if d.is_null() { 8 } else { (*d).comp[0].depth as u32 }
                };
                info.video = Some(VideoInfo {
                    codec: av::codec_name(par.codec_id),
                    width: w,
                    height: h,
                    display: (f64::from(w) * sar, f64::from(h)),
                    fps: av::ratio(s.avg_frame_rate).filter(|r| *r > 0.0 && *r < 1000.0),
                    hdr: matches!(
                        par.color_trc,
                        ff::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084
                            | ff::AVColorTransferCharacteristic::AVCOL_TRC_ARIB_STD_B67
                    ),
                    bit_depth: depth,
                });
            }
            ff::AVMediaType::AVMEDIA_TYPE_AUDIO => info.audio.push(Track {
                stream: Some(i),
                file: None,
                language: sget("language").filter(|l| l != "und"),
                title: sget("title"),
                codec: av::codec_name(par.codec_id),
                channels: Some(par.ch_layout.nb_channels as u32),
                default: disposition & ff::AV_DISPOSITION_DEFAULT != 0,
                forced: false,
            }),
            ff::AVMediaType::AVMEDIA_TYPE_SUBTITLE => info.subtitles.push(Track {
                stream: Some(i),
                file: None,
                language: sget("language").filter(|l| l != "und"),
                title: sget("title"),
                codec: av::codec_name(par.codec_id),
                channels: None,
                default: disposition & ff::AV_DISPOSITION_DEFAULT != 0,
                forced: disposition & ff::AV_DISPOSITION_FORCED != 0,
            }),
            _ => {}
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_say_what_they_are() {
        let t = Track {
            stream: Some(1),
            file: None,
            language: Some("eng".into()),
            title: None,
            codec: "eac3".into(),
            channels: Some(6),
            default: true,
            forced: false,
        };
        assert_eq!(t.label(1), "English · 5.1");
        let t = Track { title: Some("Commentary".into()), channels: Some(2), ..t };
        assert_eq!(t.label(2), "Commentary (English) · Stereo");
        let t = Track { title: None, language: None, channels: None, forced: true, ..t };
        assert_eq!(t.label(3), "Track 3 · Forced");
    }

    #[test]
    fn urls_become_paths() {
        assert_eq!(url_path("file:///home/me/A%20Film.mkv"), Some(PathBuf::from("/home/me/A Film.mkv")));
        assert_eq!(url_path("/tmp/x.mp4"), Some(PathBuf::from("/tmp/x.mp4")));
        assert_eq!(url_path("https://example.com/x.mp4"), None);
    }
}
