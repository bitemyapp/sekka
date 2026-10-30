//! Nisshi's state and what each message does to it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::prelude::*;

use crate::backend::{Client, Event, Gone};
use crate::entry::{Entry, Priority};
use crate::filter::{Chip, Filter, Scope};
use crate::model::{Store, View};
use crate::protocol::{BootInfo, FileInfo};
use crate::query::{Boot, Query};
use crate::search::{self, Search};
use crate::suggest::{self, Action, Suggestion};
use crate::time::Clock;

pub const APP_ID: &str = "org.aizome.Nisshi";

/// How many messages one read takes: the newest of the boot, then as
/// many again each time earlier ones are asked for.
pub const PAGE: u32 = 50_000;
/// The most of a log file read: its end.
pub const FILE_TAIL: u64 = 8 << 20;
/// Past this many messages, search waits for typing to pause before it
/// filters them all (the field shows each character at once).
pub const LARGE: usize = 30_000;

/// What the user may read of the system's journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Not known yet (the reader hasn't said).
    Unknown,
    /// Everything (root, or wheel, adm, systemd-journal).
    System,
    /// Only their own session's.
    Own,
}

/// Where unlocking the system's logs is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unlock {
    Locked,
    /// polkit's dialog is up (or about to be).
    Asking,
    Unlocked,
    /// It didn't work: why.
    Refused(String),
}

/// The timeline, pressed and dragged: fractions of its width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Brush {
    Down(f64),
    Move(f64),
    Up(f64),
}

#[derive(Clone, Debug)]
pub enum Msg {
    /// What a reader said: the generation of the reader it came from.
    Backend(u64, Event),
    /// Make the view again (messages arrived).
    Refresh,
    Scope(Scope),
    Level(Option<Priority>),
    Search(String),
    /// Filter by what was typed, if nothing has been typed since.
    ApplySearch(u64),
    /// Return in the search field.
    SubmitSearch,
    FocusSearch,
    Suggest(usize),
    /// Show only what one source wrote (the sidebar).
    OnlySource(std::sync::Arc<str>),
    RemoveChip(usize),
    ClearFilters,
    SelectRows(Vec<usize>),
    Activate(usize),
    /// Select a message by its seq.
    Select(u64),
    /// Read a message's fields, if it's still the one selected.
    Fetch(u64),
    /// All the suggestions for a message, if it's still the one selected.
    Deepen(u64),
    /// Which boot: an index into the boots, or `None` for all of them.
    Boot(Option<usize>),
    Live(bool),
    JumpToNewest,
    LoadOlder,
    /// Search the journal itself for what's typed (not only what's read).
    SearchJournal,
    StopSearchingJournal,
    Brush(Brush),
    Unlock,
    Lock,
    OpenFile(String),
    AllSources(bool),
    Copy,
    CopyMessage,
    Export,
    ExportTo(Option<PathBuf>),
    Exported(Result<String, String>),
    ToggleDetails,
    ToggleSidebar,
    NewestFirst(bool),
    OpenActivity(u32),
    /// Messages around this one, every filter cleared.
    Context(u64),
    /// Filter by one of a message's fields, from the details.
    FilterField(String, String),
    Reload,
    Launched(Vec<String>),
    DismissNotice,
}

/// What a launch asks for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub search: Option<String>,
    pub source: Option<String>,
    pub unit: Option<String>,
    pub level: Option<Priority>,
    pub boot: Option<i32>,
    pub scope: Option<Scope>,
    pub file: Option<String>,
    /// A message to open on, by its cursor (as `journalctl --show-cursor`
    /// prints it).
    pub cursor: Option<String>,
    /// One launch's output, by its journal identifier (`aizome-launch-…`,
    /// as the shell names a launch it watches).
    pub launch: Option<String>,
}

impl Request {
    pub fn parse(args: &[String]) -> Request {
        // A flag's value is the next argument, unless that's another flag
        // (an empty value doesn't survive the hand-over to a running window).
        let value =
            |flag: &str| args.windows(2).find(|p| p[0] == flag).map(|p| p[1].clone()).filter(|v| !v.starts_with("--"));
        Request {
            search: value("--search"),
            source: value("--source").or_else(|| value("--app")),
            unit: value("--unit"),
            level: value("--level").and_then(|l| Priority::parse(&l)),
            boot: value("--boot").and_then(|b| b.parse().ok()),
            scope: value("--scope").and_then(|s| match s.as_str() {
                "all" => Some(Scope::All),
                "problems" | "errors" => Some(Scope::Problems),
                "mine" | "session" => Some(Scope::Mine),
                "kernel" => Some(Scope::Kernel),
                "crashes" => Some(Scope::Crashes),
                "security" | "logins" => Some(Scope::Security),
                _ => None,
            }),
            file: value("--file"),
            cursor: value("--cursor"),
            launch: value("--launch").filter(|l| crate::launch::is_launch_id(l)),
        }
    }
}

/// What a request to a reader was for.
#[derive(Clone, Debug, PartialEq)]
enum Want {
    /// The newest messages: how many came.
    Load(usize),
    Older(usize),
    Follow,
    Fields(u64),
    Boots,
    Files,
    File(String),
}

