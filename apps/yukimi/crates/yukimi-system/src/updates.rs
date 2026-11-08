// SPDX-License-Identifier: MIT OR Apache-2.0
//! Updates: whether each source of the system has a newer version, and
//! getting an update ready without administrator rights.
//!
//! Checking asks where each source comes from directly when it can: GitHub
//! for the commit a branch is at, FlakeHub for its newest release, the NixOS
//! channels for theirs. That takes about a second and downloads nothing.
//! Other sources are checked by locking them anew with Nix, which downloads
//! them.
//!
//! Getting an update ready locks the chosen inputs anew into a lock file of
//! Yukimi's own and copies every source that changed into the store. The
//! helper checks that lock file and puts it in place, and the build that
//! follows finds every source already there. Without this, root's Nix
//! would download each of them again: it keeps its downloads apart from
//! each user's.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use yukimi_config::lock::{FlakeLock, Input, Locked, Movement};

use crate::channels::Channel;
use crate::nix;

/// The newest version of a source, as where it comes from tells it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub rev: Option<String>,
    /// When it was made, as Unix time.
    pub last_modified: Option<i64>,
    /// How many commits it has (FlakeHub's releases count them), which only
    /// grows.
    pub count: Option<u64>,
    /// Its content's hash, when Nix locked it.
    pub nar_hash: Option<String>,
}

impl Release {
    fn of(locked: &Locked) -> Release {
        Release {
            rev: locked.rev.clone(),
            last_modified: locked.last_modified,
            count: locked.rev_count,
            nar_hash: locked.nar_hash.clone(),
        }
    }

    /// As a lock file would record it, for showing.
    pub fn locked(&self, kind: &str) -> Locked {
        Locked {
            kind: kind.to_owned(),
            rev: self.rev.clone(),
            last_modified: self.last_modified,
            rev_count: self.count,
            nar_hash: self.nar_hash.clone(),
            ..Locked::default()
        }
    }
}

