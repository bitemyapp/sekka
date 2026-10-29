//! The window's side of the reader: start `aizome-nisshi-helper` as you,
//! or through pkexec as root (polkit asks for an administrator's password
//! in Aizome's own dialog, and remembers it for a few minutes), send it
//! requests, and turn its answers into [`Event`]s on a thread of its own,
//! where journal entries are parsed, so the window only takes them in.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::entry::{self, Entry, Interner};
use crate::protocol::{BootInfo, FileInfo, Frame, Op, Ready, Request};
use crate::time::Clock;

/// The polkit action the helper runs under as root.
pub const ACTION: &str = "org.aizome.Nisshi.read-system-logs";

/// What the reader says.
#[derive(Clone, Debug)]
pub enum Event {
    Ready(Ready),
    /// Entries in the order the request reads them (newest first, unless
    /// it follows).
    Entries {
        id: u64,
        entries: Vec<Entry>,
    },
    /// Every field of one entry, in journalctl's order.
    Fields {
        id: u64,
        fields: Vec<(String, String)>,
    },
    Boots {
        id: u64,
        boots: Vec<BootInfo>,
    },
    Files {
        id: u64,
        files: Vec<FileInfo>,
    },
    Done {
        id: u64,
    },
    Failed {
        id: u64,
        why: String,
    },
    /// The reader stopped, or never started.
    Gone(Gone),
}

/// Why the reader isn't there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gone {
    /// The password dialog was dismissed.
    Cancelled,
    /// polkit said no.
    Denied,
    /// No authentication agent answered: the dialog couldn't be shown.
    Unavailable,
    Failed(String),
}

impl Gone {
    pub fn words(&self) -> String {
        match self {
            Gone::Cancelled => "Unlocking was cancelled.".into(),
            Gone::Denied => "An administrator's permission was refused.".into(),
            Gone::Unavailable => "The password dialog isn't available right now.".into(),
            Gone::Failed(why) => why.clone(),
        }
    }
}

/// What an id's answers are.
#[derive(Clone)]
enum Kind {
    Entries,
    Fields,
    Boots,
    Files,
    /// A log file's lines, as entries from `source`.
    Lines(Arc<str>),
}

type Kinds = Arc<Mutex<HashMap<u64, Kind>>>;

/// A running reader.
pub struct Client {
    pub root: bool,
    stdin: Mutex<Option<ChildStdin>>,
    kinds: Kinds,
    next: u64,
}

