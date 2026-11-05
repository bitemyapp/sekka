//! What a turn reports as it runs. The window and `tasuke ask` consume the
//! same stream, and the session file records it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{CallId, MarkId, NoteId, Tokens, TurnNumber};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    TurnStarted {
        turn: TurnNumber,
    },
    /// The model's thinking, as it streams (not every model has any).
    ReasoningDelta {
        text: String,
    },
    /// The model's answer, as it streams.
    TextDelta {
        text: String,
    },
    /// The model asked for a tool. When `needs` asks, the turn waits for
    /// [`crate::TurnHandle::decide`] with this id.
    ToolCall {
        id: CallId,
        call: ToolCall,
        needs: Approval,
    },
    ToolStarted {
        id: CallId,
    },
    ToolOutput {
        id: CallId,
        output: ToolOutput,
    },
    /// A note was remembered or forgotten.
    MemoryChanged,
    /// Something the person should know: the model is loading, the context
    /// was trimmed or summarized, the step budget ran out.
    Notice {
        level: Level,
        text: String,
    },
    TurnFinished {
        reason: FinishReason,
        usage: Usage,
        /// The last request's decode speed, as the server measured it.
        decode_tps: Option<f32>,
    },
    Error {
        text: String,
        /// Whether sending the request again could work (the server was
        /// unreachable or busy), as opposed to a bad request.
        retryable: bool,
    },
}

/// A point in the global layout, in points (what `aizomectl input motion` takes).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn as_str(self) -> &'static str {
        match self {
            MouseButton::Left => "left",
            MouseButton::Right => "right",
            MouseButton::Middle => "middle",
        }
    }
}

/// A tool call as Tasuke understood it, with the model's arguments checked
/// and typed. Paths have `~` expanded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    RunCommand {
        command: String,
        purpose: String,
        timeout_seconds: u32,
    },
    RunAsRoot {
        command: String,
        purpose: String,
        timeout_seconds: u32,
    },
    ReadFile {
        path: PathBuf,
        /// The first line to show, from 1.
        offset: Option<u64>,
        /// How many lines.
        limit: Option<u64>,
    },
    WriteFile {
        path: PathBuf,
        content: String,
        /// What changes, as a unified diff (filled in before approval).
        diff: String,
    },
    EditFile {
        path: PathBuf,
        old: String,
        new: String,
        diff: String,
    },
    LoadSkill {
        name: String,
        /// One section of it, by heading (the whole guide when absent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    /// Earlier conversations with this person, searched for `query`.
    SearchConversations {
        query: String,
    },
    Remember {
        text: String,
    },
    Forget {
        id: NoteId,
    },
    Screenshot,
    ListWindows,
    UiElements,
    Click {
        at: ScreenPoint,
        button: MouseButton,
    },
    DoubleClick {
        at: ScreenPoint,
    },
    Drag {
        from: ScreenPoint,
        to: ScreenPoint,
    },
    Scroll {
        at: ScreenPoint,
        /// Lines: positive scrolls down.
        lines: f64,
    },
    TypeText {
        text: String,
    },
    Key {
        keys: String,
    },
    OpenApp {
        name: String,
    },
    /// A click on a numbered element of the last marked screenshot (set-of-marks).
    ClickElement {
        mark: MarkId,
        label: String,
        at: ScreenPoint,
    },
    /// A click on a numbered element, then typing into it.
    TypeInto {
        mark: MarkId,
        label: String,
        at: ScreenPoint,
        text: String,
    },
    Wait {
        seconds: f64,
    },
    /// A call that can't run as written: an unknown tool, or arguments that
    /// don't fit its schema. The model is told why.
    Invalid {
        name: String,
        arguments: String,
        error: String,
    },
}

