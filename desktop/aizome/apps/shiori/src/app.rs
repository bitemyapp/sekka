//! Shiori, the program: a window for each document, where it's scrolled
//! and how large, the drawings of its pages, its text, its links, what's
//! selected and found, and its worker.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use aizome_kit::MsgSender;
use aizome_kit::prelude::*;

use crate::engine::{Engine, Event, Failure};
use crate::layout::{self, Anchor, Layout, Zoom};
use crate::places::{Place, Places};
use crate::protocol::{DocInfo, Job, Link, OpenError, PageRect, PageText, Reply, Request, Target, TileKey};
use crate::text::{self, Caret, Selection};

pub const APP_ID: &str = "org.aizome.Shiori";

/// The width of page previews (the sidebar's, and what stands in while
/// a page's tiles come), in points.
pub const PREVIEW: f64 = 112.0;

/// What drawings of pages may take, per document: the pages in view and
/// either side, at 2× and fitted to a wide window, are about 160 MB.
const TILE_BYTES: usize = 320 << 20;

/// The largest page drawn and kept whole ahead of being scrolled to.
const WHOLE_PAGE_BYTES: usize = 64 << 20;

/// The most drawings asked for at once.
const MOST_WANTED: usize = 64;

/// How often an open file is looked at for changes.
const WATCH: Duration = Duration::from_millis(1500);

/// How many pages' text (and links) a document keeps: always those in view
/// and either side and those a selection ends on, then the nearest.
pub const KEPT_TEXTS: usize = 48;

/// How far a press on a link may move and still follow it, in points:
/// further, and it's choosing the link's text.
const LINK_SLOP: f64 = 4.0;

/// How often a selection dragged past the view's edge scrolls on.
const AUTOSCROLL: Duration = Duration::from_millis(50);

/// The pointer over the pages: on a link (the `index`th of page `page`'s),
/// on text, or neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Hover {
    #[default]
    None,
    Text,
    Link {
        page: u32,
        index: usize,
    },
}

/// The sidebar's pages that showed lately, and the rows either side of
/// them: whose previews to draw ahead and keep. A list builds only the
/// rows that show; it hears of each as it's built, and asks the program
/// (once, until it comes) for a preview it doesn't have, its neighbours'
/// too, so scrolling the sidebar a little finds them drawn.
#[derive(Debug, Default)]
pub struct ThumbRows {
    /// Newest last, at most `THUMBS_KEPT`.
    pub recent: VecDeque<u32>,
    /// Asked for and not yet come.
    pub asked: HashSet<u32>,
}

/// How many of the sidebar's pages keep their previews.
pub const THUMBS_KEPT: usize = 48;

/// How many rows either side of one that shows are drawn ahead.
pub const THUMBS_AHEAD: usize = 3;

impl ThumbRows {
    pub fn saw(&mut self, page: u32) {
        if let Some(i) = self.recent.iter().position(|p| *p == page) {
            self.recent.remove(i);
        }
        self.recent.push_back(page);
        while self.recent.len() > THUMBS_KEPT {
            self.recent.pop_front();
        }
    }
}

/// What dragging on the pages does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// Choosing text from where it began.
    Select,
    /// The scroll bar's thumb, taken this far down it (points).
    Bar(f64),
    /// The horizontal one's, taken this far along.
    HBar(f64),
    /// Moving the pages with the middle button, from where it was pressed
    /// and how they were scrolled then.
    Pan { from: Point, scroll: (f64, f64) },
}

/// A copy waiting for the text of the pages it covers that hadn't come:
/// it's put on the clipboard once they have.
#[derive(Clone, Debug)]
pub struct Copying {
    pub selection: Selection,
    /// Each page's part, in page order (`None`: the selection covers none
    /// of its text).
    pub parts: BTreeMap<u32, Option<String>>,
    pub missing: HashSet<u32>,
}

impl Copying {
    pub fn text(&self) -> String {
        let parts: Vec<&str> = self.parts.values().flatten().map(String::as_str).collect();
        parts.join("\n")
    }
}

/// Where a document is in being opened.
#[derive(Clone, Debug)]
pub enum Load {
    Opening,
    /// It needs a password (`tried`: the one given was wrong).
    Locked {
        tried: bool,
    },
    Failed(String),
    Open(Rc<DocInfo>),
}

/// The search of a document.
#[derive(Clone, Debug, Default)]
pub struct Find {
    pub query: String,
    pub case: bool,
    /// The search running (the worker's answers carry it).
    pub serial: u64,
    /// Each page searched, and its hits (rectangles, a line each).
    pub results: BTreeMap<u32, Vec<Vec<PageRect>>>,
    /// How many pages it looks through.
    pub pages: usize,
    /// The hit showing: its page and its index there.
    pub current: Option<(u32, usize)>,
    /// Go to the first hit when it comes.
    pub jump: bool,
    /// Changes to the query (each waits a moment before searching).
    pub typed: u64,
    /// The worker gave up before the search was done.
    pub stopped: bool,
}

impl Find {
    pub fn count(&self) -> usize {
        self.results.values().map(Vec::len).sum()
    }

    pub fn done(&self) -> bool {
        self.stopped || self.results.len() >= self.pages
    }

    /// The hit's place among all, from 1.
    pub fn ordinal(&self) -> Option<usize> {
        let (page, i) = self.current?;
        Some(self.results.range(..page).map(|(_, h)| h.len()).sum::<usize>() + i + 1)
    }

    /// The hit after (or before) the current one, in the document's order.
    pub fn step(&self, forward: bool) -> Option<(u32, usize)> {
        let all: Vec<(u32, usize)> =
            self.results.iter().flat_map(|(p, h)| (0..h.len()).map(move |i| (*p, i))).collect();
        if all.is_empty() {
            return None;
        }
        let Some(cur) = self.current else { return Some(all[0]) };
        let at = all.iter().position(|h| *h == cur);
        Some(match (at, forward) {
            (Some(i), true) => all[(i + 1) % all.len()],
            (Some(i), false) => all[(i + all.len() - 1) % all.len()],
            (None, true) => all.iter().copied().find(|h| *h > cur).unwrap_or(all[0]),
            (None, false) => all.iter().rev().copied().find(|h| *h < cur).unwrap_or(all[all.len() - 1]),
        })
    }
}

/// The drawings of a document's pages, the least recently used going
/// first when they take too much.
#[derive(Default)]
pub struct Tiles {
    map: HashMap<TileKey, (Image, u64)>,
    bytes: usize,
    clock: u64,
}

impl Tiles {
    pub fn get(&self, k: &TileKey) -> Option<&Image> {
        self.map.get(k).map(|(i, _)| i)
    }

    pub fn has(&self, k: &TileKey) -> bool {
        self.map.contains_key(k)
    }

    pub fn insert(&mut self, k: TileKey, img: Image, keep: &dyn Fn(&TileKey) -> bool) {
        self.clock += 1;
        self.bytes += k.bytes();
        if let Some((_, _)) = self.map.insert(k, (img, self.clock)) {
            self.bytes -= k.bytes();
        }
        if self.bytes > TILE_BYTES {
            let mut old: Vec<(u64, TileKey)> =
                self.map.iter().filter(|(k, _)| !keep(k)).map(|(k, (_, t))| (*t, *k)).collect();
            old.sort_by_key(|(t, _)| *t);
            for (_, k) in old {
                if self.bytes <= TILE_BYTES * 3 / 4 {
                    break;
                }
                self.map.remove(&k);
                self.bytes -= k.bytes();
            }
        }
    }

    /// Note that `k` was used.
    pub fn touch(&mut self, k: &TileKey) {
        self.clock += 1;
        if let Some(e) = self.map.get_mut(k) {
            e.1 = self.clock;
        }
    }

