//! The window: the disk picker, the scan under way, and the results (the
//! sunburst, the folder's list, and the Collector).

mod colors;
pub mod drag;
mod instance;
pub mod rings;
mod suggest;
mod suggest_view;
pub mod sunburst;
mod view;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::prelude::*;

use crate::format;
use crate::mounts::{self, Mount};
use crate::nix::{self, StoreSize};
use crate::scan::{self, Progress, ProgressSnapshot, Scan, ScanOptions};
use crate::tree::NodeId;
use drag::{DragEvent, ZoneCell};
use rings::{RINGS, Seg};
use sunburst::{FRAME, Labels, RingCache, RingJob, Target, Transition};

/// Open the window (scanning `paths[0]` if given), as the one instance.
pub fn run(paths: Vec<PathBuf>) {
    let args: Vec<String> = paths.iter().map(|p| absolute(p).display().to_string()).collect();
    let mut kura = Kura::new();
    match instance::claim(&args) {
        instance::Claim::Handed => return,
        instance::Claim::First(l) => kura.listener = Some(l),
        instance::Claim::Alone => {}
    }
    aizome_kit::run(kura);
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Which page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Disks,
    Scanning,
    Results,
}

/// A scan under way.
pub struct Scanning {
    pub root: PathBuf,
    pub progress: Arc<Progress>,
    pub snapshot: ProgressSnapshot,
    pub started: Instant,
    timer: Option<TimerHandle>,
}

/// Where the index for agents stands.
#[derive(Clone, Debug, PartialEq)]
pub enum IndexState {
    None,
    Writing,
    /// Analysed; written to `json` when there's an index folder.
    Written {
        json: Option<PathBuf>,
        reclaimable: u64,
    },
    Failed(String),
}

/// What an analysis found: the hints, and where the index went.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub json: Option<PathBuf>,
    pub reclaimable: u64,
    /// `safe`, `low`, `medium`, `high` → bytes.
    pub by_risk: std::collections::BTreeMap<String, u64>,
    pub hints: Arc<Vec<crate::hints::Hint>>,
}

/// Which side of the results the panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// The folder's list and the Collector.
    #[default]
    Contents,
    /// What could be freed, and how.
    Suggestions,
}

/// An action waiting for the person's yes.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    /// Move these to the Trash.
    Trash { paths: Vec<PathBuf>, what: String },
    /// Empty the Trash for good.
    EmptyTrash,
}

#[derive(Clone, Debug)]
pub enum Msg {
    ShowDisks,
    Disks(Vec<Mount>),
    StoreMeasured(Option<StoreSize>),
    Scan(PathBuf),
    ScanHome,
    ChooseFolder,
    FolderChosen(Option<Vec<PathBuf>>),
    Poll,
    Stop,
    Scanned(Result<Arc<Scan>, String>),
    Rescan,
    /// Go into a folder.
    Open(NodeId),
    Up,
    Top,
    GoTo(NodeId),
    Select(NodeId),
    SelectRows(Vec<usize>),
    ActivateRow(usize),
    OpenSelected,
    Context(NodeId, Point),
    Drag(DragEvent),
    /// The pointer is over this part of the sunburst.
    Hover(Option<Target>),
    /// The button went down on the sunburst (or the press ended).
    Press(Option<(Target, Point)>),
    /// The button came up over this part of the sunburst.
    Release(Option<Target>),
    /// A frame of the rings moving.
    Frame,
    /// Draw the rings smooth, on a worker.
    DrawRings(RingJob),
    /// The rings, drawn.
    RingsDrawn(u64, aizome_kit::sidestep_ui::Image),
    Collect(NodeId),
    CollectSelected,
    Uncollect(NodeId),
    ClearCollector,
    AskTrash,
    Trash,
    /// What moving to the Trash did.
    Moved {
        moved: Vec<PathBuf>,
        refused: Vec<(PathBuf, String)>,
    },
    Indexed(Result<Analysis, String>),
    // Suggestions.
    ShowSide(Side),
    /// Pick a hint (its paths light up in the rings), or none.
    FocusHint(Option<usize>),
    /// Show (or hide) a hint's paths.
    ExpandHint(usize, bool),
    /// Include a path in its hint's action, or leave it out.
    Choose(String, bool),
    /// Which copy of a set of duplicates stays (the set's hash, the copy).
    Keep(String, usize),
    /// Ask to carry out a hint's action.
    Act(usize),
    /// Ask to move the other copies of a set of duplicates to the Trash.
    ActGroup(usize, usize),
    /// The person said yes.
    Confirm,
    CancelPending,
    CopyText(String),
    /// The Trash was emptied: how many items went.
    Emptied(Result<usize, String>),
    /// Folders scanned again after an action.
    Rescanned(Vec<(PathBuf, Arc<crate::tree::Tree>)>),
    OpenYukimi,
    Reveal(NodeId),
    CopyPath(NodeId),
    OpenReport,
    Launched(Vec<PathBuf>),
    Dismiss,
}

