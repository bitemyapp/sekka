//! Utsushi, the program: what's queued and playing, how it's shown, and
//! what the player says.
//!
//! **What redraws.** Nothing runs on a frame clock. While playing, the
//! window is woken once a second, as the time shown turns over, and the
//! timeline's knob moves with it (twice a second for clips under a
//! minute, where a second is a long way along the timeline); when a
//! subtitle starts or ends; and by the player's events. Paused, hidden, or full screen with the controls
//! gone, only subtitles wake it. The picture itself never passes through
//! here: the player's thread shows it.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::MsgSender;
use aizome_kit::WindowState;
use aizome_kit::prelude::*;
use aizome_media::{AudioChoice, Cue, CueBody, Event, MediaInfo, Options, Pipeline, PlayState, Player, StreamKind};

use crate::mpris::{self, Mpris};
use crate::store::{Recent, Store};

pub const APP_ID: &str = "org.aizome.Utsushi";
/// The window's one video.
pub const VIDEO: aizome_kit::video::VideoId = 1;
/// How long full-screen controls stay after the pointer stops.
const CONTROLS_STAY: Duration = Duration::from_millis(2500);
/// Clips shorter than this tick twice a second.
const SHORT: f64 = 60.0;

/// What the player said, as a message.
#[derive(Clone, Debug)]
pub struct PlayerEvent(pub Event);

/// Dragging along the timeline: where, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scrub {
    Start(f64),
    Move(f64),
    End(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    One,
    All,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// Play these (the queue).
    Open(Vec<PathBuf>),
    OpenPanel,
    Chosen(Option<Vec<PathBuf>>),
    /// A later launch's files.
    Instance(Vec<PathBuf>),
    Player(PlayerEvent),
    Cover(u64, Option<Image>),
    Toggle,
    Play,
    Pause,
    SeekBy(f64),
    SeekTo(f64),
    /// To a fraction of the length (the number keys).
    SeekFraction(f64),
    Scrub(Scrub),
    Step(bool),
    Next,
    Previous,
    PlayItem(usize),
    Chapter(i32),
    Volume(f32),
    VolumeBy(f32),
    Mute,
    Speed(f64),
    SpeedBy(i32),
    Repeat(Repeat),
    Audio(Option<usize>),
    Subtitles(Option<usize>),
    CycleSubtitles,
    /// Move the subtitles later by this many seconds (none: back to
    /// their own timing).
    SubtitleDelay(Option<f64>),
    AddSubtitles,
    SubtitlesChosen(Option<Vec<PathBuf>>),
    ToggleFullscreen,
    Escape,
    /// A click on the picture (1, or 2 for a double click).
    StageClick(u32),
    /// The pointer moved over the picture (full screen).
    Pointer,
    /// The pointer is over the full-screen controls, or left them.
    OverControls(bool),
    HideControls,
    /// The keys go back to the picture (after the volume was dragged).
    FocusPicture,
    Tick,
    CueTick,
    ShowInfo,
    InfoTick,
    /// `UTSUSHI_STATS=1`: the pipeline and frame counts to the log, every
    /// five seconds (for measuring).
    LogStats,
    CloseSheet,
    ToggleRemaining,
    StartOver,
    DismissResumed,
    ShowInHako,
    /// The picture showing, to the clipboard or to Pictures.
    CopyPicture,
    SavePicture,
    Picture(bool, Option<Arc<aizome_media::Picture>>),
    /// Saving a picture ended: where, or why not.
    PictureSaved(Result<PathBuf, String>),
    TracksMenu,
    SpeedMenu,
    Mpris(mpris::Action),
    OpenRecent(usize),
    ClearRecent,
    Close,
}

/// Where in its life the item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Empty,
    Loading,
    Paused,
    Playing,
    Ended,
    Failed,
}

