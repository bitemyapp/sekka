//! The window on this computer: Tasuke's agent (`tasuke-agent`) runs each
//! turn on its own thread and wakes the window through the handle's waker;
//! its events are mapped into the window's ([`map`]). Conversations and
//! memory are read from the agent's files, models from `tasuke-models`.
//!
//! What the window assumes of the agent: every turn ends with
//! `TurnFinished` (or `Error`), a call that asks waits for `decide` with
//! its id, and `cancel` ends a turn that is waiting or running.

use std::sync::mpsc::TryRecvError;

use aizome_kit::MsgSender;
use aizome_kit::prelude::*;
use tasuke_agent as agent;
use tasuke_models::{Catalog, Config, DownloadEvent, Entry, Hardware, ModelId, Paths, Preferences, Store};

use crate::agent::{Approval, Decision, Event, FinishReason, Level, Outcome, ToolCall, ToolOutput, Usage};
use crate::app::{Backend, Chip, ConvId, DownloadNews, ModelState, Msg, Note, Tasuke, Turn};
use crate::models::{Install, install_of};
use crate::transcript::{Millis, Transcript};

/// Where Tasuke's files are.
pub struct Env {
    pub data: agent::DataDir,
    pub paths: Paths,
    /// The model server, as the settings place it.
    server: tasuke_models::Server,
}

impl Env {
    pub fn new() -> Env {
        let paths = Paths::from_env();
        let config = Config::load(&paths).unwrap_or_default();
        Env { data: agent::DataDir::user(), server: tasuke_models::Server::new(&paths, &config), paths }
    }

    pub fn server(&self) -> tasuke_models::Server {
        self.server.clone()
    }

    pub fn store(&self) -> Store {
        Store::from_paths(&self.paths)
    }

    fn config(&self) -> Config {
        Config::load(&self.paths).unwrap_or_default()
    }
}

impl Default for Env {
    fn default() -> Env {
        Env::new()
    }
}

/// A turn of the real agent.
pub struct Live {
    handle: agent::TurnHandle,
}

impl Live {
    /// The events that arrived, in the window's terms.
    pub fn drain(&self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            match self.handle.events().try_recv() {
                Ok(e) => out.push(map(e)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // The turn's thread ended; a turn that didn't say so
                    // still ends.
                    if !out.iter().any(|e| matches!(e, Event::TurnFinished { .. } | Event::Error { .. }))
                        && self.handle.is_finished()
                    {
                        out.push(Event::TurnFinished {
                            reason: FinishReason::Failed,
                            usage: Usage::default(),
                            decode_tps: None,
                        });
                    }
                    break;
                }
            }
        }
        out
    }

    pub fn decide(&self, call: &str, d: Decision) {
        let d = match d {
            Decision::Allow => agent::Decision::Allow,
            Decision::Deny { note } => agent::Decision::Deny { note },
        };
        self.handle.decide(&agent::CallId::new(call), d);
    }

    pub fn cancel(&self) {
        self.handle.cancel();
    }
}

