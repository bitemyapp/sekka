//! Shashin, the program: the picture showing, the folder it's in, how
//! it's zoomed and turned, and what's being read and drawn in the
//! background.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use aizome_kit::MsgSender;
use aizome_kit::prelude::*;
use image::RgbaImage;
use resvg::usvg;

use crate::folder;
use crate::load::{self, Kind};
use crate::thumbs;
use crate::trash::{Trash, Trashed};
use crate::view::{self, Zoom};

pub const APP_ID: &str = "org.aizome.Shashin";

/// The toolbar's height, and the filmstrip's.
pub const TOOLBAR: f64 = 30.0;
pub const FILMSTRIP: f64 = 76.0;

/// How long each picture shows in a slideshow.
pub const SLIDE: Duration = Duration::from_secs(5);

/// A drawing of the picture, made for one zoom and turn: the whole of it,
/// or the part of it around what shows.
#[derive(Clone)]
pub struct Tile {
    /// Device pixels per picture pixel.
    pub z: f64,
    /// The quarter turns of the picture it shows.
    pub turns: u8,
    /// Where it sits in the picture scaled by `z`, in device pixels.
    pub at: (f64, f64),
    pub image: Image,
    /// Different for every tile, for the stage's hash.
    pub id: u64,
    /// Every pixel opaque (a photo): drawn in place of what's under it.
    pub opaque: bool,
}

impl Tile {
    pub fn size(&self) -> (f64, f64) {
        let (w, h) = self.image.pixel_size();
        (f64::from(w), f64::from(h))
    }

    /// Whether it's the whole picture.
    pub fn whole(&self, dims: (f64, f64)) -> bool {
        let (w, h) = self.size();
        self.at == (0.0, 0.0) && (w - dims.0 * self.z).abs() < 1.5 && (h - dims.1 * self.z).abs() < 1.5
    }

    /// Whether it shows all of `rect` (device pixels of the scaled picture)
    /// at zoom `z` exactly.
    pub fn covers(&self, z: f64, rect: (f64, f64, f64, f64)) -> bool {
        let (w, h) = self.size();
        same_zoom(self.z, z)
            && self.at.0 <= rect.0 + 0.5
            && self.at.1 <= rect.1 + 0.5
            && self.at.0 + w >= rect.0 + rect.2 - 0.5
            && self.at.1 + h >= rect.1 + rect.3 - 0.5
    }

    fn bytes(&self) -> u64 {
        let (w, h) = self.image.pixel_size();
        u64::from(w) * u64::from(h) * 4
    }
}

pub fn same_zoom(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.max(b)
}

static TILE_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn tile(z: f64, turns: u8, at: (f64, f64), img: &RgbaImage, opaque: bool) -> Option<Tile> {
    let image = load::to_image(img)?;
    Some(Tile { z, turns, at, image, id: TILE_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed), opaque })
}

/// What a picture is drawn from.
#[derive(Clone)]
pub enum Body {
    /// Pixels, upright (not yet turned by the user), and whether every
    /// one is opaque.
    Raster(Arc<RgbaImage>, bool),
    /// A drawing, rendered at any zoom.
    Svg(Arc<usvg::Tree>),
    /// An animation's frames, each with how long it shows.
    Frames(Arc<Vec<(Image, Duration)>>, Arc<RgbaImage>),
}

/// A picture read, decoded and ready to show (the current one, or one
/// read ahead).
#[derive(Clone)]
pub struct Prepared {
    pub path: PathBuf,
    pub kind: Kind,
    /// Pixels, upright.
    pub dims: (u32, u32),
    pub orientation: u8,
    pub file_size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub exif: Vec<(String, String)>,
    pub body: Option<Body>,
    /// Drawn to fit the view the program had when it was read.
    pub fit: Option<Tile>,
    pub error: Option<String>,
}

impl Prepared {
    fn failed(path: &Path, error: String) -> Prepared {
        Prepared {
            path: path.to_path_buf(),
            kind: Kind::Png,
            dims: (0, 0),
            orientation: 1,
            file_size: 0,
            modified: None,
            created: None,
            exif: Vec::new(),
            body: None,
            fit: None,
            error: Some(error),
        }
    }

    fn bytes(&self) -> u64 {
        let body = match &self.body {
            Some(Body::Raster(b, _)) => b.as_raw().len() as u64,
            Some(Body::Frames(f, b)) => {
                f.iter().map(|(i, _)| u64::from(i.pixel_size().0) * u64::from(i.pixel_size().1) * 4).sum::<u64>()
                    + b.as_raw().len() as u64
            }
            _ => 0,
        };
        body + self.fit.as_ref().map_or(0, Tile::bytes)
    }
}

/// The picture showing.
pub struct Picture {
    pub serial: u64,
    pub info: Prepared,
    /// Drawings of it, oldest first.
    pub tiles: Vec<Tile>,
    /// A full-size, premultiplied copy for 100% and over, once asked for.
    pub frame: usize,
    pub decoded: bool,
}

impl Picture {
    pub fn dims(&self) -> (f64, f64) {
        (f64::from(self.info.dims.0), f64::from(self.info.dims.1))
    }

    fn add_tile(&mut self, t: Tile) {
        self.tiles.push(t);
        // Keep the newest few, and under 320 MiB.
        let mut total: u64 = self.tiles.iter().map(Tile::bytes).sum();
        while self.tiles.len() > 6 || (total > 320 << 20 && self.tiles.len() > 1) {
            total -= self.tiles[0].bytes();
            discard(self.tiles.remove(0));
        }
    }
}

/// Free `waste` on a thread of its own rather than the window's: decoded
/// pictures and their drawings are tens of megabytes each, and the system
/// takes milliseconds to take that memory back (unmapping it, or the
/// allocator trimming its heap), which moving to another picture paid in
/// the frame it moved.
fn discard<T: Send + 'static>(waste: T) {
    static BIN: std::sync::OnceLock<Option<std::sync::mpsc::Sender<Box<dyn Send>>>> = std::sync::OnceLock::new();
    let bin = BIN.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Box<dyn Send>>();
        let started = std::thread::Builder::new().name("shashin-free".into()).spawn(move || rx.iter().for_each(drop));
        started.ok().map(|_| tx)
    });
    match bin {
        Some(tx) => {
            if let Err(back) = tx.send(Box::new(waste)) {
                drop(back);
            }
        }
        None => drop(waste),
    }
}

/// What the stage saw when it drew: the view, and whether it had a
/// drawing of the picture made for exactly what showed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub serial: u64,
    pub turns: u8,
    /// The view, in points, and its scale.
    pub view: Size,
    pub scale: f64,
    pub z: f64,
    /// What showed of the scaled picture, in device pixels.
    pub visible: (f64, f64, f64, f64),
    pub exact: bool,
}