pub struct Utsushi {
    pub instance: Option<std::os::unix::net::UnixListener>,
    pub initial: Vec<PathBuf>,
    /// Where state is kept (none: the usual place).
    pub state_dir: Option<PathBuf>,
    /// Play nowhere (tests, and machines without sound).
    pub null_audio: bool,
    pub window: WindowKey,
    pub sheet: Option<WindowKey>,
    pub player: Option<Player>,
    sender: Option<MsgSender<Msg>>,
    pub queue: Vec<PathBuf>,
    pub current: Option<usize>,
    /// Counts items loaded, so late answers about an earlier one are
    /// dropped.
    pub item: u64,
    pub info: Option<Arc<MediaInfo>>,
    pub status: Status,
    pub error: Option<String>,
    pub pipeline: Pipeline,
    pub cover: Option<Image>,
    /// The time shown (moved on ticks, seeks and scrubbing).
    pub pos: f64,
    pub scrub: Option<f64>,
    scrub_playing: bool,
    last_scrub_seek: Option<Instant>,
    pub volume: f32,
    pub muted: bool,
    pub speed: f64,
    pub repeat: Repeat,
    pub audio_track: Option<usize>,
    pub sub_track: Option<usize>,
    /// The subtitle track's cues, in order, and the time they're shown at.
    pub cues: Vec<Cue>,
    pub cue_pos: f64,
    /// Seconds the subtitles are moved later (earlier when negative):
    /// for a subtitle file timed to another cut of the film.
    pub sub_delay: f64,
    pub fullscreen: bool,
    pub maximized: bool,
    /// The desktop isn't showing the window.
    pub hidden: bool,
    /// The full-screen controls show.
    pub controls: bool,
    /// The pointer is over the full-screen controls: they stay.
    over_controls: bool,
    controls_timer: Option<TimerHandle>,
    controls_shown_at: Option<Instant>,
    tick: Option<TimerHandle>,
    cue_timer: Option<TimerHandle>,
    info_timer: Option<TimerHandle>,
    pub show_remaining: bool,
    /// Started where it was left: shown for a while, with Start Over.
    pub resumed: Option<f64>,
    resumed_timer: Option<TimerHandle>,
    pub store: Store,
    mpris: Option<Mpris>,
    seeks: u64,
    /// The picture's size last fitted the window to.
    fitted: Option<(f64, f64)>,
    /// The stage's size as last laid out.
    pub stage: Size,
}

impl Default for Utsushi {
    fn default() -> Utsushi {
        Utsushi::new()
    }
}

impl Utsushi {
    pub fn new() -> Utsushi {
        Utsushi {
            instance: None,
            initial: Vec::new(),
            state_dir: None,
            null_audio: false,
            window: WindowKey::NONE,
            sheet: None,
            player: None,
            sender: None,
            queue: Vec::new(),
            current: None,
            item: 0,
            info: None,
            status: Status::Empty,
            error: None,
            pipeline: Pipeline::default(),
            cover: None,
            pos: 0.0,
            scrub: None,
            scrub_playing: false,
            last_scrub_seek: None,
            volume: 1.0,
            muted: false,
            speed: 1.0,
            repeat: Repeat::Off,
            audio_track: None,
            sub_track: None,
            cues: Vec::new(),
            cue_pos: 0.0,
            sub_delay: 0.0,
            fullscreen: false,
            maximized: false,
            hidden: false,
            controls: true,
            over_controls: false,
            controls_timer: None,
            controls_shown_at: None,
            tick: None,
            cue_timer: None,
            info_timer: None,
            show_remaining: true,
            resumed: None,
            resumed_timer: None,
            store: Store::default(),
            mpris: None,
            seeks: 0,
            fitted: None,
            stage: Size::new(960.0, 556.0),
        }
    }

    pub fn duration(&self) -> f64 {
        self.info.as_ref().and_then(|i| i.duration).unwrap_or(0.0)
    }

