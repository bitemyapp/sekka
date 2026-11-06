//! Tasuke's window, the program: conversations (each with its transcript,
//! its draft, where it was read to and what's open in it, kept while
//! another shows), the Models page, Memory, and first run.
//!
//! Nothing here waits. A turn runs on the agent's thread and wakes the
//! window through its handle's waker ([`crate::live`]); a fixture's turn
//! plays from timers ([`crate::script`]). Hardware is probed, models
//! downloaded and pictures shrunk on other threads, and their results
//! come back as messages.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aizome_kit::MsgSender;
use aizome_kit::WindowState;
use aizome_kit::prelude::*;
use tasuke_models::{Bytes, Catalog, Hardware, ModelId, Preferences};

use crate::agent::{Decision, Event, Level};
use crate::fixture;
use crate::models::{self, Install, Installs, Progress};
use crate::script::Player;
use crate::thumb::{self, Thumb};
use crate::time::Clock;
use crate::transcript::{Millis, Transcript};
use crate::widgets::follow::Reading;

pub const APP_ID: &str = "org.aizome.Tasuke";

/// How often the model server is looked at while the window shows, and
/// while the desktop isn't showing it.
#[cfg_attr(not(feature = "live"), allow(dead_code))]
const POLL_SHOWN: Duration = Duration::from_secs(5);
#[cfg_attr(not(feature = "live"), allow(dead_code))]
const POLL_HIDDEN: Duration = Duration::from_secs(30);

/// A conversation's title: its first message's first line, shortened.
pub fn title_of(text: &str) -> String {
    const MOST: usize = 60;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= MOST {
        return line.to_owned();
    }
    let cut: String = line.chars().take(MOST - 1).collect();
    format!("{}…", cut.trim_end())
}

/// A conversation's identity in this window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConvId(pub u64);

/// What the window shows beside the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Conversation(ConvId),
    Models,
    /// Every measured model side by side, with the cloud ones for reference.
    Compare,
    Memory,
}

/// A turn under way.
pub enum Turn {
    /// A fixture's script.
    Script(Player),
    #[cfg(feature = "live")]
    Live(crate::live::Live),
}

pub struct Conversation {
    pub id: ConvId,
    pub title: String,
    /// When it last changed: the sidebar's order.
    pub updated: Millis,
    pub transcript: Transcript,
    pub draft: EditorContent,
    /// Where it was read to, for when it shows again.
    pub reading: Reading,
    /// Bumped by sending: the transcript goes to its end.
    pub sent: u64,
    /// The items and runs opened (by key).
    pub open: HashSet<u64>,
    pub act: bool,
    pub desktop: bool,
    pub turn: Option<Turn>,
    #[cfg(feature = "live")]
    pub session: Option<tasuke_agent::Session>,
}

impl Conversation {
    fn new(id: ConvId, at: Millis, act: bool, desktop: bool) -> Conversation {
        Conversation {
            id,
            title: String::new(),
            updated: at,
            transcript: Transcript::new(),
            draft: EditorContent::new(""),
            reading: Reading::default(),
            sent: 0,
            open: HashSet::new(),
            act,
            desktop,
            turn: None,
            #[cfg(feature = "live")]
            session: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.transcript.items.is_empty()
    }

    /// A call is waiting for the person.
    pub fn needs_you(&self) -> bool {
        self.transcript.pending_approval().is_some()
    }
}

/// The model the assistant uses, as the composer's chip shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub name: String,
    pub state: ModelState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelState {
    /// Loaded and answering.
    Ready,
    /// Unloaded after a while unused: it loads when something is sent.
    Asleep,
    Loading,
    /// Nothing downloaded to use.
    Missing,
    /// The model server isn't running (a turn starts it).
    Off,
    /// The router tried to load it and couldn't.
    Failed,
}

impl ModelState {
    pub fn words(self) -> &'static str {
        match self {
            ModelState::Ready => "ready",
            ModelState::Asleep => "asleep, loads when you send",
            ModelState::Loading => "loading…",
            ModelState::Missing => "not downloaded",
            ModelState::Off => "server stopped",
            ModelState::Failed => "couldn't load",
        }
    }
}

/// A download under way.
pub struct Downloading {
    pub id: ModelId,
    pub progress: Progress,
    /// For a fixture: when it started (the simulation's clock).
    simulated: bool,
    #[cfg(feature = "live")]
    handle: Option<tasuke_models::DownloadHandle>,
}

/// What the Models page knows.
pub struct Models {
    /// `None` while the hardware is being probed.
    pub hardware: Option<Hardware>,
    pub catalog: Catalog,
    pub prefs: Preferences,
    pub installs: Installs,
    pub in_use: Option<ModelId>,
    pub download: Option<Downloading>,
    /// The model server, in words.
    pub server: String,
    /// How long a model stays loaded unused before it sleeps, in minutes.
    pub idle_minutes: u32,
    /// Something went wrong with the last action (a download, a removal).
    pub problem: Option<String>,
    /// How the Compare page is sorted and filtered, and its selection.
    pub compare: crate::compare::View,
}

impl Models {
    pub fn install(&self, id: &ModelId) -> Install {
        match &self.download {
            Some(d) if &d.id == id => Install::Downloading(d.progress.clone()),
            _ => self.installs.get(id).cloned().unwrap_or(Install::Absent),
        }
    }