    /// The drawings of page `p`.
    pub fn of_page(&self, p: u32) -> impl Iterator<Item = (&TileKey, &Image)> {
        self.map.iter().filter(move |(k, _)| k.page == p).map(|(k, (i, _))| (k, i))
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// A document and its window.
pub struct Doc {
    pub window: WindowKey,
    /// Different for every worker started, for its messages.
    pub serial: u64,
    pub path: PathBuf,
    pub password: Option<String>,
    pub engine: Option<Engine>,
    pub load: Load,
    /// What the file was when it was opened (to see it change).
    pub stamp: Option<(SystemTime, u64)>,
    /// A newer file is being opened; what shows stays until it has.
    pub reloading: bool,
    pub zoom: Zoom,
    pub layout: Rc<Layout>,
    /// The view's top left in the document, in points.
    pub scroll: (f64, f64),
    pub view: Size,
    pub scale: f64,
    /// A place to go once it's open (a remembered one).
    pub pending: Option<Place>,
    pub tiles: Tiles,
    /// What stands in for the tiles in view not drawn at this size yet
    /// (`standin`): the part of each that's made, and its pixels.
    pub standins: HashMap<TileKey, (TileKey, Image)>,
    /// What was last asked to be drawn and hasn't been.
    pub sent: Vec<TileKey>,
    /// Drawings that couldn't be made: each page's, by the size it was
    /// drawn at (see `broken_at`).
    pub broken: HashMap<u32, HashSet<(u32, u32)>>,
    pub texts: HashMap<u32, Rc<PageText>>,
    pub links: HashMap<u32, Rc<Vec<Link>>>,
    pub asked: HashSet<Job>,
    pub selection: Option<Selection>,
    pub drag: Option<Drag>,
    /// Where a drag is (view points), for scrolling on while it's held
    /// past the view's edge; and whether that's due.
    pub drag_at: Option<Point>,
    pub autoscrolling: bool,
    /// A link pressed and where: followed when the press ends, unless
    /// it's become a drag over its text.
    pub link_press: Option<(Target, Point)>,
    pub copying: Option<Copying>,
    pub hover: Hover,
    pub find: Option<Find>,
    pub sidebar: bool,
    /// Pages (0) or Contents (1).
    pub sidebar_tab: usize,
    /// The sidebar's pages that showed lately.
    pub thumbs: Rc<RefCell<ThumbRows>>,
    pub expanded: HashSet<u64>,
    /// Zooming by pinches or ❖-scrolls: tiles wait until it rests.
    pub zooming: u64,
    pub settling: bool,
    pub fullscreen: bool,
    /// The scroll bar shows (while scrolling, and a moment after).
    pub bar_until: u64,
    /// What the pages last reported seeing (to say once).
    pub reported: Rc<Cell<u64>>,
    /// The place last kept, to keep it again only when it moves.
    pub kept: Option<Place>,
    /// What's gone wrong, in the toolbar's place for the page number.
    pub trouble: Option<String>,
    /// The tick a passing notice goes at (lasting ones have none).
    pub trouble_until: Option<u64>,
}

impl Doc {
    pub fn info(&self) -> Option<&Rc<DocInfo>> {
        match &self.load {
            Load::Open(i) => Some(i),
            _ => None,
        }
    }

    pub fn pages(&self) -> usize {
        self.layout.len()
    }

    /// The page the view is reading: the one a third of the way down.
    pub fn current_page(&self) -> usize {
        self.layout.page_at(self.scroll.1 + self.view.height / 3.0).min(self.pages().saturating_sub(1))
    }

    pub fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Document".into())
    }

    pub fn title(&self) -> String {
        match &self.load {
            Load::Locked { .. } => format!("{} (Locked)", self.name()),
            _ => self.name(),
        }
    }

    /// The file changed and the newer one can't be read (yet: it may
    /// still be being written). What showed stays; its stamp is kept, so
    /// the next change to it is tried.
    fn unreadable_now(&mut self) {
        self.reloading = false;
        self.trouble = Some("The file changed and can't be read yet; this is how it was.".into());
        self.trouble_until = None;
    }

    fn place(&self) -> Option<Place> {
        if self.layout.is_empty() {
            return None;
        }
        let a = Anchor::of(&self.layout, self.scroll, (0.0, 0.0));
        Some(Place {
            page: a.page,
            y: a.at.1.max(0.0),
            zoom: self.zoom.into(),
            sidebar: Some((self.sidebar, self.sidebar_tab)),
        })
    }

    /// Whether page `p` drawn `size` large can't be drawn: it failed at
    /// that size, or at two sizes already. A page that's too much at one
    /// zoom may draw at another, but one that fails whatever its size costs
    /// two failures, not one at every zoom.
    pub fn broken_at(&self, p: u32, size: (u32, u32)) -> bool {
        self.broken.get(&p).is_some_and(|s| s.len() >= 2 || s.contains(&size))
    }

    fn draw_failed(&mut self, k: &TileKey) {
        self.broken.entry(k.page).or_default().insert(k.size);
    }

    /// Ask for the text of the pages a copy waits on (again, after the
    /// worker's started again).
    fn ask_copy_text(&mut self) {
        let (Some(c), Some(e)) = (&self.copying, &self.engine) else { return };
        let mut missing: Vec<u32> = c.missing.iter().copied().collect();
        missing.sort_unstable();
        for p in missing {
            if self.asked.insert(Job::Text(p)) {
                e.send(Request::Text(p));
            }
        }
    }

    /// Let go of the text and links of pages far from the view.
    fn prune_texts(&mut self) {
        let near = self.neighbours();
        let ends: Vec<u32> = self.selection.map(|s| vec![s.anchor.page, s.focus.page]).unwrap_or_default();
        let keep = |p: u32| near.contains(&(p as usize)) || ends.contains(&p);
        let here = self.current_page() as u32;
        prune(&mut self.texts, here, &keep);
        prune(&mut self.links, here, &keep);
    }

    /// The contents' entry for where the view's top is: the one that
    /// starts last at or before it, or, if that's in a closed branch, the
    /// branch that shows.
    pub fn current_entry(&self) -> Option<u64> {
        let info = self.info()?;
        if self.layout.is_empty() {
            return None;
        }
        let top = self.scroll.1 + 2.0 * layout::MARGIN;
        let page = self.layout.page_at(top);
        let y = (top - self.layout.page_rect(page).y0) / self.layout.zoom;
        let here = (page as u32, y as f32);
        let mut best: Option<((u32, f32), u64)> = None;
        let mut next = 1;
        each_entry(&info.outline, &mut next, None, &self.expanded, &mut |shown, o| {
            if let Some(Target::Page { page, y }) = &o.target {
                let at = (*page, y.unwrap_or(0.0));
                let before = at.0 < here.0 || (at.0 == here.0 && at.1 <= here.1);
                if before && best.is_none_or(|(b, _)| at.0 > b.0 || (at.0 == b.0 && at.1 >= b.1)) {
                    best = Some((at, shown));
                }
            }
        });
        best.map(|(_, id)| id)
    }

    /// Lay the pages out again (a new zoom, view or document), keeping
    /// what's under `at` (view points) where it is.
    fn relayout(&mut self, at: (f64, f64)) {
        let Some(info) = self.info().cloned() else { return };
        let anchor = (!self.layout.is_empty()).then(|| Anchor::of(&self.layout, self.scroll, at));
        let z = self.zoom.resolve(&info.pages, self.view);
        self.layout = Rc::new(Layout::new(&info.pages, z, self.view.width));
        self.scroll = match anchor {
            Some(a) => a.scroll(&self.layout, self.view),
            None => self.layout.clamp_scroll(self.scroll, self.view),
        };
    }

    fn scroll_to(&mut self, s: (f64, f64)) {
        self.scroll = self.layout.clamp_scroll(s, self.view);
    }

    /// Show page `page`, `y` page points down it at the view's top (or
    /// the page's top a margin down).
    pub fn go(&mut self, page: usize, y: Option<f64>) {
        if self.layout.is_empty() {
            return;
        }
        let page = page.min(self.pages() - 1);
        let r = self.layout.page_rect(page);
        let top = match y {
            Some(y) => r.y0 + y * self.layout.zoom - layout::MARGIN,
            None => r.y0 - layout::MARGIN,
        };
        self.scroll_to((self.scroll.0, top));
    }

    /// Bring `r` (document points) into view: centred if it was far off.
    fn reveal(&mut self, r: Rect) {
        let (sx, sy) = self.scroll;
        let (w, h) = (self.view.width, self.view.height);
        let mut s = (sx, sy);
        if r.y0 < sy || r.y1 > sy + h {
            s.1 = r.center().y - h / 2.0;
        }
        if r.x0 < sx || r.x1 > sx + w {
            s.0 = r.center().x - w / 2.0;
        }
        self.scroll_to(s);
    }

    /// The pages in view, and the page above and the page below: those
    /// drawn whole and kept, so scrolling a little finds them there.
    pub fn neighbours(&self) -> std::ops::Range<usize> {
        let v = self.layout.visible(self.scroll.1, self.scroll.1 + self.view.height);
        v.start.saturating_sub(1)..(v.end + 1).min(self.pages())
    }

    /// Whether page `i` is small enough, drawn at this zoom, to draw and
    /// keep whole ahead of being scrolled to.
    pub fn whole_page_fits(&self, i: usize) -> bool {
        let (w, h) = self.layout.device_size(i, self.scale);
        u64::from(w) * u64::from(h) * 4 <= WHOLE_PAGE_BYTES as u64
    }

    /// The pages in view, and two either side (their previews are kept).
    fn near(&self) -> std::ops::Range<usize> {
        let v = self.layout.visible(self.scroll.1, self.scroll.1 + self.view.height);
        v.start.saturating_sub(2)..(v.end + 2).min(self.pages())
    }

    fn preview_width(&self) -> u32 {
        (PREVIEW * self.scale).round() as u32
    }

    /// Make stand-ins for the parts in view of tiles not drawn at this size
    /// (`standin`), from their pages' other drawings, or larger ones where
    /// more of a tile has come into view; let go of those drawn now or out
    /// of view.
    fn stand_in(&mut self) {
        let s = self.scale;
        let view =
            Rect::new(self.scroll.0, self.scroll.1, self.scroll.0 + self.view.width, self.scroll.1 + self.view.height);
        // What's in view of each tile, in its page's pixels, rounded out
        // to `GRAIN`, so a little scrolling needs no more.
        let mut parts: HashMap<TileKey, TileKey> = HashMap::new();
        for i in self.layout.visible(view.y0, view.y1) {
            let r = self.layout.page_rect(i);
            let v = r.intersect(view);
            let lo = |p: f64, at: u32| ((p.max(0.0) as u32) / GRAIN * GRAIN).max(at);
            let hi = |p: f64, end: u32| ((p.ceil().max(0.0) as u32).div_ceil(GRAIN) * GRAIN).min(end);
            for k in self.layout.tiles(i, s, view, view.center()) {
                if self.tiles.has(&k) {
                    continue;
                }
                let (x0, x1) = (lo((v.x0 - r.x0) * s, k.at.0), hi((v.x1 - r.x0) * s, k.at.0 + k.w));
                let (y0, y1) = (lo((v.y0 - r.y0) * s, k.at.1), hi((v.y1 - r.y0) * s, k.at.1 + k.h));
                if x0 < x1 && y0 < y1 {
                    parts.insert(k, TileKey { at: (x0, y0), w: x1 - x0, h: y1 - y0, ..k });
                }
            }
        }
        self.standins.retain(|k, _| parts.contains_key(k));
        let mut wanted = Vec::new();
        let mut tile_of = HashMap::new();
        for (k, part) in parts {
            let part = match self.standins.get(&k) {
                Some((had, _)) if within(&part, had) => continue,
                Some((had, _)) => bounds(&part, had),
                None => part,
            };
            tile_of.insert(part, k);
            wanted.push((part, self.tiles.of_page(k.page).map(|(s, img)| (*s, img.clone())).collect()));
        }
        if wanted.is_empty() {
            return;
        }
        for (part, img) in crate::standin::make(wanted) {
            if let Some(k) = tile_of.get(&part) {
                self.standins.insert(*k, (part, img));
            }
        }
    }
}

/// How finely the part of a tile a stand-in is made of goes, in pixels.
const GRAIN: u32 = 64;

/// Whether part `a` of a page is within part `b` (of the same size).
fn within(a: &TileKey, b: &TileKey) -> bool {
    a.at.0 >= b.at.0 && a.at.1 >= b.at.1 && a.at.0 + a.w <= b.at.0 + b.w && a.at.1 + a.h <= b.at.1 + b.h
}

/// The part of a page that has parts `a` and `b`.
fn bounds(a: &TileKey, b: &TileKey) -> TileKey {
    let (x0, y0) = (a.at.0.min(b.at.0), a.at.1.min(b.at.1));
    let (x1, y1) = ((a.at.0 + a.w).max(b.at.0 + b.w), (a.at.1 + a.h).max(b.at.1 + b.h));
    TileKey { at: (x0, y0), w: x1 - x0, h: y1 - y0, ..*a }
}

/// What the pages saw when they drew: the view and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub view: Size,
    pub scale: f64,
}