    pub fn has_video(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.video.is_some())
    }

    pub fn playing(&self) -> bool {
        self.status == Status::Playing
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.current.and_then(|i| self.queue.get(i)).map(PathBuf::as_path)
    }

    /// The time shown: where a drag is, else the last tick's.
    pub fn shown_pos(&self) -> f64 {
        self.scrub.unwrap_or(self.pos)
    }

    pub fn can_previous(&self) -> bool {
        self.current.is_some_and(|i| i > 0) || self.pos > 3.0
    }

    pub fn can_next(&self) -> bool {
        self.current.is_some_and(|i| i + 1 < self.queue.len())
    }

    /// The cues showing now.
    pub fn active_cues(&self) -> Vec<&Cue> {
        let p = self.cue_pos - self.sub_delay;
        self.cues.iter().filter(|c| c.start <= p + 1e-3 && p < c.end).collect()
    }

    pub fn chapters(&self) -> Rc<Vec<(f64, Option<String>)>> {
        Rc::new(
            self.info.as_ref().map(|i| i.chapters.iter().map(|c| (c.start, c.title.clone())).collect()).unwrap_or_default(),
        )
    }

    fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    /// Play item `i` of the queue (where it was left, if it was).
    fn load(&mut self, i: usize, cx: &mut Cx<Msg>) {
        self.leave();
        let Some(path) = self.queue.get(i).cloned() else { return };
        self.current = Some(i);
        self.item += 1;
        self.info = None;
        self.error = None;
        self.cover = None;
        self.cues.clear();
        self.cue_pos = 0.0;
        self.sub_delay = 0.0;
        self.pos = 0.0;
        self.scrub = None;
        self.status = Status::Loading;
        let start = self.store.resume_at(&path);
        self.resumed = start;
        if let Some(h) = self.resumed_timer.take() {
            cx.cancel(h);
        }
        if start.is_some() {
            self.resumed_timer = Some(cx.after(Duration::from_secs(8), Msg::DismissResumed));
        }
        if let Some(p) = self.player() {
            p.open(path.to_string_lossy(), start.unwrap_or(0.0), true);
        }
        let title = display_name(&path);
        cx.set_title(self.window, title);
    }

    /// Remember where the current item was left.
    fn leave(&mut self) {
        let (Some(path), Some(p)) = (self.current_path().map(Path::to_path_buf), self.player()) else { return };
        let d = self.duration();
        if d > 0.0 && self.status != Status::Failed {
            let at = if self.status == Status::Ended { d } else { p.position() };
            self.store.leave(&path, at, d);
        }
    }

    fn seek(&mut self, to: f64, cx: &mut Cx<Msg>) {
        let d = self.duration();
        if d <= 0.0 || !self.info.as_ref().is_some_and(|i| i.seekable) {
            return;
        }
        let to = to.clamp(0.0, d);
        if let Some(p) = self.player() {
            p.seek(to, true);
        }
        if self.status == Status::Ended {
            self.status = Status::Paused;
        }
        self.pos = to;
        self.cue_pos = to;
        self.seeks += 1;
        self.resumed = None;
        self.schedule(cx);
        self.publish();
    }

    fn toggle(&mut self, cx: &mut Cx<Msg>) {
        match self.status {
            Status::Playing => self.pause(cx),
            Status::Paused | Status::Ended | Status::Loading => self.play(cx),
            Status::Empty => cx.send(Msg::OpenPanel),
            Status::Failed => {}
        }
    }

    fn play(&mut self, _cx: &mut Cx<Msg>) {
        if let Some(p) = self.player() {
            p.play();
        }
    }

    fn pause(&mut self, _cx: &mut Cx<Msg>) {
        if let Some(p) = self.player() {
            p.pause();
        }
        self.leave();
    }

    fn set_volume(&mut self, v: f32, muted: bool) {
        self.volume = v.clamp(0.0, 1.0);
        self.muted = muted;
        if let Some(p) = self.player() {
            p.set_volume(self.volume, self.muted);
        }
        self.publish();
    }

    /// Wake for what the window shows: the time, the knob, subtitles.
    fn schedule(&mut self, cx: &mut Cx<Msg>) {
        if let Some(h) = self.tick.take() {
            cx.cancel(h);
        }
        if let Some(h) = self.cue_timer.take() {
            cx.cancel(h);
        }
        if !self.playing() {
            return;
        }
        let Some(p) = self.player() else { return };
        let pos = p.position();
        let speed = self.speed.max(0.1);
        // Subtitles: their next start or end.
        let delay = self.sub_delay;
        let next_cue = self
            .cues
            .iter()
            .flat_map(|c| [c.start + delay, c.end + delay])
            .filter(|t| *t > pos + 1e-3 && t.is_finite())
            .fold(f64::INFINITY, f64::min);
        if next_cue.is_finite() {
            self.cue_timer = Some(cx.after(Duration::from_secs_f64((next_cue - pos) / speed + 0.002), Msg::CueTick));
        }
        // The time and the knob, while they show.
        let shown = !self.hidden && (!self.fullscreen || self.controls || !self.has_video());
        if !shown {
            return;
        }
        let step = if self.duration() < SHORT { 0.5 } else { 1.0 };
        let wait = (((pos / step).floor() + 1.0) * step - pos) / speed;
        let wait = wait.max(0.02);
        self.tick = Some(cx.after(Duration::from_secs_f64(wait + 0.004), Msg::Tick));
    }

    fn show_controls(&mut self, cx: &mut Cx<Msg>) {
        if !self.fullscreen {
            return;
        }
        // Moves come many a second: once shown, the timer is set again at
        // most four times a second, as long as one is set.
        let fresh = self.controls_shown_at.is_some_and(|t| t.elapsed() < Duration::from_millis(250));
        if self.controls && fresh && self.controls_timer.is_some() {
            return;
        }
        let was = self.controls;
        self.controls = true;
        self.controls_shown_at = Some(Instant::now());
        if let Some(h) = self.controls_timer.take() {
            cx.cancel(h);
        }
        self.controls_timer = Some(cx.after(CONTROLS_STAY, Msg::HideControls));
        if !was {
            self.pos = self.player().map_or(self.pos, Player::position);
            self.schedule(cx);
        }
    }

    /// Full-screen controls that show with nothing to hide them: a pause
    /// brings them up, and so does the window coming back from another
    /// Desk, without the pointer moving. Once playing, they go when the
    /// pointer has rested, as after it moves; otherwise they would stay
    /// over the picture, which then can't be scanned out, and keep the
    /// clock redrawing.
    fn rest_controls(&mut self, cx: &mut Cx<Msg>) {
        if self.fullscreen && self.controls && self.playing() && self.has_video() && self.controls_timer.is_none() {
            self.controls_shown_at = Some(Instant::now());
            self.controls_timer = Some(cx.after(CONTROLS_STAY, Msg::HideControls));
        }
    }

    /// Tell the desktop's media controls.
    fn publish(&self) {
        let Some(m) = &self.mpris else { return };
        let info = self.info.as_deref();
        m.publish(mpris::State {
            item: self.item,
            title: info.map(|i| i.title.clone()).unwrap_or_default(),
            artist: info.and_then(|i| i.artist.clone()),
            album: info.and_then(|i| i.album.clone()),
            url: self.current_path().map(crate::ui::url_of),
            art: None,
            status: match self.status {
                Status::Playing => "Playing",
                Status::Paused | Status::Loading => "Paused",
                _ => "Stopped",
            },
            position: self.player().map_or(0.0, Player::position),
            duration: self.duration(),
            volume: if self.muted { 0.0 } else { f64::from(self.volume) },
            rate: self.speed,
            can_next: self.can_next(),
            can_previous: self.current.is_some(),
            can_seek: info.is_some_and(|i| i.seekable),
            loop_status: match self.repeat {
                Repeat::Off => "None",
                Repeat::One => "Track",
                Repeat::All => "Playlist",
            },
            seeks: self.seeks,
        });
    }

    /// Fit the window to a video's picture: its own size, up to most of
    /// the screen, plus the controls. A song after a video gets back a
    /// window shaped for its cover.
    fn fit_window(&mut self, cx: &mut Cx<Msg>) {
        if self.fullscreen || self.maximized {
            return;
        }
        let Some(v) = self.info.as_ref().and_then(|i| i.video.clone()) else {
            if self.fitted.take().is_some() {
                cx.request_size(self.window, Size::new(560.0, 520.0));
            }
            return;
        };
        let (w, h) = v.display;
        if self.fitted.is_some_and(|(fw, fh)| ((fw / fh) - (w / h)).abs() < 0.02) {
            return;
        }
        self.fitted = Some((w, h));
        let screen = cx.outputs().first().map_or(Size::new(1440.0, 900.0), |o| o.work_area.unwrap_or(o.frame.size()));
        let max_w = (screen.width * 0.7).max(480.0);
        let max_h = (screen.height * 0.7).max(320.0) - crate::ui::BAR;
        let s = (max_w / w).min(max_h / h).min(1.0);
        let size = Size::new((w * s).round().max(480.0), (h * s).round() + crate::ui::BAR);
        cx.request_size(self.window, size);
    }

    fn on_player(&mut self, e: Event, cx: &mut Cx<Msg>) {
        match e {
            Event::Opened(info) => {
                let first = self.info.as_ref().is_none_or(|i| i.url != info.url);
                if let (true, Some(bytes)) = (first, info.cover.clone()) {
                    let item = self.item;
                    cx.spawn(move || Msg::Cover(item, Image::decode(bytes.to_vec())));
                }
                if first && let Some(path) = info.path.clone() {
                    self.store.played(Recent {
                        path,
                        title: info.title.clone(),
                        duration: info.duration,
                        video: info.video.is_some(),
                    });
                }
                cx.set_title(self.window, info.title.clone());
                self.info = Some(info);
                if first {
                    self.fit_window(cx);
                }
                self.publish();
            }
            Event::Failed(msg) => {
                self.status = Status::Failed;
                self.error = Some(msg);
                self.schedule(cx);
                self.publish();
            }
            Event::State(s) => {
                self.status = match s {
                    PlayState::Playing => Status::Playing,
                    PlayState::Paused => Status::Paused,
                    PlayState::Ended => Status::Ended,
                };
                self.pos = self.player().map_or(self.pos, Player::position);
                self.cue_pos = self.pos;
                if s == PlayState::Ended {
                    self.leave();
                    match self.repeat {
                        Repeat::One => {
                            self.seek(0.0, cx);
                            self.play(cx);
                        }
                        _ if self.can_next() => cx.send(Msg::Next),
                        Repeat::All if !self.queue.is_empty() => {
                            self.load(0, cx);
                        }
                        _ => {
                            // Full screen ends with the film.
                            if self.fullscreen {
                                cx.set_fullscreen(self.window, false);
                            }
                        }
                    }
                }
                if s == PlayState::Paused {
                    self.controls = true;
                }
                // Playing again by a key, the menus or the desktop's media
                // controls: the pointer hasn't moved, so nothing else would.
                self.rest_controls(cx);
                self.schedule(cx);
                self.publish();
            }
            Event::Seeked(t) => {
                self.pos = t;
                self.cue_pos = t;
                if self.status == Status::Loading {
                    self.status = Status::Paused;
                }
                self.schedule(cx);
            }
            Event::Cues(mut cues) => {
                cues.retain(|c| !self.cues.iter().any(|o| o.start == c.start && o.body == c.body));
                self.cues.extend(cues);
                self.cues.sort_by(|a, b| a.start.total_cmp(&b.start));
                // An open-ended cue ends when the next of its track starts.
                for i in 0..self.cues.len() {
                    if self.cues[i].end.is_infinite()
                        && let Some(n) = self.cues[i + 1..].iter().find(|c| c.start > self.cues[i].start)
                    {
                        self.cues[i].end = n.start;
                    }
                }
                self.cues.retain(|c| c.body != CueBody::Clear);
                self.cue_pos = self.player().map_or(self.cue_pos, Player::position);
                self.schedule(cx);
            }
            Event::ClearCues => self.cues.clear(),
            Event::Pipeline(p) => self.pipeline = p,
            Event::Selected { audio, subtitles } => {
                self.audio_track = audio;
                self.sub_track = subtitles;
            }
        }
    }

    fn on_mpris(&mut self, a: mpris::Action, cx: &mut Cx<Msg>) {
        match a {
            mpris::Action::Play => self.play(cx),
            mpris::Action::Pause | mpris::Action::Stop => self.pause(cx),
            mpris::Action::Toggle => self.toggle(cx),
            mpris::Action::Next => cx.send(Msg::Next),
            mpris::Action::Previous => cx.send(Msg::Previous),
            mpris::Action::Seek(t) => self.seek(t, cx),
            mpris::Action::Volume(v) => self.set_volume(v as f32, false),
            mpris::Action::Rate(r) => cx.send(Msg::Speed(r)),
            mpris::Action::Loop(l) => cx.send(Msg::Repeat(match l.as_str() {
                "Track" => Repeat::One,
                "Playlist" => Repeat::All,
                _ => Repeat::Off,
            })),
            mpris::Action::Raise => cx.activate(self.window),
            mpris::Action::Quit => cx.quit(),
            mpris::Action::Open(uri) => {
                let path = crate::ui::file_path(&uri).unwrap_or_else(|| PathBuf::from(uri));
                cx.send(Msg::Open(vec![path]));
            }
        }
    }
}

