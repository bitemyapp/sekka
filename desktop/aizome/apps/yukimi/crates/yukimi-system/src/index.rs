// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every package in Nixpkgs, searchable.
//!
//! The index is built once for each revision of Nixpkgs the system uses, by
//! asking Nix to describe every top-level package ([`expression`]): its
//! name, version, description, whether it is free software, broken or
//! available on this machine, and the program it provides. That takes a
//! minute; the result is cached, and searching it ([`PackageIndex::search`])
//! is instant.
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    /// The attribute name, which installs it: `htop`.
    pub attr: String,
    pub pname: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub long_description: String,
    #[serde(default)]
    pub homepage: String,
    /// The command it provides, when Nixpkgs says.
    #[serde(default)]
    pub main_program: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub unfree: bool,
    #[serde(default)]
    pub broken: bool,
    #[serde(default)]
    pub insecure: bool,
    /// Builds for this machine's platform.
    #[serde(default = "yes")]
    pub available: bool,
}

fn yes() -> bool {
    true
}

/// The Nix expression describing every top-level package of the Nixpkgs at
/// `nixpkgs` (a store path), as a JSON list. Evaluated with
/// `nix eval --json --impure --expr`.
pub fn expression(nixpkgs: &str) -> String {
    format!(
        r#"let
  pkgs = import {nixpkgs} {{
    config = {{ allowUnfree = true; allowBroken = true; allowUnsupportedSystem = true; allowInsecurePredicate = _: true; }};
    overlays = [ ];
  }};
  lib = pkgs.lib;
  text = value: if builtins.isString value then value else "";
  first = value: if builtins.isList value then (if value == [ ] then "" else text (builtins.head value)) else text value;
  licenses = value: let l = value.meta.license or [ ]; in if builtins.isList l then l else [ l ];
  describe = attr: value:
    let
      isPackage = builtins.tryEval (lib.isDerivation value && !(value.meta.hidden or false));
      parsed = builtins.parseDrvName (value.name or "");
      info = {{
        inherit attr;
        pname = text (value.pname or parsed.name);
        version = text (value.version or parsed.version);
        description = text (value.meta.description or "");
        longDescription = text (value.meta.longDescription or "");
        homepage = first (value.meta.homepage or "");
        mainProgram = text (value.meta.mainProgram or "");
        license = lib.concatMapStringsSep ", " (l: if builtins.isAttrs l then text (l.spdxId or l.shortName or "") else text l) (licenses value);
        unfree = builtins.any (l: builtins.isAttrs l && !(l.free or true)) (licenses value);
        broken = value.meta.broken or false;
        insecure = (value.meta.knownVulnerabilities or [ ]) != [ ];
        available = lib.meta.availableOn pkgs.stdenv.hostPlatform value;
      }};
      checked = builtins.tryEval (builtins.deepSeq info info);
    in
    if isPackage.success && isPackage.value && checked.success then [ checked.value ] else [ ];
in
builtins.concatLists (lib.mapAttrsToList describe pkgs)"#
    )
}

/// Where the index of the Nixpkgs at `nixpkgs` is kept in `cache`.
fn cache_file(cache: &Path, nixpkgs: &str) -> Option<PathBuf> {
    let name = Path::new(nixpkgs).file_name()?.to_string_lossy().into_owned();
    Some(cache.join(format!("packages-{name}.json")))
}

/// The cached indexes, newest first.
fn cached(cache: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("packages-") && name.ends_with(".json")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files.into_iter().map(|(_, path)| path).collect()
}

/// The cached index of this Nixpkgs (`true`), or else of the one indexed
/// most recently (`false`), to search while this one's is made.
pub fn load_cached(cache: &Path, nixpkgs: &str) -> Option<(PackageIndex, bool)> {
    let wanted = cache_file(cache, nixpkgs)?;
    if let Some(index) = std::fs::read_to_string(&wanted).ok().and_then(|t| PackageIndex::parse(&t).ok()) {
        return Some((index, true));
    }
    cached(cache).into_iter().find_map(|file| {
        let index = PackageIndex::parse(&std::fs::read_to_string(file).ok()?).ok()?;
        Some((index, false))
    })
}

/// Make the index of the Nixpkgs at `nixpkgs` and keep it in `cache`, with
/// the one before it (older ones go). Nix runs at the lowest priority, as
/// it is busy for a minute or so.
pub fn make(cache: &Path, nixpkgs: &str) -> Result<PackageIndex, String> {
    let file = cache_file(cache, nixpkgs).ok_or("Nixpkgs isn't a store path")?;
    let mut command = crate::nix::command();
    command
        .args(["eval", "--json", "--impure", "--expr", &expression(nixpkgs)])
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    // SAFETY: setpriority is async-signal-safe and touches only this process.
    unsafe {
        command.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, 19);
            Ok(())
        });
    }
    let output = command.output().map_err(|e| format!("Nix could not be started: {e}"))?;
    if !output.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&output.stderr));
        return Err(crate::nix::last_error(&stderr));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let index = PackageIndex::parse(&text).map_err(|e| format!("The package list could not be read: {e}"))?;
    let _ = std::fs::create_dir_all(cache);
    let partial = file.with_extension("new");
    if std::fs::write(&partial, text.as_bytes()).is_ok() {
        let _ = std::fs::rename(&partial, &file);
    }
    for old in cached(cache).into_iter().skip(2) {
        let _ = std::fs::remove_file(old);
    }
    Ok(index)
}

