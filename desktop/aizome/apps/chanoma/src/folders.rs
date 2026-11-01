//! Folders, for choosing one with a controller from across the room (the
//! kit's file panel wants a pointer and a keyboard): the places to start
//! from, a folder's own folders with a word on what's in each, and what
//! Chanoma would find in one, by the same reading it gives a games or
//! videos folder.
//!
//! Everything here reads the disk: the program calls it off the interface
//! thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::library::{self, Cores};

/// Where browsing can start.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    pub kind: PlaceKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlaceKind {
    Home,
    /// One of home's usual folders.
    Folder,
    /// A drive plugged in.
    Drive,
    /// The whole computer, from `/`.
    Computer,
}

/// Home's folders worth starting from, where they exist.
const USUAL: &[&str] = &["Games", "ROMs", "roms", "Videos", "Movies", "Downloads"];

/// Home, its usual folders, each drive under `mounts` (the folders
/// removable drives appear in: `/run/media/$USER`, `/media`…), and the
/// whole computer. An empty folder there is a drive's place while it's
/// out, and is left out.
pub fn places(home: &Path, mounts: &[PathBuf]) -> Vec<Place> {
    let mut out = vec![Place { name: "Home".into(), path: home.to_path_buf(), kind: PlaceKind::Home }];
    for name in USUAL {
        let path = home.join(name);
        if path.is_dir() {
            out.push(Place { name: (*name).into(), path, kind: PlaceKind::Folder });
        }
    }
    for root in mounts {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut drives: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            // `/media/$USER` holds drives; it isn't one.
            .filter(|p| !mounts.contains(p) && !hidden(p) && p.is_dir() && !empty(p))
            .collect();
        drives.sort();
        for path in drives {
            if out.iter().any(|q| q.path == path) {
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(Place { name, path, kind: PlaceKind::Drive });
        }
    }
    out.push(Place { name: "This computer".into(), path: PathBuf::from("/"), kind: PlaceKind::Computer });
    out
}

fn hidden(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

fn empty(p: &Path) -> bool {
    std::fs::read_dir(p).map_or(true, |mut rd| rd.next().is_none())
}

/// The place `dir` is in: the deepest one holding it.
pub fn place_of(places: &[Place], dir: &Path) -> Option<usize> {
    places
        .iter()
        .enumerate()
        .filter(|(_, p)| dir.starts_with(&p.path))
        .max_by_key(|(_, p)| p.path.components().count())
        .map(|(i, _)| i)
}

/// Where `dir` is, as words: from home, a drive or the computer, then each
/// folder down to it ("Home", "ROMs", "snes").
pub fn crumbs(places: &[Place], dir: &Path) -> Vec<String> {
    let root = places
        .iter()
        .filter(|p| p.kind != PlaceKind::Folder && dir.starts_with(&p.path))
        .max_by_key(|p| p.path.components().count());
    let (mut out, rest) = match root {
        Some(p) => (vec![p.name.clone()], dir.strip_prefix(&p.path).unwrap_or(dir)),
        None => (vec!["This computer".to_owned()], dir),
    };
    out.extend(rest.components().filter_map(|c| match c {
        std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
        _ => None,
    }));
    out
}

/// What's directly in a folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Inside {
    pub folders: usize,
    pub games: usize,
    pub videos: usize,
    pub other: usize,
}

impl Inside {
    /// A word on it, for choosing `games` (or videos): "12 games · 3
    /// folders".
    pub fn describe(&self, games: bool) -> String {
        let mut parts = Vec::new();
        // Videos are other files to a games folder, and games to a
        // videos folder.
        let (found, word, other) = if games {
            (self.games, "game", self.other + self.videos)
        } else {
            (self.videos, "video", self.other + self.games)
        };
        if found > 0 {
            parts.push(count(found, word));
        }
        if self.folders > 0 {
            parts.push(count(self.folders, "folder"));
        }
        if parts.is_empty() {
            parts.push(if other > 0 { count(other, "other file") } else { "Empty".to_owned() });
        }
        parts.join(" · ")
    }
}

/// A folder in a folder.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    /// What's directly in it, when looked into (not in a folder of
    /// thousands of folders).
    pub inside: Option<Inside>,
}

/// How many folders in one are looked into.
const LOOKED_INTO: usize = 400;
/// How many things in a folder are counted at most.
const COUNTED: usize = 5000;

/// The folders in `dir` (not hidden ones), in the order people count.
pub fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            // Followed links to folders too, as the library does.
            (!name.starts_with('.') && path.is_dir()).then_some(Entry { name, path, inside: None })
        })
        .collect();
    out.sort_by(|a, b| library::natural(&a.name.to_lowercase(), &b.name.to_lowercase()));
    if out.len() <= LOOKED_INTO {
        for e in &mut out {
            e.inside = look_into(&e.path);
        }
    }
    Ok(out)
}

fn look_into(dir: &Path) -> Option<Inside> {
    let rd = std::fs::read_dir(dir).ok()?;
    // The folder's own name says a system too (`snes/`).
    let root = dir.parent().unwrap_or(dir);
    let mut n = Inside::default();
    for e in rd.flatten().take(COUNTED) {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            n.folders += 1;
        } else if library::system_of(&path, root).is_some() {
            n.games += 1;
        } else if aizome_media::is_video(&path) {
            n.videos += 1;
        } else {
            n.other += 1;
        }
    }
    Some(n)
}

