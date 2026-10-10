// SPDX-License-Identifier: GPL-3.0-or-later
//! Read Nix's machine-readable progress and its dry-run summary.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

// Activity and result type numbers from Nix's logging protocol.
const COPY_PATH: u64 = 100;
const COPY_PATHS: u64 = 103;
const BUILDS: u64 = 104;
const BUILD: u64 = 105;
const SUBSTITUTE: u64 = 108;
const RESULT_BUILD_LOG_LINE: u64 = 101;
const RESULT_PROGRESS: u64 = 105;

/// What a dry run says must happen before the system exists in the store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DryRun {
    pub builds: u64,
    pub fetches: u64,
    /// Unpacked (NAR) bytes that substitution would add to the store.
    pub fetch_bytes: u64,
}

fn size(number: &str, unit: &str) -> Option<u64> {
    let number: f64 = number.parse().ok()?;
    let scale = match unit {
        "B" | "bytes" => 1u64,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return None,
    };
    (number.is_finite() && number >= 0.0).then(|| (number * scale as f64).ceil() as u64)
}

/// Parse `nix build --dry-run` stderr, e.g.
/// `these 2 paths will be fetched (16.1 KiB download, 51.2 KiB unpacked):`.
/// An unrecognized fetch line is an error: the caller must not guess that a
/// download fits in memory.
pub fn dry_run(stderr: &str) -> Result<DryRun> {
    let mut result = DryRun::default();
    let count = |line: &str| -> u64 {
        if line.starts_with("this ") {
            1
        } else {
            line.split_whitespace()
                .nth(1)
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        }
    };
    for line in stderr.lines() {
        if line.starts_with("this derivation will be built")
            || (line.starts_with("these ") && line.contains(" derivations will be built"))
        {
            result.builds += count(line);
        } else if line.contains(" will be fetched") {
            let words: Vec<_> = line
                .split(['(', ')', ',', ' '])
                .filter(|w| !w.is_empty())
                .collect();
            let position = words
                .iter()
                .position(|w| *w == "unpacked")
                .context("Unrecognized Nix download summary")?;
            ensure!(position >= 2, "Unrecognized Nix download summary");
            result.fetch_bytes += size(words[position - 2], words[position - 1])
                .context("Unrecognized Nix download size")?;
            result.fetches += count(line);
        }
    }
    Ok(result)
}

