// SPDX-License-Identifier: MIT OR Apache-2.0
//! Choosing what an update changes, package by package.
//!
//! An update moves the system's sources together; what it changes for the
//! person is the packages the system installs. Yukimi compares the system's
//! packages as they are with those of the updated configuration, both
//! evaluated (nothing is built), so the person can leave some out. A
//! package left out is kept at the version the system has: Yukimi's module
//! takes it from the saved configuration the system was built with
//! (`programs.yukimi.held`), the very same build.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Stdio;

use serde::Deserialize;

use crate::nix;

/// One of the system's packages, as its configuration says.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Installed {
    /// Its name without its version, which a hold is by.
    pub key: String,
    pub version: String,
    /// Where it is (or would be) in the store.
    pub path: String,
}

/// A package the update changes, which can be kept as it is instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageChange {
    pub name: String,
    /// Its version now, and after the update (a build can change with the
    /// same version, when what it is built with changed).
    pub before: String,
    pub after: String,
    /// Kept at the version the system has by an earlier choice.
    pub held: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub changes: Vec<PackageChange>,
    /// Packages the update adds or takes away, which come with it.
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Every package kept as it is now, whether the update changes it or not.
    pub held: BTreeSet<String>,
}

/// The fields of each package, for `nix eval --apply`.
const FIELDS: &str = "ps: map (p: let d = builtins.parseDrvName p.name; in \
                      { key = p.pname or d.name; version = p.version or d.version; path = p.outPath; }) ps";

/// The packages a flake's configuration installs for everyone.
pub fn installed(flake: &str, host: &str) -> Result<Vec<Installed>, String> {
    let out = nix::command()
        .args(["eval", "--json", "--apply", FIELDS])
        .arg(format!("{flake}#nixosConfigurations.\"{host}\".config.environment.systemPackages"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Nix couldn't be started: {e}"))?;
    if !out.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&out.stderr));
        return Err(crate::updates::first_line(&nix::last_error(&stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

/// What changes between two lists of packages, by name.
pub fn compare(before: &[Installed], after: &[Installed], held: &BTreeSet<String>) -> Preview {
    let group = |list: &[Installed]| {
        let mut by_name: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
        for p in list {
            let entry = by_name.entry(p.key.clone()).or_default();
            entry.0.insert(p.path.clone());
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

/// Copy a configuration directory as the helper keeps one: not hidden
/// files, build results or files over a megabyte.
fn copy_configuration(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') || text.starts_with("result") {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_configuration(&entry.path(), &to.join(&name))?;
        } else if kind.is_file() && entry.metadata()?.len() <= 1 << 20 {
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// What updating a Sekka system's `update` inputs changes, package by
/// package: its configuration in `dir` (`flake`, as Nix is given it) as it
/// is, against a copy in `scratch` updated as the helper would, without any
/// holds. `packages_file` is Yukimi's own file, which holds them.
pub fn preview(
    dir: &Path,
    flake: &str,
    host: &str,
    packages_file: Option<&Path>,
    update: &[String],
    scratch: &Path,
) -> Result<Preview, String> {
    let before = installed(flake, host)?;
    let _ = std::fs::remove_dir_all(scratch);
    copy_configuration(dir, scratch).map_err(|e| format!("Copying the configuration: {e}"))?;
    let flake_file = scratch.join("flake.nix");
    let text = std::fs::read_to_string(&flake_file).map_err(|e| format!("{}: {e}", flake_file.display()))?;
    let prepared = yukimi_config::sekka::prepare(&text)?;
    std::fs::write(&flake_file, &prepared).map_err(|e| e.to_string())?;
    let mut held = BTreeSet::new();
    if let Some(file) = packages_file.and_then(|f| f.strip_prefix(dir).ok()).map(|f| scratch.join(f))
        && let Ok(text) = std::fs::read_to_string(&file)
    {
        let mut choices = yukimi_config::packages::read(&text).map_err(|e| e.to_string())?;
        held = choices.held.keys().cloned().collect();
        choices.held.clear();
        let text = yukimi_config::packages::render(&choices).map_err(|e| e.to_string())?;
        std::fs::write(&file, text).map_err(|e| e.to_string())?;
    }
    // Inputs that follow Sekka's move with it.
    let following = yukimi_config::sekka::following(&prepared);
    let mut inputs: Vec<&String> = update.iter().filter(|i| !following.contains(&i.as_str())).collect();
    let sekka = "sekka".to_owned();
    if !inputs.contains(&&sekka) {
        inputs.push(&sekka);
    }
    let scratch_flake = format!("path:{}", scratch.display());
    let out = nix::command()
        .args(["flake", "update", "--refresh", "--flake", &scratch_flake])
        .args(inputs)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Nix couldn't be started: {e}"))?;
    if !out.status.success() {
        let stderr = crate::log::strip_ansi(&String::from_utf8_lossy(&out.stderr));
        return Err(crate::updates::first_line(&nix::last_error(&stderr)));
    }
    let after = installed(&scratch_flake, host)?;
    Ok(compare(&before, &after, &held))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(key: &str, version: &str, path: &str) -> Installed {
        Installed { key: key.into(), version: version.into(), path: format!("/nix/store/{path}") }
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
    /// Compares a real configuration with its update (evaluating, and
    /// fetching the update's sources):
    /// `YUKIMI_PREVIEW_DIR=/etc/nixos YUKIMI_PREVIEW_HOST=<name> cargo test -- --ignored preview_of_a_real_system`.
    #[test]
    #[ignore]
    fn preview_of_a_real_system() {
        let dir = std::path::PathBuf::from(std::env::var("YUKIMI_PREVIEW_DIR").unwrap());
        let host = std::env::var("YUKIMI_PREVIEW_HOST").unwrap();
        let scratch = std::env::temp_dir().join(format!("yukimi-preview-{}", std::process::id()));
        let flake = format!("path:{}", dir.display());
        let started = std::time::Instant::now();
        let preview =
            super::preview(&dir, &flake, &host, Some(&dir.join("yukimi.nix")), &["sekka".to_owned()], &scratch)
                .unwrap();
        let _ = std::fs::remove_dir_all(&scratch);
        eprintln!("took {:?}", started.elapsed());
        for change in &preview.changes {
            eprintln!(
                "  {}: {} -> {}{}",
                change.name,
                change.before,
                change.after,
                if change.held { " (kept)" } else { "" }
            );
        }
        eprintln!("added {:?}, removed {:?}, held {:?}", preview.added, preview.removed, preview.held);
    }
}