/// How updating would move a source from what it is locked to now.
pub fn movement(current: &Locked, newest: &Release) -> Movement {
    let same = match (&current.nar_hash, &newest.nar_hash, &current.rev, &newest.rev) {
        (Some(a), Some(b), _, _) => a == b,
        (_, _, Some(a), Some(b)) => a == b,
        _ => false,
    };
    let older = match (current.rev_count, newest.count, current.last_modified, newest.last_modified) {
        (Some(before), Some(after), _, _) => after < before,
        (_, _, Some(before), Some(after)) => after < before,
        _ => false,
    };
    match () {
        _ if same => Movement::Current,
        _ if older => Movement::Older(newest.locked(&current.kind)),
        _ => Movement::Newer(newest.locked(&current.kind)),
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("yukimi/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// A request's answer as text, with failures in words.
fn get(url: &str, accept: &str) -> Result<String, String> {
    let host = yukimi_config::url::host(url).unwrap_or_else(|| url.to_owned());
    let response = agent().get(url).header("Accept", accept).call().map_err(|e| match e {
        ureq::Error::StatusCode(403 | 429) if host == "api.github.com" => {
            "GitHub's limit of 60 checks an hour was reached".to_owned()
        }
        ureq::Error::StatusCode(404) => format!("{host} doesn't know it"),
        ureq::Error::StatusCode(code) => format!("{host} answered with HTTP {code}"),
        other => format!("{host} couldn't be reached: {other}"),
    })?;
    response.into_body().read_to_string().map_err(|e| e.to_string())
}

/// Read the lock file at an immutable GitHub revision without evaluating or
/// building the flake. Used when an update target has no saved generation yet.
pub fn revision_lock(source: &Locked) -> Result<String, String> {
    let Some((owner, repo)) = source.github_repo() else {
        return Err("This source has no saved lock file; remote input browsing supports github.com sources.".into());
    };
    let Some(rev) = &source.rev else {
        return Err("This source has no exact repository revision.".into());
    };
    if !rev.bytes().all(|b| b.is_ascii_hexdigit()) || rev.len() != 40 {
        return Err("This source has no full commit ID.".into());
    }
    let url = yukimi_config::url::https("raw.githubusercontent.com", &[&owner, &repo, rev, "flake.lock"], &[])
        .ok_or("Invalid repository address")?;
    get(&url, "text/plain")
}

/// Parse an RFC 3339 time in UTC (`2026-10-08T08:01:05Z`, with or without
/// fractions of a second) as Unix time.
pub fn parse_time(text: &str) -> Option<i64> {
    let (date, time) = text.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let time = time.trim_end_matches('Z');
    let time = time.split(['+', '.']).next()?;
    let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
    let (h, min, s) = (t.next()??, t.next()??, t.next()??);
    // Days since 1970-01-01 in the proleptic Gregorian calendar.
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * m + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + s)
}

/// The commit a GitHub branch or tag is at, and when it was made.
fn github(owner: &str, repo: &str, reference: &str) -> Result<Release, String> {
    let url = yukimi_config::url::https("api.github.com", &["repos", owner, repo, "commits", reference], &[])
        .ok_or("Invalid repository address")?;
    let body: serde_json::Value =
        serde_json::from_str(&get(&url, "application/vnd.github+json")?).map_err(|e| e.to_string())?;
    let rev = body["sha"].as_str().ok_or("GitHub's answer had no commit")?.to_owned();
    let last_modified = body["commit"]["committer"]["date"].as_str().and_then(parse_time);
    Ok(Release { rev: Some(rev), last_modified, ..Release::default() })
}

/// The branches of a GitHub repository, for choosing one to follow.
pub fn github_branches(owner: &str, repo: &str) -> Result<Vec<String>, String> {
    let url = yukimi_config::url::https("api.github.com", &["repos", owner, repo, "branches"], &[("per_page", "100")])
        .ok_or("Invalid repository address")?;
    let body: serde_json::Value =
        serde_json::from_str(&get(&url, "application/vnd.github+json")?).map_err(|e| e.to_string())?;
    let mut names: Vec<String> =
        body.as_array().into_iter().flatten().filter_map(|b| b["name"].as_str().map(str::to_owned)).collect();
    names.sort();
    Ok(names)
}

/// FlakeHub's newest release matching a version (`0.1`, `*`).
fn flakehub(latest_url: &str) -> Result<Release, String> {
    let url = yukimi_config::lock::flakehub_version_url(latest_url).ok_or("not a FlakeHub address")?;
    let body: serde_json::Value = serde_json::from_str(&get(&url, "application/json")?).map_err(|e| e.to_string())?;
    Ok(Release {
        rev: Some(body["revision"].as_str().ok_or("FlakeHub's answer had no revision")?.to_owned()),
        last_modified: body["published_at"].as_str().and_then(parse_time),
        count: body["commit_count"].as_u64(),
        nar_hash: None,
    })
}

/// Ask where an input comes from for its newest version, when it can be
/// asked directly. `None` when it can't, or when it is pinned to one
/// commit and so has nothing newer.
pub fn ask(input: &Input) -> Option<Result<Release, String>> {
    let locked = input.locked.as_ref()?;
    let original = input.original.as_ref().unwrap_or(locked);
    match original.kind.as_str() {
        // A Git source on GitHub, too: Sekka is fetched with Git so that an
        // update downloads only what changed, but GitHub still knows its
        // branches.
        "github" | "git" if original.rev.is_none() => {
            let (owner, repo) = original.github_repo()?;
            Some(github(&owner, &repo, original.reference.as_deref().unwrap_or("HEAD")))
        }
        "tarball" | "file" => original.flakehub_latest_url().map(|url| flakehub(&url)),
        _ => None,
    }
}

/// Lock one input anew with Nix, into `scratch/<input>.lock`, and read what
/// it was locked to: the way to check sources that can't be asked directly.
fn lock_anew(flake: &str, input: &str, scratch: &Path) -> Result<Release, String> {
    let output = scratch.join(format!("{input}.lock"));
    let result = nix::command()
        .args(["flake", "update", "--refresh", input, "--flake", flake, "--output-lock-file"])
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Nix couldn't be started: {e}"))?;
    if !result.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&result.stderr));
        return Err(first_line(&nix::last_error(&stderr)));
    }
    let text = std::fs::read_to_string(&output).map_err(|e| e.to_string())?;
    let lock = FlakeLock::parse(&text).map_err(|e| e.to_string())?;
    let locked = lock.input(input).and_then(|i| i.locked).ok_or("the new lock file doesn't have it")?;
    Ok(Release::of(&locked))
}

/// Whether Nix may lock an input anew when its host couldn't be asked. Not a
/// Git source: Nix would clone it into this user's own cache, all of it the
/// first time (hundreds of megabytes for Sekka), for an answer that GitHub
/// gives again once its hourly limit has passed.
fn retry_with_nix(input: &Input) -> bool {
    input.original.as_ref().or(input.locked.as_ref()).is_none_or(|source| source.kind != "git")
}

