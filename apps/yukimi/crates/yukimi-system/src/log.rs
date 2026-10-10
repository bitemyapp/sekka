// SPDX-License-Identifier: MIT OR Apache-2.0
//! Nix's progress, from its machine-readable log (`--log-format
//! internal-json`): each line `@nix {…}` starts or stops an activity
//! (downloading a path, building a derivation), reports a result for one
//! (progress, a build log line, a phase), or is a message.
//!
//! [`Progress`] folds those into what a person wants to see: how many
//! packages are downloading and building, how many bytes, what is happening
//! right now, and any errors, with the recent build log kept for when
//! something fails.
use std::collections::{HashMap, VecDeque};

use serde::Deserialize;
use yukimi_store::StorePath;

// Nix's activity types.
const COPY_PATH: u64 = 100;
const FILE_TRANSFER: u64 = 101;
const COPY_PATHS: u64 = 103;
const BUILDS: u64 = 104;
const BUILD: u64 = 105;
const FETCH_TREE: u64 = 112;

// Nix's result types.
const BUILD_LOG_LINE: u64 = 101;
const SET_PHASE: u64 = 104;
const PROGRESS: u64 = 105;
const SET_EXPECTED: u64 = 106;

/// How many build log lines to keep.
const LOG_LINES: usize = 400;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
enum Event {
    Start {
        id: u64,
        #[serde(rename = "type", default)]
        kind: u64,
        #[serde(default)]
        text: String,
        #[serde(default)]
        fields: Vec<serde_json::Value>,
    },
    Stop {
        id: u64,
    },
    Result {
        id: u64,
        #[serde(rename = "type")]
        kind: u64,
        #[serde(default)]
        fields: Vec<serde_json::Value>,
    },
    Msg {
        level: u8,
        msg: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counter {
    pub done: u64,
    pub expected: u64,
    pub running: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// 0 for errors, 1 for warnings, higher for information.
    pub level: u8,
    pub text: String,
}

#[derive(Debug)]
struct Activity {
    kind: u64,
    /// Nix's own description, which also names the stages of
    /// `yukimi-helper` (activities of type 0).
    text: String,
    /// The store path or URL it is about.
    subject: String,
    done: u64,
    expected: u64,
    phase: Option<String>,
}

/// Everything Nix has reported so far.
#[derive(Debug, Default)]
pub struct Progress {
    pub builds: Counter,
    /// Paths downloaded from a binary cache.
    pub downloads: Counter,
    /// Bytes downloaded, and the total expected.
    pub bytes: (u64, u64),
    pub messages: Vec<Message>,
    pub log: VecDeque<String>,
    /// How many lines have been logged in all, of which `log` keeps the
    /// last few: what a reader that saw `n` of them has yet to see is the
    /// last `logged - n`.
    pub logged: u64,
    activities: HashMap<u64, Activity>,
    expected_bytes: HashMap<u64, u64>,
    /// Activities started, most recent last, to say what is happening now.
    order: Vec<u64>,
}

/// Remove terminal colour codes from Nix's messages.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn number(value: Option<&serde_json::Value>) -> u64 {
    value.and_then(|v| v.as_u64()).unwrap_or(0)
}

/// The package a store path or derivation is, for a status line.
fn package(subject: &str) -> String {
    match StorePath::parse(subject) {
        Some(path) => {
            let name = path.name().trim_end_matches(".drv");
            name.to_owned()
        }
        None => subject.rsplit('/').next().unwrap_or(subject).to_owned(),
    }
}

impl Progress {
    /// Take one line of Nix's error output. Lines that are not progress
    /// events are kept as log lines. Returns whether anything changed.
    pub fn feed(&mut self, line: &str) -> bool {
        let Some(json) = line.strip_prefix("@nix ") else {
            if line.trim().is_empty() {
                return false;
            }
            self.push_log(strip_ansi(line));
            return true;
        };
        let Ok(event) = serde_json::from_str::<Event>(json) else {
            return false;
        };
        match event {
            Event::Start { id, kind, text, fields } => {
                let subject =
                    fields.first().and_then(|f| f.as_str()).map(str::to_owned).unwrap_or_else(|| text.clone());
                self.activities.insert(id, Activity { kind, text, subject, done: 0, expected: 0, phase: None });
                self.order.push(id);
            }
            Event::Stop { id } => {
                self.order.retain(|&o| o != id);
                if let Some(activity) = self.activities.remove(&id)
                    && activity.kind == COPY_PATH
                {
                    // A finished download counts fully.
                    self.bytes.0 = self.bytes.0.saturating_sub(activity.done) + activity.expected.max(activity.done);
                }
            }
            Event::Result { id, kind, fields } => match kind {
                PROGRESS => {
                    let counter = Counter {
                        done: number(fields.first()),
                        expected: number(fields.get(1)),
                        running: number(fields.get(2)),
                        failed: number(fields.get(3)),
                    };
                    let Some(activity) = self.activities.get_mut(&id) else {
                        return false;
                    };
                    match activity.kind {
                        BUILDS => self.builds = counter,
                        COPY_PATHS => self.downloads = counter,
                        COPY_PATH => {
                            self.bytes.0 = self.bytes.0.saturating_sub(activity.done) + counter.done;
                            activity.done = counter.done;
                            activity.expected = counter.expected;
                        }
                        _ => {
                            activity.done = counter.done;
                            activity.expected = counter.expected;
                        }
                    }
                }
                SET_EXPECTED => {
                    if number(fields.first()) == COPY_PATH {
                        self.expected_bytes.insert(id, number(fields.get(1)));
                        self.bytes.1 = self.expected_bytes.values().sum();
                    }
                }
                SET_PHASE => {
                    if let Some(activity) = self.activities.get_mut(&id) {
                        activity.phase = fields.first().and_then(|f| f.as_str()).map(str::to_owned);
                    }
                }
                BUILD_LOG_LINE => {
                    if let Some(line) = fields.first().and_then(|f| f.as_str()) {
                        self.push_log(strip_ansi(line));
                    }
                }
                _ => return false,
            },
            Event::Msg { level, msg } => {
                let text = strip_ansi(&msg);
                if level <= 1 {
                    self.messages.push(Message { level, text: text.clone() });
                }
                self.push_log(text);
            }
        }
        true
    }