/// A sheet over a document's window.
#[derive(Clone, Debug)]
pub enum Sheet {
    Password { parent: WindowKey, text: String },
    GoTo { parent: WindowKey, text: String, error: bool },
    Info { parent: WindowKey },
}

impl Sheet {
    pub fn parent(&self) -> WindowKey {
        match self {
            Sheet::Password { parent, .. } | Sheet::GoTo { parent, .. } | Sheet::Info { parent } => *parent,
        }
    }
}

/// Moving about with keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

#[derive(Clone)]
pub enum Msg {
    Nothing,
    Open(Vec<PathBuf>),
    OpenPanel,
    Opened(Option<Vec<PathBuf>>),
    Raise(Vec<PathBuf>),
    /// From a document's worker (by its serial).
    Worker(u64, Event),
    Seen(WindowKey, Seen),
    ScrollBy(WindowKey, Vec2),
    Nav(WindowKey, Nav),
    ZoomBy {
        window: WindowKey,
        factor: f64,
        at: Point,
    },
    ZoomRested(WindowKey, u64),
    ZoomPick(WindowKey, usize),
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitWidth,
    FitPage,
    PreviousPage,
    NextPage,
    FirstPage,
    LastPage,
    GoPage(WindowKey, usize),
    GoTarget(WindowKey, Target),
    Press {
        window: WindowKey,
        at: Point,
        clicks: u32,
        shift: bool,
    },
    DragTo(WindowKey, Point),
    /// A selection held past the view's edge scrolls on.
    DragTick(WindowKey),
    Release(WindowKey),
    Hover(WindowKey, Hover),
    BarPress(WindowKey, Point),
    HBarPress(WindowKey, Point),
    /// The middle button pressed: the pages move with the pointer.
    PanStart(WindowKey, Point),
    /// Escape: Find goes, else the selection, else full screen.
    Escape(WindowKey),
    Copy,
    SelectAll,
    Find,
    FindNext,
    FindPrevious,
    FindQuery(WindowKey, String),
    FindCase(WindowKey, bool),
    FindRun(WindowKey, u64),
    FindClose(WindowKey),
    ToggleSidebar,
    SidebarTab(WindowKey, usize),
    Thumb(WindowKey, u32),
    OutlineToggle(WindowKey, u64, bool),
    Outline(WindowKey, u64),
    SheetText(WindowKey, String),
    SheetOk(WindowKey),
    SheetCancel(WindowKey),
    AskPassword(WindowKey),
    GoToPanel,
    Info,
    FullScreen,
    Reload,
    ShowInHako,
    Tick,
}

pub struct Shiori {
    /// The program the workers run (this one, as a rule).
    pub worker: PathBuf,
    pub docs: Vec<Doc>,
    pub sheets: HashMap<WindowKey, Sheet>,
    /// The window with nothing open (at start, and after the last
    /// document's closed by opening another... never both).
    pub empty: Option<WindowKey>,
    pub places: Places,
    pub stream: Option<MsgSender<Msg>>,
    pub instance: Option<std::os::unix::net::UnixListener>,
    pub initial: Vec<PathBuf>,
    /// The window with the keyboard, as of the last message.
    pub focused: Option<WindowKey>,
    pub scale: f64,
    serial: u64,
    /// Ticks so far (the scroll bar's clock).
    pub ticks: u64,
    /// Start nothing outside (Hako, a browser): tests. Links that would
    /// have opened are kept in `opened`.
    pub dry_run: bool,
    pub opened: Vec<String>,
}

impl Default for Shiori {
    fn default() -> Shiori {
        Shiori::new()
    }
}

/// A file's modification time and size.
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// Keep at most `KEPT_TEXTS` of `map`'s pages: those `keep` says, then the
/// nearest `here`.
pub fn prune<T>(map: &mut HashMap<u32, T>, here: u32, keep: &dyn Fn(u32) -> bool) {
    if map.len() <= KEPT_TEXTS {
        return;
    }
    let mut far: Vec<u32> = map.keys().copied().filter(|p| !keep(*p)).collect();
    far.sort_by_key(|p| std::cmp::Reverse(p.abs_diff(here)));
    let extra = map.len() - KEPT_TEXTS;
    for p in far.into_iter().take(extra) {
        map.remove(&p);
    }
}

/// Each of the outline's entries, depth first and numbered as
/// `outline_items` numbers them, with the id of what shows it in the tree:
/// its own, or that of the closed branch it's in.
fn each_entry(
    outline: &[crate::protocol::Outline],
    next: &mut u64,
    closed: Option<u64>,
    expanded: &HashSet<u64>,
    visit: &mut dyn FnMut(u64, &crate::protocol::Outline),
) {
    for o in outline {
        let id = *next;
        *next += 1;
        visit(closed.unwrap_or(id), o);
        let inner = closed.or((!expanded.contains(&id)).then_some(id));
        each_entry(&o.children, next, inner, expanded, visit);
    }
}