/// Which wallpaper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wallpaper {
    Night,
    Day,
    Both,
}

#[derive(Clone)]
pub enum Msg {
    Nothing,
    /// Files or a folder to show (the command line, a second launch, a
    /// drop, the Open panel).
    Open(Vec<PathBuf>),
    OpenPanel,
    Opened(Option<Vec<PathBuf>>),
    Raise(Vec<PathBuf>),
    /// A folder's pictures, read.
    Listed {
        dir: PathBuf,
        files: Vec<PathBuf>,
    },
    /// What the current picture's file says, before it decodes.
    Header {
        serial: u64,
        kind: Kind,
        dims: (u32, u32),
        size: u64,
    },
    Preview {
        serial: u64,
        tile: Tile,
    },
    Decoded {
        serial: u64,
        prepared: Box<Prepared>,
    },
    Prefetched(Box<Prepared>),
    TileMade {
        serial: u64,
        tile: Option<Tile>,
    },
    Seen(Seen),
    // Moving about.
    Step(i32),
    Go(usize),
    First,
    Last,
    // Zoom and pan.
    ZoomIn,
    ZoomOut,
    ActualSize,
    Fit,
    ZoomPick(usize),
    ZoomBy {
        factor: f64,
        at: Point,
        view: Size,
    },
    PanStart {
        at: Point,
        view: Size,
    },
    PanTo(Point),
    PanEnd,
    Scroll {
        by: Vec2,
        view: Size,
    },
    DoubleClick {
        at: Point,
        view: Size,
    },
    // Turning.
    Rotate(i8),
    Save,
    Saved {
        path: PathBuf,
        result: Result<crate::save::Saved, String>,
    },
    Revert,
    // Elsewhere.
    Copy,
    Copied(Result<usize, String>),
    SetWallpaper(Wallpaper),
    WallpaperSet(Result<Wallpaper, String>),
    ShowInHako,
    Trash,
    Undo,
    Redo,
    Info,
    CloseInfo,
    Filmstrip,
    Thumb {
        path: PathBuf,
        image: Option<Image>,
    },
    FullScreen,
    Slideshow,
    Slide,
    StopSlideshow,
    Escape,
    Frame(u64),
    Said(u64),
    SaveAllAndQuit,
    SavedAll(Vec<(PathBuf, Result<crate::save::Saved, String>)>),
    DiscardAndQuit,
}

/// A message shown for a moment in the toolbar.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub error: bool,
    seq: u64,
}

/// A thumbnail in the filmstrip.
#[derive(Clone)]
pub enum Thumb {
    Pending,
    Ready(Image),
    Failed,
}

/// What Undo takes back.
#[derive(Clone, Debug)]
pub struct TrashStep {
    pub trashed: Trashed,
}

pub struct Shashin {
    pub window: WindowKey,
    pub info_sheet: Option<WindowKey>,
    /// The pictures stepped through, in order, and which shows.
    pub folder: Vec<PathBuf>,
    pub index: Option<usize>,
    /// Given on their own (several files): the folder isn't listed.
    pub chosen: bool,
    pub pic: Option<Picture>,
    pub serial: u64,
    /// Quarter turns not yet saved, by picture.
    pub turns: HashMap<PathBuf, u8>,
    pub zoom: Zoom,
    /// The picture's point (its pixels, turned) at the view's centre.
    pub center: (f64, f64),
    /// The stage, in points, and its scale.
    pub view: Size,
    pub scale: f64,
    busy: bool,
    want: Option<Seen>,
    /// Copies, saves and wallpapers under way.
    errands: usize,
    /// Pictures read ahead (the next and previous), newest last.
    pub ahead: Vec<Prepared>,
    reading_ahead: bool,
    pub filmstrip: bool,
    pub thumbs: HashMap<PathBuf, Thumb>,
    thumb_queue: VecDeque<PathBuf>,
    thumbs_running: usize,
    pub thumb_dir: PathBuf,
    pub fullscreen: bool,
    pub slideshow: Option<TimerHandle>,
    /// Whether the slideshow made the window full screen (so it ends it).
    slideshow_full: bool,
    pub undo: Vec<TrashStep>,
    pub redo: Vec<PathBuf>,
    pub note: Option<Note>,
    note_seq: u64,
    pub panning: Option<Point>,
    animation: Option<TimerHandle>,
    pub stream: Option<MsgSender<Msg>>,
    /// The last thing the stage reported, so it reports a change once.
    pub reported: Rc<Cell<u64>>,
    pub trash: Trash,
    /// The settings file (tests point it elsewhere).
    pub settings_path: Option<PathBuf>,
    /// Commands run (`aizome-hako --select …`), and whether to really run
    /// them.
    pub commands: Vec<Vec<String>>,
    pub dry_run: bool,
    /// What was copied last: a PNG's length.
    pub copied: Option<usize>,
    pub instance: Option<std::os::unix::net::UnixListener>,
    pub initial: Vec<PathBuf>,
    /// Whether anything was opened yet (the command line opens once).
    pub opened: bool,
    /// Previews shown before a picture decoded (for tests).
    pub previews: usize,
}

impl Default for Shashin {
    fn default() -> Shashin {
        Shashin::new()
    }
}

impl Shashin {
    pub fn new() -> Shashin {
        Shashin {
            window: WindowKey::NONE,
            info_sheet: None,
            folder: Vec::new(),
            index: None,
            chosen: false,
            pic: None,
            serial: 0,
            turns: HashMap::new(),
            zoom: Zoom::Fit,
            center: (0.0, 0.0),
            view: Size::new(960.0, 640.0 - TOOLBAR),
            scale: 1.0,
            busy: false,
            want: None,
            errands: 0,
            ahead: Vec::new(),
            reading_ahead: false,
            filmstrip: false,
            thumbs: HashMap::new(),
            thumb_queue: VecDeque::new(),
            thumbs_running: 0,
            thumb_dir: thumbs::cache_dir(),
            fullscreen: false,
            slideshow: None,
            slideshow_full: false,
            undo: Vec::new(),
            redo: Vec::new(),
            note: None,
            note_seq: 0,
            panning: None,
            animation: None,
            stream: None,
            reported: Rc::new(Cell::new(0)),
            trash: Trash::from_env(),
            settings_path: None,
            commands: Vec::new(),
            dry_run: false,
            copied: None,
            instance: None,
            initial: Vec::new(),
            opened: false,
            previews: 0,
        }
    }

    // What shows.

    pub fn current_path(&self) -> Option<&PathBuf> {
        self.index.and_then(|i| self.folder.get(i))
    }