    fn push_log(&mut self, line: String) {
        self.logged += 1;
        if self.log.len() == LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    /// The lines logged since a reader saw `seen` of them, as many as are
    /// still kept.
    pub fn since(&self, seen: u64) -> impl Iterator<Item = &str> {
        let new = (self.logged.saturating_sub(seen) as usize).min(self.log.len());
        self.log.iter().skip(self.log.len() - new).map(String::as_str)
    }

    /// Errors Nix reported.
    pub fn errors(&self) -> impl Iterator<Item = &str> {
        self.messages.iter().filter(|m| m.level == 0).map(|m| m.text.as_str())
    }

    /// What is happening right now, such as `Building firefox (installPhase)`
    /// or `Downloading glibc`.
    pub fn current(&self) -> Option<String> {
        self.order.iter().rev().find_map(|id| {
            let activity = self.activities.get(id)?;
            let name = package(&activity.subject);
            match activity.kind {
                BUILD => Some(match &activity.phase {
                    Some(phase) => format!("Building {name} ({phase})"),
                    None => format!("Building {name}"),
                }),
                COPY_PATH => Some(format!("Downloading {name}")),
                FILE_TRANSFER | FETCH_TREE => Some("Fetching sources".to_owned()),
                0 if !activity.text.is_empty() => Some(activity.text.clone()),
                _ => None,
            }
        })
    }

    /// One line for everything: `Downloading 34 of 120 (412 MB of 1.1 GB) ·
    /// building 1 of 3`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.downloads.expected > 0 {
            let mut part = format!("Downloaded {} of {}", self.downloads.done, self.downloads.expected);
            if self.bytes.1 > 0 {
                part.push_str(&format!(
                    " ({} of {})",
                    crate::human_size(self.bytes.0.min(self.bytes.1)),
                    crate::human_size(self.bytes.1)
                ));
            }
            parts.push(part);
        }
        if self.builds.expected > 0 {
            parts.push(format!("built {} of {}", self.builds.done, self.builds.expected));
        }
        let mut text = parts.join(" · ");
        if let Some(first) = text.get(..1) {
            text = first.to_uppercase() + &text[1..];
        }
        text
    }

