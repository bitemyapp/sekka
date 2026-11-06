//! Conversations: `sessions/<id>.jsonl` under the data directory, append
//! only, one record per line. The first line says when it was created, the
//! first user line gives the title. Screenshots go beside it in `<id>.files/`.
//!
//! A crash can leave the last line cut off; reading skips it, and the next
//! append starts a fresh line.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::chat::Message;
use crate::event::Event;
use crate::ids::{SessionId, Timestamp, TurnNumber};
use crate::options::DataDir;

const FORMAT: u32 = 1;
const TITLE_CHARS: usize = 60;

/// One line of a session file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Record {
    Meta {
        format: u32,
        id: SessionId,
        created: Timestamp,
    },
    /// What the person typed (the model's copy, with the environment block, is a `Message`).
    User {
        turn: TurnNumber,
        text: String,
        at: Timestamp,
    },
    Event {
        turn: TurnNumber,
        event: Event,
    },
    /// A message of the model's conversation, in order.
    Message {
        message: Message,
    },
    /// The conversation was summarized: the history is now this note and
    /// the last `keep` messages before it.
    Summary {
        text: String,
        keep: usize,
    },
}

/// A transcript entry, for showing a conversation again.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    User { turn: TurnNumber, text: String, at: Timestamp },
    Event { turn: TurnNumber, event: Event },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    path: PathBuf,
    title: String,
    created: Timestamp,
    updated: Timestamp,
}

impl Session {
    /// A new, empty conversation (its file exists from now on).
    pub fn create(dir: &DataDir) -> io::Result<Session> {
        let sessions = dir.sessions();
        std::fs::create_dir_all(&sessions)?;
        let created = Timestamp::now();
        let id = SessionId::generate(created);
        let path = sessions.join(format!("{id}.jsonl"));
        let mut f = OpenOptions::new().write(true).create_new(true).open(&path)?;
        let line = serde_json::to_string(&Record::Meta { format: FORMAT, id: id.clone(), created })?;
        f.write_all(format!("{line}\n").as_bytes())?;
        f.sync_all()?;
        Ok(Session { id, path, title: String::new(), created, updated: created })
    }

    pub fn load(dir: &DataDir, id: &SessionId) -> io::Result<Session> {
        Session::open(dir.sessions().join(format!("{id}.jsonl")))
    }