    /// Installs with the download under way folded in.
    pub fn installs_now(&self) -> Installs {
        let mut i = self.installs.clone();
        if let Some(d) = &self.download {
            i.insert(d.id.clone(), Install::Downloading(d.progress.clone()));
        }
        i
    }
}

/// First run: the hardware, a recommendation, its download, hello.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirstRun {
    Looking,
    Recommend,
    Downloading,
    Ready,
}

/// A note in Memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub created: Millis,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Show(Page),
    NewConversation,
    Edit(ConvId, EditorAction),
    Send(ConvId),
    /// A suggestion pressed: sent as written.
    Suggest(ConvId, String),
    Stop(ConvId),
    Retry(ConvId),
    Decide(ConvId, String, Decision),
    /// Open or close an item or run of the transcript.
    Toggle(ConvId, u64),
    Read(ConvId, Reading),
    Act(ConvId, bool),
    Desktop(ConvId, bool),
    /// A fixture's turn: its next event is due (for the player of this
    /// generation).
    Step(ConvId, u64),
    /// The agent woke the window: its turn has events.
    Woke(ConvId),
    Thumb(PathBuf, Thumb),
    OpenLink(String),
    Copy(String),
    /// The pointer came onto a suggestion, or left them.
    Hover(Option<&'static str>),
    /// Time to look at the model server again.
    Poll,
    /// What the look found.
    Polled(Box<crate::server::Polled>),
    /// The conversations and notes, read from disk after the window opened.
    #[cfg(feature = "live")]
    Loaded(Box<crate::live::Loaded>),
    /// The clock moved on: relative times say so.
    Tick,
    // Models.
    Hardware(Box<Hardware>),
    HideOrigin(String, bool),
    Download(ModelId),
    Pause,
    Resume,
    CancelDownload,
    /// A fixture's download moved on.
    DownloadTick,
    /// A real download's event.
    Downloaded(ModelId, DownloadNews),
    Use(ModelId),
    Remove(ModelId),
    Removed(ModelId, Result<(), String>),
    // Compare.
    /// Sort by the table's column, ascending or not.
    CompareSort(usize, bool),
    /// The row whose details show, by key.
    CompareSelect(Option<String>),
    CompareFitsOnly(bool),
    CompareCloud(bool),
    // Memory.
    Forget(String),
    // First run.
    FirstRunDownload,
    FirstRunDone,
    FirstRunSkip,
}

/// What a download said, as the window needs it.
#[derive(Clone, Debug)]
pub enum DownloadNews {
    Progress { done: Bytes, total: Bytes, rate: f64 },
    Checking,
    Paused,
    Resumed,
    Finished,
    Failed(String),
    Cancelled,
}

/// Where the window's data comes from.
pub enum Backend {
    /// Made-up data (`--fixture`): nothing is read or changed.
    Fixture,
    #[cfg(feature = "live")]
    Live(crate::live::Env),
}

impl Backend {
    pub fn is_fixture(&self) -> bool {
        matches!(self, Backend::Fixture)
    }
}

pub struct Tasuke {
    pub backend: Backend,
    clock: Clock,
    /// Fixtures keep their clock, moved on by the time since launch.
    fixed_clock: bool,
    launched: Instant,
    pub page: Page,
    pub conversations: Vec<Conversation>,
    next_id: u64,
    pub models: Models,
    pub notes: Vec<Note>,
    pub chip: Chip,
    /// The last finished turn's speed, in tokens a second.
    pub speed: Option<f32>,
    pub first_run: Option<FirstRun>,
    pub thumbs: HashMap<PathBuf, Thumb>,
    window: Option<WindowKey>,
    stream: Option<MsgSender<Msg>>,
    /// Pictures asked for before there was a stream to answer on.
    wanted: Vec<PathBuf>,
    /// The fixture launched, for its title in tests.
    pub fixture: Option<String>,
    /// The suggestion under the pointer.
    pub hovered: Option<&'static str>,
    /// The model server, polled: a look under way, when the last ended,
    /// and whether the desktop is showing the window (it looks less often
    /// when it isn't).
    polling: bool,
    last_poll: Option<Instant>,
    suspended: bool,
    /// The window's title as last set.
    title: String,
    /// Conversations still being read from disk.
    pub loading: bool,
}

/// The fixtures `--fixture` takes.
pub const FIXTURES: &[&str] = &[
    "home",
    "diagnose",
    "root-approval",
    "day-theme",
    "checkup",
    "server-down",
    "models-8gb",
    "models-32gb",
    "models-unified",
    "compare",
    "compare-8gb",
    "downloading",
    "first-run",
    "memory",
];

impl Tasuke {
    fn empty(backend: Backend, clock: Clock, fixed_clock: bool) -> Tasuke {
        Tasuke {
            backend,
            clock,
            fixed_clock,
            launched: Instant::now(),
            page: Page::Conversation(ConvId(0)),
            conversations: Vec::new(),
            next_id: 0,
            models: Models {
                hardware: None,
                catalog: Catalog::load_default(),
                prefs: Preferences::default(),
                installs: Installs::new(),
                in_use: None,
                download: None,
                server: String::new(),
                idle_minutes: 10,
                problem: None,
                compare: crate::compare::View::default(),
            },
            notes: Vec::new(),
            chip: Chip { name: String::new(), state: ModelState::Missing },
            speed: None,
            first_run: None,
            thumbs: HashMap::new(),
            window: None,
            stream: None,
            wanted: Vec::new(),
            fixture: None,
            hovered: None,
            polling: false,
            last_poll: None,
            suspended: false,
            title: String::new(),
            loading: false,
        }
    }

