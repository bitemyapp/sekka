// SPDX-License-Identifier: MIT OR Apache-2.0
//! `flake.lock`: the exact revision of every input a system is built from.
//!
//! [`FlakeLock::inputs`] lists the root's own inputs (nixpkgs, home-manager, a
//! desktop's modules) with where each comes from, the revision it is locked
//! to and when that revision was made, which is what "how out of date am I"
//! comes down to. Inputs that follow another one (`home-manager.inputs.nixpkgs
//! follows nixpkgs`) are shown as such.
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{Error, Result};

#[derive(Debug, Deserialize)]
struct Raw {
    nodes: BTreeMap<String, Node>,
    root: String,
}

#[derive(Debug, Deserialize)]
struct Node {
    #[serde(default)]
    inputs: BTreeMap<String, InputRef>,
    locked: Option<Locked>,
    original: Option<Locked>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InputRef {
    Node(String),
    Follows(Vec<String>),
}

/// A source as `flake.lock` records it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Locked {
    #[serde(rename = "type", default)]
    pub kind: String,
    pub owner: Option<String>,
    pub repo: Option<String>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub rev: Option<String>,
    pub url: Option<String>,
    pub host: Option<String>,
    pub last_modified: Option<i64>,
    /// How many commits the revision has, when the source says.
    pub rev_count: Option<u64>,
    pub nar_hash: Option<String>,
}

impl Locked {
    /// A short description of where the source comes from:
    /// `github:NixOS/nixpkgs/nixos-unstable`, `flakehub:NixOS/nixpkgs/0.1`.
    pub fn describe(&self) -> String {
        match (self.kind.as_str(), &self.owner, &self.repo) {
            ("github" | "gitlab" | "sourcehut", Some(owner), Some(repo)) => {
                let mut text = format!("{}:{owner}/{repo}", self.kind);
                if let Some(reference) = &self.reference {
                    text.push('/');
                    text.push_str(reference);
                }
                text
            }
            _ => match &self.url {
                Some(url) => flakehub(url).unwrap_or_else(|| url.clone()),
                None => self.kind.clone(),
            },
        }
    }

    /// The first characters of the revision, as Git shows them.
    pub fn short_rev(&self) -> Option<&str> {
        self.rev.as_deref().map(|rev| &rev[..rev.len().min(7)])
    }

    /// A web page showing this revision, for GitHub sources.
    pub fn web_url(&self) -> Option<String> {
        match (self.kind.as_str(), &self.owner, &self.repo, &self.rev) {
            ("github", Some(owner), Some(repo), Some(rev)) => {
                Some(format!("https://github.com/{owner}/{repo}/commit/{rev}"))
            }
            _ => None,
        }
    }
}

/// `https://flakehub.com/f/NixOS/nixpkgs/0.1` and the API's tarball URLs,
/// as `flakehub:NixOS/nixpkgs/0.1`.
fn flakehub(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://flakehub.com/f/")
        .or_else(|| url.strip_prefix("https://api.flakehub.com/f/pinned/"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [owner, project, version, ..] => {
            Some(format!("flakehub:{owner}/{project}/{}", unescape(version.trim_end_matches(".tar.gz"))))
        }
        _ => None,
    }
}

/// Undo URL escapes: FlakeHub's `*` (any version) is written `%2A`.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// One of the root's inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    /// What it is locked to; `None` when it follows another input.
    pub locked: Option<Locked>,
    /// What `flake.nix` asks for (a branch, a version range).
    pub original: Option<Locked>,
    /// The input path this one follows, such as `["nixpkgs"]`.
    pub follows: Option<Vec<String>>,
    /// Root inputs that this input's own inputs follow, such as `nixpkgs`
    /// for `home-manager`.
    pub follows_inputs: Vec<String>,
}

/// How updating an input would move it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Movement {
    /// It is already at the newest version.
    Current,
    /// There is a newer version.
    Newer(Locked),
    /// What `flake.nix` asks for now points at a version older than the one
    /// locked (a branch was moved back, or the lock was made from somewhere
    /// else), so updating would go back in time.
    Older(Locked),
}

/// How each locked input in `current` would move, given `updated`: the same
/// flake's inputs after `nix flake update`.
pub fn compare(current: &[Input], updated: &[Input]) -> BTreeMap<String, Movement> {
    current
        .iter()
        .filter_map(|input| {
            let now = input.locked.as_ref()?;
            let next = updated.iter().find(|u| u.name == input.name)?.locked.clone()?;
            // The content hash decides; the revision when there is none.
            let same = match (&now.nar_hash, &next.nar_hash, &now.rev, &next.rev) {
                (Some(a), Some(b), _, _) => a == b,
                (_, _, Some(a), Some(b)) => a == b,
                _ => false,
            };
            let movement = match (now.last_modified, next.last_modified) {
                _ if same => Movement::Current,
                (Some(before), Some(after)) if after < before => Movement::Older(next),
                _ => Movement::Newer(next),
            };
            Some((input.name.clone(), movement))
        })
        .collect()
}