/// An agent event as the window has it.
pub fn map(e: agent::Event) -> Event {
    match e {
        agent::Event::TurnStarted { turn } => Event::TurnStarted { turn: turn.0 },
        agent::Event::ReasoningDelta { text } => Event::ReasoningDelta { text },
        agent::Event::TextDelta { text } => Event::TextDelta { text },
        agent::Event::ToolCall { id, call, needs } => {
            // A call the agent couldn't read is shown as such, naming the
            // tool the model asked for.
            let name = match &call {
                agent::ToolCall::Invalid { .. } => "invalid".to_owned(),
                c => c.tool_name().to_owned(),
            };
            let args = serde_json::to_value(&call).unwrap_or_default();
            let needs = match needs {
                agent::Approval::None => Approval::None,
                agent::Approval::Ask { reason } => Approval::Ask { reason },
                agent::Approval::AskRoot { reason } => Approval::AskRoot { reason },
                agent::Approval::Refused { reason } => Approval::Refused { reason },
            };
            Event::ToolCall { id: id.as_str().to_owned(), call: ToolCall::new(&name, args), needs }
        }
        agent::Event::ToolStarted { id } => Event::ToolStarted { id: id.as_str().to_owned() },
        agent::Event::ToolOutput { id, output } => {
            let outcome = match output.status {
                agent::ToolStatus::Done => Outcome::Ran,
                agent::ToolStatus::Failed => Outcome::Failed,
                agent::ToolStatus::Denied => Outcome::Denied,
                agent::ToolStatus::Refused => Outcome::Refused,
                agent::ToolStatus::TimedOut => Outcome::TimedOut,
                agent::ToolStatus::Cancelled => Outcome::Cancelled,
            };
            Event::ToolOutput {
                id: id.as_str().to_owned(),
                output: ToolOutput {
                    outcome,
                    text: output.text,
                    truncated: output.truncated,
                    exit: output.exit_code,
                    image: output.image,
                },
            }
        }
        agent::Event::MemoryChanged => Event::MemoryChanged,
        agent::Event::Notice { level, text } => Event::Notice {
            level: match level {
                agent::Level::Info => Level::Info,
                agent::Level::Warning => Level::Warning,
            },
            text,
        },
        agent::Event::TurnFinished { reason, usage, decode_tps } => Event::TurnFinished {
            reason: match reason {
                agent::FinishReason::Answered => FinishReason::Done,
                agent::FinishReason::Length => FinishReason::Length,
                agent::FinishReason::StepBudget => FinishReason::StepBudget,
                agent::FinishReason::TimeLimit => FinishReason::TimeLimit,
                agent::FinishReason::Cancelled => FinishReason::Cancelled,
                agent::FinishReason::Failed => FinishReason::Failed,
            },
            usage: Usage { prompt_tokens: usage.prompt_tokens.0, completion_tokens: usage.completion_tokens.0 },
            decode_tps,
        },
        agent::Event::Error { text, retryable } => Event::Error { text, retryable },
    }
}

fn millis(t: agent::Timestamp) -> Millis {
    t.0 as Millis * 1000
}

/// The settings and which models are downloaded: a small file and a few
/// `stat`s, all the window needs to open. Conversations and notes are read
/// after it opens ([`launched`]).
pub fn load(t: &mut Tasuke) {
    let Backend::Live(env) = &t.backend else { return };
    let config = env.config();
    t.models.prefs = config.recommend.clone();
    t.models.in_use = config.model.id.clone();
    t.models.idle_minutes = config.server.idle_minutes;
    t.models.server = "Looking…".to_owned();
    let catalog: &Catalog = &t.models.catalog;
    let store = env.store();
    for e in &catalog.models {
        t.models.installs.insert(e.id.clone(), install_of(store.state(e)));
    }
    let name = t.models.in_use.as_ref().and_then(|id| catalog.get(id)).map(|e| e.name.clone());
    let installed =
        t.models.in_use.as_ref().is_some_and(|id| matches!(t.models.installs.get(id), Some(Install::Installed)));
    t.chip = Chip {
        name: name.unwrap_or_default(),
        state: if installed { ModelState::Asleep } else { ModelState::Missing },
    };
    if t.chip.state == ModelState::Missing && t.models.in_use.is_none() {
        t.first_run = Some(crate::app::FirstRun::Looking);
    }
    t.loading = true;
    let id = t.new_conversation_id();
    t.page = crate::app::Page::Conversation(id);
}

/// What's read from disk once the window is open.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// Each saved conversation, its transcript replayed, the oldest first.
    pub conversations: Vec<(agent::Session, Transcript)>,
    pub notes: Vec<Note>,
}

/// A saved conversation replayed. The agent saves each streamed piece as
/// an event; runs of them are joined first, so an answer's Markdown is
/// parsed once rather than once a piece.
pub fn replay(entries: Vec<agent::Entry>, at: Millis) -> Transcript {
    let mut t = Transcript::new();
    let mut pending: Option<Event> = None;
    let flush = |t: &mut Transcript, pending: &mut Option<Event>| {
        if let Some(e) = pending.take() {
            t.apply(e, at);
        }
    };
    for entry in entries {
        match entry {
            agent::Entry::User { text, at: when, .. } => {
                flush(&mut t, &mut pending);
                t.user(&text, millis(when));
            }
            agent::Entry::Event { event, .. } => match (map(event), pending.as_mut()) {
                (Event::TextDelta { text }, Some(Event::TextDelta { text: so_far })) => so_far.push_str(&text),
                (Event::ReasoningDelta { text }, Some(Event::ReasoningDelta { text: so_far })) => {
                    so_far.push_str(&text)
                }
                (e @ (Event::TextDelta { .. } | Event::ReasoningDelta { .. }), _) => {
                    flush(&mut t, &mut pending);
                    pending = Some(e);
                }
                (e, _) => {
                    flush(&mut t, &mut pending);
                    t.apply(e, at);
                }
            },
        }
    }
    flush(&mut t, &mut pending);
    // A turn the window didn't see end (it closed mid-turn) is over.
    if t.busy {
        t.apply(Event::TurnFinished { reason: FinishReason::Cancelled, usage: Usage::default(), decode_tps: None }, at);
    }
    t
}

