// SPDX-License-Identifier: MIT OR Apache-2.0
//! `flake.lock`: the exact revision of every input a system is built from.
//!
//! [`FlakeLock::inputs`] lists the root's own inputs (nixpkgs, home-manager, a
//! desktop's modules) with where each comes from, the revision it is locked
//! to and when that revision was made, which is what "how out of date am I"
//! comes down to. Inputs that follow another one (`home-manager.inputs.nixpkgs
//! follows nixpkgs`) are shown as such.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, flake_url::FlakeUrl, url};

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
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
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
    /// `github:NixOS/nixpkgs/nixos-unstable`, `flakehub:NixOS/nixpkgs/0.1`,
    /// `git+https://github.com/bitemyapp/sekka?ref=nightly`.
    pub fn describe(&self) -> String {
        FlakeUrl::source(self).and_then(|source| source.render()).unwrap_or_else(|| {
            self.url
                .as_ref()
                .map(|url| flakehub(url).unwrap_or_else(|| url.clone()))
                .unwrap_or_else(|| self.kind.clone())
        })
    }

    /// Branch support comes from typed source metadata, never its display label.
    pub fn branch_changeable(&self) -> bool {
        FlakeUrl::source(self).is_some()
    }

    /// The first characters of the revision, as Git shows them.
    pub fn short_rev(&self) -> Option<&str> {
        self.rev.as_deref().map(|rev| &rev[..rev.len().min(7)])
    }

    /// The repository on github.com this source comes from, as `(owner,
    /// repo)`: a `github:` source, or a Git one cloned from
    /// `https://github.com/owner/repo`, as Sekka is so that each update
    /// downloads only what changed.
    pub fn github_repo(&self) -> Option<(String, String)> {
        match self.kind.as_str() {
            "github" if self.host.as_deref().unwrap_or("github.com").eq_ignore_ascii_case("github.com") => {
                Some((self.owner.clone()?, self.repo.clone()?))
            }
            "git" => {
                let (host, parts) = url::https_parts(self.url.as_deref()?)?;
                let [owner, repo] = parts.as_slice() else { return None };
                let repo = repo.strip_suffix(".git").unwrap_or(repo);
                let name =
                    |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
                (host == "github.com" && name(owner) && name(repo)).then(|| (owner.clone(), repo.to_owned()))
            }
            _ => None,
        }
    }

    fn web_repo(&self) -> Option<(String, String, String)> {
        if self.kind == "github" {
            Some((self.host.clone().unwrap_or_else(|| "github.com".into()), self.owner.clone()?, self.repo.clone()?))
        } else {
            let (owner, repo) = self.github_repo()?;
            Some(("github.com".into(), owner, repo))
        }
    }

    /// A web page showing this revision, for GitHub sources.
    pub fn web_url(&self) -> Option<String> {
        let (host, owner, repo) = self.web_repo()?;
        url::https(&host, &[&owner, &repo, "commit", self.rev.as_deref()?], &[])
    }

    /// Compare endpoints in one repository, without interpreting refs as URL syntax.
    pub fn compare_url(&self, target: &str) -> Option<String> {
        let revision = self.rev.as_deref()?;
        if revision == target || target.is_empty() {
            return None;
        }
        let (host, owner, repo) = self.web_repo()?;
        url::https(&host, &[&owner, &repo, "compare", &format!("{revision}..{target}")], &[])
    }
}

/// `https://flakehub.com/f/NixOS/nixpkgs/0.1` and the API's tarball URLs,
/// as `flakehub:NixOS/nixpkgs/0.1`.
struct FlakeHub {
    owner: String,
    project: String,
    version: String,
    pinned: bool,
}

