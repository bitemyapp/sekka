//! What's on the disk to play and watch: games in the games folders (each
//! known by its system's folder name or its file's extension), the cores
//! that play them, and films and shows in the video folders.
//!
//! Folders are read on a worker thread, through the files index Aizome's
//! apps share ([`aizome_index`]): a second look reads only folders that
//! changed, and what was found shows before it does. The result is one
//! [`Library`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aizome_index::{Index, Kind, Refresh};

/// A games system Chanoma plays.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct System {
    pub id: &'static str,
    pub name: &'static str,
    /// What a tile says.
    pub short: &'static str,
    /// Folder names that mean it, lower case, letters and digits only.
    pub folders: &'static [&'static str],
    pub extensions: &'static [&'static str],
    /// libretro cores that play it, best first, by file stem
    /// (`snes9x` for `snes9x_libretro.so`).
    pub cores: &'static [&'static str],
    /// The tile's colour (0xRRGGBB).
    pub color: u32,
    /// Players it takes at most.
    pub players: usize,
    /// Core settings Chanoma prefers over the core's defaults.
    pub prefer: &'static [(&'static str, &'static str)],
}

impl System {
    /// Whether a file with extension `ext` (lower case) in a folder named
    /// for this system is one of its games: its own extensions, and zips,
    /// as cartridge sets come (a disc isn't played zipped). A zip elsewhere
    /// could be anything's, and isn't opened to see.
    pub fn takes(&self, ext: &str) -> bool {
        self.extensions.contains(&ext) || (ext == "zip" && self.id != "psx")
    }
}

pub const SYSTEMS: &[System] = &[
    System {
        id: "nes",
        name: "Nintendo Entertainment System",
        short: "NES",
        folders: &["nes", "famicom", "fc", "nintendo"],
        extensions: &["nes", "fds", "unf", "unif"],
        cores: &["mesen", "nestopia", "fceumm", "quicknes"],
        color: 0xb8312f,
        players: 2,
        prefer: &[],
    },
    System {
        id: "snes",
        name: "Super Nintendo",
        short: "Super NES",
        folders: &["snes", "sfc", "superfamicom", "supernintendo", "supernes"],
        extensions: &["sfc", "smc", "fig", "swc", "bs"],
        cores: &["snes9x", "bsnes", "bsnes_mercury_balanced", "snes9x2010"],
        color: 0x5b4ea8,
        players: 2,
        prefer: &[],
    },
    System {
        id: "gb",
        name: "Game Boy",
        short: "Game Boy",
        folders: &["gb", "gameboy"],
        extensions: &["gb", "dmg"],
        cores: &["gambatte", "sameboy", "mgba"],
        color: 0x7c8a3a,
        players: 1,
        // The green screen it had.
        prefer: &[("gambatte_gb_colorization", "internal"), ("gambatte_gb_internal_palette", "GB - DMG")],
    },
    System {
        id: "gbc",
        name: "Game Boy Color",
        short: "Game Boy Color",
        folders: &["gbc", "gameboycolor", "gameboycolour"],
        extensions: &["gbc"],
        cores: &["gambatte", "sameboy", "mgba"],
        color: 0x9b3b8c,
        players: 1,
        prefer: &[],
    },
    System {
        id: "gba",
        name: "Game Boy Advance",
        short: "Game Boy Advance",
        folders: &["gba", "gameboyadvance"],
        extensions: &["gba"],
        cores: &["mgba", "vbam", "vba_next", "gpsp"],
        color: 0x3b4fa0,
        players: 1,
        prefer: &[],
    },
    System {
        id: "genesis",
        name: "Sega Genesis",
        short: "Genesis",
        folders: &["genesis", "megadrive", "md", "segagenesis"],
        extensions: &["md", "gen", "smd", "68k"],
        cores: &["genesis_plus_gx", "picodrive"],
        color: 0x1d1d1d,
        players: 2,
        prefer: &[],
    },
    System {
        id: "sms",
        name: "Sega Master System",
        short: "Master System",
        folders: &["sms", "mastersystem", "segamastersystem"],
        extensions: &["sms"],
        cores: &["genesis_plus_gx", "picodrive", "smsplus"],
        color: 0xc0392b,
        players: 2,
        prefer: &[],
    },
    System {
        id: "gg",
        name: "Sega Game Gear",
        short: "Game Gear",
        folders: &["gg", "gamegear"],
        extensions: &["gg"],
        cores: &["genesis_plus_gx", "picodrive"],
        color: 0x2c3e50,
        players: 1,
        prefer: &[],
    },
    System {
        id: "pce",
        name: "PC Engine",
        short: "PC Engine",
        folders: &["pce", "pcengine", "tg16", "turbografx", "turbografx16"],
        extensions: &["pce"],
        cores: &["mednafen_pce_fast", "mednafen_pce", "mednafen_supergrafx"],
        color: 0xd35400,
        players: 2,
        prefer: &[],
    },
    System {
        id: "psx",
        name: "PlayStation",
        short: "PlayStation",
        folders: &["psx", "ps1", "playstation", "psone"],
        extensions: &["chd", "cue", "pbp", "m3u"],
        // PCSX-ReARMed first: it plays without a BIOS image of the console's.
        cores: &["pcsx_rearmed", "swanstation", "mednafen_psx"],
        color: 0x5f6b7a,
        players: 2,
        prefer: &[("swanstation_GPU_Renderer", "Software")],
    },
    System {
        id: "atari2600",
        name: "Atari 2600",
        short: "Atari 2600",
        folders: &["atari2600", "a2600", "atari"],
        extensions: &["a26"],
        cores: &["stella", "stella2014"],
        color: 0x8e5b2e,
        players: 2,
        prefer: &[],
    },
    System {
        id: "lynx",
        name: "Atari Lynx",
        short: "Lynx",
        folders: &["lynx", "atarilynx"],
        extensions: &["lnx"],
        cores: &["handy", "mednafen_lynx"],
        color: 0x6d6d6d,
        players: 1,
        prefer: &[],
    },
    System {
        id: "ngp",
        name: "Neo Geo Pocket",
        short: "Neo Geo Pocket",
        folders: &["ngp", "ngpc", "neogeopocket", "neogeopocketcolor"],
        extensions: &["ngp", "ngc"],
        cores: &["mednafen_ngp"],
        color: 0x2471a3,
        players: 1,
        prefer: &[],
    },
    System {
        id: "wswan",
        name: "WonderSwan",
        short: "WonderSwan",
        folders: &["wswan", "wonderswan", "wonderswancolor", "wsc"],
        extensions: &["ws", "wsc"],
        cores: &["mednafen_wswan"],
        color: 0x16a085,
        players: 1,
        prefer: &[],
    },
];

