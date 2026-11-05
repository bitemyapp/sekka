//! What the agent tells the window, in the window's own terms: the event
//! stream of `apps/tasuke/DESIGN.md` ("Events"), which `tasuke-agent`'s
//! `TurnHandle` delivers and [`crate::live`] maps into these. The window
//! folds them into a transcript ([`crate::transcript`]) and never asks
//! the agent anything but to start, decide, or cancel.
//!
//! A tool call is kept as the model made it, a name and its JSON
//! arguments, so the window shows any tool the agent offers, and words
//! the ones it knows ([`ToolCall::describe`]).

use std::path::PathBuf;

use aizome_kit::Icon;
use serde_json::Value;

/// One thing that happened in a turn.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    TurnStarted { turn: u32 },
    ReasoningDelta { text: String },
    TextDelta { text: String },
    ToolCall { id: String, call: ToolCall, needs: Approval },
    ToolStarted { id: String },
    ToolOutput { id: String, output: ToolOutput },
    /// The notes changed (`remember`, `forget`): read them again.
    MemoryChanged,
    Notice { level: Level, text: String },
    TurnFinished { reason: FinishReason, usage: Usage, decode_tps: Option<f32> },
    Error { text: String, retryable: bool },
}

/// Whether a call waits for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Approval {
    None,
    /// It changes something: the reason the policy gave.
    Ask { reason: String },
    /// It runs as root: approval, then polkit's password.
    AskRoot { reason: String },
}

/// What the person decided about a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny { note: Option<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishReason {
    /// The model answered without asking for a tool.
    Done,
    /// The answer reached the model's length limit.
    Length,
    /// The step budget ran out.
    StepBudget,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// What a tool gave back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolOutput {
    /// Its output, capped by the agent (head and tail).
    pub text: String,
    /// A command's exit status; `None` for tools that have none.
    pub exit: Option<i32>,
    /// A screenshot it took.
    pub image: Option<PathBuf>,
    /// The call didn't run: the person said no (or the turn ended first).
    pub refused: bool,
}

/// A call as the model made it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
}

/// How a call is shown: what it does, to what, and for what.
#[derive(Clone, Debug, PartialEq)]
pub struct Described {
    pub icon: Icon,
    /// While it runs and once it has: "Running", "Ran".
    pub doing: &'static str,
    pub done: &'static str,
    /// The command, path, keys or app: shown in monospace when `code`.
    pub target: String,
    pub code: bool,
    /// The model's stated purpose, for an approval.
    pub purpose: Option<String>,
    pub kind: Kind,
}

/// What an approval would let it do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Command,
    RootCommand,
    /// Writing or editing a file: the change, as a unified diff.
    FileChange { path: String, diff: String },
    Desktop,
    Other,
}

impl ToolCall {
    pub fn new(name: &str, args: Value) -> ToolCall {
        ToolCall { name: name.to_owned(), args }
    }

    fn arg(&self, key: &str) -> Option<&str> {
        self.args.get(key).and_then(Value::as_str)
    }

    fn num(&self, key: &str) -> Option<f64> {
        self.args.get(key).and_then(Value::as_f64)
    }