/// Whether Shiori opens `uri`: what a browser or mail program should have.
pub fn opens(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    ["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s))
}

/// The outline as the sidebar's tree: ids are places in a depth-first
/// walk, from 1.
pub fn outline_items(
    outline: &[crate::protocol::Outline],
    next: &mut u64,
    targets: &mut HashMap<u64, Target>,
) -> Vec<TreeItem> {
    outline
        .iter()
        .map(|o| {
            let id = *next;
            *next += 1;
            if let Some(t) = &o.target {
                targets.insert(id, t.clone());
            }
            let title = if o.title.is_empty() { "Untitled".to_owned() } else { o.title.clone() };
            if o.children.is_empty() {
                TreeItem::leaf(id, title)
            } else {
                TreeItem::branch(id, title, outline_items(&o.children, next, targets))
            }
        })
        .collect()
}

impl Shiori {
    pub fn new() -> Shiori {
        Shiori {
            worker: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("aizome-shiori")),
            docs: Vec::new(),
            sheets: HashMap::new(),
            empty: None,
            places: Places::load(Places::default_file()),
            stream: None,
            instance: None,
            initial: Vec::new(),
            focused: None,
            scale: 1.0,
            serial: 0,
            ticks: 0,
            dry_run: false,
            opened: Vec::new(),
        }
    }

    pub fn doc(&self, w: WindowKey) -> Option<&Doc> {
        self.docs.iter().find(|d| d.window == w)
    }

    fn doc_mut(&mut self, w: WindowKey) -> Option<&mut Doc> {
        self.docs.iter_mut().find(|d| d.window == w)
    }

    /// The document menu commands are for: the key window's (or its
    /// sheet's parent's), else the last focused.
    pub fn target(&self) -> Option<WindowKey> {
        let k = self.focused.map(|k| self.sheets.get(&k).map_or(k, Sheet::parent));
        k.filter(|w| self.doc(*w).is_some()).or_else(|| self.docs.last().map(|d| d.window))
    }

    pub fn focused_doc(&self) -> Option<&Doc> {
        self.target().and_then(|w| self.doc(w))
    }

    /// Nothing is being opened or drawn (tests settle on it).
    pub fn idle(&self) -> bool {
        self.docs.iter().all(|d| {
            !matches!(d.load, Load::Opening)
                && !d.reloading
                && d.sent.is_empty()
                && !d.settling
                && d.find.as_ref().is_none_or(|f| f.query.is_empty() || f.done())
                && d.asked.is_empty()
                && d.copying.is_none()
        })
    }

    fn next_serial(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    /// Open each of `paths` in a window of its own (or bring forward the
    /// one it's open in).
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        for path in paths {
            let path = std::path::absolute(&path).unwrap_or(path);
            if let Some(d) = self.docs.iter().find(|d| d.path == path) {
                cx.activate(d.window);
                continue;
            }
            let window = match self.empty.take() {
                Some(w) => {
                    cx.set_title(w, path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    w
                }
                None => self.open_window(cx),
            };
            let serial = self.next_serial();
            let view = self.docs.first().map_or(Size::new(900.0, 700.0), |d| d.view);
            let pending = self.places.get(&path);
            let mut doc = Doc {
                window,
                serial,
                path: path.clone(),
                password: None,
                engine: None,
                load: Load::Opening,
                stamp: stamp(&path),
                reloading: false,
                zoom: pending.map_or(Zoom::FitWidth, |p| p.zoom.into()),
                layout: Rc::new(Layout::default()),
                scroll: (0.0, 0.0),
                view,
                scale: self.scale,
                pending,
                tiles: Tiles::default(),
                standins: HashMap::new(),
                sent: Vec::new(),
                broken: HashMap::new(),
                texts: HashMap::new(),
                links: HashMap::new(),
                asked: HashSet::new(),
                selection: None,
                drag: None,
                drag_at: None,
                autoscrolling: false,
                link_press: None,
                copying: None,
                hover: Hover::None,
                find: None,
                sidebar: false,
                sidebar_tab: 0,
                thumbs: Rc::new(RefCell::new(ThumbRows::default())),
                expanded: HashSet::new(),
                zooming: 0,
                settling: false,
                fullscreen: false,
                bar_until: 0,
                reported: Rc::new(Cell::new(0)),
                kept: pending,
                trouble: None,
                trouble_until: None,
            };
            doc.engine = Some(self.start_engine(serial, &path, None));
            cx.set_title(window, doc.title());
            self.docs.push(doc);
        }
    }

    fn open_window(&mut self, cx: &mut Cx<Msg>) -> WindowKey {
        cx.open_window(WindowSpec::new("Shiori").size(900.0, 760.0).min_size(420.0, 320.0))
    }

    fn start_engine(&self, serial: u64, path: &Path, password: Option<String>) -> Engine {
        let tx = self.stream.clone();
        Engine::start(self.worker.clone(), path.to_path_buf(), password, move |ev| {
            if let Some(tx) = &tx {
                tx.send(Msg::Worker(serial, ev));
            }
        })
    }

    /// Start a new worker for `w`'s document (a password, a newer file).
    fn restart(&mut self, w: WindowKey) {
        let serial = self.next_serial();
        let Some(d) = self.doc(w) else { return };
        let engine = self.start_engine(serial, &d.path.clone(), d.password.clone());
        let Some(d) = self.doc_mut(w) else { return };
        d.serial = serial;
        d.engine = Some(engine);
        d.sent.clear();
        d.asked.clear();
    }

    fn keep_place(&mut self, w: WindowKey) {
        let Some(d) = self.doc(w) else { return };
        let (Some(place), path) = (d.place(), d.path.clone()) else { return };
        if d.kept != Some(place) {
            self.places.set(&path, place);
            self.places.save();
            if let Some(d) = self.doc_mut(w) {
                d.kept = Some(place);
            }
        }
    }

    /// Ask the worker for what the view needs and hasn't, most needed
    /// first: the tiles of what shows, nearest the middle first; the
    /// previews of the pages that show; then the rest of those pages and
    /// the whole of the page above and the page below, so scrolling a
    /// little either way finds them drawn; the previews of the pages
    /// around, and the sidebar's. And the text and links of those pages.
    /// It asks again only when what's wanted changes, not when the same
    /// drawings would come in another order.
    pub fn request(&mut self, w: WindowKey) {
        let Some(d) = self.doc_mut(w) else { return };
        if d.info().is_none() || d.layout.is_empty() {
            return;
        }
        d.stand_in();
        if d.engine.is_none() {
            return;
        }
        let s = d.scale;
        let view = Rect::new(d.scroll.0, d.scroll.1, d.scroll.0 + d.view.width, d.scroll.1 + d.view.height);
        let centre = view.center();
        let shown = d.layout.visible(view.y0, view.y1);
        let mut want: Vec<TileKey> = Vec::new();
        let missing = |d: &Doc, k: &TileKey| !d.tiles.has(k) && !d.broken_at(k.page, k.size);
        let add = |d: &Doc, want: &mut Vec<TileKey>, k: TileKey| {
            if missing(d, &k) && !want.contains(&k) {
                want.push(k);
            }
        };
        if !d.settling {
            for i in shown.clone() {
                for k in d.layout.tiles(i, s, view, centre) {
                    add(d, &mut want, k);
                }
            }
        }
        let pw = d.preview_width();
        for i in shown.clone() {
            add(d, &mut want, d.layout.preview(i, pw));
        }
        if !d.settling {
            let mut ahead: Vec<TileKey> = Vec::new();
            for i in d.neighbours() {
                let area = if d.whole_page_fits(i) {
                    d.layout.page_rect(i)
                } else {
                    // A page too large to keep whole (zoomed far in):
                    // a view's height of it either way.
                    view.inflate(0.0, view.height())
                };
                ahead.extend(d.layout.tiles(i, s, area, centre));
            }
            let dist = |k: &TileKey| {
                let r = d.layout.page_rect(k.page as usize);
                let f = r.width() / f64::from(k.size.0.max(1));
                let y = r.y0 + (f64::from(k.at.1) + f64::from(k.h) / 2.0) * f;
                (y - centre.y).abs()
            };
            ahead.sort_by(|a, b| dist(a).total_cmp(&dist(b)));
            for k in ahead {
                add(d, &mut want, k);
            }
        }
        for i in d.near() {
            add(d, &mut want, d.layout.preview(i, pw));
        }
        if d.sidebar && d.sidebar_tab == 0 {
            let thumbs: Vec<u32> = d.thumbs.borrow().recent.iter().rev().copied().collect();
            for p in thumbs {
                if (p as usize) < d.pages() {
                    add(d, &mut want, d.layout.preview(p as usize, pw));
                }
            }
        }
        want.truncate(MOST_WANTED);
        let mut asks = Vec::new();
        for i in d.neighbours() {
            let p = i as u32;
            if !d.texts.contains_key(&p) && !d.asked.contains(&Job::Text(p)) {
                asks.push(Job::Text(p));
            }
            if !d.links.contains_key(&p) && !d.asked.contains(&Job::Links(p)) {
                asks.push(Job::Links(p));
            }
        }
        let Some(engine) = d.engine.as_ref() else { return };
        let same = want.len() == d.sent.len() && want.iter().all(|k| d.sent.contains(k));
        if !same {
            engine.send(Request::Draw(want.clone()));
            d.sent = want;
        }
        for a in asks {
            match a {
                Job::Text(p) => engine.send(Request::Text(p)),
                Job::Links(p) => engine.send(Request::Links(p)),
                _ => {}
            }
            d.asked.insert(a);
        }
    }

    fn worker(&mut self, serial: u64, ev: Event, cx: &mut Cx<Msg>) {
        let Some(w) = self.docs.iter().find(|d| d.serial == serial).map(|d| d.window) else { return };
        match ev {
            Event::Drawn(key, img) => {
                let Some(d) = self.doc_mut(w) else { return };
                d.sent.retain(|k| *k != key);
                if d.reloading {
                    return;
                }
                // Kept whatever else goes: the drawings of the pages in view
                // and either side at this zoom, and the previews around.
                let (whole, near) = (d.neighbours(), d.near());
                let pw = d.preview_width();
                if key == d.layout.preview(key.page as usize, pw) {
                    d.thumbs.borrow_mut().asked.remove(&key.page);
                }
                let sidebar: HashSet<u32> =
                    if d.sidebar { d.thumbs.borrow().recent.iter().copied().collect() } else { HashSet::new() };
                let keep = |k: &TileKey| {
                    let p = k.page as usize;
                    p < d.layout.len()
                        && ((whole.contains(&p) && k.size == d.layout.device_size(p, d.scale))
                            || ((near.contains(&p) || sidebar.contains(&k.page)) && *k == d.layout.preview(p, pw)))
                };
                let keep: Vec<TileKey> = d.tiles.map.keys().filter(|k| keep(k)).copied().collect();
                d.tiles.insert(key, img, &|k| keep.contains(k));
                self.request(w);
            }
            Event::Reply(Reply::DrawFailed(key)) => {
                if let Some(d) = self.doc_mut(w) {
                    d.sent.retain(|k| *k != key);
                    d.draw_failed(&key);
                }
                self.request(w);
            }
            Event::Reply(Reply::Opened(info)) => self.opened(w, *info, cx),
            Event::Reply(Reply::OpenFailed(e)) => {
                let Some(d) = self.doc_mut(w) else { return };
                if d.reloading {
                    d.unreadable_now();
                    return;
                }
                d.engine = None;
                match e {
                    OpenError::Password { tried } => {
                        d.load = Load::Locked { tried };
                        cx.set_title(w, d.title());
                        self.ask_password(w, cx);
                    }
                    e => d.load = Load::Failed(e.message()),
                }
            }
            Event::Reply(Reply::Text { page, text }) => {
                if let Some(d) = self.doc_mut(w) {
                    d.asked.remove(&Job::Text(page));
                    d.texts.insert(page, Rc::new(text));
                }
                self.text_came(w, page, cx);
            }
            Event::Reply(Reply::Links { page, links }) => {
                if let Some(d) = self.doc_mut(w) {
                    d.asked.remove(&Job::Links(page));
                    d.links.insert(page, Rc::new(links));
                    d.prune_texts();
                }
            }
            Event::Reply(Reply::Found { serial, page, hits }) => {
                let Some(d) = self.doc_mut(w) else { return };
                let Some(f) = d.find.as_mut().filter(|f| f.serial == serial) else { return };
                let first = !hits.is_empty() && f.jump;
                f.results.insert(page, hits);
                if first {
                    f.jump = false;
                    f.current = Some((page, 0));
                    self.reveal_hit(w);
                }
            }
            Event::Reply(Reply::Working(_) | Reply::Drawn(_)) => {}
            Event::Failed { job, why, gave_up } => {
                let ticks = self.ticks;
                let Some(d) = self.doc_mut(w) else { return };
                let what = match why {
                    Failure::TooLong => "took too long",
                    Failure::Crashed => "stopped",
                };
                d.sent.clear();
                let mut no_text = None;
                match job {
                    Some(Job::Draw(k)) => {
                        d.draw_failed(&k);
                    }
                    Some(Job::Text(p)) => {
                        d.texts.insert(p, Rc::new(PageText::default()));
                        no_text = Some(p);
                    }
                    Some(Job::Links(p)) => {
                        d.links.insert(p, Rc::new(Vec::new()));
                    }
                    Some(Job::Find(p)) => {
                        if let Some(f) = d.find.as_mut() {
                            f.results.insert(p, Vec::new());
                        }
                    }
                    Some(Job::Open) | None => {}
                }
                d.asked.retain(|j| Some(j) != job.as_ref());
                d.trouble = Some(format!("The page reader {what}; Shiori started it again."));
                // About ten seconds, then the page number again.
                d.trouble_until = Some(ticks + 7);
                if gave_up {
                    d.engine = None;
                    d.asked.clear();
                    d.trouble = None;
                    // What was waiting on the worker won't come.
                    d.copying = None;
                    if let Some(f) = d.find.as_mut().filter(|f| !f.done()) {
                        f.stopped = true;
                    }
                    if d.reloading && job == Some(Job::Open) {
                        // The changed file broke it: what showed stays.
                        d.unreadable_now();
                    } else if matches!(d.load, Load::Opening) || job == Some(Job::Open) {
                        d.load = Load::Failed(format!("The page reader {what} while opening it."));
                    } else {
                        d.trouble = Some(format!(
                            "The page reader {what} too often; the rest of this document can't be drawn."
                        ));
                        d.trouble_until = None;
                    }
                }
                if let Some(p) = no_text {
                    self.text_came(w, p, cx);
                }
            }
        }
    }

    fn opened(&mut self, w: WindowKey, info: DocInfo, cx: &mut Cx<Msg>) {
        let Some(d) = self.doc_mut(w) else { return };
        let again = matches!(d.load, Load::Open(_));
        let reloading = d.reloading;
        if again && !reloading {
            // A worker started again after a failure: carry on, and ask
            // for what's still wanted (the search too).
            d.sent.clear();
            d.asked.clear();
            let find = d.find.as_ref().map(|f| (f.serial, f.query.clone(), f.case, f.results.clone()));
            if let (Some((serial, query, case, results)), Some(e)) = (find, d.engine.as_ref())
                && !query.is_empty()
            {
                let pages = (0..d.layout.len() as u32).filter(|p| !results.contains_key(p)).collect();
                e.send(Request::Find { serial, query, case, pages });
            }
            d.ask_copy_text();
            self.request(w);
            return;
        }
        if info.pages.is_empty() {
            // hayro read it and found no pages it can show (a damaged
            // page tree, a filter it doesn't have; a file still being
            // written, when it changed while open).
            if reloading {
                d.unreadable_now();
            } else {
                d.load = Load::Failed("It has no pages Shiori can read.".into());
                d.engine = None;
            }
            return;
        }
        if reloading {
            d.reloading = false;
            d.tiles.clear();
            d.standins.clear();
            d.broken.clear();
            d.texts.clear();
            d.links.clear();
            d.asked.clear();
            d.selection = None;
            d.copying = None;
            d.trouble = None;
        }
        let first = !again;
        d.load = Load::Open(Rc::new(info));
        d.relayout((0.0, 0.0));
        if first {
            let pending = d.pending.take();
            match pending {
                Some(p) => d.go(p.page, Some(p.y + layout::MARGIN / d.layout.zoom)),
                None => d.scroll = (d.scroll.0, 0.0),
            }
            // Centre a page wider than the view.
            let extra = d.layout.size.width - d.view.width;
            if extra > 0.0 {
                d.scroll.0 = (extra / 2.0).round();
            }
            // The sidebar as it was left; else, the contents if there are
            // some and there's room.
            let outline = d.info().is_some_and(|i| !i.outline.is_empty());
            (d.sidebar, d.sidebar_tab) = match pending.and_then(|p| p.sidebar) {
                Some((shown, tab)) => (shown, if outline { tab.min(1) } else { 0 }),
                None => {
                    let shown = outline && d.view.width >= 700.0;
                    (shown, usize::from(shown))
                }
            };
        }
        cx.set_title(w, d.title());
        let rerun = d.find.as_ref().is_some_and(|f| !f.query.is_empty());
        self.request(w);
        if rerun {
            let typed = self.doc(w).and_then(|d| d.find.as_ref()).map_or(0, |f| f.typed);
            self.run_find(w, typed);
        }
    }

    fn ask_password(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        if self.sheets.values().any(|s| matches!(s, Sheet::Password { parent, .. } if *parent == w)) {
            return;
        }
        let s = cx.show_sheet(w);
        self.sheets.insert(s, Sheet::Password { parent: w, text: String::new() });
        cx.focus("password");
    }

    fn zoom_to(&mut self, w: WindowKey, zoom: Zoom, at: Option<Point>) {
        let Some(d) = self.doc_mut(w) else { return };
        let at = at.map_or((d.view.width / 2.0, d.view.height / 2.0), |p| (p.x, p.y));
        d.zoom = zoom;
        d.relayout(at);
        self.request(w);
    }

    fn zoom_step(&mut self, w: WindowKey, inward: bool) {
        let Some(d) = self.doc(w) else { return };
        let z = d.layout.zoom;
        let to = if inward { layout::step_in(z) } else { layout::step_out(z) };
        self.zoom_to(w, Zoom::Fixed(to), None);
    }

    fn go_page(&mut self, w: WindowKey, page: usize) {
        if let Some(d) = self.doc_mut(w) {
            d.go(page, None);
        }
        self.request(w);
    }

    fn go_target(&mut self, w: WindowKey, t: Target, cx: &mut Cx<Msg>) {
        match t {
            Target::Page { page, y } => {
                if let Some(d) = self.doc_mut(w) {
                    d.go(page as usize, y.map(f64::from));
                }
                self.request(w);
            }
            Target::Uri(u) => {
                if !opens(&u) {
                    return;
                }
                if self.dry_run {
                    self.opened.push(u);
                } else {
                    cx.open_url(&u);
                }
            }
        }
    }

    fn nav(&mut self, w: WindowKey, n: Nav) {
        let ticks = self.ticks;
        let Some(d) = self.doc_mut(w) else { return };
        let page = (d.view.height - aizome_kit::LINE).max(aizome_kit::LINE);
        let (x, y) = d.scroll;
        let fits = d.layout.size.width <= d.view.width + 0.5;
        match n {
            Nav::Up => d.scroll_to((x, y - aizome_kit::LINE)),
            Nav::Down => d.scroll_to((x, y + aizome_kit::LINE)),
            Nav::PageUp => d.scroll_to((x, y - page)),
            Nav::PageDown => d.scroll_to((x, y + page)),
            Nav::Home => d.scroll_to((x, 0.0)),
            Nav::End => d.scroll_to((x, f64::MAX / 4.0)),
            Nav::Left if fits => {
                let p = d.current_page();
                d.go(p.saturating_sub(1), None);
            }
            Nav::Right if fits => {
                let p = d.current_page();
                d.go(p + 1, None);
            }
            Nav::Left => d.scroll_to((x - aizome_kit::LINE, y)),
            Nav::Right => d.scroll_to((x + aizome_kit::LINE, y)),
        }
        d.bar_until = ticks + 1;
        self.request(w);
    }

    /// Where `at` (view points) falls in the text: the page, and the
    /// caret, if that page's text has come.
    fn caret(&self, w: WindowKey, at: Point) -> Option<Caret> {
        let d = self.doc(w)?;
        let p = Point::new(d.scroll.0 + at.x, d.scroll.1 + at.y);
        let page = d.layout.page_at(p.y);
        let text = d.texts.get(&(page as u32))?;
        let on = d.layout.on_page(page, p);
        let index = text::caret_at(text, on)?;
        Some(Caret { page: page as u32, index })
    }

    /// The link under `at` (view points).
    pub fn link_at(&self, w: WindowKey, at: Point) -> Option<Target> {
        let d = self.doc(w)?;
        let p = Point::new(d.scroll.0 + at.x, d.scroll.1 + at.y);
        let (page, on) = d.layout.hit(p)?;
        let links = d.links.get(&(page as u32))?;
        links
            .iter()
            .find(|l| {
                let r = l.rect;
                on.0 >= f64::from(r[0]) && on.0 <= f64::from(r[2]) && on.1 >= f64::from(r[1]) && on.1 <= f64::from(r[3])
            })
            .map(|l| l.target.clone())
    }

    fn press(&mut self, w: WindowKey, at: Point, clicks: u32, shift: bool) {
        // A link is followed when the press ends where it began
        // (`Msg::Release`); dragged from, it's text to choose.
        let link = if clicks == 1 && !shift { self.link_at(w, at) } else { None };
        let caret = self.caret(w, at);
        let Some(d) = self.doc_mut(w) else { return };
        d.link_press = link.map(|t| (t, at));
        d.drag_at = Some(at);
        let p = Point::new(d.scroll.0 + at.x, d.scroll.1 + at.y);
        let Some(c) = caret.filter(|_| d.layout.hit(p).is_some() || d.hover == Hover::Text) else {
            d.selection = None;
            d.drag = Some(Drag::Select);
            return;
        };
        let text = d.texts.get(&c.page).cloned().unwrap_or_default();
        let on = d.layout.on_page(c.page as usize, p);
        let under = text::char_at(&text, on);
        d.selection = Some(match (clicks, under) {
            (2, Some(i)) => {
                let (s, e) = text::word(&text, i);
                Selection { anchor: Caret { page: c.page, index: s }, focus: Caret { page: c.page, index: e } }
            }
            (n, Some(i)) if n >= 3 => {
                let (s, e) = text::line(&text, i);
                Selection { anchor: Caret { page: c.page, index: s }, focus: Caret { page: c.page, index: e } }
            }
            _ => match (shift, d.selection) {
                (true, Some(sel)) => Selection { anchor: sel.anchor, focus: c },
                _ => Selection { anchor: c, focus: c },
            },
        });
        d.drag = Some(Drag::Select);
    }

    fn drag_to(&mut self, w: WindowKey, at: Point, cx: &mut Cx<Msg>) {
        let ticks = self.ticks;
        let Some(d) = self.doc_mut(w) else { return };
        d.drag_at = Some(at);
        let drag = d.drag;
        match drag {
            Some(Drag::Select) => {
                // A link pressed and barely moved from is still a click.
                if let Some((_, from)) = d.link_press {
                    if (at - from).hypot() <= LINK_SLOP {
                        return;
                    }
                    d.link_press = None;
                }
                // Past the view's top or bottom, it scrolls, and goes on
                // scrolling while it's held there (`Msg::DragTick`).
                let over = if at.y < 0.0 { at.y } else { (at.y - d.view.height).max(0.0) };
                if over != 0.0 {
                    let s = (d.scroll.0, d.scroll.1 + over.clamp(-aizome_kit::LINE, aizome_kit::LINE));
                    d.scroll_to(s);
                    if !d.autoscrolling {
                        d.autoscrolling = true;
                        cx.after(AUTOSCROLL, Msg::DragTick(w));
                    }
                }
                let caret = self.caret(w, at);
                let Some(d) = self.doc_mut(w) else { return };
                if let (Some(c), Some(sel)) = (caret, d.selection.as_mut()) {
                    sel.focus = c;
                }
                if over != 0.0 {
                    self.request(w);
                }
            }
            Some(Drag::Bar(grab)) => {
                let track = d.view.height;
                let thumb = (track * d.view.height / d.layout.size.height).clamp(24.0, track);
                let room = (track - thumb).max(1.0);
                let t = ((at.y - grab) / room).clamp(0.0, 1.0);
                let y = t * (d.layout.size.height - d.view.height);
                d.scroll_to((d.scroll.0, y));
                d.bar_until = ticks + 1;
                self.request(w);
            }
            Some(Drag::HBar(grab)) => {
                let track = d.view.width;
                let thumb = (track * d.view.width / d.layout.size.width).clamp(24.0, track);
                let room = (track - thumb).max(1.0);
                let t = ((at.x - grab) / room).clamp(0.0, 1.0);
                let x = t * (d.layout.size.width - d.view.width);
                d.scroll_to((x, d.scroll.1));
                d.bar_until = ticks + 1;
                self.request(w);
            }
            Some(Drag::Pan { from, scroll }) => {
                d.scroll_to((scroll.0 - (at.x - from.x), scroll.1 - (at.y - from.y)));
                d.bar_until = ticks + 1;
                self.request(w);
            }
            None => {}
        }
    }

    pub fn selected_text(&self, w: WindowKey) -> Option<String> {
        let d = self.doc(w)?;
        let sel = d.selection.filter(|s| !s.is_empty())?;
        let s = text::copy(&sel, |p| d.texts.get(&p).map(|t| t.as_ref()));
        (!s.is_empty()).then_some(s)
    }

    /// Copy the selection, if its author allows it. Pages it covers whose
    /// text hasn't come (passed over with the scroll bar, say) are asked
    /// for first, and the copy waits for them.
    fn copy(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let Some(d) = self.doc_mut(w) else { return };
        let Some(sel) = d.selection.filter(|s| !s.is_empty()) else { return };
        if !d.info().is_some_and(|i| i.can_copy) {
            return;
        }
        let (a, b) = sel.ordered();
        let mut c = Copying { selection: sel, parts: BTreeMap::new(), missing: HashSet::new() };
        for p in a.page..=b.page {
            match d.texts.get(&p) {
                Some(t) => {
                    c.parts.insert(p, text::page_part(&sel, p, t));
                }
                None => {
                    c.missing.insert(p);
                }
            }
        }
        if c.missing.is_empty() {
            d.copying = None;
            cx.set_clipboard_text(&c.text());
            return;
        }
        d.copying = Some(c);
        d.ask_copy_text();
    }

    /// Page `page`'s text has come (or won't: it's empty): a copy waiting
    /// on it takes its part, and is made once it has them all.
    fn text_came(&mut self, w: WindowKey, page: u32, cx: &mut Cx<Msg>) {
        let Some(d) = self.doc_mut(w) else { return };
        if let Some(c) = d.copying.as_mut()
            && c.missing.remove(&page)
        {
            let part = d.texts.get(&page).and_then(|t| text::page_part(&c.selection, page, t));
            c.parts.insert(page, part);
            if c.missing.is_empty() {
                let text = c.text();
                d.copying = None;
                cx.set_clipboard_text(&text);
            }
        }
        d.prune_texts();
    }

    fn close_find(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        if let Some(d) = self.doc_mut(w) {
            d.find = None;
            if let Some(e) = d.engine.as_ref() {
                e.send(Request::StopFind);
            }
        }
        cx.focus("pages");
    }

    fn open_find(&mut self, w: WindowKey, cx: &mut Cx<Msg>) {
        let sel = self.selected_text(w).filter(|s| !s.contains('\n') && s.len() < 200);
        let Some(d) = self.doc_mut(w) else { return };
        let f = d.find.get_or_insert_with(Find::default);
        if let Some(s) = sel {
            f.query = s;
            f.typed += 1;
            let typed = f.typed;
            self.run_find(w, typed);
        }
        cx.focus("find");
    }

    fn run_find(&mut self, w: WindowKey, typed: u64) {
        let Some(d) = self.doc_mut(w) else { return };
        let n = d.pages();
        let start = d.current_page();
        let Some(f) = d.find.as_mut().filter(|f| f.typed == typed) else { return };
        f.serial += 1;
        f.results.clear();
        f.current = None;
        f.jump = true;
        f.pages = n;
        f.stopped = false;
        let Some(e) = d.engine.as_ref() else {
            // No worker to search with (it gave up).
            f.stopped = !f.query.is_empty();
            return;
        };
        if f.query.is_empty() {
            f.pages = 0;
            e.send(Request::StopFind);
            return;
        }
        // From the page being read, round to it.
        let pages: Vec<u32> = (start..n).chain(0..start).map(|p| p as u32).collect();
        e.send(Request::Find { serial: f.serial, query: f.query.clone(), case: f.case, pages });
    }

    fn find_step(&mut self, w: WindowKey, forward: bool) {
        let Some(d) = self.doc_mut(w) else { return };
        let Some(f) = d.find.as_mut() else { return };
        if let Some(next) = f.step(forward) {
            f.current = Some(next);
            f.jump = false;
            self.reveal_hit(w);
        }
    }

    fn reveal_hit(&mut self, w: WindowKey) {
        let Some(d) = self.doc_mut(w) else { return };
        let Some((page, i)) = d.find.as_ref().and_then(|f| f.current) else { return };
        let Some(rects) = d.find.as_ref().and_then(|f| f.results.get(&page)).and_then(|h| h.get(i)).cloned() else {
            return;
        };
        let Some(first) = rects.first() else { return };
        let r = d.layout.rect_to_doc(page as usize, first);
        d.reveal(r.inflate(24.0, 24.0));
        self.request(w);
    }

    fn select_all(&mut self, w: WindowKey) {
        let Some(d) = self.doc_mut(w) else { return };
        let page = d.current_page() as u32;
        let Some(t) = d.texts.get(&page) else { return };
        d.selection = Some(Selection { anchor: Caret { page, index: 0 }, focus: Caret { page, index: t.chars.len() } });
    }

    fn sheet_ok(&mut self, s: WindowKey, cx: &mut Cx<Msg>) {
        let Some(sheet) = self.sheets.get(&s).cloned() else { return };
        match sheet {
            Sheet::Password { parent, text } => {
                cx.close_window(s);
                self.sheets.remove(&s);
                if let Some(d) = self.doc_mut(parent) {
                    d.password = Some(text);
                    d.load = Load::Opening;
                }
                self.restart(parent);
            }
            Sheet::GoTo { parent, text, .. } => {
                let n = self.doc(parent).map_or(0, Doc::pages);
                match text.trim().parse::<usize>() {
                    Ok(p) if p >= 1 && p <= n => {
                        cx.close_window(s);
                        self.sheets.remove(&s);
                        self.go_page(parent, p - 1);
                    }
                    _ => {
                        if let Some(Sheet::GoTo { error, .. }) = self.sheets.get_mut(&s) {
                            *error = true;
                        }
                    }
                }
            }
            Sheet::Info { .. } => {
                cx.close_window(s);
                self.sheets.remove(&s);
            }
        }
    }

    fn close_doc(&mut self, w: WindowKey) {
        self.keep_place(w);
        if let Some(i) = self.docs.iter().position(|d| d.window == w) {
            // Dropping the engine stops its worker.
            self.docs.remove(i);
        }
        self.sheets.retain(|_, s| s.parent() != w);
    }

    /// Look at each file for a newer one (a document being written by
    /// another program, LaTeX's say) and open that in its place.
    fn watch(&mut self) {
        let mut changed = Vec::new();
        for d in &mut self.docs {
            if !matches!(d.load, Load::Open(_)) || d.reloading {
                continue;
            }
            let now = stamp(&d.path);
            match (d.stamp, now) {
                (Some(a), Some(b)) if a != b => {
                    d.stamp = Some(b);
                    changed.push(d.window);
                }
                (None, Some(b)) => d.stamp = Some(b),
                _ => {}
            }
        }
        for w in changed {
            if let Some(d) = self.doc_mut(w) {
                d.reloading = true;
            }
            self.restart(w);
        }
    }
}

impl Program for Shiori {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Shiori", "Documents").icon(Icon::BookOpen).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.scale = cx.outputs().iter().map(|o| o.scale).fold(1.0, f64::max);
        self.stream = Some(cx.stream());
        if let Some(listener) = self.instance.take() {
            crate::instance::serve(listener, cx.stream(), Msg::Raise);
        }
        cx.every(WATCH, Msg::Tick);
        let initial = std::mem::take(&mut self.initial);
        if initial.is_empty() {
            self.empty = Some(self.open_window(cx));
        } else {
            self.open_paths(initial, cx);
        }
        cx.focus("pages");
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        self.open_paths(paths, cx);
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        if let Some(k) = cx.key_window() {
            self.focused = Some(k);
        }
        let target = self.target();
        match msg {
            Msg::Nothing => {}
            Msg::Open(paths) => self.open_paths(paths, cx),
            Msg::OpenPanel => {
                let dir = self.focused_doc().and_then(|d| d.path.parent()).map(Path::to_path_buf);
                let mut panel = FileDialog::new().title("Open a Document").filter("PDF documents", &["pdf"]).multiple();
                if let Some(dir) = dir {
                    panel = panel.directory(dir);
                }
                let w = target.or(self.empty).or_else(|| cx.key_window());
                if let Some(w) = w {
                    cx.open_panel(w, panel, Msg::Opened);
                }
            }
            Msg::Opened(Some(paths)) => self.open_paths(paths, cx),
            Msg::Opened(None) => {}
            Msg::Raise(paths) => {
                if paths.is_empty() {
                    if let Some(w) = target.or(self.empty) {
                        cx.activate(w);
                    }
                } else {
                    self.open_paths(paths, cx);
                }
            }
            Msg::Worker(serial, ev) => self.worker(serial, ev, cx),
            Msg::Seen(w, seen) => {
                let Some(d) = self.doc_mut(w) else { return };
                let scale_changed = (seen.scale - d.scale).abs() > 1e-9;
                d.scale = seen.scale;
                if seen.view != d.view {
                    let old = d.view;
                    d.view = seen.view;
                    // Keep what was at the top centre there.
                    let at = (old.width / 2.0, 0.0);
                    let anchor = (!d.layout.is_empty()).then(|| Anchor::of(&d.layout, d.scroll, at));
                    d.relayout((0.0, 0.0));
                    if let Some(mut a) = anchor {
                        a.view_at = (seen.view.width / 2.0, 0.0);
                        d.scroll = a.scroll(&d.layout, d.view);
                    }
                }
                self.scale = seen.scale;
                if scale_changed {
                    self.docs.iter_mut().for_each(|d| d.sent.clear());
                }
                self.request(w);
            }
            Msg::ScrollBy(w, delta) => {
                let ticks = self.ticks;
                if let Some(d) = self.doc_mut(w) {
                    d.scroll_to((d.scroll.0 + delta.x, d.scroll.1 + delta.y));
                    d.bar_until = ticks + 1;
                }
                self.request(w);
            }
            Msg::Nav(w, n) => self.nav(w, n),
            Msg::ZoomBy { window, factor, at } => {
                let Some(d) = self.doc_mut(window) else { return };
                let to = layout::clamp_zoom(d.layout.zoom * factor);
                d.zooming += 1;
                d.settling = true;
                let seq = d.zooming;
                cx.after(Duration::from_millis(160), Msg::ZoomRested(window, seq));
                self.zoom_to(window, Zoom::Fixed(to), Some(at));
            }
            Msg::ZoomRested(w, seq) => {
                if let Some(d) = self.doc_mut(w).filter(|d| d.zooming == seq) {
                    d.settling = false;
                }
                self.request(w);
            }
            Msg::ZoomPick(w, i) => {
                if let Some(z) = crate::ui::ZOOM_CHOICES.get(i) {
                    self.zoom_to(w, *z, None);
                }
            }
            Msg::ZoomIn => {
                if let Some(w) = target {
                    self.zoom_step(w, true);
                }
            }
            Msg::ZoomOut => {
                if let Some(w) = target {
                    self.zoom_step(w, false);
                }
            }
            Msg::ActualSize => {
                if let Some(w) = target {
                    self.zoom_to(w, Zoom::Fixed(1.0), None);
                }
            }
            Msg::FitWidth => {
                if let Some(w) = target {
                    self.zoom_to(w, Zoom::FitWidth, None);
                }
            }
            Msg::FitPage => {
                if let Some(w) = target {
                    self.zoom_to(w, Zoom::FitPage, None);
                }
            }
            Msg::PreviousPage | Msg::NextPage | Msg::FirstPage | Msg::LastPage => {
                let Some(w) = target else { return };
                let Some(d) = self.doc(w) else { return };
                let (p, n) = (d.current_page(), d.pages());
                let to = match msg {
                    Msg::PreviousPage => p.saturating_sub(1),
                    Msg::NextPage => (p + 1).min(n.saturating_sub(1)),
                    Msg::FirstPage => 0,
                    _ => n.saturating_sub(1),
                };
                self.go_page(w, to);
            }
            Msg::GoPage(w, p) => self.go_page(w, p),
            Msg::GoTarget(w, t) => self.go_target(w, t, cx),
            Msg::Press { window, at, clicks, shift } => self.press(window, at, clicks, shift),
            Msg::DragTo(w, at) => self.drag_to(w, at, cx),
            Msg::DragTick(w) => {
                let Some(d) = self.doc_mut(w) else { return };
                d.autoscrolling = false;
                if d.drag == Some(Drag::Select)
                    && let Some(at) = d.drag_at
                {
                    self.drag_to(w, at, cx);
                }
            }
            Msg::Release(w) => {
                let Some(d) = self.doc_mut(w) else { return };
                d.drag = None;
                d.drag_at = None;
                if d.selection.is_some_and(|s| s.is_empty()) {
                    d.selection = None;
                }
                if let Some((t, _)) = d.link_press.take() {
                    self.go_target(w, t, cx);
                }
            }
            Msg::PanStart(w, at) => {
                if let Some(d) = self.doc_mut(w) {
                    d.link_press = None;
                    d.drag = Some(Drag::Pan { from: at, scroll: d.scroll });
                }
            }
            Msg::Escape(w) => {
                let Some(d) = self.doc_mut(w) else { return };
                if d.find.is_some() {
                    self.close_find(w, cx);
                } else if d.selection.is_some() {
                    d.selection = None;
                } else if d.fullscreen {
                    cx.set_fullscreen(w, false);
                }
            }
            Msg::Hover(w, h) => {
                if let Some(d) = self.doc_mut(w) {
                    d.hover = h;
                }
            }
            Msg::BarPress(w, at) => {
                let ticks = self.ticks;
                let Some(d) = self.doc_mut(w) else { return };
                let track = d.view.height;
                let thumb = (track * d.view.height / d.layout.size.height.max(1.0)).clamp(24.0, track);
                let top = (track - thumb) * (d.scroll.1 / (d.layout.size.height - d.view.height).max(1.0));
                let grab = if at.y >= top && at.y <= top + thumb { at.y - top } else { thumb / 2.0 };
                d.drag = Some(Drag::Bar(grab));
                d.bar_until = ticks + 1;
                self.drag_to(w, at, cx);
            }
            Msg::HBarPress(w, at) => {
                let ticks = self.ticks;
                let Some(d) = self.doc_mut(w) else { return };
                let track = d.view.width;
                let thumb = (track * d.view.width / d.layout.size.width.max(1.0)).clamp(24.0, track);
                let left = (track - thumb) * (d.scroll.0 / (d.layout.size.width - d.view.width).max(1.0));
                let grab = if at.x >= left && at.x <= left + thumb { at.x - left } else { thumb / 2.0 };
                d.drag = Some(Drag::HBar(grab));
                d.bar_until = ticks + 1;
                self.drag_to(w, at, cx);
            }
            Msg::Copy => {
                if let Some(w) = target {
                    self.copy(w, cx);
                }
            }
            Msg::SelectAll => {
                if let Some(w) = target {
                    self.select_all(w);
                }
            }
            Msg::Find => {
                if let Some(w) = target {
                    self.open_find(w, cx);
                }
            }
            Msg::FindNext | Msg::FindPrevious => {
                let Some(w) = target else { return };
                if self.doc(w).is_some_and(|d| d.find.is_none()) {
                    self.open_find(w, cx);
                }
                self.find_step(w, matches!(msg, Msg::FindNext));
            }
            Msg::FindQuery(w, q) => {
                let Some(d) = self.doc_mut(w) else { return };
                let f = d.find.get_or_insert_with(Find::default);
                if f.query == q {
                    return;
                }
                f.query = q;
                f.typed += 1;
                let typed = f.typed;
                cx.after(Duration::from_millis(200), Msg::FindRun(w, typed));
            }
            Msg::FindCase(w, on) => {
                let Some(d) = self.doc_mut(w) else { return };
                let f = d.find.get_or_insert_with(Find::default);
                f.case = on;
                f.typed += 1;
                let typed = f.typed;
                self.run_find(w, typed);
            }
            Msg::FindRun(w, typed) => self.run_find(w, typed),
            Msg::FindClose(w) => self.close_find(w, cx),
            Msg::ToggleSidebar => {
                let Some(w) = target else { return };
                if let Some(d) = self.doc_mut(w) {
                    d.sidebar = !d.sidebar;
                }
                self.request(w);
            }
            Msg::SidebarTab(w, t) => {
                if let Some(d) = self.doc_mut(w) {
                    d.sidebar_tab = t;
                }
                self.request(w);
            }
            Msg::Thumb(w, _) => self.request(w),
            Msg::OutlineToggle(w, id, open) => {
                if let Some(d) = self.doc_mut(w) {
                    if open {
                        d.expanded.insert(id);
                    } else {
                        d.expanded.remove(&id);
                    }
                }
            }
            Msg::Outline(w, id) => {
                let Some(info) = self.doc(w).and_then(Doc::info).cloned() else { return };
                let mut targets = HashMap::new();
                outline_items(&info.outline, &mut 1, &mut targets);
                if let Some(t) = targets.remove(&id) {
                    self.go_target(w, t, cx);
                }
            }
            Msg::SheetText(s, t) => match self.sheets.get_mut(&s) {
                Some(Sheet::Password { text, .. }) => *text = t,
                Some(Sheet::GoTo { text, error, .. }) => {
                    *text = t;
                    *error = false;
                }
                _ => {}
            },
            Msg::SheetOk(s) => self.sheet_ok(s, cx),
            Msg::SheetCancel(s) => {
                cx.close_window(s);
                self.sheets.remove(&s);
            }
            Msg::AskPassword(w) => self.ask_password(w, cx),
            Msg::GoToPanel => {
                let Some(w) = target.filter(|w| self.doc(*w).is_some_and(|d| d.pages() > 0)) else { return };
                let s = cx.show_sheet(w);
                self.sheets.insert(s, Sheet::GoTo { parent: w, text: String::new(), error: false });
                cx.focus("goto");
            }
            Msg::Info => {
                let Some(w) = target.filter(|w| self.doc(*w).and_then(Doc::info).is_some()) else { return };
                let s = cx.show_sheet(w);
                self.sheets.insert(s, Sheet::Info { parent: w });
            }
            Msg::FullScreen => {
                if let Some(w) = target {
                    let on = !self.doc(w).is_some_and(|d| d.fullscreen);
                    cx.set_fullscreen(w, on);
                }
            }
            Msg::Reload => {
                let Some(w) = target else { return };
                if let Some(d) = self.doc_mut(w) {
                    d.reloading = matches!(d.load, Load::Open(_));
                    if !d.reloading {
                        d.load = Load::Opening;
                    }
                    d.stamp = stamp(&d.path);
                }
                self.restart(w);
            }
            Msg::ShowInHako => {
                let Some(path) = self.focused_doc().map(|d| d.path.clone()) else { return };
                if !self.dry_run {
                    let _ = std::process::Command::new("aizome-hako").arg("--select").arg(path).spawn();
                }
            }
            Msg::Tick => {
                self.ticks += 1;
                let ticks = self.ticks;
                for d in &mut self.docs {
                    if d.trouble_until.is_some_and(|t| ticks >= t) {
                        d.trouble = None;
                        d.trouble_until = None;
                    }
                }
                self.watch();
                let windows: Vec<WindowKey> = self.docs.iter().map(|d| d.window).collect();
                for w in windows {
                    self.keep_place(w);
                }
            }
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        if let Some(s) = self.sheets.get(&window) {
            return crate::ui::sheet(self, window, s);
        }
        crate::ui::window(self, window)
    }

    fn menus(&self) -> MenuBar<Msg> {
        crate::ui::menus(self)
    }

    fn window_state(&mut self, window: WindowKey, state: aizome_kit::WindowState, _cx: &mut Cx<Msg>) {
        if let Some(d) = self.doc_mut(window) {
            d.fullscreen = state.fullscreen;
        }
    }

    fn close_requested(&mut self, window: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        if self.sheets.remove(&window).is_some() {
            return true;
        }
        if self.empty == Some(window) {
            self.empty = None;
        }
        self.close_doc(window);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_pages_text_goes_first() {
        let mut map: HashMap<u32, ()> = (0..KEPT_TEXTS as u32 + 10).map(|p| (p, ())).collect();
        prune(&mut map, 20, &|p| p == 0);
        assert_eq!(map.len(), KEPT_TEXTS);
        // The farthest went, but not page 0, which was asked to stay.
        assert!(map.contains_key(&0) && map.contains_key(&20));
        assert!(!map.contains_key(&(KEPT_TEXTS as u32 + 9)));
    }

    #[test]
    fn web_and_mail_links_open_and_no_others() {
        assert!(opens("https://sekka.computer/") && opens("MAILTO:a@b.c"));
        assert!(!opens("file:///etc/passwd") && !opens("javascript:alert(1)"));
    }
}
