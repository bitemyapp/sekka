//! Tansu's state and what each message does to it.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_sys::Task;
use aizome_sys::disks::{DisksState, Encryption, Erase, SelfTest, SmartAttribute, TableKind};

use crate::backend::Backend;
use crate::drives::{self, Entry};
use crate::fs;
use crate::layout::{Flags, Layout, MIB, Target};
use crate::plan::{Action, NewFs, NewPartition, Plan};
use crate::run::{Outcome, Run};

pub const APP_ID: &str = "org.aizome.Tansu";

/// What the selection names, so it survives the layout changing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    Target(Target),
    Free { offset: u64, logical: bool },
}

/// The size units the create and resize sheets offer.
pub const UNITS: [(&str, u64); 2] = [("MB", 1_000_000), ("GB", 1_000_000_000)];

#[derive(Clone, Debug, PartialEq)]
pub struct FormatForm {
    pub seg: usize,
    pub fs: usize,
    pub label: String,
    pub erase: bool,
    pub encrypt: bool,
    pub pass: String,
    pub pass2: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateForm {
    pub seg: usize,
    pub offset: u64,
    pub max: u64,
    pub size: u64,
    pub size_text: String,
    /// Invalid typed input must never submit the last valid slider value.
    pub size_error: Option<String>,
    pub unit: usize,
    /// An index into [`fs::FORMATS`]; one past them is "Unformatted".
    pub fs: usize,
    pub label: String,
    pub name: String,
    pub encrypt: bool,
    pub pass: String,
    pub pass2: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResizeForm {
    pub seg: usize,
    /// Bytes in use when it was last seen mounted.
    pub last_used: Option<u64>,
    pub from: u64,
    pub min: u64,
    pub max: u64,
    pub size: u64,
    pub size_text: String,
    pub size_error: Option<String>,
    pub unit: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApplyForm {
    /// For each internal disk the plan destroys data on: its name, and
    /// what has been typed.
    pub typed: Vec<(String, String)>,
}

pub enum SheetKind {
    Format(FormatForm),
    Create(CreateForm),
    Resize(ResizeForm),
    Label { seg: usize, label: String },
    Flags { seg: usize, flags: Flags },
    Table { kind: TableKind },
    Unlock { seg: usize, pass: String },
    Attach { path: String, read_only: bool },
    Apply(ApplyForm),
    Running,
    Smart { drive: String, attrs: Option<Result<Vec<SmartAttribute>, String>>, task: Option<Task<Vec<SmartAttribute>>> },
}

pub struct Sheet {
    pub key: WindowKey,
    pub kind: SheetKind,
    pub error: Option<String>,
}

/// An operation done at once (mount, unlock…), until it finishes.
pub struct Busy {
    pub what: String,
    pub done: String,
    pub task: BusyTask,
}

pub enum BusyTask {
    Unit(Task<()>),
    Text(Task<String>),
    Check(Task<bool>),
}

#[derive(Clone, Debug)]
pub enum SheetMsg {
    Fs(usize),
    Label(String),
    Name(String),
    Erase(bool),
    Encrypt(bool),
    Pass(String),
    Pass2(String),
    Size(f64),
    SizeText(String),
    Unit(usize),
    Flag(u8, bool),
    Table(TableKind),
    Path(String),
    ReadOnly(bool),
    Typed(usize, String),
    Choose,
    Chosen(Option<Vec<PathBuf>>),
    SelfTest(SelfTest),
    Submit,
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// UDisks (or a task) changed something.
    Changed,
    Tick,
    SelectDrive(String),
    SelectSeg(usize),
    SelectRows(Vec<usize>),
    Context(usize, Point),
    Do(Action),
    NewTable,
    AttachImage,
    DetachImage,
    PowerOff,
    ShowSmart,
    Sheet(SheetMsg),
    CloseSheet,
    Undo,
    ClearPlan,
    TogglePending,
    Apply,
    StopRun,
    CancelJob(String),
    Launched(Vec<PathBuf>),
}

/// A table row: a segment, or what an unlocked volume holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub seg: usize,
    pub inner: bool,
}

pub struct Tansu {
    pub backend: Rc<dyn Backend>,
    pub window: WindowKey,
    pub seen: u64,
    pub state: Arc<DisksState>,
    pub drives: Vec<Entry>,
    pub drive: Option<String>,
    pub layout: Rc<Layout>,
    pub rows: Rc<Vec<Row>>,
    pub sel: Option<Sel>,
    pub plan: Plan,
    history: Vec<Plan>,
    pub show_pending: bool,
    pub sheet: Option<Sheet>,
    pub run: Option<Run>,
    run_timer: Option<TimerHandle>,
    pub busy: Vec<Busy>,
    /// The status line's message, and whether it's an error.
    pub notice: Option<(String, bool)>,
    pub listener: Option<std::os::unix::net::UnixListener>,
    /// Paths handed over before the window opened.
    start_paths: Vec<PathBuf>,
    /// The drive to show first: a drive, or a block device on it
    /// (`--show`, from the desktop's disk icons).
    start_show: Option<String>,
    /// Follow UDisks on the system bus once the window opens.
    live: bool,
    /// Filesystems this system lacks the tools to make: the missing
    /// program, by filesystem (asked of UDisks once it's there).
    pub missing: std::collections::HashMap<String, String>,
    tools: Vec<(String, Task<(bool, String)>)>,
    asked_tools: bool,
    /// Where the selection was (its offset, and whether logical), for
    /// when it goes.
    sel_at: Option<(u64, bool)>,
    /// Bytes in use on each filesystem (by UUID) when last seen mounted,
    /// for how far it can shrink once unmounted.
    seen_used: std::collections::HashMap<String, u64>,
}

impl Tansu {
    pub fn new(backend: Rc<dyn Backend>) -> Tansu {
        let mut t = Tansu {
            backend,
            window: WindowKey::NONE,
            seen: u64::MAX,
            state: Arc::new(DisksState::default()),
            drives: Vec::new(),
            drive: None,
            layout: Rc::new(Layout::default()),
            rows: Rc::new(Vec::new()),
            sel: None,
            plan: Plan::default(),
            history: Vec::new(),
            show_pending: false,
            sheet: None,
            run: None,
            run_timer: None,
            busy: Vec::new(),
            notice: None,
            listener: None,
            start_paths: Vec::new(),
            start_show: None,
            live: false,
            sel_at: None,
            missing: Default::default(),
            tools: Vec::new(),
            asked_tools: false,
            seen_used: Default::default(),
        };
        t.refresh();
        t
    }

    /// Tansu on this computer's disks: UDisks2 on the system bus, followed
    /// from when the window opens.
    pub fn live() -> Tansu {
        let empty = crate::mock::Mock::new(DisksState { available: true, ..Default::default() });
        Tansu { live: true, ..Tansu::new(Rc::new(empty)) }
    }

    pub fn with_paths(mut self, paths: Vec<PathBuf>) -> Tansu {
        self.start_paths = paths;
        self
    }

    /// Show this drive first: a drive's object path, or a block device's
    /// (object path or `/dev` node) on it, a partition or an unlocked
    /// volume included.
    pub fn with_show(mut self, what: Option<String>) -> Tansu {
        self.start_show = what;
        self.refresh_show();
        self
    }

    /// Choose the drive `--show` asked for, once the drives are known.
    fn refresh_show(&mut self) {
        let Some(what) = self.start_show.clone() else { return };
        if self.drives.is_empty() {
            return;
        }
        self.start_show = None;
        if let Some(key) = crate::drives::entry_for(&self.state, &self.drives, &what) {
            self.drive = Some(key);
            self.sel = None;
            self.relayout();
        }
    }

    // --- Reading the state --------------------------------------------------

    /// Re-read UDisks's state if it changed, and the preview.
    pub fn refresh(&mut self) {
        let generation = self.backend.generation();
        if generation != self.seen {
            self.seen = generation;
            self.state = self.backend.snapshot();
            self.drives = drives::entries(&self.state);
            for b in &self.state.blocks {
                if let Some(u) = b.filesystem.as_ref().and_then(|f| f.usage)
                    && !b.id_uuid.is_empty()
                {
                    self.seen_used.insert(b.id_uuid.clone(), u.used);
                }
            }
            if self.run.is_none() {
                let dropped = self.plan.prune(&self.state);
                if !dropped.is_empty() {
                    self.notice = Some((format!("Dropped, as the disk is gone: {}.", dropped.join("; ")), true));
                }
            }
            if self.state.available && !self.asked_tools && !self.state.supported_filesystems.is_empty() {
                self.asked_tools = true;
                for fs in crate::fs::FORMATS {
                    self.tools.push((fs.to_owned(), self.backend.can_format(fs)));
                }
            }
            if self.drive.as_ref().is_none_or(|d| !self.drives.iter().any(|e| &e.key == d)) {
                self.drive = self.drives.first().map(|e| e.key.clone());
                self.sel = None;
            }
        }
        self.relayout();
        self.refresh_show();
    }

    /// The preview of the selected drive, its rows, and the selection's
    /// place in it.
    fn relayout(&mut self) {
        let layout = self
            .entry()
            .and_then(|e| e.disk.clone())
            .and_then(|d| self.plan.preview(&self.state, &d))
            .unwrap_or_default();
        let mut rows = Vec::new();
        for (i, s) in layout.segs.iter().enumerate() {
            rows.push(Row { seg: i, inner: false });
            if s.is_unlocked() {
                rows.push(Row { seg: i, inner: true });
            }
        }
        if *self.layout != layout {
            self.layout = Rc::new(layout);
        }
        if *self.rows != rows {
            self.rows = Rc::new(rows);
        }
        if self.selected().is_none()
            && let Some((offset, logical)) = self.sel_at.filter(|_| self.sel.is_some())
        {
            // What was selected is gone from the preview (deleted): what
            // took its place.
            self.sel = Some(Sel::Free { offset, logical });
        }
        if self.selected().is_none() {
            // Something a click would pick: the first partition.
            self.sel = self
                .layout
                .segs
                .iter()
                .position(|s| !s.is_free())
                .or(if self.layout.segs.is_empty() { None } else { Some(0) })
                .map(|i| self.sel_of(i));
        }
        if let (Some(Sel::Free { .. }), Some(i)) = (&self.sel, self.selected())
            && !self.layout.segs[i].is_free()
        {
            self.sel = Some(self.sel_of(i));
        }
        self.sel_at = self.selected().map(|i| (self.layout.segs[i].offset, self.layout.segs[i].logical));
    }

    pub fn entry(&self) -> Option<&Entry> {
        let d = self.drive.as_ref()?;
        self.drives.iter().find(|e| &e.key == d)
    }

    fn sel_of(&self, i: usize) -> Sel {
        let s = &self.layout.segs[i];
        match &s.target {
            Some(t) => Sel::Target(t.clone()),
            None => Sel::Free { offset: s.offset, logical: s.logical },
        }
    }

    /// The selected segment's index in the layout.
    pub fn selected(&self) -> Option<usize> {
        match self.sel.as_ref()? {
            Sel::Target(t) => self.layout.find(t),
            Sel::Free { offset, logical } => {
                // Free space moves as partitions change: the stretch that
                // holds the old start, else the nearest.
                let free = |s: &&crate::layout::Seg| s.is_free() && s.logical == *logical;
                let holds = |s: &crate::layout::Seg| s.logical == *logical && s.offset <= *offset && *offset < s.end();
                self.layout
                    .segs
                    .iter()
                    .position(|s| free(&s) && holds(s))
                    // A partition back where it was (an undone deletion).
                    .or_else(|| {
                        self.layout.segs.iter().position(|s| holds(s) && s.kind() != crate::layout::SegKind::Extended)
                    })
                    .or_else(|| {
                        self.layout
                            .segs
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| free(s))
                            .min_by_key(|(_, s)| s.offset.abs_diff(*offset))
                            .map(|(i, _)| i)
                    })
            }
        }
    }

    /// Whether `action` can be done to the selection now.
    pub fn can(&self, action: Action) -> Result<usize, String> {
        if self.run.is_some() {
            return Err("Wait for the operations to finish.".into());
        }
        let i = self.selected().ok_or("Select a partition or unallocated space.")?;
        self.plan.can(&self.layout, i, action)?;
        Ok(i)
    }

    /// The segment's block device, or for an unlocked volume, what it
    /// holds (to mount and unmount).
    fn mount_target(&self, i: usize) -> Option<String> {
        let s = &self.layout.segs[i];
        match s.crypt.as_ref().and_then(|c| c.unlocked.as_ref()) {
            Some(inner) => Some(inner.path.clone()),
            None => s.block().map(str::to_owned),
        }
    }

    /// Internal disks whose data the plan can destroy: their names, for
    /// typing.
    pub fn disks_to_type(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for op in self.plan.ops.iter().filter(|o| o.is_destructive()) {
            let entry = self.drives.iter().find(|e| e.disk.as_deref() == Some(op.disk.as_str()));
            if entry.is_some_and(|e| !e.removable) {
                let name = op.disk_device.trim_start_matches("/dev/").to_owned();
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        out
    }

    // --- Doing ----------------------------------------------------------------

    fn say(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some((text.into(), error));
    }

    fn queue(&mut self, f: impl FnOnce(&mut Plan, &DisksState, &Layout) -> Result<(), String>) -> bool {
        let before = self.plan.clone();
        let state = self.state.clone();
        let layout = self.layout.clone();
        match f(&mut self.plan, &state, &layout) {
            Ok(()) => {
                if self.plan != before {
                    self.history.push(before);
                    let n = self.plan.len();
                    self.say(
                        format!(
                            "{n} pending operation{} — nothing changes until you apply.",
                            if n == 1 { "" } else { "s" }
                        ),
                        false,
                    );
                }
                self.relayout();
                true
            }
            Err(e) => {
                if let Some(s) = &mut self.sheet {
                    s.error = Some(e);
                } else {
                    self.say(e, true);
                }
                false
            }
        }
    }

    fn open_sheet(&mut self, kind: SheetKind, cx: &mut Cx<Msg>, focus: Option<&str>) {
        if let Some(s) = self.sheet.take() {
            cx.close_window(s.key);
        }
        let key = cx.show_sheet(self.window);
        self.sheet = Some(Sheet { key, kind, error: None });
        if let Some(f) = focus {
            cx.focus(f);
        }
    }

    fn close_sheet(&mut self, cx: &mut Cx<Msg>) {
        if let Some(s) = self.sheet.take() {
            cx.close_window(s.key);
        }
    }

    fn start(&mut self, what: String, done: String, task: BusyTask) {
        self.say(what.clone(), false);
        self.busy.push(Busy { what, done, task });
        self.poll_busy(None);
    }

    /// Collect immediate operations that finished.
    fn poll_busy(&mut self, mut cx: Option<&mut Cx<Msg>>) {
        let mut i = 0;
        while i < self.busy.len() {
            let finished = match &self.busy[i].task {
                BusyTask::Unit(t) => t.is_done(),
                BusyTask::Text(t) => t.is_done(),
                BusyTask::Check(t) => t.is_done(),
            };
            if !finished {
                i += 1;
                continue;
            }
            let b = self.busy.remove(i);
            let result: Result<String, String> = match b.task {
                BusyTask::Unit(t) => t.take().unwrap_or(Ok(())).map(|()| b.done.clone()).map_err(|e| e.to_string()),
                BusyTask::Text(t) => {
                    t.take().unwrap_or(Ok(String::new())).map(|s| b.done.replace("{}", &s)).map_err(|e| e.to_string())
                }
                BusyTask::Check(t) => match t.take().unwrap_or(Ok(true)) {
                    Ok(true) => Ok(b.done.clone()),
                    Ok(false) => Err(format!(
                        "{} has errors: repair it (Partition ▸ Repair…).",
                        b.done.split(' ').next().unwrap_or("It")
                    )),
                    Err(e) => Err(e.to_string()),
                },
            };
            match result {
                Ok(text) => self.say(text, false),
                Err(e) => {
                    self.say(format!("{} failed: {e}", b.what.trim_end_matches('…')), true);
                    if let Some(cx) = cx.as_deref_mut() {
                        cx.alert(
                            self.window,
                            Alert::new(format!("{} failed", b.what.trim_end_matches('…')), e).cancel_button("OK"),
                        );
                    }
                }
            }
        }
    }

    fn advance_run(&mut self, cx: &mut Cx<Msg>) {
        let Some(run) = &mut self.run else { return };
        run.advance(&*self.backend);
        if run.is_running() {
            return;
        }
        let run = self.run.as_ref().expect("a run");
        let done = run.done_ids();
        let outcome = run.outcome.clone();
        self.plan.ops.retain(|o| !done.contains(&o.id));
        self.history.clear();
        if let Some(t) = self.run_timer.take() {
            cx.cancel(t);
        }
        match outcome {
            Some(Outcome::Done) => {
                self.say(format!("Applied {} operation{}.", done.len(), if done.len() == 1 { "" } else { "s" }), false);
            }
            Some(Outcome::Failed { op, error }) => {
                self.say(format!("Stopped at “{op}”: {error}"), true);
            }
            Some(Outcome::Stopped) => self.say("Stopped: what didn't run is still pending.", false),
            None => {}
        }
        self.seen = u64::MAX;
        self.refresh();
    }

    fn act(&mut self, action: Action, cx: &mut Cx<Msg>) {
        let i = match self.can(action) {
            Ok(i) => i,
            Err(e) => {
                self.say(e, true);
                return;
            }
        };
        let seg = self.layout.segs[i].clone();
        let name = seg.short_name();
        let block = seg.block().map(str::to_owned);
        let b = self.backend.clone();
        match action {
            Action::Mount => {
                if let Some(target) = self.mount_target(i) {
                    self.start(
                        format!("Mounting {name}…"),
                        format!("{name} is mounted at {{}}."),
                        BusyTask::Text(b.mount(&target)),
                    );
                }
            }
            Action::Unmount => {
                if let Some(target) = self.mount_target(i) {
                    self.start(
                        format!("Unmounting {name}…"),
                        format!("{name} is unmounted."),
                        BusyTask::Unit(b.unmount(&target)),
                    );
                }
            }
            Action::Lock => {
                if let Some(p) = block {
                    self.start(format!("Locking {name}…"), format!("{name} is locked."), BusyTask::Unit(b.lock(&p)));
                }
            }
            Action::SwapOn | Action::SwapOff => {
                if let Some(p) = block {
                    let on = action == Action::SwapOn;
                    let done = if on {
                        format!("{name} is in use as swap.")
                    } else {
                        format!("{name} is no longer used as swap.")
                    };
                    self.start(
                        format!("{} {name}…", if on { "Turning on swap on" } else { "Turning off swap on" }),
                        done,
                        BusyTask::Unit(b.set_swap(&p, on)),
                    );
                }
            }
            Action::Check => {
                if let Some(p) = block {
                    self.start(
                        format!("Checking {name}…"),
                        format!("{name} is consistent."),
                        BusyTask::Check(b.check(&p)),
                    );
                }
            }
            Action::Unlock => {
                self.open_sheet(SheetKind::Unlock { seg: i, pass: String::new() }, cx, Some("unlock-pass"))
            }
            Action::New => {
                let size = seg.size;
                let text = size_text(size, 1);
                let form = CreateForm {
                    seg: i,
                    offset: seg.offset,
                    max: seg.size,
                    size,
                    size_text: text,
                    size_error: None,
                    unit: 1,
                    fs: 0,
                    label: String::new(),
                    name: String::new(),
                    encrypt: false,
                    pass: String::new(),
                    pass2: String::new(),
                };
                self.open_sheet(SheetKind::Create(form), cx, Some("create-size"));
            }
            Action::Delete => {
                self.queue(|p, _, l| p.delete(l, i));
            }
            Action::Format => {
                let current = fs::FORMATS.iter().position(|f| *f == seg.fs).unwrap_or(0);
                let form = FormatForm {
                    seg: i,
                    fs: current,
                    label: if seg.is_locked() { String::new() } else { seg.label.clone() },
                    erase: false,
                    encrypt: seg.fs == "crypto_LUKS",
                    pass: String::new(),
                    pass2: String::new(),
                };
                self.open_sheet(SheetKind::Format(form), cx, Some("format-label"));
            }
            Action::Resize => {
                let last = self.last_used(&seg);
                let (min, max) = self.plan.resize_range_knowing(&self.layout, i, last);
                let unit = if seg.size >= 10_000_000_000 { 1 } else { 0 };
                let form = ResizeForm {
                    seg: i,
                    from: seg.size,
                    min,
                    max,
                    size: seg.size,
                    size_text: size_text(seg.size, unit),
                    size_error: None,
                    unit,
                    last_used: last,
                };
                self.open_sheet(SheetKind::Resize(form), cx, Some("resize-size"));
            }
            Action::Label => {
                self.open_sheet(SheetKind::Label { seg: i, label: seg.label.clone() }, cx, Some("label-field"))
            }
            Action::Flags => {
                let flags = Flags::of(&self.layout.table, &seg.type_id, seg.flag_bits);
                self.open_sheet(SheetKind::Flags { seg: i, flags }, cx, None);
            }
            Action::Repair => {
                self.queue(|p, _, l| p.repair(l, i));
            }
        }
    }

    fn sheet_msg(&mut self, m: SheetMsg, cx: &mut Cx<Msg>) {
        let Some(sheet) = &mut self.sheet else { return };
        sheet.error = None;
        let state = self.state.clone();
        match (&mut sheet.kind, m) {
            (SheetKind::Format(f), SheetMsg::Fs(i)) => f.fs = i,
            (SheetKind::Format(f), SheetMsg::Label(l)) => f.label = l,
            (SheetKind::Format(f), SheetMsg::Erase(on)) => f.erase = on,
            (SheetKind::Format(f), SheetMsg::Encrypt(on)) => f.encrypt = on,
            (SheetKind::Format(f), SheetMsg::Pass(p)) => f.pass = p,
            (SheetKind::Format(f), SheetMsg::Pass2(p)) => f.pass2 = p,
            (SheetKind::Format(f), SheetMsg::Submit) => {
                let f = f.clone();
                let fs = fs::FORMATS[f.fs.min(fs::FORMATS.len() - 1)].to_owned();
                if let Some(e) = passphrase_problem(f.encrypt, &f.pass, &f.pass2) {
                    sheet.error = Some(e);
                    return;
                }
                if let Some(why) = self.cannot_make(&fs) {
                    if let Some(s) = &mut self.sheet {
                        s.error = Some(why);
                    }
                    return;
                }
                let spec = NewFs {
                    fs,
                    label: f.label.trim().to_owned(),
                    erase: if f.erase { Erase::Zeros } else { Erase::Quick },
                    encrypt: f.encrypt.then(|| Encryption { passphrase: f.pass.clone(), luks1: false }),
                };
                if self.queue(|p, _, l| p.format(l, f.seg, spec)) {
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Create(f), SheetMsg::Fs(i)) => f.fs = i,
            (SheetKind::Create(f), SheetMsg::Label(l)) => f.label = l,
            (SheetKind::Create(f), SheetMsg::Name(n)) => f.name = n,
            (SheetKind::Create(f), SheetMsg::Encrypt(on)) => f.encrypt = on,
            (SheetKind::Create(f), SheetMsg::Pass(p)) => f.pass = p,
            (SheetKind::Create(f), SheetMsg::Pass2(p)) => f.pass2 = p,
            (SheetKind::Create(f), SheetMsg::Size(mib)) => {
                f.size = ((mib.round() as u64) * MIB).clamp(MIB, f.max);
                f.size_text = size_text(f.size, f.unit);
                f.size_error = None;
            }
            (SheetKind::Create(f), SheetMsg::Unit(u)) => {
                f.unit = u;
                f.size_text = size_text(f.size, u);
                f.size_error = None;
            }
            (SheetKind::Create(f), SheetMsg::SizeText(t)) => {
                f.size_error = match typed_size(&t, UNITS[f.unit].1, MIB, f.max) {
                    Ok(size) => {
                        f.size = size;
                        None
                    }
                    Err(error) => Some(error),
                };
                f.size_text = t;
            }
            (SheetKind::Create(f), SheetMsg::Submit) => {
                if f.size_error.is_some() {
                    return;
                }
                let f = f.clone();
                if let Some(e) = passphrase_problem(f.encrypt, &f.pass, &f.pass2) {
                    sheet.error = Some(e);
                    return;
                }
                let fs = fs::FORMATS.get(f.fs).map(|s| (*s).to_owned()).unwrap_or_default();
                if let Some(why) = self.cannot_make(&fs) {
                    if let Some(s) = &mut self.sheet {
                        s.error = Some(why);
                    }
                    return;
                }
                let p = NewPartition {
                    offset: f.offset,
                    size: f.size,
                    fs,
                    label: f.label.trim().to_owned(),
                    name: f.name.trim().to_owned(),
                    encrypt: f.encrypt.then(|| Encryption { passphrase: f.pass.clone(), luks1: false }),
                    flags: Flags::default(),
                    logical: false,
                };
                let mut made = None;
                if self.queue(|plan, _, l| plan.create(l, f.seg, p).map(|id| made = Some(id))) {
                    if let Some(id) = made {
                        self.sel = Some(Sel::Target(Target::New(id)));
                    }
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Resize(f), SheetMsg::Size(mib)) => {
                f.size = ((mib.round() as u64) * MIB).clamp(f.min, f.max);
                f.size_text = size_text(f.size, f.unit);
                f.size_error = None;
            }
            (SheetKind::Resize(f), SheetMsg::Unit(u)) => {
                f.unit = u;
                f.size_text = size_text(f.size, u);
                f.size_error = None;
            }
            (SheetKind::Resize(f), SheetMsg::SizeText(t)) => {
                f.size_error = match typed_size(&t, UNITS[f.unit].1, f.min, f.max) {
                    Ok(size) => {
                        f.size = size;
                        None
                    }
                    Err(error) => Some(error),
                };
                f.size_text = t;
            }
            (SheetKind::Resize(f), SheetMsg::Submit) => {
                if f.size_error.is_some() {
                    return;
                }
                let (seg, size) = (f.seg, f.size);
                if self.queue(|p, _, l| p.resize(l, seg, size)) {
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Label { label, .. }, SheetMsg::Label(l)) => *label = l,
            (SheetKind::Label { seg, label }, SheetMsg::Submit) => {
                let (seg, label) = (*seg, label.trim().to_owned());
                if self.queue(|p, s, l| p.label(s, l, seg, &label)) {
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Flags { flags, .. }, SheetMsg::Flag(which, on)) => match which {
                0 => flags.boot = on,
                1 => {
                    flags.esp = on;
                    if on {
                        flags.bios_grub = false;
                    }
                }
                2 => {
                    flags.bios_grub = on;
                    if on {
                        flags.esp = false;
                    }
                }
                _ => flags.hidden = on,
            },
            (SheetKind::Flags { seg, flags }, SheetMsg::Submit) => {
                let (seg, flags) = (*seg, *flags);
                if self.queue(|p, _, l| p.flags(l, seg, flags)) {
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Table { kind }, SheetMsg::Table(k)) => *kind = k,
            (SheetKind::Table { kind }, SheetMsg::Submit) => {
                let kind = *kind;
                if self.queue(|p, s, l| p.new_table(s, l, kind)) {
                    self.sel = None;
                    self.relayout();
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Unlock { pass, .. }, SheetMsg::Pass(p)) => *pass = p,
            (SheetKind::Unlock { seg, pass }, SheetMsg::Submit) => {
                if pass.is_empty() {
                    sheet.error = Some("Type its passphrase.".into());
                    return;
                }
                let seg = *seg;
                let pass = std::mem::take(pass);
                let Some(block) = self.layout.segs.get(seg).and_then(|s| s.block()).map(str::to_owned) else { return };
                let name = self.layout.segs[seg].short_name();
                let task = self.backend.unlock(&block, &pass);
                self.close_sheet(cx);
                self.start(format!("Unlocking {name}…"), format!("{name} is unlocked."), BusyTask::Text(task));
            }
            (SheetKind::Attach { path, .. }, SheetMsg::Path(p)) => *path = p,
            (SheetKind::Attach { read_only, .. }, SheetMsg::ReadOnly(on)) => *read_only = on,
            (SheetKind::Attach { .. }, SheetMsg::Choose) => {
                let dialog = FileDialog::new()
                    .title("Attach a Disk Image")
                    .filter("Disk images", &["iso", "img", "raw", "qcow2"]);
                cx.open_panel(self.window, dialog, |paths| Msg::Sheet(SheetMsg::Chosen(paths)));
            }
            (SheetKind::Attach { path, .. }, SheetMsg::Chosen(Some(paths))) => {
                if let Some(p) = paths.first() {
                    *path = p.display().to_string();
                }
            }
            (SheetKind::Attach { path, read_only }, SheetMsg::Submit) => {
                let file = PathBuf::from(path.trim());
                if !file.is_file() {
                    sheet.error = Some("Choose an image file (an .iso or .img).".into());
                    return;
                }
                if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("qcow2")) {
                    sheet.error = Some(
                        "QEMU's qcow2 images aren't raw disks: convert one with `qemu-img convert -O raw`.".into(),
                    );
                    return;
                }
                let ro = *read_only;
                let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let task = self.backend.loop_setup(&file, ro);
                self.close_sheet(cx);
                self.start(format!("Attaching {name}…"), format!("{name} is attached as {{}}."), BusyTask::Text(task));
            }
            (SheetKind::Apply(f), SheetMsg::Typed(i, text)) => {
                if let Some(t) = f.typed.get_mut(i) {
                    t.1 = text;
                }
            }
            (SheetKind::Apply(f), SheetMsg::Submit) => {
                let problems = self.plan.problems(&state);
                if !problems.is_empty() {
                    sheet.error = Some(problems.join(" "));
                    return;
                }
                if f.typed.iter().any(|(name, typed)| typed.trim() != name) {
                    sheet.error = Some("Type each disk's name as shown to confirm.".into());
                    return;
                }
                // The person agreed to the plan as shown: confirm it so.
                let run = Run::agree(&self.plan, &state);
                sheet.kind = SheetKind::Running;
                self.run = Some(run);
                self.run_timer = Some(cx.every(Duration::from_millis(250), Msg::Tick));
                self.advance_run(cx);
            }
            (SheetKind::Running, SheetMsg::Submit) => {
                if self.run.as_ref().is_none_or(|r| !r.is_running()) {
                    self.run = None;
                    self.close_sheet(cx);
                }
            }
            (SheetKind::Smart { drive, .. }, SheetMsg::SelfTest(test)) => {
                let drive = drive.clone();
                let task = self.backend.smart_selftest(&drive, test);
                let what = if test == SelfTest::Short { "short" } else { "extended" };
                self.start(
                    format!("Starting the {what} self-test…"),
                    format!("The {what} self-test is running."),
                    BusyTask::Unit(task),
                );
            }
            (SheetKind::Smart { .. }, SheetMsg::Submit) => self.close_sheet(cx),
            _ => {}
        }
    }

    fn poll_smart(&mut self) {
        if let Some(Sheet { kind: SheetKind::Smart { attrs, task, .. }, .. }) = &mut self.sheet
            && let Some(t) = task.as_ref()
            && t.is_done()
        {
            *attrs = Some(t.take().unwrap_or(Err(aizome_sys::Error::Cancelled)).map_err(|e| e.to_string()));
            *task = None;
        }
    }

    // --- Menus ------------------------------------------------------------------

    /// The actions on the selection, for the context menu and the menu
    /// bar's Partition menu.
    pub fn action_items(&self) -> Vec<MenuItem<Msg>> {
        self.action_items_at(self.selected())
    }

    /// The actions on segment `i`, enabled as they could be done to it.
    pub fn action_items_at(&self, at: Option<usize>) -> Vec<MenuItem<Msg>> {
        let ok = |a: Action| self.run.is_none() && at.is_some_and(|i| self.plan.can(&self.layout, i, a).is_ok());
        let item = |label: &str, a: Action| MenuItem::new(label, Msg::Do(a)).enabled(ok(a));
        let swap = at.and_then(|i| self.layout.segs.get(i)).is_some_and(|s| s.fs == "swap");
        let mut v = Vec::new();
        if swap {
            v.push(item("Use as Swap", Action::SwapOn));
            v.push(item("Stop Using as Swap", Action::SwapOff));
        } else {
            v.push(item("Mount", Action::Mount));
            v.push(item("Unmount", Action::Unmount));
        }
        v.push(item("Unlock…", Action::Unlock));
        v.push(item("Lock", Action::Lock));
        v.push(MenuItem::separator());
        v.push(item("New Partition…", Action::New).shortcut(Shortcut::cmd('N')));
        v.push(item("Delete Partition", Action::Delete).shortcut(Shortcut::cmd_key(ShortcutKey::Backspace)));
        v.push(item("Format…", Action::Format).shortcut(Shortcut::cmd('F').shift()));
        v.push(item("Resize…", Action::Resize).shortcut(Shortcut::cmd('R')));
        v.push(MenuItem::separator());
        v.push(item("Label…", Action::Label).shortcut(Shortcut::cmd('L')));
        v.push(item("Flags…", Action::Flags));
        v.push(MenuItem::separator());
        v.push(item("Check", Action::Check));
        v.push(item("Repair…", Action::Repair));
        v
    }

    /// The context menu of segment `i`.
    pub fn context_menu_at(&self, i: Option<usize>) -> Menu<Msg> {
        let mut m = Menu::new("").items(self.action_items_at(i));
        m = m
            .separator()
            .item(MenuItem::new("New Partition Table…", Msg::NewTable).enabled(self.can_new_table().is_ok()));
        m
    }

    pub fn can_new_table(&self) -> Result<(), String> {
        if self.run.is_some() {
            return Err("Wait for the operations to finish.".into());
        }
        if self.layout.disk.is_empty() {
            return Err("Select a drive.".into());
        }
        self.plan.can_new_table(&self.state, &self.layout)
    }
}

/// Why a passphrase can't be used, if it can't.
fn passphrase_problem(encrypt: bool, pass: &str, again: &str) -> Option<String> {
    if !encrypt {
        return None;
    }
    if pass.is_empty() {
        return Some("Choose a passphrase.".into());
    }
    if pass != again {
        return Some("The passphrases don't match.".into());
    }
    None
}

/// `bytes` in `UNITS[unit]`, for the size field.
pub fn size_text(bytes: u64, unit: usize) -> String {
    let v = bytes as f64 / UNITS[unit].1 as f64;
    if unit == 0 || v >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}

/// "20", "20.5", "512 MB", "1.5 GB", "2 GiB" in bytes (`unit` for bare
/// numbers).
pub fn parse_size(text: &str, unit: u64) -> Option<u64> {
    let t = text.trim().replace(',', ".").to_ascii_lowercase();
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    let (num, suffix) = t.split_at(split);
    let n: f64 = num.parse().ok()?;
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    let mult = match suffix.trim() {
        "" => unit,
        "b" => 1,
        "k" | "kb" => 1_000,
        "m" | "mb" => 1_000_000,
        "g" | "gb" => 1_000_000_000,
        "t" | "tb" => 1_000_000_000_000,
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        "gib" => 1 << 30,
        "tib" => 1 << 40,
        _ => return None,
    };
    let bytes = n * mult as f64;
    (bytes.is_finite() && bytes < u64::MAX as f64).then_some(bytes as u64)
}

/// Validate edited text before aligning the requested size. Slider and unit
/// changes already hold exact bytes; don't reparse their rounded display text.
fn typed_size(text: &str, unit: u64, min: u64, max: u64) -> Result<u64, String> {
    let bytes = parse_size(text, unit).ok_or_else(|| "Enter a size, such as 512 MB or 1.5 GB.".to_owned())?;
    if bytes < min || bytes > max {
        return Err(format!("Enter a size from {min} to {max} bytes."));
    }
    Ok((bytes / MIB * MIB).max(min))
}

impl Program for Tansu {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Tansu", "Disks").icon(Icon::HardDrive).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Tansu").size(1120.0, 720.0).min_size(860.0, 560.0));
        if self.live {
            let tx = cx.stream();
            let disks = aizome_sys::disks::Disks::new(aizome_sys::Wake::new(move || {
                tx.send(Msg::Changed);
            }));
            self.backend = Rc::new(disks);
            self.seen = u64::MAX;
            self.refresh();
        }
        if let Some(l) = self.listener.take() {
            crate::instance::serve(l, cx.stream(), Msg::Launched);
        }
        let paths = std::mem::take(&mut self.start_paths);
        if !paths.is_empty() {
            self.open(paths, cx);
        }
    }

    fn open(&mut self, paths: Vec<PathBuf>, cx: &mut Cx<Msg>) {
        if let Some(p) = paths.into_iter().next() {
            self.open_sheet(SheetKind::Attach { path: p.display().to_string(), read_only: true }, cx, None);
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Changed | Msg::Tick => {}
            Msg::SelectDrive(key) => {
                if self.drive.as_ref() != Some(&key) {
                    self.drive = Some(key);
                    self.sel = None;
                }
            }
            Msg::SelectSeg(i) => {
                if i < self.layout.segs.len() {
                    self.sel = Some(self.sel_of(i));
                }
            }
            Msg::SelectRows(rows) => {
                if let Some(r) = rows.first().and_then(|r| self.rows.get(*r))
                    && r.seg < self.layout.segs.len()
                {
                    self.sel = Some(self.sel_of(r.seg));
                }
            }
            Msg::Context(i, at) => {
                if i < self.layout.segs.len() {
                    self.sel = Some(self.sel_of(i));
                    let menu = self.context_menu_at(Some(i));
                    cx.show_context_menu(self.window, at, menu);
                }
            }
            Msg::Do(a) => self.act(a, cx),
            Msg::NewTable => match self.can_new_table() {
                Ok(()) => {
                    let kind = if self.layout.table == "dos" && self.layout.size < (2u64 << 40) {
                        TableKind::Dos
                    } else {
                        TableKind::Gpt
                    };
                    self.open_sheet(SheetKind::Table { kind }, cx, None);
                }
                Err(e) => self.say(e, true),
            },
            Msg::AttachImage => {
                self.open_sheet(SheetKind::Attach { path: String::new(), read_only: true }, cx, Some("attach-path"))
            }
            Msg::DetachImage => {
                if let Some(e) = self.entry().filter(|e| e.is_image()).cloned() {
                    if self.state.in_use(&e.key) {
                        self.say(format!("{} is mounted: unmount it first.", e.name), true);
                    } else {
                        let task = self.backend.loop_delete(&e.key);
                        self.start(
                            format!("Detaching {}…", e.name),
                            format!("{} is detached.", e.name),
                            BusyTask::Unit(task),
                        );
                    }
                }
            }
            Msg::PowerOff => {
                if let Some(e) = self.entry().filter(|e| e.can_power_off).cloned() {
                    if self.plan.ops.iter().any(|o| Some(&o.disk) == e.disk.as_ref()) {
                        self.say("Apply or clear the pending operations on it first.", true);
                    } else {
                        let task = self.backend.power_off(&e.key);
                        self.start(
                            format!("Powering off {}…", e.name),
                            format!("{} can be unplugged.", e.name),
                            BusyTask::Unit(task),
                        );
                    }
                }
            }
            Msg::ShowSmart => {
                if let Some(e) = self.entry().filter(|e| !e.is_image()).cloned() {
                    let has = self.state.drive(&e.key).and_then(|d| d.smart.as_ref()).is_some_and(|s| s.supported);
                    let task = has.then(|| self.backend.smart_attributes(&e.key));
                    let attrs = (!has).then(|| {
                        Err("This drive doesn't report SMART data (USB sticks and card readers seldom do).".to_owned())
                    });
                    self.open_sheet(SheetKind::Smart { drive: e.key, attrs, task }, cx, None);
                    self.poll_smart();
                }
            }
            Msg::Sheet(m) => self.sheet_msg(m, cx),
            Msg::CloseSheet => {
                if !matches!(self.sheet.as_ref().map(|s| &s.kind), Some(SheetKind::Running))
                    || self.run.as_ref().is_none_or(|r| !r.is_running())
                {
                    if matches!(self.sheet.as_ref().map(|s| &s.kind), Some(SheetKind::Running)) {
                        self.run = None;
                    }
                    self.close_sheet(cx);
                }
            }
            Msg::Undo => {
                if self.run.is_none()
                    && let Some(p) = self.history.pop()
                {
                    self.plan = p;
                    self.say("Undone.", false);
                }
            }
            Msg::ClearPlan => {
                if self.run.is_none() && !self.plan.is_empty() {
                    self.history.push(self.plan.clone());
                    self.plan = Plan::default();
                    self.say("Cleared the pending operations.", false);
                }
            }
            Msg::TogglePending => self.show_pending = !self.show_pending,
            Msg::Apply => {
                if self.run.is_none() && !self.plan.is_empty() {
                    let typed = self.disks_to_type().into_iter().map(|n| (n, String::new())).collect();
                    let focus = if self.disks_to_type().is_empty() { None } else { Some("type-0") };
                    self.open_sheet(SheetKind::Apply(ApplyForm { typed }), cx, focus);
                }
            }
            Msg::StopRun => {
                if let Some(r) = &mut self.run {
                    r.stop_requested = true;
                }
            }
            Msg::CancelJob(job) => {
                let _ = self.backend.cancel_job(&job);
            }
            Msg::Launched(paths) => {
                cx.activate(self.window);
                self.open(paths, cx);
            }
        }
        self.poll_busy(Some(cx));
        self.poll_smart();
        self.refresh();
        self.poll_tools();
        if self.run.is_some() {
            self.advance_run(cx);
        }
    }

    fn view(&self, window: WindowKey) -> Element<Msg> {
        match &self.sheet {
            Some(s) if s.key == window => crate::ui::sheets::view(self, s),
            _ => crate::ui::view::window(self),
        }
    }

    fn menus(&self) -> MenuBar<Msg> {
        let idle = self.run.is_none();
        let entry = self.entry();
        let image = entry.is_some_and(|e| e.is_image());
        let file = Menu::new("File")
            .item(MenuItem::new("Attach Disk Image…", Msg::AttachImage).shortcut(Shortcut::cmd('O')))
            .item(MenuItem::new("Detach Image", Msg::DetachImage).enabled(image && idle))
            .separator()
            .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
        let undo_label = match self.history.last() {
            Some(_) if idle => "Undo Last Operation",
            _ => "Undo",
        };
        let edit = Menu::new("Edit")
            .item(MenuItem::new(undo_label, Msg::Undo).role(MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
            .item(MenuItem::role_item("Redo", MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
            .separator()
            .item(MenuItem::role_item("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
            .item(MenuItem::role_item("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
            .item(MenuItem::role_item("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
            .item(MenuItem::role_item("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
            .separator()
            .item(
                MenuItem::new("Apply Pending Operations…", Msg::Apply)
                    .shortcut(Shortcut::cmd_key(ShortcutKey::Enter))
                    .enabled(idle && !self.plan.is_empty()),
            )
            .item(MenuItem::new("Clear Pending Operations", Msg::ClearPlan).enabled(idle && !self.plan.is_empty()));
        let partition = Menu::new("Partition").items(self.action_items());
        let drive = Menu::new("Drive")
            .item(MenuItem::new("New Partition Table…", Msg::NewTable).enabled(self.can_new_table().is_ok()))
            .item(
                MenuItem::new("SMART Data and Self-Tests…", Msg::ShowSmart)
                    .shortcut(Shortcut::cmd('I'))
                    .enabled(entry.is_some_and(|e| !e.is_image())),
            )
            .separator()
            .item(MenuItem::new("Power Off", Msg::PowerOff).enabled(entry.is_some_and(|e| e.can_power_off) && idle));
        MenuBar::standard(&self.info()).replace("Edit", edit).with_file_menu(file).with_menu(partition).with_menu(drive)
    }

    fn close_requested(&mut self, window: WindowKey, cx: &mut Cx<Msg>) -> bool {
        if window == self.window && self.run.as_ref().is_some_and(|r| r.is_running()) {
            cx.alert(
                self.window,
                Alert::new(
                    "Operations are running",
                    "Closing Tansu now wouldn't stop them, but you'd lose sight of them. Wait for them to finish.",
                )
                .cancel_button("OK"),
            );
            return false;
        }
        true
    }
}

impl Tansu {
    /// What a segment's filesystem had in use when last seen mounted.
    fn last_used(&self, seg: &crate::layout::Seg) -> Option<u64> {
        seg.used.or_else(|| {
            let b = self.state.block(seg.block()?)?;
            self.seen_used.get(&b.id_uuid).copied()
        })
    }
}

impl Tansu {
    /// Note which filesystems' tools UDisks found missing.
    fn poll_tools(&mut self) {
        let mut i = 0;
        while i < self.tools.len() {
            if !self.tools[i].1.is_done() {
                i += 1;
                continue;
            }
            let (fs, task) = self.tools.remove(i);
            if let Some(Ok((false, program))) = task.take() {
                self.missing.insert(fs, if program.is_empty() { "its tools".into() } else { program });
            }
        }
    }

    /// Why this system can't make `fs`, if it can't.
    pub fn cannot_make(&self, fs: &str) -> Option<String> {
        if let Some(program) = self.missing.get(fs) {
            return Some(format!(
                "This system can't make {} filesystems: {program} is missing (add its package to the system).",
                crate::fs::name(fs)
            ));
        }
        (!fs.is_empty() && !self.state.supported_filesystems.iter().any(|s| s == fs))
            .then(|| format!("This system can't make {} filesystems.", crate::fs::name(fs)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_with_and_without_units() {
        assert_eq!(parse_size("20", 1_000_000_000), Some(20_000_000_000));
        assert_eq!(parse_size("512 MB", 1), Some(512_000_000));
        assert_eq!(parse_size("1,5 GiB", 1), Some(1_610_612_736));
        assert_eq!(parse_size("lots", 1), None);
        assert_eq!(size_text(20_000_000_000, 1), "20.0");
        assert_eq!(size_text(512_000_000, 0), "512");
    }
}
