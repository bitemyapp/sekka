//! Console BIOS images a grown-up keeps beside their games: a `bios`
//! folder by the games folder, as Batocera and RetroBat keep theirs. Of
//! its files only those Chanoma's cores look for are linked into the
//! cores' system folder, each where its core looks (Flycast's under `dc`,
//! one PlayStation 2 BIOS under `pcsx2/bios`), whatever the case of their
//! names; the folder itself is never the cores' to write in. A BIOS the
//! grown-up put in the system folder themselves stays theirs.
//!
//! Which core plays a game goes by them too: a system's best core when
//! the BIOS it can't play without is there ([`NEEDS`]), else the next.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The images Chanoma's cores look for in their system folder, by their
/// place there, and the names collections keep them under (lower case).
const WANTED: &[(&str, &[&str])] = &[
    // Beetle Saturn's (Japan's, the rest of the world's, two cartridges'),
    // Yabause's.
    ("sega_101.bin", &["sega_101.bin"]),
    ("mpr-17933.bin", &["mpr-17933.bin"]),
    ("mpr-18811-mx.ic1", &["mpr-18811-mx.ic1"]),
    ("mpr-19367-mx.ic1", &["mpr-19367-mx.ic1"]),
    ("saturn_bios.bin", &["saturn_bios.bin"]),
    // Flycast's, in a folder of their own.
    ("dc/dc_boot.bin", &["dc/dc_boot.bin", "dc_boot.bin"]),
    ("dc/dc_flash.bin", &["dc/dc_flash.bin", "dc_flash.bin"]),
    // The PlayStation's: PCSX-ReARMed plays better with one.
    ("scph5500.bin", &["scph5500.bin"]),
    ("scph5501.bin", &["scph5501.bin", "scph-5501.bin"]),
    ("scph5502.bin", &["scph5502.bin"]),
    ("scph1001.bin", &["scph1001.bin"]),
    ("scph7001.bin", &["scph7001.bin"]),
    ("scph101.bin", &["scph101.bin"]),
    ("gba_bios.bin", &["gba_bios.bin"]),
];

/// Where PCSX2 looks for a PlayStation 2 BIOS, and where collections keep
/// them.
const PS2_BIOS: &str = "pcsx2/bios";
const PS2_FOLDERS: &[&str] = &["pcsx2/bios", "ps2", ""];
/// A PlayStation 2 BIOS image's size: 4 MiB.
const PS2_BIOS_SIZE: u64 = 4 << 20;
/// Models whose BIOS plays most games (North America's, then others').
const PS2_MODELS: &[&str] = &["70012", "39001", "77001", "50001", "30001", "90001"];

/// Cores that play nothing without their console's BIOS, and the images
/// (any of them, by their place in the system folder) they take.
pub const NEEDS: &[(&str, &[&str])] =
    &[("mednafen_saturn", &["sega_101.bin", "mpr-17933.bin"]), ("pcsx2", &["pcsx2/bios/"])];

/// The `bios` folders near `game`: in its folder or one above it, up to
/// beside the games folder it's in (`folders`, the settings').
pub fn near(game: &Path, folders: &[PathBuf]) -> Vec<PathBuf> {
    let Some(root) = folders.iter().filter(|f| game.starts_with(f)).max_by_key(|f| f.components().count()) else {
        return Vec::new();
    };
    let top = root.parent().unwrap_or(root);
    game.ancestors()
        .skip(1)
        .take_while(|d| d.starts_with(top))
        .flat_map(|d| [d.join("bios"), d.join("BIOS")])
        .filter(|b| b.is_dir())
        .fold(Vec::new(), |mut out, b| {
            // A drive that ignores case finds one folder twice.
            if !out.iter().any(|o: &PathBuf| same_file(o, &b)) {
                out.push(b);
            }
            out
        })
}

fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => a == b,
    }
}

/// A folder's files by their names in lower case.
fn files_in(dir: &Path) -> HashMap<String, PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
                .map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path()))
                .collect()
        })
        .unwrap_or_default()
}

/// What of `found` (the `bios` folders near a game) goes where in the
/// cores' system folder.
pub fn wanted(found: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    let mut out: Vec<(PathBuf, PathBuf)> = Vec::new();
    for dir in found {
        let listed: HashMap<&str, HashMap<String, PathBuf>> =
            ["", "dc"].iter().chain(PS2_FOLDERS).map(|sub| (*sub, files_in(&dir.join(sub)))).collect();
        for (place, names) in WANTED {
            if out.iter().any(|(p, _)| p == Path::new(place)) {
                continue;
            }
            let hit = names.iter().find_map(|n| {
                let (sub, name) = n.rsplit_once('/').unwrap_or(("", n));
                listed.get(sub)?.get(name).cloned()
            });
            if let Some(from) = hit {
                out.push((PathBuf::from(place), from));
            }
        }
        if !out.iter().any(|(p, _)| p.starts_with(PS2_BIOS)) {
            let mut ps2: Vec<PathBuf> = PS2_FOLDERS
                .iter()
                .filter_map(|sub| listed.get(sub))
                .flat_map(|files| files.values().cloned())
                .filter(|p| {
                    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin"))
                        && std::fs::metadata(p).is_ok_and(|m| m.len() == PS2_BIOS_SIZE)
                })
                .collect();
            let rank = |p: &PathBuf| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (PS2_MODELS.iter().position(|m| name.contains(m)).unwrap_or(PS2_MODELS.len()), name)
            };
            ps2.sort_by_key(rank);
            if let Some(best) = ps2.into_iter().next() {
                let name = best.file_name().unwrap_or_default().to_os_string();
                out.push((Path::new(PS2_BIOS).join(name), best));
            }
        }
    }
    out
}

