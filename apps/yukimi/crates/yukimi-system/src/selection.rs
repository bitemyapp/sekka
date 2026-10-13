// SPDX-License-Identifier: MIT OR Apache-2.0
//! Choosing what an update changes, package by package.
//!
//! An update moves the system's sources together; what it changes for the
//! person is the packages the system installs. The system as it runs is
//! read from its system profile's derivation, which lists the outputs of
//! each package it links; the updated system's profile is evaluated
//! (nothing is built) in a copy of the configuration updated as the helper
//! would, without holds. A package left out of the update is kept at the
//! version the system has: Yukimi's module takes it from the saved
//! configuration the system was built with (`programs.yukimi.held`), the
//! very same build.
//!
//! The comparison is worked out once for each Sekka revision a check finds
//! (Yukimi starts it as soon as the check does) and kept, with the lock
//! file it was worked out with, so that the update builds what was shown.
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::nix;

/// One package of a system profile: the outputs of it the profile links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// Its name without its version, which a hold is by.
    pub key: String,
    pub version: String,
    pub paths: Vec<String>,
}

/// A package the update changes, which can be kept as it is instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageChange {
    pub name: String,
    /// Its version now, and after the update (a build can change with the
    /// same version, when what it is built with changed).
    pub before: String,
    pub after: String,
    /// Kept at the version the system has by an earlier choice.
    pub held: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub changes: Vec<PackageChange>,
    /// Packages the update adds or takes away, which come with it.
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Every package kept as it is now, whether the update changes it or not.
    pub held: BTreeSet<String>,
}

/// What to compare.
#[derive(Clone, Debug)]
pub struct Request {
    /// The configuration's directory, and the configuration as Nix is
    /// given it.
    pub dir: PathBuf,
    pub flake: String,
    pub host: String,
    /// Yukimi's own file, which holds packages.
    pub packages_file: Option<PathBuf>,
    /// The inputs the update moves.
    pub update: Vec<String>,
    /// The Sekka revision the last check found.
    pub target: String,
    /// Where comparisons and their copies of the configuration are kept.
    pub cache: PathBuf,
}

/// A comparison worked out, and the lock file it was worked out with.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prepared {
    key: String,
    pub preview: Preview,
    /// The Sekka revision the update locks.
    pub rev: String,
    /// The lock file to update with, so that the update builds what was
    /// shown. None when the update also converts the flake, which the
    /// helper then locks itself.
    pub lock: Option<PathBuf>,
}

/// A name and a version, as Nix's `builtins.parseDrvName` splits them: at
/// the first dash not followed by a letter.
pub fn parse_drv_name(name: &str) -> (&str, &str) {
    let bytes = name.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'-' && bytes.get(i + 1).is_some_and(|b| !b.is_ascii_alphabetic()) {
            return (&name[..i], &name[i + 1..]);
        }
    }
    (name, "")
}

/// Outputs a profile links besides `out`, whose name a store path carries
/// after the version (`git-2.50-doc`).
const OUTPUTS: &[&str] = &["bin", "man", "dev", "lib", "doc", "info", "data", "debug", "devdoc", "terminfo"];

/// The packages a system profile's derivation links: NixOS's `buildEnv`
/// lists each package's chosen outputs (`chosenOutputs`, or `pkgs` before
/// it was called that).
pub fn profile_packages(derivation: &serde_json::Value) -> Result<Vec<Installed>, String> {
    let all = derivation.get("derivations").unwrap_or(derivation);
    let info = all.as_object().and_then(|d| d.values().next()).ok_or("the derivation is missing")?;
    let attrs = match info.get("structuredAttrs") {
        Some(attrs) => attrs.clone(),
        None => info["env"]["__json"]
            .as_str()
            .and_then(|json| serde_json::from_str(json).ok())
            .ok_or("the profile's derivation doesn't list its packages")?,
    };
    let entries = attrs.get("chosenOutputs").or_else(|| attrs.get("pkgs")).and_then(|e| e.as_array());
    let entries = entries.ok_or("the profile's derivation doesn't list its packages")?;
    let mut packages = Vec::new();
    for entry in entries {
        let mut paths: Vec<String> =
            entry["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
        let Some(first) = paths.first() else { continue };
        let name = first.rsplit('/').next().unwrap_or(first).get(33..).unwrap_or_default();
        let (key, version) = parse_drv_name(name);
        let version = OUTPUTS.iter().find_map(|o| version.strip_suffix(&format!("-{o}"))).unwrap_or(version);
        let (key, version) = (key.to_owned(), version.to_owned());
        paths.sort();
        packages.push(Installed { key, version, paths });
    }
    Ok(packages)
}

/// The packages of the system profile a derivation builds.
fn profile_of(drv: &str) -> Result<Vec<Installed>, String> {
    let json = run(nix::command().args(["derivation", "show", drv]))?;
    profile_packages(&serde_json::from_str(&json).map_err(|e| e.to_string())?)
}

/// The system profile's derivation of a flake's configuration (evaluated,
/// not built).
fn profile_derivation(flake: &str, host: &str) -> Result<String, String> {
    let attr = format!("{flake}#nixosConfigurations.\"{host}\".config.system.path.drvPath");
    Ok(run(nix::command().args(["eval", "--raw", &attr]))?.trim().to_owned())
}

/// The running system's profile and the derivation it was built from.
fn running_profile() -> Option<(PathBuf, String)> {
    let profile = std::fs::canonicalize("/run/current-system/sw").ok()?;
    let out = std::process::Command::new("nix-store")
        .args(["--query", "--deriver"])
        .arg(&profile)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let drv = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && Path::new(&drv).exists()).then_some((profile, drv))
}

