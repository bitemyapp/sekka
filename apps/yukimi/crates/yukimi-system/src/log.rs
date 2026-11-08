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
    /// How far Git is with the repository Nix is fetching, as Git said last.
    git: Option<GitProgress>,
}

/// Git's own progress while it fetches a repository for Nix, which passes
/// it on as Git writes it, each line rewritten in place with `\r`:
/// `Receiving objects:  45% (2488/5529), 108.15 MiB | 917.00 KiB/s`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GitProgress {
    phase: GitPhase,
    percent: Option<u8>,
    /// What follows the counts while receiving: the size so far and the
    /// speed, as Git writes them (`108.15 MiB | 917.00 KiB/s`).
    detail: Option<String>,
    /// The phase's last line, which ends `done.`
    done: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GitPhase {
    /// The server finding and compressing what to send (`remote: …`).
    Preparing,
    Receiving,
    /// Working out the objects sent as differences from others.
    Resolving,
}

/// Git's progress, from one of its lines; `None` for anything else.
fn git_progress(line: &str) -> Option<GitProgress> {
    let line = line.trim();
    let (remote, line) = match line.strip_prefix("remote: ") {
        Some(rest) => (true, rest.trim_start()),
        None => (false, line),
    };
    let (phase, rest) = line.split_once(':')?;
    let phase = match (remote, phase) {
        (true, "Enumerating objects" | "Counting objects" | "Compressing objects") => GitPhase::Preparing,
        (false, "Receiving objects") => GitPhase::Receiving,
        (false, "Resolving deltas") => GitPhase::Resolving,
        _ => return None,
    };
    let rest = rest.trim();
    let done = rest.ends_with("done.");
    let rest = rest.trim_end_matches("done.").trim_end().trim_end_matches(',');
    let percent = rest.split_once('%').and_then(|(p, _)| p.trim().parse::<u8>().ok()).filter(|p| *p <= 100);
    let detail = rest.split_once("), ").map(|(_, detail)| detail.trim().to_owned()).filter(|d| !d.is_empty());
    Some(GitProgress { phase, percent, detail, done })
}

/// The address an activity fetches: its first field, or the one quoted in
/// its description (`fetching Git repository 'https://…'`).
fn fetched_url(activity: &Activity) -> &str {
    let address = |s: &str| s.contains("://") && !s.contains(char::is_whitespace);
    if address(&activity.subject) {
        return &activity.subject;
    }
    activity.text.split('\'').nth(1).filter(|q| address(q)).unwrap_or(&activity.subject)
}

/// What a source fetched from `url` is called: `bitemyapp/sekka` for a
/// repository on GitHub or GitLab or a FlakeHub flake, else the host.
fn source_name(url: &str) -> String {
    // Other addresses (with user info, another port, SSH) are named by host.
    let Some((host, parts)) = yukimi_config::url::https_parts(url) else {
        return yukimi_config::url::host(url).unwrap_or_else(|| url.to_owned());
    };
    let parts: Vec<&str> = parts.iter().map(String::as_str).filter(|p| !p.is_empty()).collect();
    let pair = |owner: &str, repo: &str| format!("{owner}/{}", repo.strip_suffix(".git").unwrap_or(repo));
    match (host.as_str(), parts.as_slice()) {
        ("api.github.com", ["repos", owner, repo, ..])
        | ("github.com" | "codeload.github.com" | "gitlab.com", [owner, repo, ..])
        | ("api.flakehub.com", ["f", "pinned", owner, repo, ..])
        | ("flakehub.com", ["f", owner, repo, ..]) => pair(owner, repo),
        _ => host,
    }
}

/// Whether a download is part of copying a path from a binary cache, which
/// that copy already reports by its package's name.
fn from_binary_cache(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.ends_with(".narinfo") || path.contains("/nar/") || path.ends_with("/nix-cache-info") || path.ends_with(".ls")
}

impl Activity {
    /// Whether it fetches a source to evaluate, such as a flake input,
    /// rather than part of a path copied from a binary cache.
    fn is_source_fetch(&self) -> bool {
        match self.kind {
            FETCH_TREE => true,
            FILE_TRANSFER => !from_binary_cache(fetched_url(self)),
            _ => false,
        }
    }