impl FlakeHub {
    fn parse(value: &str) -> Option<Self> {
        let (host, parts) = url::https_parts(value)?;
        if fluent_uri::Uri::parse(value).ok()?.query().is_some() {
            return None;
        }
        let parts: Vec<_> = parts.iter().map(String::as_str).collect();
        let (owner, project, version, pinned) = match parts.as_slice() {
            ["f", owner, project, version] if host == "flakehub.com" || host == "api.flakehub.com" => {
                (*owner, *project, version.strip_suffix(".tar.gz").unwrap_or(version), false)
            }
            ["f", "pinned", owner, project, version, _tail @ ..] if host == "api.flakehub.com" => {
                (*owner, *project, *version, true)
            }
            _ => return None,
        };
        if [owner, project, version].iter().any(|s| s.is_empty() || s.contains('/')) {
            return None;
        }
        Some(Self { owner: owner.into(), project: project.into(), version: version.into(), pinned })
    }

    fn api_url(&self, endpoint: &str) -> Option<String> {
        if self.pinned {
            return None;
        }
        url::https("api.flakehub.com", &[endpoint, &self.owner, &self.project, &self.version], &[])
    }
}

fn flakehub(url: &str) -> Option<String> {
    let source = FlakeHub::parse(url)?;
    Some(format!("flakehub:{}/{}/{}", source.owner, source.project, source.version))
}

