//! Autosave: what's open and every unsaved change, kept beside the
//! documents in `$XDG_STATE_HOME/aizome/sumi/` so that nothing typed is
//! lost to a crash, a logout or Quit, and the documents come back as they
//! were.
//!
//! - `session.json` lists the open documents: their files (or that they're
//!   untitled), language, encoding, line endings and caret;
//! - `<id>.txt` holds a document's text while it has unsaved changes
//!   (UTF-8, `\n` line breaks), and goes once it's saved.
//!
//! Writes happen in order on a thread of their own, each to a temporary
//! file renamed into place, so the files are always whole.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};

use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::files::{Encoding, LineEnding, Stamp};

/// The default folder: `$XDG_STATE_HOME/aizome/sumi`.
pub fn default_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("aizome/sumi")
}

/// An open document, as the session keeps it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionDoc {
    pub id: String,
    pub path: Option<PathBuf>,
    /// "Untitled N" for a document without a file.
    pub untitled: u32,
    pub language: String,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    /// The selection, in characters: anchor, head.
    pub selection: (usize, usize),
    /// `<id>.txt` holds unsaved text.
    pub unsaved: bool,
    /// The file as it was when last read or written.
    pub stamp: Option<Stamp>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub documents: Vec<SessionDoc>,
}

enum Job {
    Text { id: String, rope: Rope },
    Remove { id: String },
    Session(String),
    Flush(Sender<()>),
}

/// Where autosaves go (nowhere for a store made with no folder).
pub struct Store {
    dir: Option<PathBuf>,
    tx: Option<Sender<Job>>,
}

fn write_atomic(path: &Path, write: impl FnOnce(&mut fs::File) -> std::io::Result<()>) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)?;
    write(&mut f)?;
    f.sync_data()?;
    drop(f);
    fs::rename(&tmp, path)
}

impl Store {
    pub fn new(dir: Option<PathBuf>) -> Store {
        let Some(d) = dir.clone() else { return Store { dir: None, tx: None } };
        let (tx, rx) = channel::<Job>();
        let spawned = std::thread::Builder::new().name("sumi-autosave".into()).spawn(move || {
            let _ = fs::create_dir_all(&d);
            for job in rx {
                let r = match job {
                    Job::Text { id, rope } => write_atomic(&d.join(format!("{id}.txt")), |f| {
                        let mut w = std::io::BufWriter::new(f);
                        for chunk in rope.chunks() {
                            w.write_all(chunk.as_bytes())?;
                        }
                        w.flush()
                    }),
                    Job::Remove { id } => match fs::remove_file(d.join(format!("{id}.txt"))) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                        _ => Ok(()),
                    },
                    Job::Session(json) => write_atomic(&d.join("session.json"), |f| f.write_all(json.as_bytes())),
                    Job::Flush(done) => {
                        let _ = done.send(());
                        Ok(())
                    }
                };
                if let Err(e) = r {
                    log::warn!("autosave in {}: {e}", d.display());
                }
            }
        });
        Store { dir, tx: spawned.ok().map(|_| tx) }
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn send(&self, job: Job) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }

    pub fn load_session(&self) -> Session {
        let Some(d) = &self.dir else { return Session::default() };
        fs::read_to_string(d.join("session.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// A document's unsaved text, if it has some.
    pub fn load_text(&self, id: &str) -> Option<String> {
        if id.is_empty() || id.contains('/') {
            return None;
        }
        fs::read_to_string(self.dir.as_ref()?.join(format!("{id}.txt"))).ok()
    }

    /// Keep `rope` as `id`'s unsaved text.
    pub fn write_text(&self, id: &str, rope: Rope) {
        self.send(Job::Text { id: id.to_owned(), rope });
    }

    /// `id` has nothing unsaved (it was saved, or closed without).
    pub fn remove_text(&self, id: &str) {
        self.send(Job::Remove { id: id.to_owned() });
    }

    pub fn write_session(&self, session: &Session) {
        if let Ok(json) = serde_json::to_string_pretty(session) {
            self.send(Job::Session(json));
        }
    }

    /// Wait until everything sent so far is on disk.
    pub fn flush(&self) {
        if let Some(tx) = &self.tx {
            let (done, wait) = channel();
            if tx.send(Job::Flush(done)).is_ok() {
                let _ = wait.recv_timeout(std::time::Duration::from_secs(10));
            }
        }
    }

    /// Unsaved texts no document in `session` claims (left by a crash in
    /// the middle of closing one): removed.
    pub fn tidy(&self, session: &Session) {
        let Some(d) = &self.dir else { return };
        let Ok(entries) = fs::read_dir(d) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".txt")
                && !session.documents.iter().any(|doc| doc.id == id)
            {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

/// A new document's id: unique enough, and safe as a file name.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!(
        "{:x}-{:x}-{:x}",
        now.as_secs(),
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed) + u64::from(now.subsec_nanos() % 4096)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_session_and_unsaved_text() {
        let dir = std::env::temp_dir().join(format!("sumi-autosave-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::new(Some(dir.clone()));
        let doc = SessionDoc {
            id: new_id(),
            untitled: 1,
            language: "markdown".into(),
            unsaved: true,
            selection: (3, 3),
            ..Default::default()
        };
        store.write_text(&doc.id, Rope::from_str("# Draft\n"));
        store.write_session(&Session { documents: vec![doc.clone()] });
        store.flush();
        let again = Store::new(Some(dir.clone()));
        let s = again.load_session();
        assert_eq!(s.documents, vec![doc.clone()]);
        assert_eq!(again.load_text(&doc.id).as_deref(), Some("# Draft\n"));
        fs::write(dir.join("stray.txt"), "x").unwrap();
        again.tidy(&s);
        assert!(!dir.join("stray.txt").exists());
        again.remove_text(&doc.id);
        again.flush();
        assert_eq!(again.load_text(&doc.id), None);
        assert_ne!(new_id(), new_id());
        let _ = fs::remove_dir_all(&dir);
    }
}
