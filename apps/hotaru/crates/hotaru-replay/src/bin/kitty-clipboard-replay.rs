//! Kitty clipboard protocol (OSC 5522) scenarios through hotaru-core, with
//! the embedder model `tools/kitty-clipboard-oracle.py` gives the pinned
//! libghostty-vt. It reports what the program is sent and what the embedder
//! is asked to answer, step by step.
//!
//! Usage: kitty-clipboard-replay --scenarios PATH [--chunk N]
//!
//! PATH holds a list of scenarios, or `upstream/kitty-clipboard-fixtures.json`.
//!
//! The embedder answers each request after the feed that made it, as
//! Hotaru's PTY worker does. Ghostty answers within its callback instead,
//! so a step's bytes match as long as a step completes at most one request.

use std::collections::VecDeque;
use std::sync::Arc;

use hotaru_core::kitty_clipboard::{Direction, Location, Request, Status};
use hotaru_core::{Terminal, TerminalEvent};
use serde_json::{Value, json};

type Error = Box<dyn std::error::Error>;
/// Typed representations, as a clipboard holds them.
type Contents = Vec<(String, Arc<[u8]>)>;

const OTP_LEN: usize = 22;
const OTP_ALPHABET: usize = 57;

fn main() {
    if let Err(error) = run() {
        eprintln!("kitty-clipboard-replay: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let (mut scenarios, mut chunk) = (None, 65536usize);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenarios" => scenarios = Some(args.next().ok_or("--scenarios requires a path")?),
            "--chunk" => chunk = args.next().ok_or("--chunk requires a number")?.parse()?,
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    let scenarios: Value =
        serde_json::from_slice(&std::fs::read(scenarios.ok_or("--scenarios is required")?)?)?;
    // A list of scenarios, or the oracle's fixtures holding them.
    let results = scenarios
        .get("cases")
        .unwrap_or(&scenarios)
        .as_array()
        .ok_or("scenarios must be a list")?
        .iter()
        .map(|scenario| Scenario::new(scenario).map(|scenario| scenario.run(chunk.max(1))))
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_writer(std::io::stdout().lock(), &results)?;
    Ok(())
}

struct Decision {
    result: String,
    remember: bool,
}

struct Scenario<'a> {
    case: &'a Value,
    clipboards: [Contents; 2],
    reads: VecDeque<Decision>,
    writes: VecDeque<Decision>,
    entropy: Vec<u8>,
    entropy_at: usize,
}

impl<'a> Scenario<'a> {
    fn new(case: &'a Value) -> Result<Self, Error> {
        let clipboard = |location: &str| -> Result<Contents, Error> {
            let held = case["clipboards"][location]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            held.iter()
                .map(|entry| {
                    Ok((
                        text(&entry[0])?.to_owned(),
                        Arc::from(unhex(text(&entry[1])?)?),
                    ))
                })
                .collect()
        };
        let decisions = |key: &str| -> VecDeque<Decision> {
            let list = case[key].as_array().map(Vec::as_slice).unwrap_or_default();
            list.iter()
                .map(|d| Decision {
                    result: d["result"].as_str().unwrap_or("success").to_owned(),
                    remember: d["remember"].as_bool().unwrap_or(false),
                })
                .collect()
        };
        Ok(Self {
            case,
            clipboards: [clipboard("standard")?, clipboard("primary")?],
            reads: decisions("reads"),
            writes: decisions("writes"),
            entropy: unhex(case["entropy"].as_str().unwrap_or(""))?,
            entropy_at: 0,
        })
    }

    fn run(mut self, chunk: usize) -> Value {
        let mut terminal = Terminal::new(80, 24, 0);
        if let Some(limit) = self.case["write_limit"].as_u64() {
            terminal.set_kitty_clipboard_write_limit(limit as usize);
        }
        let mut steps = Vec::new();
        for item in self.case["steps"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let (mut pty, mut requests) = (Vec::new(), Vec::new());
            if let Some(feed) = item["feed"].as_str() {
                for part in unhex(feed).unwrap_or_default().chunks(chunk) {
                    terminal.feed(part);
                    self.drain(&mut terminal, &mut pty, &mut requests);
                }
            } else {
                let paste = &item["paste"];
                let primary = paste["location"].as_str() != Some("standard");
                let mimes: Vec<String> = paste["mimes"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|mime| mime.as_str().map(str::to_owned))
                    .collect();
                // Only an event can be sent; Hotaru's text paste is the
                // application's, and the corpus pastes no text.
                let event = terminal
                    .modes()
                    .kitty_paste_events
                    .then(|| self.password_bytes())
                    .flatten()
                    .and_then(|random| terminal.kitty_paste_event(primary, &mimes, &random));
                requests.push(json!({"kind": "paste", "written": event.is_some()}));
                pty.extend(event.unwrap_or_default());
            }
            steps.push(json!({"pty": hex(&pty), "requests": requests}));
        }
        json!({"steps": steps})
    }

    /// Entropy as the pinned library draws it: blocks of twice the password
    /// length, until enough bytes pass rejection sampling.
    fn password_bytes(&mut self) -> Option<Vec<u8>> {
        let limit = 256 / OTP_ALPHABET * OTP_ALPHABET;
        let (mut taken, mut accepted) = (Vec::new(), 0);
        while accepted < OTP_LEN {
            let block = self
                .entropy
                .get(self.entropy_at..self.entropy_at + 2 * OTP_LEN)?;
            self.entropy_at += block.len();
            accepted += block
                .iter()
                .filter(|&&byte| usize::from(byte) < limit)
                .count();
            taken.extend_from_slice(block);
        }
        Some(taken)
    }

    fn drain(&mut self, terminal: &mut Terminal, pty: &mut Vec<u8>, requests: &mut Vec<Value>) {
        pty.extend(terminal.take_responses());
        for event in terminal.take_events() {
            let TerminalEvent::KittyClipboard(request) = event else {
                continue;
            };
            match request {
                Request::Read(read) => {
                    let at = usize::from(read.loc == Location::Primary);
                    requests.push(json!({
                        "kind": "read", "location": location(read.loc), "mimes": read.mimes, "list": read.list,
                        "name": read.name, "granted": read.granted, "can_remember": !read.pw.is_empty(),
                    }));
                    let decision = self.reads.pop_front().unwrap_or(Decision {
                        result: "success".into(),
                        remember: false,
                    });
                    if decision.result != "success" {
                        read.status(status(&decision.result), pty);
                        continue;
                    }
                    let held = &self.clipboards[at];
                    let served: Contents = read
                        .mimes
                        .iter()
                        .filter_map(|mime| {
                            held.iter().find(|(offered, _)| offered == mime).cloned()
                        })
                        .collect();
                    let available: Vec<String> =
                        held.iter().map(|(mime, _)| mime.clone()).collect();
                    if decision.remember && !read.pw.is_empty() {
                        terminal.kitty_clipboard_grant(&read.pw, Direction::Read);
                    }
                    read.success(&available, &served, pty);
                }
                Request::Write(write) => {
                    requests.push(json!({
                        "kind": "write", "location": location(write.loc),
                        "contents": write.contents.iter().map(|(mime, data)| json!([mime, hex(data)])).collect::<Vec<_>>(),
                        "name": write.name, "granted": write.granted, "can_remember": !write.pw.is_empty(),
                    }));
                    let decision = self.writes.pop_front().unwrap_or(Decision {
                        result: "success".into(),
                        remember: false,
                    });
                    if decision.result == "success" {
                        self.clipboards[usize::from(write.loc == Location::Primary)] =
                            write.contents.clone();
                        if decision.remember && !write.pw.is_empty() {
                            terminal.kitty_clipboard_grant(&write.pw, Direction::Write);
                        }
                    }
                    write.status(status(&decision.result), pty);
                }
            }
        }
        // Answers can bring nothing more from the terminal; read it all.
        pty.extend(terminal.take_responses());
    }
}

fn location(location: Location) -> &'static str {
    match location {
        Location::Standard => "standard",
        Location::Primary => "primary",
    }
}

fn status(result: &str) -> Status {
    match result {
        "success" => Status::Done,
        "denied" => Status::Eperm,
        "unsupported" => Status::Enosys,
        "busy" => Status::Ebusy,
        "invalid_data" => Status::Einval,
        _ => Status::Eio,
    }
}

fn text(value: &Value) -> Result<&str, Error> {
    value.as_str().ok_or_else(|| "expected a string".into())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, Error> {
    if !text.len().is_multiple_of(2) {
        return Err("odd hex".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|at| Ok(u8::from_str_radix(&text[at..at + 2], 16)?))
        .collect()
}