pub fn system(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// A folder name as `System::folders` writes them.
fn folder_key(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The system a folder's name says (`snes`, `Super Nintendo`).
pub fn system_named(name: &str) -> Option<&'static System> {
    let key = folder_key(name);
    SYSTEMS.iter().find(|s| s.folders.contains(&key.as_str()))
}

/// The system a file is for: its nearest folder that names one, else its
/// extension (when only one system uses it).
pub fn system_of(path: &Path, root: &Path) -> Option<&'static System> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        if let Some(s) = d.file_name().and_then(|n| system_named(&n.to_string_lossy())) {
            // A system's folder holds its games; other files there aren't.
            return s.takes(&ext).then_some(s);
        }
        dir = d.parent();
    }
    let mut by_ext = SYSTEMS.iter().filter(|s| s.extensions.contains(&ext.as_str()));
    let first = by_ext.next()?;
    by_ext.next().is_none().then_some(first)
}

/// A game's name from its file's: without the extension, the dump's
/// tags (`(USA)`, `[!]`), or a trailing `, The` moved to the front.
pub fn title_of(path: &Path) -> String {
    clean(&path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
}

/// A name without its tags, as [`title_of`] makes them (folders' names).
pub fn clean(stem: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in stem.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let mut t = out.split_whitespace().collect::<Vec<_>>().join(" ").replace('_', " ");
    // "Legend of Zelda, The - A Link to the Past"
    if let Some(i) = t.find(", The") {
        let after = &t[i + 5..];
        if after.is_empty() || after.starts_with(' ') {
            t = format!("The {}{}", &t[..i], after);
        }
    }
    let t = t.trim().to_owned();
    if t.is_empty() { stem.to_owned() } else { t }
}

/// A game on the disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Game {
    /// `<system>/<path in its games folder>`: what rules and saves go by.
    pub key: String,
    pub title: String,
    pub path: PathBuf,
    pub system: &'static System,
    /// Box art found beside it (or that its folder's `gamelist.xml` names).
    pub art: Option<PathBuf>,
    /// What the databases say of it.
    pub info: Option<crate::metadata::Info>,
    /// The name its database knows it by, when it's in one: its box art's
    /// name in libretro-thumbnails.
    pub known_as: Option<String>,
}

impl Game {
    /// A file-name-safe form of the key, for saves.
    pub fn file_key(&self) -> String {
        self.key.chars().map(|c| if c == '/' || c == '\\' { '~' } else { c }).collect()
    }
}

