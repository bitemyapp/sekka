//! Myaku's state and what each message does to it.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use aizome_kit::prelude::*;
use aizome_sys::sysinfo::Signal;

use crate::history::History;
use crate::model::{self, Frame, Kind, Lens, Meter};
use crate::sampler::{self, Command};
use crate::ui::columns::{self, Col};

pub const APP_ID: &str = "org.aizome.Myaku";

/// Which processes the table lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Show {
    /// Applications, each with its processes beneath.
    Apps,
    All,
    Mine,
}

/// What is selected: a group or a process, by its first (or own)
/// process's pid and start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sel {
    Group((u32, u64)),
    Process((u32, u64)),
}

/// A row of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowRef {
    Group(usize),
    /// A process; `true` when listed under its group.
    Process(usize, bool),
}

/// How often to read.
pub const INTERVALS: [(&str, u64); 3] = [("Every Second", 1000), ("Every 2 Seconds", 2000), ("Every 5 Seconds", 5000)];

#[derive(Clone, Debug)]
pub enum Msg {
    Frame(Arc<Frame>),
    Lens(Lens),
    Show(Show),
    Search(String),
    FocusSearch,
    Sort(usize, bool),
    Toggle((u32, u64)),
    SelectRows(Vec<usize>),
    Activate(usize),
    Select(Sel),
    Inspector(bool),
    /// Ask before signalling the selection (`true`: force).
    AskQuit(bool),
    /// Signal what was selected when asked.
    Signal(Sel, Signal),
    Pause(bool),
    Interval(u64),
    Launched(Vec<String>),
}

/// What sends signals: aizome-sys for real, a recorder in tests (a test's
/// made-up pids must never reach a real process).
pub type Signaller = Box<dyn FnMut(u32, u64, Signal) -> Result<(), String>>;

/// What a test's recorder was sent: pids and signals, in order.
pub type Sent = Rc<std::cell::RefCell<Vec<(u32, Signal)>>>;

/// What a launch asks for (`--pid`, `--lens`, `--search`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub pid: Option<u32>,
    pub lens: Option<Lens>,
    pub search: Option<String>,
}

impl Request {
    pub fn parse(args: &[String]) -> Request {
        let value = |flag: &str| args.windows(2).find(|p| p[0] == flag).map(|p| p[1].clone());
        Request {
            pid: value("--pid").and_then(|p| p.parse().ok()),
            lens: value("--lens").and_then(|l| Lens::from_id(&l)),
            search: value("--search"),
        }
    }
}

pub struct Myaku {
    pub window: WindowKey,
    pub frame: Arc<Frame>,
    pub history: History,
    pub lens: Lens,
    pub show: Show,
    /// The sort of each lens: a column and whether ascending.
    pub sorts: [(Col, bool); 5],
    pub search: String,
    pub expanded: HashSet<(u32, u64)>,
    pub rows: Rc<Vec<RowRef>>,
    pub sel: Option<Sel>,
    pub inspector: bool,
    pub interval: u64,
    pub notice: Option<(String, bool)>,
    sampler: Option<mpsc::Sender<Command>>,
    live: bool,
    signaller: Signaller,
    request: Request,
    pub listener: Option<std::os::unix::net::UnixListener>,
    /// Frames seen, and the last focus told to the sampler.
    pub frames: u64,
    told: Option<(Lens, Vec<u32>)>,
}

impl Myaku {
    /// Myaku on this computer: the sampler starts when the window opens.
    pub fn live(request: Request) -> Myaku {
        let signaller: Signaller = Box::new(|pid, start, signal| {
            aizome_sys::sysinfo::signal_started(pid, start, signal).map_err(|e| e.to_string())
        });
        Myaku { live: true, ..Myaku::with_frame(Arc::new(Frame::default()), signaller, request) }
    }