pub struct Nisshi {
    pub window: WindowKey,
    pub clock: Clock,
    pub uid: u32,
    live: bool,
    user: Option<Client>,
    root: Option<Client>,
    user_gen: u64,
    root_gen: u64,
    pending: HashMap<(u64, u64), Want>,
    pub access: Access,
    /// What the user may read as themselves.
    user_access: Access,
    pub unlock: Unlock,
    /// What to do once unlocked: read this file.
    after_unlock: Option<String>,
    /// The message to select once it's read, by its cursor (`--cursor`).
    want_cursor: Option<String>,
    pub boots: Vec<BootInfo>,
    pub boot: Boot,
    pub store: Store,
    pub files: Vec<FileInfo>,
    pub file_store: Store,
    pub filter: Filter,
    pub search_text: String,
    /// Typing that hasn't filtered yet (a large boot), by its count.
    search_due: Option<u64>,
    typed: u64,
    /// The journal's own search (`--grep`), when it's been asked.
    pub journal_search: Option<String>,
    pub view: View,
    pub selected: Vec<u64>,
    /// Live: new messages come in as they're written.
    pub live_on: bool,
    /// The table follows the newest message.
    pub following: bool,
    /// Messages that came while it didn't.
    pub unseen: usize,
    /// The newest message at the top (the default), or the oldest.
    pub newest_first: bool,
    /// Where the settings are kept: none in tests and pictures.
    settings_path: Option<std::path::PathBuf>,
    pub details: bool,
    pub sidebar: bool,
    pub all_sources: bool,
    pub fields: Option<(u64, Vec<(String, String)>)>,
    pub suggestions: Vec<Suggestion>,
    pub loading: bool,
    pub notice: Option<(String, bool)>,
    /// The timeline's drag, as fractions of its width.
    pub brush: Option<(f64, f64)>,
    refresh_due: bool,
    full_refresh: bool,
    request: Request,
    pub listener: Option<std::os::unix::net::UnixListener>,
}

impl Nisshi {
    /// Nisshi on this computer: the reader starts when the window opens.
    pub fn live(request: Request) -> Nisshi {
        // SAFETY: getuid has no preconditions.
        let uid = unsafe { libc::getuid() };
        let path = crate::settings::default_path();
        let settings = crate::settings::Settings::load(&path);
        Nisshi {
            live: true,
            newest_first: settings.newest_first,
            settings_path: Some(path),
            ..Nisshi::empty(Clock::system(), uid, request)
        }
    }

    fn empty(clock: Clock, uid: u32, request: Request) -> Nisshi {
        let mut n = Nisshi {
            window: WindowKey::NONE,
            clock,
            uid,
            live: false,
            user: None,
            root: None,
            user_gen: 0,
            root_gen: 0,
            pending: HashMap::new(),
            access: Access::Unknown,
            user_access: Access::Unknown,
            unlock: Unlock::Locked,
            after_unlock: None,
            want_cursor: None,
            boots: Vec::new(),
            boot: request.boot.map_or(Boot::Current, |b| if b == 0 { Boot::Current } else { Boot::Offset(b) }),
            store: Store::default(),
            files: Vec::new(),
            file_store: Store::default(),
            filter: Filter::default(),
            search_text: String::new(),
            search_due: None,
            typed: 0,
            journal_search: None,
            view: View::default(),
            selected: Vec::new(),
            live_on: true,
            following: true,
            unseen: 0,
            newest_first: true,
            settings_path: None,
            details: true,
            sidebar: true,
            all_sources: false,
            fields: None,
            suggestions: Vec::new(),
            loading: false,
            notice: None,
            brush: None,
            refresh_due: false,
            full_refresh: false,
            request: Request::default(),
            listener: None,
        };
        n.take_request(request);
        n
    }

    /// Nisshi showing made-up messages, with no reader: for tests and
    /// pictures. `access` is what the user may read.
    pub fn sample(
        entries: Vec<Entry>,
        boots: Vec<BootInfo>,
        files: Vec<FileInfo>,
        clock: Clock,
        uid: u32,
        access: Access,
    ) -> Nisshi {
        let mut n = Nisshi::empty(clock, uid, Request::default());
        n.store.append(entries);
        n.store.complete = true;
        n.boots = boots;
        n.files = files;
        n.access = access;
        n.user_access = access;
        n.rebuild();
        n
    }

    /// Take what a test sends as [`Event::Entries`] for the id this gives
    /// as new messages, as if the journal were followed.
    #[doc(hidden)]
    pub fn sample_follow(&mut self) -> u64 {
        self.pending.insert((self.user_gen, 1), Want::Follow);
        1
    }

    /// Show a file's lines, as a test gives them.
    pub fn sample_file(&mut self, path: &str, lines: Vec<Entry>) {
        self.filter.scope = Scope::File(path.to_owned());
        self.file_store.clear();
        self.file_store.append(lines);
        self.file_store.complete = true;
        self.selected.clear();
        self.rebuild();
    }

    fn take_request(&mut self, r: Request) {
        if let Some(s) = r.scope.clone() {
            self.filter.scope = s;
        }
        if let Some(l) = r.level {
            self.filter.level = Some(l);
        }
        if let Some(s) = &r.source {
            self.filter.add(Chip::Source { name: s.as_str().into(), not: false });
        }
        if let Some(u) = &r.unit {
            let u = if u.contains('.') { u.clone() } else { format!("{u}.service") };
            self.filter.add(Chip::Unit { name: u.into(), not: false });
        }
        if let Some(q) = &r.search {
            self.search_text = q.clone();
            self.filter.search = Search::parse(q, &self.clock);
        }
        if let Some(f) = &r.file {
            self.filter.scope = Scope::File(f.clone());
        }
        if let Some(l) = &r.launch {
            self.filter.add(Chip::Launch(l.as_str().into()));
        }
        if r.cursor.is_some() {
            self.want_cursor = r.cursor.clone();
        }
        self.request = r;
    }