/// A video on the disk.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Video {
    pub title: String,
    pub path: PathBuf,
}

/// A film, or a show's folder of episodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shelf {
    /// Its folder (a show, or a film in a folder of its own) or its file,
    /// as kids' rules keep it.
    pub key: String,
    pub title: String,
    /// One for a film; a show's episodes in order.
    pub videos: Vec<Video>,
    pub poster: Option<PathBuf>,
}

/// Folders of console BIOS images to offer the cores: `$CHANOMA_BIOS`
/// (several, colon-separated), which the module points at OpenBIOS.
pub fn bios_dirs() -> Vec<PathBuf> {
    std::env::var_os("CHANOMA_BIOS").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default()
}

/// Cores found, by stem.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cores {
    pub found: Vec<(String, PathBuf)>,
}

impl Cores {
    /// The folders cores are looked for in: `$CHANOMA_CORES` (several,
    /// colon-separated), then the usual places.
    pub fn search_path() -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> =
            std::env::var_os("CHANOMA_CORES").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            dirs.push(home.join(".local/share/aizome/chanoma/cores"));
            dirs.push(home.join(".nix-profile/lib/retroarch/cores"));
        }
        for d in [
            "/run/current-system/sw/lib/retroarch/cores",
            "/run/current-system/sw/lib/libretro",
            "/usr/lib/libretro",
            "/usr/lib/x86_64-linux-gnu/libretro",
            "/usr/lib64/libretro",
        ] {
            dirs.push(PathBuf::from(d));
        }
        dirs
    }

    pub fn scan(dirs: &[PathBuf]) -> Cores {
        let mut found = Vec::new();
        for d in dirs {
            let Ok(entries) = std::fs::read_dir(d) else { continue };
            let mut here: Vec<(String, PathBuf)> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let stem = name.strip_suffix("_libretro.so")?.to_owned();
                    Some((stem, e.path()))
                })
                .collect();
            here.sort();
            for (stem, path) in here {
                if !found.iter().any(|(s, _)| *s == stem) {
                    found.push((stem, path));
                }
            }
        }
        Cores { found }
    }

    /// The best core here for `system`.
    pub fn for_system(&self, system: &System) -> Option<&Path> {
        system.cores.iter().find_map(|c| self.found.iter().find(|(s, _)| s == c).map(|(_, p)| p.as_path()))
    }
}

/// Everything found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    pub games: Vec<Game>,
    pub shelves: Vec<Shelf>,
    pub cores: Cores,
    /// Games found whose system has no core here.
    pub unplayable: usize,
    /// What each folder holds, in the settings' order.
    pub held: Vec<Held>,
}

/// What one of the folders holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub folder: PathBuf,
    /// Whether it's there: a drive's folder isn't while the drive is out.
    pub here: bool,
    pub games: usize,
    pub systems: usize,
    /// Games for systems with no core here.
    pub unplayable: usize,
    pub shows: usize,
    pub films: usize,
}

const ART_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// After a picture's game's name: none, or as Batocera's scraper names
/// them (the box first).
const ART_SUFFIXES: &[&str] = &["", "-thumb", "-boxart", "-box", "-cover", "-image"];
/// What's in a folder, as the shared index has it ([`aizome_index`]),
/// brought up to date first when `refresh` (only what changed is read
/// again): its files' paths, and its pictures' for box art. A folder
/// that isn't there (its drive is out) has nothing.
pub struct Read {
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
    pub pictures: HashSet<PathBuf>,
    /// The pictures' names without their extensions.
    pub picture_names: HashSet<std::ffi::OsString>,
}

/// How a look at the folders goes, for the screens that show it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Look {
    /// The folder being read, as the settings have it; none once they're
    /// all read and the games are being recognised.
    pub folder: Option<PathBuf>,
    /// Folders and files read in it so far.
    pub folders: usize,
    pub files: usize,
    /// Its own folders read, of how many: how far through it is. The index
    /// reads them one whole at a time, so this only grows. None where it
    /// has none.
    pub parts: Option<(usize, usize)>,
    /// Its read is over.
    pub over: bool,
}

/// Hears how a look goes.
pub type Progress<'a> = &'a dyn Fn(&Look);

