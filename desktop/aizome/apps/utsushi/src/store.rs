//! What Utsushi remembers between runs, in `$XDG_STATE_HOME/aizome/utsushi/`:
//! where each film was left (`positions.json`, so it opens there again),
//! what was played lately (`recent.json`), and the volume, mute and which
//! time the timeline shows (`state.json`). Written as they change,
//! atomically.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Items remembered in Open Recent.
const RECENT: usize = 20;
/// Positions remembered.
const POSITIONS: usize = 400;

/// Where a film was left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Left {
    pub position: f64,
    pub duration: f64,
    /// Seconds since the epoch, to forget the oldest.
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub path: PathBuf,
    pub title: String,
    pub duration: Option<f64>,
    pub video: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub volume: f32,
    pub muted: bool,
    /// The time shown at the timeline's right is what's left (else the
    /// whole length).
    pub remaining: bool,
    /// PipeWire node.name, not its temporary numeric registry ID.
    pub audio_device: Option<String>,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { volume: 1.0, muted: false, remaining: true, audio_device: None }
    }
}

#[derive(Default)]
pub struct Store {
    dir: Option<PathBuf>,
    pub positions: BTreeMap<String, Left>,
    pub recent: Vec<Recent>,
    pub prefs: Prefs,
}

fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("aizome/utsushi"))
}

fn read<T: for<'de> Deserialize<'de> + Default>(dir: &Path, name: &str) -> T {
    std::fs::read(dir.join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write<T: Serialize>(dir: &Path, name: &str, value: &T) {
    let Ok(bytes) = serde_json::to_vec_pretty(value) else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!(".{name}.tmp"));
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, dir.join(name)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The key a file's position is kept under.
fn key(path: &Path) -> String {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

impl Store {
    /// What's remembered (nothing where there's no state directory, or
    /// with `dir` none: tests).
    pub fn load(dir: Option<PathBuf>) -> Store {
        let dir = dir.or_else(state_dir);
        let Some(d) = &dir else { return Store::default() };
        Store {
            positions: read(d, "positions.json"),
            recent: read(d, "recent.json"),
            prefs: read(d, "state.json"),
            dir,
        }
    }

    /// Where to start `path`: where it was left, unless that was near its
    /// start or its end (or it's short).
    pub fn resume_at(&self, path: &Path) -> Option<f64> {
        let left = self.positions.get(&key(path))?;
        let long = left.duration >= 5.0 * 60.0;
        let useful = left.position >= 30.0 && left.position < left.duration - 30.0_f64.max(left.duration * 0.05);
        (long && useful).then_some(left.position)
    }

    /// Remember where `path` was left; a film watched to its end (or barely
    /// begun) is forgotten.
    pub fn leave(&mut self, path: &Path, position: f64, duration: f64) {
        let k = key(path);
        if duration < 5.0 * 60.0 || position < 30.0 || position >= duration - 30.0_f64.max(duration * 0.05) {
            if self.positions.remove(&k).is_none() {
                return;
            }
        } else {
            self.positions.insert(k, Left { position, duration, at: now() });
            if self.positions.len() > POSITIONS
                && let Some(oldest) = self.positions.iter().min_by_key(|(_, l)| l.at).map(|(k, _)| k.clone())
            {
                self.positions.remove(&oldest);
            }
        }
        if let Some(d) = &self.dir {
            write(d, "positions.json", &self.positions);
        }
    }

    /// Put `item` first in Open Recent.
    pub fn played(&mut self, item: Recent) {
        self.recent.retain(|r| r.path != item.path);
        self.recent.insert(0, item);
        self.recent.truncate(RECENT);
        if let Some(d) = &self.dir {
            write(d, "recent.json", &self.recent);
        }
    }

    pub fn clear_recent(&mut self) {
        self.recent.clear();
        if let Some(d) = &self.dir {
            write(d, "recent.json", &self.recent);
        }
    }

    pub fn save_prefs(&self) {
        if let Some(d) = &self.dir {
            write(d, "state.json", &self.prefs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("utsushi-store-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn films_resume_where_they_were_left_but_not_at_either_end() {
        let dir = temp();
        let film = dir.join("film.mkv");
        let mut s = Store::load(Some(dir.clone()));
        s.leave(&film, 1800.0, 6000.0);
        assert_eq!(Store::load(Some(dir.clone())).resume_at(&film), Some(1800.0));
        // Watched to the end: forgotten.
        s.leave(&film, 5990.0, 6000.0);
        assert_eq!(Store::load(Some(dir.clone())).resume_at(&film), None);
        // Barely begun, or a short clip: not kept.
        s.leave(&film, 12.0, 6000.0);
        let clip = dir.join("clip.mp4");
        s.leave(&clip, 60.0, 120.0);
        let s = Store::load(Some(dir.clone()));
        assert_eq!((s.resume_at(&film), s.resume_at(&clip)), (None, None));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_items_come_first_once() {
        let dir = temp();
        let mut s = Store::load(Some(dir.clone()));
        let item = |p: &str| Recent { path: PathBuf::from(p), title: p.into(), duration: None, video: true };
        s.played(item("/a"));
        s.played(item("/b"));
        s.played(item("/a"));
        let s = Store::load(Some(dir.clone()));
        let paths: Vec<_> = s.recent.iter().map(|r| r.path.to_string_lossy().into_owned()).collect();
        assert_eq!(paths, ["/a", "/b"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn output_preferences_are_compatible_and_survive_reopening() {
        let old: Prefs = serde_json::from_str(r#"{"volume":0.5,"muted":true,"remaining":false}"#).unwrap();
        assert_eq!(old.audio_device, None);
        let dir = temp();
        let mut store = Store::load(Some(dir.clone()));
        store.prefs.audio_device = Some("alsa_output.usb-speakers".into());
        store.save_prefs();
        assert_eq!(Store::load(Some(dir.clone())).prefs.audio_device, store.prefs.audio_device);
        store.prefs.audio_device = None;
        store.save_prefs();
        assert_eq!(Store::load(Some(dir.clone())).prefs.audio_device, None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