/// What Chanoma would find in a folder chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Found {
    Games { games: usize, systems: usize, unplayable: usize },
    Videos { shows: usize, films: usize },
}

/// The games in `dir`, as a games folder of Chanoma's is read.
pub fn games_in(dir: &Path, cores: &Cores) -> Found {
    let (games, unplayable) = library::scan_games(&[dir.to_path_buf()], cores);
    let systems: HashSet<&str> = games.iter().map(|g| g.system.id).collect();
    Found::Games { games: games.len(), systems: systems.len(), unplayable }
}

/// The films and shows in `dir`, as a videos folder of Chanoma's is read.
pub fn videos_in(dir: &Path) -> Found {
    let shelves = library::scan_videos(&[dir.to_path_buf()]);
    let shows = shelves.iter().filter(|s| s.videos.len() > 1).count();
    Found::Videos { shows, films: shelves.len() - shows }
}

impl Found {
    /// Whether there's anything to play.
    pub fn any(&self) -> bool {
        match *self {
            Found::Games { games, .. } => games > 0,
            Found::Videos { shows, films } => shows + films > 0,
        }
    }

    /// What it says beside the choice.
    pub fn describe(&self) -> String {
        match *self {
            Found::Games { games: 0, unplayable: 0, .. } => "No games in here yet".into(),
            Found::Games { games: 0, unplayable, .. } => {
                format!("{}, for systems without an emulator here", count(unplayable, "game"))
            }
            Found::Games { games, systems, unplayable } => {
                let mut s = format!("{} for {}", count(games, "game"), count(systems, "system"));
                if unplayable > 0 {
                    s += &format!(", and {unplayable} without an emulator");
                }
                s
            }
            Found::Videos { shows: 0, films: 0 } => "No films or shows in here yet".into(),
            Found::Videos { shows: 0, films } => count(films, "film"),
            Found::Videos { shows, films: 0 } => count(shows, "show"),
            Found::Videos { shows, films } => format!("{} and {}", count(shows, "show"), count(films, "film")),
        }
    }
}

fn count(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chanoma-folders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn places_are_home_its_usual_folders_drives_in_and_the_computer() {
        let root = temp("places");
        let home = root.join("home");
        for d in ["Games", "Videos", "Music"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let media = root.join("media");
        // A drive in, another's place while it's out, and the user's own
        // folder of drives.
        std::fs::create_dir_all(media.join("SD CARD/snes")).unwrap();
        std::fs::create_dir_all(media.join("USB")).unwrap();
        std::fs::create_dir_all(media.join("alice/STICK/gba")).unwrap();
        let mounts = vec![media.join("alice"), media.clone()];
        let p = places(&home, &mounts);
        let names: Vec<&str> = p.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Home", "Games", "Videos", "STICK", "SD CARD", "This computer"]);
        assert_eq!(p[3].kind, PlaceKind::Drive);
        assert_eq!(place_of(&p, &home.join("Games/snes")), Some(1));
        assert_eq!(place_of(&p, &home.join("Music")), Some(0));
        assert_eq!(place_of(&p, Path::new("/etc")), Some(5));
        assert_eq!(crumbs(&p, &home.join("Games/snes")), ["Home", "Games", "snes"]);
        assert_eq!(crumbs(&p, &media.join("SD CARD/snes")), ["SD CARD", "snes"]);
        assert_eq!(crumbs(&p, Path::new("/")), ["This computer"]);
    }

    #[test]
    fn a_folders_folders_say_whats_in_them() {
        let root = temp("list");
        for d in ["snes", "Disc 10", "Disc 9", ".hidden", "empty"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("snes/Kirby.sfc"), b"").unwrap();
        std::fs::write(root.join("snes/Mario.sfc"), b"").unwrap();
        std::fs::write(root.join("Disc 9/Film.mkv"), b"").unwrap();
        std::fs::write(root.join("Disc 9/notes.txt"), b"").unwrap();
        std::fs::create_dir_all(root.join("Disc 10/extras")).unwrap();
        std::fs::write(root.join("loose.sfc"), b"").unwrap();
        let entries = list(&root).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Disc 9", "Disc 10", "empty", "snes"], "folders only, none hidden, counted as people do");
        let said: Vec<String> = entries.iter().map(|e| e.inside.unwrap().describe(true)).collect();
        assert_eq!(said, ["2 other files", "1 folder", "Empty", "2 games"]);
        assert_eq!(entries[0].inside.unwrap().describe(false), "1 video");
        assert!(list(&root.join("missing")).is_err());
    }

    #[test]
    fn what_a_folder_holds_is_said_plainly() {
        let g = |games, systems, unplayable| Found::Games { games, systems, unplayable }.describe();
        assert_eq!(g(0, 0, 0), "No games in here yet");
        assert_eq!(g(1, 1, 0), "1 game for 1 system");
        assert_eq!(g(38, 4, 2), "38 games for 4 systems, and 2 without an emulator");
        assert_eq!(g(0, 0, 3), "3 games, for systems without an emulator here");
        let v = |shows, films| Found::Videos { shows, films }.describe();
        assert_eq!(v(0, 0), "No films or shows in here yet");
        assert_eq!(v(2, 1), "2 shows and 1 film");
        assert!(!Found::Videos { shows: 0, films: 0 }.any());
    }
}