pub struct Kura {
    pub window: WindowKey,
    pub page: Page,
    pub mounts: Vec<Mount>,
    pub store: Option<StoreSize>,
    pub measuring_store: bool,
    pub scanning: Option<Scanning>,
    pub scan: Option<Arc<Scan>>,
    /// Bumped whenever the tree changes after its scan.
    pub generation: u64,
    pub center: NodeId,
    /// The centre's children, largest first.
    pub rows: Rc<Vec<NodeId>>,
    /// The sunburst's segments for the centre, and the outer folder's while
    /// the centre moves.
    pub segs: Rc<Vec<Seg>>,
    pub outer_segs: Option<Rc<Vec<Seg>>>,
    pub hover: Option<Target>,
    pub press: Option<(Target, Point)>,
    pub origin: Rc<Cell<Option<Point>>>,
    labels: Rc<std::cell::RefCell<Labels>>,
    /// The rings drawn smooth, ahead (see [`RingCache`]).
    pub rings: Rc<std::cell::RefCell<RingCache>>,
    /// Bumped when a drawing of the rings arrives.
    pub rings_gen: u64,
    pub(super) ask: Option<aizome_kit::MsgSender<Msg>>,
    pub selected: Option<NodeId>,
    pub transition: Option<Transition>,
    pub collector: Vec<NodeId>,
    pub dragging: Option<NodeId>,
    pub drag_over: bool,
    pub zone: ZoneCell,
    /// A line for the status bar, until the next action.
    pub notice: Option<String>,
    pub index: IndexState,
    /// Where `index.json` goes (`None`: not written).
    pub index_dir: Option<PathBuf>,
    pub trash_dir: Option<PathBuf>,
    /// Measure the Nix store (tests turn it off).
    pub measure_store: bool,
    /// Look for duplicates when indexing.
    pub duplicates: bool,
    pub last_root: Option<PathBuf>,
    /// Whether there's a Nix store here.
    pub has_store: bool,
    /// Where hints look (home, caches, Downloads, Trash), when not this
    /// user's own (tests' fixture homes).
    pub places: Option<crate::hints::Context>,
    /// Look for hints after each scan (tests showing made-up hints turn it off).
    pub analyse: bool,
    // Suggestions.
    pub side: Side,
    pub analysis: Analysis,
    /// The hint picked, by its key ([`hint_key`]).
    pub focus_hint: Option<String>,
    /// Hints whose paths show, by key.
    pub expanded: std::collections::HashSet<String>,
    /// Paths put in or left out of their hint's action (else by its risk).
    pub choice: std::collections::HashMap<String, bool>,
    /// The copy kept of each set of duplicates, by the set's hash.
    pub keep: std::collections::HashMap<String, usize>,
    pub pending: Option<Pending>,
    /// What the picked hint covers, in the tree.
    pub highlight: Rc<Vec<NodeId>>,
    /// Each segment's part in the highlight (see [`sunburst::Mark`]).
    pub marks: Rc<Vec<sunburst::Mark>>,
    /// Bumped when the highlight changes (the rings draw again).
    pub highlight_gen: u64,
    /// Actions and rescans under way.
    pub busy: u32,
    listener: Option<std::os::unix::net::UnixListener>,
}

/// A hint's identity across analyses: its id and title.
pub fn hint_key(h: &crate::hints::Hint) -> String {
    format!("{}\u{1f}{}", h.id, h.title)
}

impl Default for Kura {
    fn default() -> Kura {
        Kura::new()
    }
}

