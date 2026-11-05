//! The tools: what the model is offered (by mode), how its arguments are
//! checked into a [`ToolCall`], what each needs before it runs, and
//! running it.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::desktop::Desktop;
use crate::event::{Approval, MouseButton, ScreenPoint, ToolCall, ToolOutput, ToolStatus};
use crate::exec;
use crate::ids::{MarkId, NoteId};
use crate::memory::Memory;
use crate::options::{Options, home_dir};
use crate::policy;

const DEFAULT_TIMEOUT: u32 = 60;
const MAX_TIMEOUT: u32 = 300;
const READ_LINES: u64 = 400;

fn function(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type": "function", "function": {
        "name": name, "description": description,
        "parameters": {"type": "object", "properties": properties, "required": required}}})
}

/// The tools offered for these options, in a stable order (the prompt cache).
pub(crate) fn schemas(o: &Options, screen: (u32, u32)) -> Vec<Value> {
    let mut t = Vec::new();
    let purpose = json!({"type": "string",
        "description": "Why, in a few plain words the person sees, e.g. \"See why the sound service stopped\"."});
    let timeout = json!({"type": "integer", "description": "Seconds before it is stopped (default 60, at most 300)."});
    t.push(function(
        "run_command",
        if o.act {
            "Run a bash command as the user and get its exit status and output (stdout and stderr, cut when long). \
             No terminal and no input: use non-interactive flags (--no-pager). Commands that only look run at once; \
             anything that may change something waits for the person's approval."
        } else {
            "Run a bash command as the user and get its exit status and output (stdout and stderr, cut when long). \
             No terminal and no input: use non-interactive flags (--no-pager). Only commands that look run: Act is off."
        },
        json!({"command": {"type": "string", "description": "The bash command line."}, "purpose": purpose, "timeout": timeout}),
        &["command", "purpose"],
    ));
    if o.act {
        t.push(function(
            "run_as_root",
            "Run a bash command as root (administrator): for system services, system files and logs only root can \
             read. The person approves it and then enters their password.",
            json!({"command": {"type": "string", "description": "The bash command line."}, "purpose": purpose, "timeout": timeout}),
            &["command", "purpose"],
        ));
    }
    t.push(function(
        "read_file",
        "Read a text file, up to 400 lines at a time.",
        json!({"path": {"type": "string", "description": "Absolute, or relative to the home folder; ~ works."},
               "offset": {"type": "integer", "description": "The first line to read, from 1."},
               "limit": {"type": "integer", "description": "How many lines (at most 400)."}}),
        &["path"],
    ));
    if o.act {
        t.push(function(
            "write_file",
            "Create a file or replace its whole content. The person sees the change and approves it.",
            json!({"path": {"type": "string"}, "content": {"type": "string"}}),
            &["path", "content"],
        ));
        t.push(function(
            "edit_file",
            "Replace one exact piece of text in a file (it must occur once). The person sees the change and approves it.",
            json!({"path": {"type": "string"}, "old": {"type": "string", "description": "The exact text to replace."},
                   "new": {"type": "string", "description": "What replaces it."}}),
            &["path", "old", "new"],
        ));
    }
    if !o.skills.is_empty() {
        let names: Vec<&str> = o.skills.index().iter().map(|s| s.name.as_str()).collect();
        t.push(function(
            "load_skill",
            "Read one of the guides to this computer (listed in your instructions) before answering or acting in its area.",
            json!({"name": {"type": "string", "enum": names}}),
            &["name"],
        ));
    }
    t.push(function(
        "remember",
        "Keep a short note for future conversations: something the person told you about themselves, their \
         preferences or this computer. Never secrets.",
        json!({"text": {"type": "string"}}),
        &["text"],
    ));
    t.push(function("forget", "Remove a note by its id (like m3).", json!({"id": {"type": "string"}}), &["id"]));
    if o.desktop_control {
        let doc = o.coords.describe(screen.0, screen.1);
        let xy = json!({"x": {"type": "number", "description": doc}, "y": {"type": "number", "description": "See x."}});
        let with = |extra: Value| {
            let mut m = xy.as_object().cloned().unwrap_or_default();
            if let Value::Object(e) = extra {
                m.extend(e);
            }
            Value::Object(m)
        };
        if o.vision {
            t.push(function("screenshot", "Look at the screen again.", json!({}), &[]));
        }
        t.push(function(
            "ui_elements",
            "List the windows and the numbered controls on screen.",
            json!({}),
            &[],
        ));
        if o.act {
            if o.set_of_marks {
                t.push(function(
                    "click_element",
                    "Click a numbered control (its number from the screenshot or the list).",
                    json!({"mark": {"type": "integer"}}),
                    &["mark"],
                ));
                t.push(function(
                    "type_into",
                    "Click a numbered text field, then type text into it.",
                    json!({"mark": {"type": "integer"}, "text": {"type": "string"}}),
                    &["mark", "text"],
                ));
            }
            t.push(function(
                "click",
                "Move the pointer to a point and click.",
                with(json!({"button": {"type": "string", "enum": ["left", "right", "middle"]},
                            "count": {"type": "integer", "description": "2 for a double click."}})),
                &["x", "y"],
            ));
            t.push(function(
                "drag",
                "Press the left button at one point, move to another and release.",
                with(json!({"to_x": {"type": "number"}, "to_y": {"type": "number"}})),
                &["x", "y", "to_x", "to_y"],
            ));
            t.push(function(
                "scroll",
                "Scroll at a point: positive lines scroll down, negative up.",
                with(json!({"lines": {"type": "number"}})),
                &["x", "y", "lines"],
            ));
            t.push(function(
                "type_text",
                "Type text into whatever has the keyboard focus (ASCII only).",
                json!({"text": {"type": "string"}}),
                &["text"],
            ));
            t.push(function(
                "key",
                "Press a key or chord: return, escape, tab, backspace, up, down, f1…f12, or a chord like super+space \
                 (the command palette), super+q (quit an app), super+w (close a window), ctrl+a.",
                json!({"keys": {"type": "string"}}),
                &["keys"],
            ));
            t.push(function(
                "open_app",
                "Open an app by its name in the Toolchest (Settings, Files, Notes, Terminal, Logs, Calculator…).",
                json!({"name": {"type": "string"}}),
                &["name"],
            ));
        }
        t.push(function("wait", "Wait for the screen to settle (seconds, at most 10).", json!({"seconds": {"type": "number"}}), &["seconds"]));
    }
    t
}

