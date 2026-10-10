// SPDX-License-Identifier: MIT OR Apache-2.0
//! Garbage-collector roots: the links that keep store paths alive.
//!
//! Found the way Nix finds them (`LocalStore::findRoots`): every symlink
//! below `/nix/var/nix/gcroots` and `/nix/var/nix/profiles` that points into
//! the store is a root, and one that points at another symlink (the
//! indirect roots in `gcroots/auto`, such as a `result` link in a project)
//! is a root through that link. Each root is then sorted into a kind a
//! person recognizes: a system generation, a profile, a build result, a
//! development shell.
use std::path::{Path, PathBuf};

use crate::STORE_DIR;
use crate::path::StorePath;

/// The directories Nix searches for roots.
pub const ROOT_DIRS: [&str; 2] = ["/nix/var/nix/gcroots", "/nix/var/nix/profiles"];

/// One root: a link, and the store path it keeps alive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// The link a person would remove to let the path go: a profile
    /// generation link, or the `result` link in a project.
    pub link: PathBuf,
    pub target: StorePath,
    pub kind: RootKind,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootKind {
    /// `/run/current-system`: the system running now.
    CurrentSystem,
    /// `/run/booted-system`: the system the machine started with.
    BootedSystem,
    /// A system generation, `/nix/var/nix/profiles/system-<n>-link`.
    SystemGeneration(u32),
    /// A generation of another profile, such as a user's packages.
    Profile {
        name: String,
        user: Option<String>,
        generation: Option<u32>,
    },
    /// A `result` link left by a build.
    BuildResult,
    /// A development environment kept by direnv or `nix develop`.
    DevShell,
    Other,
}

/// All roots on this system that the current user can see.
pub fn scan() -> Vec<Root> {
    scan_dirs(&ROOT_DIRS.map(Path::new))
}

/// Roots below the given directories.
pub fn scan_dirs(dirs: &[&Path]) -> Vec<Root> {
    let mut roots = Vec::new();
    for dir in dirs {
        walk(dir, &mut roots);
    }
    roots.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.link.cmp(&b.link)));
    roots.dedup_by(|a, b| a.link == b.link);
    roots
}

fn walk(path: &Path, roots: &mut Vec<Root>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            walk(&entry.path(), roots);
        }
    } else if meta.file_type().is_symlink() {
        let Ok(target) = std::fs::read_link(path) else {
            return;
        };
        if let Some(store_path) = in_store(&target) {
            found(path, store_path, roots);
            return;
        }
        // An indirect root: a link to a link into the store.
        let target = if target.is_absolute() { target } else { path.parent().unwrap_or(Path::new("/")).join(target) };
        let is_link = std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink());
        if is_link
            && let Ok(second) = std::fs::read_link(&target)
            && let Some(store_path) = in_store(&second)
        {
            found(&target, store_path, roots);
        }
    }
}

fn in_store(target: &Path) -> Option<StorePath> {
    let text = target.to_str()?;
    text.starts_with(STORE_DIR).then(|| StorePath::parse(text)).flatten()
}

fn found(link: &Path, target: StorePath, roots: &mut Vec<Root>) {
    roots.push(Root { kind: classify(link), link: link.to_path_buf(), target });
}

/// What kind of root a link is, from where it lives.
pub fn classify(link: &Path) -> RootKind {
    let text = link.to_string_lossy();
    let base = link.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if text == "/run/current-system" {
        return RootKind::CurrentSystem;
    }
    if text == "/run/booted-system" {
        return RootKind::BootedSystem;
    }
    if text.contains("/.direnv/") || base.starts_with("flake-profile") || base.ends_with("-dev-shell") {
        return RootKind::DevShell;
    }
    if text.contains("/profiles/") || text.contains("/profiles-") {
        let (name, generation) = match base.strip_suffix("-link").and_then(|rest| rest.rsplit_once('-')) {
            Some((name, number)) => match number.parse() {
                Ok(n) => (name.to_owned(), Some(n)),
                Err(_) => (base.clone(), None),
            },
            None => (base.clone(), None),
        };
        if text.starts_with("/nix/var/nix/profiles/") && !text.contains("/per-user/") && name == "system" {
            return match generation {
                Some(n) => RootKind::SystemGeneration(n),
                None => RootKind::Profile { name, user: None, generation },
            };
        }
        return RootKind::Profile { name, user: owner(&text), generation };
    }
    if base.starts_with("result") {
        return RootKind::BuildResult;
    }
    RootKind::Other
}

/// The user a path belongs to, from `/home/<user>` or a per-user profile
/// directory.
fn owner(text: &str) -> Option<String> {
    let rest = text.strip_prefix("/home/").or_else(|| text.split_once("/per-user/").map(|(_, rest)| rest))?;
    rest.split('/').next().filter(|user| !user.is_empty()).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const H: &str = "0123456789abcdfghijklmnpqrsvwxyz";

    #[test]
    fn kinds_from_where_links_live() {
        assert_eq!(classify(Path::new("/run/current-system")), RootKind::CurrentSystem);
        assert_eq!(classify(Path::new("/nix/var/nix/profiles/system-42-link")), RootKind::SystemGeneration(42));
        assert_eq!(
            classify(Path::new("/home/ref/.local/state/nix/profiles/profile-3-link")),
            RootKind::Profile { name: "profile".into(), user: Some("ref".into()), generation: Some(3) }
        );
        assert_eq!(classify(Path::new("/home/ref/src/app/result")), RootKind::BuildResult);
        assert_eq!(classify(Path::new("/home/ref/src/app/result-man")), RootKind::BuildResult);
        assert_eq!(classify(Path::new("/home/ref/src/app/.direnv/flake-profile-a5d5b61a")), RootKind::DevShell);
        assert_eq!(classify(Path::new("/nix/var/nix/gcroots/booted-system")), RootKind::Other);
    }

    #[test]
    fn direct_and_indirect_roots() {
        let dir = std::env::temp_dir().join(format!("yukimi-roots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (gcroots, auto, home) = (dir.join("gcroots"), dir.join("gcroots/auto"), dir.join("home/ref/app"));
        for d in [&auto, &home] {
            std::fs::create_dir_all(d).unwrap();
        }
        let firefox = format!("/nix/store/{H}-firefox-142.0");
        symlink(&firefox, gcroots.join("firefox")).unwrap();
        // auto/<x> -> home/ref/app/result -> store
        let result = home.join("result");
        symlink(format!("/nix/store/{H}-app-1.0"), &result).unwrap();
        symlink(&result, auto.join("abc")).unwrap();
        // A stale indirect root and a link outside the store are ignored.
        symlink(dir.join("missing"), auto.join("stale")).unwrap();
        symlink("/etc/hostname", gcroots.join("elsewhere")).unwrap();
        let roots = scan_dirs(&[&gcroots]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(roots.len(), 2, "{roots:?}");
        let app = roots.iter().find(|r| r.target.name() == "app").unwrap();
        assert_eq!(app.link, result);
        assert_eq!(app.kind, RootKind::BuildResult);
        assert!(roots.iter().any(|r| r.target.as_str() == firefox));
    }
}