    /// The window on a fixture: made-up machines, conversations and notes,
    /// the real catalog.
    pub fn fixture(name: &str) -> Option<Tasuke> {
        if !FIXTURES.contains(&name) {
            return None;
        }
        let mut t = Tasuke::empty(Backend::Fixture, fixture::CLOCK, true);
        t.fixture = Some(name.to_owned());
        let hw = match name {
            "models-8gb" | "compare-8gb" | "first-run" | "downloading" => fixture::laptop_8gb(),
            "models-unified" => fixture::apu_128gb(),
            _ => fixture::desktop_32gb(),
        };
        t.models.server = format!("Running · a model unloads after {} minutes unused", t.models.idle_minutes);
        t.notes =
            fixture::notes().into_iter().map(|(id, text, created)| Note { id, text: text.into(), created }).collect();
        // What's downloaded on each machine.
        let installed: &[&str] = match name {
            "models-8gb" | "compare-8gb" => &["gemma-4-12b"],
            "models-unified" => &["gemma-4-31b"],
            "first-run" | "downloading" => &[],
            _ => &["gemma-4-31b", "gpt-oss-20b"],
        };
        for id in installed {
            t.models.installs.insert(ModelId::new(*id), Install::Installed);
        }
        if name == "models-unified" {
            let total =
                t.models.catalog.get(&ModelId::new("nemotron-3.5-bolt")).map(|e| e.download_size()).unwrap_or_default();
            t.models
                .installs
                .insert(ModelId::new("nemotron-3.5-bolt"), Install::Partial { have: total.scale(0.38), total });
        }
        t.models.in_use = installed.first().map(|id| ModelId::new(*id));
        t.models.hardware = Some(hw);
        t.chip = Chip {
            name: t
                .models
                .in_use
                .as_ref()
                .and_then(|id| t.models.catalog.get(id))
                .map(|e| e.name.clone())
                .unwrap_or_default(),
            state: if t.models.in_use.is_some() { ModelState::Asleep } else { ModelState::Missing },
        };
        // The earlier conversations, played to where they stopped, and the
        // one the check-up and server-down fixtures add.
        let n = fixture::CLOCK.now;
        let mut all = fixture::conversations();
        match name {
            "checkup" => all.push(fixture::Conversation {
                title: "Give my computer a check-up",
                at: n - 40_000,
                text: "Give my computer a check-up",
                script: fixture::checkup(),
                act: true,
                desktop: false,
            }),
            "server-down" => all.push(fixture::Conversation {
                title: "Is my graphics card running hot?",
                at: n - 30_000,
                text: "Is my graphics card running hot?",
                script: fixture::server_down(),
                act: true,
                desktop: false,
            }),
            _ => {}
        }
        for c in all {
            let id = t.add_conversation(c.at, c.act, c.desktop);
            let conv = t.conversation_mut(id).expect("just added");
            conv.title = c.title.to_owned();
            conv.transcript.user(c.text, c.at);
            let mut player = Player::new(c.script);
            let mut at = c.at;
            loop {
                while let Some(d) = player.next_in() {
                    at += d.as_millis() as Millis;
                    if let Some(e) = player.advance() {
                        conv.transcript.apply(e, at);
                    }
                }
                // Earlier conversations were answered Run; the latest two
                // wait for the person.
                let pending = conv.transcript.pending_approval().map(|p| p.call_id.clone());
                match pending {
                    Some(call) if c.at < fixture::CLOCK.now - 10 * 60_000 => {
                        conv.transcript.decide(&call, Decision::Allow);
                        player.decide(&call, &Decision::Allow);
                        at += 1_500;
                    }
                    _ => break,
                }
            }
            conv.updated = at.min(fixture::CLOCK.now);
            if !player.is_done() {
                conv.turn = Some(Turn::Script(player));
            }
            if let Some(tps) = conv.transcript.decode_tps {
                t.speed = Some(tps);
            }
        }
        let by_title = |t: &Tasuke, title: &str| t.conversations.iter().find(|c| c.title == title).map(|c| c.id);
        t.page = match name {
            "diagnose" => {
                t.chip.state = ModelState::Ready;
                Page::Conversation(by_title(&t, "Why is my disk filling up?").expect("in the fixture"))
            }
            "root-approval" => {
                t.chip.state = ModelState::Ready;
                Page::Conversation(by_title(&t, "Clean up old system versions").expect("in the fixture"))
            }
            "day-theme" => Page::Conversation(by_title(&t, "Switch to the Day theme").expect("in the fixture")),
            "checkup" => {
                t.chip.state = ModelState::Ready;
                Page::Conversation(by_title(&t, "Give my computer a check-up").expect("in the fixture"))
            }
            "server-down" => {
                t.chip.state = ModelState::Off;
                t.models.server = "Stopped · it starts when you send something".into();
                Page::Conversation(by_title(&t, "Is my graphics card running hot?").expect("in the fixture"))
            }
            "models-8gb" | "models-32gb" | "models-unified" => Page::Models,
            "compare" => {
                t.models.compare.selected = Some("gemma-4-31b".into());
                Page::Compare
            }
            "compare-8gb" => {
                t.models.compare.fits_only = true;
                t.models.compare.selected = Some("gemma-4-26b-a4b".into());
                Page::Compare
            }
            "memory" => Page::Memory,
            _ => Page::Conversation(t.new_conversation_id()),
        };
        // Pending fixtures stay busy (their turns wait on a card): the
        // others' players are done.
        if name == "first-run" || name == "downloading" {
            t.conversations.clear();
            t.notes.clear();
            t.speed = None;
            let id = t.new_conversation_id();
            t.page = Page::Conversation(id);
            t.first_run = Some(FirstRun::Recommend);
            if name == "downloading" {
                t.first_run = Some(FirstRun::Downloading);
                if let Some(rec) = t.first_choice() {
                    let total = t.models.catalog.get(&rec).map(|e| e.download_size()).unwrap_or_default();
                    t.models.download = Some(Downloading {
                        id: rec,
                        progress: Progress {
                            done: total.scale(0.42),
                            total,
                            rate: Some(38.6 * 1024.0 * 1024.0),
                            paused: false,
                            checking: false,
                        },
                        simulated: true,
                        #[cfg(feature = "live")]
                        handle: None,
                    });
                }
            }
        }
        if name == "home" {
            t.chip.state = ModelState::Asleep;
        }
        Some(t)
    }