fn read(data: &agent::DataDir) -> Loaded {
    let sessions = agent::Session::list(data).unwrap_or_default();
    let conversations = sessions
        .into_iter()
        .rev()
        .map(|s| {
            let t = replay(s.transcript().unwrap_or_default(), millis(s.created()));
            (s, t)
        })
        .collect();
    Loaded { conversations, notes: notes_in(data) }
}

/// The conversations and notes read: into the sidebar and Memory.
pub fn loaded(t: &mut Tasuke, l: Loaded, cx: &mut Cx<Msg>) {
    let (act, desktop) = t.recent().first().map(|c| (c.act, c.desktop)).unwrap_or_else(|| {
        let Backend::Live(env) = &t.backend else { return (true, false) };
        let c = env.config();
        (c.agent.act, c.agent.desktop_control)
    });
    for (s, transcript) in l.conversations {
        // A conversation started before the read finished is already here.
        if t.conversations.iter().any(|c| c.session.as_ref().is_some_and(|held| held.id() == s.id())) {
            continue;
        }
        let id = t.add_conversation(millis(s.updated()), act, desktop);
        if let Some(c) = t.conversation_mut(id) {
            c.title = s.title().to_owned();
            c.transcript = transcript;
            c.session = Some(s);
        }
    }
    t.notes = l.notes;
    t.loading = false;
    let _ = cx;
}

/// After the window opens: read the conversations and notes, probe the
/// hardware.
pub fn launched(t: &mut Tasuke, cx: &mut Cx<Msg>) {
    if let Backend::Live(env) = &t.backend {
        let data = env.data.clone();
        cx.spawn(move || Msg::Loaded(Box::new(read(&data))));
    }
    cx.spawn(|| Msg::Hardware(Box::new(Hardware::detect())));
}

/// Start a turn of `id` with `text`.
pub fn start(t: &mut Tasuke, id: ConvId, text: &str, cx: &mut Cx<Msg>) {
    let Backend::Live(env) = &t.backend else { return };
    let stream = t.stream().cloned();
    let data = env.data.clone();
    let Some(c) = t.conversation_mut(id) else { return };
    if c.session.is_none() {
        match agent::Session::create(&data) {
            Ok(s) => c.session = Some(s),
            Err(e) => {
                c.transcript.apply(
                    Event::Error { text: format!("The conversation couldn't be saved: {e}"), retryable: true },
                    0,
                );
                return;
            }
        }
    }
    let Some(session) = &c.session else { return };
    let mut options = agent::Options::from_config();
    options.act = c.act;
    options.desktop_control = c.desktop;
    let handle = agent::Agent::start(session, text, options);
    if let Some(s) = stream {
        handle.set_waker(move || {
            s.send(Msg::Woke(id));
        });
    }
    c.turn = Some(Turn::Live(Live { handle }));
    if t.chip.state == ModelState::Asleep {
        t.chip.state = ModelState::Loading;
    }
    let _ = cx;
}

/// The notes in memory.toml.
pub fn notes(env: &Env) -> Vec<Note> {
    notes_in(&env.data)
}

fn notes_in(data: &agent::DataDir) -> Vec<Note> {
    agent::Memory::load(data)
        .map(|m| {
            m.list()
                .iter()
                .map(|n| Note { id: n.id.as_str().to_owned(), text: n.text.clone(), created: millis(n.created) })
                .collect()
        })
        .unwrap_or_default()
}

pub fn forget(env: &Env, id: &str) {
    if let Ok(mut m) = agent::Memory::load(&env.data)
        && let Err(e) = m.remove(&agent::NoteId::new(id))
    {
        log::warn!("forgetting {id}: {e}");
    }
}

pub fn save_model(env: &Env, id: &ModelId) {
    let mut c = env.config();
    c.model.id = Some(id.clone());
    if let Err(e) = c.save(&env.paths) {
        log::warn!("saving the model choice: {e}");
    }
}