    pub fn current_turns(&self) -> u8 {
        self.current_path().and_then(|p| self.turns.get(p)).copied().unwrap_or(0)
    }

    /// The picture's size in pixels as it shows (turned).
    pub fn shown_dims(&self) -> (f64, f64) {
        self.pic.as_ref().map_or((0.0, 0.0), |p| view::turned(p.dims(), self.current_turns()))
    }

    fn view_px(&self) -> (f64, f64) {
        (self.view.width * self.scale, self.view.height * self.scale)
    }

    /// The zoom now, in device pixels per picture pixel.
    pub fn z(&self) -> f64 {
        self.zoom.resolve(self.shown_dims(), self.view_px())
    }

    /// The zoom as a percentage.
    pub fn percent(&self) -> String {
        let p = self.z() * 100.0;
        if p >= 10.0 || p == 0.0 { format!("{p:.0}%") } else { format!("{p:.1}%") }
    }

    /// Whether nothing is being read or drawn in the background.
    pub fn idle(&self) -> bool {
        !self.loading()
            && !self.busy
            && self.errands == 0
            && self.want.is_none()
            && !self.reading_ahead
            && self.thumbs_running == 0
            && self.thumb_queue.is_empty()
    }

    pub fn loading(&self) -> bool {
        self.pic.as_ref().is_some_and(|p| !p.decoded)
    }

    fn say(&mut self, text: impl Into<String>, error: bool, cx: &mut Cx<Msg>) {
        self.note_seq += 1;
        self.note = Some(Note { text: text.into(), error, seq: self.note_seq });
        cx.after(Duration::from_secs(if error { 6 } else { 3 }), Msg::Said(self.note_seq));
    }

    fn title(&self) -> String {
        match self.current_path() {
            Some(p) => {
                let name = folder::display_name(p);
                if self.current_turns() != 0 { format!("{name} — Turned") } else { name }
            }
            None => "Shashin".into(),
        }
    }

    // Opening.