    /// The window on this computer.
    #[cfg(feature = "live")]
    pub fn live() -> Tasuke {
        let env = crate::live::Env::new();
        let mut t = Tasuke::empty(Backend::Live(env), Clock::system(), false);
        crate::live::load(&mut t);
        t
    }

    pub fn clock(&self) -> Clock {
        if self.fixed_clock {
            Clock { now: self.clock.now + self.launched.elapsed().as_millis() as Millis, ..self.clock }
        } else {
            self.clock
        }
    }

    pub fn now(&self) -> Millis {
        if self.fixed_clock { self.clock().now } else { Clock::system().now }
    }

    pub(crate) fn add_conversation(&mut self, at: Millis, act: bool, desktop: bool) -> ConvId {
        self.next_id += 1;
        let id = ConvId(self.next_id);
        self.conversations.push(Conversation::new(id, at, act, desktop));
        id
    }

    /// The empty conversation New shows: the one there is, or a new one.
    pub(crate) fn new_conversation_id(&mut self) -> ConvId {
        if let Some(c) = self.conversations.iter().find(|c| c.is_empty() && c.turn.is_none()) {
            return c.id;
        }
        // A new conversation takes the modes of the last one used.
        let (act, desktop) = self.recent().first().map(|c| (c.act, c.desktop)).unwrap_or((true, false));
        let now = self.now();
        self.add_conversation(now, act, desktop)
    }