pub fn save_prefs(env: &Env, prefs: &Preferences) {
    let mut c = env.config();
    c.recommend = prefs.clone();
    if let Err(e) = c.save(&env.paths) {
        log::warn!("saving the hidden origins: {e}");
    }
}

/// Start downloading `entry`; its events come back as messages.
pub fn download(
    env: &Env,
    entry: &Entry,
    stream: Option<MsgSender<Msg>>,
    id: ModelId,
) -> tasuke_models::DownloadHandle {
    let mut handle = tasuke_models::Download::start(entry, &env.store());
    handle_forward(&mut handle, stream, id);
    handle
}

fn handle_forward(handle: &mut tasuke_models::DownloadHandle, stream: Option<MsgSender<Msg>>, id: ModelId) {
    // Take the receiver, leaving one that never hears anything.
    let (_dead_tx, dead_rx) = std::sync::mpsc::channel();
    let rx = std::mem::replace(&mut handle.events, dead_rx);
    let Some(stream) = stream else { return };
    let _ = std::thread::Builder::new().name("tasuke-download-events".into()).spawn(move || {
        for e in rx {
            let news = match e {
                DownloadEvent::Waiting | DownloadEvent::Verified { .. } | DownloadEvent::Retrying { .. } => continue,
                DownloadEvent::Started { have, total } => DownloadNews::Progress { done: have, total, rate: 0.0 },
                DownloadEvent::Rehashing { .. } => DownloadNews::Checking,
                DownloadEvent::Progress(p) => DownloadNews::Progress { done: p.done, total: p.total, rate: p.rate.0 },
                DownloadEvent::Paused => DownloadNews::Paused,
                DownloadEvent::Resumed => DownloadNews::Resumed,
                DownloadEvent::Finished { .. } => DownloadNews::Finished,
                DownloadEvent::Failed { error } => DownloadNews::Failed(error),
                DownloadEvent::Cancelled => DownloadNews::Cancelled,
            };
            if !stream.send(Msg::Downloaded(id.clone(), news)) {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Kind;

    #[test]
    fn agent_calls_are_put_in_the_window_s_words() {
        let call = |c: agent::ToolCall| match map(agent::Event::ToolCall {
            id: agent::CallId::new("c"),
            call: c,
            needs: agent::Approval::None,
        }) {
            Event::ToolCall { call, .. } => call,
            other => panic!("{other:?}"),
        };
        let run = call(agent::ToolCall::RunCommand {
            command: "df -h".into(),
            purpose: "See the disk".into(),
            timeout_seconds: 60,
        });
        let d = run.describe();
        assert_eq!((d.done, d.target.as_str(), d.purpose.as_deref()), ("Ran", "df -h", Some("See the disk")));
        let edit = call(agent::ToolCall::EditFile {
            path: "/home/u/a.toml".into(),
            old: "a".into(),
            new: "b".into(),
            diff: "@@ -1 +1 @@\n-a\n+b".into(),
        });
        assert!(matches!(edit.describe().kind, Kind::FileChange { ref diff, .. } if diff.contains("+b")));
        let click = call(agent::ToolCall::Click {
            at: agent::ScreenPoint { x: 120.4, y: 80.0 },
            button: agent::MouseButton::Left,
        });
        assert_eq!(click.describe().target, "120, 80");
        let bad = call(agent::ToolCall::Invalid {
            name: "frobnicate".into(),
            arguments: "{".into(),
            error: "not JSON".into(),
        });
        assert_eq!(bad.name, "invalid");
        assert_eq!(bad.describe().target, "frobnicate");
        assert!(!bad.looks_only(&Approval::None), "a call that couldn't be read isn't folded away");
    }

    #[test]
    fn outcomes_and_endings_map_one_to_one() {
        let out = map(agent::Event::ToolOutput {
            id: agent::CallId::new("c"),
            output: agent::ToolOutput {
                status: agent::ToolStatus::Refused,
                text: String::new(),
                exit_code: None,
                image: None,
                truncated: false,
            },
        });
        let Event::ToolOutput { output, .. } = out else { panic!() };
        assert!(output.refused());
        let end = map(agent::Event::TurnFinished {
            reason: agent::FinishReason::Answered,
            usage: agent::Usage::default(),
            decode_tps: Some(70.0),
        });
        assert!(matches!(end, Event::TurnFinished { reason: FinishReason::Done, decode_tps: Some(_), .. }));
    }
}