    /// Myaku showing `frame` (and what's sent as [`Msg::Frame`]), with no
    /// sampler: for tests and pictures. Signals go to `signaller`.
    pub fn with_frame(frame: Arc<Frame>, signaller: Signaller, request: Request) -> Myaku {
        let mut m = Myaku {
            window: WindowKey::NONE,
            frame: Arc::new(Frame::default()),
            history: History::default(),
            lens: request.lens.unwrap_or(Lens::Cpu),
            show: Show::Apps,
            sorts: Lens::ALL.map(|l| (columns::default_sort(l), false)),
            search: request.search.clone().unwrap_or_default(),
            expanded: HashSet::new(),
            rows: Rc::new(Vec::new()),
            sel: None,
            inspector: request.pid.is_some(),
            interval: 1000,
            notice: None,
            sampler: None,
            live: false,
            signaller,
            request,
            listener: None,
            frames: 0,
            told: None,
        };
        if frame.tick > 0 {
            m.take(frame);
        }
        m
    }

    /// A recorder of signals, for tests: what was sent, in order.
    pub fn recorder() -> (Signaller, Sent) {
        let sent: Sent = Rc::default();
        let s = sent.clone();
        (
            Box::new(move |pid, _, signal| {
                s.borrow_mut().push((pid, signal));
                Ok(())
            }),
            sent,
        )
    }

    pub fn sort(&self) -> (Col, bool) {
        self.sorts[self.lens.index()]
    }

    /// The lenses that have something to show here.
    pub fn lens_shown(&self, lens: Lens) -> bool {
        match lens {
            Lens::Energy => self.frame.power.meter != Meter::None,
            Lens::Gpu => !self.frame.gpus.is_empty(),
            _ => true,
        }
    }

    /// Take a new reading.
    fn take(&mut self, frame: Arc<Frame>) {
        self.history.push(&frame);
        self.frame = frame;
        self.frames += 1;
        if !self.lens_shown(self.lens) {
            self.lens = Lens::Cpu;
        }
        // A launch that named a process selects it once it's read.
        if let Some(pid) = self.request.pid.take() {
            match self.frame.processes.iter().find(|p| p.pid == pid) {
                Some(p) => {
                    self.sel = Some(Sel::Process(p.key()));
                    self.expanded.insert(self.frame.groups[p.group].key);
                }
                None => self.notice = Some((format!("There's no process {pid}."), true)),
            }
        } else if self.sel.is_none() && self.frames == 1 {
            // Open on the application that had the focus.
            if let Some(g) = self.frame.groups.iter().find(|g| g.focused) {
                self.sel = Some(Sel::Group(g.key));
            }
        }
        self.rebuild();
    }

    /// The group or process selected, as indices into the frame.
    pub fn selected(&self) -> Option<RowRef> {
        match self.sel? {
            Sel::Group(k) => self.frame.groups.iter().position(|g| g.key == k).map(RowRef::Group),
            Sel::Process(k) => {
                self.frame.processes.iter().position(|p| p.key() == k).map(|i| RowRef::Process(i, false))
            }
        }
    }

    fn matches(&self, p: &model::Process) -> bool {
        if self.search.is_empty() {
            return true;
        }
        let q = self.search.to_lowercase();
        p.name.to_lowercase().contains(&q)
            || p.role.to_lowercase().contains(&q)
            || p.command.to_lowercase().contains(&q)
            || p.pid.to_string() == q
            || p.user.to_lowercase() == q
    }

    /// The table's rows, filtered and sorted.
    pub fn rebuild(&mut self) {
        let f = self.frame.clone();
        let (col, asc) = self.sort();
        let cmp_p = |a: &usize, b: &usize| columns::compare_process(col, &f.processes[*a], &f.processes[*b], asc);
        let mut rows = Vec::new();
        match self.show {
            Show::Apps => {
                let mut groups: Vec<usize> = (0..f.groups.len())
                    .filter(|&g| {
                        self.search.is_empty()
                            || f.groups[g].name.to_lowercase().contains(&self.search.to_lowercase())
                            || f.groups[g].members.iter().any(|&m| self.matches(&f.processes[m]))
                    })
                    .collect();
                groups.sort_by(|a, b| columns::compare_group(col, &f.groups[*a], &f.groups[*b], &f, asc));
                for g in groups {
                    rows.push(RowRef::Group(g));
                    let grp = &f.groups[g];
                    if grp.members.len() > 1
                        && (self.expanded.contains(&grp.key)
                            || !self.search.is_empty()
                                && !grp.name.to_lowercase().contains(&self.search.to_lowercase()))
                    {
                        let mut members: Vec<usize> = grp
                            .members
                            .iter()
                            .copied()
                            .filter(|&m| {
                                self.search.is_empty()
                                    || self.matches(&f.processes[m])
                                    || grp.name.to_lowercase().contains(&self.search.to_lowercase())
                            })
                            .collect();
                        members.sort_by(cmp_p);
                        rows.extend(members.into_iter().map(|m| RowRef::Process(m, true)));
                    }
                }
            }
            Show::All | Show::Mine => {
                let mut procs: Vec<usize> = (0..f.processes.len())
                    .filter(|&i| {
                        (self.show == Show::All || f.processes[i].uid == f.uid) && self.matches(&f.processes[i])
                    })
                    .collect();
                procs.sort_by(cmp_p);
                rows.extend(procs.into_iter().map(|p| RowRef::Process(p, false)));
            }
        }
        if *self.rows != rows {
            self.rows = Rc::new(rows);
        }
    }