    /// The store the table shows: a file's, or the journal's.
    pub fn shown(&self) -> &Store {
        if matches!(self.filter.scope, Scope::File(_)) { &self.file_store } else { &self.store }
    }

    pub fn in_file(&self) -> bool {
        matches!(self.filter.scope, Scope::File(_))
    }

    /// The reader to ask: root's, once unlocked.
    fn reader(&mut self) -> Option<(u64, &mut Client)> {
        if self.unlock == Unlock::Unlocked
            && let Some(r) = self.root.as_mut()
        {
            return Some((self.root_gen, r));
        }
        let rgen = self.user_gen;
        self.user.as_mut().map(|c| (rgen, c))
    }

    fn start_user(&mut self, cx: &mut Cx<Msg>) {
        self.user_gen = self.user_gen.max(self.root_gen) + 1;
        let rgen = self.user_gen;
        let tx = cx.stream();
        self.user = Some(Client::start(false, self.clock.clone(), move |e| {
            tx.send(Msg::Backend(rgen, e));
        }));
    }

    fn start_root(&mut self, cx: &mut Cx<Msg>) {
        self.root_gen = self.user_gen.max(self.root_gen) + 1;
        let rgen = self.root_gen;
        let tx = cx.stream();
        self.unlock = Unlock::Asking;
        self.root = Some(Client::start(true, self.clock.clone(), move |e| {
            tx.send(Msg::Backend(rgen, e));
        }));
    }

    /// Read the boot's newest messages again (and then follow it).
    fn load(&mut self) {
        self.stop_reads(|w| matches!(w, Want::Load(_) | Want::Older(_) | Want::Follow));
        self.store.clear();
        self.selected.clear();
        self.fields = None;
        self.unseen = 0;
        self.following = true;
        self.loading = true;
        let q = Query {
            boot: self.boot.clone(),
            matches: self.filter.search.fields.clone(),
            grep: self.journal_search.clone(),
            limit: Some(PAGE),
            ..Query::default()
        };
        if let Some((rgen, c)) = self.reader() {
            let id = c.query(q);
            self.pending.insert((rgen, id), Want::Load(0));
        } else {
            self.loading = false;
        }
        self.full_refresh = true;
        self.rebuild();
    }

    fn load_older(&mut self) {
        if self.store.complete || self.pending.values().any(|w| matches!(w, Want::Older(_) | Want::Load(_))) {
            return;
        }
        let Some(cursor) = self.store.entries.first().and_then(|e| e.cursor.text()) else { return };
        let q = Query {
            boot: self.boot.clone(),
            matches: self.filter.search.fields.clone(),
            grep: self.journal_search.clone(),
            limit: Some(PAGE),
            before: Some(cursor),
            ..Query::default()
        };
        if let Some((rgen, c)) = self.reader() {
            let id = c.query(q);
            self.pending.insert((rgen, id), Want::Older(0));
            self.loading = true;
        }
    }

    /// Follow the journal from its newest message read, if it's the
    /// running boot's and Live is on.
    fn follow(&mut self) {
        if !self.live_on || !matches!(self.boot, Boot::Current | Boot::All) {
            return;
        }
        if self.pending.values().any(|w| *w == Want::Follow) {
            return;
        }
        // From the newest read, or from now when nothing was.
        let cursor = self.store.entries.last().and_then(|e| e.cursor.text());
        let q = Query {
            boot: self.boot.clone(),
            matches: self.filter.search.fields.clone(),
            grep: self.journal_search.clone(),
            follow: cursor.is_none(),
            follow_after: cursor,
            ..Query::default()
        };
        if let Some((rgen, c)) = self.reader() {
            let id = c.query(q);
            self.pending.insert((rgen, id), Want::Follow);
        }
    }

    /// Cancel the reads `which` says.
    fn stop_reads(&mut self, which: impl Fn(&Want) -> bool) {
        let gone: Vec<(u64, u64)> = self.pending.iter().filter(|(_, w)| which(w)).map(|(k, _)| *k).collect();
        for (rgen, id) in gone {
            self.pending.remove(&(rgen, id));
            let client = if rgen == self.root_gen {
                self.root.as_mut()
            } else if rgen == self.user_gen {
                self.user.as_mut()
            } else {
                None
            };
            if let Some(c) = client {
                c.cancel(id);
            }
        }
    }

    fn ask_boots_and_files(&mut self) {
        if let Some((rgen, c)) = self.reader() {
            let b = c.boots();
            let f = c.files();
            self.pending.insert((rgen, b), Want::Boots);
            self.pending.insert((rgen, f), Want::Files);
        }
    }

    fn read_file(&mut self, path: String) {
        self.stop_reads(|w| matches!(w, Want::File(_)));
        self.file_store.clear();
        self.selected.clear();
        self.fields = None;
        let readable = self.files.iter().find(|f| f.path == path).is_none_or(|f| f.readable);
        if !readable && self.unlock != Unlock::Unlocked {
            self.after_unlock = Some(path);
            return;
        }
        if let Some((rgen, c)) = self.reader() {
            let id = c.read(path.clone(), FILE_TAIL);
            self.pending.insert((rgen, id), Want::File(path));
            self.loading = true;
        }
    }

    /// Note that messages changed; the view follows soon.
    fn changed(&mut self, full: bool, cx: &mut Cx<Msg>) {
        self.full_refresh |= full;
        if !self.refresh_due {
            self.refresh_due = true;
            cx.after(Duration::from_millis(if self.loading { 90 } else { 30 }), Msg::Refresh);
        }
    }

