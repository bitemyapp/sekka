//! What Chanoma remembers between runs besides the grown-ups' settings:
//! what was played lately, where films were left and the languages each
//! show was last watched in (`recent.json`), and controllers' buttons as
//! set up here (`controllers.json`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use aizome_media::{Track, language_name};

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

/// A track chosen: by its language where it says one (so the next
/// episode's English is found wherever it is), and by its place among
/// the others.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    pub language: Option<String>,
    pub index: usize,
}

impl Pick {
    pub fn of(tracks: &[Track], index: usize) -> Pick {
        Pick { language: tracks.get(index).and_then(|t| t.language.clone()), index }
    }

    /// The track it is among `tracks`: of the same language (named alike:
    /// `en` and `eng` are both English), at the same place if several
    /// are; with no language, at the same place. None where it isn't.
    pub fn find(&self, tracks: &[Track]) -> Option<usize> {
        let Some(lang) = &self.language else { return (self.index < tracks.len()).then_some(self.index) };
        let name = language_name(lang);
        let same: Vec<usize> = (0..tracks.len())
            .filter(|i| tracks[*i].language.as_deref().map(language_name).as_ref() == Some(&name))
            .collect();
        if same.contains(&self.index) { Some(self.index) } else { same.first().copied() }
    }
}

/// The sound and subtitles chosen for a show or a film; what isn't
/// chosen is the film's own default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Languages {
    pub audio: Option<Pick>,
    pub subtitles: Option<Subtitles>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subtitles {
    Off,
    On(Pick),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recent {
    pub items: Vec<Item>,
    /// Where each video was left (seconds, and its length), by path:
    /// more than "Keep playing" shows.
    pub positions: HashMap<PathBuf, (f64, f64)>,
    /// The sound and subtitles chosen for a show (its folder) or a film
    /// (its file), kept for its next episodes and the next time.
    pub languages: BTreeMap<String, Languages>,
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

    /// Where a video was left: none if it was short, barely begun or
    /// finished, as Utsushi says too ([`aizome_media::resume_point`]).
    pub fn resume_at(&self, path: &Path) -> Option<f64> {
        let (pos, dur) = *self.positions.get(path)?;
        aizome_media::resume_point(pos, dur)
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
    fn videos_resume_unless_short_finished_or_barely_begun() {
        let mut r = Recent::default();
        let p = Path::new("/v/a.mkv");
        r.set_position(p, 5.0, 1200.0);
        assert_eq!(r.resume_at(p), None);
        r.set_position(p, 600.0, 1200.0);
        assert_eq!(r.resume_at(p), Some(600.0));
        r.set_position(p, 1190.0, 1200.0);
        assert_eq!(r.resume_at(p), None);
        // A short one starts again.
        r.set_position(p, 120.0, 240.0);
        assert_eq!(r.resume_at(p), None);
    }

    #[test]
    fn a_language_chosen_is_found_again_in_the_next_episode() {
        let track = |lang: Option<&str>| Track {
            stream: Some(0),
            file: None,
            language: lang.map(String::from),
            title: None,
            codec: String::new(),
            channels: None,
            default: false,
            forced: false,
        };
        let one = [track(Some("jpn")), track(Some("eng"))];
        let english = Pick::of(&one, 1);
        assert_eq!(english, Pick { language: Some("eng".into()), index: 1 });
        // The next episode lists English first, and as `en`.
        assert_eq!(english.find(&[track(Some("en")), track(Some("ja"))]), Some(0));
        // Two English tracks: the one at the same place.
        assert_eq!(english.find(&[track(Some("eng")), track(Some("eng"))]), Some(1));
        // None in English: none, rather than another language.
        assert_eq!(english.find(&[track(Some("fra"))]), None);
        // Tracks that say no language: by place.
        let second = Pick::of(&[track(None), track(None)], 1);
        assert_eq!(second.find(&[track(None), track(None)]), Some(1));
        assert_eq!(second.find(&[track(None)]), None);
        // Kept as the file keeps it.
        let l = Languages { audio: Some(english), subtitles: Some(Subtitles::Off) };
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<Languages>(&json).unwrap(), l);
    }
}