    /// Whether it only asks GitHub which commit a branch is at: a few
    /// kilobytes before a `github:` source's tarball, not the source.
    fn looks_up_commit(&self) -> bool {
        self.kind == FILE_TRANSFER
            && yukimi_config::url::https_parts(fetched_url(self)).is_some_and(|(host, parts)| {
                host == "api.github.com" && parts.len() > 4 && parts[0] == "repos" && parts[3] == "commits"
            })
    }
}

impl GitProgress {
    fn describe(&self, name: &str) -> String {
        let percent = self.percent.map(|p| format!("{p}%"));
        match (self.phase, &self.detail, percent) {
            (GitPhase::Preparing, ..) => format!("Fetching {name}: the server is preparing it"),
            (GitPhase::Receiving, Some(detail), Some(percent)) => {
                format!("Downloading {name}: {percent}, {}", detail.replace(" | ", " at "))
            }
            (GitPhase::Receiving, None, Some(percent)) => format!("Downloading {name}: {percent}"),
            (GitPhase::Receiving, ..) => format!("Downloading {name}"),
            (GitPhase::Resolving, _, Some(percent)) => format!("Unpacking {name}: {percent}"),
            (GitPhase::Resolving, ..) => format!("Unpacking {name}"),
        }
    }
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
            // Git pads what it rewrites in place with spaces.
            let text = strip_ansi(line).trim_end().to_owned();
            if let Some(git) = git_progress(&text) {
                // Each phase's last line goes in the log, not every step.
                if git.done {
                    self.push_log(text);
                }
                self.git = Some(git);
                return true;
            }
            self.push_log(text);
            return true;
        };
        let Ok(event) = serde_json::from_str::<Event>(json) else {
            return false;
        };
        match event {
            Event::Start { id, kind, text, fields } => {
                let subject =
                    fields.first().and_then(|f| f.as_str()).map(str::to_owned).unwrap_or_else(|| text.clone());
                let activity = Activity { kind, text, subject, done: 0, expected: 0, phase: None };
                // Fetching a source says so in the log, so a long download
                // isn't taken for evaluating.
                if activity.is_source_fetch() && !activity.text.is_empty() {
                    self.push_log(activity.text.clone());
                }
                if kind == FETCH_TREE {
                    self.git = None;
                }
                self.activities.insert(id, activity);
                self.order.push(id);
            }
            Event::Stop { id } => {
                self.order.retain(|&o| o != id);
                let Some(activity) = self.activities.remove(&id) else { return true };
                match activity.kind {
                    COPY_PATH => {
                        // A finished download counts fully.
                        self.bytes.0 =
                            self.bytes.0.saturating_sub(activity.done) + activity.expected.max(activity.done);
                    }
                    FILE_TRANSFER if activity.is_source_fetch() && !activity.looks_up_commit() && activity.done > 0 => {
                        let name = source_name(fetched_url(&activity));
                        self.push_log(format!("Downloaded {name}: {}", crate::human_size(activity.done)));
                    }
                    FETCH_TREE => self.git = None,
                    _ => {}
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
                // Part of copying a path, which says what it is.
                FILE_TRANSFER if !activity.is_source_fetch() => None,
                FILE_TRANSFER if activity.looks_up_commit() => {
                    Some(format!("Fetching {}", source_name(fetched_url(activity))))
                }
                FILE_TRANSFER => {
                    let source = source_name(fetched_url(activity));
                    Some(match (activity.done, activity.expected) {
                        (0, _) => format!("Downloading {source}"),
                        (done, 0) => format!("Downloading {source}: {}", crate::human_size(done)),
                        (done, expected) => format!(
                            "Downloading {source}: {} of {}",
                            crate::human_size(done.min(expected)),
                            crate::human_size(expected)
                        ),
                    })
                }
                FETCH_TREE => {
                    let source = source_name(fetched_url(activity));
                    Some(match &self.git {
                        Some(git) => git.describe(&source),
                        None => format!("Fetching {source}"),
                    })
                }
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

    /// How far along, from 0 to 1, when Nix has said what to expect. Before
    /// anything is downloaded or built, how far the source being fetched is,
    /// when its size is known.
    pub fn fraction(&self) -> Option<f64> {
        let total = self.downloads.expected + self.builds.expected * 4;
        if total == 0 {
            return self.fetched();
        }
        let done = self.downloads.done + self.builds.done * 4;
        Some((done as f64 / total as f64).clamp(0.0, 1.0))
    }

    /// How far the most recent source fetch is, when it can tell.
    fn fetched(&self) -> Option<f64> {
        let activity =
            self.order.iter().rev().filter_map(|id| self.activities.get(id)).find(|a| a.is_source_fetch())?;
        match activity.kind {
            FETCH_TREE => match &self.git {
                Some(GitProgress { phase: GitPhase::Receiving, percent: Some(percent), .. }) => {
                    Some(f64::from(*percent) / 100.0)
                }
                _ => None,
            },
            _ if activity.looks_up_commit() => None,
            _ if activity.expected > 0 => Some((activity.done as f64 / activity.expected as f64).clamp(0.0, 1.0)),
            _ => None,
        }
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

    const REV: &str = "2111c84ac0b4555518b9482ec436f813d26905ef";

    /// A `github:` input, downloaded as one tarball, as Determinate Nix
    /// reports it (with its own attempt activity beneath).
    #[test]
    fn source_tarballs_show_how_much_has_arrived() {
        let mut p = Progress::default();
        let url = format!("https://api.github.com/repos/bitemyapp/sekka/tarball/{REV}");
        p.feed(&format!(
            r#"@nix {{"action":"start","fields":["{url}"],"id":1,"level":4,"parent":0,"text":"downloading '{url}'","type":101}}"#
        ));
        p.feed(r#"@nix {"action":"start","id":2,"level":6,"name":"FileTransferAttempt","parent":1,"payload":{},"text":"","type":10113}"#);
        assert_eq!(p.current().as_deref(), Some("Downloading bitemyapp/sekka"));
        assert_eq!(p.log.back(), Some(&format!("downloading '{url}'")));
        // GitHub generates the tarball as it sends it, so its size is unknown.
        p.feed(r#"@nix {"action":"result","fields":[346489581,0,0,0],"id":1,"type":105}"#);
        assert_eq!(p.current().as_deref(), Some("Downloading bitemyapp/sekka: 346 MB"));
        assert_eq!(p.fraction(), None);
        p.feed(r#"@nix {"action":"result","fields":[100000000,400000000,0,0],"id":1,"type":105}"#);
        assert_eq!(p.current().as_deref(), Some("Downloading bitemyapp/sekka: 100 MB of 400 MB"));
        assert_eq!(p.fraction(), Some(0.25));
        p.feed(r#"@nix {"action":"result","id":2,"payload":{"bodySize":7822,"httpStatus":200}}"#);
        p.feed(r#"@nix {"action":"stop","id":2}"#);
        p.feed(r#"@nix {"action":"stop","id":1}"#);
        assert_eq!(p.log.back().map(String::as_str), Some("Downloaded bitemyapp/sekka: 100 MB"));
        assert_eq!(p.current(), None);
    }

    /// Before a `github:` tarball, Nix asks GitHub's API which commit the
    /// branch is at: a few kilobytes that aren't the source.
    #[test]
    fn commit_lookups_are_not_downloads() {
        let mut p = Progress::default();
        let url = "https://api.github.com/repos/bitemyapp/sekka/commits/nightly";
        let logged = p.logged;
        p.feed(&format!(
            r#"@nix {{"action":"start","fields":["{url}"],"id":1,"level":4,"parent":0,"text":"downloading '{url}'","type":101}}"#
        ));
        p.feed(r#"@nix {"action":"result","fields":[2155,2155,0,0],"id":1,"type":105}"#);
        assert_eq!(p.current().as_deref(), Some("Fetching bitemyapp/sekka"));
        assert_eq!(p.fraction(), None);
        p.feed(r#"@nix {"action":"stop","id":1}"#);
        assert_eq!(p.since(logged).collect::<Vec<_>>(), [format!("downloading '{url}'")]);
    }

    /// A `git+https:` input: Nix reports the fetch, and Git its progress,
    /// as plain lines.
    #[test]
    fn git_fetches_show_gits_progress() {
        let mut p = Progress::default();
        p.feed(r#"@nix {"action":"start","id":1,"level":4,"parent":0,"text":"fetching Git repository 'https://github.com/bitemyapp/sekka'","type":112}"#);
        assert_eq!(p.current().as_deref(), Some("Fetching bitemyapp/sekka"));
        assert_eq!(
            p.log.back().map(String::as_str),
            Some("fetching Git repository 'https://github.com/bitemyapp/sekka'")
        );
        let logged = p.logged;
        assert!(p.feed("remote: Counting objects:  10% (553/5529)        "));
        assert_eq!(p.current().as_deref(), Some("Fetching bitemyapp/sekka: the server is preparing it"));
        assert!(p.feed("remote: Compressing objects: 100% (3947/3947), done.        "));
        assert!(p.feed("Receiving objects:   8% (462/5529), 10.04 MiB | 3.17 MiB/s"));
        assert_eq!(p.current().as_deref(), Some("Downloading bitemyapp/sekka: 8%, 10.04 MiB at 3.17 MiB/s"));
        assert_eq!(p.fraction(), Some(0.08));
        assert!(p.feed("Receiving objects: 100% (5529/5529), 300.12 MiB | 1.20 MiB/s, done."));
        assert!(p.feed("Resolving deltas:  40% (40/100)"));
        assert_eq!(p.current().as_deref(), Some("Unpacking bitemyapp/sekka: 40%"));
        assert_eq!(p.fraction(), None);
        assert!(p.feed("From https://github.com/bitemyapp/sekka"));
        // Only each phase's last line, and Git's other lines, are logged.
        assert_eq!(
            p.since(logged).collect::<Vec<_>>(),
            [
                "remote: Compressing objects: 100% (3947/3947), done.",
                "Receiving objects: 100% (5529/5529), 300.12 MiB | 1.20 MiB/s, done.",
                "From https://github.com/bitemyapp/sekka",
            ]
        );
        p.feed(r#"@nix {"action":"stop","id":1}"#);
        assert_eq!(p.current(), None);
        // The next fetch starts afresh.
        p.feed(r#"@nix {"action":"start","id":3,"level":4,"text":"fetching Git repository 'https://example.org/x.git'","type":112}"#);
        assert_eq!(p.current().as_deref(), Some("Fetching example.org"));
    }

    #[test]
    fn binary_cache_transfers_leave_the_package_named() {
        let mut p = Progress::default();
        p.feed(&format!(
            r#"@nix {{"action":"start","id":3,"level":4,"type":100,"text":"copying path","fields":["/nix/store/{H}-glibc-2.40","https://cache.nixos.org","local"]}}"#
        ));
        let logged = p.logged;
        p.feed(r#"@nix {"action":"start","id":4,"level":4,"type":101,"text":"downloading 'https://cache.nixos.org/nar/0abc.nar.xz'","fields":["https://cache.nixos.org/nar/0abc.nar.xz"]}"#);
        p.feed(r#"@nix {"action":"result","id":4,"type":105,"fields":[500,1000,0,0]}"#);
        assert_eq!(p.current().as_deref(), Some("Downloading glibc"));
        p.feed(r#"@nix {"action":"stop","id":4}"#);
        assert_eq!(p.logged, logged);
    }

    #[test]
    fn sources_by_name() {
        for (url, name) in [
            (format!("https://api.github.com/repos/bitemyapp/sekka/tarball/{REV}"), "bitemyapp/sekka"),
            (format!("https://codeload.github.com/bitemyapp/sekka/tar.gz/{REV}"), "bitemyapp/sekka"),
            ("https://github.com/bitemyapp/sekka".into(), "bitemyapp/sekka"),
            ("https://github.com/o/r.git?ref=main".into(), "o/r"),
            ("https://GitHub.com:443/bit%65myapp/sekka/".into(), "bitemyapp/sekka"),
            // Credentials and ports are never shown.
            ("https://token@github.com/o/r".into(), "github.com"),
            ("ssh://git@example.org:2222/r.git".into(), "example.org"),
            ("https://api.flakehub.com/f/pinned/NixOS/nixpkgs/0.1.1%2Brev-1/1/source.tar.gz".into(), "NixOS/nixpkgs"),
            (
                "https://install.determinate.systems/flake-registry/stable/flake-registry.json".into(),
                "install.determinate.systems",
            ),
        ] {
            assert_eq!(source_name(&url), name, "{url}");
        }
        assert_eq!(git_progress("remote: Total 5529 (delta 12), reused 3 (delta 0)"), None);
        assert_eq!(git_progress("Receiving objects"), None);
        assert_eq!(
            git_progress("remote: Enumerating objects: 5529, done.        "),
            Some(GitProgress { phase: GitPhase::Preparing, percent: None, detail: None, done: true })
        );
    }
}