    /// Make the view again from the messages and the filter.
    pub fn rebuild(&mut self) {
        self.full_refresh = false;
        let view = View::make(self.shown(), &self.filter, self.uid);
        self.view = view;
        self.after_view();
    }

    fn after_view(&mut self) {
        // A selection the filter hides goes.
        let store = if self.in_file() { &self.file_store } else { &self.store };
        let rows = &self.view.rows;
        self.selected.retain(|s| store.index_of(*s).is_some_and(|i| rows.binary_search(&(i as u32)).is_ok()));
        if self.selected.len() != 1 {
            self.fields = None;
        }
        self.suggest();
    }

    fn suggest(&mut self) {
        let store = self.shown();
        self.suggestions = if let Some((key, partial)) = search::completing(&self.search_text) {
            suggest::completions(store, &self.view, key, partial, &self.search_text)
        } else if let [seq] = self.selected[..]
            && let Some(e) = store.get(seq)
        {
            suggest::for_entry(store, &self.view, &self.filter, e, &self.clock, true)
        } else {
            suggest::overview(store, &self.view, &self.filter, &self.clock)
        };
    }

    /// The message selected, when one is.
    pub fn selection(&self) -> Option<&Entry> {
        match self.selected[..] {
            [seq] => self.shown().get(seq),
            _ => None,
        }
    }

    /// The table's row for an index into the rows (oldest first).
    pub fn display_row(&self, row: usize) -> usize {
        if self.newest_first { self.view.rows.len().saturating_sub(row + 1) } else { row }
    }

    /// The table row selected messages show in.
    pub fn selected_rows(&self) -> Vec<usize> {
        let store = self.shown();
        self.selected
            .iter()
            .filter_map(|s| store.index_of(*s))
            .filter_map(|i| self.view.row_of(i))
            .map(|r| self.display_row(r))
            .collect()
    }

    /// The row the table scrolls to show: the newest while following, the
    /// selection otherwise.
    pub fn reveal(&self) -> Option<usize> {
        if self.following && self.selected.is_empty() {
            let n = self.view.rows.len();
            return (n > 0).then(|| self.display_row(n - 1));
        }
        self.selected_rows().into_iter().next()
    }

    fn seq_at(&self, display: usize) -> Option<u64> {
        let row = self.display_row(display);
        let i = *self.view.rows.get(row)? as usize;
        self.shown().entries.get(i).map(|e| e.seq)
    }

    /// Whether the journal (not only what's read) could find more.
    pub fn can_search_journal(&self) -> bool {
        !self.in_file() && !self.store.complete && self.journal_search.is_none() && self.filter.search.grep().is_some()
    }

    /// A message as a line of text, as journalctl's short format has it.
    pub fn line_of(&self, e: &Entry) -> String {
        let pid = if e.pid > 0 { format!("[{}]", e.pid) } else { String::new() };
        format!("{} {}{pid}: {}", self.clock.stamp(e.time), e.source, e.message)
    }

    fn select(&mut self, seqs: Vec<u64>, cx: &mut Cx<Msg>) {
        let changed = seqs != self.selected;
        self.selected = seqs;
        // A selection holds the table where it is.
        self.following = self.selected.is_empty() && self.following;
        if changed {
            self.fields = None;
            if let [seq] = self.selected[..] {
                cx.after(Duration::from_millis(120), Msg::Fetch(seq));
                // What's quick now; the topics, counted over all the
                // source's messages, a moment after.
                if let Some(e) = self.shown().get(seq) {
                    self.suggestions =
                        suggest::for_entry(self.shown(), &self.view, &self.filter, e, &self.clock, false);
                }
                cx.after(Duration::from_millis(30), Msg::Deepen(seq));
            } else {
                self.suggest();
            }
        }
    }

    fn apply(&mut self, action: Action, cx: &mut Cx<Msg>) {
        match action {
            Action::Narrow { chips, scope, level } => {
                if let Some(s) = scope {
                    self.set_scope(s, cx);
                }
                if let Some(l) = level {
                    self.filter.level = Some(l);
                }
                for c in chips {
                    if let Chip::Source { not: false, .. } = c {
                        self.filter.chips.retain(|x| !matches!(x, Chip::Source { not: false, .. }));
                    }
                    self.filter.add(c);
                }
                self.rebuild();
            }
            Action::Context(seq) => self.context(seq),
            Action::Search(text) => {
                cx.focus("search");
                self.set_search(text);
            }
        }
    }

    #[doc(hidden)]
    pub fn set_search_for_test(&mut self, text: &str) -> bool {
        self.set_search(text.to_owned())
    }

    fn apply_search(&mut self, text: String) {
        if self.set_search(text) {
            self.load();
        } else {
            self.rebuild();
        }
    }

    /// Take what's typed in search: whether the journal must be read again
    /// (its fields changed, or the journal's own search no longer fits).
    fn set_search(&mut self, text: String) -> bool {
        let before = self.filter.search.fields.clone();
        self.filter.search = Search::parse(&text, &self.clock);
        self.search_text = text;
        let mut reload = self.filter.search.fields != before;
        if self.journal_search.is_some() && self.filter.search.grep() != self.journal_search {
            self.journal_search = None;
            reload = true;
        }
        reload
    }

    fn set_scope(&mut self, scope: Scope, _cx: &mut Cx<Msg>) {
        if scope == self.filter.scope {
            return;
        }
        let was_file = self.in_file();
        self.filter.scope = scope.clone();
        self.selected.clear();
        self.fields = None;
        if let Scope::File(path) = scope {
            self.read_file(path);
        } else if was_file {
            self.stop_reads(|w| matches!(w, Want::File(_)));
        }
    }

