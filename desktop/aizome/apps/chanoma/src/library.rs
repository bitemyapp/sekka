//! What's on the disk to play and watch: games in the games folders (each
//! known by its system's folder name or its file's extension), the cores
//! that play them, and films and shows in the video folders.
//!
//! Folders are read on a worker thread; the result is one [`Library`].

use std::path::{Path, PathBuf};

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

/// The system a file is for: its nearest folder that names one, else its
/// extension (when only one system uses it).
pub fn system_of(path: &Path, root: &Path) -> Option<&'static System> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || !d.starts_with(root) {
            break;
        }
        let key = d.file_name().map(|n| folder_key(&n.to_string_lossy())).unwrap_or_default();
        if let Some(s) = SYSTEMS.iter().find(|s| s.folders.contains(&key.as_str())) {
            // A system's folder holds its games; other files there aren't.
            return s.extensions.contains(&ext.as_str()).then_some(s);
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
    /// Box art found beside it.
    pub art: Option<PathBuf>,
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
    pub title: String,
    /// One for a film; a show's episodes in order.
    pub videos: Vec<Video>,
    pub poster: Option<PathBuf>,
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
}

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "m4v", "mov", "avi", "mpg", "mpeg", "ts", "ogv", "wmv"];
const ART_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
/// How deep folders are read.
const DEPTH: usize = 5;
/// How many files at most, so a mistaken folder (home) doesn't take all day.
const LIMIT: usize = 20_000;

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > DEPTH || out.len() >= LIMIT {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        let path = e.path();
        // Followed links to folders too: folders kept elsewhere.
        if t.is_dir() || (t.is_symlink() && path.is_dir()) {
            walk(&path, depth + 1, out);
        } else {
            out.push(path);
        }
        if out.len() >= LIMIT {
            return;
        }
    }
}

/// Whether `path` is a video Chanoma plays, by its extension.
pub fn is_video(path: &Path) -> bool {
    path.extension().is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
}

/// A picture for `path`'s stem in its own folder or a pictures folder
/// beside it (`media`, `boxart`, `Named_Boxarts`, `images`, `covers`).
fn art_for(path: &Path, root: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
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
        for ext in ART_EXTENSIONS {
            let p = place.join(format!("{stem}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The games under `folders`.
pub fn scan_games(folders: &[PathBuf], cores: &Cores) -> (Vec<Game>, usize) {
    let mut games = Vec::new();
    let mut unplayable = 0;
    for root in folders {
        let mut files = Vec::new();
        walk(root, 0, &mut files);
        // A disc's tracks are named by its sheet: they aren't games of
        // their own.
        let listed: std::collections::HashSet<PathBuf> = files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue") || e.eq_ignore_ascii_case("m3u")))
            .flat_map(|p| referenced(p))
            .collect();
        for path in files {
            if listed.contains(&path) {
                continue;
            }
            let Some(system) = system_of(&path, root) else { continue };
            if cores.for_system(system).is_none() {
                unplayable += 1;
                continue;
            }
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().into_owned();
            games.push(Game {
                key: format!("{}/{}", system.id, rel),
                title: title_of(&path),
                art: art_for(&path, root),
                path,
                system,
            });
        }
    }
    games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.key.cmp(&b.key)));
    games.dedup_by(|a, b| a.key == b.key);
    (games, unplayable)
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

fn poster_in(dir: &Path) -> Option<PathBuf> {
    for name in ["poster", "folder", "cover", "show"] {
        for ext in ART_EXTENSIONS {
            let p = dir.join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Films and shows under `folders`: a folder of several videos is a show,
/// a lone video (or a folder of one) a film.
pub fn scan_videos(folders: &[PathBuf]) -> Vec<Shelf> {
    let mut shelves: Vec<Shelf> = Vec::new();
    for root in folders {
        let mut files = Vec::new();
        walk(root, 0, &mut files);
        let videos: Vec<PathBuf> = files.into_iter().filter(|p| is_video(p)).collect();
        // Grouped by their folder just under the root.
        let mut groups: std::collections::BTreeMap<PathBuf, Vec<PathBuf>> = Default::default();
        for v in videos {
            let top = v.strip_prefix(root).ok().and_then(|r| r.components().next()).map(|c| root.join(c));
            let key = match top {
                Some(t) if t != v => t,
                _ => v.clone(),
            };
            groups.entry(key).or_default().push(v);
        }
        for (key, mut vids) in groups {
            vids.sort_by(|a, b| natural(&a.to_string_lossy(), &b.to_string_lossy()));
            let folder = key.is_dir();
            let title = if folder {
                clean(&key.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
            } else {
                title_of(&key)
            };
            let poster = if folder { poster_in(&key).or_else(|| art_for(&vids[0], root)) } else { art_for(&key, root) };
            let several = vids.len() > 1;
            let videos = vids
                .iter()
                .map(|p| {
                    let t = title_of(p);
                    Video { title: if several { episode_title(&title, &t) } else { t }, path: p.clone() }
                })
                .collect();
            shelves.push(Shelf { title, videos, poster });
        }
    }
    shelves.sort_by(|a, b| natural(&a.title.to_lowercase(), &b.title.to_lowercase()));
    shelves
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

/// Read everything: cores, games and videos.
pub fn scan(game_folders: &[PathBuf], video_folders: &[PathBuf], core_dirs: &[PathBuf]) -> Library {
    let cores = Cores::scan(core_dirs);
    let (games, unplayable) = scan_games(game_folders, &cores);
    let shelves = scan_videos(video_folders);
    Library { games, shelves, cores, unplayable }
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
        d
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
        let lib = scan(std::slice::from_ref(&games), &[], &[cores_dir]);
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
        let shelves = scan_videos(std::slice::from_ref(&d));
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].title, "Bluey");
        assert_eq!(shelves[0].videos.iter().map(|v| v.title.as_str()).collect::<Vec<_>>(), ["Episode 2", "Episode 10"]);
        assert_eq!(shelves[0].poster.as_deref(), Some(d.join("Bluey/poster.jpg").as_path()));
        assert_eq!(shelves[1].title, "My Neighbour Totoro");
        assert_eq!(shelves[1].videos.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
