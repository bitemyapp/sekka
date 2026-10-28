//! Your place in each document — what a 栞 keeps: the page and where on
//! it, and the zoom, by the file's path, in
//! `$XDG_STATE_HOME/aizome/shiori/places.json`. The newest few hundred.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::Zoom;

/// How many documents' places are kept.
const KEPT: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Zoomed {
    FitWidth,
    FitPage,
    Fixed(f64),
}

impl From<Zoom> for Zoomed {
    fn from(z: Zoom) -> Zoomed {
        match z {
            Zoom::FitWidth => Zoomed::FitWidth,
            Zoom::FitPage => Zoomed::FitPage,
            Zoom::Fixed(f) => Zoomed::Fixed(f),
        }
    }
}

impl From<Zoomed> for Zoom {
    fn from(z: Zoomed) -> Zoom {
        match z {
            Zoomed::FitWidth => Zoom::FitWidth,
            Zoomed::FitPage => Zoom::FitPage,
            Zoomed::Fixed(f) => Zoom::Fixed(crate::layout::clamp_zoom(f)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub page: usize,
    /// How far down the page the view's top was, in page points.
    pub y: f64,
    pub zoom: Zoomed,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Places {
    #[serde(skip)]
    file: Option<PathBuf>,
    /// Oldest first.
    places: Vec<(PathBuf, Place)>,
}

impl Places {
    /// The places in `file` (none if it isn't there or can't be read).
    pub fn load(file: PathBuf) -> Places {
        let mut p: Places = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        p.file = Some(file);
        p
    }

    /// Where Shiori keeps them.
    pub fn default_file() -> PathBuf {
        aizome_config::state_dir().join("shiori/places.json")
    }

    /// Places kept nowhere (tests).
    pub fn in_memory() -> Places {
        Places::default()
    }

    pub fn get(&self, path: &Path) -> Option<Place> {
        self.places.iter().rev().find(|(p, _)| p == path).map(|(_, pl)| *pl)
    }

    pub fn set(&mut self, path: &Path, place: Place) {
        self.places.retain(|(p, _)| p != path);
        self.places.push((path.to_path_buf(), place));
        if self.places.len() > KEPT {
            self.places.drain(..self.places.len() - KEPT);
        }
    }

    /// Write them out (beside, then over, the old file).
    pub fn save(&self) {
        let Some(file) = &self.file else { return };
        let Ok(json) = serde_json::to_vec(self) else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, file)).is_err() {
            log::warn!("couldn't keep places in {}", file.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_and_comes_back() {
        let dir = std::env::temp_dir().join(format!("shiori-places-{}", std::process::id()));
        let file = dir.join("places.json");
        let mut p = Places::load(file.clone());
        let place = |page| Place { page, y: 10.5, zoom: Zoomed::Fixed(1.5) };
        for i in 0..KEPT + 3 {
            p.set(Path::new(&format!("/d/{i}.pdf")), place(i));
        }
        p.set(Path::new("/d/5.pdf"), place(99));
        p.save();
        let q = Places::load(file);
        assert_eq!(q.get(Path::new("/d/5.pdf")), Some(place(99)));
        assert_eq!(q.get(Path::new("/d/0.pdf")), None);
        assert_eq!(q.get(Path::new(&format!("/d/{}.pdf", KEPT + 2))), Some(place(KEPT + 2)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