    fn open(path: PathBuf) -> io::Result<Session> {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let id = SessionId::parse(stem).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a session"))?;
        let meta = std::fs::metadata(&path)?;
        let updated = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| Timestamp(d.as_secs()))
            .unwrap_or_default();
        let mut created = None;
        let mut title = String::new();
        // The head is enough: the meta line, then the first user line.
        for record in read_records(&path)?.into_iter().take(64) {
            match record {
                Record::Meta { created: c, .. } => created = Some(c),
                Record::User { text, .. } => {
                    title = make_title(&text);
                    break;
                }
                _ => {}
            }
        }
        Ok(Session { id, path, title, created: created.unwrap_or(updated), updated })
    }

    /// Every conversation, the most recently changed first.
    pub fn list(dir: &DataDir) -> io::Result<Vec<Session>> {
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(dir.sessions()) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e),
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("jsonl")
                && let Ok(s) = Session::open(p)
            {
                out.push(s);
            }
        }
        out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
        Ok(out)
    }

    pub fn delete(dir: &DataDir, id: &SessionId) -> io::Result<()> {
        let sessions = dir.sessions();
        let files = sessions.join(format!("{id}.files"));
        if files.is_dir() {
            std::fs::remove_dir_all(files)?;
        }
        std::fs::remove_file(sessions.join(format!("{id}.jsonl")))
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// The first request's first line, shortened; empty until there is one.
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn created(&self) -> Timestamp {
        self.created
    }

    pub fn updated(&self) -> Timestamp {
        self.updated
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where this conversation's screenshots go.
    pub fn files_dir(&self) -> PathBuf {
        self.path.with_extension("files")
    }

    /// What was said and done, to show the conversation again.
    pub fn transcript(&self) -> io::Result<Vec<Entry>> {
        Ok(read_records(&self.path)?
            .into_iter()
            .filter_map(|r| match r {
                Record::User { turn, text, at } => Some(Entry::User { turn, text, at }),
                Record::Event { turn, event } => Some(Entry::Event { turn, event }),
                _ => None,
            })
            .collect())
    }

    /// The model's conversation so far, and the last turn's number.
    pub(crate) fn history(&self) -> io::Result<(Vec<Message>, TurnNumber)> {
        let mut messages = Vec::new();
        let mut turn = TurnNumber(0);
        for r in read_records(&self.path)? {
            match r {
                Record::Message { message } => messages.push(message),
                Record::Summary { text, keep } => {
                    let keep = keep.min(messages.len());
                    let tail = messages.split_off(messages.len() - keep);
                    messages = vec![Message::user(text)];
                    messages.extend(tail);
                }
                Record::User { turn: t, .. } => turn = turn.max(t),
                _ => {}
            }
        }
        Ok((messages, turn))
    }
}

/// What earlier conversations say about `query`: for the best matches
/// (every word of the query counts, newest first on ties), when it was,
/// what the person asked, where the words matched, and the changes that
/// ran with their approval. At most `limit` characters, for the model.
pub fn search(dir: &DataDir, query: &str, exclude: Option<&SessionId>, limit: usize) -> String {
    let words: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '.')
        .filter(|w| w.len() > 2)
        .map(str::to_owned)
        .collect();
    if words.is_empty() {
        return "Give a few words to search for.".into();
    }
    let mut hits = Vec::new();
    for s in Session::list(dir).unwrap_or_default() {
        if exclude == Some(&s.id) {
            continue;
        }
        let Ok(records) = read_records(&s.path) else { continue };
        let mut asked = Vec::new();
        let mut said = String::new();
        let mut asking = std::collections::HashMap::new();
        let mut changed = Vec::new();
        for r in records {
            match r {
                Record::User { text, .. } => asked.push(text),
                Record::Event { event: Event::TextDelta { text }, .. } => said.push_str(&text),
                Record::Event { event: Event::ToolCall { id, call, needs }, .. } if needs.asks() => {
                    asking.insert(id, call.summary());
                }
                Record::Event { event: Event::ToolOutput { id, output }, .. } => {
                    if output.status == crate::event::ToolStatus::Done
                        && let Some(what) = asking.remove(&id)
                    {
                        changed.push(what);
                    }
                }
                _ => {}
            }
        }
        let text = format!("{}\n{}\n{}", asked.join("\n"), said, changed.join("\n"));
        let lower = text.to_lowercase();
        let score = words.iter().filter(|w| lower.contains(w.as_str())).count();
        if score == 0 {
            continue;
        }
        let at = words.iter().filter_map(|w| lower.find(w.as_str())).min().unwrap_or(0);
        hits.push((
            score,
            s.updated,
            s.title.clone(),
            asked.first().cloned().unwrap_or_default(),
            snippet(&text, at),
            changed,
        ));
    }
    if hits.is_empty() {
        return format!("No earlier conversation mentions {query:?}.");
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut out = String::new();
    for (_, updated, title, asked, found, changed) in hits.into_iter().take(5) {
        let mut entry =
            format!("{} — {}\n  asked: {}\n  matched: {}\n", day(updated), title, one_line(&asked, 160), found);
        if !changed.is_empty() {
            entry.push_str(&format!("  changes made: {}\n", one_line(&changed.join("; "), 240)));
        }
        if out.len() + entry.len() > limit {
            break;
        }
        out.push_str(&entry);
    }
    out
}

fn one_line(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// About 160 characters around byte `at` of `text`, on one line.
fn snippet(text: &str, at: usize) -> String {
    let mut start = at.saturating_sub(60);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + 100).min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    one_line(&text[start..end], 170)
}

/// A timestamp's local date, YYYY-MM-DD.
fn day(t: Timestamp) -> String {
    // SAFETY: localtime_r writes only into the local `tm`.
    unsafe {
        let secs = t.0 as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
    }
}

pub(crate) fn make_title(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= TITLE_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(TITLE_CHARS).collect();
    // At a word boundary when there is one in the second half.
    let at = cut.rfind(' ').filter(|&i| i > TITLE_CHARS / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end())
}

/// Every record that parses; a line cut off by a crash is skipped.
fn read_records(path: &Path) -> io::Result<Vec<Record>> {
    let f = File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).split(b'\n') {
        let line = line?;
        if let Ok(r) = serde_json::from_slice::<Record>(&line) {
            out.push(r);
        }
    }
    Ok(out)
}