    /// The row the selection is on.
    pub fn selected_row(&self) -> Option<usize> {
        let sel = self.sel?;
        self.rows.iter().position(|r| match (r, sel) {
            (RowRef::Group(g), Sel::Group(k)) => self.frame.groups[*g].key == k,
            (RowRef::Process(p, _), Sel::Process(k)) => self.frame.processes[*p].key() == k,
            _ => false,
        })
    }

    fn sel_of(&self, r: RowRef) -> Sel {
        match r {
            RowRef::Group(g) => Sel::Group(self.frame.groups[g].key),
            RowRef::Process(p, _) => Sel::Process(self.frame.processes[p].key()),
        }
    }

    /// The processes the selection stands for, and a name for them.
    pub fn targets(&self, sel: Sel) -> (Vec<usize>, String) {
        let f = &self.frame;
        match sel {
            Sel::Group(k) => match f.groups.iter().find(|g| g.key == k) {
                Some(g) => (g.members.clone(), g.name.clone()),
                None => (Vec::new(), String::new()),
            },
            Sel::Process(k) => match f.processes.iter().position(|p| p.key() == k) {
                Some(i) => (vec![i], f.processes[i].name.clone()),
                None => (Vec::new(), String::new()),
            },
        }
    }

    /// Whether the selection can be quit from here, and if not, why.
    pub fn can_quit(&self) -> Result<Sel, String> {
        let sel = self.sel.ok_or("Select an application or process.")?;
        let (members, _) = self.targets(sel);
        if members.is_empty() {
            return Err("It has ended.".into());
        }
        let f = &self.frame;
        if members.iter().any(|&m| f.processes[m].kernel) {
            return Err("The kernel's threads can't be quit.".into());
        }
        if f.uid != 0 && members.iter().any(|&m| f.processes[m].uid != f.uid) {
            return Err("It belongs to another user: quit it as them, or as root.".into());
        }
        if members.iter().any(|&m| f.processes[m].pid == std::process::id()) {
            return Err("That's Myaku itself: quit it from its menu.".into());
        }
        Ok(sel)
    }

    fn tell_sampler(&mut self) {
        let Some(s) = &self.sampler else { return };
        let pids: Vec<u32> = if self.inspector {
            self.sel
                .map(|s| self.targets(s).0)
                .unwrap_or_default()
                .into_iter()
                .map(|i| self.frame.processes[i].pid)
                .collect()
        } else {
            Vec::new()
        };
        let now = (self.lens, pids);
        if self.told.as_ref() != Some(&now) {
            let _ = s.send(Command::Focus { lens: now.0, pids: now.1.clone() });
            self.told = Some(now);
        }
    }

    fn signal(&mut self, sel: Sel, signal: Signal) {
        let (members, name) = self.targets(sel);
        let mut failed = Vec::new();
        // Children first, so a parent doesn't restart what it loses.
        for &m in members.iter().rev() {
            let p = &self.frame.processes[m];
            if let Err(e) = (self.signaller)(p.pid, p.start_ticks, signal) {
                failed.push(format!("{} ({}): {e}", p.name, p.pid));
            }
        }
        let verb = match signal {
            Signal::Kill => "Force quit",
            Signal::Stop => "Paused",
            Signal::Continue => "Resumed",
            _ => "Asked to quit:",
        };
        self.notice = Some(if failed.is_empty() {
            (format!("{verb} {name}."), false)
        } else {
            (format!("{verb} {name}, but not all of it: {}", failed.join("; ")), true)
        });
    }
}