    pub fn conversation(&self, id: ConvId) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.id == id)
    }

    pub fn conversation_mut(&mut self, id: ConvId) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|c| c.id == id)
    }

    /// The conversations with something in them, the most recent first.
    pub fn recent(&self) -> Vec<&Conversation> {
        let mut v: Vec<&Conversation> = self.conversations.iter().filter(|c| !c.is_empty()).collect();
        v.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.id.cmp(&a.id)));
        v
    }

    /// The model first run suggests.
    pub fn first_choice(&self) -> Option<ModelId> {
        let hw = self.models.hardware.as_ref()?;
        models::first_choice(hw, &self.models.catalog, &self.models.prefs).map(|r| r.id)
    }

    /// The picture at `path`, shrunk: asked for once, then kept.
    pub fn thumb(&self, path: &PathBuf) -> Option<&Thumb> {
        self.thumbs.get(path)
    }

    fn want_thumbs(&mut self, cx: &mut Cx<Msg>) {
        let mut paths: Vec<PathBuf> = std::mem::take(&mut self.wanted);
        for c in &self.conversations {
            for i in &c.transcript.items {
                if let crate::transcript::Body::Tool(t) = &i.body
                    && let Some(p) = t.output.as_ref().and_then(|o| o.image.clone())
                    && !self.thumbs.contains_key(&p)
                    && !paths.contains(&p)
                {
                    paths.push(p);
                }
            }
        }
        for p in paths {
            self.thumbs.insert(p.clone(), Thumb::Loading);
            let path = p.clone();
            cx.spawn(move || Msg::Thumb(path.clone(), thumb::make(&path)));
        }
    }

    fn schedule(&mut self, id: ConvId, cx: &mut Cx<Msg>) {
        let Some(c) = self.conversation_mut(id) else { return };
        if let Some(Turn::Script(p)) = &c.turn
            && let Some(d) = p.next_in()
        {
            cx.after(d, Msg::Step(id, p.generation));
        }
    }

    fn apply(&mut self, id: ConvId, events: Vec<Event>, cx: &mut Cx<Msg>) {
        let now = self.now();
        let Some(c) = self.conversation_mut(id) else { return };
        let mut finished = false;
        // The model answering (thinking, writing, asking for a tool) means
        // it's loaded.
        let mut answering = false;
        let mut starting = false;
        for e in events {
            if let Event::Notice { text, .. } = &e
                && text.starts_with("Starting the model server")
            {
                starting = true;
            }
            if matches!(e, Event::TurnFinished { .. } | Event::Error { .. }) {
                finished = true;
            }
            if matches!(e, Event::ReasoningDelta { .. } | Event::TextDelta { .. } | Event::ToolCall { .. }) {
                answering = true;
            }
            c.transcript.apply(e, now);
        }
        c.updated = now;
        let memory_changed = std::mem::take(&mut c.transcript.memory_changed);
        let tps = c.transcript.decode_tps;
        if finished {
            c.turn = None;
        }
        if let Some(tps) = tps {
            self.speed = Some(tps);
        }
        if (finished || answering) && self.chip.state == ModelState::Loading {
            self.chip.state = ModelState::Ready;
        } else if starting && !matches!(self.chip.state, ModelState::Missing) {
            self.chip.state = ModelState::Loading;
        }
        if memory_changed {
            self.reload_notes(cx);
        }
        self.want_thumbs(cx);
    }

    fn reload_notes(&mut self, cx: &mut Cx<Msg>) {
        #[cfg(feature = "live")]
        if let Backend::Live(env) = &self.backend {
            self.notes = crate::live::notes(env);
        }
        let _ = cx;
    }

    fn send(&mut self, id: ConvId, text: String, cx: &mut Cx<Msg>) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let now = self.now();
        let fixture = self.backend.is_fixture();
        let missing = self.chip.state == ModelState::Missing && !fixture;
        let Some(c) = self.conversation_mut(id) else { return };
        if c.turn.is_some() || c.transcript.busy {
            return;
        }
        if c.title.is_empty() {
            c.title = title_of(&text);
        }
        c.transcript.user(&text, now);
        c.draft = EditorContent::new("");
        c.sent += 1;
        c.updated = now;
        if missing {
            c.transcript.apply(
                Event::Notice {
                    level: Level::Warning,
                    text: "No model is downloaded yet. Choose one in Models, and it will answer.".into(),
                },
                now,
            );
            return;
        }
        match &self.backend {
            Backend::Fixture => {
                let p = Player::new(fixture::script_for(&text));
                if let Some(c) = self.conversation_mut(id) {
                    c.turn = Some(Turn::Script(p));
                }
                if self.chip.state == ModelState::Asleep {
                    self.chip.state = ModelState::Loading;
                }
                self.schedule(id, cx);
            }
            #[cfg(feature = "live")]
            Backend::Live(_) => crate::live::start(self, id, &text, cx),
        }
    }

    /// Look at the model server, unless a look is under way or one was
    /// made lately: every 5 seconds while the window shows, every 30 while
    /// the desktop isn't showing it. Fixtures have no server.
    fn poll(&mut self, cx: &mut Cx<Msg>) {
        #[cfg(feature = "live")]
        if let Backend::Live(env) = &self.backend {
            let every = if self.suspended { POLL_HIDDEN } else { POLL_SHOWN };
            if self.polling || self.last_poll.is_some_and(|t| t.elapsed() + Duration::from_millis(500) < every) {
                return;
            }
            self.polling = true;
            let server = env.server();
            cx.spawn(move || Msg::Polled(Box::new(crate::server::poll(&server))));
        }
        let _ = cx;
    }

    pub fn set_stream(&mut self, s: MsgSender<Msg>) {
        self.stream = Some(s);
    }

    pub fn stream(&self) -> Option<&MsgSender<Msg>> {
        self.stream.as_ref()
    }

    fn start_download(&mut self, id: ModelId, cx: &mut Cx<Msg>) {
        if self.models.download.is_some() {
            return;
        }
        let Some(entry) = self.models.catalog.get(&id).cloned() else { return };
        let total = entry.download_size();
        let have = match self.models.installs.get(&id) {
            Some(Install::Partial { have, .. }) => *have,
            _ => Bytes::ZERO,
        };
        self.models.problem = None;
        let progress = Progress { done: have, total, rate: None, paused: false, checking: have > Bytes::ZERO };
        match &self.backend {
            Backend::Fixture => {
                self.models.download = Some(Downloading {
                    id,
                    progress,
                    simulated: true,
                    #[cfg(feature = "live")]
                    handle: None,
                });
                cx.after(Duration::from_millis(250), Msg::DownloadTick);
            }
            #[cfg(feature = "live")]
            Backend::Live(env) => {
                let handle = crate::live::download(env, &entry, self.stream.clone(), id.clone());
                self.models.download = Some(Downloading { id, progress, simulated: false, handle: Some(handle) });
            }
        }
    }

    fn download_finished(&mut self, id: &ModelId) {
        self.models.download = None;
        self.models.installs.insert(id.clone(), Install::Installed);
        if self.models.in_use.is_none() {
            self.use_model(id.clone());
        }
        if self.first_run == Some(FirstRun::Downloading) {
            self.first_run = Some(FirstRun::Ready);
        }
    }

    fn use_model(&mut self, id: ModelId) {
        let name = self.models.catalog.get(&id).map(|e| e.name.clone()).unwrap_or_else(|| id.0.clone());
        self.chip = Chip { name, state: ModelState::Asleep };
        #[cfg(feature = "live")]
        if let Backend::Live(env) = &self.backend {
            crate::live::save_model(env, &id);
        }
        self.models.in_use = Some(id);
    }
}