/// `progress` hears how far the look at `root` has got.
pub fn read(index: &mut Index, root: &Path, refresh: bool, progress: Progress) -> Read {
    let given = root.to_path_buf();
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    // Its own folders, as the index will read them (not hidden ones).
    let parts = std::fs::read_dir(&root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().as_encoded_bytes().starts_with(b".")
                })
                .count()
        })
        .unwrap_or(0);
    let seen = std::cell::RefCell::new(HashSet::new());
    let tell = |r: &aizome_index::Report| {
        let over = r.took > std::time::Duration::ZERO;
        let mut seen = seen.borrow_mut();
        if let Some(part) = r.at.strip_prefix(&root).ok().and_then(|p| p.components().next()) {
            seen.insert(part.as_os_str().to_owned());
        }
        // The one being read isn't done yet.
        let done = if over { parts } else { seen.len().saturating_sub(1).min(parts) };
        progress(&Look {
            folder: Some(given.clone()),
            folders: r.folders,
            files: r.files,
            parts: (parts > 0).then_some((done, parts)),
            over,
        });
    };
    let how = Refresh { progress: Some(&tell), ..Refresh::default() };
    if refresh && let Err(e) = index.refresh(&root, &how) {
        log::warn!("chanoma: reading {}: {e}", root.display());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let mut pictures = HashSet::new();
    let mut picture_names = HashSet::new();
    for e in index.entries(&root).unwrap_or_default() {
        // Hidden files aren't games: a Mac leaves `._Game.sfc` (its
        // resource fork) beside every file it copies to a FAT drive.
        if e.kind != Kind::File || e.path.file_name().is_some_and(|n| n.as_encoded_bytes().starts_with(b".")) {
            continue;
        }
        if e.ext.as_deref().is_some_and(|x| ART_EXTENSIONS.contains(&x)) {
            pictures.insert(e.path.clone());
            picture_names.extend(e.path.file_stem().map(ToOwned::to_owned));
        }
        files.push(e.path);
    }
    files.sort();
    Read { root, files, pictures, picture_names }
}