/// The gist of an error: its first line, without an "error:" label.
pub fn first_line(error: &str) -> String {
    error
        .lines()
        .map(|line| line.trim().trim_start_matches("error:").trim())
        .find(|line| !line.is_empty())
        .unwrap_or("unknown error")
        .to_owned()
}

/// The newest version of each of a flake's inputs that can move (those that
/// are locked, not following another), side by side. Each fails on its own.
pub fn check_flake(flake: &str, inputs: &[Input], scratch: &Path) -> BTreeMap<String, Result<Release, String>> {
    let movable: Vec<&Input> = inputs.iter().filter(|i| i.follows.is_none() && i.locked.is_some()).collect();
    std::thread::scope(|scope| {
        let checks: Vec<_> = movable
            .iter()
            .map(|input| {
                scope.spawn(move || match ask(input) {
                    Some(Ok(release)) => Ok(release),
                    Some(Err(asked)) if !retry_with_nix(input) => Err(asked),
                    // When the host can't be asked (offline, or GitHub's hourly
                    // limit), Nix may still get through.
                    Some(Err(asked)) => lock_anew(flake, &input.name, scratch).map_err(|_| asked),
                    None => lock_anew(flake, &input.name, scratch),
                })
            })
            .collect();
        movable
            .iter()
            .zip(checks)
            .map(|(input, check)| {
                let result = check.join().unwrap_or_else(|_| Err("the check stopped unexpectedly".to_owned()));
                (input.name.clone(), result)
            })
            .collect()
    })
}

/// On a Sekka system, where updating takes Sekka: the newest commit of its
/// `nightly` branch, which the update follows instead of `main`. Sekka on
/// another branch, or pinned, keeps its answer, and so does Sekka's `main`
/// chosen for early updates (`flake`, the text of `flake.nix`, says
/// [`yukimi_config::sekka::EARLY`]), which the update keeps: the lock can't
/// tell that from the `main` it moves to the nightly. Inputs that follow
/// Sekka's own move with it, and aren't checked by themselves.
pub fn sekka_targets(flake: &str, inputs: &[Input], newest: &mut BTreeMap<String, Result<Release, String>>) {
    let address = yukimi_config::edit::string_value(flake, &["inputs", "sekka", "url"]).ok().flatten();
    // Use the same parsed migration decision as the writer. In particular,
    // custom fetcher options can prevent a legacy main URL from migrating.
    let Some(target) = address.as_deref().and_then(yukimi_config::sekka::fetched_with_git) else {
        return;
    };
    if yukimi_config::sekka::early(&target) {
        return;
    }
    let Some(original) = inputs.iter().find(|i| i.name == "sekka").and_then(|i| i.original.as_ref()) else {
        return;
    };
    // Only the old tarball addresses can move to another branch during
    // prepare. Existing Git sources keep their URL and check_flake's answer.
    if original.kind != "github"
        || !original.github_repo().is_some_and(|(owner, repo)| owner == "bitemyapp" && repo == "sekka")
        || original.rev.is_some()
        || !matches!(original.reference.as_deref(), None | Some("main" | "nightly"))
    {
        return;
    }
    newest.insert("sekka".to_owned(), github("bitemyapp", "sekka", "nightly"));
}

/// The commit a NixOS channel is at now.
fn channel(channel: &Channel) -> Result<Release, String> {
    let at = channel.published_at().ok_or("it isn't one of the NixOS channels, so Yukimi can't ask it")?;
    let rev = get(&format!("{at}/git-revision"), "text/plain")?.trim().to_owned();
    if rev.is_empty() || !rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the channel's answer wasn't a revision".to_owned());
    }
    // When that commit was made, if GitHub says.
    let last_modified = github("NixOS", "nixpkgs", &rev).ok().and_then(|r| r.last_modified);
    Ok(Release { rev: Some(rev), last_modified, ..Release::default() })
}

/// The newest version of each channel.
pub fn check_channels(channels: &[Channel]) -> BTreeMap<String, Result<Release, String>> {
    std::thread::scope(|scope| {
        let checks: Vec<_> = channels.iter().map(|c| scope.spawn(move || channel(c))).collect();
        channels
            .iter()
            .zip(checks)
            .map(|(c, check)| {
                (c.name.clone(), check.join().unwrap_or_else(|_| Err("the check stopped unexpectedly".to_owned())))
            })
            .collect()
    })
}