fn run(command: &mut std::process::Command) -> Result<String, String> {
    let out = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Nix couldn't be started: {e}"))?;
    if !out.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&out.stderr));
        return Err(crate::updates::first_line(&nix::last_error(&stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What changes between two lists of packages, by name.
pub fn compare(before: &[Installed], after: &[Installed], held: &BTreeSet<String>) -> Preview {
    let group = |list: &[Installed]| {
        let mut by_name: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
        for p in list {
            let entry = by_name.entry(p.key.clone()).or_default();
            entry.0.extend(p.paths.iter().cloned());
            if !p.version.is_empty() {
                entry.1.insert(p.version.clone());
            }
        }
        by_name
    };
    let versions = |set: &BTreeSet<String>| set.iter().cloned().collect::<Vec<_>>().join(", ");
    let (was, will) = (group(before), group(after));
    let mut preview = Preview { held: held.clone(), ..Preview::default() };
    for (name, (paths, after)) in &will {
        match was.get(name) {
            None => preview.added.push(name.clone()),
            Some((old_paths, before)) if old_paths != paths => preview.changes.push(PackageChange {
                name: name.clone(),
                before: versions(before),
                after: versions(after),
                held: held.contains(name),
            }),
            Some(_) => {}
        }
    }
    preview.removed = was.keys().filter(|name| !will.contains_key(*name)).cloned().collect();
    preview
}

/// The files of a configuration as the helper keeps one: not hidden files,
/// build results or files over a megabyte.
fn configuration_files(dir: &Path, below: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir.join(below))? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') || text.starts_with("result") {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            configuration_files(dir, &below.join(&name), out)?;
        } else if kind.is_file() && entry.metadata()?.len() <= 1 << 20 {
            out.push(below.join(&name));
        }
    }
    Ok(())
}

fn copy_configuration(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut files = Vec::new();
    configuration_files(from, Path::new(""), &mut files)?;
    for file in files {
        if let Some(parent) = to.join(&file).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from.join(&file), to.join(&file))?;
    }
    Ok(())
}

/// What a comparison depends on: the running system, the configuration's
/// files, what the update moves and to which Sekka.
fn key(request: &Request) -> Result<String, String> {
    let mut files = Vec::new();
    configuration_files(&request.dir, Path::new(""), &mut files).map_err(|e| e.to_string())?;
    files.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for file in &files {
        file.hash(&mut hasher);
        std::fs::read(request.dir.join(file)).map_err(|e| e.to_string())?.hash(&mut hasher);
    }
    let running = running_profile().map(|(profile, _)| profile.display().to_string()).unwrap_or_default();
    Ok(format!("{running} {:016x} {} {}", hasher.finish(), request.update.join(","), request.target))
}

