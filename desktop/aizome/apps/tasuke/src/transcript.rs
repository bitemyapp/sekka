//! A conversation as it reads: the agent's events folded into items, in
//! the order they happened. Answers are Markdown, parsed as they stream;
//! reasoning keeps when it began and ended ("Thought for 4 s"); each tool
//! call is one item from its card or row to its output; notices and
//! errors stand where they arose.
//!
//! Folding is the whole of it: [`Transcript::apply`] takes one event and
//! the time it came, so a session read from disk and a turn arriving live
//! make the same transcript.

use std::sync::Arc;

use crate::agent::{Approval, Described, Event, FinishReason, Level, ToolCall, ToolOutput};
use crate::approval::Gate;
use crate::markdown::{self, Doc};

/// Milliseconds since the Unix epoch.
pub type Millis = i64;

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable within its transcript: the key its widgets keep state by.
    pub id: u64,
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    User { text: String, at: Millis },
    Answer(Answer),
    Reasoning(Reasoning),
    Tool(Tool),
    Notice { level: Level, text: String },
    Error { text: String, retryable: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub text: String,
    pub doc: Arc<Doc>,
    /// Still being written.
    pub open: bool,
    /// The turn's first answer, which carries its heading.
    pub first: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reasoning {
    pub text: String,
    pub started: Millis,
    pub ended: Option<Millis>,
}

impl Reasoning {
    /// "Thought for 4 s": whole seconds, at least one.
    pub fn seconds(&self) -> Option<i64> {
        self.ended.map(|e| ((e - self.started + 500) / 1000).max(1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub call_id: String,
    pub call: ToolCall,
    pub described: Described,
    /// Its approval, if it needed one.
    pub gate: Option<Gate>,
    pub state: ToolState,
    pub output: Option<ToolOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolState {
    /// Not started: waiting for its approval, or for the agent.
    Waiting,
    Running,
    Done { exit: Option<i32> },
    Failed { exit: i32 },
    Denied,
    /// The turn ended before it ran or finished.
    Stopped,
}

impl ToolState {
    /// Its state in words, as the row says it.
    pub fn words(&self) -> String {
        match self {
            ToolState::Waiting => "waiting".into(),
            ToolState::Running => "running".into(),
            ToolState::Done { exit: Some(code) } => format!("done (exit {code})"),
            ToolState::Done { exit: None } => "done".into(),
            ToolState::Failed { exit } => format!("failed (exit {exit})"),
            ToolState::Denied => "denied".into(),
            ToolState::Stopped => "stopped".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    pub items: Vec<Item>,
    /// A turn is under way.
    pub busy: bool,
    /// The last finished turn's decode speed, in tokens a second.
    pub decode_tps: Option<f32>,
    /// The notes changed during the last events applied.
    pub memory_changed: bool,
    /// Bumped with every change, for views that cache by it.
    pub revision: u64,
    next_id: u64,
}

impl Transcript {
    pub fn new() -> Transcript {
        Transcript::default()
    }

    /// Replay events, as a session's file holds them, with their times.
    pub fn replay(events: impl IntoIterator<Item = (Millis, Event)>) -> Transcript {
        let mut t = Transcript::new();
        for (at, e) in events {
            t.apply(e, at);
        }
        t
    }

    fn push(&mut self, body: Body) {
        self.next_id += 1;
        self.items.push(Item { id: self.next_id, body });
    }

    /// The person's message, as it's sent.
    pub fn user(&mut self, text: &str, at: Millis) {
        self.end_reasoning(at);
        self.close_answer();
        self.push(Body::User { text: text.to_owned(), at });
        self.revision += 1;
    }

    /// The person's last message: what Retry sends again.
    pub fn last_prompt(&self) -> Option<&str> {
        self.items.iter().rev().find_map(|i| match &i.body {
            Body::User { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    pub fn tool_mut(&mut self, call_id: &str) -> Option<&mut Tool> {
        self.items.iter_mut().rev().find_map(|i| match &mut i.body {
            Body::Tool(t) if t.call_id == call_id => Some(t),
            _ => None,
        })
    }

    pub fn tool(&self, call_id: &str) -> Option<&Tool> {
        self.items.iter().rev().find_map(|i| match &i.body {
            Body::Tool(t) if t.call_id == call_id => Some(t),
            _ => None,
        })
    }

    /// The approval waiting for the person, if one is.
    pub fn pending_approval(&self) -> Option<&Tool> {
        self.items.iter().rev().find_map(|i| match &i.body {
            Body::Tool(t) if t.gate.as_ref().is_some_and(Gate::is_pending) => Some(t),
            _ => None,
        })
    }

    fn end_reasoning(&mut self, at: Millis) {
        if let Some(Item { body: Body::Reasoning(r), .. }) = self.items.last_mut()
            && r.ended.is_none()
        {
            r.ended = Some(at);
        }
    }

    fn close_answer(&mut self) {
        for i in self.items.iter_mut().rev() {
            match &mut i.body {
                Body::Answer(a) => {
                    a.open = false;
                    return;
                }
                Body::User { .. } => return,
                _ => {}
            }
        }
    }

    /// Whether this turn has an answer yet (since the last message).
    fn answered(&self) -> bool {
        for i in self.items.iter().rev() {
            match &i.body {
                Body::Answer(_) => return true,
                Body::User { .. } => return false,
                _ => {}
            }
        }
        false
    }

    pub fn apply(&mut self, event: Event, at: Millis) {
        self.revision += 1;
        match event {
            Event::TurnStarted { .. } => self.busy = true,
            Event::ReasoningDelta { text } => {
                self.close_answer();
                match self.items.last_mut() {
                    Some(Item { body: Body::Reasoning(r), .. }) if r.ended.is_none() => r.text.push_str(&text),
                    _ => self.push(Body::Reasoning(Reasoning { text, started: at, ended: None })),
                }
            }
            Event::TextDelta { text } => {
                self.end_reasoning(at);
                if let Some(Item { body: Body::Answer(a), .. }) = self.items.last_mut()
                    && a.open
                {
                    a.text.push_str(&text);
                    a.doc = Arc::new(markdown::parse(&a.text));
                    return;
                }
                // Whitespace between a tool's output and the next answer
                // isn't an answer of its own.
                if text.trim().is_empty() {
                    return;
                }
                let first = !self.answered();
                let text = text.trim_start().to_owned();
                let doc = Arc::new(markdown::parse(&text));
                self.push(Body::Answer(Answer { text, doc, open: true, first }));
            }
            Event::ToolCall { id, call, needs } => {
                self.end_reasoning(at);
                self.close_answer();
                let gate = match needs {
                    Approval::None => None,
                    Approval::Ask { reason } => Some(Gate::pending(reason, false)),
                    Approval::AskRoot { reason } => Some(Gate::pending(reason, true)),
                };
                let described = call.describe();
                self.push(Body::Tool(Tool { call_id: id, call, described, gate, state: ToolState::Waiting, output: None }));
            }
            Event::ToolStarted { id } => {
                if let Some(t) = self.tool_mut(&id) {
                    if let Some(g) = &mut t.gate {
                        g.started();
                    }
                    if !matches!(t.state, ToolState::Denied) {
                        t.state = ToolState::Running;
                    }
                }
            }
            Event::ToolOutput { id, output } => {
                if let Some(t) = self.tool_mut(&id) {
                    if output.refused {
                        if let Some(g) = &mut t.gate {
                            g.refused();
                        }
                        t.state = ToolState::Denied;
                    } else {
                        if let Some(g) = &mut t.gate {
                            g.started();
                        }
                        t.state = match output.exit {
                            Some(code) if code != 0 => ToolState::Failed { exit: code },
                            exit => ToolState::Done { exit },
                        };
                    }
                    t.output = Some(output);
                }
            }
            Event::MemoryChanged => self.memory_changed = true,
            Event::Notice { level, text } => {
                self.end_reasoning(at);
                self.push(Body::Notice { level, text });
            }
            Event::TurnFinished { reason, decode_tps, .. } => {
                self.finish(at);
                if decode_tps.is_some() {
                    self.decode_tps = decode_tps;
                }
                if reason == FinishReason::Cancelled {
                    self.push(Body::Notice { level: Level::Info, text: "Stopped.".into() });
                }
            }
            Event::Error { text, retryable } => {
                self.finish(at);
                self.push(Body::Error { text, retryable });
            }
        }
    }

    /// The turn is over: whatever was open closes, and whatever waited
    /// won't run.
    fn finish(&mut self, at: Millis) {
        self.busy = false;
        self.end_reasoning(at);
        self.close_answer();
        for i in &mut self.items {
            if let Body::Tool(t) = &mut i.body {
                if let Some(g) = &mut t.gate {
                    g.turn_ended();
                }
                if matches!(t.state, ToolState::Waiting | ToolState::Running) {
                    t.state = ToolState::Stopped;
                }
            }
        }
    }

    /// The person answered a card: the decision to send, if it was theirs
    /// to make (once).
    pub fn decide(&mut self, call_id: &str, decision: crate::agent::Decision) -> Option<crate::agent::Decision> {
        let t = self.tool_mut(call_id)?;
        let gate = t.gate.as_mut()?;
        let sent = gate.decide(decision).ok()?;
        if matches!(sent, crate::agent::Decision::Deny { .. }) {
            t.state = ToolState::Denied;
        }
        self.revision += 1;
        Some(sent)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agent::{Decision, Usage};

    fn finished() -> Event {
        Event::TurnFinished { reason: FinishReason::Done, usage: Usage::default(), decode_tps: Some(70.4) }
    }

    fn texts(t: &Transcript) -> Vec<String> {
        t.items
            .iter()
            .map(|i| match &i.body {
                Body::User { text, .. } => format!("user: {text}"),
                Body::Answer(a) => format!("answer{}: {}", if a.open { " (open)" } else { "" }, a.text),
                Body::Reasoning(r) => format!("reasoning: {}", r.text),
                Body::Tool(t) => format!("tool {}: {}", t.call_id, t.state.words()),
                Body::Notice { text, .. } => format!("notice: {text}"),
                Body::Error { text, .. } => format!("error: {text}"),
            })
            .collect()
    }

    #[test]
    fn deltas_fold_into_reasoning_then_an_answer() {
        let mut t = Transcript::new();
        t.user("Why is it slow?", 0);
        t.apply(Event::TurnStarted { turn: 1 }, 10);
        t.apply(Event::ReasoningDelta { text: "Let me ".into() }, 100);
        t.apply(Event::ReasoningDelta { text: "look.".into() }, 900);
        t.apply(Event::TextDelta { text: "It's **swap**".into() }, 4100);
        t.apply(Event::TextDelta { text: " pressure.".into() }, 4200);
        assert!(t.busy);
        assert_eq!(texts(&t), ["user: Why is it slow?", "reasoning: Let me look.", "answer (open): It's **swap** pressure."]);
        let Body::Reasoning(r) = &t.items[1].body else { panic!() };
        assert_eq!(r.seconds(), Some(4), "from its first delta to the answer's");
        t.apply(finished(), 4300);
        assert!(!t.busy);
        assert_eq!(t.decode_tps, Some(70.4));
        let Body::Answer(a) = &t.items[2].body else { panic!() };
        assert!(!a.open && a.first);
        assert!(matches!(a.doc.blocks.first(), Some(markdown::Block::Paragraph(_))));
    }

    #[test]
    fn a_tool_call_ends_the_answer_before_it_and_the_next_text_is_a_new_one() {
        let mut t = Transcript::new();
        t.user("Is the sound on?", 0);
        t.apply(Event::TextDelta { text: "I'll check.".into() }, 10);
        t.apply(
            Event::ToolCall {
                id: "c1".into(),
                call: ToolCall::new("run_command", json!({"command": "wpctl status"})),
                needs: Approval::None,
            },
            20,
        );
        t.apply(Event::ToolStarted { id: "c1".into() }, 30);
        assert_eq!(t.tool("c1").unwrap().state, ToolState::Running);
        t.apply(Event::ToolOutput { id: "c1".into(), output: ToolOutput { text: "ok".into(), exit: Some(0), ..Default::default() } }, 40);
        t.apply(Event::TextDelta { text: "\n\n".into() }, 45);
        t.apply(Event::TextDelta { text: "It's muted.".into() }, 50);
        t.apply(finished(), 60);
        assert_eq!(texts(&t), ["user: Is the sound on?", "answer: I'll check.", "tool c1: done (exit 0)", "answer: It's muted."]);
        let Body::Answer(second) = &t.items[3].body else { panic!() };
        assert!(!second.first, "only the turn's first answer carries the heading");
    }

    #[test]
    fn exit_codes_and_refusals_are_told_apart() {
        let mut t = Transcript::new();
        for (id, exit) in [("a", Some(0)), ("b", Some(1)), ("c", None)] {
            t.apply(Event::ToolCall { id: id.into(), call: ToolCall::new("run_command", json!({})), needs: Approval::None }, 0);
            t.apply(Event::ToolOutput { id: id.into(), output: ToolOutput { exit, ..Default::default() } }, 1);
        }
        assert_eq!(t.tool("a").unwrap().state.words(), "done (exit 0)");
        assert_eq!(t.tool("b").unwrap().state.words(), "failed (exit 1)");
        assert_eq!(t.tool("c").unwrap().state.words(), "done");
    }

    #[test]
    fn an_approval_waits_then_runs_once_allowed() {
        let mut t = Transcript::new();
        t.apply(Event::TurnStarted { turn: 1 }, 0);
        t.apply(
            Event::ToolCall {
                id: "fix".into(),
                call: ToolCall::new("run_command", json!({"command": "systemctl --user restart wireplumber"})),
                needs: Approval::Ask { reason: "It restarts a service".into() },
            },
            10,
        );
        assert_eq!(t.pending_approval().map(|t| t.call_id.as_str()), Some("fix"));
        assert_eq!(t.decide("fix", Decision::Allow), Some(Decision::Allow));
        assert_eq!(t.decide("fix", Decision::Allow), None, "answered once");
        assert!(t.pending_approval().is_none());
        t.apply(Event::ToolStarted { id: "fix".into() }, 20);
        let tool = t.tool("fix").unwrap();
        assert_eq!((tool.gate.clone(), tool.state.clone()), (Some(Gate::Allowed), ToolState::Running));
    }

    #[test]
    fn dont_run_denies_and_stopping_withdraws_an_open_card() {
        let mut t = Transcript::new();
        let ask = |id: &str| Event::ToolCall {
            id: id.into(),
            call: ToolCall::new("write_file", json!({"path": "/tmp/x", "content": "y"})),
            needs: Approval::Ask { reason: "It writes a file".into() },
        };
        t.apply(Event::TurnStarted { turn: 1 }, 0);
        t.apply(ask("one"), 1);
        t.decide("one", Decision::Deny { note: None });
        assert_eq!(t.tool("one").unwrap().state, ToolState::Denied);
        // The agent reports the refusal back; it stays denied.
        t.apply(Event::ToolOutput { id: "one".into(), output: ToolOutput { refused: true, ..Default::default() } }, 2);
        assert_eq!(t.tool("one").unwrap().state, ToolState::Denied);
        t.apply(ask("two"), 3);
        t.apply(Event::TurnFinished { reason: FinishReason::Cancelled, usage: Usage::default(), decode_tps: None }, 4);
        let two = t.tool("two").unwrap();
        assert_eq!((two.gate.clone(), two.state.clone()), (Some(Gate::Withdrawn), ToolState::Stopped));
        assert_eq!(texts(&t).last().unwrap(), "notice: Stopped.");
        assert_eq!(t.decode_tps, None, "a stopped turn keeps the speed before it");
    }

    #[test]
    fn an_error_ends_the_turn_and_retry_sends_the_last_message_again() {
        let mut t = Transcript::new();
        t.user("Free some disk space", 0);
        t.apply(Event::TurnStarted { turn: 1 }, 1);
        t.apply(Event::Error { text: "The model server stopped answering.".into(), retryable: true }, 2);
        assert!(!t.busy);
        assert_eq!(t.last_prompt(), Some("Free some disk space"));
        assert!(matches!(t.items.last().unwrap().body, Body::Error { retryable: true, .. }));
    }

    #[test]
    fn a_session_replays_to_the_same_transcript() {
        let events = vec![
            (0, Event::TurnStarted { turn: 1 }),
            (5, Event::TextDelta { text: "Hello".into() }),
            (9, finished()),
        ];
        let mut live = Transcript::new();
        for (at, e) in events.clone() {
            live.apply(e, at);
        }
        assert_eq!(Transcript::replay(events).items, live.items);
    }
}
