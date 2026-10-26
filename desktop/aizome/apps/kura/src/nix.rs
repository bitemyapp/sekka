//! Nix: the store's size, the generations kept, and the way to Yukimi.
//!
//! Kura never offers to delete anything in `/nix/store`: store paths go
//! when nothing roots them and the garbage collector runs, which is
//! Yukimi's Storage view's job. Kura shows how large the store is, how
//! many old generations hold it, and opens Yukimi.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

pub const STORE: &str = "/nix/store";

/// The command that opens Yukimi's Storage view.
pub const YUKIMI_STORAGE: [&str; 3] = ["aizome-yukimi", "--page", "storage"];

/// The command line for people and agents: `aizome-yukimi --page storage`.
pub fn yukimi_command_line() -> String {
    YUKIMI_STORAGE.join(" ")
}

/// Whether there's a Nix store here.
pub fn has_store() -> bool {
    Path::new(STORE).is_dir()
}

/// Whether `path` is in the store (or is it): never offered for deletion.
pub fn in_store(path: &Path) -> bool {
    path.starts_with(STORE)
}

/// What Kura knows of the store's size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoreSize {
    /// The sum of every valid path's NAR size: the bytes of the files, before
    /// any deduplication by hard links (`auto-optimise-store`).
    pub bytes: u64,
    pub paths: u64,
    /// When it was measured, as Unix time.
    pub measured_at: i64,
    /// What it was measured with.
    pub source: String,
    /// The store database's modification time then (a cache key).
    #[serde(default)]
    pub db_mtime: i64,
}

#[derive(Deserialize)]
struct PathInfo {
    #[serde(rename = "narSize")]
    nar_size: Option<u64>,
}

#[derive(Deserialize)]
struct ListedInfo {
    #[serde(rename = "narSize")]
    nar_size: Option<u64>,
}

/// The modification time of Nix's database: it changes when paths come or go.
fn db_mtime() -> i64 {
    use std::os::unix::fs::MetadataExt;
    ["/nix/var/nix/db/db.sqlite-wal", "/nix/var/nix/db/db.sqlite"]
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.mtime())
        .max()
        .unwrap_or(0)
}

/// Sum NAR sizes from `nix path-info --all --json`'s output (an object keyed
/// by path in Nix 2.19 and later, a list of objects before).
pub fn parse_path_info(json: &str) -> Option<(u64, u64)> {
    let start = json.trim_start();
    if start.starts_with('{') {
        let m: HashMap<String, Option<PathInfo>> = serde_json::from_str(start).ok()?;
        let bytes = m.values().flatten().filter_map(|i| i.nar_size).sum();
        Some((bytes, m.len() as u64))
    } else {
        let v: Vec<ListedInfo> = serde_json::from_str(start).ok()?;
        let bytes = v.iter().filter_map(|i| i.nar_size).sum();
        Some((bytes, v.len() as u64))
    }
}

/// Measure the store through the Nix daemon (a few seconds), or take the
/// last measurement from `cache` if the database hasn't changed since.
pub fn store_size(cache: Option<&Path>) -> Option<StoreSize> {
    if !has_store() {
        return None;
    }
    let mtime = db_mtime();
    if let Some(c) = cache
        && let Ok(text) = std::fs::read_to_string(c)
        && let Ok(s) = serde_json::from_str::<StoreSize>(&text)
        && s.db_mtime == mtime
        && mtime != 0
    {
        return Some(s);
    }
    let out = Command::new("nix")
        .args(["--extra-experimental-features", "nix-command", "path-info", "--all", "--json"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (bytes, paths) = parse_path_info(&text)?;
    let s = StoreSize {
        bytes,
        paths,
        measured_at: crate::time::now_unix(),
        source: "nix path-info --all --json (sum of narSize)".into(),
        db_mtime: mtime,
    };
    if let Some(c) = cache {
        let _ = crate::write_atomic(c, serde_json::to_string_pretty(&s).unwrap_or_default().as_bytes());
    }
    Some(s)
}

/// One profile's generations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// `system`, `home-manager`, `profile` (the user's `nix profile`).
    pub name: String,
    pub path: PathBuf,
    pub generations: u32,
    /// The current one's number, if the profile link says.
    pub current: Option<u32>,
}

impl Profile {
    /// Generations besides the current one.
    pub fn old(&self) -> u32 {
        self.generations.saturating_sub(1)
    }
}

/// `name-42-link` → 42.
fn generation_number(file: &str, name: &str) -> Option<u32> {
    file.strip_prefix(name)?.strip_prefix('-')?.strip_suffix("-link")?.parse().ok()
}

/// The generations of the profile at `profile` (a symlink like
/// `/nix/var/nix/profiles/system`).
pub fn profile(profile: &Path) -> Option<Profile> {
    let dir = profile.parent()?;
    let name = profile.file_name()?.to_str()?;
    let mut count = 0;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if generation_number(&e.file_name().to_string_lossy(), name).is_some() {
            count += 1;
        }
    }
    if count == 0 {
        return None;
    }
    let current = std::fs::read_link(profile)
        .ok()
        .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
        .and_then(|f| generation_number(&f, name));
    Some(Profile { name: name.to_owned(), path: profile.to_path_buf(), generations: count, current })
}

/// The profiles with generations: the system's, and the user's own.
pub fn profiles() -> Vec<Profile> {
    let mut candidates = vec![PathBuf::from("/nix/var/nix/profiles/system")];
    let user = std::env::var("USER").unwrap_or_default();
    let state = crate::xdg_dir("XDG_STATE_HOME", ".local/state");
    for name in ["profile", "home-manager"] {
        if let Some(s) = &state {
            candidates.push(s.join("nix/profiles").join(name));
        }
        if !user.is_empty() {
            candidates.push(PathBuf::from("/nix/var/nix/profiles/per-user").join(&user).join(name));
        }
    }
    let mut seen = std::collections::HashSet::new();
    candidates
        .iter()
        .filter_map(|p| profile(p))
        .filter(|p| {
            seen.insert(std::fs::canonicalize(p.path.parent().unwrap_or(&p.path)).unwrap_or_default().join(&p.name))
        })
        .collect()
}

/// Whether a symlink points into the store (a `result` link, a GC root).
pub fn points_into_store(link: &Path) -> bool {
    std::fs::read_link(link).is_ok_and(|t| in_store(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_both_shapes() {
        let new =
            r#"{"/nix/store/a-x":{"narSize":100,"references":[]},"/nix/store/b-y":{"narSize":23},"/nix/store/c":null}"#;
        assert_eq!(parse_path_info(new), Some((123, 3)));
        let old = r#"[{"path":"/nix/store/a","narSize":5},{"path":"/nix/store/b","narSize":6}]"#;
        assert_eq!(parse_path_info(old), Some((11, 2)));
        assert_eq!(parse_path_info("nope"), None);
    }

    #[test]
    fn counts_generations() {
        let dir = crate::testdir("nix-profiles");
        for n in [1, 2, 5] {
            std::os::unix::fs::symlink("/nix/store/x", dir.join(format!("system-{n}-link"))).unwrap();
        }
        std::os::unix::fs::symlink("system-5-link", dir.join("system")).unwrap();
        let p = profile(&dir.join("system")).unwrap();
        assert_eq!((p.generations, p.current, p.old()), (3, Some(5), 2));
        assert!(in_store(Path::new("/nix/store/abc-foo/bin")));
        assert!(!in_store(Path::new("/nix/var")));
    }
}