    fn context(&mut self, seq: u64) {
        let Some(e) = self.shown().get(seq) else { return };
        let t = e.time;
        let scope = if self.in_file() { self.filter.scope.clone() } else { Scope::All };
        self.filter = Filter { scope, ..Filter::default() };
        self.search_text.clear();
        self.filter.add(Chip::Range { from: t - 60_000_000, to: t + 60_000_000 });
        self.selected = vec![seq];
        self.following = false;
        self.rebuild();
    }

    /// Select the message `--cursor` named, once it's read; say so when
    /// reading ends without it.
    fn take_cursor(&mut self, cx: &mut Cx<Msg>) {
        let Some(c) = self.want_cursor.clone() else { return };
        let found = self.shown().entries.iter().find(|e| e.cursor.text().as_deref() == Some(c.as_str())).map(|e| e.seq);
        match found {
            Some(seq) => {
                self.want_cursor = None;
                self.following = false;
                self.select(vec![seq], cx);
            }
            None if !self.loading => {
                self.want_cursor = None;
                self.notice =
                    Some(("That message isn't among those read: try another boot, or Read Earlier.".into(), true));
            }
            None => {}
        }
    }

    /// What's being asked for an entry's fields.
    fn fetch(&mut self, seq: u64) {
        if self.selected != [seq] || self.fields.as_ref().is_some_and(|(s, _)| *s == seq) {
            return;
        }
        if !self.live {
            // No journal to ask: what the entry itself says.
            if let Some(e) = self.shown().get(seq) {
                self.fields = Some((seq, own_fields(e)));
            }
            return;
        }
        let Some(cursor) = self.shown().get(seq).and_then(|e| e.cursor.text()) else { return };
        self.stop_reads(|w| matches!(w, Want::Fields(_)));
        if let Some((rgen, c)) = self.reader() {
            let id = c.fields(cursor);
            self.pending.insert((rgen, id), Want::Fields(seq));
        }
    }

    fn take_event(&mut self, rgen: u64, event: Event, cx: &mut Cx<Msg>) {
        let from_root = rgen == self.root_gen && self.root.is_some();
        if rgen != self.user_gen && !from_root {
            return;
        }
        match event {
            Event::Ready(r) => {
                if from_root {
                    self.unlock = Unlock::Unlocked;
                    self.access = Access::System;
                    self.notice = None;
                    // Everything again, as root.
                    self.stop_reads(|_| true);
                    self.ask_boots_and_files();
                    if let Some(path) = self.after_unlock.take() {
                        self.filter.scope = Scope::File(path.clone());
                        self.read_file(path);
                    }
                    self.load();
                } else {
                    self.user_access = if r.system { Access::System } else { Access::Own };
                    self.access = self.user_access;
                    if r.journalctl.is_empty() {
                        self.notice = Some(("journalctl wasn't found: the journal can't be read.".into(), true));
                    }
                    self.ask_boots_and_files();
                    if let Scope::File(p) = self.filter.scope.clone() {
                        self.read_file(p);
                    }
                    self.load();
                }
            }
            Event::Entries { id, entries } => {
                let n = entries.len();
                match self.pending.get_mut(&(rgen, id)) {
                    Some(Want::Load(got)) => {
                        *got += n;
                        self.store.prepend(entries);
                        self.changed(true, cx);
                    }
                    Some(Want::Older(got)) => {
                        // The first is the cursor's own, read again.
                        let first = self.store.entries.first().map(|e| e.cursor.clone());
                        let entries: Vec<Entry> =
                            entries.into_iter().filter(|e| Some(&e.cursor) != first.as_ref()).collect();
                        *got += entries.len();
                        self.store.prepend(entries);
                        self.changed(true, cx);
                    }
                    Some(Want::Follow) => {
                        let last = self.store.entries.last().map(|e| e.cursor.clone());
                        let entries: Vec<Entry> =
                            entries.into_iter().filter(|e| Some(&e.cursor) != last.as_ref()).collect();
                        if !self.following || !self.selected.is_empty() {
                            self.unseen += entries.len();
                        }
                        self.store.append(entries);
                        self.changed(false, cx);
                    }
                    Some(Want::File(path)) if self.filter.scope == Scope::File(path.clone()) => {
                        self.file_store.append(entries);
                        self.changed(true, cx);
                    }
                    _ => {}
                }
            }
            Event::Fields { id, fields } => {
                if let Some(Want::Fields(seq)) = self.pending.get(&(rgen, id)).cloned() {
                    self.fields = Some((seq, fields));
                }
            }
            Event::Boots { id, mut boots } => {
                if self.pending.contains_key(&(rgen, id)) {
                    boots.sort_by_key(|b| std::cmp::Reverse(b.index));
                    self.boots = boots;
                }
            }
            Event::Files { id, files } => {
                if self.pending.contains_key(&(rgen, id)) {
                    self.files = files;
                }
            }
            Event::Done { id } => match self.pending.remove(&(rgen, id)) {
                Some(Want::Load(got)) => {
                    self.loading = false;
                    self.store.complete = got < PAGE as usize;
                    self.changed(true, cx);
                    self.follow();
                }
                Some(Want::Older(got)) => {
                    self.loading = false;
                    self.store.complete = got < PAGE as usize;
                    self.changed(true, cx);
                }
                Some(Want::File(_)) => {
                    self.loading = false;
                    self.file_store.complete = true;
                    self.changed(true, cx);
                }
                _ => {}
            },
            Event::Failed { id, why } => {
                if let Some(w) = self.pending.remove(&(rgen, id)) {
                    self.loading = false;
                    if crate::backend::nothing_to_read(&why) && matches!(w, Want::Load(_) | Want::Older(_)) {
                        // Nothing here the reader may read: an empty boot, which
                        // the banner explains when it's for want of permission.
                        self.store.complete = true;
                        self.changed(true, cx);
                        self.follow();
                    } else if !matches!(w, Want::Fields(_)) {
                        self.notice = Some((why, true));
                    }
                }
            }
            Event::Gone(g) => {
                self.pending.retain(|(g2, _), _| *g2 != rgen);
                if from_root {
                    let was = std::mem::replace(&mut self.unlock, Unlock::Locked);
                    self.root = None;
                    self.after_unlock = None;
                    match (&was, &g) {
                        (Unlock::Asking, Gone::Cancelled) => self.notice = Some((g.words(), false)),
                        (Unlock::Asking, _) => self.unlock = Unlock::Refused(g.words()),
                        _ => {
                            self.notice = Some((format!("The system's logs locked again. {}", g.words()), true));
                        }
                    }
                    if was == Unlock::Unlocked {
                        // Back to what the user may read.
                        self.access = self.user_access;
                        self.ask_boots_and_files();
                        self.load();
                    }
                } else {
                    self.user = None;
                    self.loading = false;
                    self.notice = Some((g.words(), true));
                }
            }
        }
    }