    /// Show `paths`: one file among its folder's pictures, several files on
    /// their own, or a folder's first picture.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        self.opened = true;
        let paths: Vec<PathBuf> = paths.into_iter().map(|p| std::path::absolute(&p).unwrap_or(p)).collect();
        match paths.as_slice() {
            [] => {}
            [one] if one.is_dir() => {
                let dir = one.clone();
                self.chosen = false;
                self.folder.clear();
                self.index = None;
                self.list(dir, None, cx);
            }
            [one] => {
                self.chosen = false;
                self.folder = vec![one.clone()];
                self.index = Some(0);
                self.show(cx);
                if let Some(dir) = one.parent() {
                    self.list(dir.to_path_buf(), Some(one.clone()), cx);
                }
            }
            many => {
                let mut files: Vec<PathBuf> = many.iter().filter(|p| p.is_file()).cloned().collect();
                folder::sort(&mut files);
                self.chosen = true;
                self.folder = files;
                self.index = (!self.folder.is_empty()).then_some(0);
                self.show(cx);
            }
        }
    }

    fn list(&mut self, dir: PathBuf, _select: Option<PathBuf>, cx: &mut Cx<Msg>) {
        let hidden = self.current_path().is_some_and(|p| folder::is_hidden(p));
        cx.spawn(move || {
            let files = folder::list(&dir, hidden);
            Msg::Listed { dir, files }
        });
    }

    /// Start showing the picture at `index`.
    fn show(&mut self, cx: &mut Cx<Msg>) {
        self.serial += 1;
        let serial = self.serial;
        self.zoom = Zoom::Fit;
        self.panning = None;
        self.busy = false;
        self.want = None;
        if let Some(h) = self.animation.take() {
            cx.cancel(h);
        }
        let Some(path) = self.current_path().cloned() else {
            discard(self.pic.take());
            cx.set_title(self.window, "Shashin");
            return;
        };
        cx.set_title(self.window, self.title());
        // Read ahead already?
        if let Some(i) = self.ahead.iter().position(|p| p.path == path && fresh(p)) {
            let prepared = self.ahead.remove(i);
            self.install(serial, prepared, cx);
            return;
        }
        discard(self.pic.take());
        self.pic = Some(Picture {
            serial,
            info: Prepared::failed(&path, String::new()),
            tiles: Vec::new(),
            frame: 0,
            decoded: false,
        });
        if let Some(p) = &mut self.pic {
            p.info.error = None;
        }
        let tx = self.stream.clone();
        let view = self.view_px();
        cx.spawn(move || {
            let prepared = prepare(&path, view, |m| {
                if let Some(tx) = &tx {
                    tx.send(match m {
                        Early::Header { kind, dims, size } => Msg::Header { serial, kind, dims, size },
                        Early::Preview(tile) => Msg::Preview { serial, tile },
                    });
                }
            });
            Msg::Decoded { serial, prepared: Box::new(prepared) }
        });
    }

    /// Show what was read.
    fn install(&mut self, serial: u64, prepared: Prepared, cx: &mut Cx<Msg>) {
        let mut tiles = Vec::new();
        if let Some(t) = &prepared.fit {
            tiles.push(t.clone());
        }
        let keep = match self.pic.take() {
            Some(p) if p.serial == serial => p.tiles,
            Some(p) => {
                discard(p);
                Vec::new()
            }
            None => Vec::new(),
        };
        // A preview stays until something better is drawn.
        let mut all = keep;
        all.extend(tiles);
        self.pic = Some(Picture { serial, info: prepared, tiles: all, frame: 0, decoded: true });
        self.center = {
            let d = self.shown_dims();
            (d.0 / 2.0, d.1 / 2.0)
        };
        if let Some(Body::Frames(frames, _)) = self.pic.as_ref().and_then(|p| p.info.body.clone()) {
            self.animation = Some(cx.after(frames[0].1, Msg::Frame(serial)));
        }
        cx.set_title(self.window, self.title());
        // A turn not yet saved: draw it turned.
        if self.current_turns() != 0 {
            self.ask_for_view(cx);
        }
        self.read_ahead(cx);
        self.queue_thumbs(cx);
    }

    /// Ask for a drawing of what shows now, as the stage would.
    fn ask_for_view(&mut self, cx: &mut Cx<Msg>) {
        let Some(p) = &self.pic else { return };
        let dims = self.shown_dims();
        let view = self.view_px();
        let z = self.z();
        let c = view::clamp_center(dims, view, z, self.center);
        let seen = Seen {
            serial: p.serial,
            turns: self.current_turns(),
            view: self.view,
            scale: self.scale,
            z,
            visible: view::visible(dims, view, z, c),
            exact: false,
        };
        self.want = Some(seen);
        self.run_jobs(cx);
    }

    /// Read the next and previous pictures ahead, one at a time, while
    /// nothing else is being read.
    fn read_ahead(&mut self, cx: &mut Cx<Msg>) {
        if self.reading_ahead || self.loading() || self.busy {
            return;
        }
        let Some(i) = self.index else { return };
        let n = self.folder.len();
        if n < 2 {
            return;
        }
        let candidates = [(i + 1) % n, (i + n - 1) % n];
        let Some(path) = candidates
            .iter()
            .map(|&j| self.folder[j].clone())
            .find(|p| Some(p) != self.current_path() && !self.ahead.iter().any(|a| &a.path == p))
        else {
            return;
        };
        self.reading_ahead = true;
        let view = self.view_px();
        cx.spawn(move || Msg::Prefetched(Box::new(prepare(&path, view, |_| {}))));
    }

    fn keep_ahead(&mut self, p: Prepared) {
        // Only the neighbours are worth keeping.
        let near: Vec<PathBuf> = match self.index {
            Some(i) if !self.folder.is_empty() => {
                let n = self.folder.len();
                vec![self.folder[(i + 1) % n].clone(), self.folder[(i + n - 1) % n].clone()]
            }
            _ => Vec::new(),
        };
        let (kept, gone): (Vec<Prepared>, Vec<Prepared>) =
            std::mem::take(&mut self.ahead).into_iter().partition(|a| near.contains(&a.path) && a.path != p.path);
        self.ahead = kept;
        discard(gone);
        if near.contains(&p.path) && p.error.is_none() {
            self.ahead.push(p);
        } else {
            discard(p);
        }
        let mut total: u64 = self.ahead.iter().map(Prepared::bytes).sum();
        while total > 640 << 20 && !self.ahead.is_empty() {
            total -= self.ahead[0].bytes();
            discard(self.ahead.remove(0));
        }
    }

    // Drawings.

    /// Make the drawing the stage wants, one job at a time.
    fn run_jobs(&mut self, cx: &mut Cx<Msg>) {
        if self.busy {
            return;
        }
        let Some(want) = self.want.take() else { return };
        let Some(pic) = &self.pic else { return };
        if want.serial != pic.serial || !pic.decoded || want.turns != self.current_turns() {
            return;
        }
        let dims = view::turned(pic.dims(), want.turns);
        let z = want.z;
        if pic.tiles.iter().any(|t| t.turns == want.turns && t.covers(z, want.visible)) {
            return;
        }
        let (serial, turns) = (pic.serial, want.turns);
        let (sw, sh) = (dims.0 * z, dims.1 * z);
        // A part around what shows, with a margin to pan into.
        let region = || {
            let (x, y, w, h) = want.visible;
            let (mx, my) = (w * 0.25, h * 0.25);
            let x0 = (x - mx).max(0.0).floor();
            let y0 = (y - my).max(0.0).floor();
            let x1 = (x + w + mx).min(sw).ceil();
            let y1 = (y + h + my).min(sh).ceil();
            (x0 as u32, y0 as u32, (x1 - x0).max(1.0) as u32, (y1 - y0).max(1.0) as u32)
        };
        let job: Box<dyn FnOnce() -> Option<Tile> + Send> = match pic.info.body.clone() {
            Some(Body::Raster(base, opaque)) => {
                if z > 2.0 + 1e-9 || same_zoom(z, 1.0) {
                    // Drawn from the full picture, nearest pixel above 200%.
                    if pic.tiles.iter().any(|t| t.turns == turns && same_zoom(t.z, 1.0) && t.whole(dims)) {
                        return;
                    }
                    Box::new(move || {
                        let turned = load::turn((*base).clone(), turns);
                        tile(1.0, turns, (0.0, 0.0), &turned, opaque)
                    })
                } else if z < 1.0 && sw * sh <= 40e6 {
                    Box::new(move || {
                        // Scale first, then turn: turning the small one is cheap.
                        let (tw, th) = if turns % 2 == 1 { (sh, sw) } else { (sw, sh) };
                        let scaled = load::scale_to(&base, tw.round() as u32, th.round() as u32);
                        tile(z, turns, (0.0, 0.0), &load::turn(scaled, turns), opaque)
                    })
                } else {
                    let r = region();
                    Box::new(move || {
                        tile(z, turns, (f64::from(r.0), f64::from(r.1)), &load::region(&base, turns, z, r), opaque)
                    })
                }
            }
            Some(Body::Svg(tree)) => {
                let r = if sw * sh <= 16e6 {
                    (0, 0, sw.round().max(1.0) as u32, sh.round().max(1.0) as u32)
                } else {
                    region()
                };
                Box::new(move || {
                    // Drawn unturned, then turned.
                    let (bw, bh) = (f64::from(load::svg_size(&tree).0) * z, f64::from(load::svg_size(&tree).1) * z);
                    let u = view::unturn_rect(
                        (f64::from(r.0), f64::from(r.1), f64::from(r.2), f64::from(r.3)),
                        (bw, bh),
                        turns,
                    );
                    let region = (
                        u.0.round().max(0.0) as u32,
                        u.1.round().max(0.0) as u32,
                        u.2.round().max(1.0) as u32,
                        u.3.round().max(1.0) as u32,
                    );
                    let img = load::render_svg(&tree, z, region)?;
                    tile(z, turns, (f64::from(r.0), f64::from(r.1)), &load::turn(img, turns), false)
                })
            }
            _ => return,
        };
        self.busy = true;
        cx.spawn(move || Msg::TileMade { serial, tile: job() });
    }

    // Moving about.

    fn step(&mut self, by: i32, cx: &mut Cx<Msg>) {
        let n = self.folder.len();
        let Some(i) = self.index else { return };
        if n < 2 {
            return;
        }
        let j = (i as i64 + i64::from(by)).rem_euclid(n as i64) as usize;
        self.go(j, cx);
    }

    fn go(&mut self, j: usize, cx: &mut Cx<Msg>) {
        if j < self.folder.len() && Some(j) != self.index {
            self.index = Some(j);
            self.show(cx);
            self.queue_thumbs(cx);
        }
    }

    // Zoom.

    fn zoom_to(&mut self, to: f64, at: Option<(f64, f64)>) {
        let dims = self.shown_dims();
        let view = self.view_px();
        let z = self.z();
        let to = view::clamp_zoom(to);
        let at = at.unwrap_or((view.0 / 2.0, view.1 / 2.0));
        let c = view::clamp_center(dims, view, z, self.center);
        self.center = view::zoom_about(dims, view, z, c, to, at);
        self.zoom = Zoom::Fixed(to);
    }

    fn learn_view(&mut self, view: Size) {
        if view.width > 0.0 && view.height > 0.0 {
            self.view = view;
        }
    }

    // Filmstrip.

    fn queue_thumbs(&mut self, cx: &mut Cx<Msg>) {
        if !self.filmstrip {
            return;
        }
        let Some(i) = self.index else { return };
        let n = self.folder.len() as i64;
        self.thumb_queue.clear();
        // Nearest first.
        for d in 0..=14i64 {
            for j in [i as i64 + d, i as i64 - d] {
                if (0..n).contains(&j) {
                    let p = self.folder[j as usize].clone();
                    if !self.thumbs.contains_key(&p) && !self.thumb_queue.contains(&p) {
                        self.thumb_queue.push_back(p);
                    }
                }
            }
        }
        self.run_thumbs(cx);
    }

    fn run_thumbs(&mut self, cx: &mut Cx<Msg>) {
        // The picture itself comes first.
        while self.thumbs_running < 2 && !self.loading() {
            let Some(path) = self.thumb_queue.pop_front() else { return };
            self.thumbs.insert(path.clone(), Thumb::Pending);
            self.thumbs_running += 1;
            let dir = self.thumb_dir.clone();
            cx.spawn(move || {
                let img = make_thumb(&dir, &path);
                Msg::Thumb { path, image: img.as_ref().and_then(load::to_image) }
            });
        }
    }

    // Elsewhere.

    fn trash_current(&mut self, cx: &mut Cx<Msg>) {
        let Some(i) = self.index else { return };
        let path = self.folder[i].clone();
        match self.trash.trash(&path) {
            Ok(t) => {
                self.undo.push(TrashStep { trashed: t });
                self.redo.clear();
                self.turns.remove(&path);
                self.thumbs.remove(&path);
                self.ahead.retain(|a| a.path != path);
                self.folder.remove(i);
                self.index = if self.folder.is_empty() { None } else { Some(i.min(self.folder.len() - 1)) };
                self.show(cx);
                self.say(format!("Moved “{}” to the Trash", folder::display_name(&path)), false, cx);
            }
            Err(e) => {
                let why = load::describe(&e);
                self.say(format!("Couldn't move it to the Trash: {why}"), true, cx);
            }
        }
    }

    fn undo_trash(&mut self, cx: &mut Cx<Msg>) {
        let Some(step) = self.undo.pop() else { return };
        match self.trash.restore(&step.trashed) {
            Ok(()) => {
                let path = step.trashed.original.clone();
                let at = folder::insertion_point(&self.folder, &path);
                self.folder.insert(at, path.clone());
                self.index = Some(at);
                self.redo.push(path);
                self.show(cx);
                self.queue_thumbs(cx);
            }
            Err(e) => {
                self.say(format!("Couldn't put it back: {}", load::describe(&e)), true, cx);
            }
        }
    }

    fn run(&mut self, args: Vec<String>) {
        if !self.dry_run
            && let Some((cmd, rest)) = args.split_first()
            && let Err(e) = std::process::Command::new(cmd).args(rest).spawn()
        {
            log::warn!("{cmd}: {e}");
        }
        self.commands.push(args);
    }

    fn set_wallpaper(&mut self, which: Wallpaper, cx: &mut Cx<Msg>) {
        let Some(path) = self.current_path().cloned() else { return };
        let settings = self.settings_path.clone().unwrap_or_else(aizome_config::Settings::path);
        let tell = !self.dry_run;
        self.errands += 1;
        cx.spawn(move || {
            let r = (|| {
                // The viewer also reads SVG, but the desktop's decoder does
                // not. Check the file again (including all pixels), before
                // touching either theme's setting or notifying the desktop.
                aizome_face::wallpaper::validate(&path)?;
                let mut s = aizome_config::Settings::load_from(&settings).map_err(|e| e.to_string())?;
                let p = Some(path.to_string_lossy().into_owned());
                if matches!(which, Wallpaper::Night | Wallpaper::Both) {
                    s.appearance.wallpaper_night = p.clone();
                }
                if matches!(which, Wallpaper::Day | Wallpaper::Both) {
                    s.appearance.wallpaper_day = p;
                }
                s.save_to(&settings).map_err(|e| e.to_string())?;
                // The compositor applies it; the shell follows.
                if tell && aizome_ipc::socket_path().is_some() {
                    let r =
                        aizome_ipc::Client::connect().and_then(|mut c| c.request(&aizome_ipc::Request::ReloadConfig));
                    if let Err(e) = r {
                        log::info!("reload-config: {e}");
                    }
                }
                Ok(which)
            })();
            Msg::WallpaperSet(r)
        });
    }

    fn copy(&mut self, cx: &mut Cx<Msg>) {
        let Some(pic) = &self.pic else { return };
        let turns = self.current_turns();
        let base = match &pic.info.body {
            Some(Body::Raster(b, _)) => b.clone(),
            Some(Body::Frames(_, first)) => first.clone(),
            Some(Body::Svg(tree)) => {
                let tree = tree.clone();
                let dims = load::svg_size(&tree);
                self.errands += 1;
                cx.spawn(move || {
                    let r = load::render_svg(&tree, 1.0, (0, 0, dims.0, dims.1))
                        .map(|img| load::turn(img, turns))
                        .and_then(|img| crate::clipboard::encode_png(&img))
                        .ok_or_else(|| "The SVG didn't draw.".to_owned());
                    Msg::Copied(r.map(|png| {
                        let n = png.len();
                        crate::clipboard::offer_png(png);
                        n
                    }))
                });
                return;
            }
            None => return,
        };
        self.errands += 1;
        cx.spawn(move || {
            let img = load::turn((*base).clone(), turns);
            let r = crate::clipboard::encode_png(&img).ok_or_else(|| "The picture didn't encode.".to_owned());
            Msg::Copied(r.map(|png| {
                let n = png.len();
                crate::clipboard::offer_png(png);
                n
            }))
        });
    }

    fn start_slideshow(&mut self, cx: &mut Cx<Msg>) {
        if self.slideshow.is_some() || self.folder.is_empty() {
            return;
        }
        self.slideshow = Some(cx.every(SLIDE, Msg::Slide));
        self.slideshow_full = !self.fullscreen;
        if !self.fullscreen {
            crate::platform::set_fullscreen(cx, self.window, true);
            self.fullscreen = true;
        }
        self.zoom = Zoom::Fit;
    }

    fn stop_slideshow(&mut self, cx: &mut Cx<Msg>) {
        if let Some(h) = self.slideshow.take() {
            cx.cancel(h);
            if self.slideshow_full {
                crate::platform::set_fullscreen(cx, self.window, false);
                self.fullscreen = false;
            }
        }
    }
}

