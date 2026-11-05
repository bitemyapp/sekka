//! The window on this computer: Tasuke's agent (`tasuke-agent`) runs each
//! turn on its own thread and wakes the window through the handle's waker;
//! its events are mapped into the window's ([`map`]). Conversations and
//! memory are read from the agent's files, models from `tasuke-models`.
//!
//! What the window assumes of the agent: every turn ends with
//! `TurnFinished` (or `Error`), a call that asks waits for `decide` with
//! its id, and `cancel` ends a turn that is waiting or running.

use std::sync::mpsc::TryRecvError;

use aizome_kit::prelude::*;
use aizome_kit::MsgSender;
use tasuke_agent as agent;
use tasuke_models::{Catalog, Config, DownloadEvent, Entry, Hardware, ModelId, Paths, Preferences, Store};

use crate::agent::{Approval, Decision, Event, FinishReason, Level, Outcome, ToolCall, ToolOutput, Usage};
use crate::app::{Backend, Chip, ConvId, DownloadNews, ModelState, Msg, Note, Tasuke, Turn};
use crate::models::{Install, install_of};
use crate::transcript::Millis;

/// Where Tasuke's files are.
pub struct Env {
    pub data: agent::DataDir,
    pub paths: Paths,
}

impl Env {
    pub fn new() -> Env {
        Env { data: agent::DataDir::user(), paths: Paths::from_env() }
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
                    if !out.iter().any(|e| matches!(e, Event::TurnFinished { .. } | Event::Error { .. })) && self.handle.is_finished() {
                        out.push(Event::TurnFinished { reason: FinishReason::Failed, usage: Usage::default(), decode_tps: None });
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
            let name = call.tool_name().to_owned();
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

/// Read the conversations, the notes, the catalog and the settings: what
/// the window shows before anything is probed.
pub fn load(t: &mut Tasuke) {
    let Backend::Live(env) = &t.backend else { return };
    let config = env.config();
    let sessions = agent::Session::list(&env.data).unwrap_or_default();
    let notes = notes(env);
    t.models.prefs = config.recommend.clone();
    t.models.in_use = config.model.id.clone();
    t.models.server = format!("A model unloads after {} minutes unused", config.server.idle_minutes);
    t.notes = notes;
    let catalog: &Catalog = &t.models.catalog;
    let store = env.store();
    for e in &catalog.models {
        t.models.installs.insert(e.id.clone(), install_of(store.state(e)));
    }
    let name = t.models.in_use.as_ref().and_then(|id| catalog.get(id)).map(|e| e.name.clone());
    let installed = t.models.in_use.as_ref().is_some_and(|id| matches!(t.models.installs.get(id), Some(Install::Installed)));
    t.chip = Chip {
        name: name.unwrap_or_default(),
        state: if installed { ModelState::Asleep } else { ModelState::Missing },
    };
    let (act, desktop) = (config.agent.act, config.agent.desktop_control);
    for s in sessions.into_iter().rev() {
        let id = t.add_conversation(millis(s.updated()), act, desktop);
        let Some(c) = t.conversation_mut(id) else { continue };
        c.title = s.title().to_owned();
        let at = millis(s.created());
        for entry in s.transcript().unwrap_or_default() {
            match entry {
                agent::Entry::User { text, at: when, .. } => c.transcript.user(&text, millis(when)),
                agent::Entry::Event { event, .. } => c.transcript.apply(map(event), at),
            }
        }
        // A turn the window didn't see end (it closed mid-turn) is over.
        if c.transcript.busy {
            c.transcript.apply(
                Event::TurnFinished { reason: FinishReason::Cancelled, usage: Usage::default(), decode_tps: None },
                at,
            );
        }
        c.session = Some(s);
    }
    if t.chip.state == ModelState::Missing && t.models.in_use.is_none() {
        t.first_run = Some(crate::app::FirstRun::Looking);
    }
    let id = t.new_conversation_id();
    t.page = crate::app::Page::Conversation(id);
}

/// After the window opens: probe the hardware.
pub fn launched(_t: &mut Tasuke, cx: &mut Cx<Msg>) {
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
                c.transcript.apply(Event::Error { text: format!("The conversation couldn't be saved: {e}"), retryable: true }, 0);
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
    agent::Memory::load(&env.data)
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
pub fn download(env: &Env, entry: &Entry, stream: Option<MsgSender<Msg>>, id: ModelId) -> tasuke_models::DownloadHandle {
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