    /// Text for the selected messages, a line each.
    pub fn copied_text(&self) -> String {
        let store = self.shown();
        let mut lines: Vec<(usize, String)> = self
            .selected
            .iter()
            .filter_map(|s| store.index_of(*s))
            .map(|i| (i, self.line_of(&store.entries[i])))
            .collect();
        lines.sort_by_key(|(i, _)| *i);
        lines.into_iter().map(|(_, l)| l).collect::<Vec<_>>().join("\n")
    }

    /// Everything shown, for export: journal lines, or JSON lines when
    /// `json`.
    pub fn export_text(&self, json: bool) -> String {
        let store = self.shown();
        let mut out = String::new();
        for &i in self.view.rows.iter() {
            let e = &store.entries[i as usize];
            if json {
                let v = serde_json::json!({
                    "time": e.time,
                    "priority": e.priority.level(),
                    "source": &*e.source,
                    "unit": e.unit.as_deref(),
                    "pid": e.pid,
                    "message": &*e.message,
                    "cursor": e.cursor.text(),
                });
                out.push_str(&v.to_string());
            } else {
                out.push_str(&self.line_of(e));
            }
            out.push('\n');
        }
        out
    }
}

impl Program for Nisshi {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Nisshi", "Logs").icon(Icon::NotepadText).version(env!("CARGO_PKG_VERSION"))
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.window = cx.open_window(WindowSpec::new("Nisshi").size(1180.0, 760.0).min_size(780.0, 480.0));
        if self.live {
            self.start_user(cx);
            self.loading = true;
        }
        if let Some(l) = self.listener.take() {
            let tx = cx.stream();
            crate::instance::serve(l, move |args| tx.send(Msg::Launched(args)));
        }
        cx.focus("search");
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Backend(rgen, e) => self.take_event(rgen, e, cx),
            Msg::Refresh => {
                self.refresh_due = false;
                let follow_new = self.following && self.selected.is_empty();
                if self.full_refresh || self.view.seen > self.shown().entries.len() {
                    self.rebuild();
                } else {
                    let store = if self.in_file() { &self.file_store } else { &self.store };
                    self.view.extend(store, &self.filter, self.uid);
                    // The overview's counts follow along; a selection's
                    // suggestions stay put.
                    if self.selected.is_empty() {
                        self.suggest();
                    }
                }
                if follow_new {
                    self.unseen = 0;
                }
                self.take_cursor(cx);
            }
            Msg::Scope(s) => {
                self.set_scope(s, cx);
                self.rebuild();
            }
            Msg::Level(l) => {
                self.filter.level = l;
                self.rebuild();
            }
            Msg::Search(text) => {
                if self.shown().entries.len() > LARGE {
                    self.typed += 1;
                    self.search_text = text;
                    self.search_due = Some(self.typed);
                    cx.after(Duration::from_millis(80), Msg::ApplySearch(self.typed));
                } else {
                    self.search_due = None;
                    self.apply_search(text);
                }
            }
            Msg::ApplySearch(n) => {
                if self.search_due == Some(n) {
                    self.search_due = None;
                    self.apply_search(self.search_text.clone());
                }
            }
            Msg::SubmitSearch => {
                if self.search_due.take().is_some() {
                    self.apply_search(self.search_text.clone());
                }
                // Return takes the first completion, else searches the
                // journal itself when what's read isn't all of it.
                if search::completing(&self.search_text).is_some()
                    && let Some(Suggestion { action: Action::Search(t), .. }) = self.suggestions.first().cloned()
                {
                    self.set_search(t);
                    self.rebuild();
                } else if self.can_search_journal() {
                    cx.send(Msg::SearchJournal);
                }
            }
            Msg::FocusSearch => cx.focus("search"),
            Msg::Suggest(i) => {
                if let Some(s) = self.suggestions.get(i).cloned() {
                    self.apply(s.action, cx);
                }
            }
            Msg::OnlySource(name) => self
                .apply(Action::Narrow { chips: vec![Chip::Source { name, not: false }], scope: None, level: None }, cx),
            Msg::RemoveChip(i) => {
                if i < self.filter.chips.len() {
                    self.filter.chips.remove(i);
                    self.rebuild();
                }
            }
            Msg::ClearFilters => {
                let scope = if self.in_file() { self.filter.scope.clone() } else { Scope::All };
                let reload = !self.filter.search.fields.is_empty() || self.journal_search.is_some();
                self.filter = Filter { scope, ..Filter::default() };
                self.search_text.clear();
                self.journal_search = None;
                if reload {
                    self.load();
                } else {
                    self.rebuild();
                }
            }
            Msg::SelectRows(rows) => {
                let seqs: Vec<u64> = rows.iter().filter_map(|r| self.seq_at(*r)).collect();
                self.select(seqs, cx);
            }
            Msg::Activate(row) => {
                if let Some(seq) = self.seq_at(row) {
                    self.select(vec![seq], cx);
                    self.details = true;
                }
            }
            Msg::Select(seq) => {
                self.select(vec![seq], cx);
            }
            Msg::Fetch(seq) => self.fetch(seq),
            Msg::Deepen(seq) => {
                if self.selected == [seq] {
                    self.suggest();
                }
            }
            Msg::Boot(i) => {
                let boot = match i.and_then(|i| self.boots.get(i)) {
                    Some(b) if b.index == 0 => Boot::Current,
                    Some(b) => Boot::Id(b.boot_id.clone()),
                    None => Boot::All,
                };
                if boot != self.boot {
                    self.boot = boot;
                    if self.in_file() {
                        self.filter.scope = Scope::All;
                    }
                    self.filter.chips.retain(|c| !matches!(c, Chip::Range { .. }));
                    self.load();
                }
            }
            Msg::Live(on) => {
                self.live_on = on;
                if on {
                    self.follow();
                    self.following = true;
                    self.selected.clear();
                    self.unseen = 0;
                    self.rebuild();
                } else {
                    self.stop_reads(|w| *w == Want::Follow);
                }
            }
            Msg::JumpToNewest => {
                self.selected.clear();
                self.fields = None;
                self.following = true;
                self.unseen = 0;
                self.suggest();
            }
            Msg::LoadOlder => self.load_older(),
            Msg::SearchJournal => {
                if let Some(g) = self.filter.search.grep() {
                    self.journal_search = Some(g);
                    self.load();
                }
            }
            Msg::StopSearchingJournal => {
                self.journal_search = None;
                self.load();
            }
            Msg::Brush(b) => {
                let h = self.view.histogram.clone();
                match b {
                    Brush::Down(f) => self.brush = Some((f, f)),
                    Brush::Move(f) => {
                        if let Some((a, _)) = self.brush {
                            self.brush = Some((a, f));
                        }
                    }
                    Brush::Up(f) => {
                        let Some((a, _)) = self.brush.take() else { return };
                        if (a - f).abs() < 0.006 {
                            // A press: the message nearest that moment.
                            let t = h.time_at(f);
                            let store = self.shown();
                            let near = self
                                .view
                                .rows
                                .iter()
                                .map(|&i| &store.entries[i as usize])
                                .min_by_key(|e| (e.time - t).abs())
                                .map(|e| e.seq);
                            if let Some(seq) = near {
                                self.select(vec![seq], cx);
                            }
                        } else {
                            let (lo, hi) = if a < f { (a, f) } else { (f, a) };
                            self.filter.add(Chip::Range { from: h.time_at(lo), to: h.time_at(hi) });
                            self.rebuild();
                        }
                    }
                }
            }
            Msg::Unlock => {
                if self.root.is_none() && self.live {
                    self.notice = None;
                    self.start_root(cx);
                } else if !self.live {
                    self.unlock = Unlock::Asking;
                }
            }
            Msg::Lock => {
                if let Some(mut r) = self.root.take() {
                    r.stop();
                }
                self.unlock = Unlock::Locked;
                self.pending.retain(|(g, _), _| *g != self.root_gen);
                self.access = self.user_access;
                // A file read as root may not be the user's to read.
                if self.in_file() {
                    self.filter.scope = Scope::All;
                }
                self.ask_boots_and_files();
                self.load();
            }
            Msg::OpenFile(path) => {
                let scope = Scope::File(path.clone());
                if self.filter.scope != scope {
                    self.filter.chips.clear();
                    self.set_scope(scope, cx);
                    if self.after_unlock.is_some() && self.unlock != Unlock::Asking {
                        cx.send(Msg::Unlock);
                    }
                    self.rebuild();
                }
            }
            Msg::AllSources(on) => self.all_sources = on,
            Msg::Copy => {
                let text = self.copied_text();
                if !text.is_empty() {
                    cx.set_clipboard_text(&text);
                }
            }
            Msg::CopyMessage => {
                if let Some(e) = self.selection() {
                    let m = e.message.to_string();
                    cx.set_clipboard_text(&m);
                }
            }
            Msg::Export => {
                let name = format!("nisshi-{}.log", self.clock.stamp(self.clock.now()).replace([' ', ':'], "-"));
                cx.save_panel(
                    self.window,
                    FileDialog::new().title("Export the Messages Shown").file_name(name),
                    Msg::ExportTo,
                );
            }
            Msg::ExportTo(Some(path)) => {
                let json = path.extension().is_some_and(|x| x == "json" || x == "jsonl");
                let text = self.export_text(json);
                let n = self.view.rows.len();
                cx.spawn(move || {
                    Msg::Exported(
                        std::fs::write(&path, text)
                            .map(|()| {
                                format!("Exported {} to {}.", crate::count(n, "message", "messages"), path.display())
                            })
                            .map_err(|e| format!("{}: {e}", path.display())),
                    )
                });
            }
            Msg::ExportTo(None) => {}
            Msg::Exported(r) => {
                self.notice = Some(match r {
                    Ok(m) => (m, false),
                    Err(e) => (e, true),
                })
            }
            Msg::ToggleDetails => self.details = !self.details,
            Msg::ToggleSidebar => self.sidebar = !self.sidebar,
            Msg::NewestFirst(on) => {
                // The view keeps to the newest, or to the message selected.
                self.newest_first = on;
                if let Some(path) = &self.settings_path {
                    crate::settings::Settings { newest_first: on }.save(path);
                }
            }
            Msg::OpenActivity(pid) => {
                if let Err(e) = std::process::Command::new("aizome-myaku").arg("--pid").arg(pid.to_string()).spawn() {
                    self.notice = Some((format!("Activity couldn't open: {e}"), true));
                }
            }
            Msg::Context(seq) => self.context(seq),
            Msg::FilterField(field, value) => {
                match field.as_str() {
                    "_PID" => {
                        if let Ok(p) = value.parse() {
                            self.filter.add(Chip::Pid(p));
                        }
                    }
                    "SYSLOG_IDENTIFIER" => self.apply(
                        Action::Narrow {
                            chips: vec![Chip::Source { name: value.as_str().into(), not: false }],
                            scope: None,
                            level: None,
                        },
                        cx,
                    ),
                    "_SYSTEMD_UNIT" | "_SYSTEMD_USER_UNIT" | "UNIT" | "USER_UNIT" => {
                        self.filter.add(Chip::Unit { name: value.as_str().into(), not: false })
                    }
                    "PRIORITY" => self.filter.level = value.parse().ok().map(Priority::from_level),
                    "MESSAGE_ID" => self.filter.add(Chip::MessageId {
                        id: value.as_str().into(),
                        label: format!("Catalog id {value}").into(),
                    }),
                    "MESSAGE" => {
                        if let Some(seq) = self.selected.first().copied()
                            && let Some(e) = self.shown().get(seq)
                        {
                            let (shape, text) = (e.shape, crate::shape::shape(&e.message));
                            self.filter.add(Chip::Shape { shape, text: text.into(), not: false });
                        }
                    }
                    f => {
                        // Any other field: read again, matched by the journal.
                        let token = if value.contains(char::is_whitespace) {
                            format!("{f}=\"{value}\"")
                        } else {
                            format!("{f}={value}")
                        };
                        let text = if self.search_text.trim().is_empty() {
                            token
                        } else {
                            format!("{} {token}", self.search_text.trim_end())
                        };
                        if self.set_search(text) {
                            self.load();
                        }
                        return;
                    }
                }
                self.rebuild();
            }
            Msg::Reload => {
                if self.in_file() {
                    if let Scope::File(p) = self.filter.scope.clone() {
                        self.read_file(p);
                    }
                } else {
                    self.load();
                }
                self.ask_boots_and_files();
            }
            Msg::Launched(args) => {
                cx.activate(self.window);
                let r = Request::parse(&args);
                let reads = r.boot.is_some() || r.file.is_some();
                self.filter = Filter::default();
                self.search_text.clear();
                self.take_request(r.clone());
                if let Some(b) = r.boot {
                    self.boot = if b == 0 { Boot::Current } else { Boot::Offset(b) };
                }
                if let Some(f) = r.file {
                    self.read_file(f);
                }
                if reads && !self.in_file() {
                    self.load();
                } else {
                    self.rebuild();
                }
                self.take_cursor(cx);
            }
            Msg::DismissNotice => self.notice = None,
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        crate::ui::view::window(self)
    }

    fn menus(&self) -> MenuBar<Msg> {
        crate::ui::menus::menus(self)
    }
}