/// FlakeHub's JSON API endpoint for an unpinned release source.
pub fn flakehub_version_url(url: &str) -> Option<String> {
    FlakeHub::parse(url)?.api_url("version")
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

/// A source and its resolved inputs. Names come from input edges, not the
/// arbitrary node IDs Nix chooses when writing a lock file.
#[derive(Clone, Debug, Default)]
pub struct InputTree {
    pub locked: Option<Locked>,
    pub children: BTreeMap<String, InputTree>,
}

impl FlakeLock {
    /// Resolve an input path, including `follows` paths rooted at this lock.
    pub fn input_tree(&self, path: &[&str]) -> Result<InputTree> {
        fn resolve<'a>(raw: &'a Raw, path: &[&str], depth: usize) -> Result<&'a str> {
            if depth > 64 {
                return Err(Error::Lock("input follows paths form a cycle".into()));
            }
            let mut id = raw.root.as_str();
            for name in path {
                id = match raw.nodes.get(id).and_then(|n| n.inputs.get(*name)) {
                    Some(InputRef::Node(id)) => id,
                    Some(InputRef::Follows(path)) => {
                        resolve(raw, &path.iter().map(String::as_str).collect::<Vec<_>>(), depth + 1)?
                    }
                    None => return Err(Error::Lock(format!("input {name} is missing"))),
                };
            }
            Ok(id)
        }
        fn tree(raw: &Raw, id: &str, ancestors: &mut Vec<String>, remaining: &mut usize) -> Result<InputTree> {
            if *remaining == 0 || ancestors.len() > 64 {
                return Err(Error::Lock("input graph is too large to display".into()));
            }
            *remaining -= 1;
            let node = raw.nodes.get(id).ok_or_else(|| Error::Lock(format!("node {id} is missing")))?;
            let mut out = InputTree { locked: node.locked.clone(), ..Default::default() };
            if ancestors.iter().any(|a| a == id) {
                return Ok(out);
            }
            ancestors.push(id.to_owned());
            for (name, input) in &node.inputs {
                let child = match input {
                    InputRef::Node(id) => id,
                    InputRef::Follows(path) => resolve(raw, &path.iter().map(String::as_str).collect::<Vec<_>>(), 0)?,
                };
                out.children.insert(name.clone(), tree(raw, child, ancestors, remaining)?);
            }
            ancestors.pop();
            Ok(out)
        }
        tree(&self.raw, resolve(&self.raw, path, 0)?, &mut Vec::new(), &mut 4096)
    }

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
        FlakeHub::parse(self.url.as_deref()?)?.api_url("f")
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
    let source = FlakeHub::parse(url)?;
    if !source.pinned {
        return None;
    }
    let version = source.version;
    let (number, rev) = version.split_once("+rev-")?;
    let count = number.rsplit('.').next()?.parse().ok()?;
    Some((count, rev.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_source_urls_encode_metadata_and_preserve_transport_queries() {
        let source = Locked {
            kind: "git".into(),
            url: Some("https://github.com/o/r?token=a%2Bb%26c".into()),
            reference: Some("feature/x&y#z".into()),
            ..Default::default()
        };
        let description = source.describe();
        let parsed = FlakeUrl::parse(&description).unwrap();
        assert_eq!(parsed.reference, source.reference);
        assert_eq!(parsed.options["token"], "a+b&c");
        assert!(source.branch_changeable());
        let encoded = Locked { url: Some("https://GITHUB.com:443/%6f/r.git".into()), ..source.clone() };
        assert_eq!(encoded.github_repo(), Some(("o".into(), "r".into())));
        for url in ["https://github.com@elsewhere/o/r", "https://github.com/o%2Fr/other", "https://github.com:8443/o/r"]
        {
            assert_eq!(Locked { url: Some(url.into()), ..source.clone() }.github_repo(), None);
        }
        let pinned = Locked { rev: Some("abc".into()), ..source };
        assert_eq!(
            pinned.compare_url("feature/x&y#z").unwrap(),
            "https://github.com/o/r/compare/abc..feature%2Fx%26y%23z"
        );
    }

    #[test]
    fn flakehub_urls_are_parsed_and_rebuilt_by_component() {
        let source =
            Locked { url: Some("https://flakehub.com/f/Nix%4fS/nixpkgs/%2A.tar.gz".into()), ..Default::default() };
        assert_eq!(source.flakehub_latest_url().as_deref(), Some("https://api.flakehub.com/f/NixOS/nixpkgs/%2A"));
        assert_eq!(
            flakehub_version_url(source.url.as_deref().unwrap()).as_deref(),
            Some("https://api.flakehub.com/version/NixOS/nixpkgs/%2A")
        );
        for url in [
            "https://flakehub.com/f/o/r/*?pin=1",
            "https://flakehub.com.evil/f/o/r/*",
            "https://flakehub.com/f/o%2Fr/other/*",
            "https://api.flakehub.com/f/pinned/o/r/1+rev-abc/hash/source.tar.gz",
        ] {
            assert_eq!(flakehub_version_url(url), None, "{url}");
        }
    }

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
        assert!(flakehub("https://flakehub.com/f/o/r/100%").is_none());
    }

    #[test]
    fn git_sources_on_github() {
        let rev = "2111c84ac0b4555518b9482ec436f813d26905ef";
        let sekka = Locked {
            kind: "git".into(),
            url: Some("https://github.com/bitemyapp/sekka".into()),
            reference: Some("nightly".into()),
            rev: Some(rev.into()),
            ..Default::default()
        };
        assert_eq!(sekka.describe(), "git+https://github.com/bitemyapp/sekka?ref=nightly");
        assert_eq!(sekka.github_repo(), Some(("bitemyapp".into(), "sekka".into())));
        assert_eq!(sekka.web_url().unwrap(), format!("https://github.com/bitemyapp/sekka/commit/{rev}"));
        let tarball = Locked {
            kind: "github".into(),
            owner: Some("bitemyapp".into()),
            repo: Some("sekka".into()),
            rev: Some("5d1d471".into()),
            ..Default::default()
        };
        // Moving from the tarball to Git still compares within one repository.
        assert_eq!(tarball.github_repo(), sekka.github_repo());
        assert_eq!(
            tarball.compare_url(rev).unwrap(),
            format!("https://github.com/bitemyapp/sekka/compare/5d1d471..{rev}")
        );
        let dotted = Locked { url: Some("https://github.com/o/r.git".into()), ..sekka.clone() };
        assert_eq!(dotted.github_repo(), Some(("o".into(), "r".into())));
        for elsewhere in
            ["https://example.org/o/r", "https://github.com/o", "https://github.com/o/r/tree/x", "file:///x"]
        {
            let other = Locked { url: Some(elsewhere.into()), ..sekka.clone() };
            assert_eq!(other.github_repo(), None, "{elsewhere}");
            assert_eq!(other.web_url(), None, "{elsewhere}");
        }
        let enterprise = Locked { host: Some("git.example.org".into()), ..tarball };
        assert_eq!(enterprise.github_repo(), None);
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