/// A file's name as people read it: without its folder or extension.
pub fn display_name(path: &Path) -> String {
    path.file_stem().or(path.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Whether a file looks like something Utsushi plays (for folders and
/// drops: by its extension).
pub fn playable(path: &Path) -> bool {
    const KINDS: &[&str] = &[
        "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "mpg", "mpeg", "ts", "m2ts", "mts", "ogv", "flv", "3gp",
        "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wav", "aiff", "aif", "wv", "ape", "wma", "mka", "alac",
    ];
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| KINDS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Paths to play from what was given: files as they are, folders' playable
/// files in order.
pub fn expand(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let Ok(entries) = std::fs::read_dir(&p) else { continue };
            let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|f| playable(f)).collect();
            files.sort_by(|a, b| natural(&a.to_string_lossy(), &b.to_string_lossy()));
            out.extend(files);
        } else {
            out.push(p);
        }
    }
    out
}

/// Names in the order people expect: "Episode 2" before "Episode 10".
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let chunks = |s: &str| -> Vec<(bool, String)> {
        let mut v: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match v.last_mut() {
                Some((kind, chunk)) if *kind == d => chunk.push(c),
                _ => v.push((d, c.to_string())),
            }
        }
        v
    };
    let (ca, cb) = (chunks(a), chunks(b));
    for ((da, sa), (db, sb)) in ca.iter().zip(cb.iter()) {
        let o = if *da && *db {
            let (na, nb) = (sa.trim_start_matches('0'), sb.trim_start_matches('0'));
            na.len().cmp(&nb.len()).then_with(|| na.cmp(nb))
        } else {
            sa.to_lowercase().cmp(&sb.to_lowercase())
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

impl Program for Utsushi {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Utsushi", "Media Player").icon(Icon::Film)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.store = Store::load(self.state_dir.clone());
        self.volume = self.store.prefs.volume;
        self.muted = self.store.prefs.muted;
        self.show_remaining = self.store.prefs.remaining;
        self.window = cx.open_window(WindowSpec::new("Utsushi").size(960.0, 600.0).min_size(400.0, 260.0).video());
        let tx = cx.stream();
        self.sender = Some(tx.clone());
        let sink = cx.video_sink(self.window, VIDEO);
        let audio = if self.null_audio {
            AudioChoice::Null
        } else {
            AudioChoice::PipeWire(StreamKind {
                latency: 0.170,
                role: "Movie",
                app_name: "Utsushi".into(),
                app_id: APP_ID.into(),
            })
        };
        let ptx = tx.clone();
        let player = Player::new(Options { sink, audio, hardware: true, caps: None }, move |e| {
            ptx.send(Msg::Player(PlayerEvent(e)));
        });
        player.set_volume(self.volume, self.muted);
        self.player = Some(player);
        if !self.null_audio {
            let mtx = tx.clone();
            let clock = self.player.as_ref().map(Player::clock);
            self.mpris = Some(Mpris::start(
                move |a| {
                    mtx.send(Msg::Mpris(a));
                },
                move || clock.as_ref().map_or(0.0, |c| c.now()),
            ));
            self.publish();
        }
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, tx, Msg::Instance);
        }
        if std::env::var_os("UTSUSHI_STATS").is_some() {
            cx.every(Duration::from_secs(5), Msg::LogStats);
        }
        cx.focus("stage");
        let initial = std::mem::take(&mut self.initial);
        if !initial.is_empty() {
            cx.send(Msg::Open(initial));
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Open(paths) | Msg::Instance(paths) => {
                let paths = expand(paths);
                if paths.is_empty() {
                    cx.activate(self.window);
                    return;
                }
                self.leave();
                self.queue = paths;
                self.load(0, cx);
                cx.activate(self.window);
            }
            Msg::OpenPanel => {
                let dialog = FileDialog::new().title("Open").multiple().filter("Videos and music", &[
                    "mp4", "m4v", "mkv", "webm", "mov", "avi", "mpg", "ts", "ogv", "mp3", "m4a", "flac", "ogg", "opus",
                    "wav", "aac",
                ]);
                cx.open_panel(self.window, dialog, Msg::Chosen);
            }
            Msg::Chosen(Some(paths)) => cx.send(Msg::Open(paths)),
            Msg::Chosen(None) => {}
            Msg::Player(PlayerEvent(e)) => self.on_player(e, cx),
            Msg::Cover(item, img) => {
                if item == self.item {
                    self.cover = img;
                }
            }
            Msg::Toggle => self.toggle(cx),
            Msg::Play => self.play(cx),
            Msg::Pause => self.pause(cx),
            Msg::SeekBy(d) => {
                let base = self.player().map_or(self.pos, Player::position);
                self.seek(base + d, cx);
            }
            Msg::SeekTo(t) => self.seek(t, cx),
            Msg::SeekFraction(f) => {
                let d = self.duration();
                self.seek(d * f, cx);
            }
            Msg::Scrub(s) => match s {
                Scrub::Start(t) => {
                    self.scrub_playing = self.playing();
                    if self.scrub_playing
                        && let Some(p) = self.player()
                    {
                        p.pause();
                    }
                    self.scrub = Some(t);
                    self.last_scrub_seek = Some(Instant::now());
                    if let Some(p) = self.player() {
                        p.seek(t, false);
                    }
                }
                Scrub::Move(t) => {
                    self.scrub = Some(t);
                    // A keyframe now and then while dragging: what's there.
                    if self.last_scrub_seek.is_none_or(|i| i.elapsed() > Duration::from_millis(60)) {
                        self.last_scrub_seek = Some(Instant::now());
                        if let Some(p) = self.player() {
                            p.seek(t, false);
                        }
                    }
                }
                Scrub::End(t) => {
                    self.scrub = None;
                    self.seek(t, cx);
                    if self.scrub_playing {
                        self.play(cx);
                    }
                }
            },
            Msg::Step(forward) => {
                if let Some(p) = self.player() {
                    p.step(forward);
                }
            }
            Msg::Next => {
                if let Some(i) = self.current.filter(|i| i + 1 < self.queue.len()) {
                    self.load(i + 1, cx);
                }
            }
            Msg::Previous => match self.current {
                Some(i) if i > 0 && self.player().map_or(0.0, Player::position) < 3.0 => self.load(i - 1, cx),
                _ => self.seek(0.0, cx),
            },
            Msg::PlayItem(i) => self.load(i, cx),
            Msg::Chapter(dir) => {
                let chapters = self.chapters();
                let pos = self.player().map_or(self.pos, Player::position);
                let i = chapters.iter().rposition(|(s, _)| *s <= pos + 0.25).unwrap_or(0);
                let to = if dir > 0 {
                    chapters.get(i + 1).map(|c| c.0)
                } else if pos - chapters.get(i).map_or(0.0, |c| c.0) > 3.0 {
                    chapters.get(i).map(|c| c.0)
                } else {
                    chapters.get(i.saturating_sub(1)).map(|c| c.0)
                };
                if let Some(t) = to {
                    self.seek(t, cx);
                }
            }
            Msg::Volume(v) => self.set_volume(v, false),
            Msg::VolumeBy(d) => self.set_volume(self.volume + d, false),
            Msg::Mute => self.set_volume(self.volume, !self.muted),
            Msg::Speed(s) => {
                self.speed = s;
                if let Some(p) = self.player() {
                    p.set_speed(s);
                }
                self.schedule(cx);
                self.publish();
            }
            Msg::SpeedBy(dir) => {
                const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
                let i = SPEEDS.iter().position(|s| (*s - self.speed).abs() < 1e-3).unwrap_or(2) as i32;
                let next = SPEEDS[(i + dir).clamp(0, 6) as usize];
                cx.send(Msg::Speed(next));
            }
            Msg::Repeat(r) => {
                self.repeat = r;
                self.publish();
            }
            Msg::Audio(i) => {
                if let Some(p) = self.player() {
                    p.select_audio(i);
                }
            }
            Msg::Subtitles(i) => {
                if let Some(p) = self.player() {
                    p.select_subtitles(i);
                }
            }
            Msg::CycleSubtitles => {
                let n = self.info.as_ref().map_or(0, |i| i.subtitles.len());
                if n > 0 {
                    let next = match self.sub_track {
                        None => Some(0),
                        Some(i) if i + 1 < n => Some(i + 1),
                        Some(_) => None,
                    };
                    cx.send(Msg::Subtitles(next));
                }
            }
            Msg::SubtitleDelay(d) => {
                self.sub_delay = d.map_or(0.0, |d| ((self.sub_delay + d) * 10.0).round() / 10.0);
                self.cue_pos = self.player().map_or(self.cue_pos, Player::position);
                self.schedule(cx);
            }
            Msg::AddSubtitles => {
                let mut dialog =
                    FileDialog::new().title("Add Subtitles").filter("Subtitles", &["srt", "ass", "ssa", "vtt", "sub"]);
                if let Some(dir) = self.current_path().and_then(Path::parent) {
                    dialog = dialog.directory(dir);
                }
                cx.open_panel(self.window, dialog, Msg::SubtitlesChosen);
            }
            Msg::SubtitlesChosen(Some(paths)) => {
                if let (Some(p), Some(path)) = (self.player(), paths.into_iter().next()) {
                    p.add_subtitles(path);
                }
            }
            Msg::SubtitlesChosen(None) => {}
            Msg::ToggleFullscreen => cx.set_fullscreen(self.window, !self.fullscreen),
            Msg::Escape => {
                if self.fullscreen {
                    cx.set_fullscreen(self.window, false);
                }
            }
            Msg::StageClick(n) => {
                if n >= 2 {
                    // The first click toggled; a double click is for full
                    // screen, so it goes back.
                    self.toggle(cx);
                    if self.has_video() {
                        cx.set_fullscreen(self.window, !self.fullscreen);
                    }
                } else if self.info.is_some() {
                    self.toggle(cx);
                } else if self.status == Status::Empty {
                    cx.send(Msg::OpenPanel);
                }
                self.show_controls(cx);
            }
            Msg::Pointer => {
                // Over the picture, so off the controls, which are over it:
                // the bar hears the pointer leave only from its own
                // background, not from a control it went straight off.
                self.over_controls = false;
                self.show_controls(cx);
            }
            Msg::OverControls(on) => {
                self.over_controls = on;
                if !on {
                    self.show_controls(cx);
                }
            }
            Msg::HideControls => {
                self.controls_timer = None;
                if self.fullscreen && self.playing() && !self.over_controls && self.has_video() {
                    self.controls = false;
                    cx.hide_cursor_until_moved();
                    // What had the keys in the bar goes with it.
                    cx.focus("stage");
                    self.schedule(cx);
                }
            }
            Msg::FocusPicture => cx.focus("stage"),
            Msg::Tick => {
                self.tick = None;
                self.pos = self.player().map_or(self.pos, Player::position);
                if let Some(r) = cx.element_rect(self.window, "stage") {
                    self.stage = r.size();
                }
                self.schedule(cx);
            }
            Msg::CueTick => {
                self.cue_timer = None;
                self.cue_pos = self.player().map_or(self.cue_pos, Player::position);
                if let Some(r) = cx.element_rect(self.window, "stage") {
                    self.stage = r.size();
                }
                self.schedule(cx);
            }
            Msg::ShowInfo => {
                if self.sheet.is_none() && self.info.is_some() {
                    self.sheet = Some(cx.show_sheet(self.window));
                    self.info_timer = Some(cx.every(Duration::from_secs(1), Msg::InfoTick));
                }
            }
            Msg::InfoTick => {}
            Msg::LogStats => {
                if let Some(p) = self.player() {
                    eprintln!("utsushi: {:?} {:?} at {:.2}", self.pipeline, p.stats(), p.position());
                }
            }
            Msg::CloseSheet => {
                if let Some(s) = self.sheet.take() {
                    cx.close_window(s);
                }
                if let Some(h) = self.info_timer.take() {
                    cx.cancel(h);
                }
            }
            Msg::ToggleRemaining => {
                self.show_remaining = !self.show_remaining;
                self.store.prefs.remaining = self.show_remaining;
                self.store.save_prefs();
            }
            Msg::StartOver => {
                self.resumed = None;
                self.seek(0.0, cx);
            }
            Msg::DismissResumed => {
                self.resumed = None;
                self.resumed_timer = None;
            }
            Msg::CopyPicture | Msg::SavePicture => {
                let save = matches!(msg, Msg::SavePicture);
                if let (Some(p), Some(tx), true) = (self.player(), self.sender.clone(), self.has_video()) {
                    p.snapshot(move |pic| {
                        tx.send(Msg::Picture(save, pic.map(Arc::new)));
                    });
                }
            }
            Msg::Picture(save, Some(pic)) => {
                let name = format!(
                    "{} {}.png",
                    self.info.as_ref().map_or("Utsushi".to_owned(), |i| i.title.clone()),
                    crate::widgets::clock(self.player().map_or(self.pos, Player::position)).replace(':', "."),
                );
                cx.spawn(move || {
                    let Some(png) = crate::picture::encode(&pic) else {
                        return Msg::PictureSaved(Err("The picture didn't encode.".into()));
                    };
                    if !save {
                        crate::picture::offer(png);
                        return Msg::PictureSaved(Err(String::new()));
                    }
                    Msg::PictureSaved(crate::picture::save(&name, &png))
                });
            }
            Msg::Picture(_, None) => {}
            Msg::PictureSaved(Ok(path)) => {
                cx.notify("Picture saved", &path.display().to_string());
            }
            Msg::PictureSaved(Err(e)) => {
                if !e.is_empty() {
                    cx.notify("Utsushi couldn't save the picture", &e);
                }
            }
            Msg::ShowInHako => {
                if let Some(path) = self.current_path() {
                    let _ = std::process::Command::new("aizome-hako").arg("--select").arg(path).spawn();
                }
            }
            Msg::TracksMenu => {
                if let Some(r) = cx.element_rect(self.window, "tracks") {
                    let menu = crate::ui::tracks_menu(self);
                    cx.show_menu(self.window, r, menu, MenuStyle::default());
                }
            }
            Msg::SpeedMenu => {
                if let Some(r) = cx.element_rect(self.window, "speed") {
                    let menu = crate::ui::speed_menu(self);
                    cx.show_menu(self.window, r, menu, MenuStyle::default());
                }
            }
            Msg::Mpris(a) => self.on_mpris(a, cx),
            Msg::OpenRecent(i) => {
                if let Some(r) = self.store.recent.get(i) {
                    let p = r.path.clone();
                    cx.send(Msg::Open(vec![p]));
                }
            }
            Msg::ClearRecent => self.store.clear_recent(),
            Msg::Close => {
                self.leave();
                if let Some(p) = self.player() {
                    p.close();
                }
                self.queue.clear();
                self.current = None;
                self.info = None;
                self.cues.clear();
                self.status = Status::Empty;
                self.error = None;
                self.cover = None;
                self.fitted = None;
                if self.fullscreen {
                    cx.set_fullscreen(self.window, false);
                }
                cx.set_title(self.window, "Utsushi");
                self.schedule(cx);
                self.publish();
            }
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.sheet {
            return crate::ui::info_sheet(self);
        }
        crate::ui::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        crate::ui::menus(self)
    }

    fn window_state(&mut self, window: WindowKey, state: WindowState, cx: &mut Cx<Msg>) {
        if window != self.window {
            return;
        }
        let was = self.fullscreen;
        let entering = state.fullscreen && !was;
        self.fullscreen = state.fullscreen;
        self.maximized = state.maximized;
        if state.suspended != self.hidden {
            self.hidden = state.suspended;
            if let Some(p) = self.player() {
                p.set_visible(!state.suspended);
            }
        }
        self.controls = true;
        if was != state.fullscreen {
            // The controls move between the bar and over the picture: keys
            // go to the picture, whatever had them in the bar, and the
            // floating bar starts with the pointer off it (as it hears it
            // come).
            self.over_controls = false;
            cx.focus("stage");
        }
        if entering {
            self.show_controls(cx);
        }
        self.rest_controls(cx);
        self.pos = self.player().map_or(self.pos, Player::position);
        self.schedule(cx);
    }

    fn pointer_presence(&mut self, window: WindowKey, inside: bool, cx: &mut Cx<Msg>) {
        // Off the window from a control (to another output, or under
        // another window): the bar doesn't hear it go, so the controls go
        // as though the pointer had rested.
        if window == self.window && !inside && self.over_controls {
            self.over_controls = false;
            self.rest_controls(cx);
        }
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if Some(window) == self.sheet {
            cx.send(Msg::CloseSheet);
            return false;
        }
        self.leave();
        self.store.prefs.volume = self.volume;
        self.store.prefs.muted = self.muted;
        self.store.save_prefs();
        cx.quit();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_as_people_count() {
        let mut v = vec!["Episode 10.mkv", "episode 2.mkv", "Episode 1.mkv", "Extras.mkv"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["Episode 1.mkv", "episode 2.mkv", "Episode 10.mkv", "Extras.mkv"]);
    }

    #[test]
    fn what_plays_is_known_by_its_name() {
        assert!(playable(Path::new("/a/B.MKV")));
        assert!(playable(Path::new("song.flac")));
        assert!(!playable(Path::new("notes.txt")));
        assert!(!playable(Path::new("folder")));
    }
}