impl Program for Tasuke {
    type Msg = Msg;

    fn info(&self) -> AppInfo {
        AppInfo::new(APP_ID, "Tasuke", "Assistant").icon(Icon::MessageSquare)
    }

    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let w = cx.open_window(WindowSpec::new("Tasuke").size(1180.0, 780.0).min_size(760.0, 520.0));
        self.window = Some(w);
        self.stream = Some(cx.stream());
        self.want_thumbs(cx);
        // Relative times in the sidebar and the notes.
        cx.every(Duration::from_secs(30), Msg::Tick);
        if self.models.download.as_ref().is_some_and(|d| d.simulated) {
            cx.after(Duration::from_millis(250), Msg::DownloadTick);
        }
        #[cfg(feature = "live")]
        if let Backend::Live(_) = &self.backend {
            crate::live::launched(self, cx);
            cx.every(POLL_SHOWN, Msg::Poll);
            cx.send(Msg::Poll);
        }
    }

    fn menus(&self) -> MenuBar<Msg> {
        let current = match self.page {
            Page::Conversation(id) => Some(id),
            _ => None,
        };
        let busy = current.and_then(|id| self.conversation(id)).is_some_and(|c| c.turn.is_some());
        let mut file = Menu::new("File")
            .item(MenuItem::new("New Conversation", Msg::NewConversation).shortcut(Shortcut::cmd('N')));
        if let Some(id) = current {
            file =
                file.separator().item(MenuItem::new("Stop", Msg::Stop(id)).shortcut(Shortcut::cmd('.')).enabled(busy));
        }
        let go = Menu::new("Go")
            .item(MenuItem::new("Models", Msg::Show(Page::Models)).shortcut(Shortcut::cmd('1').shift()))
            .item(MenuItem::new("Memory", Msg::Show(Page::Memory)).shortcut(Shortcut::cmd('2').shift()))
            .item(MenuItem::new("Compare Models", Msg::Show(Page::Compare)).shortcut(Shortcut::cmd('3').shift()));
        MenuBar::standard(&self.info()).with_file_menu(file).with_menu(go)
    }

    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        self.handle(msg, cx);
        let title = self.window_title();
        if title != self.title
            && let Some(w) = self.window
        {
            cx.set_title(w, title.clone());
            self.title = title;
        }
    }

    fn window_state(&mut self, _window: WindowKey, state: WindowState, cx: &mut Cx<Msg>) {
        let shown_again = self.suspended && !state.suspended;
        self.suspended = state.suspended;
        if shown_again {
            cx.send(Msg::Poll);
        }
    }

    fn view(&self, _window: WindowKey) -> Element<Msg> {
        crate::ui::view(self)
    }
}

impl Tasuke {
    /// "Why is my disk filling up? — Tasuke", "Models — Tasuke".
    pub fn window_title(&self) -> String {
        let page = match (self.first_run.is_some(), self.page) {
            (true, _) => None,
            (_, Page::Models) => Some("Models".to_owned()),
            (_, Page::Memory) => Some("Memory".to_owned()),
            (_, Page::Compare) => Some("Compare Models".to_owned()),
            (_, Page::Conversation(id)) => self.conversation(id).map(|c| c.title.clone()).filter(|t| !t.is_empty()),
        };
        match page {
            Some(p) => format!("{p} — Tasuke"),
            None => "Tasuke".to_owned(),
        }
    }