/// A parsed `flake.lock`.
#[derive(Debug)]
pub struct FlakeLock {
    raw: Raw,
}

impl FlakeLock {
    pub fn parse(text: &str) -> Result<FlakeLock> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| Error::Lock(e.to_string()))?;
        if !raw.nodes.contains_key(&raw.root) {
            return Err(Error::Lock(format!("the root node {:?} is missing", raw.root)));
        }
        Ok(FlakeLock { raw })
    }

    /// The root's inputs, by name.
    pub fn inputs(&self) -> Vec<Input> {
        let root = &self.raw.nodes[&self.raw.root];
        root.inputs
            .iter()
            .map(|(name, input)| match input {
                InputRef::Node(node) => {
                    let found = self.raw.nodes.get(node);
                    let follows_inputs = found
                        .map(|n| {
                            n.inputs
                                .values()
                                .filter_map(|r| match r {
                                    InputRef::Follows(path) if path.len() == 1 && path[0] != *name => {
                                        Some(path[0].clone())
                                    }
                                    _ => None,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Input {
                        name: name.clone(),
                        locked: found.and_then(|n| n.locked.clone()),
                        original: found.and_then(|n| n.original.clone()),
                        follows: None,
                        follows_inputs,
                    }
                }
                InputRef::Follows(path) => Input {
                    name: name.clone(),
                    locked: None,
                    original: None,
                    follows: Some(path.clone()),
                    follows_inputs: Vec::new(),
                },
            })
            .collect()
    }

    /// One input, by name.
    pub fn input(&self, name: &str) -> Option<Input> {
        self.inputs().into_iter().find(|input| input.name == name)
    }
}

/// A node with every input it refers to written out in full, so that two
/// lock files can be compared input by input whatever they name their
/// nodes. `follows` paths stay as they are.
fn expanded(nodes: &serde_json::Map<String, serde_json::Value>, id: &str, depth: usize) -> Result<serde_json::Value> {
    if depth > 64 {
        return Err(Error::Lock("its inputs refer to each other in a loop".into()));
    }
    let mut node = nodes.get(id).cloned().ok_or_else(|| Error::Lock(format!("the node {id:?} is missing")))?;
    if let Some(inputs) = node.get_mut("inputs").and_then(|i| i.as_object_mut()) {
        for value in inputs.values_mut() {
            if let Some(child) = value.as_str().map(str::to_owned) {
                *value = expanded(nodes, &child, depth + 1)?;
            }
        }
    }
    Ok(node)
}

/// The nodes and the root's inputs of a lock file, as JSON.
fn raw_parts(text: &str) -> Result<(serde_json::Map<String, serde_json::Value>, serde_json::Value)> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| Error::Lock(e.to_string()))?;
    let nodes = value["nodes"].as_object().cloned().ok_or_else(|| Error::Lock("it has no nodes".into()))?;
    let root = value["root"].as_str().ok_or_else(|| Error::Lock("it has no root".into()))?;
    let inputs = nodes.get(root).map(|n| n["inputs"].clone()).unwrap_or_default();
    Ok((nodes, inputs))
}

/// Check that `proposed` is `current` with only the inputs in `updated`
/// moved, each to another version of the same source: the same inputs, each
/// one not updated exactly as it was (with everything it brings), and each
/// updated one still asking for what it asked for before. This is what the
/// helper requires of a lock file Yukimi prepared without administrator
/// rights before putting it in place.
pub fn check_update(current: &str, proposed: &str, updated: &[String]) -> Result<()> {
    let (current_nodes, current_inputs) = raw_parts(current)?;
    let (proposed_nodes, proposed_inputs) = raw_parts(proposed)?;
    let (Some(before), Some(after)) = (current_inputs.as_object(), proposed_inputs.as_object()) else {
        return Err(Error::Lock("the root has no inputs".into()));
    };
    if before.keys().ne(after.keys()) {
        return Err(Error::Lock("it has different inputs".into()));
    }
    for (name, was) in before {
        let now = &after[name];
        let (was_node, now_node) = match (was.as_str(), now.as_str()) {
            (Some(a), Some(b)) => (expanded(&current_nodes, a, 0)?, expanded(&proposed_nodes, b, 0)?),
            // A follows path stays the same path.
            _ if was == now => continue,
            _ => return Err(Error::Lock(format!("{name} follows something else"))),
        };
        if !updated.contains(name) {
            if was_node != now_node {
                return Err(Error::Lock(format!("{name} changed, but only {} were to", updated.join(", "))));
            }
        } else if was_node.get("original") != now_node.get("original") || now_node.get("locked").is_none() {
            return Err(Error::Lock(format!("{name} comes from somewhere else")));
        }
    }
    Ok(())
}

impl Locked {
    /// For a FlakeHub source, the address that redirects to its newest
    /// release matching the version asked for: `https://api.flakehub.com/f/NixOS/nixpkgs/0.1`.
    pub fn flakehub_latest_url(&self) -> Option<String> {
        let url = self.url.as_deref()?;
        let rest =
            url.strip_prefix("https://flakehub.com/f/").or_else(|| url.strip_prefix("https://api.flakehub.com/f/"))?;
        let rest = rest.strip_suffix(".tar.gz").unwrap_or(rest);
        (rest.split('/').count() == 3 && !rest.starts_with("pinned/"))
            .then(|| format!("https://api.flakehub.com/f/{rest}"))
    }

    /// For a FlakeHub release, its commit count and revision, from the
    /// address it is pinned at:
    /// `…/pinned/NixOS/nixpkgs/0.1.1086391%2Brev-151fa4e…/…/source.tar.gz`.
    pub fn flakehub_release(&self) -> Option<(u64, String)> {
        flakehub_release(self.url.as_deref()?)
    }
}

/// The commit count and revision a pinned FlakeHub address names.
pub fn flakehub_release(url: &str) -> Option<(u64, String)> {
    let version = url.strip_prefix("https://api.flakehub.com/f/pinned/")?.split('/').nth(2)?;
    let version = unescape(version);
    let (number, rev) = version.split_once("+rev-")?;
    let count = number.rsplit('.').next()?.parse().ok()?;
    Some((count, rev.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"{
      "nodes": {
        "calamares": {
          "inputs": { "nixpkgs": ["nixpkgs"], "tatami": ["tatami"] },
          "locked": { "lastModified": 1791340000, "narHash": "sha256-x", "owner": "bitemyapp", "repo": "calamares",
                      "rev": "0d10e385c48d2c5be463aeb839d73047dc770665", "type": "github" },
          "original": { "owner": "bitemyapp", "ref": "stable", "repo": "calamares", "type": "github" }
        },
        "nixpkgs": {
          "locked": { "lastModified": 1790600678, "narHash": "sha256-y", "rev": "f45c6f04c2f013f004bf94e284e95d72898d9393",
                      "type": "tarball", "url": "https://api.flakehub.com/f/pinned/NixOS/nixpkgs/0.1.912345%2Brev-f45c6f04/0199/source.tar.gz" },
          "original": { "type": "tarball", "url": "https://flakehub.com/f/NixOS/nixpkgs/0.1" }
        },
        "tatami": {
          "inputs": { "nixpkgs": ["nixpkgs"] },
          "locked": { "lastModified": 1791338000, "owner": "bitemyapp", "repo": "tatami", "rev": "b925d7a9c9", "type": "github" },
          "original": { "owner": "bitemyapp", "ref": "stable", "repo": "tatami", "type": "github" }
        },
        "root": { "inputs": { "calamares": "calamares", "nixpkgs": "nixpkgs", "tatami": "tatami", "pkgs": ["nixpkgs"] } }
      },
      "root": "root",
      "version": 7
    }"#;

    #[test]
    fn root_inputs_with_sources_and_ages() {
        let lock = FlakeLock::parse(LOCK).unwrap();
        let inputs = lock.inputs();
        assert_eq!(inputs.len(), 4);
        let calamares = lock.input("calamares").unwrap();
        let locked = calamares.locked.unwrap();
        assert_eq!(locked.short_rev(), Some("0d10e38"));
        assert_eq!(locked.last_modified, Some(1791340000));
        assert_eq!(calamares.original.unwrap().describe(), "github:bitemyapp/calamares/stable");
        assert_eq!(calamares.follows_inputs, vec!["nixpkgs".to_owned(), "tatami".to_owned()]);
        let nixpkgs = lock.input("nixpkgs").unwrap();
        assert_eq!(nixpkgs.original.unwrap().describe(), "flakehub:NixOS/nixpkgs/0.1");
        assert_eq!(lock.input("pkgs").unwrap().follows, Some(vec!["nixpkgs".to_owned()]));
        assert_eq!(
            lock.input("tatami").unwrap().locked.unwrap().web_url().as_deref(),
            Some("https://github.com/bitemyapp/tatami/commit/b925d7a9c9")
        );
        assert!(FlakeLock::parse("{}").is_err());
    }

    #[test]
    fn updates_compared_with_the_lock() {
        let current = FlakeLock::parse(LOCK).unwrap().inputs();
        let updated = FlakeLock::parse(
            &LOCK
                // nixpkgs moved forward, calamares's branch moved back, tatami stayed.
                .replace(
                    "\"lastModified\": 1790600678, \"narHash\": \"sha256-y\"",
                    "\"lastModified\": 1791000000, \"narHash\": \"sha256-z\"",
                )
                .replace(
                    "\"lastModified\": 1791340000, \"narHash\": \"sha256-x\"",
                    "\"lastModified\": 1790000000, \"narHash\": \"sha256-w\"",
                )
                .replace("0d10e385c48d2c5be463aeb839d73047dc770665", "0cbb39e2c42ab4b0a05690a7727dd69dea051545")
                .replace("f45c6f04c2f013f004bf94e284e95d72898d9393", "a7868a7f9e0d2c5be463aeb839d73047dc770665"),
        )
        .unwrap()
        .inputs();
        let moves = compare(&current, &updated);
        assert!(matches!(&moves["nixpkgs"], Movement::Newer(l) if l.last_modified == Some(1791000000)));
        assert!(matches!(&moves["calamares"], Movement::Older(l) if l.short_rev() == Some("0cbb39e")));
        assert_eq!(moves["tatami"], Movement::Current);
        // Inputs that follow another have nothing of their own to move.
        assert!(!moves.contains_key("pkgs"));
    }

    #[test]
    fn flakehub_versions_unescaped() {
        assert_eq!(
            flakehub("https://flakehub.com/f/DeterminateSystems/fh/%2A.tar.gz").as_deref(),
            Some("flakehub:DeterminateSystems/fh/*")
        );
        assert_eq!(
            flakehub("https://api.flakehub.com/f/pinned/NixOS/nixpkgs/0.1.912345%2Brev-f45c6f04/0199/source.tar.gz")
                .as_deref(),
            Some("flakehub:NixOS/nixpkgs/0.1.912345+rev-f45c6f04")
        );
        assert_eq!(unescape("100%"), "100%");
    }

    #[test]
    fn flakehub_releases() {
        let nixpkgs = FlakeLock::parse(LOCK).unwrap().input("nixpkgs").unwrap();
        assert_eq!(nixpkgs.locked.unwrap().flakehub_release(), Some((912345, "f45c6f04".to_owned())));
        assert_eq!(
            nixpkgs.original.unwrap().flakehub_latest_url().as_deref(),
            Some("https://api.flakehub.com/f/NixOS/nixpkgs/0.1")
        );
        let fh = Locked {
            url: Some("https://flakehub.com/f/DeterminateSystems/fh/%2A.tar.gz".into()),
            ..Default::default()
        };
        assert_eq!(fh.flakehub_latest_url().as_deref(), Some("https://api.flakehub.com/f/DeterminateSystems/fh/%2A"));
        assert_eq!(Locked::default().flakehub_latest_url(), None);
    }

    #[test]
    fn updates_must_keep_everything_else() {
        let nixpkgs_moved = LOCK.replace("\"narHash\": \"sha256-y\"", "\"narHash\": \"sha256-z\"");
        let only = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(check_update(LOCK, &nixpkgs_moved, &only(&["nixpkgs"])), Ok(()));
        // Moving an input that was not to move is refused.
        assert!(check_update(LOCK, &nixpkgs_moved, &only(&["tatami"])).is_err());
        // So is pointing an input somewhere else.
        let elsewhere =
            LOCK.replace("\"ref\": \"stable\", \"repo\": \"tatami\"", "\"ref\": \"main\", \"repo\": \"tatami\"");
        assert!(check_update(LOCK, &elsewhere, &only(&["tatami"])).is_err());
        // Or adding one, or changing what one follows.
        let added = LOCK.replace("\"pkgs\": [\"nixpkgs\"]", "\"pkgs\": [\"nixpkgs\"], \"x\": \"tatami\"");
        assert!(check_update(LOCK, &added, &only(&["x"])).is_err());
        let follows = LOCK.replace("\"pkgs\": [\"nixpkgs\"]", "\"pkgs\": [\"tatami\"]");
        assert!(check_update(LOCK, &follows, &only(&["pkgs"])).is_err());
        // What the nodes are called doesn't matter, only what they hold.
        let renamed = LOCK
            .replace("\"tatami\": \"tatami\", \"pkgs\"", "\"tatami\": \"tatami_2\", \"pkgs\"")
            .replace("\"tatami\": {\n          \"inputs\"", "\"tatami_2\": {\n          \"inputs\"");
        assert_eq!(check_update(LOCK, &renamed, &only(&[])), Ok(()));
        assert!(check_update(LOCK, "{}", &only(&[])).is_err());
    }
}