/// A picture for `path`'s stem in its own folder or a pictures folder
/// beside it (`media`, `boxart`, `Named_Boxarts`, `images`, `covers`),
/// among the folder's `pictures` (nothing more is read for it).
fn art_for(path: &Path, root: &Path, read: &Read) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    // Only the names a picture has: most games have none, and every place
    // looked in for every name was most of a big collection's start.
    let suffixes: Vec<&str> = ART_SUFFIXES
        .iter()
        .copied()
        .filter(|s| read.picture_names.contains(std::ffi::OsStr::new(&format!("{stem}{s}"))))
        .collect();
    if suffixes.is_empty() {
        return None;
    }
    let dir = path.parent()?;
    let mut places = vec![dir.to_path_buf()];
    let mut d = Some(dir);
    while let Some(here) = d {
        for sub in ["media", "boxart", "Named_Boxarts", "images", "covers"] {
            places.push(here.join(sub));
        }
        if here == root {
            break;
        }
        d = here.parent();
    }
    for place in places {
        for suffix in &suffixes {
            for ext in ART_EXTENSIONS {
                let p = place.join(format!("{stem}{suffix}.{ext}"));
                if read.pictures.contains(&p) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Each of `folders` read (refreshed first when `refresh`), with its place
/// in `folders`: the deepest first, so a file in two (a folder added inside
/// another) is the nearer one's alone, and keeps the key it had there.
pub fn read_all(index: &mut Index, folders: &[PathBuf], refresh: bool, progress: Progress) -> Vec<(usize, Read)> {
    let depth = |p: &PathBuf| p.canonicalize().unwrap_or_else(|_| p.clone()).components().count();
    let mut order: Vec<usize> = (0..folders.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(depth(&folders[i])));
    let mut seen = HashSet::new();
    order
        .into_iter()
        .map(|i| {
            let mut r = read(index, &folders[i], refresh, progress);
            r.files.retain(|f| seen.insert(f.clone()));
            (i, r)
        })
        .collect()
}

/// Whether `path` is in a system's folder under `root` (or `root` is one):
/// a collection of games, whose videos are its games' clips, not films.
fn in_system_folder(path: &Path, root: &Path) -> bool {
    path.ancestors()
        .skip(1)
        .take_while(|d| d.starts_with(root))
        .any(|d| d.file_name().is_some_and(|n| system_named(&n.to_string_lossy()).is_some()))
}

/// What a disc's sheet may name: its tracks' images, or (a playlist) its
/// discs' sheets.
const DISC_PARTS: &[&str] = &["bin", "img", "iso", "chd", "cue", "ccd", "pbp", "ecm", "mdf", "m3u"];

/// The games in folders read, each counted in its folder's `held`.
fn games_of(reads: &[(usize, Read)], cores: &Cores, held: &mut [Held]) -> (Vec<Game>, usize) {
    let mut games = Vec::new();
    let mut unplayable = 0;
    for (i, read) in reads {
        let Read { root, files, pictures, .. } = read;
        // A disc's tracks are named by its sheet: they aren't games of
        // their own.
        let sheets: Vec<(&PathBuf, Vec<PathBuf>)> = files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue") || e.eq_ignore_ascii_case("m3u")))
            .map(|p| (p, referenced(p)))
            .collect();
        let listed: HashSet<&Path> = sheets.iter().flat_map(|(_, r)| r.iter().map(PathBuf::as_path)).collect();
        // What EmulationStation's lists say the games' pictures are.
        let listed_art: std::collections::HashMap<PathBuf, PathBuf> = files
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("gamelist.xml")))
            .flat_map(|p| crate::art::gamelist(p))
            .filter(|(_, picture)| pictures.contains(picture))
            .collect();
        let mut systems = HashSet::new();
        for path in files {
            if listed.contains(path.as_path()) {
                continue;
            }
            let Some(system) = system_of(path, root) else { continue };
            // A sheet known only by its extension is a disc's if it names
            // one: an album's names its music, a playlist its videos.
            if !in_system_folder(path, root)
                && let Some((_, parts)) = sheets.iter().find(|(s, _)| *s == path)
                && !parts.iter().any(|p| {
                    p.extension()
                        .is_some_and(|e| DISC_PARTS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
                })
            {
                continue;
            }
            if cores.for_system(system).is_none() {
                unplayable += 1;
                held[*i].unplayable += 1;
                continue;
            }
            held[*i].games += 1;
            systems.insert(system.id);
            let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
            games.push(Game {
                key: format!("{}/{}", system.id, rel),
                title: title_of(path),
                art: listed_art.get(path).cloned().or_else(|| art_for(path, root, read)),
                info: None,
                known_as: None,
                path: path.clone(),
                system,
            });
        }
        held[*i].systems = systems.len();
    }
    // Each title lowered once, not at each comparison.
    games.sort_by_cached_key(|g| (g.title.to_lowercase(), g.key.clone()));
    games.dedup_by(|a, b| a.key == b.key);
    (games, unplayable)
}

/// What each folder read holds, found by `games_of` and `shelves_of`.
fn held_of(folders: &[PathBuf], reads: &[(usize, Read)]) -> Vec<Held> {
    let mut held: Vec<Held> = folders.iter().map(|f| Held { folder: f.clone(), ..Held::default() }).collect();
    for (i, r) in reads {
        held[*i].here = r.root.is_dir();
    }
    held
}

/// The games under `folders`, as the index has them (refreshed first
/// when `refresh`).
pub fn scan_games(index: &mut Index, folders: &[PathBuf], cores: &Cores, refresh: bool) -> (Vec<Game>, usize) {
    let reads = read_all(index, folders, refresh, &|_| {});
    games_of(&reads, cores, &mut held_of(folders, &reads))
}

/// A folder of games found on a drive (or at home), for a grown-up to add.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundFolder {
    /// The drive's name ("Home" for home).
    pub place: String,
    pub folder: PathBuf,
    pub system: &'static System,
    pub games: usize,
}

/// The folders of games under `places` (named), as the files index has
/// them, brought up to date first when `refresh` (`progress` hears which
/// place and how many folders so far). A system's games are counted at
/// the folder named for it (`roms/psx`, holding a folder a disc); others
/// at their own folder, if it holds a few: a stray `.bin` isn't a
/// collection.
pub fn find_games(
    index: &mut Index,
    places: &[(String, PathBuf)],
    refresh: bool,
    progress: &dyn Fn(&str, usize),
) -> Vec<FoundFolder> {
    let mut out: Vec<FoundFolder> = Vec::new();
    for (name, path) in places {
        if refresh {
            let tell = |r: &aizome_index::Report| progress(name, r.folders);
            let how = Refresh { progress: Some(&tell), ..Refresh::default() };
            if let Err(e) = index.refresh(path, &how) {
                log::warn!("chanoma: looking for games in {}: {e}", path.display());
            }
        }
        let Read { root, files, .. } = read(index, path, false, &|_| {});
        let mut counts: std::collections::BTreeMap<(PathBuf, &'static str), (usize, bool)> = Default::default();
        for f in &files {
            let Some(system) = system_of(f, &root) else { continue };
            // The nearest folder named for the system, else the game's own.
            let named = f.ancestors().skip(1).take_while(|d| d.starts_with(&root) && *d != root).find(|d| {
                d.file_name().is_some_and(|n| system.folders.contains(&folder_key(&n.to_string_lossy()).as_str()))
            });
            let folder = named.or(f.parent()).unwrap_or(&root).to_path_buf();
            let c = counts.entry((folder, system.id)).or_insert((0, named.is_some()));
            c.0 += 1;
        }
        for ((folder, id), (games, named)) in counts {
            if !named && games < 3 {
                continue;
            }
            let Some(system) = SYSTEMS.iter().find(|s| s.id == id) else { continue };
            out.push(FoundFolder { place: name.clone(), folder, system, games });
        }
    }
    out.sort_by(|a, b| a.place.cmp(&b.place).then(a.system.name.cmp(b.system.name)).then(a.folder.cmp(&b.folder)));
    out
}

/// The files a cue sheet or playlist names, beside it.
fn referenced(sheet: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(sheet) else { return Vec::new() };
    let dir = sheet.parent().unwrap_or(Path::new("."));
    let m3u = sheet.extension().is_some_and(|e| e.eq_ignore_ascii_case("m3u"));
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if m3u {
                (!l.is_empty() && !l.starts_with('#')).then(|| dir.join(l))
            } else {
                let rest = l.strip_prefix("FILE ")?;
                let name = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split('"').next())
                    .unwrap_or_else(|| rest.rsplit_once(' ').map_or(rest, |(n, _)| n));
                Some(dir.join(name))
            }
        })
        .collect()
}