impl Kura {
    pub fn new() -> Kura {
        Kura {
            window: WindowKey::NONE,
            page: Page::Disks,
            mounts: Vec::new(),
            store: None,
            measuring_store: false,
            scanning: None,
            scan: None,
            generation: 0,
            center: NodeId::ROOT,
            rows: Rc::new(Vec::new()),
            segs: Rc::new(Vec::new()),
            outer_segs: None,
            hover: None,
            press: None,
            origin: Rc::new(Cell::new(None)),
            labels: Rc::default(),
            rings: Rc::default(),
            rings_gen: 0,
            ask: None,
            selected: None,
            transition: None,
            collector: Vec::new(),
            dragging: None,
            drag_over: false,
            zone: Rc::new(Cell::new(None)),
            notice: None,
            index: IndexState::None,
            index_dir: crate::index::default_dir(),
            trash_dir: crate::trash::home_trash(),
            measure_store: true,
            duplicates: true,
            last_root: None,
            has_store: nix::has_store(),
            places: None,
            analyse: true,
            side: Side::Contents,
            analysis: Analysis::default(),
            focus_hint: None,
            expanded: Default::default(),
            choice: Default::default(),
            keep: Default::default(),
            pending: None,
            highlight: Rc::new(Vec::new()),
            marks: Rc::new(Vec::new()),
            highlight_gen: 0,
            busy: 0,
            listener: None,
        }
    }

    /// A Kura that writes nothing outside `out` (for tests): the index goes
    /// there, the Trash is there, and the Nix store isn't measured.
    pub fn sandboxed(out: &Path) -> Kura {
        Kura {
            index_dir: Some(out.join("index")),
            trash_dir: Some(out.join("Trash")),
            measure_store: false,
            ..Kura::new()
        }
    }

    /// Show the scanning page with `snapshot` as its progress, without a
    /// scan (for pictures of it).
    pub fn show_progress(&mut self, root: PathBuf, snapshot: ProgressSnapshot) {
        self.scanning =
            Some(Scanning { root, progress: Progress::new(), snapshot, started: Instant::now(), timer: None });
        self.page = Page::Scanning;
    }

    pub fn tree(&self) -> Option<&crate::tree::Tree> {
        self.scan.as_deref().map(|s| &s.tree)
    }

    /// Total allocated bytes of what's collected.
    pub fn collected_bytes(&self) -> u64 {
        self.tree().map_or(0, |t| self.collector.iter().map(|&n| t.node(n).alloc).sum())
    }

    fn set_center(&mut self, center: NodeId, cx: &mut Cx<Msg>) {
        let Some(tree) = self.tree() else { return };
        if center == self.center {
            return;
        }
        let old = self.center;
        let duration = if cx.theme().motion() {
            Duration::from_millis(u64::from(cx.theme().appearance.effects.motion_ms()))
        } else {
            Duration::ZERO
        };
        // Down or up one level moves; other jumps cut.
        let started = Instant::now();
        self.transition = match (tree.parent(center), tree.parent(old)) {
            (Some(p), _) if p == old => {
                Some(Transition { outer: old, inner: center, zoom_in: true, started, duration })
            }
            (_, Some(p)) if p == center => {
                Some(Transition { outer: center, inner: old, zoom_in: false, started, duration })
            }
            _ => None,
        }
        .filter(|_| !duration.is_zero());
        self.outer_segs =
            self.transition.and_then(|t| self.tree().map(|tree| Rc::new(rings::segments(tree, t.outer, RINGS + 1))));
        if self.transition.is_some() {
            cx.after(FRAME, Msg::Frame);
        }
        self.center = center;
        self.refresh_rows();
        if self.selected.is_some_and(|s| !self.rows.contains(&s)) {
            self.selected = None;
        }
    }

    /// The centre's list and rings, after the centre or the tree changed.
    fn refresh_rows(&mut self) {
        let (rows, segs) = match self.tree() {
            Some(t) => (t.sorted_children(self.center), rings::segments(t, self.center, RINGS)),
            None => (Vec::new(), Vec::new()),
        };
        self.rows = Rc::new(rows);
        self.segs = Rc::new(segs);
        self.hover = None;
        self.press = None;
        self.refresh_marks();
    }