/// Whether a picture read ahead is still what's in its file.
fn fresh(p: &Prepared) -> bool {
    std::fs::metadata(&p.path).ok().and_then(|m| m.modified().ok()) == p.modified
}

/// What a read tells before it's done.
pub enum Early {
    Header { kind: Kind, dims: (u32, u32), size: u64 },
    Preview(Tile),
}

/// Read and decode `path`, telling `early` what it learns on the way, and
/// draw it to fit a view of `view` device pixels.
pub fn prepare(path: &Path, view: (f64, f64), early: impl Fn(Early)) -> Prepared {
    let src = match load::read(path) {
        Ok(s) => s,
        Err(e) => return Prepared::failed(path, e),
    };
    let size = src.bytes.len() as u64;
    let mut out = Prepared {
        path: path.to_path_buf(),
        kind: src.kind,
        dims: (0, 0),
        orientation: 1,
        file_size: size,
        modified: src.modified,
        created: src.created,
        exif: Vec::new(),
        body: None,
        fit: None,
        error: None,
    };
    if src.kind == Kind::Svg {
        match load::parse_svg(&src) {
            Ok(tree) => {
                out.dims = load::svg_size(&tree);
                early(Early::Header { kind: src.kind, dims: out.dims, size });
                let d = (f64::from(out.dims.0), f64::from(out.dims.1));
                let z = view::fit(d, view);
                let (w, h) = ((d.0 * z).round().max(1.0) as u32, (d.1 * z).round().max(1.0) as u32);
                out.fit = load::render_svg(&tree, z, (0, 0, w, h)).and_then(|img| tile(z, 0, (0.0, 0.0), &img, false));
                out.body = Some(Body::Svg(tree));
            }
            Err(e) => out.error = Some(e),
        }
        return out;
    }
    let (dims, orientation) = match load::header(&src) {
        Ok(h) => h,
        Err(e) => {
            out.error = Some(format!("It doesn't read as a picture ({e})."));
            return out;
        }
    };
    out.dims = dims;
    out.orientation = orientation;
    early(Early::Header { kind: src.kind, dims, size });
    let d = (f64::from(dims.0), f64::from(dims.1));
    let z = view::fit(d, view);
    let fit_size = ((d.0 * z).round().max(1.0) as u32, (d.1 * z).round().max(1.0) as u32);
    let exif = load::exif(&src);
    if let Some(ex) = &exif {
        out.exif = load::exif_details(ex);
        // A camera's own small copy, while the big one decodes.
        if size > 512 << 10
            && let Some(thumb) = load::exif_thumbnail(ex, orientation)
            && (f64::from(thumb.width()) / f64::from(thumb.height()) - d.0 / d.1).abs() < 0.05
            && let Some(t) = tile(z, 0, (0.0, 0.0), &load::scale_to(&thumb, fit_size.0, fit_size.1), true)
        {
            early(Early::Preview(t));
        }
    }
    if let Some(frames) = load::frames(&src) {
        let first = Arc::new(frames[0].0.clone());
        let images: Vec<(Image, Duration)> =
            frames.iter().filter_map(|(f, d)| Some((load::to_image(f)?, *d))).collect();
        if images.len() == frames.len() {
            out.body = Some(Body::Frames(Arc::new(images), first));
            return out;
        }
    }
    match load::decode_opaque(&src) {
        Ok((img, opaque)) => {
            out.dims = img.dimensions();
            out.fit = tile(z, 0, (0.0, 0.0), &load::scale_to(&img, fit_size.0, fit_size.1), opaque);
            out.body = Some(Body::Raster(Arc::new(img), opaque));
        }
        Err(e) => out.error = Some(format!("It doesn't decode ({e}).")),
    }
    out
}