/// A loaded index.
#[derive(Default)]
pub struct PackageIndex {
    packages: Vec<Package>,
    /// Lowercased search text for each package, in the same order.
    haystacks: Vec<(String, String, String)>,
}

impl PackageIndex {
    pub fn new(mut packages: Vec<Package>) -> PackageIndex {
        packages.sort_by(|a, b| a.attr.cmp(&b.attr));
        let haystacks = packages
            .iter()
            .map(|p| (p.attr.to_lowercase(), p.pname.to_lowercase(), p.description.to_lowercase()))
            .collect();
        PackageIndex { packages, haystacks }
    }

    pub fn parse(json: &str) -> serde_json::Result<PackageIndex> {
        Ok(PackageIndex::new(serde_json::from_str(json)?))
    }

    pub fn len(&self) -> usize {
        self.packages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    pub fn get(&self, attr: &str) -> Option<&Package> {
        self.packages.binary_search_by(|p| p.attr.as_str().cmp(attr)).ok().map(|i| &self.packages[i])
    }

    /// Packages matching every word of the query, best first: exact names,
    /// then names starting with the query, names containing it, and
    /// descriptions. Broken and unavailable packages sink.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Package> {
        let query = query.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(i64, usize)> = self
            .haystacks
            .iter()
            .enumerate()
            .filter_map(|(i, (attr, pname, description))| {
                let mut score = 0i64;
                for (n, word) in words.iter().enumerate() {
                    let best = if attr == word || pname == word {
                        1000
                    } else if pname.starts_with(word) || attr.starts_with(word) {
                        600 - (pname.len().min(60) as i64)
                    } else if pname.contains(word) || attr.contains(word) {
                        350 - (pname.len().min(60) as i64)
                    } else if contains_word(description, word) {
                        120
                    } else if description.contains(word) {
                        60
                    } else {
                        return None;
                    };
                    // The first word matters most.
                    score += if n == 0 { best * 2 } else { best };
                }
                let p = &self.packages[i];
                // Broken and unavailable packages cannot be installed: below
                // every working match.
                if p.broken || !p.available {
                    score -= 10_000;
                }
                if p.description.is_empty() {
                    score -= 40;
                }
                // Prefer top-level names over versioned variants (`firefox` over
                // `firefox-esr-128-unwrapped`).
                score -= p.attr.matches(['-', '_']).count() as i64 * 8;
                Some((score, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| self.packages[a.1].attr.cmp(&self.packages[b.1].attr)));
        scored.into_iter().take(limit).map(|(_, i)| &self.packages[i]).collect()
    }
}

fn contains_word(text: &str, word: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|w| w == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(attr: &str, description: &str) -> Package {
        Package {
            attr: attr.into(),
            pname: attr.into(),
            version: "1.0".into(),
            description: description.into(),
            available: true,
            ..Default::default()
        }
    }

    fn index() -> PackageIndex {
        let mut broken = pkg("firefox-broken", "Web browser");
        broken.broken = true;
        PackageIndex::new(vec![
            pkg("firefox-esr-unwrapped", "Web browser built from Firefox source tree"),
            pkg("firefox", "Web browser built from Firefox source tree"),
            pkg("librewolf", "Fork of Firefox, focused on privacy"),
            pkg("htop", "Interactive process viewer"),
            pkg("btop", "Monitor of resources"),
            broken,
        ])
    }

    #[test]
    fn search_ranks_names_then_descriptions() {
        let idx = index();
        let attrs: Vec<&str> = idx.search("firefox", 10).iter().map(|p| p.attr.as_str()).collect();
        assert_eq!(attrs[0], "firefox");
        assert_eq!(attrs[1], "firefox-esr-unwrapped");
        assert!(attrs.contains(&"librewolf"));
        assert_eq!(attrs.last(), Some(&"firefox-broken"));
        let words: Vec<&str> = idx.search("process viewer", 10).iter().map(|p| p.attr.as_str()).collect();
        assert_eq!(words, vec!["htop"]);
        assert!(idx.search("   ", 10).is_empty());
        assert_eq!(idx.get("htop").unwrap().description, "Interactive process viewer");
    }

    #[test]
    fn expression_mentions_the_nixpkgs_path() {
        let text = expression("/nix/store/0123456789abcdfghijklmnpqrsvwxyz-source");
        assert!(text.contains("import /nix/store/0123456789abcdfghijklmnpqrsvwxyz-source {"));
        let json = r#"[{"attr":"htop","pname":"htop","version":"3.4.1","description":"Interactive process viewer",
          "longDescription":"","homepage":"https://htop.dev","mainProgram":"htop","license":"GPL-2.0-only",
          "unfree":false,"broken":false,"insecure":false,"available":true}]"#;
        let idx = PackageIndex::parse(json).unwrap();
        assert_eq!(idx.get("htop").unwrap().main_program, "htop");
    }
}