    /// A click on the sunburst: into a folder, up from the middle, or
    /// selecting a file.
    fn click(&mut self, t: Target, cx: &mut Cx<Msg>) {
        match t {
            Target::Center => self.update(Msg::Up, cx),
            Target::Seg(i) => {
                let Some(&seg) = self.segs.get(i) else { return };
                match seg.node {
                    Some(n) if seg.is_dir && self.tree().is_some_and(|t| t.node(n).count > 0) => {
                        self.update(Msg::Open(n), cx)
                    }
                    Some(n) => self.selected = Some(n),
                    // Smaller items: their folder.
                    None if seg.parent != self.center => self.update(Msg::Open(seg.parent), cx),
                    None => {}
                }
            }
        }
    }

    fn start_scan(&mut self, root: PathBuf, cx: &mut Cx<Msg>) {
        if let Some(s) = &self.scanning {
            s.progress.cancel();
        }
        self.finish_scan(cx);
        let progress = Progress::new();
        let p = progress.clone();
        let options = ScanOptions::new(root.clone());
        cx.spawn(move || Msg::Scanned(scan::scan(options, p).map(Arc::new).map_err(|e| e.to_string())));
        let timer = Some(cx.every(Duration::from_millis(100), Msg::Poll));
        self.scanning = Some(Scanning {
            root: root.clone(),
            progress,
            snapshot: ProgressSnapshot::default(),
            started: Instant::now(),
            timer,
        });
        self.last_root = Some(root.clone());
        self.page = Page::Scanning;
        self.notice = None;
        cx.set_title(self.window, format!("Kura — {}", format::path(&root)));
    }

    fn finish_scan(&mut self, cx: &mut Cx<Msg>) {
        if let Some(s) = self.scanning.take()
            && let Some(t) = s.timer
        {
            cx.cancel(t);
        }
    }

    /// Find the hints for the current tree and write the index for agents
    /// (when there's an index folder), in the background.
    fn write_index(&mut self, cx: &mut Cx<Msg>) {
        let Some(scan) = self.scan.clone().filter(|_| self.analyse) else { return };
        let dir = self.index_dir.clone();
        let measure = self.measure_store;
        let duplicates = self.duplicates;
        let places = self.places.clone();
        let store = self.store.clone();
        self.index = IndexState::Writing;
        cx.spawn(move || {
            let mut hcx = match places {
                Some(p) => crate::hints::Context { nix_store: p.nix_store.clone().or(store), ..p },
                None => crate::context(measure, dir.as_deref()),
            };
            hcx.duplicates = duplicates;
            let ix = crate::analyse(&scan, &hcx, None);
            let json = match &dir {
                Some(d) => match crate::index::write(&ix, d) {
                    Ok((json, _)) => Some(json),
                    Err(e) => return Msg::Indexed(Err(e.to_string())),
                },
                None => None,
            };
            Msg::Indexed(Ok(Analysis {
                json,
                reclaimable: ix.reclaimable.total_bytes,
                by_risk: ix.reclaimable.by_risk.clone(),
                hints: Arc::new(ix.hints),
            }))
        });
    }

    /// Put `node` in the Collector, if it may go to the Trash: what's in
    /// the Nix store may not, and what's inside something collected already
    /// is collected.
    pub fn collect(&mut self, node: NodeId) {
        let Some(tree) = self.scan.as_deref().map(|s| &s.tree) else { return };
        if node == tree.root() {
            self.notice = Some("The scanned folder itself can't be collected.".into());
            return;
        }
        let path = tree.path(node);
        if nix::in_store(&path) || path == Path::new(nix::STORE) {
            self.notice = Some("Store paths go when Yukimi's Storage collects garbage, not to the Trash.".into());
            return;
        }
        if self.collector.iter().any(|&c| tree.is_within(node, c)) {
            return;
        }
        self.collector.retain(|&c| !tree.is_within(c, node));
        self.collector.push(node);
        self.notice = None;
    }

    fn context_menu(&self, node: NodeId) -> Menu<Msg> {
        match self.tree() {
            Some(t) => view::menu_for(t, node, self.center, &self.collector),
            None => Menu::new(""),
        }
    }

    fn trash_targets(&self) -> Vec<(NodeId, PathBuf)> {
        let Some(tree) = self.tree() else { return Vec::new() };
        self.collector.iter().map(|&n| (n, tree.path(n))).collect()
    }
}