/// The names of the tools offered.
pub(crate) fn names(schemas: &[Value]) -> Vec<String> {
    schemas.iter().filter_map(|s| s.pointer("/function/name")?.as_str().map(str::to_owned)).collect()
}

fn invalid(name: &str, arguments: &str, error: impl Into<String>) -> ToolCall {
    ToolCall::Invalid { name: name.to_owned(), arguments: arguments.to_owned(), error: error.into() }
}

fn text_arg(args: &Map<String, Value>, key: &str) -> Option<String> {
    match args.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn number_arg(args: &Map<String, Value>, key: &str) -> Option<f64> {
    match args.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `~`, `~/x` and relative paths, against the home folder.
pub(crate) fn expand_path(raw: &str, home: &Path) -> PathBuf {
    let raw = raw.trim();
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    let p = PathBuf::from(raw);
    if p.is_absolute() { p } else { home.join(p) }
}

/// Parse the model's call into a typed one (or an `Invalid` saying why).
pub(crate) fn parse(name: &str, arguments: &str, offered: &[String], desktop: &Desktop<'_>) -> ToolCall {
    if !offered.iter().any(|n| n == name) {
        return invalid(name, arguments, format!("there is no tool named {name:?}; the tools are: {}", offered.join(", ")));
    }
    let args: Map<String, Value> = if arguments.trim().is_empty() {
        Map::new()
    } else {
        match serde_json::from_str::<Value>(arguments) {
            Ok(Value::Object(m)) => m,
            // Some templates double-encode the arguments.
            Ok(Value::String(s)) => match serde_json::from_str::<Value>(&s) {
                Ok(Value::Object(m)) => m,
                _ => return invalid(name, arguments, "the arguments must be a JSON object"),
            },
            _ => return invalid(name, arguments, "the arguments must be a JSON object"),
        }
    };
    let need = |key: &str| text_arg(&args, key).ok_or_else(|| format!("{name} needs a text argument {key:?}"));
    let home = home_dir();
    let timeout = number_arg(&args, "timeout").map_or(DEFAULT_TIMEOUT, |t| (t as u32).clamp(1, MAX_TIMEOUT));
    let point = |kx: &str, ky: &str| -> Result<ScreenPoint, String> {
        match (number_arg(&args, kx), number_arg(&args, ky)) {
            (Some(x), Some(y)) => Ok(desktop.point(x, y)),
            _ => Err(format!("{name} needs numbers {kx:?} and {ky:?}")),
        }
    };
    let mark = |key: &str| -> Result<(MarkId, String, ScreenPoint), String> {
        let n = number_arg(&args, key).ok_or_else(|| format!("{name} needs the control's number, {key:?}"))?;
        let m = desktop
            .mark(MarkId(n as u32))
            .ok_or_else(|| format!("there is no control [{n}] on the screen now; look again with ui_elements"))?;
        Ok((m.id, m.label.clone(), m.area.center()))
    };
    let call = (|| -> Result<ToolCall, String> {
        Ok(match name {
            "run_command" | "run_as_root" => {
                let command = need("command")?;
                if command.trim().is_empty() {
                    return Err("the command is empty".into());
                }
                let purpose = text_arg(&args, "purpose").unwrap_or_default();
                if name == "run_command" {
                    ToolCall::RunCommand { command, purpose, timeout_seconds: timeout }
                } else {
                    ToolCall::RunAsRoot { command, purpose, timeout_seconds: timeout }
                }
            }
            "read_file" => ToolCall::ReadFile {
                path: expand_path(&need("path")?, &home),
                offset: number_arg(&args, "offset").map(|v| v.max(1.0) as u64),
                limit: number_arg(&args, "limit").map(|v| (v.max(1.0) as u64).min(READ_LINES)),
            },
            "write_file" => {
                let path = expand_path(&need("path")?, &home);
                let content = need("content")?;
                let old = std::fs::read_to_string(&path).unwrap_or_default();
                let diff = unified_diff(&old, &content, &path);
                ToolCall::WriteFile { path, content, diff }
            }
            "edit_file" => {
                let path = expand_path(&need("path")?, &home);
                let (old, new) = (need("old")?, need("new")?);
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                match text.matches(old.as_str()).count() {
                    0 => return Err(format!("the text to replace isn't in {}; read the file and copy it exactly", path.display())),
                    1 => {}
                    n => return Err(format!("the text to replace occurs {n} times in {}; include more of its surroundings", path.display())),
                }
                let diff = unified_diff(&text, &text.replacen(old.as_str(), &new, 1), &path);
                ToolCall::EditFile { path, old, new, diff }
            }
            "load_skill" => ToolCall::LoadSkill { name: need("name")? },
            "remember" => ToolCall::Remember { text: need("text")? },
            "forget" => ToolCall::Forget { id: NoteId::new(need("id")?.trim()) },
            "screenshot" => ToolCall::Screenshot,
            "list_windows" => ToolCall::ListWindows,
            "ui_elements" => ToolCall::UiElements,
            "click" => {
                let at = point("x", "y")?;
                let button = match text_arg(&args, "button").as_deref().map(str::to_lowercase).as_deref() {
                    Some("right") => MouseButton::Right,
                    Some("middle") => MouseButton::Middle,
                    _ => MouseButton::Left,
                };
                if number_arg(&args, "count").is_some_and(|c| c >= 2.0) {
                    ToolCall::DoubleClick { at }
                } else {
                    ToolCall::Click { at, button }
                }
            }
            "double_click" => ToolCall::DoubleClick { at: point("x", "y")? },
            "drag" => ToolCall::Drag { from: point("x", "y")?, to: point("to_x", "to_y")? },
            "scroll" => {
                let lines = number_arg(&args, "lines").or_else(|| number_arg(&args, "dy")).ok_or("scroll needs \"lines\"")?;
                ToolCall::Scroll { at: point("x", "y")?, lines: lines.clamp(-50.0, 50.0) }
            }
            "type_text" => ToolCall::TypeText { text: need("text")? },
            "key" => ToolCall::Key { keys: need("keys")? },
            "open_app" => ToolCall::OpenApp { name: need("name")? },
            "click_element" => {
                let (mark, label, at) = mark("mark")?;
                ToolCall::ClickElement { mark, label, at }
            }
            "type_into" => {
                let (mark, label, at) = mark("mark")?;
                ToolCall::TypeInto { mark, label, at, text: need("text")? }
            }
            "wait" => ToolCall::Wait { seconds: number_arg(&args, "seconds").unwrap_or(1.0).clamp(0.0, 10.0) },
            other => return Err(format!("{other} isn't a tool Tasuke knows")),
        })
    })();
    call.unwrap_or_else(|e| invalid(name, arguments, e))
}

/// What a call needs before it runs. `desktop_asked` says whether this turn
/// already had the person's yes for using the mouse and keyboard.
pub(crate) fn approval(call: &ToolCall, o: &Options, desktop_asked: bool) -> Approval {
    let home = home_dir();
    let act_off = || Approval::Refused { reason: "Act is off, so Tasuke only looks and explains.".into() };
    match call {
        ToolCall::RunCommand { command, .. } => match policy::classify_command(command, &home) {
            Approval::Ask { .. } if !o.act => act_off(),
            a => a,
        },
        ToolCall::RunAsRoot { command, .. } => {
            if !o.act {
                return act_off();
            }
            match policy::refused(command, &home) {
                Some(reason) => Approval::Refused { reason },
                None => Approval::AskRoot { reason: "It runs as administrator, with full control of the system.".into() },
            }
        }
        ToolCall::ReadFile { path, .. } => policy::classify_read(path, &home),
        ToolCall::WriteFile { path, .. } | ToolCall::EditFile { path, .. } => {
            if !o.act {
                act_off()
            } else if path.exists() {
                Approval::Ask { reason: "It changes a file.".into() }
            } else {
                Approval::Ask { reason: "It creates a file.".into() }
            }
        }
        c if c.is_desktop_action() => {
            if !o.desktop_control {
                Approval::Refused { reason: "Desktop control is off.".into() }
            } else if !o.act {
                act_off()
            } else if desktop_asked {
                Approval::None
            } else {
                Approval::Ask { reason: "Tasuke will use your mouse and keyboard for this request.".into() }
            }
        }
        _ => Approval::None,
    }
}

/// A call's result: what the person sees, what the model is told, and a
/// screenshot to show it.
pub(crate) struct Outcome {
    pub output: ToolOutput,
    pub for_model: String,
    pub image: Option<PathBuf>,
    pub memory_changed: bool,
}

impl Outcome {
    fn text(output: ToolOutput) -> Outcome {
        let for_model = match output.status {
            ToolStatus::Failed => format!("Error: {}", output.text),
            _ => output.text.clone(),
        };
        Outcome { output, for_model, image: None, memory_changed: false }
    }
}

const COMMAND_ENV: &[(&str, &str)] = &[
    ("PAGER", "cat"),
    ("SYSTEMD_PAGER", ""),
    ("GIT_PAGER", "cat"),
    ("SYSTEMD_COLORS", "0"),
    ("NO_COLOR", "1"),
    ("TERM", "dumb"),
];

fn run_output(run: &exec::Run, timeout: Duration) -> ToolOutput {
    let status = if run.timed_out {
        ToolStatus::TimedOut
    } else if run.cancelled {
        ToolStatus::Cancelled
    } else {
        ToolStatus::Done
    };
    ToolOutput { status, text: exec::describe(run, timeout), exit_code: run.exit_code, image: None, truncated: run.truncated }
}

/// Run an allowed call.
pub(crate) fn execute(call: &ToolCall, o: &Options, desktop: &mut Desktop<'_>, cancel: &AtomicBool) -> Outcome {
    match call {
        ToolCall::RunCommand { command, timeout_seconds, .. } => {
            let mut argv = o.user_command_prefix.clone();
            argv.extend(["bash".into(), "-c".into(), command.clone()]);
            let timeout = Duration::from_secs(u64::from(*timeout_seconds));
            match exec::run(&argv, COMMAND_ENV, None, timeout, cancel) {
                Ok(run) => Outcome::text(run_output(&run, timeout)),
                Err(e) => Outcome::text(ToolOutput::failed(format!("couldn't start bash: {e}"))),
            }
        }
        ToolCall::RunAsRoot { command, purpose, timeout_seconds } => {
            Outcome::text(run_as_root(command, purpose, *timeout_seconds, o, cancel))
        }
        ToolCall::ReadFile { path, offset, limit } => Outcome::text(read_file(path, *offset, *limit)),
        ToolCall::WriteFile { path, content, .. } => Outcome::text(match write_file(path, content) {
            Ok(()) => ToolOutput::done(format!("Wrote {} ({} bytes).", path.display(), content.len())),
            Err(e) => ToolOutput::failed(e),
        }),
        ToolCall::EditFile { path, old, new, .. } => Outcome::text(match std::fs::read_to_string(path) {
            Ok(text) if text.matches(old.as_str()).count() == 1 => match write_file(path, &text.replacen(old.as_str(), new, 1)) {
                Ok(()) => ToolOutput::done(format!("Edited {}.", path.display())),
                Err(e) => ToolOutput::failed(e),
            },
            Ok(_) => ToolOutput::failed(format!("{} changed since; read it again", path.display())),
            Err(e) => ToolOutput::failed(format!("{}: {e}", path.display())),
        }),
        ToolCall::LoadSkill { name } => Outcome::text(match o.skills.load(name) {
            Ok(body) => ToolOutput::done(body),
            Err(e) => ToolOutput::failed(e),
        }),
        ToolCall::Remember { text } => {
            let r = Memory::load(&o.data_dir).and_then(|mut m| m.add(text));
            match r {
                Ok(id) => Outcome {
                    memory_changed: true,
                    ..Outcome::text(ToolOutput::done(format!("Remembered as {id}.")))
                },
                Err(e) => Outcome::text(ToolOutput::failed(e.to_string())),
            }
        }
        ToolCall::Forget { id } => match Memory::load(&o.data_dir).and_then(|mut m| m.remove(id)) {
            Ok(true) => Outcome { memory_changed: true, ..Outcome::text(ToolOutput::done(format!("Forgot {id}."))) },
            Ok(false) => Outcome::text(ToolOutput::failed(format!("there is no note {id}"))),
            Err(e) => Outcome::text(ToolOutput::failed(e.to_string())),
        },
        ToolCall::Screenshot | ToolCall::UiElements | ToolCall::ListWindows => {
            let obs = desktop.observe();
            let mut output = ToolOutput::done(obs.text.clone());
            output.image = obs.image.clone();
            Outcome { output, for_model: obs.text, image: obs.image, memory_changed: false }
        }
        ToolCall::Wait { seconds } => {
            let until = std::time::Instant::now() + Duration::from_secs_f64(*seconds);
            while std::time::Instant::now() < until && !cancel.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            after_action(desktop, Ok("Waited.".into()))
        }
        ToolCall::Invalid { error, .. } => Outcome::text(ToolOutput::failed(error.clone())),
        action => {
            let done = match action {
                ToolCall::Click { at, button } => desktop.click(*at, *button, 1).map(|()| format!("Clicked at ({:.0}, {:.0}).", at.x, at.y)),
                ToolCall::DoubleClick { at } => desktop.click(*at, MouseButton::Left, 2).map(|()| format!("Double-clicked at ({:.0}, {:.0}).", at.x, at.y)),
                ToolCall::Drag { from, to } => desktop.drag(*from, *to).map(|()| "Dragged.".to_owned()),
                ToolCall::Scroll { at, lines } => desktop.scroll(*at, *lines).map(|()| format!("Scrolled {lines:+}.")),
                ToolCall::TypeText { text } => desktop.type_text(text).map(|()| format!("Typed {} characters.", text.chars().count())),
                ToolCall::Key { keys } => desktop.key(keys).map(|k| format!("Pressed {k}.")),
                ToolCall::OpenApp { name } => desktop.open_app(name).map(|n| {
                    std::thread::sleep(Duration::from_millis(1200));
                    format!("Opened {n}.")
                }),
                ToolCall::ClickElement { mark, label, at } => {
                    desktop.click(*at, MouseButton::Left, 1).map(|()| format!("Clicked [{mark}] \"{label}\"."))
                }
                ToolCall::TypeInto { mark, label, at, text } => desktop
                    .click(*at, MouseButton::Left, 1)
                    .and_then(|()| {
                        std::thread::sleep(Duration::from_millis(150));
                        desktop.type_text(text)
                    })
                    .map(|()| format!("Typed into [{mark}] \"{label}\".")),
                _ => Err("not a desktop action".into()),
            };
            after_action(desktop, done)
        }
    }
}

/// After an action: what it did, then a fresh look at the screen.
fn after_action(desktop: &mut Desktop<'_>, done: Result<String, String>) -> Outcome {
    match done {
        Err(e) => Outcome::text(ToolOutput::failed(e)),
        Ok(done) => {
            std::thread::sleep(Duration::from_millis(600));
            let obs = desktop.observe();
            let text = format!("{done}\n\n{}", obs.text);
            let mut output = ToolOutput::done(done);
            output.image = obs.image.clone();
            Outcome { output, for_model: text, image: obs.image, memory_changed: false }
        }
    }
}

fn run_as_root(command: &str, purpose: &str, timeout_seconds: u32, o: &Options, cancel: &AtomicBool) -> ToolOutput {
    let timeout = Duration::from_secs(u64::from(timeout_seconds));
    // SAFETY: geteuid can't fail.
    if unsafe { libc::geteuid() } == 0 {
        let argv = vec!["bash".to_owned(), "-c".to_owned(), command.to_owned()];
        return match exec::run(&argv, COMMAND_ENV, None, timeout, cancel) {
            Ok(run) => run_output(&run, timeout),
            Err(e) => ToolOutput::failed(format!("couldn't start bash: {e}")),
        };
    }
    let request = json!({"command": command, "purpose": purpose, "timeout_seconds": timeout_seconds}).to_string();
    let argv = vec!["pkexec".to_owned(), o.helper.to_string_lossy().into_owned(), "run".to_owned()];
    // The password dialog's time on top of the command's.
    let run = match exec::run(&argv, &[], Some(request.as_bytes()), timeout + Duration::from_secs(180), cancel) {
        Ok(run) => run,
        Err(e) => return ToolOutput::failed(format!("couldn't start pkexec: {e}")),
    };
    if run.cancelled {
        return ToolOutput::with_status(ToolStatus::Cancelled, "stopped: the person cancelled");
    }
    let reply: Option<Value> = run.output.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok());
    match (run.exit_code, reply) {
        (_, Some(v)) if v.get("output").is_some() => {
            let exit_code = v.get("exit_code").and_then(Value::as_i64).map(|c| c as i32);
            let r = exec::Run {
                exit_code,
                output: v.get("output").and_then(Value::as_str).unwrap_or("").to_owned(),
                truncated: v.get("truncated").and_then(Value::as_bool).unwrap_or(false),
                timed_out: v.get("timed_out").and_then(Value::as_bool).unwrap_or(false),
                cancelled: false,
            };
            run_output(&r, timeout)
        }
        (_, Some(v)) if v.get("error").is_some() => {
            ToolOutput::with_status(ToolStatus::Refused, v["error"].as_str().unwrap_or("refused").to_owned())
        }
        (Some(126), _) => ToolOutput::with_status(ToolStatus::Denied, "The person closed the password dialog; nothing ran."),
        (Some(127), _) => ToolOutput::with_status(
            ToolStatus::Denied,
            "The administrator password wasn't given or wasn't accepted; nothing ran.",
        ),
        _ => ToolOutput::failed(format!("tasuke-helper failed: {}", run.output.trim().chars().take(400).collect::<String>())),
    }
}

fn read_file(path: &Path, offset: Option<u64>, limit: Option<u64>) -> ToolOutput {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return ToolOutput::failed(format!("{}: {e}", path.display())),
    };
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return ToolOutput::done(format!("{} is a binary file ({} bytes).", path.display(), bytes.len()));
    }
    let text = String::from_utf8_lossy(&bytes);
    let total = text.lines().count() as u64;
    let start = offset.unwrap_or(1).max(1);
    let n = limit.unwrap_or(READ_LINES).min(READ_LINES);
    let mut out = String::new();
    let mut shown = 0;
    for line in text.lines().skip((start - 1) as usize).take(n as usize) {
        if out.len() + line.len() > exec::OUTPUT_CAP {
            break;
        }
        out.push_str(line);
        out.push('\n');
        shown += 1;
    }
    let end = start + shown - 1;
    let header = if total == 0 {
        format!("{} is empty.", path.display())
    } else if shown == 0 {
        format!("{} has {total} lines; none from line {start}.", path.display())
    } else if start == 1 && end >= total {
        format!("{} ({total} lines):", path.display())
    } else {
        format!("{} (lines {start}–{end} of {total}):", path.display())
    };
    let mut o = ToolOutput::done(format!("{header}\n{out}"));
    o.truncated = end < total;
    o
}