impl ToolCall {
    /// The tool's name, as the model calls it.
    pub fn tool_name(&self) -> &str {
        match self {
            ToolCall::RunCommand { .. } => "run_command",
            ToolCall::RunAsRoot { .. } => "run_as_root",
            ToolCall::ReadFile { .. } => "read_file",
            ToolCall::WriteFile { .. } => "write_file",
            ToolCall::EditFile { .. } => "edit_file",
            ToolCall::LoadSkill { .. } => "load_skill",
            ToolCall::SearchConversations { .. } => "search_conversations",
            ToolCall::Remember { .. } => "remember",
            ToolCall::Forget { .. } => "forget",
            ToolCall::Screenshot => "screenshot",
            ToolCall::ListWindows => "list_windows",
            ToolCall::UiElements => "ui_elements",
            ToolCall::Click { .. } => "click",
            ToolCall::DoubleClick { .. } => "double_click",
            ToolCall::Drag { .. } => "drag",
            ToolCall::Scroll { .. } => "scroll",
            ToolCall::TypeText { .. } => "type_text",
            ToolCall::Key { .. } => "key",
            ToolCall::OpenApp { .. } => "open_app",
            ToolCall::ClickElement { .. } => "click_element",
            ToolCall::TypeInto { .. } => "type_into",
            ToolCall::Wait { .. } => "wait",
            ToolCall::Invalid { name, .. } => name,
        }
    }

    /// One line for a transcript row: the verb and its command or target.
    pub fn summary(&self) -> String {
        fn pt(p: &ScreenPoint) -> String {
            format!("({:.0}, {:.0})", p.x, p.y)
        }
        match self {
            ToolCall::RunCommand { command, .. } => format!("Run {}", first_line(command)),
            ToolCall::RunAsRoot { command, .. } => format!("Run as administrator {}", first_line(command)),
            ToolCall::ReadFile { path, .. } => format!("Read {}", path.display()),
            ToolCall::WriteFile { path, .. } => format!("Write {}", path.display()),
            ToolCall::EditFile { path, .. } => format!("Edit {}", path.display()),
            ToolCall::LoadSkill { name, section: None } => format!("Read the guide {name}"),
            ToolCall::LoadSkill { name, section: Some(s) } => format!("Read \"{}\" in the guide {name}", first_line(s)),
            ToolCall::SearchConversations { query } => {
                format!("Search earlier conversations for \"{}\"", first_line(query))
            }
            ToolCall::Remember { text } => format!("Remember \"{}\"", first_line(text)),
            ToolCall::Forget { id } => format!("Forget note {id}"),
            ToolCall::Screenshot => "Look at the screen".into(),
            ToolCall::ListWindows => "List the windows".into(),
            ToolCall::UiElements => "List the controls on screen".into(),
            ToolCall::Click { at, button } => match button {
                MouseButton::Left => format!("Click at {}", pt(at)),
                b => format!("Click {} at {}", b.as_str(), pt(at)),
            },
            ToolCall::DoubleClick { at } => format!("Double-click at {}", pt(at)),
            ToolCall::Drag { from, to } => format!("Drag from {} to {}", pt(from), pt(to)),
            ToolCall::Scroll { at, lines } => {
                format!("Scroll {} {} lines at {}", if *lines < 0.0 { "up" } else { "down" }, lines.abs(), pt(at))
            }
            ToolCall::TypeText { text } => format!("Type \"{}\"", first_line(text)),
            ToolCall::Key { keys } => format!("Press {keys}"),
            ToolCall::OpenApp { name } => format!("Open {name}"),
            ToolCall::ClickElement { mark, label, .. } => format!("Click [{mark}] \"{}\"", first_line(label)),
            ToolCall::TypeInto { mark, label, text, .. } => {
                format!("Type \"{}\" into [{mark}] \"{}\"", first_line(text), first_line(label))
            }
            ToolCall::Wait { seconds } => format!("Wait {seconds} s"),
            ToolCall::Invalid { name, .. } => format!("{name} (not understood)"),
        }
    }

    /// Whether it acts on the desktop through the compositor's input.
    pub fn is_desktop_action(&self) -> bool {
        matches!(
            self,
            ToolCall::Click { .. }
                | ToolCall::DoubleClick { .. }
                | ToolCall::Drag { .. }
                | ToolCall::Scroll { .. }
                | ToolCall::TypeText { .. }
                | ToolCall::Key { .. }
                | ToolCall::OpenApp { .. }
                | ToolCall::ClickElement { .. }
                | ToolCall::TypeInto { .. }
        )
    }
}

fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(200).collect();
    if out.len() < line.len() || s.lines().nth(1).is_some() {
        out.push('…');
    }
    out
}

/// What a call needs before it runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Approval {
    /// It only inspects: it runs at once.
    None,
    /// It may change something: the person decides.
    Ask { reason: String },
    /// It runs as root: the person decides, then polkit asks for a password.
    AskRoot { reason: String },
    /// Tasuke never runs it (it would end the session, delete everything,
    /// or cut the assistant off). No decision is asked for; the model is told.
    Refused { reason: String },
}

impl Approval {
    pub fn asks(&self) -> bool {
        matches!(self, Approval::Ask { .. } | Approval::AskRoot { .. })
    }
}

/// The person's answer to an approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Decision {
    Allow,
    /// Don't run it; the note, if any, is passed to the model.
    Deny {
        note: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// It ran (a command may still have exited non-zero: see `exit_code`).
    Done,
    /// It could not run, or the tool failed.
    Failed,
    /// The person said no.
    Denied,
    /// The policy refused it outright.
    Refused,
    TimedOut,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub status: ToolStatus,
    /// What it printed or returned, capped (head and tail kept).
    pub text: String,
    pub exit_code: Option<i32>,
    /// A screenshot it took, if any.
    pub image: Option<PathBuf>,
    /// Whether `text` lost its middle to the cap.
    pub truncated: bool,
}

impl ToolOutput {
    pub(crate) fn done(text: impl Into<String>) -> ToolOutput {
        ToolOutput { status: ToolStatus::Done, text: text.into(), exit_code: None, image: None, truncated: false }
    }

    pub(crate) fn failed(text: impl Into<String>) -> ToolOutput {
        ToolOutput { status: ToolStatus::Failed, ..ToolOutput::done(text) }
    }

    pub(crate) fn with_status(status: ToolStatus, text: impl Into<String>) -> ToolOutput {
        ToolOutput { status, ..ToolOutput::done(text) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model answered without asking for another tool.
    Answered,
    /// The model's reply was cut off by the output or context limit.
    Length,
    StepBudget,
    TimeLimit,
    Cancelled,
    /// The server failed, or the model kept sending calls that couldn't be parsed.
    Failed,
}

/// What one turn cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt tokens over all requests (each request counts its whole prompt).
    pub prompt_tokens: Tokens,
    /// Of those, how many came from the server's prompt cache.
    pub cached_tokens: Tokens,
    pub completion_tokens: Tokens,
    /// The largest prompt: how full the context got.
    pub max_prompt_tokens: Tokens,
    pub requests: u32,
    pub tool_calls: u32,
    /// Wall time in the model's requests.
    pub llm_seconds: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_through_json() {
        let events = vec![
            Event::TurnStarted { turn: TurnNumber(1) },
            Event::ToolCall {
                id: CallId::new("c1"),
                call: ToolCall::RunCommand { command: "ls".into(), purpose: "look".into(), timeout_seconds: 60 },
                needs: Approval::None,
            },
            Event::ToolCall { id: CallId::new("c2"), call: ToolCall::Screenshot, needs: Approval::None },
            Event::ToolOutput { id: CallId::new("c1"), output: ToolOutput::done("x") },
            Event::TurnFinished { reason: FinishReason::Answered, usage: Usage::default(), decode_tps: Some(42.0) },
        ];
        for e in events {
            let s = serde_json::to_string(&e).unwrap();
            assert_eq!(serde_json::from_str::<Event>(&s).unwrap(), e, "{s}");
        }
    }

    #[test]
    fn summaries_are_one_line() {
        let c = ToolCall::RunCommand { command: "ls\nrm x".into(), purpose: String::new(), timeout_seconds: 60 };
        assert_eq!(c.summary(), "Run ls…");
        let c = ToolCall::Click { at: ScreenPoint { x: 10.4, y: 20.6 }, button: MouseButton::Right };
        assert_eq!(c.summary(), "Click right at (10, 21)");
        assert!(c.is_desktop_action());
        assert!(!ToolCall::Screenshot.is_desktop_action());
    }
}
