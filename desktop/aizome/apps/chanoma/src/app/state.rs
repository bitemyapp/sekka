//! What Chanoma remembers between runs besides the grown-ups' settings:
//! what was played lately and where films were left (`recent.json`), and
//! controllers' buttons as set up here (`controllers.json`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pad::mapping::Mapping;
use crate::store::Dirs;

/// Items "Keep playing" holds at most.
pub const RECENT: usize = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Item {
    Game {
        key: String,
    },
    Service {
        id: String,
    },
    /// A site a grown-up added, by its address.
    Site {
        url: String,
    },
    Video {
        path: PathBuf,
        position: f64,
        duration: f64,
    },
}

impl Item {
    fn same(&self, other: &Item) -> bool {
        match (self, other) {
            (Item::Game { key: a }, Item::Game { key: b }) => a == b,
            (Item::Service { id: a }, Item::Service { id: b }) => a == b,
            (Item::Site { url: a }, Item::Site { url: b }) => a == b,
            (Item::Video { path: a, .. }, Item::Video { path: b, .. }) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recent {
    pub items: Vec<Item>,
    /// Where each video was left (seconds, and its length), by path:
    /// more than "Keep playing" shows.
    pub positions: HashMap<PathBuf, (f64, f64)>,
}

impl Recent {
    pub fn load(dirs: &Dirs) -> Recent {
        crate::store::read(&dirs.state.join("recent.json"))
    }

    pub fn save(&self, dirs: &Dirs) {
        if let Err(e) = crate::store::write(&dirs.state.join("recent.json"), self) {
            log::warn!("chanoma: keeping what was played: {e}");
        }
    }

    /// Put `item` first.
    pub fn touch(&mut self, item: Item) {
        self.items.retain(|i| !i.same(&item));
        self.items.insert(0, item);
        self.items.truncate(RECENT);
    }

    pub fn forget(&mut self, item: &Item) {
        self.items.retain(|i| !i.same(item));
    }

    /// Where a video was left: none if it was finished or barely begun.
    pub fn resume_at(&self, path: &Path) -> Option<f64> {
        let (pos, dur) = *self.positions.get(path)?;
        (pos > 15.0 && (dur <= 0.0 || pos < dur - 30.0)).then_some(pos)
    }

    pub fn set_position(&mut self, path: &Path, position: f64, duration: f64) {
        self.positions.insert(path.to_owned(), (position, duration));
        if self.positions.len() > 2000 {
            // The oldest can't be told apart here; trim some away.
            let drop: Vec<PathBuf> = self.positions.keys().take(500).cloned().collect();
            for k in drop {
                self.positions.remove(&k);
            }
        }
        for i in &mut self.items {
            if let Item::Video { path: p, position: pos, duration: dur } = i
                && p == path
            {
                *pos = position;
                *dur = duration;
            }
        }
    }
}

/// Controllers' layouts set up in Chanoma, by SDL's guid.
pub fn load_layouts(dirs: &Dirs) -> HashMap<String, Mapping> {
    crate::store::read(&dirs.state.join("controllers.json"))
}

pub fn save_layouts(dirs: &Dirs, layouts: &HashMap<String, Mapping>) {
    if let Err(e) = crate::store::write(&dirs.state.join("controllers.json"), layouts) {
        log::warn!("chanoma: keeping a controller's buttons: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_items_come_first_once() {
        let mut r = Recent::default();
        r.touch(Item::Game { key: "a".into() });
        r.touch(Item::Service { id: "netflix".into() });
        r.touch(Item::Game { key: "a".into() });
        assert_eq!(r.items, vec![Item::Game { key: "a".into() }, Item::Service { id: "netflix".into() }]);
        for i in 0..20 {
            r.touch(Item::Game { key: i.to_string() });
        }
        assert_eq!(r.items.len(), RECENT);
    }

    #[test]
    fn videos_resume_unless_finished_or_barely_begun() {
        let mut r = Recent::default();
        let p = Path::new("/v/a.mkv");
        r.set_position(p, 5.0, 1200.0);
        assert_eq!(r.resume_at(p), None);
        r.set_position(p, 600.0, 1200.0);
        assert_eq!(r.resume_at(p), Some(600.0));
        r.set_position(p, 1190.0, 1200.0);
        assert_eq!(r.resume_at(p), None);
    }
}