/// Write atomically, keeping an existing file's owner and mode; a new file
/// written as root belongs to its folder's owner (the eval's root writing
/// into a person's home).
fn write_file(path: &Path, content: &str) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let existing = std::fs::metadata(path).ok();
    let owner = existing.as_ref().map(|m| (m.uid(), m.gid())).or_else(|| {
        std::fs::metadata(dir).ok().map(|m| (m.uid(), m.gid()))
    });
    let tmp = dir.join(format!(".{}.tasuke-{}", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    std::fs::write(&tmp, content).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if let Some(m) = &existing {
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(m.mode() & 0o7777));
    }
    // SAFETY: geteuid can't fail.
    if unsafe { libc::geteuid() } == 0
        && let Some((uid, gid)) = owner
    {
        let _ = std::os::unix::fs::chown(&tmp, Some(uid), Some(gid));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn unified_diff(old: &str, new: &str, path: &Path) -> String {
    let name = path.display().to_string();
    similar::TextDiff::from_lines(old, new).unified_diff().context_radius(3).header(&name, &name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        let mut o = Options::new("http://x/v1", "m");
        o.skills = crate::skills::Skills::default();
        o
    }

    #[test]
    fn modes_decide_what_is_offered() {
        let mut o = opts();
        let n = names(&schemas(&o, (1440, 900)));
        assert!(n.contains(&"run_as_root".to_owned()) && n.contains(&"write_file".to_owned()));
        assert!(!n.contains(&"click".to_owned()));
        o.act = false;
        let n = names(&schemas(&o, (1440, 900)));
        assert!(!n.contains(&"run_as_root".to_owned()) && !n.contains(&"edit_file".to_owned()));
        assert!(n.contains(&"run_command".to_owned()));
        o.act = true;
        o.desktop_control = true;
        o.set_of_marks = true;
        let n = names(&schemas(&o, (1440, 900)));
        assert!(n.contains(&"click_element".to_owned()) && n.contains(&"ui_elements".to_owned()));
        assert!(!n.contains(&"screenshot".to_owned()), "no images for a model without vision");
    }

    #[test]
    fn arguments_are_checked() {
        let o = opts();
        let cancel = AtomicBool::new(false);
        let d = Desktop::new(&o, &cancel, std::env::temp_dir());
        let offered = names(&schemas(&o, (1440, 900)));
        let c = parse("run_command", r#"{"command":"ls","purpose":"look","timeout":9999}"#, &offered, &d);
        assert_eq!(c, ToolCall::RunCommand { command: "ls".into(), purpose: "look".into(), timeout_seconds: 300 });
        let c = parse("run_command", r#""{\"command\":\"ls\"}""#, &offered, &d);
        assert!(matches!(c, ToolCall::RunCommand { .. }), "double-encoded arguments: {c:?}");
        assert!(matches!(parse("run_command", "{}", &offered, &d), ToolCall::Invalid { .. }));
        assert!(matches!(parse("run_command", "[1]", &offered, &d), ToolCall::Invalid { .. }));
        assert!(matches!(parse("rm_everything", "{}", &offered, &d), ToolCall::Invalid { .. }));
        assert!(matches!(parse("click_element", r#"{"mark":3}"#, &offered, &d), ToolCall::Invalid { .. }));
        let home = home_dir();
        let c = parse("read_file", r#"{"path":"~/x.txt","limit":9000}"#, &offered, &d);
        assert_eq!(c, ToolCall::ReadFile { path: home.join("x.txt"), offset: None, limit: Some(400) });
    }

    #[test]
    fn edits_must_match_once() {
        let dir = std::env::temp_dir().join(format!("tasuke-edit-{}", crate::ids::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.toml");
        std::fs::write(&f, "theme = \"night\"\nx = 1\nx = 1\n").unwrap();
        let o = opts();
        let cancel = AtomicBool::new(false);
        let d = Desktop::new(&o, &cancel, dir.clone());
        let offered = names(&schemas(&o, (1, 1)));
        let args = |old: &str| json!({"path": f, "old": old, "new": "theme = \"day\""}).to_string();
        assert!(matches!(parse("edit_file", &args("x = 1"), &offered, &d), ToolCall::Invalid { .. }));
        assert!(matches!(parse("edit_file", &args("nope"), &offered, &d), ToolCall::Invalid { .. }));
        let call = parse("edit_file", &args("theme = \"night\""), &offered, &d);
        let ToolCall::EditFile { diff, .. } = &call else { panic!("{call:?}") };
        assert!(diff.contains("-theme = \"night\"") && diff.contains("+theme = \"day\""));
        let mut d = d;
        let out = execute(&call, &o, &mut d, &cancel);
        assert_eq!(out.output.status, ToolStatus::Done);
        assert!(std::fs::read_to_string(&f).unwrap().starts_with("theme = \"day\""));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn files_keep_their_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tasuke-write-{}", crate::ids::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.sh");
        std::fs::write(&f, "old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o750)).unwrap();
        write_file(&f, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new");
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o750);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reading_pages_and_spots_binaries() {
        let dir = std::env::temp_dir().join(format!("tasuke-read-{}", crate::ids::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("l.txt");
        std::fs::write(&f, (1..=1000).map(|i| format!("line {i}\n")).collect::<String>()).unwrap();
        let o = read_file(&f, Some(10), Some(5));
        assert!(o.text.contains("(lines 10–14 of 1000)") && o.text.contains("line 14\n") && !o.text.contains("line 15"));
        assert!(o.truncated);
        std::fs::write(&f, b"\x00\x01binary").unwrap();
        assert!(read_file(&f, None, None).text.contains("binary file"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn approvals_follow_the_modes() {
        let mut o = opts();
        let root = ToolCall::RunAsRoot { command: "systemctl restart x".into(), purpose: String::new(), timeout_seconds: 60 };
        assert!(matches!(approval(&root, &o, false), Approval::AskRoot { .. }));
        let click = ToolCall::Click { at: ScreenPoint { x: 1.0, y: 1.0 }, button: MouseButton::Left };
        assert!(matches!(approval(&click, &o, false), Approval::Refused { .. }));
        o.desktop_control = true;
        assert!(matches!(approval(&click, &o, false), Approval::Ask { .. }));
        assert_eq!(approval(&click, &o, true), Approval::None);
        o.act = false;
        assert!(matches!(approval(&root, &o, false), Approval::Refused { .. }));
        assert_eq!(approval(&ToolCall::LoadSkill { name: "x".into() }, &o, false), Approval::None);
    }
}
