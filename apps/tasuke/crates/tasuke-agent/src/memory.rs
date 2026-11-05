//! Memory: short notes the assistant keeps between conversations, in
//! `memory.toml` (`[[note]] id, text, created`). The person sees them in
//! the Memory list and can remove any; the prompt frames them as notes,
//! not instructions.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ids::{NoteId, Timestamp};
use crate::options::DataDir;

pub const MAX_NOTES: usize = 40;
/// The most characters of notes the prompt carries (newest kept).
pub const MAX_PROMPT_CHARS: usize = 4000;
pub const MAX_NOTE_CHARS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: NoteId,
    pub text: String,
    pub created: Timestamp,
}

#[derive(Debug)]
pub enum MemoryError {
    Io(std::io::Error),
    /// The file is there but isn't memory TOML; it is left alone.
    Corrupt(String),
    Empty,
    TooLong {
        chars: usize,
    },
    Full,
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::Io(e) => write!(f, "{e}"),
            MemoryError::Corrupt(e) => write!(f, "the memory file can't be read: {e}"),
            MemoryError::Empty => write!(f, "the note is empty"),
            MemoryError::TooLong { chars } => {
                write!(f, "the note is {chars} characters; notes are at most {MAX_NOTE_CHARS}")
            }
            MemoryError::Full => write!(f, "memory is full ({MAX_NOTES} notes); forget one first"),
        }
    }
}

impl std::error::Error for MemoryError {}

impl From<std::io::Error> for MemoryError {
    fn from(e: std::io::Error) -> Self {
        MemoryError::Io(e)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default, rename = "note")]
    notes: Vec<Note>,
}

#[derive(Clone, Debug)]
pub struct Memory {
    path: PathBuf,
    notes: Vec<Note>,
}

impl Memory {
    pub fn load(dir: &DataDir) -> Result<Memory, MemoryError> {
        Memory::load_from(dir.memory_file())
    }

    /// No file yet is an empty memory.
    pub fn load_from(path: impl Into<PathBuf>) -> Result<Memory, MemoryError> {
        let path = path.into();
        let notes = match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str::<File>(&s).map_err(|e| MemoryError::Corrupt(e.to_string()))?.notes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Memory { path, notes })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Oldest first.
    pub fn list(&self) -> &[Note] {
        &self.notes
    }

    pub fn add(&mut self, text: &str) -> Result<NoteId, MemoryError> {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err(MemoryError::Empty);
        }
        let chars = text.chars().count();
        if chars > MAX_NOTE_CHARS {
            return Err(MemoryError::TooLong { chars });
        }
        if self.notes.len() >= MAX_NOTES {
            return Err(MemoryError::Full);
        }
        if let Some(same) = self.notes.iter().find(|n| n.text == text) {
            return Ok(same.id.clone());
        }
        let next = self.notes.iter().filter_map(|n| n.id.as_str().strip_prefix('m')?.parse::<u32>().ok()).max();
        let id = NoteId::new(format!("m{}", next.map_or(1, |n| n + 1)));
        self.notes.push(Note { id: id.clone(), text, created: Timestamp::now() });
        if let Err(e) = self.save() {
            self.notes.pop();
            return Err(e);
        }
        Ok(id)
    }

    /// Whether there was such a note.
    pub fn remove(&mut self, id: &NoteId) -> Result<bool, MemoryError> {
        let Some(i) = self.notes.iter().position(|n| &n.id == id) else { return Ok(false) };
        let note = self.notes.remove(i);
        if let Err(e) = self.save() {
            self.notes.insert(i, note);
            return Err(e);
        }
        Ok(true)
    }

    /// Write the file whole, atomically: a temporary beside it, synced, renamed over it.
    fn save(&self) -> Result<(), MemoryError> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let text =
            toml::to_string(&File { notes: self.notes.clone() }).map_err(|e| MemoryError::Corrupt(e.to_string()))?;
        let tmp = dir.join(format!(".memory.toml.{}", std::process::id()));
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &self.path)?;
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    }

    /// The notes as the system prompt carries them: newest first until the
    /// character limit, then shown oldest first. Empty when there are none.
    pub fn prompt_block(&self) -> String {
        let mut used = 0;
        let mut kept = Vec::new();
        for n in self.notes.iter().rev() {
            let line = format!("- [{}] {}", n.id, n.text);
            if used + line.len() > MAX_PROMPT_CHARS {
                break;
            }
            used += line.len() + 1;
            kept.push(line);
        }
        kept.reverse();
        kept.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("tasuke-memory-{}", crate::ids::random_hex(4)));
        d.join("memory.toml")
    }

    #[test]
    fn add_list_remove_persist() {
        let p = temp();
        let mut m = Memory::load_from(&p).unwrap();
        assert!(m.list().is_empty());
        let a = m.add("  The user's   printer is a Brother HL-L2350DW ").unwrap();
        let b = m.add("Prefers the Night theme").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("m1", "m2"));
        assert_eq!(m.add("Prefers the Night theme").unwrap(), b, "a repeat is the same note");
        let again = Memory::load_from(&p).unwrap();
        assert_eq!(again.list().len(), 2);
        assert_eq!(again.list()[0].text, "The user's printer is a Brother HL-L2350DW");
        let mut m = again;
        assert!(m.remove(&a).unwrap());
        assert!(!m.remove(&a).unwrap());
        assert_eq!(m.add("third").unwrap().as_str(), "m3");
        let m = Memory::load_from(&p).unwrap();
        assert_eq!(m.list().iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["m2", "m3"]);
        assert_eq!(m.prompt_block(), "- [m2] Prefers the Night theme\n- [m3] third");
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn limits() {
        let p = temp();
        let mut m = Memory::load_from(&p).unwrap();
        assert!(matches!(m.add("   "), Err(MemoryError::Empty)));
        assert!(matches!(m.add(&"x".repeat(MAX_NOTE_CHARS + 1)), Err(MemoryError::TooLong { .. })));
        for i in 0..MAX_NOTES {
            m.add(&format!("note {i} {}", "y".repeat(200))).unwrap();
        }
        assert!(matches!(m.add("one more"), Err(MemoryError::Full)));
        let block = m.prompt_block();
        assert!(block.len() <= MAX_PROMPT_CHARS);
        assert!(block.ends_with(&"y".repeat(200)), "the newest notes are kept");
        assert!(block.contains(&format!("[m{MAX_NOTES}]")));
        assert!(!block.contains("[m1]"));
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_corrupt_file_is_an_error_and_left_alone() {
        let p = temp();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "this is [not toml").unwrap();
        assert!(matches!(Memory::load_from(&p), Err(MemoryError::Corrupt(_))));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "this is [not toml");
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