/// Work out what the update changes.
fn work_out(request: &Request, key: String) -> Result<Prepared, String> {
    let before = match running_profile() {
        Some((_, drv)) => profile_of(&drv)?,
        // Without the running profile's derivation, the configuration as it
        // is stands in for it.
        None => profile_of(&profile_derivation(&request.flake, &request.host)?)?,
    };
    let scratch = request.cache.join("preview");
    let _ = std::fs::remove_dir_all(&scratch);
    copy_configuration(&request.dir, &scratch).map_err(|e| format!("Copying the configuration: {e}"))?;
    let flake_file = scratch.join("flake.nix");
    let text = std::fs::read_to_string(&flake_file).map_err(|e| format!("{}: {e}", flake_file.display()))?;
    let prepared = yukimi_config::sekka::prepare(&text)?;
    std::fs::write(&flake_file, &prepared).map_err(|e| e.to_string())?;
    let mut held = BTreeSet::new();
    if let Some(file) = request.packages_file.as_ref().and_then(|f| f.strip_prefix(&request.dir).ok())
        && let Ok(text) = std::fs::read_to_string(scratch.join(file))
    {
        let mut choices = yukimi_config::packages::read(&text).map_err(|e| e.to_string())?;
        held = choices.held.keys().cloned().collect();
        choices.held.clear();
        let text = yukimi_config::packages::render(&choices).map_err(|e| e.to_string())?;
        std::fs::write(scratch.join(file), text).map_err(|e| e.to_string())?;
    }
    // Inputs that follow Sekka's move with it.
    let following = yukimi_config::sekka::following(&prepared);
    let mut inputs: Vec<&str> = request.update.iter().map(String::as_str).filter(|i| !following.contains(i)).collect();
    if !inputs.contains(&"sekka") {
        inputs.push("sekka");
    }
    let scratch_flake = format!("path:{}", scratch.display());
    run(nix::command().args(["flake", "update", "--refresh", "--flake", &scratch_flake]).args(&inputs))?;
    let after = profile_of(&profile_derivation(&scratch_flake, &request.host)?)?;
    let lock_text = std::fs::read_to_string(scratch.join("flake.lock")).map_err(|e| e.to_string())?;
    let rev = yukimi_config::lock::FlakeLock::parse(&lock_text)
        .ok()
        .and_then(|lock| lock.input("sekka"))
        .and_then(|input| input.locked)
        .and_then(|locked| locked.rev)
        .unwrap_or_default();
    // A lock file for the flake as it is; when the update also converts the
    // flake, the helper locks it after converting.
    let lock = if prepared == text {
        let file = request.cache.join("selection.lock");
        std::fs::write(&file, &lock_text).map_err(|e| e.to_string())?;
        Some(file)
    } else {
        None
    };
    Ok(Prepared { key, preview: compare(&before, &after, &held), rev, lock })
}

/// One comparison at a time: one asked for while another is worked out
/// waits for it, and usually finds it is the same.
static WORKING: Mutex<()> = Mutex::new(());

fn cache_file(request: &Request) -> PathBuf {
    request.cache.join("selection.json")
}

/// The comparison for this request if it is worked out already.
pub fn cached(request: &Request) -> Option<Prepared> {
    let key = key(request).ok()?;
    let prepared: Prepared = serde_json::from_slice(&std::fs::read(cache_file(request)).ok()?).ok()?;
    (prepared.key == key && prepared.lock.as_ref().is_none_or(|lock| lock.exists())).then_some(prepared)
}