impl Program for Myaku {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Myaku", "Activity").icon(Icon::Activity).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Myaku").size(1080.0, 720.0).min_size(760.0, 480.0));
        if self.live {
            let tx = cx.stream();
            self.sampler = Some(sampler::spawn(Duration::from_millis(self.interval), move |f| tx.send(Msg::Frame(f))));
            self.tell_sampler();
        }
        if let Some(l) = self.listener.take() {
            let tx = cx.stream();
            crate::instance::serve(l, move |args| tx.send(Msg::Launched(args)));
        }
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Frame(f) => self.take(f),
            Msg::Lens(l) => {
                if self.lens_shown(l) {
                    self.lens = l;
                    self.rebuild();
                }
            }
            Msg::Show(s) => {
                self.show = s;
                self.rebuild();
            }
            Msg::Search(q) => {
                self.search = q;
                self.rebuild();
            }
            Msg::FocusSearch => cx.focus("search"),
            Msg::Sort(c, asc) => {
                let cols = columns::for_lens(self.lens);
                if let Some(col) = cols.get(c) {
                    self.sorts[self.lens.index()] = (*col, asc);
                    self.rebuild();
                }
            }
            Msg::Toggle(key) => {
                if !self.expanded.remove(&key) {
                    self.expanded.insert(key);
                }
                self.rebuild();
            }
            Msg::SelectRows(rows) => {
                if let Some(r) = rows.first().and_then(|r| self.rows.get(*r)) {
                    self.sel = Some(self.sel_of(*r));
                }
            }
            Msg::Activate(r) => {
                if let Some(r) = self.rows.get(r).copied() {
                    self.sel = Some(self.sel_of(r));
                    self.inspector = true;
                }
            }
            Msg::Select(sel) => {
                self.sel = Some(sel);
                if let Sel::Process(k) = sel
                    && let Some(p) = self.frame.processes.iter().find(|p| p.key() == k)
                {
                    self.expanded.insert(self.frame.groups[p.group].key);
                    self.rebuild();
                }
            }
            Msg::Inspector(on) => self.inspector = on,
            Msg::AskQuit(force) => match self.can_quit() {
                Ok(sel) => {
                    let (members, name) = self.targets(sel);
                    let n = members.len();
                    let what = if n > 1 { format!("its {n} processes") } else { "it".to_owned() };
                    let message = if force {
                        format!("Force quitting stops {what} at once: anything unsaved in “{name}” is lost.")
                    } else {
                        format!(
                            "Quitting asks {what} to finish and close, as {name} chooses. Force quit stops {} at once, losing what's unsaved.",
                            if n > 1 { "them" } else { "it" }
                        )
                    };
                    let mut alert = Alert::new(format!("Quit “{name}”?"), message).cancel_button("Cancel");
                    alert = if force {
                        alert.destructive_button("Force Quit", Msg::Signal(sel, Signal::Kill))
                    } else {
                        alert
                            .button("Force Quit", Msg::Signal(sel, Signal::Kill))
                            .default_button("Quit", Msg::Signal(sel, Signal::Terminate))
                    };
                    cx.alert(self.window, alert);
                }
                Err(e) => self.notice = Some((e, true)),
            },
            Msg::Signal(sel, signal) => self.signal(sel, signal),
            Msg::Pause(pause) => match self.can_quit() {
                Ok(sel) => self.signal(sel, if pause { Signal::Stop } else { Signal::Continue }),
                Err(e) => self.notice = Some((e, true)),
            },
            Msg::Interval(ms) => {
                self.interval = ms;
                if let Some(s) = &self.sampler {
                    let _ = s.send(Command::Interval(Duration::from_millis(ms)));
                }
            }
            Msg::Launched(args) => {
                cx.activate(self.window);
                let r = Request::parse(&args);
                if let Some(l) = r.lens {
                    self.lens = l;
                }
                if let Some(q) = r.search {
                    self.search = q;
                }
                if r.pid.is_some() {
                    self.request.pid = r.pid;
                    self.inspector = true;
                }
                self.rebuild();
            }
        }
        self.tell_sampler();
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        crate::ui::view::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        let lens = Lens::ALL.iter().enumerate().fold(Menu::new("View"), |m, (i, l)| {
            m.item(
                MenuItem::new(l.title(), Msg::Lens(*l))
                    .checked(self.lens == *l)
                    .enabled(self.lens_shown(*l))
                    .shortcut(Shortcut::cmd(char::from(b'1' + i as u8))),
            )
        });
        let view = lens
            .separator()
            .item(MenuItem::new("Applications", Msg::Show(Show::Apps)).checked(self.show == Show::Apps))
            .item(MenuItem::new("All Processes", Msg::Show(Show::All)).checked(self.show == Show::All))
            .item(MenuItem::new("My Processes", Msg::Show(Show::Mine)).checked(self.show == Show::Mine))
            .separator()
            .item(
                MenuItem::new(
                    if self.inspector { "Hide Inspector" } else { "Show Inspector" },
                    Msg::Inspector(!self.inspector),
                )
                .shortcut(Shortcut::cmd('I')),
            )
            .submenu(INTERVALS.iter().fold(Menu::new("Update Frequency"), |m, (label, ms)| {
                m.item(MenuItem::new(*label, Msg::Interval(*ms)).checked(self.interval == *ms))
            }));
        let can = self.can_quit().is_ok();
        let paused = self
            .sel
            .map(|s| self.targets(s).0)
            .is_some_and(|m| !m.is_empty() && m.iter().all(|&i| self.frame.processes[i].paused()));
        let process = Menu::new("Process")
            .item(MenuItem::new("Quit…", Msg::AskQuit(false)).shortcut(Shortcut::cmd('Q').alt()).enabled(can))
            .item(
                MenuItem::new("Force Quit…", Msg::AskQuit(true))
                    .shortcut(Shortcut::cmd('Q').alt().shift())
                    .enabled(can),
            )
            .separator()
            .item(
                MenuItem::new(if paused { "Resume" } else { "Pause" }, Msg::Pause(!paused))
                    .shortcut(Shortcut::cmd('P').alt())
                    .enabled(can),
            );
        let file = Menu::new("File")
            .item(MenuItem::role_item("Close Window", MenuRole::CloseWindow).shortcut(Shortcut::cmd('W')));
        let edit = Menu::new("Edit")
            .item(MenuItem::role_item("Undo", MenuRole::Undo).shortcut(Shortcut::cmd('Z')))
            .item(MenuItem::role_item("Redo", MenuRole::Redo).shortcut(Shortcut::cmd('Z').shift()))
            .separator()
            .item(MenuItem::role_item("Cut", MenuRole::Cut).shortcut(Shortcut::cmd('X')))
            .item(MenuItem::role_item("Copy", MenuRole::Copy).shortcut(Shortcut::cmd('C')))
            .item(MenuItem::role_item("Paste", MenuRole::Paste).shortcut(Shortcut::cmd('V')))
            .item(MenuItem::role_item("Select All", MenuRole::SelectAll).shortcut(Shortcut::cmd('A')))
            .separator()
            .item(MenuItem::new("Find…", Msg::FocusSearch).role(MenuRole::Find).shortcut(Shortcut::cmd('F')));
        MenuBar::standard(&self.info()).replace("Edit", edit).with_file_menu(file).with_menu(view).with_menu(process)
    }
}

/// The kinds, in the order their groups are listed when sorted by name.
pub fn kind_rank(k: Option<Kind>) -> u8 {
    match k {
        Some(Kind::App) => 0,
        Some(Kind::Service) => 1,
        Some(Kind::System) => 2,
        Some(Kind::Kernel) | None => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_requests() {
        let args: Vec<String> = ["--pid", "42", "--lens", "memory", "--search", "fox"].map(String::from).to_vec();
        assert_eq!(
            Request::parse(&args),
            Request { pid: Some(42), lens: Some(Lens::Memory), search: Some("fox".into()) }
        );
    }
}