/// Whether `place` (a file in the cores' system folder, or a folder of
/// them, ending `/`) is there: the grown-up's own in `system`, a
/// package's in `offered`, or one of `found`'s.
pub fn has(place: &str, system: &Path, offered: &[PathBuf], found: &[(PathBuf, PathBuf)]) -> bool {
    let here = |dir: &Path| {
        let p = dir.join(place.trim_end_matches('/'));
        if place.ends_with('/') {
            std::fs::read_dir(&p).is_ok_and(|mut rd| rd.any(|e| e.is_ok_and(|e| e.path().exists())))
        } else {
            p.exists()
        }
    };
    here(system)
        || offered.iter().any(|d| here(d))
        || found.iter().any(|(p, _)| p.starts_with(place.trim_end_matches('/')))
}

/// Link `found`'s images into `system`: a file of the grown-up's own there
/// stays; links to what's gone (a drive taken out) go.
pub fn offer(found: &[(PathBuf, PathBuf)], system: &Path) {
    for sub in ["", "dc", PS2_BIOS] {
        if let Ok(rd) = std::fs::read_dir(system.join(sub)) {
            for e in rd.flatten() {
                if e.file_type().is_ok_and(|t| t.is_symlink()) && !e.path().exists() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    for (place, from) in found {
        let to = system.join(place);
        if let Some(dir) = to.parent() {
            // A folder of links, not a link to the drive's folder.
            if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
                let _ = std::fs::remove_file(dir);
            }
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::symlink_metadata(&to) {
            Ok(m) if !m.file_type().is_symlink() => continue,
            Ok(_) if std::fs::read_link(&to).is_ok_and(|t| t == *from) => continue,
            Ok(_) => {
                let _ = std::fs::remove_file(&to);
            }
            Err(_) => {}
        }
        if let Err(e) = std::os::unix::fs::symlink(from, &to) {
            log::warn!("chanoma: offering {} to the cores: {e}", from.display());
        }
    }
}

/// Of `cores` (a system's, best first), the first whose BIOS `has` says is
/// there.
pub fn playable<'a>(cores: impl IntoIterator<Item = &'a str>, has: impl Fn(&str) -> bool) -> Vec<&'a str> {
    cores
        .into_iter()
        .filter(|c| NEEDS.iter().find(|(core, _)| core == c).is_none_or(|(_, any)| any.iter().any(|p| has(p))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chanoma-bios-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_collections_bios_folder_is_offered_as_the_cores_look() {
        let d = temp("found");
        let drive = d.join("Extreme Pro");
        let games = drive.join("roms");
        std::fs::create_dir_all(games.join("dreamcast")).unwrap();
        let bios = drive.join("bios");
        std::fs::create_dir_all(bios.join("ps2")).unwrap();
        for (f, size) in [
            ("dc_boot.bin", 16),
            ("mpr-17933.bin", 16),
            ("SCPH-5501.BIN", 16),
            ("SCPH-70004_BIOS_V12_PAL_200.BIN", PS2_BIOS_SIZE),
            ("ps2/SCPH-70012.bin", PS2_BIOS_SIZE),
            // A PlayStation's, not a PlayStation 2's.
            ("scph1001.bin", 512 << 10),
            ("readme.txt", 5),
        ] {
            std::fs::File::create(bios.join(f)).unwrap().set_len(size).unwrap();
        }
        let game = games.join("dreamcast/Crazy Taxi (USA).chd");
        let found = near(&game, std::slice::from_ref(&games));
        assert_eq!(found, std::slice::from_ref(&bios));
        let mut w = wanted(&found);
        w.sort();
        let places: Vec<String> = w.iter().map(|(p, _)| p.to_string_lossy().into_owned()).collect();
        assert_eq!(
            places,
            ["dc/dc_boot.bin", "mpr-17933.bin", "pcsx2/bios/SCPH-70012.bin", "scph1001.bin", "scph5501.bin"]
        );
        let system = d.join("system");
        std::fs::create_dir_all(&system).unwrap();
        // The grown-up's own Saturn BIOS stays.
        std::fs::write(system.join("mpr-17933.bin"), b"theirs").unwrap();
        offer(&w, &system);
        assert_eq!(std::fs::read_link(system.join("dc/dc_boot.bin")).unwrap(), bios.join("dc_boot.bin"));
        assert_eq!(std::fs::read(system.join("mpr-17933.bin")).unwrap(), b"theirs");
        assert!(std::fs::symlink_metadata(system.join("pcsx2/bios")).unwrap().is_dir());
        // The best core is the one whose BIOS is there.
        let there = |p: &str| has(p, &system, &[], &w);
        assert_eq!(playable(["pcsx2", "play"], there), ["pcsx2", "play"]);
        assert_eq!(playable(["mednafen_saturn", "yabause"], there), ["mednafen_saturn", "yabause"]);
        assert_eq!(playable(["pcsx2", "play"], |_| false), ["play"]);
        // The drive taken out: its links go.
        std::fs::remove_dir_all(&drive).unwrap();
        offer(&[], &system);
        assert!(std::fs::symlink_metadata(system.join("dc/dc_boot.bin")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