/// The comparison for this request: the one worked out already, or worked
/// out now (which takes as long as evaluating the system, and longer the
/// first time after an update, when Nix fetches the new sources).
pub fn prepared(request: &Request) -> Result<Prepared, String> {
    let _one_at_a_time = WORKING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = cached(request) {
        return Ok(found);
    }
    std::fs::create_dir_all(&request.cache).map_err(|e| e.to_string())?;
    let fresh = work_out(request, key(request)?)?;
    let json = serde_json::to_vec(&fresh).map_err(|e| e.to_string())?;
    std::fs::write(cache_file(request), json).map_err(|e| e.to_string())?;
    Ok(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(key: &str, version: &str, path: &str) -> Installed {
        Installed { key: key.into(), version: version.into(), paths: vec![format!("/nix/store/{path}")] }
    }

    #[test]
    fn names_split_as_nix_splits_them() {
        assert_eq!(parse_drv_name("firefox-157.0.1"), ("firefox", "157.0.1"));
        assert_eq!(parse_drv_name("gst-plugins-good-1.26"), ("gst-plugins-good", "1.26"));
        assert_eq!(parse_drv_name("python3.13-requests-2.32"), ("python3.13-requests", "2.32"));
        assert_eq!(parse_drv_name("hwdb.bin"), ("hwdb.bin", ""));
        assert_eq!(parse_drv_name("x-"), ("x-", ""));
    }

    #[test]
    fn profiles_list_their_packages() {
        let hash = "a".repeat(32);
        let entry = |paths: &[&str]| serde_json::json!({"paths": paths.iter().map(|p| format!("/nix/store/{hash}-{p}")).collect::<Vec<_>>(), "priority": 5});
        let chosen = serde_json::json!({"derivations": {"x-system-path.drv": {"structuredAttrs": {
            "chosenOutputs": [entry(&["git-2.50", "git-2.50-doc"]), entry(&["coreutils-9.8-bin"]), entry(&[])]
        }}}});
        let packages = profile_packages(&chosen).unwrap();
        assert_eq!(packages.len(), 2);
        assert_eq!((packages[0].key.as_str(), packages[0].version.as_str()), ("git", "2.50"));
        assert_eq!(packages[0].paths.len(), 2);
        assert_eq!((packages[1].key.as_str(), packages[1].version.as_str()), ("coreutils", "9.8"));
        // Older NixOS: `pkgs`, inside the environment's JSON.
        let json = serde_json::json!({"pkgs": [entry(&["htop-3.4"])]}).to_string();
        let older = serde_json::json!({"/nix/store/x-system-path.drv": {"env": {"__json": json}}});
        assert_eq!(profile_packages(&older).unwrap()[0].key, "htop");
        assert!(profile_packages(&serde_json::json!({})).is_err());
    }

    #[test]
    fn changes_are_by_name() {
        let before = [
            p("firefox", "157.0.1", "a-firefox"),
            p("firefox", "157.0.1", "b-firefox"),
            p("codex", "0.162.1", "c-codex"),
            p("htop", "3.4", "d-htop"),
            p("old-tool", "1", "e-old"),
            p("git", "2.50", "f-git"),
        ];
        let after = [
            p("firefox", "157.0.1", "a-firefox"),
            p("firefox", "158.0", "g-firefox"),
            p("codex", "0.163.0", "h-codex"),
            p("htop", "3.4", "d-htop"),
            p("new-tool", "1", "i-new"),
            p("git", "2.50", "j-git"),
        ];
        let preview = compare(&before, &after, &BTreeSet::from(["codex".to_owned(), "nvim".to_owned()]));
        assert_eq!(
            preview.changes,
            vec![
                PackageChange { name: "codex".into(), before: "0.162.1".into(), after: "0.163.0".into(), held: true },
                PackageChange {
                    name: "firefox".into(),
                    before: "157.0.1".into(),
                    after: "157.0.1, 158.0".into(),
                    held: false
                },
                // Rebuilt with the same version.
                PackageChange { name: "git".into(), before: "2.50".into(), after: "2.50".into(), held: false },
            ]
        );
        assert_eq!(preview.added, vec!["new-tool"]);
        assert_eq!(preview.removed, vec!["old-tool"]);
        assert!(preview.held.contains("nvim"));
    }
}

#[cfg(test)]
mod real {
    /// Works out a real configuration's update (evaluating, and fetching
    /// the update's sources), twice, the second time from the cache:
    /// `YUKIMI_PREVIEW_DIR=/etc/nixos YUKIMI_PREVIEW_HOST=<name> cargo test -- --ignored preview_of_a_real_system`.
    #[test]
    #[ignore]
    fn preview_of_a_real_system() {
        let dir = std::path::PathBuf::from(std::env::var("YUKIMI_PREVIEW_DIR").unwrap());
        let request = super::Request {
            flake: format!("path:{}", dir.display()),
            host: std::env::var("YUKIMI_PREVIEW_HOST").unwrap(),
            packages_file: Some(dir.join("yukimi.nix")),
            update: vec!["sekka".to_owned()],
            target: "test".to_owned(),
            cache: std::env::temp_dir().join(format!("yukimi-preview-{}", std::process::id())),
            dir,
        };
        for _ in 0..2 {
            let started = std::time::Instant::now();
            let prepared = super::prepared(&request).unwrap();
            eprintln!("took {:?}; sekka {}, lock {:?}", started.elapsed(), prepared.rev, prepared.lock);
            let preview = &prepared.preview;
            let newer = preview.changes.iter().filter(|c| c.before != c.after).count();
            eprintln!("  {} changes ({newer} new versions), added {:?}", preview.changes.len(), preview.added);
        }
        let _ = std::fs::remove_dir_all(&request.cache);
    }
}