fn poster_in(dir: &Path, pictures: &HashSet<PathBuf>) -> Option<PathBuf> {
    for name in ["poster", "folder", "cover", "show"] {
        for ext in ART_EXTENSIONS {
            let p = dir.join(format!("{name}.{ext}"));
            if pictures.contains(&p) {
                return Some(p);
            }
        }
    }
    None
}

/// Folders that hold films and shows rather than being one: each video
/// straight in one is a film of its own.
const VIDEO_SHELVES: &[&str] = &[
    "movies",
    "movie",
    "films",
    "film",
    "videos",
    "video",
    "tv",
    "tvshows",
    "shows",
    "series",
    "cartoons",
    "anime",
    "kids",
    "documentaries",
    "clips",
    "media",
    "downloads",
    "homevideos",
];

/// A folder within a show, not a show of its own: a season, a disc,
/// extras.
fn within_a_show(name: &str) -> bool {
    let key = folder_key(name);
    if ["specials", "extras", "bonus", "featurettes"].contains(&key.as_str()) {
        return true;
    }
    ["season", "series", "disc", "volume", "vol", "s"]
        .iter()
        .any(|p| key.strip_prefix(p).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
}

/// The shelf a video under `root` is on: the folder it's in, past
/// seasons and discs (a show, or a film in a folder of its own), or
/// itself (a film), when that folder is `root` or holds films
/// (`Movies`).
fn shelf_of(video: &Path, root: &Path) -> PathBuf {
    let mut dir = video.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if within_a_show(&name) {
            dir = d.parent();
            continue;
        }
        if VIDEO_SHELVES.contains(&folder_key(&name).as_str()) {
            break;
        }
        return d.to_path_buf();
    }
    video.to_path_buf()
}