/// The fields an entry was read with, as the journal names them (for
/// made-up entries, which have no journal to ask).
pub fn own_fields(e: &Entry) -> Vec<(String, String)> {
    let mut f: Vec<(String, String)> = vec![
        ("MESSAGE".into(), e.message.to_string()),
        ("PRIORITY".into(), e.priority.level().to_string()),
        ("SYSLOG_IDENTIFIER".into(), e.source.to_string()),
    ];
    if let Some(fac) = e.facility {
        f.push(("SYSLOG_FACILITY".into(), fac.to_string()));
    }
    if let Some(id) = &e.message_id {
        f.push(("MESSAGE_ID".into(), id.to_string()));
    }
    if let Some(u) = &e.about {
        f.push(("UNIT".into(), u.to_string()));
    }
    if e.pid > 0 {
        f.push(("_PID".into(), e.pid.to_string()));
    }
    if let Some(uid) = e.uid {
        f.push(("_UID".into(), uid.to_string()));
    }
    if let Some(u) = &e.unit {
        f.push((
            if e.uid.is_some_and(|u| u > 0) { "_SYSTEMD_USER_UNIT" } else { "_SYSTEMD_UNIT" }.into(),
            u.to_string(),
        ));
    }
    let transport = format!("{:?}", e.transport).to_lowercase();
    f.push(("_TRANSPORT".into(), transport));
    if let Some(b) = &e.boot {
        f.push(("_BOOT_ID".into(), b.to_string()));
    }
    if let Some(c) = e.cursor.text() {
        f.push(("__CURSOR".into(), c));
    }
    f.push(("__REALTIME_TIMESTAMP".into(), e.time.to_string()));
    f
}