/// The first derivation and its `out` path from `nix build --json`.
pub fn build_result(stdout: &str) -> Result<(String, String)> {
    let value: Value = serde_json::from_str(stdout).context("Invalid nix build --json output")?;
    let first = value
        .as_array()
        .filter(|items| items.len() == 1)
        .and_then(|items| items.first())
        .context("Expected exactly one built derivation")?;
    let text = |value: &Value| -> Result<String> {
        let text = value.as_str().context("Missing store path")?;
        ensure!(
            crate::precache::store_path(text).is_some(),
            "Invalid store path {text:?}"
        );
        Ok(text.to_owned())
    };
    Ok((text(&first["drvPath"])?, text(&first["outputs"]["out"])?))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Update {
    /// A build or substitution started, e.g. `building '/nix/store/…'`.
    Activity(String),
    Progress,
}

/// Accumulates `--log-format internal-json` (`@nix {…}` stderr lines).
#[derive(Default)]
pub struct Tracker {
    kinds: HashMap<u64, u64>,
    copied: HashMap<u64, u64>,
    /// NAR bytes copied so far across all copy activities.
    pub copied_bytes: u64,
    /// Paths done/expected for the overall copy.
    pub paths: (u64, u64),
    /// Builds done/expected.
    pub builds: (u64, u64),
    /// Error messages, for a readable failure report.
    errors: Vec<String>,
    /// The most recent build log lines.
    tail: VecDeque<String>,
}

/// Remove terminal escape sequences (Nix colors its messages).
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI: ESC [ parameters final-byte
            if chars.next() == Some('[') {
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
impl Tracker {
    pub fn line(&mut self, line: &str) -> Option<Update> {
        let value: Value = serde_json::from_str(line.strip_prefix("@nix ")?).ok()?;
        let id = value["id"].as_u64();
        match value["action"].as_str()? {
            "start" => {
                let kind = value["type"].as_u64().unwrap_or(0);
                self.kinds.insert(id?, kind);
                let text = value["text"].as_str().unwrap_or_default();
                (matches!(kind, BUILD | SUBSTITUTE) && !text.is_empty())
                    .then(|| Update::Activity(text.chars().take(300).collect()))
            }
            "stop" => {
                self.kinds.remove(&id?);
                None
            }
            "msg" if value["level"].as_u64() == Some(0) => {
                if self.errors.len() < 20 {
                    let text = plain(value["msg"].as_str().unwrap_or_default());
                    self.errors.push(text.chars().take(4000).collect());
                }
                None
            }
            "result" if value["type"].as_u64() == Some(RESULT_BUILD_LOG_LINE) => {
                if let Some(line) = value["fields"][0].as_str() {
                    if self.tail.len() == 40 {
                        self.tail.pop_front();
                    }
                    self.tail.push_back(plain(line).chars().take(500).collect());
                }
                None
            }
            "result" if value["type"].as_u64() == Some(RESULT_PROGRESS) => {
                let fields = value["fields"].as_array()?;
                let done = fields.first()?.as_u64()?;
                let expected = fields.get(1)?.as_u64()?;
                match *self.kinds.get(&id?)? {
                    COPY_PATH => {
                        let previous = self.copied.insert(id?, done).unwrap_or(0);
                        self.copied_bytes += done.saturating_sub(previous);
                    }
                    COPY_PATHS => self.paths = (done, expected),
                    BUILDS => self.builds = (done, expected),
                    _ => return None,
                }
                Some(Update::Progress)
            }
            _ => None,
        }
    }
}

impl Tracker {
    /// Nix's own error messages and the last build log lines, or nothing when
    /// Nix reported neither (the caller then keeps its raw error).
    pub fn failure_report(&self) -> Option<String> {
        if self.errors.is_empty() {
            return None;
        }
        let mut report = self.errors.join("\n");
        if !self.tail.is_empty() {
            report.push_str("\n\nLast build output:\n");
            report.push_str(&self.tail.iter().cloned().collect::<Vec<_>>().join("\n"));
        }
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_run_counts_builds_and_unpacked_downloads() {
        let text = "these 3 derivations will be built:\n  /nix/store/a.drv\nthese 2 paths will be fetched (16.1 KiB download, 51.2 KiB unpacked):\n  /nix/store/b\nthis derivation will be built:\n  /nix/store/c.drv\nthis path will be fetched (1.5 GiB download, 2.0 GiB unpacked):\n";
        let parsed = dry_run(text).unwrap();
        assert_eq!(parsed.builds, 4);
        assert_eq!(parsed.fetches, 3);
        assert_eq!(
            parsed.fetch_bytes,
            (51.2f64 * 1024.0).ceil() as u64 + 2 * (1 << 30)
        );
        assert_eq!(dry_run("").unwrap(), DryRun::default());
        assert!(dry_run("these 2 paths will be fetched (lots):").is_err());
        assert!(
            dry_run("these 2 paths will be fetched (1 PiB download, 2 PiB unpacked):").is_err()
        );
    }
    #[test]
    fn build_json_requires_one_store_derivation() {
        let (drv, out) = build_result(
            r#"[{"drvPath":"/nix/store/x-a.drv","outputs":{"out":"/nix/store/y-a"}}]"#,
        )
        .unwrap();
        assert_eq!(
            (drv.as_str(), out.as_str()),
            ("/nix/store/x-a.drv", "/nix/store/y-a")
        );
        for bad in [
            "[]",
            r#"[{"drvPath":"/tmp/x.drv","outputs":{"out":"/nix/store/y"}}]"#,
            r#"[{"drvPath":"/nix/store/x.drv","outputs":{}}]"#,
            "not json",
        ] {
            assert!(build_result(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn tracker_sums_copy_bytes_and_reports_builds() {
        let mut t = Tracker::default();
        assert_eq!(
            t.line(r#"@nix {"action":"start","id":1,"type":103,"text":"copying 2 paths"}"#),
            None
        );
        t.line(r#"@nix {"action":"start","id":2,"type":100,"text":"copying path"}"#);
        t.line(r#"@nix {"action":"result","id":2,"type":105,"fields":[100,300]}"#);
        t.line(r#"@nix {"action":"result","id":2,"type":105,"fields":[300,300]}"#);
        t.line(r#"@nix {"action":"result","id":1,"type":105,"fields":[1,2,1,0]}"#);
        assert_eq!(t.copied_bytes, 300);
        assert_eq!(t.paths, (1, 2));
        assert_eq!(
            t.line(
                r#"@nix {"action":"start","id":3,"type":105,"text":"building '/nix/store/x.drv'"}"#
            ),
            Some(Update::Activity("building '/nix/store/x.drv'".into()))
        );
        assert_eq!(t.line("plain text"), None);
        assert_eq!(t.line("@nix {broken"), None);
        assert_eq!(t.failure_report(), None);
        t.line(r#"@nix {"action":"result","id":3,"type":101,"fields":["error[E0425]: cannot find value"]}"#);
        t.line(r#"@nix {"action":"msg","level":0,"msg":"\u001b[31;1merror:\u001b[0m builder for 'x.drv' failed"}"#);
        let report = t.failure_report().unwrap();
        assert!(report.starts_with("error: builder for 'x.drv' failed"));
        assert!(report.ends_with("error[E0425]: cannot find value"));
    }
}
