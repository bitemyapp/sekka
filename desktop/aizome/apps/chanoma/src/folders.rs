//! Folders, for choosing one with a controller from across the room (the
//! kit's file panel wants a pointer and a keyboard): the places to start
//! from, a folder's own folders with a word on what's in each, the folders
//! that look like a collection (a drive's `roms`, a home's `Movies`), and
//! what the files index already knows is in one.
//!
//! Browsing reads a folder and the folders in it, never deeper: a folder
//! is looked through to the bottom once it's added, not while passing by,
//! as the first look through a big drive can take minutes.
//!
//! Everything here reads the disk: the program calls it off the interface
//! thread.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::library::{self, Cores, Held};

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
    out.extend(names(rest));
    out
}

fn names(p: &Path) -> impl Iterator<Item = String> + '_ {
    p.components().filter_map(|c| match c {
        std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
        _ => None,
    })
}

/// A folder as people say it: its name, and where it is ("roms", "On
/// Extreme Pro"; "Videos", "In your home folder"), from `home` and the
/// folders drives appear in (`mounts`), so a drive's folder is said the
/// same while the drive is out.
pub fn said(home: &Path, mounts: &[PathBuf], dir: &Path) -> (String, String) {
    let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned());
    if let Ok(rest) = dir.strip_prefix(home) {
        let mut up: Vec<String> = names(rest).collect();
        up.pop();
        return match up.len() {
            _ if rest.as_os_str().is_empty() => ("Home".into(), "Your home folder".into()),
            0 => (name, "In your home folder".into()),
            _ => (name, format!("In Home › {}", up.join(" › "))),
        };
    }
    // The drive is the first folder under the folder drives appear in
    // (the user's own, `/run/media/$USER`, before `/run/media`).
    let mount = mounts.iter().filter(|m| dir.starts_with(m)).max_by_key(|m| m.components().count());
    if let Some(m) = mount {
        let mut parts: Vec<String> = names(dir.strip_prefix(m).unwrap_or(dir)).collect();
        if !parts.is_empty() {
            let drive = parts.remove(0);
            parts.pop();
            return match parts.len() {
                _ if dir.parent() == Some(m.as_path()) => (drive, "The whole drive".into()),
                0 => (name, format!("On {drive}")),
                _ => (name, format!("On {drive} › {}", parts.join(" › "))),
            };
        }
    }
    let up = dir.parent().map_or_else(String::new, |p| p.display().to_string());
    (name, format!("On this computer, in {up}"))
}

/// What's directly in a folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Inside {
    pub folders: usize,
    /// The systems its folders are named for (`snes/`), by short name.
    pub systems: Vec<&'static str>,
    pub games: usize,
    pub videos: usize,
    pub other: usize,
}