impl Program for Kura {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(crate::APP_ID, "Kura", "Disk Usage").icon(Icon::ChartPie).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Kura").size(1080.0, 700.0).min_size(760.0, 520.0));
        self.ask = Some(cx.stream());
        if let Some(l) = self.listener.take() {
            instance::serve(l, cx.stream(), Msg::Launched);
        }
        cx.spawn(|| Msg::Disks(mounts::list()));
        if self.measure_store && self.has_store {
            self.measuring_store = true;
            let cache = self.index_dir.as_ref().map(|d| d.join("nix-store.json"));
            cx.spawn(move || Msg::StoreMeasured(nix::store_size(cache.as_deref())));
        }
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        if let Some(p) = paths.into_iter().next() {
            self.start_scan(absolute(&p), cx);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        if log::log_enabled!(log::Level::Debug)
            && !matches!(msg, Msg::Scanned(_) | Msg::Disks(_) | Msg::Poll | Msg::DrawRings(_) | Msg::RingsDrawn(..))
        {
            log::debug!("{msg:?}");
        }
        match msg {
            Msg::ShowDisks => {
                self.page = Page::Disks;
                cx.spawn(|| Msg::Disks(mounts::list()));
            }
            Msg::Disks(m) => self.mounts = m,
            Msg::StoreMeasured(s) => {
                self.measuring_store = false;
                self.store = s;
            }
            Msg::Scan(p) => self.start_scan(p, cx),
            Msg::ScanHome => {
                if let Some(h) = crate::home_dir() {
                    self.start_scan(h, cx);
                }
            }
            Msg::ChooseFolder => {
                let dialog = FileDialog::new().title("Scan a Folder").directories();
                cx.open_panel(self.window, dialog, Msg::FolderChosen);
            }
            Msg::FolderChosen(Some(paths)) => {
                if let Some(p) = paths.into_iter().next() {
                    self.start_scan(p, cx);
                }
            }
            Msg::FolderChosen(None) => {}
            Msg::Poll => {
                if let Some(s) = &mut self.scanning {
                    s.snapshot = s.progress.snapshot();
                }
            }
            Msg::Stop => {
                if let Some(s) = &self.scanning {
                    s.progress.cancel();
                }
            }
            Msg::Scanned(result) => {
                self.finish_scan(cx);
                match result {
                    Ok(scan) => {
                        let root = scan.tree.root_path().to_path_buf();
                        self.scan = Some(scan);
                        self.generation += 1;
                        self.center = NodeId::ROOT;
                        self.transition = None;
                        self.selected = None;
                        self.collector.clear();
                        self.focus_hint = None;
                        self.highlight = Rc::new(Vec::new());
                        self.analysis = Analysis::default();
                        self.refresh_rows();
                        self.page = Page::Results;
                        cx.set_title(self.window, format!("Kura — {}", format::path(&root)));
                        // The keyboard goes to the list: arrows, Return to go in, ❖↑ out.
                        cx.focus("folder-list");
                        self.write_index(cx);
                    }
                    Err(e) => {
                        self.page = if self.scan.is_some() { Page::Results } else { Page::Disks };
                        self.notice = Some(format!("The scan didn't finish: {e}."));
                    }
                }
            }
            Msg::Rescan => {
                if let Some(root) = self.tree().map(|t| t.root_path().to_path_buf()).or_else(|| self.last_root.clone())
                {
                    self.start_scan(root, cx);
                }
            }
            Msg::Open(n) => {
                if self.tree().is_some_and(|t| t.node(n).is_dir() && t.node(n).count > 0) {
                    self.set_center(n, cx);
                }
            }
            Msg::Up => {
                if let Some(p) = self.tree().and_then(|t| t.parent(self.center)) {
                    let was = self.center;
                    self.set_center(p, cx);
                    self.selected = Some(was);
                }
            }
            Msg::Top => self.set_center(NodeId::ROOT, cx),
            Msg::GoTo(n) => self.set_center(n, cx),
            Msg::Select(n) => {
                self.selected = Some(n);
                self.notice = None;
            }
            Msg::SelectRows(rows) => self.selected = rows.first().and_then(|&i| self.rows.get(i).copied()),
            Msg::ActivateRow(i) => {
                if let Some(&n) = self.rows.get(i) {
                    self.update(Msg::Open(n), cx);
                }
            }
            Msg::OpenSelected => {
                if let Some(n) = self.selected {
                    self.update(Msg::Open(n), cx);
                }
            }
            Msg::Context(n, at) => {
                if self.rows.contains(&n) {
                    self.selected = Some(n);
                }
                let menu = self.context_menu(n);
                cx.show_context_menu(self.window, at, menu);
            }
            Msg::Drag(ev) => match ev {
                DragEvent::Start(n) => {
                    self.dragging = Some(n);
                    self.drag_over = false;
                }
                DragEvent::Over(o) => self.drag_over = o,
                DragEvent::Drop(n) => {
                    self.dragging = None;
                    self.drag_over = false;
                    self.collect(n);
                }
                DragEvent::Cancel => {
                    self.dragging = None;
                    self.drag_over = false;
                }
            },
            Msg::Hover(t) => self.hover = t,
            Msg::Press(p) => self.press = p,
            Msg::Release(t) => {
                let press = self.press.take();
                if let Some(n) = self.dragging.take() {
                    if std::mem::take(&mut self.drag_over) {
                        self.collect(n);
                    }
                } else if let Some((pt, _)) = press
                    && Some(pt) == t
                {
                    self.click(pt, cx);
                }
            }
            Msg::DrawRings(job) => cx.spawn(move || {
                let image = job.draw();
                Msg::RingsDrawn(job.key, image)
            }),
            Msg::RingsDrawn(key, image) => {
                self.rings.borrow_mut().arrived(key, image);
                self.rings_gen += 1;
            }
            Msg::Frame => {
                if self.transition.is_some_and(|t| t.progress(Instant::now()).is_some()) {
                    cx.after(FRAME, Msg::Frame);
                } else {
                    self.transition = None;
                    self.outer_segs = None;
                }
            }
            Msg::Collect(n) => self.collect(n),
            Msg::CollectSelected => {
                if let Some(n) = self.selected {
                    self.collect(n);
                }
            }
            Msg::Uncollect(n) => self.collector.retain(|&c| c != n),
            Msg::ClearCollector => self.collector.clear(),
            Msg::AskTrash => {
                if self.collector.is_empty() {
                    return;
                }
                let n = self.collector.len() as u64;
                let what = if n == 1 {
                    self.tree().map(|t| format!("“{}”", t.name(self.collector[0]))).unwrap_or_default()
                } else {
                    format::plural(n, "item", "items")
                };
                let alert = Alert::new(
                    format!("Move {what} to the Trash?"),
                    format!(
                        "{} will go to the Trash. The space is freed when the Trash is emptied; until then any file manager can put {} back.",
                        format::size(self.collected_bytes()),
                        if n == 1 { "it" } else { "them" }
                    ),
                )
                .cancel_button("Cancel")
                .destructive_button("Move to Trash", Msg::Trash);
                cx.alert(self.window, alert);
            }
            Msg::Trash => {
                let paths: Vec<PathBuf> = self.trash_targets().into_iter().map(|(_, p)| p).collect();
                self.move_to_trash(paths, cx);
            }
            Msg::Moved { moved, refused } => self.moved(moved, refused, cx),
            Msg::Indexed(r) => match r {
                Ok(a) => {
                    self.index = IndexState::Written { json: a.json.clone(), reclaimable: a.reclaimable };
                    self.analysis = a;
                    self.refresh_highlight();
                }
                Err(e) => self.index = IndexState::Failed(e),
            },
            Msg::ShowSide(s) => self.side = s,
            Msg::FocusHint(i) => self.focus_hint(i, cx),
            Msg::ExpandHint(i, on) => {
                if let Some(h) = self.analysis.hints.get(i) {
                    let k = hint_key(h);
                    if on {
                        self.expanded.insert(k);
                    } else {
                        self.expanded.remove(&k);
                    }
                }
            }
            Msg::Choose(path, on) => {
                self.choice.insert(path, on);
            }
            Msg::Keep(hash, i) => {
                self.keep.insert(hash, i);
            }
            Msg::Act(i) => self.ask_act(i, cx),
            Msg::ActGroup(i, g) => self.ask_group(i, g, cx),
            Msg::Confirm => {
                if let Some(p) = self.pending.take() {
                    self.carry_out(p, cx);
                }
            }
            Msg::CancelPending => self.pending = None,
            Msg::CopyText(t) => {
                cx.set_clipboard_text(&t);
                self.notice = Some(format!("Copied `{t}`"));
            }
            Msg::Emptied(r) => self.emptied(r, cx),
            Msg::Rescanned(subs) => self.rescanned(subs, cx),
            Msg::OpenYukimi => {
                let [cmd, args @ ..] = nix::YUKIMI_STORAGE;
                if let Err(e) = std::process::Command::new(cmd).args(args).spawn() {
                    self.notice = Some(format!("Yukimi didn't open ({e}): run `{}`.", nix::yukimi_command_line()));
                }
            }
            Msg::Reveal(n) => {
                if let Some(t) = self.tree() {
                    let p = t.path(n);
                    let dir = if t.node(n).is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
                    cx.open_url(&dir.display().to_string());
                }
            }
            Msg::CopyPath(n) => {
                if let Some(p) = self.tree().map(|t| t.path(n)) {
                    cx.set_clipboard_text(&p.display().to_string());
                    self.notice = Some(format!("Copied {}", format::path(&p)));
                }
            }
            Msg::OpenReport => {
                if let Some(d) = &self.index_dir {
                    cx.open_url(&d.join("index.md").display().to_string());
                }
            }
            Msg::Launched(paths) => {
                cx.activate(self.window);
                if let Some(p) = paths.into_iter().next() {
                    self.start_scan(p, cx);
                }
            }
            Msg::Dismiss => self.notice = None,
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        view::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let results = self.page == Page::Results;
        let scanning = self.scanning.is_some();
        let tree = self.tree();
        let can_up = results && tree.is_some_and(|t| t.parent(self.center).is_some());
        let sel_dir =
            results && self.selected.is_some_and(|s| tree.is_some_and(|t| t.node(s).is_dir() && t.node(s).count > 0));
        let selected = results && self.selected.is_some();
        let file = Menu::new("File")
            .item(MenuItem::new("Scan Home Folder", Msg::ScanHome).shortcut(Shortcut::cmd('N')))
            .item(MenuItem::new("Scan Folder…", Msg::ChooseFolder).shortcut(Shortcut::cmd('O')))
            .item(MenuItem::new("Show Disks", Msg::ShowDisks).shortcut(Shortcut::cmd('D')))
            .separator()
            .item(
                MenuItem::new("Rescan", Msg::Rescan)
                    .shortcut(Shortcut::cmd('R'))
                    .enabled(!scanning && self.last_root.is_some()),
            )
            .item(MenuItem::new("Stop Scanning", Msg::Stop).shortcut(Shortcut::cmd('.')).enabled(scanning))
            .separator()
            .item(
                MenuItem::new("Open Report for Agents", Msg::OpenReport)
                    .enabled(matches!(self.index, IndexState::Written { .. })),
            );
        let go = Menu::new("Go")
            .item(
                MenuItem::new("Enclosing Folder", Msg::Up).shortcut(Shortcut::cmd_key(ShortcutKey::Up)).enabled(can_up),
            )
            .item(
                MenuItem::new("Open Selected", Msg::OpenSelected)
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Down))
                    .enabled(sel_dir),
            )
            .item(MenuItem::new("Top", Msg::Top).shortcut(Shortcut::cmd_key(ShortcutKey::Up).shift()).enabled(can_up));
        let sel = self.selected.unwrap_or(NodeId::ROOT);
        let items = Menu::new("Items")
            .item(MenuItem::new("Collect", Msg::CollectSelected).shortcut(Shortcut::cmd('K')).enabled(selected))
            .item(
                MenuItem::new("Move Collected to Trash…", Msg::AskTrash)
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Backspace))
                    .enabled(!self.collector.is_empty()),
            )
            .item(MenuItem::new("Clear Collector", Msg::ClearCollector).enabled(!self.collector.is_empty()))
            .separator()
            .item(MenuItem::new("Reveal in Files", Msg::Reveal(sel)).enabled(selected))
            .item(MenuItem::new("Copy Path", Msg::CopyPath(sel)).shortcut(Shortcut::cmd('C').shift()).enabled(selected))
            .separator()
            .item(
                MenuItem::new("Open Yukimi's Storage", Msg::OpenYukimi).icon(Icon::Snowflake).enabled(self.has_store),
            );
        MenuBar::standard(&self.info()).with_file_menu(file).with_menu(go).with_menu(items)
    }
}