/// Appends records to a session's file as they happen.
pub(crate) struct Writer {
    file: File,
}

impl Writer {
    pub(crate) fn open(session: &Session) -> io::Result<Writer> {
        let mut file = OpenOptions::new().read(true).append(true).open(&session.path)?;
        // A cut-off last line (a crash mid-write) must not swallow the next record.
        let len = file.metadata()?.len();
        if len > 0 {
            use std::io::{Read, Seek, SeekFrom};
            let mut last = [0u8];
            let mut r = File::open(&session.path)?;
            r.seek(SeekFrom::Start(len - 1))?;
            r.read_exact(&mut last)?;
            if last[0] != b'\n' {
                file.write_all(b"\n")?;
            }
        }
        Ok(Writer { file })
    }

    /// One line, in one write (the file is opened for appending).
    pub(crate) fn append(&mut self, record: &Record) -> io::Result<()> {
        let mut line = serde_json::to_vec(record)?;
        line.push(b'\n');
        self.file.write_all(&line)
    }

    pub(crate) fn sync(&mut self) -> io::Result<()> {
        self.file.sync_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{FinishReason, Usage};

    fn temp_dir() -> DataDir {
        DataDir::new(std::env::temp_dir().join(format!("tasuke-sessions-{}", crate::ids::random_hex(4))))
    }

    #[test]
    fn search_finds_what_was_asked_and_what_was_changed() {
        use crate::event::{Approval, ToolCall, ToolOutput};
        use crate::ids::CallId;
        let dir = temp_dir();
        let a = Session::create(&dir).unwrap();
        let mut w = Writer::open(&a).unwrap();
        let t = TurnNumber(1);
        w.append(&Record::User { turn: t, text: "My sound stopped working".into(), at: Timestamp(5) }).unwrap();
        let call = ToolCall::RunAsRoot {
            command: "systemctl unmask pipewire.socket".into(),
            purpose: String::new(),
            timeout_seconds: 60,
        };
        let id = CallId::new("c1");
        w.append(&Record::Event {
            turn: t,
            event: Event::ToolCall { id: id.clone(), call, needs: Approval::AskRoot { reason: "root".into() } },
        })
        .unwrap();
        w.append(&Record::Event { turn: t, event: Event::ToolOutput { id, output: ToolOutput::done("ok") } }).unwrap();
        w.append(&Record::Event {
            turn: t,
            event: Event::TextDelta { text: "PipeWire was masked; I unmasked it.".into() },
        })
        .unwrap();
        let b = Session::create(&dir).unwrap();
        let mut w = Writer::open(&b).unwrap();
        w.append(&Record::User { turn: t, text: "Set my time zone to Tokyo".into(), at: Timestamp(6) }).unwrap();
        let found = search(&dir, "sound pipewire", None, 1500);
        assert!(found.contains("asked: My sound stopped working"), "{found}");
        assert!(found.contains("changes made: Run as administrator systemctl unmask pipewire.socket"), "{found}");
        assert!(!found.contains("Tokyo"), "{found}");
        assert!(search(&dir, "pipewire", Some(a.id()), 1500).starts_with("No earlier conversation"));
        assert_eq!(search(&dir, "a", None, 1500), "Give a few words to search for.");
        std::fs::remove_dir_all(dir.path()).ok();
    }

    #[test]
    fn create_write_list_load_delete() {
        let dir = temp_dir();
        assert!(Session::list(&dir).unwrap().is_empty());
        let s = Session::create(&dir).unwrap();
        assert_eq!(s.title(), "");
        let mut w = Writer::open(&s).unwrap();
        let t = TurnNumber(1);
        w.append(&Record::User { turn: t, text: "My sound stopped working after the update".into(), at: Timestamp(5) })
            .unwrap();
        w.append(&Record::Message { message: Message::user("env\n\nMy sound stopped working") }).unwrap();
        w.append(&Record::Event { turn: t, event: Event::TextDelta { text: "Let me look.".into() } }).unwrap();
        w.append(&Record::Message { message: Message::assistant("Let me look.") }).unwrap();
        let finished =
            Event::TurnFinished { reason: FinishReason::Answered, usage: Usage::default(), decode_tps: None };
        w.append(&Record::Event { turn: t, event: finished.clone() }).unwrap();
        w.sync().unwrap();

        let loaded = Session::load(&dir, s.id()).unwrap();
        assert_eq!(loaded.title(), "My sound stopped working after the update");
        assert_eq!(loaded.created(), s.created());
        let (history, turn) = loaded.history().unwrap();
        assert_eq!(turn, TurnNumber(1));
        assert_eq!(history.len(), 2);
        let transcript = loaded.transcript().unwrap();
        assert_eq!(transcript.len(), 3);
        assert_eq!(transcript[2], Entry::Event { turn: t, event: finished });

        let second = Session::create(&dir).unwrap();
        let listed = Session::list(&dir).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|x| x.id() == second.id()));