/// How a channel would move: its revision says, the dates when known.
pub fn channel_movement(channel: &Channel, newest: &Release) -> Movement {
    let current = Locked { kind: "channel".into(), rev: channel.revision.clone(), ..Locked::default() };
    movement(&current, newest)
}

/// The command that locks `inputs` of `flake` anew into `lock`, reporting
/// Nix's progress: the first step of getting an update ready.
pub fn lock_command(flake: &str, inputs: &[String], lock: &Path) -> Command {
    let mut command = nix::command();
    // Nix remembers where a branch pointed for an hour; a commit pushed a
    // minute ago is what was asked for.
    command.args(["flake", "update", "--refresh", "--log-format", "internal-json", "-v", "--flake", flake]);
    command.args(inputs);
    command.arg("--output-lock-file").arg(lock);
    command
}

/// The sources a new lock file has that the current one doesn't, as their
/// locked attributes, which `builtins.fetchTree` takes.
pub fn new_sources(current: &str, proposed: &str) -> Vec<serde_json::Value> {
    let hashes = |text: &str| -> Vec<serde_json::Value> {
        let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        value["nodes"]
            .as_object()
            .map(|nodes| nodes.values().map(|n| n["locked"].clone()).collect())
            .unwrap_or_default()
    };
    let known: BTreeSet<String> =
        hashes(current).iter().filter_map(|l| l["narHash"].as_str().map(str::to_owned)).collect();
    let mut seen = BTreeSet::new();
    hashes(proposed)
        .into_iter()
        .filter(|l| l["narHash"].as_str().is_some_and(|h| !known.contains(h) && seen.insert(h.to_owned())))
        .collect()
}