impl Client {
    /// Start the reader: as root through pkexec, or as you. `on` hears its
    /// events, from a thread of the client's.
    pub fn start(root: bool, clock: Clock, on: impl Fn(Event) + Send + 'static) -> Client {
        let kinds: Kinds = Arc::default();
        let spawned = command(root).and_then(|mut c| {
            c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())
        });
        let mut child = match spawned {
            Ok(c) => c,
            Err(why) => {
                on(Event::Gone(Gone::Failed(format!("Nisshi's reader couldn't start: {why}"))));
                return Client { root, stdin: Mutex::new(None), kinds, next: 1 };
            }
        };
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let k = kinds.clone();
        let mut c = child;
        let _ = std::thread::Builder::new().name("nisshi-reader".into()).spawn(move || {
            let complaint = std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(e) = stderr {
                    let _ = e.take(16 << 10).read_to_string(&mut s);
                }
                s
            });
            let ready = match stdout {
                Some(out) => read(out, &k, &clock, &on),
                None => false,
            };
            let status = Some(c.wait());
            let complaint = complaint.join().unwrap_or_default();
            let code = status.and_then(|s| s.ok()).and_then(|s| s.code());
            on(Event::Gone(if ready {
                Gone::Failed(match complaint.trim() {
                    "" => "Nisshi's reader stopped.".into(),
                    why => format!("Nisshi's reader stopped: {why}"),
                })
            } else {
                refusal(code, &complaint, root)
            }));
        });
        Client { root, stdin: Mutex::new(stdin), kinds, next: 1 }
    }

    fn send(&mut self, op: Op, kind: Kind) -> u64 {
        let id = self.next;
        self.next += 1;
        self.kinds.lock().unwrap().insert(id, kind);
        self.write(&Request { id, op });
        id
    }

    fn write(&self, r: &Request) {
        let mut stdin = self.stdin.lock().unwrap();
        if let Some(s) = stdin.as_mut() {
            let mut text = serde_json::to_string(r).unwrap_or_default();
            text.push('\n');
            if s.write_all(text.as_bytes()).and_then(|()| s.flush()).is_err() {
                *stdin = None;
            }
        }
    }

    pub fn query(&mut self, q: crate::query::Query) -> u64 {
        self.send(Op::Query(q), Kind::Entries)
    }

    pub fn fields(&mut self, cursor: String) -> u64 {
        self.send(Op::Entry { cursor }, Kind::Fields)
    }

    pub fn boots(&mut self) -> u64 {
        self.send(Op::Boots, Kind::Boots)
    }

    pub fn files(&mut self) -> u64 {
        self.send(Op::Files, Kind::Files)
    }

    pub fn read(&mut self, path: String, max: u64) -> u64 {
        let name = Path::new(&path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        self.send(Op::Read { path, max }, Kind::Lines(name.into()))
    }

    pub fn cancel(&mut self, id: u64) {
        if self.kinds.lock().unwrap().remove(&id).is_some() {
            self.write(&Request { id, op: Op::Cancel });
        }
    }

    /// Stop the reader (closing its input ends it and every read it runs).
    pub fn stop(&mut self) {
        *self.stdin.lock().unwrap() = None;
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Its input closed, the helper stops its reads (a journalctl
        // following the journal would otherwise run on) and ends. Not
        // killed: it would leave its reads behind.
        self.stop();
    }
}

/// Read the helper's answers until it ends. Whether it said it was ready.
fn read(out: impl Read, kinds: &Kinds, clock: &Clock, on: &impl Fn(Event)) -> bool {
    let mut reader = BufReader::with_capacity(256 << 10, out);
    let mut intern = Interner::default();
    let mut ready = false;
    // Entries and lines gathered by id, sent once the helper has nothing
    // more ready (or a bundle is large).
    let mut pending: Vec<(u64, Vec<Entry>)> = Vec::new();
    let mut files: HashMap<u64, Vec<FileInfo>> = HashMap::new();
    let mut last_time: HashMap<u64, i64> = HashMap::new();
    let mut buf = Vec::new();
    let flush = |pending: &mut Vec<(u64, Vec<Entry>)>| {
        for (id, entries) in pending.drain(..) {
            on(Event::Entries { id, entries });
        }
    };
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let Some(frame) = Frame::parse(text.trim_end_matches(['\n', '\r'])) else { continue };
        let kind = |id: u64| kinds.lock().unwrap().get(&id).cloned();
        match frame {
            Frame::Ready(json) => {
                ready = true;
                on(Event::Ready(serde_json::from_str(json).unwrap_or_default()));
            }
            Frame::Entry(id, json) => match kind(id) {
                Some(Kind::Fields) => on(Event::Fields { id, fields: all_fields(json) }),
                Some(_) => {
                    if let Some(e) = entry::parse(json, &mut intern) {
                        push(&mut pending, id, e);
                    }
                }
                None => {}
            },
            Frame::Text(id, line) => {
                if let Some(Kind::Lines(source)) = kind(id) {
                    let prev = last_time.get(&id).copied().unwrap_or(0);
                    let e = entry::from_line(line, &source, prev, clock);
                    last_time.insert(id, e.time);
                    push(&mut pending, id, e);
                }
            }
            Frame::Boots(id, json) => {
                flush(&mut pending);
                on(Event::Boots { id, boots: serde_json::from_str(json).unwrap_or_default() });
            }
            Frame::File(id, json) => {
                if let Ok(f) = serde_json::from_str(json) {
                    files.entry(id).or_default().push(f);
                }
            }
            Frame::Done(id) => {
                flush(&mut pending);
                if let Some(Kind::Files) = kind(id) {
                    on(Event::Files { id, files: files.remove(&id).unwrap_or_default() });
                }
                last_time.remove(&id);
                kinds.lock().unwrap().remove(&id);
                on(Event::Done { id });
            }
            Frame::Failed(id, why) => {
                flush(&mut pending);
                files.remove(&id);
                kinds.lock().unwrap().remove(&id);
                on(Event::Failed { id, why: why.to_owned() });
            }
        }
        if reader.buffer().is_empty() || pending.iter().any(|(_, v)| v.len() >= 4096) {
            flush(&mut pending);
        }
    }
    flush(&mut pending);
    ready
}

fn push(pending: &mut Vec<(u64, Vec<Entry>)>, id: u64, e: Entry) {
    match pending.iter_mut().find(|(i, _)| *i == id) {
        Some((_, v)) => v.push(e),
        None => pending.push((id, vec![e])),
    }
}