        std::fs::create_dir_all(s.files_dir()).unwrap();
        std::fs::write(s.files_dir().join("shot-1.png"), b"png").unwrap();
        Session::delete(&dir, s.id()).unwrap();
        assert!(!s.files_dir().exists());
        assert_eq!(Session::list(&dir).unwrap().len(), 1);
        std::fs::remove_dir_all(dir.path()).unwrap();
    }

    #[test]
    fn a_cut_off_last_line_is_skipped_and_not_joined() {
        let dir = temp_dir();
        let s = Session::create(&dir).unwrap();
        let mut w = Writer::open(&s).unwrap();
        w.append(&Record::Message { message: Message::user("first") }).unwrap();
        drop(w);
        // A crash mid-line.
        let mut f = OpenOptions::new().append(true).open(s.path()).unwrap();
        f.write_all(br#"{"kind":"message","message":{"role":"assistant","content":"cut"#).unwrap();
        drop(f);
        assert_eq!(s.history().unwrap().0.len(), 1);
        let mut w = Writer::open(&s).unwrap();
        w.append(&Record::Message { message: Message::user("after") }).unwrap();
        let (h, _) = s.history().unwrap();
        assert_eq!(h.iter().map(|m| m.content.text()).collect::<Vec<_>>(), ["first", "after"]);
        std::fs::remove_dir_all(dir.path()).unwrap();
    }

    #[test]
    fn a_summary_replaces_what_came_before() {
        let dir = temp_dir();
        let s = Session::create(&dir).unwrap();
        let mut w = Writer::open(&s).unwrap();
        for i in 0..5 {
            w.append(&Record::Message { message: Message::user(format!("m{i}")) }).unwrap();
        }
        w.append(&Record::Summary { text: "Earlier: m0–m2".into(), keep: 2 }).unwrap();
        w.append(&Record::Message { message: Message::assistant("next") }).unwrap();
        let (h, _) = s.history().unwrap();
        assert_eq!(h.iter().map(|m| m.content.text()).collect::<Vec<_>>(), ["Earlier: m0–m2", "m3", "m4", "next"]);
        std::fs::remove_dir_all(dir.path()).unwrap();
    }

    #[test]
    fn titles() {
        assert_eq!(make_title("\n  hello there  \nsecond"), "hello there");
        let long = "How do I switch the keyboard layout to Dvorak and keep my shortcuts the same as before";
        let t = make_title(long);
        assert!(t.ends_with('…') && t.chars().count() <= TITLE_CHARS + 1, "{t}");
        assert!(t.starts_with("How do I switch the keyboard layout"));
    }
}