    fn handle(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Show(page) => self.page = page,
            Msg::NewConversation => {
                let id = self.new_conversation_id();
                self.page = Page::Conversation(id);
                cx.focus("composer");
            }
            Msg::Edit(id, action) => {
                if let Some(c) = self.conversation_mut(id) {
                    c.draft.perform(action);
                }
            }
            Msg::Send(id) => {
                let text = self.conversation(id).map(|c| c.draft.text()).unwrap_or_default();
                self.send(id, text, cx);
            }
            Msg::Suggest(id, text) => self.send(id, text, cx),
            Msg::Stop(id) => {
                let now = self.now();
                let Some(c) = self.conversation_mut(id) else { return };
                match c.turn.as_mut() {
                    Some(Turn::Script(p)) => {
                        let events = p.cancel();
                        for e in events {
                            c.transcript.apply(e, now);
                        }
                        c.turn = None;
                    }
                    #[cfg(feature = "live")]
                    Some(Turn::Live(l)) => l.cancel(),
                    None => {}
                }
            }
            Msg::Retry(id) => {
                let Some(text) = self.conversation(id).and_then(|c| c.transcript.last_prompt()).map(str::to_owned)
                else {
                    return;
                };
                // The message and what went wrong stay; the turn runs again
                // (once: a turn under way isn't started twice).
                if self.conversation(id).is_none_or(|c| c.turn.is_some()) {
                    return;
                }
                if self.chip.state == ModelState::Asleep || self.chip.state == ModelState::Off {
                    self.chip.state = ModelState::Loading;
                }
                match &self.backend {
                    Backend::Fixture => {
                        if let Some(c) = self.conversation_mut(id) {
                            c.turn = Some(Turn::Script(Player::new(fixture::script_for(&text))));
                        }
                        self.schedule(id, cx);
                    }
                    #[cfg(feature = "live")]
                    Backend::Live(_) => crate::live::start(self, id, &text, cx),
                }
            }
            Msg::Decide(id, call, decision) => {
                let Some(c) = self.conversation_mut(id) else { return };
                let Some(sent) = c.transcript.decide(&call, decision) else { return };
                match c.turn.as_mut() {
                    Some(Turn::Script(p)) => {
                        p.decide(&call, &sent);
                        self.schedule(id, cx);
                    }
                    #[cfg(feature = "live")]
                    Some(Turn::Live(l)) => l.decide(&call, sent),
                    None => {}
                }
            }
            Msg::Toggle(id, key) => {
                if let Some(c) = self.conversation_mut(id)
                    && !c.open.remove(&key)
                {
                    c.open.insert(key);
                }
            }
            Msg::Read(id, r) => {
                if let Some(c) = self.conversation_mut(id) {
                    c.reading = r;
                }
            }
            Msg::Act(id, on) => {
                if let Some(c) = self.conversation_mut(id) {
                    c.act = on;
                }
            }
            Msg::Desktop(id, on) => {
                if let Some(c) = self.conversation_mut(id) {
                    c.desktop = on;
                }
            }
            Msg::Step(id, generation) => {
                let Some(c) = self.conversation_mut(id) else { return };
                let Some(Turn::Script(p)) = c.turn.as_mut() else { return };
                if p.generation != generation {
                    return;
                }
                let events: Vec<Event> = p.advance().into_iter().collect();
                self.apply(id, events, cx);
                self.schedule(id, cx);
            }
            Msg::Woke(id) => {
                #[cfg(feature = "live")]
                {
                    let events = match self.conversation(id).and_then(|c| c.turn.as_ref()) {
                        Some(Turn::Live(l)) => l.drain(),
                        _ => Vec::new(),
                    };
                    if !events.is_empty() {
                        self.apply(id, events, cx);
                    }
                }
                let _ = id;
            }
            Msg::Thumb(path, thumb) => {
                self.thumbs.insert(path, thumb);
            }
            Msg::OpenLink(url) => cx.open_url(&url),
            Msg::Hover(which) => self.hovered = which,
            Msg::Poll => self.poll(cx),
            Msg::Polled(p) => {
                self.polling = false;
                self.last_poll = Some(Instant::now());
                let in_use = self.models.in_use.clone();
                let installed = in_use.as_ref().is_some_and(|id| self.models.install(id) == Install::Installed);
                let busy = self.conversations.iter().any(|c| c.turn.is_some());
                self.chip.state = crate::server::chip_state(&p, in_use.as_ref(), installed, busy, self.chip.state);
                let name = self.chip.name.clone();
                self.models.server =
                    crate::server::line(&p, in_use.as_ref().map(|id| (id, name.as_str())), self.models.idle_minutes);
            }
            #[cfg(feature = "live")]
            Msg::Loaded(l) => crate::live::loaded(self, *l, cx),
            Msg::Copy(text) => cx.set_clipboard_text(&text),
            Msg::Tick => {
                if !self.fixed_clock {
                    self.clock = Clock::system();
                }
            }
            Msg::Hardware(hw) => {
                self.models.hardware = Some(*hw);
                if self.first_run == Some(FirstRun::Looking) {
                    self.first_run = Some(FirstRun::Recommend);
                }
            }
            Msg::HideOrigin(origin, hide) => {
                let list = &mut self.models.prefs.exclude_lineage;
                list.retain(|o| !o.eq_ignore_ascii_case(&origin));
                if hide {
                    list.push(origin);
                }
                #[cfg(feature = "live")]
                if let Backend::Live(env) = &self.backend {
                    crate::live::save_prefs(env, &self.models.prefs);
                }
            }
            Msg::CompareSort(column, ascending) => {
                if let Some(&c) = crate::compare::Column::ALL.get(column) {
                    // A new column sorts best first; the same one again flips.
                    let view = &mut self.models.compare;
                    view.ascending = if view.sort == c { ascending } else { c.ascending_first() };
                    view.sort = c;
                }
            }
            Msg::CompareSelect(key) => self.models.compare.selected = key,
            Msg::CompareFitsOnly(on) => self.models.compare.fits_only = on,
            Msg::CompareCloud(on) => self.models.compare.cloud = on,
            Msg::Download(id) => self.start_download(id, cx),
            Msg::Pause => {
                if let Some(d) = &mut self.models.download {
                    d.progress.paused = true;
                    #[cfg(feature = "live")]
                    if let Some(h) = &d.handle {
                        h.pause();
                    }
                }
            }
            Msg::Resume => {
                if let Some(d) = &mut self.models.download {
                    d.progress.paused = false;
                    #[cfg(feature = "live")]
                    if let Some(h) = &d.handle {
                        h.resume();
                    }
                    if d.simulated {
                        cx.after(Duration::from_millis(250), Msg::DownloadTick);
                    }
                }
            }
            Msg::CancelDownload => {
                if let Some(d) = self.models.download.take() {
                    #[cfg(feature = "live")]
                    if let Some(h) = &d.handle {
                        h.cancel();
                    }
                    let p = d.progress;
                    if p.done > Bytes::ZERO {
                        self.models.installs.insert(d.id, Install::Partial { have: p.done, total: p.total });
                    }
                    if self.first_run == Some(FirstRun::Downloading) {
                        self.first_run = Some(FirstRun::Recommend);
                    }
                }
            }
            Msg::DownloadTick => {
                let Some(d) = self.models.download.as_mut().filter(|d| d.simulated) else { return };
                if d.progress.paused {
                    return;
                }
                let p = &mut d.progress;
                p.checking = false;
                // A steady connection, a little uneven.
                let wobble = 1.0 + 0.06 * ((p.done.0 / (37 << 20)) % 5) as f64 / 4.0;
                let rate = 38.6 * 1024.0 * 1024.0 * wobble;
                p.rate = Some(rate);
                p.done = Bytes((p.done.0 + (rate / 4.0) as u64).min(p.total.0));
                if p.done >= p.total {
                    let id = d.id.clone();
                    self.download_finished(&id);
                } else {
                    cx.after(Duration::from_millis(250), Msg::DownloadTick);
                }
            }
            Msg::Downloaded(id, news) => {
                let Some(d) = self.models.download.as_mut().filter(|d| d.id == id) else { return };
                match news {
                    DownloadNews::Progress { done, total, rate } => {
                        d.progress.done = done;
                        d.progress.total = total;
                        d.progress.rate = (rate > 0.0).then_some(rate);
                        d.progress.checking = false;
                    }
                    DownloadNews::Checking => d.progress.checking = true,
                    DownloadNews::Paused => d.progress.paused = true,
                    DownloadNews::Resumed => d.progress.paused = false,
                    DownloadNews::Finished => self.download_finished(&id),
                    DownloadNews::Failed(why) => {
                        let p = self.models.download.take().map(|d| d.progress);
                        if let Some(p) = p.filter(|p| p.done > Bytes::ZERO) {
                            self.models.installs.insert(id, Install::Partial { have: p.done, total: p.total });
                        }
                        self.models.problem = Some(format!("The download stopped: {why}"));
                        if self.first_run == Some(FirstRun::Downloading) {
                            self.first_run = Some(FirstRun::Recommend);
                        }
                    }
                    DownloadNews::Cancelled => {
                        self.models.download = None;
                    }
                }
            }
            Msg::Use(id) => self.use_model(id),
            Msg::Remove(id) => match &self.backend {
                Backend::Fixture => {
                    self.models.installs.insert(id.clone(), Install::Absent);
                    if self.models.in_use.as_ref() == Some(&id) {
                        self.models.in_use = None;
                        self.chip.state = ModelState::Missing;
                    }
                }
                #[cfg(feature = "live")]
                Backend::Live(env) => {
                    let store = env.store();
                    let target = id.clone();
                    cx.spawn(move || Msg::Removed(target.clone(), store.remove(&target).map_err(|e| e.to_string())));
                }
            },
            Msg::Removed(id, result) => match result {
                Ok(()) => {
                    self.models.installs.insert(id.clone(), Install::Absent);
                    if self.models.in_use.as_ref() == Some(&id) {
                        self.models.in_use = None;
                        self.chip.state = ModelState::Missing;
                    }
                }
                Err(e) => self.models.problem = Some(format!("It couldn't be removed: {e}")),
            },
            Msg::Forget(note) => {
                self.notes.retain(|n| n.id != note);
                #[cfg(feature = "live")]
                if let Backend::Live(env) = &self.backend {
                    crate::live::forget(env, &note);
                }
            }
            Msg::FirstRunDownload => {
                if let Some(id) = self.first_choice() {
                    self.first_run = Some(FirstRun::Downloading);
                    self.start_download(id, cx);
                }
            }
            Msg::FirstRunDone | Msg::FirstRunSkip => {
                self.first_run = None;
                let id = self.new_conversation_id();
                self.page = Page::Conversation(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Body;

    #[test]
    fn every_fixture_builds() {
        for name in FIXTURES {
            let t = Tasuke::fixture(name).unwrap_or_else(|| panic!("{name}"));
            assert!(t.models.hardware.is_some(), "{name}");
        }
        assert!(Tasuke::fixture("nonsense").is_none());
    }

    #[test]
    fn the_diagnosis_waits_on_its_card_and_earlier_ones_were_answered() {
        let t = Tasuke::fixture("diagnose").unwrap();
        let Page::Conversation(id) = t.page else { panic!() };
        let c = t.conversation(id).unwrap();
        assert_eq!(c.title, "Why is my disk filling up?");
        assert!(c.needs_you(), "the card waits");
        assert!(c.transcript.busy);
        let sound = t.conversations.iter().find(|c| c.title == "My sound stopped working").unwrap();
        assert!(!sound.needs_you() && !sound.transcript.busy);
        assert!(matches!(sound.transcript.items.last().map(|i| &i.body), Some(Body::Answer(_))));
        // The sidebar's order: the most recent first.
        let titles: Vec<&str> = t.recent().iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles[..2], ["Clean up old system versions", "Why is my disk filling up?"]);
        assert_eq!(*titles.last().unwrap(), "What's a swap file?");
    }

    #[test]
    fn new_reuses_an_empty_conversation_and_keeps_modes() {
        let mut t = Tasuke::fixture("home").unwrap();
        let Page::Conversation(first) = t.page else { panic!() };
        assert!(t.conversation(first).unwrap().is_empty());
        assert_eq!(t.new_conversation_id(), first, "one empty conversation at a time");
        assert_eq!(t.recent().len(), 6, "empty ones aren't listed");
    }
}
