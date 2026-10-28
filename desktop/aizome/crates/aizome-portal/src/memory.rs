//! What choosers remember between requests, in
//! `$XDG_STATE_HOME/aizome/portal/file-chooser.json`: the folder each app
//! last chose in (apps outside a sandbox mostly share the empty id, so
//! theirs is simply the last folder), whether hidden files show, and the
//! order.

use std::path::{Path, PathBuf};

use hako::model::entry::Sort;
use serde::{Deserialize, Serialize};

use crate::request::{bytes_of_path, path_of_bytes};

/// How many apps' folders are kept, most recent first.
const APPS: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    /// (app id, folder as bytes), most recent first.
    pub folders: Vec<(String, Vec<u8>)>,
    pub hidden: bool,
    pub sort: Sort,
}

impl Memory {
    /// The default file.
    pub fn path() -> PathBuf {
        hako::xdg_dir("XDG_STATE_HOME", ".local/state").join("aizome/portal/file-chooser.json")
    }

    /// What `path` holds; nothing remembered when it's missing or unreadable.
    pub fn load(path: &Path) -> Memory {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Write it to `path`, whole or not at all.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    /// The folder `app_id` last chose in, if it's still there.
    pub fn folder(&self, app_id: &str) -> Option<PathBuf> {
        self.folders.iter().find(|(id, _)| id == app_id).map(|(_, f)| path_of_bytes(f)).filter(|p| p.is_dir())
    }

    /// Remember `folder` as where `app_id` last chose.
    pub fn chose_in(&mut self, app_id: &str, folder: &Path) {
        self.folders.retain(|(id, _)| id != app_id);
        self.folders.insert(0, (app_id.to_owned(), bytes_of_path(folder)));
        self.folders.truncate(APPS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_folders_by_app() {
        let dir = std::env::temp_dir().join(format!("aizome-portal-memory-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        let file = dir.join("state/file-chooser.json");
        let mut m = Memory::load(&file);
        assert_eq!(m, Memory::default());
        m.chose_in("org.example.A", &dir.join("a"));
        m.chose_in("", &dir);
        m.chose_in("org.example.A", &dir);
        m.save(&file).unwrap();
        let m = Memory::load(&file);
        assert_eq!(m.folders.len(), 2);
        assert_eq!(m.folder("org.example.A"), Some(dir.clone()));
        assert_eq!(m.folder("org.example.B"), None);
        std::fs::remove_dir_all(&dir).unwrap();
        // Gone folders aren't offered.
        assert_eq!(m.folder(""), None);
    }

    #[test]
    fn keeps_a_bounded_list() {
        let mut m = Memory::default();
        for i in 0..100 {
            m.chose_in(&format!("app{i}"), Path::new("/"));
        }
        assert_eq!(m.folders.len(), APPS);
        assert_eq!(m.folders[0].0, "app99");
    }
}