/// An entry's fields, in words: its own first, then journald's (`_…`),
/// then the journal's addresses (`__…`).
pub fn all_fields(json: &str) -> Vec<(String, String)> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let words = |v: &serde_json::Value| -> Vec<String> {
        use serde_json::Value;
        let bytes = |a: &[Value]| -> Option<String> {
            let b: Option<Vec<u8>> = a.iter().map(|n| n.as_u64().and_then(|n| u8::try_from(n).ok())).collect();
            b.map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) if a.iter().all(Value::is_number) => bytes(a).into_iter().collect(),
            Value::Array(a) => a
                .iter()
                .filter_map(|x| match x {
                    Value::String(s) => Some(s.clone()),
                    Value::Array(b) => bytes(b),
                    _ => None,
                })
                .collect(),
            Value::Null => vec!["(too large to show)".into()],
            other => vec![other.to_string()],
        }
    };
    let mut out: Vec<(String, String)> =
        map.iter().flat_map(|(k, v)| words(v).into_iter().map(move |w| (k.clone(), w))).collect();
    let rank = |k: &str| {
        if k == "MESSAGE" {
            0
        } else if k.starts_with("__") {
            3
        } else if k.starts_with('_') {
            2
        } else {
            1
        }
    };
    out.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0)));
    out
}

/// journalctl's words for a journal with nothing the reader may read (a
/// new user's, before their own has a message), or none at all: an empty
/// read, not a failure.
pub fn nothing_to_read(why: &str) -> bool {
    let why = why.trim();
    why.starts_with("No journal files were opened due to insufficient permissions")
        || why.starts_with("No journal files were found")
}

/// pkexec's refusal, from its exit status and its whole complaint (in C's
/// locale), before it ran the helper. Anything else is a failure as it is.
fn refusal(code: Option<i32>, complaint: &str, root: bool) -> Gone {
    if !root {
        return Gone::Failed(match complaint.trim() {
            "" => "Nisshi's reader couldn't start.".into(),
            why => format!("Nisshi's reader couldn't start: {why}"),
        });
    }
    match (code, complaint.trim()) {
        (Some(126), "Error executing command as another user: Request dismissed") => Gone::Cancelled,
        (Some(127), "Error executing command as another user: Not authorized\n\nThis incident has been reported.") => {
            Gone::Denied
        }
        (Some(127), "Error executing command as another user: No authentication agent found.") => Gone::Unavailable,
        (Some(127), d) if !d.contains('\n') && d.starts_with("Error checking for authorization ") => Gone::Unavailable,
        (Some(127), d) if !d.contains('\n') && d.starts_with("Error getting authority: ") => Gone::Unavailable,
        (_, "") => Gone::Failed("The system's logs couldn't be unlocked.".into()),
        (_, why) => Gone::Failed(format!("The system's logs couldn't be unlocked: {why}")),
    }
}

/// The helper beside this program: `../libexec` in the package (the path
/// the polkit action names), or the same folder in a build.
pub fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NISSHI_HELPER") {
        return Some(p.into());
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("../libexec/aizome-nisshi-helper"), dir.join("aizome-nisshi-helper")]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}

fn command(root: bool) -> Result<Command, String> {
    let helper = helper_path().ok_or("aizome-nisshi-helper isn't installed beside Nisshi")?;
    if !root {
        return Ok(Command::new(helper));
    }
    // On NixOS only the wrapper in /run/wrappers is setuid; the polkit
    // package's own pkexec, which can come first in the search path,
    // refuses to run. There's no terminal to ask on: the session's agent
    // asks, or pkexec says it can't.
    // A test names a stand-in (the window runs as you: this changes
    // nothing polkit allows).
    let named = std::env::var_os("NISSHI_PKEXEC").map(PathBuf::from);
    let wrapper = Path::new("/run/wrappers/bin/pkexec");
    let mut c = match &named {
        Some(p) => Command::new(p),
        None => Command::new(if wrapper.exists() { wrapper } else { Path::new("pkexec") }),
    };
    c.arg("--disable-internal-agent").arg(helper).env("LC_ALL", "C");
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkexecs_refusals_in_its_words() {
        assert_eq!(
            refusal(Some(126), "Error executing command as another user: Request dismissed\n", true),
            Gone::Cancelled
        );
        assert_eq!(
            refusal(
                Some(127),
                "Error executing command as another user: Not authorized\n\nThis incident has been reported.\n",
                true
            ),
            Gone::Denied
        );
        assert_eq!(
            refusal(Some(127), "Error executing command as another user: No authentication agent found.\n", true),
            Gone::Unavailable
        );
        // Only the whole of a known complaint counts.
        assert!(matches!(refusal(Some(127), "something else", true), Gone::Failed(_)));
        assert!(matches!(refusal(Some(126), "Request dismissed", false), Gone::Failed(_)));
    }

    #[test]
    fn fields_in_order() {
        let f = all_fields(
            r#"{"_PID":"1","MESSAGE":"hi","__CURSOR":"c","CODE_LINE":"5","TAGS":["a","b"],"BIN":[104,105]}"#,
        );
        let keys: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["MESSAGE", "BIN", "CODE_LINE", "TAGS", "TAGS", "_PID", "__CURSOR"]);
        assert_eq!(f[1].1, "hi");
    }
}