impl Inside {
    /// A word on it, whatever it holds: "Folders for 31 systems: Game Boy,
    /// Genesis, NES…", "12 games · 3 folders", "4 videos".
    pub fn describe(&self) -> String {
        if !self.systems.is_empty() {
            let n = self.systems.len();
            let first: Vec<&str> = self.systems.iter().take(3).copied().collect();
            let more = if n > 3 { "…" } else { "" };
            let mut s = if n == 1 {
                format!("A folder for {}", first[0])
            } else {
                format!("Folders for {n} systems: {}{more}", first.join(", "))
            };
            if self.videos > 0 {
                s.push_str(&format!(" · {}", count(self.videos, "video")));
            }
            return s;
        }
        let mut parts = Vec::new();
        if self.games > 0 {
            parts.push(count(self.games, "game"));
        }
        if self.videos > 0 {
            parts.push(count(self.videos, "video"));
        }
        if self.folders > 0 {
            parts.push(count(self.folders, "folder"));
        }
        if parts.is_empty() {
            parts.push(if self.other > 0 { count(self.other, "other file") } else { "Empty".to_owned() });
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

/// Whether a directory entry is a folder: by the listing's word for it,
/// a link followed (as the library does).
fn is_folder(e: &std::fs::DirEntry) -> bool {
    match e.file_type() {
        Ok(t) if t.is_symlink() => e.path().is_dir(),
        Ok(t) => t.is_dir(),
        Err(_) => false,
    }
}

/// The folders in `dir` (not hidden ones), in the order people count.
pub fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.') && is_folder(&e)).then(|| Entry { name, path: e.path(), inside: None })
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

/// What's directly in `dir`: one listing, nothing deeper.
pub fn look_into(dir: &Path) -> Option<Inside> {
    let rd = std::fs::read_dir(dir).ok()?;
    // The folder's own name says a system too (`snes/`).
    let root = dir.parent().unwrap_or(dir);
    let mut n = Inside::default();
    for e in rd.flatten().take(COUNTED) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        if is_folder(&e) {
            n.folders += 1;
            if let Some(s) = library::system_named(&name)
                && !n.systems.contains(&s.short)
            {
                n.systems.push(s.short);
            }
        } else if library::system_of(&path, root).is_some() {
            n.games += 1;
        } else if aizome_media::is_video(&path) {
            n.videos += 1;
        } else {
            n.other += 1;
        }
    }
    n.systems.sort_unstable();
    Some(n)
}

/// A folder that looks like a collection, offered before browsing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Suggestion {
    pub path: PathBuf,
    pub name: String,
    /// Where it is ("On Extreme Pro").
    pub place: String,
    /// What it looks like ("Folders for 31 systems: Game Boy, Genesis, NES…").
    pub word: String,
}

/// Names a folder of games goes by, or a folder holding one does
/// (Batocera's `share`, an `Emulation` folder holding `roms`).
const GAMES_NAMES: &[&str] =
    &["games", "roms", "rom", "retro", "emulation", "emulators", "consoles", "share", "userdata"];
/// Names a folder of films and shows goes by.
const VIDEO_NAMES: &[&str] =
    &["videos", "movies", "films", "tv", "tvshows", "shows", "series", "cartoons", "anime", "kids", "documentaries"];
/// How many of a folder's folders are peeked into, looking a level deeper.
const PEEKED: usize = 24;

fn key(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The folders on `places` (home's, and each drive's) that look like a
/// collection: games (folders named for systems, or games of their own)
/// or films and shows. Each place and the folders in it are listed once
/// (two levels, never the whole drive), and a level deeper under a folder
/// named for games (`Emulation/roms`) or videos (`Movies/Up (2009)`).
/// `skip` says which folders are already looked through. Games first, the
/// most systems first.
pub fn suggest(places: &[Place], home: &Path, mounts: &[PathBuf], skip: &dyn Fn(&Path) -> bool) -> Vec<Suggestion> {
    let mut found: Vec<(usize, PathBuf, Inside, usize)> = Vec::new();
    let mut seen = HashSet::new();
    for p in places.iter().filter(|p| matches!(p.kind, PlaceKind::Home | PlaceKind::Folder | PlaceKind::Drive)) {
        // A drive whole (its top may be a collection), not a home.
        let mut cands = Vec::new();
        if p.kind != PlaceKind::Home {
            cands.push(p.path.clone());
        }
        cands.extend(subfolders(&p.path).into_iter().take(60));
        for c in cands {
            if !seen.insert(c.clone()) || skip(&c) || found.iter().any(|(_, f, _, _)| c.starts_with(f)) {
                continue;
            }
            let Some(inside) = look_into(&c) else { continue };
            let name = key(&c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if inside.systems.len() >= 2
                || inside.games >= 3
                || (GAMES_NAMES.contains(&name.as_str()) && inside.systems.len() + inside.games > 0)
            {
                let rank = 1_000_000 + inside.systems.len() * 1000 + inside.games.min(999);
                found.push((rank, c, inside, 0));
                continue;
            }
            if GAMES_NAMES.contains(&name.as_str()) {
                // A level deeper: `Emulation/roms`.
                for d in subfolders(&c).into_iter().take(PEEKED) {
                    if let Some(i) = look_into(&d).filter(|i| i.systems.len() >= 2)
                        && !skip(&d)
                    {
                        found.push((1_000_000 + i.systems.len() * 1000, d, i, 0));
                    }
                }
                continue;
            }
            // Films: loose, or a folder each (`Movies/Up (2009)/Up.mkv`).
            let mut videos = inside.videos;
            if VIDEO_NAMES.contains(&name.as_str()) && inside.folders > 0 {
                videos += subfolders(&c)
                    .into_iter()
                    .take(PEEKED)
                    .filter(|d| look_into(d).is_some_and(|i| i.videos > 0))
                    .count();
            }
            if videos >= 3 || (VIDEO_NAMES.contains(&name.as_str()) && videos > 0) {
                found.push((videos, c, inside, videos));
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found
        .into_iter()
        .take(8)
        .map(|(_, path, inside, videos)| {
            let (name, place) = said(home, mounts, &path);
            let word = if videos > inside.videos {
                if videos == 1 { "1 film or show".to_owned() } else { format!("{videos} films or shows") }
            } else {
                inside.describe()
            };
            Suggestion { path, name, place, word }
        })
        .collect()
}

/// `dir`'s folders, not hidden ones, by name.
fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.') && is_folder(e))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// What the files index already knows is in `dir`, counted as Chanoma
/// counts its folders: nothing is read from the disk for it. `None` when
/// it knows of nothing there (it may never have looked).
pub fn known_in(index: &mut aizome_index::Index, dir: &Path, cores: &Cores) -> Option<Held> {
    let held = library::held_in(index, &[dir.to_path_buf()], cores, false).pop()?;
    (held.games + held.unplayable + held.shows + held.films > 0).then_some(held)
}

/// What a folder holds, said plainly: "6509 games for 31 systems · 2 shows
/// and 3 films".
pub fn held_word(h: &Held) -> String {
    let mut parts = Vec::new();
    if h.games > 0 {
        let mut s = format!("{} for {}", count(h.games, "game"), count(h.systems, "system"));
        if h.unplayable > 0 {
            s.push_str(&format!(", and {} without a game player here", h.unplayable));
        }
        parts.push(s);
    } else if h.unplayable > 0 {
        parts.push(format!("{}, for systems without a game player here", count(h.unplayable, "game")));
    }
    match (h.shows, h.films) {
        (0, 0) => {}
        (0, f) => parts.push(count(f, "film")),
        (s, 0) => parts.push(count(s, "show")),
        (s, f) => parts.push(format!("{} and {}", count(s, "show"), count(f, "film"))),
    }
    if parts.is_empty() {
        return "Nothing to play or watch in it yet".into();
    }
    parts.join(" · ")
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

    fn touch(root: &Path, files: &[&str]) {
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
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
    fn a_folder_is_said_as_people_say_it_drive_out_or_in() {
        let home = Path::new("/home/ann");
        let mounts = vec![PathBuf::from("/run/media/ann"), PathBuf::from("/run/media")];
        let s = |p: &str| said(home, &mounts, Path::new(p));
        assert_eq!(s("/home/ann/Videos"), ("Videos".into(), "In your home folder".into()));
        assert_eq!(s("/home/ann/Games/snes"), ("snes".into(), "In Home › Games".into()));
        assert_eq!(s("/home/ann"), ("Home".into(), "Your home folder".into()));
        assert_eq!(s("/run/media/ann/Extreme Pro/roms"), ("roms".into(), "On Extreme Pro".into()));
        assert_eq!(s("/run/media/ann/Extreme Pro/a/b"), ("b".into(), "On Extreme Pro › a".into()));
        assert_eq!(s("/run/media/ann/Extreme Pro"), ("Extreme Pro".into(), "The whole drive".into()));
        assert_eq!(s("/srv/films"), ("films".into(), "On this computer, in /srv".into()));
    }

    #[test]
    fn a_folders_folders_say_whats_in_them() {
        let root = temp("list");
        for d in ["snes", "Disc 10", "Disc 9", ".hidden", "empty", "roms/gba", "roms/nes", "roms/Mega Drive"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        touch(&root, &["snes/Kirby.sfc", "snes/Mario.sfc", "Disc 9/Film.mkv", "Disc 9/notes.txt", "loose.sfc"]);
        std::fs::create_dir_all(root.join("Disc 10/extras")).unwrap();
        let entries = list(&root).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["Disc 9", "Disc 10", "empty", "roms", "snes"],
            "folders only, none hidden, counted as people do"
        );
        let said: Vec<String> = entries.iter().map(|e| e.inside.as_ref().unwrap().describe()).collect();
        assert_eq!(
            said,
            ["1 video", "1 folder", "Empty", "Folders for 3 systems: Game Boy Advance, Genesis, NES", "2 games"]
        );
        assert!(list(&root.join("missing")).is_err());
    }

    #[test]
    fn collections_are_suggested_from_two_levels_never_the_whole_drive() {
        let root = temp("suggest");
        let home = root.join("home");
        let media = root.join("media");
        let drive = media.join("Extreme Pro");
        touch(
            &drive,
            &[
                "roms/snes/Kirby.sfc",
                "roms/gba/Golden Sun.gba",
                "roms/psx/Disc/Disc.cue",
                "backup/2019/photo.jpg",
                "backup/2019/deep/snes/Old.sfc",
                "bios/scph5501.bin",
            ],
        );
        touch(
            &home,
            &[
                "Movies/Up (2009)/Up.mkv",
                "Movies/Coco (2017)/Coco.mkv",
                "Emulation/roms/gb/Tetris.gb",
                "Emulation/roms/nes/Zelda.nes",
                "Documents/letter.txt",
            ],
        );
        let mounts = vec![media.clone()];
        let places = places(&home, &mounts);
        let s = suggest(&places, &home, &mounts, &|_| false);
        let got: Vec<(&str, &str, &str)> =
            s.iter().map(|s| (s.name.as_str(), s.place.as_str(), s.word.as_str())).collect();
        assert_eq!(
            got,
            [
                ("roms", "On Extreme Pro", "Folders for 3 systems: Game Boy Advance, PlayStation, Super NES"),
                ("roms", "In Home › Emulation", "Folders for 2 systems: Game Boy, NES"),
                ("Movies", "In your home folder", "2 films or shows"),
            ],
            "{s:?}"
        );
        // One already looked through isn't offered.
        let added = drive.join("roms");
        let s = suggest(&places, &home, &mounts, &|p| p.starts_with(&added));
        assert!(s.iter().all(|s| s.path != added), "{s:?}");
    }

    #[test]
    fn what_a_folder_holds_is_said_plainly() {
        let h = |games, systems, unplayable, shows, films| {
            held_word(&Held { games, systems, unplayable, shows, films, ..Held::default() })
        };
        assert_eq!(h(0, 0, 0, 0, 0), "Nothing to play or watch in it yet");
        assert_eq!(h(1, 1, 0, 0, 0), "1 game for 1 system");
        assert_eq!(h(38, 4, 2, 0, 0), "38 games for 4 systems, and 2 without a game player here");
        assert_eq!(h(0, 0, 3, 0, 0), "3 games, for systems without a game player here");
        assert_eq!(h(0, 0, 0, 2, 1), "2 shows and 1 film");
        assert_eq!(h(6509, 31, 0, 0, 2), "6509 games for 31 systems · 2 films");
    }
}