/// A thumbnail for the filmstrip: the cache's, or made (and kept there).
pub fn make_thumb(dir: &Path, path: &Path) -> Option<RgbaImage> {
    if let Some(t) = thumbs::cached(dir, path) {
        return Some(t);
    }
    let src = load::read(path).ok()?;
    let img = if src.kind == Kind::Svg {
        let tree = load::parse_svg(&src).ok()?;
        let (w, h) = load::svg_size(&tree);
        let z = f64::from(thumbs::NORMAL) / f64::from(w.max(h));
        let (tw, th) = ((f64::from(w) * z).round().max(1.0) as u32, (f64::from(h) * z).round().max(1.0) as u32);
        load::render_svg(&tree, z, (0, 0, tw, th))?
    } else {
        let (dims, orientation) = load::header(&src).ok()?;
        let quick = load::exif(&src).and_then(|ex| load::exif_thumbnail(&ex, orientation)).filter(|t| {
            t.width().max(t.height()) >= 120
                && (f64::from(t.width()) / f64::from(t.height()) - f64::from(dims.0) / f64::from(dims.1)).abs() < 0.05
        });
        match quick {
            Some(t) => t,
            None => load::decode(&src).ok()?,
        }
    };
    let thumb = thumbs::shrink(&img);
    if let Err(e) = thumbs::store(dir, path, &thumb) {
        log::debug!("thumbnail of {}: {e}", path.display());
    }
    Some(thumb)
}