    pub fn describe(&self) -> Described {
        let purpose = self.arg("purpose").map(str::to_owned).filter(|p| !p.trim().is_empty());
        let d = |icon, doing, done, target: String, code| Described {
            icon,
            doing,
            done,
            target,
            code,
            purpose: purpose.clone(),
            kind: Kind::Other,
        };
        let at = |c: &ToolCall| match (c.num("x"), c.num("y")) {
            (Some(x), Some(y)) => format!("{x:.0}, {y:.0}"),
            _ => String::new(),
        };
        match self.name.as_str() {
            "run_command" => Described {
                kind: Kind::Command,
                ..d(Icon::SquareTerminal, "Running", "Ran", self.arg("command").unwrap_or("").to_owned(), true)
            },
            "run_as_root" => Described {
                kind: Kind::RootCommand,
                ..d(Icon::ShieldCheck, "Running as administrator", "Ran as administrator", self.arg("command").unwrap_or("").to_owned(), true)
            },
            "read_file" => d(Icon::FileText, "Reading", "Read", self.arg("path").unwrap_or("").to_owned(), true),
            "write_file" => {
                let path = self.arg("path").unwrap_or("").to_owned();
                let diff = self.arg("diff").map(str::to_owned).unwrap_or_else(|| {
                    crate::diff::unified("", self.arg("content").unwrap_or(""))
                });
                Described { kind: Kind::FileChange { path: path.clone(), diff }, ..d(Icon::Pencil, "Writing", "Wrote", path, true) }
            }
            "edit_file" => {
                let path = self.arg("path").unwrap_or("").to_owned();
                let diff = self.arg("diff").map(str::to_owned).unwrap_or_else(|| {
                    crate::diff::unified(self.arg("old").unwrap_or(""), self.arg("new").unwrap_or(""))
                });
                Described { kind: Kind::FileChange { path: path.clone(), diff }, ..d(Icon::Pencil, "Changing", "Changed", path, true) }
            }
            "load_skill" => d(Icon::BookOpen, "Reading notes on", "Read notes on", self.arg("name").unwrap_or("").to_owned(), false),
            "remember" => d(Icon::NotebookPen, "Remembering", "Remembered", self.arg("text").unwrap_or("").to_owned(), false),
            "forget" => d(Icon::NotebookPen, "Forgetting", "Forgot", self.arg("id").unwrap_or("").to_owned(), false),
            "screenshot" => Described { kind: Kind::Desktop, ..d(Icon::Camera, "Looking at the screen", "Looked at the screen", String::new(), false) },
            "list_windows" => Described { kind: Kind::Desktop, ..d(Icon::AppWindow, "Listing windows", "Listed the windows", String::new(), false) },
            "ui_elements" => Described { kind: Kind::Desktop, ..d(Icon::LayoutList, "Reading the window", "Read the window", String::new(), false) },
            "click" | "double_click" => Described {
                kind: Kind::Desktop,
                ..d(
                    Icon::MousePointer,
                    if self.name == "click" { "Clicking" } else { "Double-clicking" },
                    if self.name == "click" { "Clicked" } else { "Double-clicked" },
                    self.arg("target").map(str::to_owned).unwrap_or_else(|| at(self)),
                    false,
                )
            },
            "drag" => Described { kind: Kind::Desktop, ..d(Icon::Move, "Dragging", "Dragged", String::new(), false) },
            "scroll" => Described { kind: Kind::Desktop, ..d(Icon::ArrowUpDown, "Scrolling", "Scrolled", String::new(), false) },
            "type_text" => Described { kind: Kind::Desktop, ..d(Icon::Keyboard, "Typing", "Typed", self.arg("text").unwrap_or("").to_owned(), true) },
            "key" => Described { kind: Kind::Desktop, ..d(Icon::Keyboard, "Pressing", "Pressed", self.arg("keys").unwrap_or("").to_owned(), true) },
            "open_app" => Described { kind: Kind::Desktop, ..d(Icon::AppWindow, "Opening", "Opened", self.arg("name").unwrap_or("").to_owned(), false) },
            "wait" => d(
                Icon::Hourglass,
                "Waiting",
                "Waited",
                self.num("seconds").map(|s| format!("{s:.0} s")).unwrap_or_default(),
                false,
            ),
            other => d(Icon::Wrench, "Using", "Used", other.to_owned(), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn known_tools_are_put_in_words() {
        let c = ToolCall::new("run_command", json!({"command": "wpctl status", "purpose": "See the sound devices"}));
        let d = c.describe();
        assert_eq!((d.done, d.target.as_str(), d.code), ("Ran", "wpctl status", true));
        assert_eq!(d.purpose.as_deref(), Some("See the sound devices"));
        assert_eq!(d.kind, Kind::Command);
        assert_eq!(ToolCall::new("run_as_root", json!({"command": "nixos-rebuild switch"})).describe().kind, Kind::RootCommand);
        let edit = ToolCall::new("edit_file", json!({"path": "/home/u/a.conf", "old": "a = 1\n", "new": "a = 2\n"})).describe();
        let Kind::FileChange { path, diff } = edit.kind else { panic!() };
        assert_eq!(path, "/home/u/a.conf");
        assert!(diff.contains("-a = 1") && diff.contains("+a = 2"), "{diff}");
        // A tool the window doesn't know still shows, by its name.
        assert_eq!(ToolCall::new("frobnicate", json!({})).describe().target, "frobnicate");
    }
}