/// The command that copies sources (written as a JSON list in `list`) into
/// the store, where root's build finds them: the second step of getting an
/// update ready.
pub fn fetch_command(list: &Path) -> Command {
    let mut command = nix::command();
    let expression = format!(
        "builtins.concatStringsSep \"\\n\" (map (locked: \"${{builtins.fetchTree locked}}\") (builtins.fromJSON (builtins.readFile {})))",
        yukimi_config::edit::nix_string(&list.to_string_lossy())
    );
    command.args(["eval", "--impure", "--raw", "--log-format", "internal-json", "-v", "--expr", &expression]);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-08T08:01:05Z"), Some(1_791_446_465));
        assert_eq!(parse_time("2026-10-08T14:40:59.082306Z"), Some(1_791_470_459));
        assert_eq!(parse_time("2000-02-29T12:00:00+00:00"), Some(951_825_600));
        assert_eq!(parse_time("yesterday"), None);
    }

    #[test]
    fn movements() {
        let now =
            Locked { kind: "github".into(), rev: Some("aaa".into()), last_modified: Some(100), ..Locked::default() };
        let same = Release { rev: Some("aaa".into()), ..Release::default() };
        assert_eq!(movement(&now, &same), Movement::Current);
        let newer = Release { rev: Some("bbb".into()), last_modified: Some(200), ..Release::default() };
        assert!(matches!(movement(&now, &newer), Movement::Newer(l) if l.last_modified == Some(200)));
        let older = Release { rev: Some("ccc".into()), last_modified: Some(50), ..Release::default() };
        assert!(matches!(movement(&now, &older), Movement::Older(_)));
        // FlakeHub's commit counts decide when both are known.
        let counted = Locked { rev_count: Some(10), ..now.clone() };
        let fewer = Release { rev: Some("ddd".into()), count: Some(9), last_modified: Some(300), ..Release::default() };
        assert!(matches!(movement(&counted, &fewer), Movement::Older(_)));
    }

    #[test]
    fn only_changed_sources_are_fetched() {
        let current = r#"{"nodes":{"a":{"locked":{"narHash":"sha256-a","type":"github"}},"root":{}},"root":"root"}"#;
        let proposed = r#"{"nodes":{"a":{"locked":{"narHash":"sha256-a2","type":"github"}},
            "b":{"locked":{"narHash":"sha256-a2","type":"github"}},"c":{"locked":{"narHash":"sha256-a"}},"root":{}},"root":"root"}"#;
        let fresh = new_sources(current, proposed);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0]["narHash"], "sha256-a2");
        let command = fetch_command(Path::new("/home/a b/.cache/yukimi/sources.json"));
        let expression = command.get_args().last().unwrap().to_string_lossy().into_owned();
        assert!(expression.contains("builtins.readFile \"/home/a b/.cache/yukimi/sources.json\""), "{expression}");
    }

    #[test]
    fn updates_see_what_was_just_pushed() {
        // Without --refresh, Nix would lock what a branch pointed at up to an
        // hour ago.
        let command = lock_command("/etc/nixos", &["tatami".to_owned()], Path::new("/tmp/u.lock"));
        let args: Vec<String> = command.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.contains(&"--refresh".to_owned()), "{args:?}");
        assert!(args.windows(2).any(|w| w == ["--output-lock-file", "/tmp/u.lock"]), "{args:?}");
    }

    #[test]
    fn what_can_be_asked() {
        let lock = FlakeLock::parse(
            r#"{"nodes":{
              "a":{"locked":{"owner":"o","repo":"r","rev":"1","type":"github"},"original":{"owner":"o","repo":"r","rev":"1","type":"github"}},
              "b":{"locked":{"type":"git","url":"https://example.org/x","rev":"2"},"original":{"type":"git","url":"https://example.org/x"}},
              "root":{"inputs":{"a":"a","b":"b"}}},"root":"root","version":7}"#,
        )
        .unwrap();
        // Pinned to a commit, or from a plain Git server: not asked.
        assert!(ask(&lock.input("a").unwrap()).is_none());
        assert!(ask(&lock.input("b").unwrap()).is_none());
    }

    #[test]
    fn retained_main_sources_keep_mains_answer() {
        let lock = FlakeLock::parse(
            r#"{ "root": "root", "version": 7, "nodes": {
              "root": { "inputs": { "sekka": "sekka" } },
              "sekka": { "locked": { "type": "github", "owner": "bitemyapp", "repo": "sekka", "rev": "1", "narHash": "sha256-y", "lastModified": 1 },
                         "original": { "type": "github", "owner": "bitemyapp", "repo": "sekka", "ref": "main" } } } }"#,
        )
        .unwrap();
        let main = Release { rev: Some("2".repeat(40)), last_modified: Some(2), ..Release::default() };
        // Explicit early updates and custom settings that prevent migration
        // must keep the target of their original main source.
        for early in [
            yukimi_config::sekka::EARLY,
            "github:bitemyapp/sekka?ref=main",
            "github:bitemyapp/sekka?%72ef=%6dain",
            "github:bitemyapp/sekka/main?dir=sub",
            "github:bitemyapp/sekka/main?narHash=sha256-custom",
        ] {
            let mut newest = BTreeMap::from([("sekka".to_owned(), Ok(main.clone()))]);
            let flake = format!("{{ inputs.sekka.url = \"{early}\"; outputs = _: {{ }}; }}");
            sekka_targets(&flake, &lock.inputs(), &mut newest);
            assert_eq!(newest["sekka"], Ok(main.clone()), "{early}");
        }
    }

    #[test]
    fn sekka_from_other_repositories_keeps_its_answer() {
        let lock = FlakeLock::parse(
            r#"{ "root": "root", "version": 7, "nodes": {
              "root": { "inputs": { "sekka": "sekka" } },
              "sekka": { "locked": { "type": "git", "url": "https://github.com/someone/sekka", "ref": "nightly", "rev": "1", "narHash": "sha256-y", "lastModified": 1 },
                         "original": { "type": "git", "url": "https://github.com/someone/sekka", "ref": "nightly" } } } }"#,
        )
        .unwrap();
        let fork = Release { rev: Some("3".repeat(40)), ..Release::default() };
        let mut newest = BTreeMap::from([("sekka".to_owned(), Ok(fork.clone()))]);
        sekka_targets(
            "{ inputs.sekka.url = \"git+https://github.com/someone/sekka?ref=nightly\"; }",
            &lock.inputs(),
            &mut newest,
        );
        assert_eq!(newest["sekka"], Ok(fork));
    }

    #[test]
    fn existing_git_sources_keep_their_update_target() {
        let lock = FlakeLock::parse(
            r#"{ "root": "root", "version": 7, "nodes": {
              "root": { "inputs": { "sekka": "sekka" } },
              "sekka": { "locked": { "type": "git", "url": "https://github.com/bitemyapp/sekka", "ref": "main", "rev": "1" },
                         "original": { "type": "git", "url": "https://github.com/bitemyapp/sekka", "ref": "main" } } } }"#,
        )
        .unwrap();
        let mut inputs = lock.inputs();
        let target = Release { rev: Some("2".repeat(40)), last_modified: Some(2), ..Release::default() };
        // prepare leaves these Git URLs unchanged. Checking must retain
        // the answer for that source, including the repository's HEAD.
        for (query, reference) in [
            ("?ref=main", Some("main")),
            ("?shallow=1&ref=main", Some("main")),
            ("?ref=refs/heads/main&shallow=1", Some("refs/heads/main")),
            ("?ref=nightly&shallow=1", Some("nightly")),
            ("?shallow=1", None),
            ("", None),
        ] {
            let flake = format!(
                "{{ inputs.sekka.url = \"git+https://github.com/bitemyapp/sekka{query}\"; outputs = _: {{ }}; }}"
            );
            assert_eq!(yukimi_config::sekka::prepare(&flake).unwrap(), flake);
            inputs[0].original.as_mut().unwrap().reference = reference.map(str::to_owned);
            let mut newest = BTreeMap::from([("sekka".to_owned(), Ok(target.clone()))]);
            sekka_targets(&flake, &inputs, &mut newest);
            assert_eq!(newest["sekka"], Ok(target.clone()), "{query}");
        }
    }

    #[test]
    fn git_sources_wait_for_github_rather_than_cloning() {
        let lock = FlakeLock::parse(
            r#"{ "root": "root", "version": 7, "nodes": {
              "root": { "inputs": { "sekka": "sekka", "tatami": "tatami" } },
              "sekka": { "locked": { "type": "git", "url": "https://github.com/bitemyapp/sekka", "ref": "nightly", "rev": "1" },
                         "original": { "type": "git", "url": "https://github.com/bitemyapp/sekka", "ref": "nightly", "shallow": true } },
              "tatami": { "locked": { "type": "github", "owner": "bitemyapp", "repo": "tatami", "rev": "1" },
                          "original": { "type": "github", "owner": "bitemyapp", "repo": "tatami" } } } }"#,
        )
        .unwrap();
        // GitHub's limit reached: a tarball source may still be locked by
        // Nix, but a Git one would be cloned into the user's cache.
        assert!(!retry_with_nix(&lock.input("sekka").unwrap()));
        assert!(retry_with_nix(&lock.input("tatami").unwrap()));
    }

    /// Asks GitHub; run with `cargo test -- --ignored` when online.
    #[test]
    #[ignore = "asks GitHub over the network"]
    fn sekka_targets_its_nightly() {
        let lock = FlakeLock::parse(
            r#"{ "root": "root", "version": 7, "nodes": {
              "root": { "inputs": { "nixpkgs": "nixpkgs", "sekka": "sekka" } },
              "nixpkgs": { "locked": { "type": "tarball", "narHash": "sha256-x", "lastModified": 1,
                             "url": "https://api.flakehub.com/f/pinned/NixOS/nixpkgs/0.1.1%2Brev-1/1/source.tar.gz" },
                           "original": { "type": "tarball", "url": "https://flakehub.com/f/NixOS/nixpkgs/0.1" } },
              "sekka": { "inputs": { "nixpkgs": ["nixpkgs"] },
                         "locked": { "type": "github", "owner": "bitemyapp", "repo": "sekka", "rev": "1", "narHash": "sha256-y", "lastModified": 1 },
                         "original": { "type": "github", "owner": "bitemyapp", "repo": "sekka", "ref": "main" } } } }"#,
        )
        .unwrap();
        let inputs = lock.inputs();
        let mut newest = BTreeMap::new();
        sekka_targets("{ inputs.sekka.url = \"github:bitemyapp/sekka/main\"; }", &inputs, &mut newest);
        let sekka = newest["sekka"].as_ref().unwrap();
        assert_eq!(sekka.rev.as_deref().map(str::len), Some(40));
        // Fetched with Git, it is asked of GitHub all the same.
        let git = FlakeLock::parse(
            r#"{ "root": "root", "version": 7, "nodes": {
              "root": { "inputs": { "sekka": "sekka" } },
              "sekka": { "locked": { "type": "git", "url": "https://github.com/bitemyapp/sekka", "ref": "nightly", "shallow": true, "rev": "1", "narHash": "sha256-y", "lastModified": 1 },
                         "original": { "type": "git", "url": "https://github.com/bitemyapp/sekka", "ref": "nightly", "shallow": true } } } }"#,
        )
        .unwrap();
        let mut newest = BTreeMap::from([("sekka".to_owned(), ask(&git.input("sekka").unwrap()).unwrap())]);
        sekka_targets(
            &format!("{{ inputs.sekka.url = \"{}\"; }}", yukimi_config::sekka::NIGHTLY),
            &git.inputs(),
            &mut newest,
        );
        assert_eq!(newest["sekka"].as_ref().unwrap().rev, sekka.rev);
    }
}
