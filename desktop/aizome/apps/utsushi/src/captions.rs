//! Opt-in Koe file captions: bounded in-memory lookahead and pipe backpressure.
//! Each playback discontinuity cancels the child and invalidates its generation.
//! No transcript, audio, model download or clipboard operation is performed here.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use aizome_media::{Cue, CueBody};
use serde::Deserialize;

pub const MAX_CUES: usize = 128;
const LOOKAHEAD: f64 = 90.0;
const MAX_LINE: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Captions,
    Turns,
    Lyrics,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Authored,
    Generated(Mode),
    Lyrics(PathBuf),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Ready,
    Cue {
        start: f64,
        end: f64,
        text: String,
        speaker: Option<String>,
    },
    Gap {
        message: String,
    },
    Error {
        message: String,
    },
    #[serde(skip)]
    Finished,
}

#[derive(Clone, Debug)]
pub struct Update {
    pub generation: u64,
    pub event: Event,
}

#[derive(Default)]
pub struct Timeline {
    pub generation: u64,
    pub cues: Vec<Cue>,
    next_id: u64,
}

impl Timeline {
    pub fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cues.clear();
    }

    pub fn prune(&mut self, position: f64) {
        self.cues.retain(|cue| cue.end > position);
    }

    pub fn wants_more(&self, position: f64) -> bool {
        self.cues.len() < MAX_CUES && self.cues.last().is_none_or(|cue| cue.start < position + LOOKAHEAD)
    }

    /// Returns false for stale/expired events. Never evicts a future cue:
    /// the caller stops draining the bounded pipe queue when lookahead is full.
    pub fn insert(&mut self, update: Update, position: f64, turns: bool) -> Result<bool, String> {
        if update.generation != self.generation {
            return Ok(false);
        }
        let Event::Cue { start, end, text, speaker } = update.event else { return Ok(false) };
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || text.len() > MAX_LINE as usize {
            return Err("Koe returned an invalid caption. Update Koe and retry live captions.".into());
        }
        if end <= position || text.trim().is_empty() {
            return Ok(false);
        }
        if self.cues.len() >= MAX_CUES {
            return Err("Caption lookahead is full. Restart live captions at the current position.".into());
        }
        let words = match speaker.filter(|s| turns && !s.trim().is_empty()) {
            Some(speaker) => format!("{speaker} (detected turn): {text}"),
            None => text,
        };
        self.next_id = self.next_id.wrapping_add(1);
        let cue = Cue { id: self.next_id, start, end, body: CueBody::Text { text: words, italic: false, top: false } };
        let at = self.cues.partition_point(|c| c.start <= start);
        self.cues.insert(at, cue);
        Ok(true)
    }
}

pub struct Job {
    child: Arc<Mutex<Child>>,
    receiver: Option<mpsc::Receiver<Update>>,
}

impl Job {
    pub fn start(path: &Path, start: f64, audio_track: usize, mode: Mode, generation: u64) -> Result<Self, String> {
        if !path.is_file() {
            return Err(
                "Live captions need a local media file. Save the media locally, then open it in Utsushi.".into()
            );
        }
        let mut command = Command::new("aizome-koe");
        command
            .args(["captions", "--json", "--input"])
            .arg(path)
            .arg("--start")
            .arg(format!("{:.6}", start.max(0.0)))
            .arg("--audio-track")
            .arg(audio_track.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if mode == Mode::Turns {
            command.arg("--diarize");
        }
        let mut child = command.spawn().map_err(|e| {
            format!("Cannot start Koe: {e}. Install or enable Koe, select an already-downloaded local model in Koe, then retry.")
        })?;
        let stdout = child.stdout.take().expect("piped caption stdout");
        let child = Arc::new(Mutex::new(child));
        let worker_child = child.clone();
        let (tx, receiver) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            let mut failed = false;
            loop {
                line.clear();
                let event = match reader.by_ref().take(MAX_LINE + 1).read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() as u64 <= MAX_LINE => match serde_json::from_str::<Event>(&line) {
                        Ok(event) => event,
                        Err(e) => {
                            Event::Error { message: format!("Cannot read Koe captions: {e}. Update Koe and retry.") }
                        }
                    },
                    Ok(_) => {
                        Event::Error { message: "Koe returned an oversized caption. Update Koe and retry.".into() }
                    }
                    Err(e) => Event::Error { message: format!("Cannot read Koe captions: {e}. Retry live captions.") },
                };
                failed = matches!(event, Event::Error { .. });
                if tx.send(Update { generation, event }).is_err() || failed {
                    let _ = worker_child.lock().unwrap_or_else(|e| e.into_inner()).kill();
                    break;
                }
            }
            // Never hold the child lock across a blocking wait: the UI must
            // be able to kill it immediately even if stdout closed early.
            let status = loop {
                let status = worker_child.lock().unwrap_or_else(|e| e.into_inner()).try_wait();
                match status {
                    Ok(Some(status)) => break Some(status),
                    Err(_) => break None,
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                }
            };
            if !failed && status.is_none_or(|s| !s.success()) {
                let _ = tx.send(Update {
                    generation,
                    event: Event::Error {
                        message: "Koe stopped unexpectedly. Check the local model in Koe, then retry live captions."
                            .into(),
                    },
                });
            }
            let _ = tx.send(Update { generation, event: Event::Finished });
        });
        Ok(Self { child, receiver: Some(receiver) })
    }

    pub fn try_recv(&self) -> Result<Update, mpsc::TryRecvError> {
        self.receiver.as_ref().expect("active caption receiver").try_recv()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Disconnect first so a worker blocked on its bounded queue can exit.
        self.receiver.take();
        let _ = self.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}