impl Program for Shashin {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Shashin", "Images").icon(Icon::Image).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.scale = cx.outputs().iter().map(|o| o.scale).fold(1.0, f64::max);
        self.window = cx.open_window(WindowSpec::new("Shashin").size(960.0, 640.0).min_size(360.0, 260.0));
        self.view = Size::new(960.0, 640.0 - TOOLBAR);
        self.stream = Some(cx.stream());
        cx.focus("stage");
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), Msg::Raise);
        }
        let initial = std::mem::take(&mut self.initial);
        if !initial.is_empty() {
            self.open_paths(initial, cx);
        }
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        if !self.opened {
            self.open_paths(paths, cx);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Nothing => {}
            Msg::Open(paths) => self.open_paths(paths, cx),
            Msg::OpenPanel => {
                let dir = self.current_path().and_then(|p| p.parent()).map(Path::to_path_buf);
                let mut d = FileDialog::new().title("Open a Picture").filter("Pictures", folder::EXTENSIONS).multiple();
                if let Some(dir) = dir {
                    d = d.directory(dir);
                }
                cx.open_panel(self.window, d, Msg::Opened);
            }
            Msg::Opened(Some(paths)) => self.open_paths(paths, cx),
            Msg::Opened(None) => {}
            Msg::Raise(paths) => {
                if !paths.is_empty() {
                    self.open_paths(paths, cx);
                }
                cx.activate(self.window);
            }
            Msg::Listed { dir, files } => {
                if self.chosen {
                    return;
                }
                let current = self.current_path().cloned();
                let same_dir = current.as_ref().is_none_or(|c| c.parent() == Some(dir.as_path()));
                if !same_dir {
                    return;
                }
                match current {
                    Some(c) => {
                        let mut files = files;
                        if !files.contains(&c) {
                            // Shown though it's not named as a picture.
                            let at = folder::insertion_point(&files, &c);
                            files.insert(at, c.clone());
                        }
                        self.index = files.iter().position(|p| *p == c);
                        self.folder = files;
                    }
                    None => {
                        self.folder = files;
                        self.index = (!self.folder.is_empty()).then_some(0);
                        self.show(cx);
                    }
                }
                self.read_ahead(cx);
                self.queue_thumbs(cx);
            }
            Msg::Header { serial, kind, dims, size } => {
                if let Some(p) = self.pic.as_mut().filter(|p| p.serial == serial && !p.decoded) {
                    p.info.kind = kind;
                    p.info.dims = dims;
                    p.info.file_size = size;
                    let d = self.shown_dims();
                    self.center = (d.0 / 2.0, d.1 / 2.0);
                }
            }
            Msg::Preview { serial, tile } => {
                if let Some(p) = self.pic.as_mut().filter(|p| p.serial == serial && !p.decoded) {
                    p.tiles.push(tile);
                    self.previews += 1;
                }
            }
            Msg::Decoded { serial, prepared } => {
                if serial == self.serial {
                    self.install(serial, *prepared, cx);
                    self.run_thumbs(cx);
                } else {
                    self.keep_ahead(*prepared);
                }
            }
            Msg::Prefetched(p) => {
                self.reading_ahead = false;
                self.keep_ahead(*p);
                self.read_ahead(cx);
            }
            Msg::TileMade { serial, tile } => {
                self.busy = false;
                if let (Some(pic), Some(t)) = (self.pic.as_mut(), tile)
                    && pic.serial == serial
                {
                    pic.add_tile(t);
                }
                self.run_jobs(cx);
                self.read_ahead(cx);
            }
            Msg::Seen(seen) => {
                self.view = seen.view;
                self.scale = seen.scale;
                if !seen.exact {
                    self.want = Some(seen);
                    self.run_jobs(cx);
                }
            }
            Msg::Step(by) => {
                self.step(by, cx);
            }
            Msg::Go(i) => self.go(i, cx),
            Msg::First => self.go(0, cx),
            Msg::Last => self.go(self.folder.len().saturating_sub(1), cx),
            Msg::ZoomIn => {
                let z = view::step_in(self.z());
                self.zoom_to(z, None);
            }
            Msg::ZoomOut => {
                let z = view::step_out(self.z());
                let fit = view::fit(self.shown_dims(), self.view_px());
                if z <= fit && self.z() > fit {
                    self.zoom = Zoom::Fit;
                } else {
                    self.zoom_to(z, None);
                }
            }
            Msg::ActualSize => self.zoom_to(1.0, None),
            Msg::Fit => self.zoom = Zoom::Fit,
            Msg::ZoomPick(i) => match crate::ui::ZOOM_CHOICES.get(i) {
                Some(None) => self.zoom = Zoom::Fit,
                Some(Some(z)) => self.zoom_to(*z, None),
                None => {}
            },
            Msg::ZoomBy { factor, at, view } => {
                self.learn_view(view);
                let to = self.z() * factor;
                let fit = view::fit(self.shown_dims(), self.view_px());
                self.zoom_to(to, Some((at.x * self.scale, at.y * self.scale)));
                // Pinching back to the fit snaps to it.
                if factor < 1.0 && (to - fit).abs() < fit * 0.02 {
                    self.zoom = Zoom::Fit;
                }
            }
            Msg::PanStart { at, view } => {
                self.learn_view(view);
                self.panning = Some(at);
            }
            Msg::PanTo(at) => {
                if let Some(last) = self.panning {
                    let by = ((last.x - at.x) * self.scale, (last.y - at.y) * self.scale);
                    self.pan(by);
                    self.panning = Some(at);
                }
            }
            Msg::PanEnd => self.panning = None,
            Msg::Scroll { by, view } => {
                self.learn_view(view);
                self.pan((by.x * self.scale, by.y * self.scale));
            }
            Msg::DoubleClick { at, view } => {
                self.learn_view(view);
                let fit = view::fit(self.shown_dims(), self.view_px());
                if self.zoom == Zoom::Fit && fit < 1.0 {
                    self.zoom_to(1.0, Some((at.x * self.scale, at.y * self.scale)));
                } else {
                    self.zoom = Zoom::Fit;
                }
            }
            Msg::Rotate(by) => {
                let Some(path) = self.current_path().cloned() else { return };
                let t = (i16::from(self.current_turns()) + i16::from(by)).rem_euclid(4) as u8;
                if t == 0 {
                    self.turns.remove(&path);
                } else {
                    self.turns.insert(path, t);
                }
                let d = self.shown_dims();
                self.zoom = Zoom::Fit;
                self.center = (d.0 / 2.0, d.1 / 2.0);
                cx.set_title(self.window, self.title());
                self.ask_for_view(cx);
            }
            Msg::Save => {
                let Some(path) = self.current_path().cloned() else { return };
                let turns = self.current_turns();
                if turns == 0 {
                    return;
                }
                let p = path.clone();
                self.errands += 1;
                cx.spawn(move || Msg::Saved { path: p.clone(), result: crate::save::save_turned(&p, turns) });
            }
            Msg::Saved { path, result } => {
                self.errands = self.errands.saturating_sub(1);
                match result {
                    Ok(how) => {
                        self.turns.remove(&path);
                        self.thumbs.remove(&path);
                        self.ahead.retain(|a| a.path != path);
                        if self.current_path() == Some(&path) {
                            self.show(cx);
                        }
                        let name = folder::display_name(&path);
                        let said = match how {
                            crate::save::Saved::Lossless => format!("Saved “{name}”, turned without re-encoding"),
                            crate::save::Saved::Rewritten => format!("Saved “{name}”"),
                        };
                        self.say(said, false, cx);
                        self.queue_thumbs(cx);
                    }
                    Err(e) => self.say(format!("Couldn't save: {e}"), true, cx),
                }
            }
            Msg::Revert => {
                if let Some(path) = self.current_path().cloned() {
                    self.turns.remove(&path);
                    self.zoom = Zoom::Fit;
                    let d = self.shown_dims();
                    self.center = (d.0 / 2.0, d.1 / 2.0);
                    cx.set_title(self.window, self.title());
                }
            }
            Msg::Copy => self.copy(cx),
            Msg::Copied(r) => {
                self.errands = self.errands.saturating_sub(1);
                match r {
                    Ok(n) => {
                        self.copied = Some(n);
                        self.say("Copied the picture", false, cx);
                    }
                    Err(e) => self.say(format!("Couldn't copy: {e}"), true, cx),
                }
            }
            Msg::SetWallpaper(which) => self.set_wallpaper(which, cx),
            Msg::WallpaperSet(r) => {
                self.errands = self.errands.saturating_sub(1);
                match r {
                    Ok(w) => {
                        let which = match w {
                            Wallpaper::Night => "Night's",
                            Wallpaper::Day => "Day's",
                            Wallpaper::Both => "the",
                        };
                        self.say(format!("Set as {which} wallpaper"), false, cx);
                    }
                    Err(e) => self.say(format!("Wallpaper unchanged: {e}"), true, cx),
                }
            }
            Msg::ShowInHako => {
                if let Some(p) = self.current_path().cloned() {
                    self.run(vec!["aizome-hako".into(), "--select".into(), p.to_string_lossy().into_owned()]);
                }
            }
            Msg::Trash => self.trash_current(cx),
            Msg::Undo => self.undo_trash(cx),
            Msg::Redo => {
                if let Some(path) = self.redo.pop()
                    && let Some(i) = self.folder.iter().position(|p| *p == path)
                {
                    self.index = Some(i);
                    let keep = std::mem::take(&mut self.redo);
                    self.trash_current(cx);
                    self.redo = keep;
                }
            }
            Msg::Info => {
                if self.info_sheet.is_none() && self.pic.is_some() {
                    self.info_sheet = Some(cx.show_sheet(self.window));
                    cx.focus("info-done");
                }
            }
            Msg::CloseInfo => {
                if let Some(k) = self.info_sheet.take() {
                    cx.close_window(k);
                }
                cx.focus("stage");
            }
            Msg::Filmstrip => {
                self.filmstrip = !self.filmstrip;
                let h = if self.filmstrip { -FILMSTRIP } else { FILMSTRIP };
                self.view = Size::new(self.view.width, (self.view.height + h).max(1.0));
                self.queue_thumbs(cx);
            }
            Msg::Thumb { path, image } => {
                self.thumbs_running = self.thumbs_running.saturating_sub(1);
                if self.thumbs.contains_key(&path) {
                    self.thumbs.insert(path, image.map_or(Thumb::Failed, Thumb::Ready));
                }
                self.run_thumbs(cx);
            }
            Msg::FullScreen => {
                let on = !self.fullscreen;
                crate::platform::set_fullscreen(cx, self.window, on);
                self.fullscreen = on;
                cx.focus("stage");
                if !on {
                    self.stop_slideshow(cx);
                }
            }
            Msg::Slideshow => {
                if self.slideshow.is_some() {
                    self.stop_slideshow(cx);
                } else {
                    self.start_slideshow(cx);
                }
            }
            Msg::Slide => {
                self.step(1, cx);
            }
            Msg::StopSlideshow => self.stop_slideshow(cx),
            Msg::Escape => {
                if self.slideshow.is_some() {
                    self.stop_slideshow(cx);
                } else if self.fullscreen {
                    crate::platform::set_fullscreen(cx, self.window, false);
                    self.fullscreen = false;
                }
            }
            Msg::Frame(serial) => {
                self.animation = None;
                if let Some(pic) = self.pic.as_mut().filter(|p| p.serial == serial)
                    && let Some(Body::Frames(frames, _)) = &pic.info.body
                {
                    pic.frame = (pic.frame + 1) % frames.len();
                    let d = frames[pic.frame].1;
                    self.animation = Some(cx.after(d, Msg::Frame(serial)));
                }
            }
            Msg::Said(seq) => {
                if self.note.as_ref().is_some_and(|n| n.seq == seq) {
                    self.note = None;
                }
            }
            Msg::SaveAllAndQuit => {
                let all: Vec<(PathBuf, u8)> = self.turns.iter().map(|(p, t)| (p.clone(), *t)).collect();
                self.errands += 1;
                cx.spawn(move || {
                    Msg::SavedAll(
                        all.into_iter()
                            .map(|(p, t)| {
                                let r = crate::save::save_turned(&p, t);
                                (p, r)
                            })
                            .collect(),
                    )
                });
            }
            Msg::SavedAll(results) => {
                self.errands = self.errands.saturating_sub(1);
                let failed: Vec<String> = results
                    .iter()
                    .filter_map(|(p, r)| r.as_ref().err().map(|e| format!("{}: {e}", folder::display_name(p))))
                    .collect();
                for (p, r) in &results {
                    if r.is_ok() {
                        self.turns.remove(p);
                    }
                }
                if failed.is_empty() {
                    cx.quit();
                } else {
                    cx.alert(
                        self.window,
                        Alert::new("Some pictures weren't saved", failed.join("\n")).default_button("OK", Msg::Nothing),
                    );
                }
            }
            Msg::DiscardAndQuit => {
                self.turns.clear();
                cx.quit();
            }
        }
    }

    fn window_state(&mut self, window: WindowKey, state: aizome_kit::WindowState, cx: &mut Cx<Msg>) {
        if window != self.window {
            return;
        }
        self.fullscreen = state.fullscreen;
        // Leaving full screen (⌃❖F) ends a slideshow too.
        if !state.fullscreen && self.slideshow.is_some() {
            self.slideshow_full = false;
            self.stop_slideshow(cx);
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if Some(window) == self.info_sheet {
            return crate::ui::info(self);
        }
        crate::ui::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        crate::ui::menus(self)
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if Some(window) == self.info_sheet {
            self.info_sheet = None;
            return true;
        }
        if !self.turns.is_empty() && window == self.window {
            let n = self.turns.len();
            let what = if n == 1 { "a turned picture".to_owned() } else { format!("{n} turned pictures") };
            cx.alert(
                self.window,
                Alert::new(format!("Save {what}?"), "Turns you haven't saved are lost when Shashin closes.")
                    .default_button("Save", Msg::SaveAllAndQuit)
                    .button("Don't Save", Msg::DiscardAndQuit)
                    .cancel_button("Cancel"),
            );
            return false;
        }
        true
    }
}

impl Shashin {
    fn pan(&mut self, by: (f64, f64)) {
        let dims = self.shown_dims();
        let view = self.view_px();
        let z = self.z();
        if let Zoom::Fit = self.zoom {
            // Panning a fitted picture does nothing; fix the zoom so a pan
            // after a zoom keeps working.
            return;
        }
        let c = view::clamp_center(dims, view, z, self.center);
        self.center = view::pan(dims, view, z, c, by);
    }
}