/// The films and shows in folders read, each counted in its folder's
/// `held`. Videos in a collection of games (`roms/snes/videos`) are its
/// games' clips, and left out.
fn shelves_of(reads: &[(usize, Read)], held: &mut [Held]) -> Vec<Shelf> {
    let mut shelves: Vec<Shelf> = Vec::new();
    for (i, read) in reads {
        let Read { root, files, pictures, .. } = read;
        let videos = files.iter().filter(|p| aizome_media::is_video(p) && !in_system_folder(p, root)).cloned();
        let mut groups: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> = Default::default();
        for v in videos {
            groups.entry(shelf_of(&v, root)).or_default().push(v);
        }
        for (key, mut vids) in groups {
            if vids.len() > 1 {
                held[*i].shows += 1;
            } else {
                held[*i].films += 1;
            }
            vids.sort_by(|a, b| natural(&a.to_string_lossy(), &b.to_string_lossy()));
            // A video by itself at the top is its own key; a folder's aren't.
            let folder = vids[0] != key;
            let title = if folder {
                clean(&key.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            } else {
                title_of(&key)
            };
            let poster = if folder {
                poster_in(&key, pictures).or_else(|| art_for(&vids[0], root, read))
            } else {
                art_for(&key, root, read)
            };
            let several = vids.len() > 1;
            let videos = vids
                .iter()
                .map(|p| {
                    let t = title_of(p);
                    Video { title: if several { episode_title(&title, &t) } else { t }, path: p.clone() }
                })
                .collect();
            shelves.push(Shelf { key: key.to_string_lossy().into_owned(), title, videos, poster });
        }
    }
    shelves.sort_by(|a, b| natural(&a.title.to_lowercase(), &b.title.to_lowercase()));
    shelves
}

/// Films and shows under `folders`: a folder of several videos is a show,
/// a lone video (or a folder of one) a film. As the index has them
/// (refreshed first when `refresh`).
pub fn scan_videos(index: &mut Index, folders: &[PathBuf], refresh: bool) -> Vec<Shelf> {
    let reads = read_all(index, folders, refresh, &|_| {});
    shelves_of(&reads, &mut held_of(folders, &reads))
}

/// What each of `folders` holds, counted as [`scan`] counts (refreshed
/// first when `refresh`).
pub fn held_in(index: &mut Index, folders: &[PathBuf], cores: &Cores, refresh: bool) -> Vec<Held> {
    let reads = read_all(index, folders, refresh, &|_| {});
    let mut held = held_of(folders, &reads);
    games_of(&reads, cores, &mut held);
    shelves_of(&reads, &mut held);
    held
}

/// An episode's name within its show: without the show's name before it,
/// and `S01E03` said as people say it.
pub fn episode_title(show: &str, title: &str) -> String {
    let rest = match title.get(..show.len()) {
        Some(head) if head.eq_ignore_ascii_case(show) => &title[show.len()..],
        _ => title,
    };
    let rest = rest.trim_start_matches([' ', '-', '.', '_', '·']).trim();
    // S01E03, perhaps followed by the episode's own name: the letters and
    // digits are ASCII, so byte offsets in `rest` are its characters'.
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let b = rest.as_bytes();
    if b.first().is_some_and(|c| c.eq_ignore_ascii_case(&b's')) {
        let s = digits(&rest[1..]);
        if s > 0 && b.get(1 + s).is_some_and(|c| c.eq_ignore_ascii_case(&b'e')) {
            let e = digits(&rest[2 + s..]);
            if e > 0 {
                let season: u32 = rest[1..1 + s].parse().unwrap_or(1);
                let episode: u32 = rest[2 + s..2 + s + e].parse().unwrap_or(1);
                let name = rest[2 + s + e..].trim_start_matches([' ', '-', '.', '_']).trim();
                let said = if season > 1 {
                    format!("Season {season}, Episode {episode}")
                } else {
                    format!("Episode {episode}")
                };
                return if name.is_empty() { said } else { format!("{said}: {name}") };
            }
        }
    }
    if rest.is_empty() { title.to_owned() } else { rest.to_owned() }
}

/// Order with numbers by their value: episode 2 before episode 10.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let (na, nb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = na.len().cmp(&nb.len()).then(na.cmp(nb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Everything to play and watch, as the shared index has it: brought up to
/// date first when `refresh`, the games found in the databases in
/// `metadata` (by what's in them too when `refresh`: [`crate::metadata`]),
/// those read before kept in `catalogs` for the next look.
pub fn scan(
    index: &mut Index,
    folders: &[PathBuf],
    core_dirs: &[PathBuf],
    metadata: Option<&Path>,
    catalogs: &mut crate::metadata::Catalogs,
    refresh: bool,
    progress: Progress,
) -> Library {
    let began = std::time::Instant::now();
    let cores = Cores::scan(core_dirs);
    // Whether a read took long enough to be shown going (an unfinished
    // folder was told of): then recognising the games is shown too.
    let shown = std::cell::Cell::new(false);
    let tell = |l: &Look| {
        shown.set(shown.get() || !l.over);
        progress(l);
    };
    let reads = read_all(index, folders, refresh, &tell);
    let read = began.elapsed();
    let mut held = held_of(folders, &reads);
    let (mut games, unplayable) = games_of(&reads, &cores, &mut held);
    log::debug!("chanoma: the folders read in {read:.0?}, their games recognised in {:.0?}", began.elapsed() - read);
    if shown.get() {
        progress(&Look::default());
    }
    if let Some(dir) = metadata {
        let began = std::time::Instant::now();
        let (by_name, by_content) = crate::metadata::describe(&mut games, index, dir, refresh, catalogs);
        log::info!(
            "chanoma: {by_name} of {} games found in the databases by name, {by_content} more by what's in them ({:.0?})",
            games.len(),
            began.elapsed()
        );
    }
    let shelves = shelves_of(&reads, &mut held);
    Library { games, shelves, cores, unplayable, held }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_lose_their_dump_tags() {
        assert_eq!(title_of(Path::new("Super Mario World (USA).sfc")), "Super Mario World");
        assert_eq!(title_of(Path::new("Tetris (World) (Rev 1) [!].gb")), "Tetris");
        assert_eq!(
            title_of(Path::new("Legend of Zelda, The - A Link to the Past (USA).sfc")),
            "The Legend of Zelda - A Link to the Past"
        );
        assert_eq!(title_of(Path::new("Kirby's_Dream_Land.gb")), "Kirby's Dream Land");
        assert_eq!(title_of(Path::new("(Proto).nes")), "(Proto)");
    }

    #[test]
    fn systems_by_folder_then_by_extension() {
        let root = Path::new("/games");
        assert_eq!(system_of(Path::new("/games/Super Nintendo/Kirby.sfc"), root).map(|s| s.id), Some("snes"));
        assert_eq!(system_of(Path::new("/games/whatever/Tetris.gb"), root).map(|s| s.id), Some("gb"));
        // Of the systems here only the PlayStation's games come as cue sheets.
        assert_eq!(system_of(Path::new("/games/misc/Disc.cue"), root).map(|s| s.id), Some("psx"));
        assert_eq!(system_of(Path::new("/games/GBC/Pokemon.gbc"), root).map(|s| s.id), Some("gbc"));
        // A system's folder holds only its own games.
        assert_eq!(system_of(Path::new("/games/snes/readme.txt"), root), None);
        assert_eq!(system_of(Path::new("/games/snes/Tetris.gb"), root), None);
    }

    #[test]
    fn episodes_are_named_as_people_say_them() {
        assert_eq!(episode_title("Bluey", "Bluey S01E03"), "Episode 3");
        assert_eq!(episode_title("Bluey", "Bluey - S02E10 - Camping"), "Season 2, Episode 10: Camping");
        assert_eq!(episode_title("Bluey", "Magic Xylophone"), "Magic Xylophone");
        assert_eq!(episode_title("Bluey", "Bluey"), "Bluey");
    }

    #[test]
    fn episodes_sort_by_number() {
        let mut v = vec!["Ep 10", "Ep 2", "Ep 1"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["Ep 1", "Ep 2", "Ep 10"]);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chanoma-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // As the files index gives paths back.
        d.canonicalize().unwrap()
    }

    /// A files index of the test's own.
    fn index(d: &Path) -> Index {
        Index::open_at(&d.join("index.sqlite")).unwrap()
    }

    #[test]
    fn a_folder_of_games_reads_with_its_art_and_discs() {
        let d = tmp("games");
        let cores_dir = d.join("cores");
        std::fs::create_dir_all(&cores_dir).unwrap();
        for core in ["snes9x", "gambatte", "swanstation"] {
            std::fs::write(cores_dir.join(format!("{core}_libretro.so")), b"").unwrap();
        }
        let games = d.join("Games");
        for f in [
            "SNES/Kirby (USA).sfc",
            // A Mac's resource fork, copied beside it: not a game.
            "SNES/._Kirby (USA).sfc",
            "SNES/media/Kirby (USA).png",
            "GB/Tetris.gb",
            "PS1/Disc.cue",
            "PS1/Disc (Track 1).bin",
            "N64/Mario.z64",
            "nes/Zelda.nes",
        ] {
            let p = games.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        std::fs::write(games.join("PS1/Disc.cue"), "FILE \"Disc (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n")
            .unwrap();
        let lib = scan(
            &mut index(&d),
            std::slice::from_ref(&games),
            &[cores_dir],
            None,
            &mut Default::default(),
            true,
            &|_| {},
        );
        let keys: Vec<&str> = lib.games.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, ["psx/PS1/Disc.cue", "snes/SNES/Kirby (USA).sfc", "gb/GB/Tetris.gb"]);
        assert_eq!(lib.games[1].title, "Kirby");
        assert_eq!(lib.games[1].art.as_deref(), Some(games.join("SNES/media/Kirby (USA).png").as_path()));
        // The NES game has no core here; the N64 one no system.
        assert_eq!(lib.unplayable, 1);
        assert_eq!(lib.games[0].file_key(), "psx~PS1~Disc.cue");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_of_episodes_is_a_show() {
        let d = tmp("videos");
        for f in [
            "Bluey/Bluey S01E10.mkv",
            "Bluey/Bluey S01E02.mkv",
            "Bluey/poster.jpg",
            "My Neighbour Totoro (1988).mp4",
            "notes.txt",
        ] {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let shelves = scan_videos(&mut index(&tmp("videos-index")), std::slice::from_ref(&d), true);
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].title, "Bluey");
        assert_eq!(shelves[0].videos.iter().map(|v| v.title.as_str()).collect::<Vec<_>>(), ["Episode 2", "Episode 10"]);
        assert_eq!(shelves[0].poster.as_deref(), Some(d.join("Bluey/poster.jpg").as_path()));
        assert_eq!(shelves[1].title, "My Neighbour Totoro");
        assert_eq!(shelves[1].videos.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
