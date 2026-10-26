//! Frecency: how often and how recently something was picked, so the
//! palette learns that "term" means foot and "set" means Settings.
//!
//! Each pick adds a visit; a visit's weight halves every [`HALF_LIFE`], so
//! last week's habit fades in a month. The store is a small JSON file in the
//! state directory (`$XDG_STATE_HOME/aizome/frecency.json`), capped at
//! [`CAPACITY`] entries, the faintest dropped first.
//!
//! The file is the history, not a copy of what a process holds: picks are
//! recorded into it ([`Frecency::record`]: read, add, write), and a
//! process holding a store reads it again when the file changed under it
//! ([`Frecency::is_stale`]), so forgetting it (Settings removes the file)
//! sticks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How long a visit takes to count half as much.
pub const HALF_LIFE: f64 = 7.0 * 24.0 * 3600.0;
/// Most entries kept.
pub const CAPACITY: usize = 2000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Entry {
    /// The decayed score at `at`.
    score: f64,
    /// Seconds since the epoch of the last update.
    at: f64,
}

/// Picks by key: `"app:org.aizome.Settei"`, `"file:/home/…/notes.md"`,
/// `"cmd:org.aizome.Sumi:Edit/Find/Find and Replace…"`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Frecency {
    entries: HashMap<String, Entry>,
    #[serde(skip)]
    path: Option<PathBuf>,
    /// The file as it was when last read or written: none if it wasn't
    /// there.
    #[serde(skip)]
    seen: Option<Stamp>,
}

/// What tells a file's versions apart: its modification time and length.
type Stamp = (SystemTime, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn decayed(entry: &Entry, at: f64) -> f64 {
    entry.score * 0.5f64.powf(((at - entry.at).max(0.0)) / HALF_LIFE)
}

impl Frecency {
    /// The default store's path.
    pub fn default_path() -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        state.join("aizome/frecency.json")
    }

    /// Loads the store, or starts an empty one if it's missing or unreadable.
    pub fn load(path: impl AsRef<Path>) -> Frecency {
        let path = path.as_ref();
        let seen = stamp(path);
        let mut store: Frecency =
            std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        store.path = Some(path.to_owned());
        store.seen = seen;
        store
    }

    /// Writes the store back where it was loaded from, atomically.
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, &path)?;
        self.seen = stamp(&path);
        Ok(())
    }

    /// Whether the file changed (or went) since this store read or wrote
    /// it: someone else recorded a pick, or forgot the history.
    pub fn is_stale(&self) -> bool {
        self.path.as_deref().is_some_and(|p| stamp(p) != self.seen)
    }

    /// Records picks of `keys` now into the file at `path` (read, add,
    /// write), and returns the store as written: what the file held, not
    /// what a caller remembered, so a history forgotten meanwhile stays
    /// forgotten.
    pub fn record(path: impl AsRef<Path>, keys: &[String]) -> std::io::Result<Frecency> {
        let mut store = Frecency::load(path);
        let at = now();
        for key in keys {
            store.visit_at(key, at);
        }
        store.save()?;
        Ok(store)
    }

    /// Whether it remembers anything.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records a pick of `key` now.
    pub fn visit(&mut self, key: &str) {
        self.visit_at(key, now());
    }

    pub fn visit_at(&mut self, key: &str, at: f64) {
        let entry = self.entries.entry(key.to_owned()).or_default();
        entry.score = decayed(entry, at) + 1.0;
        entry.at = at;
        if self.entries.len() > CAPACITY {
            let mut scores: Vec<(String, f64)> =
                self.entries.iter().map(|(k, e)| (k.clone(), decayed(e, at))).collect();
            scores.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (k, _) in scores.into_iter().take(self.entries.len() - CAPACITY) {
                self.entries.remove(&k);
            }
        }
    }

    /// The key's current score: about the number of picks in the last
    /// half-life, 0.0 if never picked.
    pub fn score(&self, key: &str) -> f64 {
        self.score_at(key, now())
    }

    pub fn score_at(&self, key: &str, at: f64) -> f64 {
        self.entries.get(key).map_or(0.0, |e| decayed(e, at))
    }

    /// A boost to add to a match score. Matcher scores run to a few hundred
    /// for a good match, so a habit worth a few picks moves an item a few
    /// places but never lifts a poor match over a good one.
    pub fn boost(&self, key: &str) -> i64 {
        (self.score(key).ln_1p() * 24.0).round() as i64
    }

    pub fn forget(&mut self, key: &str) {
        self.entries.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_decay_by_half_life() {
        let mut f = Frecency::default();
        f.visit_at("app:a", 0.0);
        f.visit_at("app:a", 0.0);
        assert!((f.score_at("app:a", 0.0) - 2.0).abs() < 1e-9);
        assert!((f.score_at("app:a", HALF_LIFE) - 1.0).abs() < 1e-9);
        assert_eq!(f.score_at("app:b", 0.0), 0.0);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("aizome-frecency-{}", std::process::id()));
        let path = dir.join("f.json");
        let mut f = Frecency::load(&path);
        f.visit("file:/x");
        f.save().unwrap();
        let g = Frecency::load(&path);
        assert!(g.score("file:/x") > 0.9);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_forgotten_history_stays_forgotten() {
        let dir = std::env::temp_dir().join(format!("aizome-frecency-forget-{}", std::process::id()));
        let path = dir.join("frecency.json");
        let _ = std::fs::remove_dir_all(&dir);
        // A process holding the store (the shell) records a pick.
        let mut held = Frecency::record(&path, &["app:org.aizome.Sumi".into()]).unwrap();
        assert!(!held.is_stale());
        assert!(held.score("app:org.aizome.Sumi") > 0.9);
        // Settings forgets the history.
        std::fs::remove_file(&path).unwrap();
        assert!(held.is_stale(), "the file went");
        // The next pick doesn't bring the old ones back.
        held = Frecency::record(&path, &["app:org.aizome.Tokei".into()]).unwrap();
        assert_eq!(held.score("app:org.aizome.Sumi"), 0.0);
        let on_disk = Frecency::load(&path);
        assert_eq!(on_disk.score("app:org.aizome.Sumi"), 0.0);
        assert!(on_disk.score("app:org.aizome.Tokei") > 0.9);
        // Another writer's pick makes a held store stale too.
        let other = Frecency::record(&path, &["file:/x".into(), "file:/y".into()]).unwrap();
        assert!(!other.is_stale());
        assert!(held.is_stale());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn capacity_drops_the_faintest() {
        let mut f = Frecency::default();
        for i in 0..CAPACITY {
            f.visit_at(&format!("k{i}"), i as f64);
        }
        f.visit_at("new", CAPACITY as f64);
        assert_eq!(f.entries.len(), CAPACITY);
        assert!(!f.entries.contains_key("k0"));
        assert!(f.entries.contains_key("new"));
    }
}