    /// How far along, from 0 to 1, when Nix has said what to expect.
    pub fn fraction(&self) -> Option<f64> {
        let total = self.downloads.expected + self.builds.expected * 4;
        if total == 0 {
            return None;
        }
        let done = self.downloads.done + self.builds.done * 4;
        Some((done as f64 / total as f64).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdfghijklmnpqrsvwxyz";

    #[test]
    fn downloads_builds_and_errors() {
        let mut p = Progress::default();
        let lines = [
            r#"@nix {"action":"start","id":1,"level":4,"type":102,"text":"","fields":[]}"#.to_owned(),
            r#"@nix {"action":"start","id":2,"level":4,"type":103,"text":"","fields":[]}"#.to_owned(),
            r#"@nix {"action":"result","id":1,"type":106,"fields":[100,3000000]}"#.to_owned(),
            r#"@nix {"action":"result","id":2,"type":105,"fields":[1,3,1,0]}"#.to_owned(),
            format!(
                r#"@nix {{"action":"start","id":3,"level":4,"type":100,"text":"copying path","fields":["/nix/store/{H}-glibc-2.40","https://cache.nixos.org","local"]}}"#
            ),
            r#"@nix {"action":"result","id":3,"type":105,"fields":[500000,1000000,0,0]}"#.to_owned(),
            r#"@nix {"action":"start","id":4,"level":4,"type":104,"text":"","fields":[]}"#.to_owned(),
            r#"@nix {"action":"result","id":4,"type":105,"fields":[0,2,1,0]}"#.to_owned(),
            format!(
                r#"@nix {{"action":"start","id":5,"level":3,"type":105,"text":"building","fields":["/nix/store/{H}-firefox-142.0.drv","",1,1]}}"#
            ),
            r#"@nix {"action":"result","id":5,"type":104,"fields":["installPhase"]}"#.to_owned(),
            r#"@nix {"action":"result","id":5,"type":101,"fields":["\u001b[32mcompiling\u001b[0m"]}"#.to_owned(),
        ];
        for line in &lines {
            assert!(p.feed(line), "{line}");
        }
        assert_eq!(p.current().as_deref(), Some("Building firefox (installPhase)"));
        assert_eq!(p.bytes, (500000, 3000000));
        assert_eq!(p.summary(), "Downloaded 1 of 3 (500 kB of 3.0 MB) · built 0 of 2");
        assert_eq!(p.log.back().map(String::as_str), Some("compiling"));
        assert!(p.fraction().unwrap() > 0.0);
        p.feed(r#"@nix {"action":"stop","id":5}"#);
        assert_eq!(p.current().as_deref(), Some("Downloading glibc"));
        p.feed(r#"@nix {"action":"stop","id":3}"#);
        assert_eq!(p.bytes.0, 1000000);
        p.feed(r#"@nix {"action":"start","id":9,"level":0,"type":0,"text":"Switching to the new system","fields":[]}"#);
        assert_eq!(p.current().as_deref(), Some("Switching to the new system"));
        p.feed(r#"@nix {"action":"msg","level":0,"msg":"\u001b[31;1merror:\u001b[0m attribute 'nope' missing"}"#);
        assert_eq!(p.errors().collect::<Vec<_>>(), vec!["error: attribute 'nope' missing"]);
        assert!(p.feed("plain stderr line"));
        assert!(!p.feed("@nix not json"));
    }
}
