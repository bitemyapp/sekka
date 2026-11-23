//! Whose a game's saves are.
//!
//! A game goes by its system and its path in its games folder
//! ([`crate::library::Game::key`]), and its saves by that name
//! ([`crate::library::Game::file_key`]). That is decided on purpose
//! (docs/chanoma.md, "Decided on purpose"): the same collection on another
//! drive is the same games, with their saves and what kids may play. It
//! leaves two different files able to go by one name: the same path in two
//! folders added separately, or a file replaced by another of its name.
//! Played under the first's name, the second would carry on from the
//! first's place and write over its save.
//!
//! So a name's saves say which file they were made with: a fingerprint of
//! it ([`Print`]), kept beside them. The first file played under a name has
//! the name's own files. A different file played under it keeps its own,
//! named by its fingerprint as well, and the first's are left as they were.
//! It starts from the first's battery save while it has none of its own (a
//! better copy of the same game keeps its progress that way; another game
//! finds a save that isn't its own, as from a dead battery, and starts
//! over), and never from the first's place: a core's state belongs to the
//! game that made it.
//!
//! Everything here reads the disk: a game's session calls it as it starts,
//! on its own thread.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How much of a file's start its fingerprint reads.
const HEAD: u64 = 1 << 20;
/// How many of a disc's files are read; the rest count by their sizes.
const READ_PARTS: usize = 4;
/// A fingerprint's length, in hexadecimal digits.
const DIGITS: usize = 16;

/// A fingerprint of a game's file: enough to tell two files of one name
/// apart, read from little of them.
///
/// - A zipped game's is its files' sizes and checksums as the zip lists
///   them, so the same game zipped again is the same.
/// - A disc's sheet (`.cue`, `.gdi`, `.ccd`, or the first disc of an
///   `.m3u`) stands for the files it names, not for its own words: two
///   games' `disc.cue` can be the same to the letter.
/// - Any other file's, and each of a sheet's, is its size and its first
///   mebibyte; past a sheet's fourth file, the size alone.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Print(String);

impl Print {
    /// The fingerprint of the game at `path`.
    pub fn of(path: &Path) -> std::io::Result<Print> {
        let mut hash = Sha256::new();
        hash.update(b"chanoma game 1\0");
        // One called a zip that isn't, or holds nothing, is its own bytes.
        let mut zipped =
            if extension(path) == "zip" { crate::zip::entries(path).unwrap_or_default() } else { Vec::new() };
        if zipped.is_empty() {
            for (i, part) in parts(path, 0).iter().enumerate() {
                add(&mut hash, part, i < READ_PARTS)?;
            }
        } else {
            zipped.sort_by_key(|e| (e.crc, e.size));
            for e in zipped {
                hash.update(e.size.to_le_bytes());
                hash.update(e.crc.to_le_bytes());
            }
        }
        let digest = format!("{:x}", hash.finalize());
        Ok(Print(digest[..DIGITS].to_owned()))
    }

    /// A fingerprint as it was written down, if that's what `text` is.
    pub fn parse(text: &str) -> Option<Print> {
        let ok = text.len() == DIGITS && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        ok.then(|| Print(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Print {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// A file's size, and its start when `read`, into `hash`.
fn add(hash: &mut Sha256, path: &Path, read: bool) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    hash.update(file.metadata()?.len().to_le_bytes());
    if read {
        let mut head = Vec::new();
        file.take(HEAD).read_to_end(&mut head)?;
        hash.update(&head);
    }
    Ok(())
}

/// The files a game is: those its sheet names that are there, or itself.
fn parts(path: &Path, depth: u8) -> Vec<PathBuf> {
    let named: Vec<PathBuf> = match extension(path).as_str() {
        // A playlist is its first disc: one added to it later is the same
        // game still.
        "m3u" if depth < 2 => {
            return match crate::library::referenced(path).first() {
                Some(first) => parts(first, depth + 1),
                None => vec![path.to_owned()],
            };
        }
        "cue" => crate::library::referenced(path),
        "gdi" => gdi_tracks(path),
        "ccd" => vec![path.with_extension("img")],
        _ => Vec::new(),
    };
    let there: Vec<PathBuf> = named.into_iter().filter(|p| p.is_file()).collect();
    if there.is_empty() { vec![path.to_owned()] } else { there }
}

/// A GD-ROM sheet's tracks: `3 45000 4 2352 track03.bin 0`, after a first
/// line that counts them.
fn gdi_tracks(sheet: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(sheet) else { return Vec::new() };
    let dir = sheet.parent().unwrap_or(Path::new("."));
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let quoted = l.split_once('"').and_then(|(_, rest)| rest.split('"').next());
            Some(dir.join(quoted.or_else(|| l.split_whitespace().nth(4))?))
        })
        .collect()
}

/// Whose a name's saves are, for a file played under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The name's own: the first file played under it, or one that can't
    /// be told from it.
    Own,
    /// Another file than the first played under the name: kept apart, by
    /// its fingerprint.
    Apart(Print),
}

/// `path` for a file kept apart: `name-fingerprint.srm` beside `name.srm`.
pub fn apart(path: &Path, print: &Print) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = match path.extension() {
        Some(ext) => format!("{stem}-{print}.{}", ext.to_string_lossy()),
        None => format!("{stem}-{print}"),
    };
    path.with_file_name(name)
}

/// Where a name's saves say which file they were made with: beside its
/// battery save.
fn record(save_ram: &Path) -> PathBuf {
    save_ram.with_extension("fingerprint")
}

/// A game's saves: where each is read as it starts, and written after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub whose: Whose,
    /// The battery save, as it's written.
    pub save_ram: PathBuf,
    /// The battery save to start from: its own, an older version's under
    /// another name, or (kept apart, with none yet) the name's own.
    pub save_ram_from: PathBuf,
    /// Where it was left, as it's written.
    pub state: PathBuf,
    /// Where it was left, to carry on from: its own, or an older
    /// version's. Never another file's.
    pub state_from: PathBuf,
    /// A picture of where it was left.
    pub picture: PathBuf,
}

impl Kept {
    /// The saves of the game at `game`, among those of its name: the
    /// name's own are `save_ram`, `state` and `picture`, and older
    /// versions kept them under `legacy_key`.
    ///
    /// The first file played under a name is written down as the one its
    /// saves were made with. Saves from before anything was written down
    /// are the first file's that is played under the name from now on.
    pub fn of(game: &Path, save_ram: &Path, state: &Path, picture: &Path, legacy_key: Option<&str>) -> Kept {
        let whose = whose(game, &record(save_ram));
        let older = |path: &Path| crate::store::existing_game_file(path, legacy_key);
        match &whose {
            Whose::Own => Kept {
                save_ram: save_ram.to_owned(),
                save_ram_from: older(save_ram),
                state: state.to_owned(),
                state_from: older(state),
                picture: picture.to_owned(),
                whose,
            },
            Whose::Apart(print) => {
                let own = apart(save_ram, print);
                Kept {
                    save_ram_from: if own.is_file() { own.clone() } else { older(save_ram) },
                    save_ram: own,
                    state: apart(state, print),
                    state_from: apart(state, print),
                    picture: apart(picture, print),
                    whose,
                }
            }
        }
    }
}

/// Whose the saves `record` speaks for are, for the game at `game`. With
/// nothing written down yet, this game is, from now on.
fn whose(game: &Path, record: &Path) -> Whose {
    let print = match Print::of(game) {
        Ok(print) => print,
        Err(e) => {
            // Its core won't read it either; nothing to tell it apart by.
            log::warn!("chanoma: reading {} for its fingerprint: {e}", game.display());
            return Whose::Own;
        }
    };
    match std::fs::read_to_string(record) {
        Ok(text) => match Print::parse(text.trim()) {
            Some(first) if first != print => Whose::Apart(print),
            Some(_) => Whose::Own,
            None => {
                log::warn!("chanoma: {} doesn't hold a fingerprint", record.display());
                Whose::Own
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Err(e) = crate::store::write_bytes(record, format!("{print}\n").as_bytes()) {
                log::warn!("chanoma: writing {}: {e}", record.display());
            }
            Whose::Own
        }
        Err(e) => {
            log::warn!("chanoma: reading {}: {e}", record.display());
            Whose::Own
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chanoma-saves-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn file(path: &Path, bytes: &[u8]) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
        path.to_owned()
    }

    /// A zip holding `name`, deflated or stored.
    fn zip(path: &Path, name: &str, bytes: &[u8], deflate: bool) -> PathBuf {
        file(path, &crate::zip::tests::zip(&[(name, bytes)], deflate))
    }

    #[test]
    fn a_fingerprint_tells_files_apart_and_knows_the_same_one_anywhere() {
        let t = temp("print");
        let a = file(&t.join("home/gb/Demo.gb"), b"one game");
        let same = file(&t.join("stick/gb/Demo.gb"), b"one game");
        let other = file(&t.join("other/gb/Demo.gb"), b"another!");
        let print = |p: &Path| Print::of(p).unwrap();
        assert_eq!(print(&a), print(&same), "the same file in another folder");
        assert_ne!(print(&a), print(&other), "another file of the same name and size");
        assert_eq!(Print::parse(print(&a).as_str()), Some(print(&a)));
        assert_eq!(Print::parse("not one"), None);
        assert!(Print::of(&t.join("gone.gb")).is_err());

        // Past the first mebibyte only the size counts.
        let mut big = vec![7u8; HEAD as usize + 16];
        let one = file(&t.join("a/Big.n64"), &big);
        *big.last_mut().unwrap() = 8;
        assert_eq!(print(&one), print(&file(&t.join("b/Big.n64"), &big)));
        big.push(0);
        assert_ne!(print(&one), print(&file(&t.join("c/Big.n64"), &big)));
    }

    #[test]
    fn a_zipped_game_is_what_is_in_it() {
        let t = temp("zip");
        let print = |p: &Path| Print::of(p).unwrap();
        let a = zip(&t.join("a/Demo.zip"), "Demo.gb", b"one game", false);
        // Zipped again, another way and under another name.
        let again = zip(&t.join("b/Demo.zip"), "Demo (USA).gb", b"one game", true);
        assert_ne!(std::fs::read(&a).unwrap().len(), std::fs::read(&again).unwrap().len());
        let other = zip(&t.join("c/Demo.zip"), "Demo.gb", b"another!", false);
        assert_eq!(print(&a), print(&again));
        assert_ne!(print(&a), print(&other));
        // Called a zip and not one: its bytes.
        let not = file(&t.join("d/Demo.zip"), b"one game");
        assert_eq!(print(&not), print(&file(&t.join("e/Demo.zip"), b"one game")));
    }

    #[test]
    fn a_discs_sheet_is_the_files_it_names() {
        let t = temp("sheet");
        let print = |p: &Path| Print::of(p).unwrap();
        let cue = b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n";
        let disc = |dir: &str, track: &[u8]| {
            file(&t.join(dir).join("disc.bin"), track);
            file(&t.join(dir).join("disc.cue"), cue)
        };
        // Two games' sheets, the same to the letter.
        let (a, b) = (disc("Crazy Taxi", b"a taxi"), disc("Sonic", b"a hedgehog, longer"));
        assert_ne!(print(&a), print(&b));
        assert_eq!(print(&a), print(&disc("copy", b"a taxi")));
        // The sheet written again another way, for the same track.
        let spaced = file(&t.join("Crazy Taxi/spaced.cue"), b"FILE disc.bin BINARY\r\n  TRACK 01 MODE2/2352\r\n");
        assert_eq!(print(&a), print(&spaced));
        // A playlist is its first disc, whatever comes after it.
        let m3u = file(&t.join("Crazy Taxi/game.m3u"), b"# discs\ndisc.cue\n");
        let longer = file(&t.join("Crazy Taxi/longer.m3u"), b"disc.cue\ndisc2.cue\n");
        assert_eq!(print(&m3u), print(&a));
        assert_eq!(print(&longer), print(&a));
        // A GD-ROM's sheet, and a CloneCD's image.
        let gdi = b"2\n1 0 4 2352 track01.bin 0\n2 45000 4 2352 \"track 03.bin\" 0\n";
        let gd = |dir: &str, high: &[u8]| {
            file(&t.join(dir).join("track01.bin"), b"the same warning");
            file(&t.join(dir).join("track 03.bin"), high);
            file(&t.join(dir).join("disc.gdi"), gdi)
        };
        assert_ne!(print(&gd("gd-a", b"one game")), print(&gd("gd-b", b"another!")));
        let ccd = |dir: &str, image: &[u8]| {
            file(&t.join(dir).join("disc.img"), image);
            file(&t.join(dir).join("disc.ccd"), b"[CloneCD]\n")
        };
        assert_ne!(print(&ccd("ccd-a", b"one game")), print(&ccd("ccd-b", b"another!")));
        // A sheet whose files aren't there is its own words.
        let alone = file(&t.join("alone/disc.cue"), cue);
        assert_eq!(print(&alone), print(&file(&t.join("alone2/disc.cue"), cue)));
    }

    #[test]
    fn another_file_of_the_name_keeps_its_own_saves() {
        let t = temp("kept");
        let first = file(&t.join("home/gb/Demo.gb"), b"one game");
        let second = file(&t.join("stick/gb/Demo.gb"), b"another!");
        let (ram, state, picture) = (t.join("saves/abc.srm"), t.join("states/abc.state"), t.join("states/abc.png"));
        let kept = |game: &Path| Kept::of(game, &ram, &state, &picture, Some("gb~Demo.gb"));

        // The first played has the name's own files, and is written down.
        let own = kept(&first);
        assert_eq!(own.whose, Whose::Own);
        assert_eq!((&own.save_ram, &own.state, &own.picture), (&ram, &state, &picture));
        assert_eq!((&own.save_ram_from, &own.state_from), (&ram, &state));
        let written = std::fs::read_to_string(t.join("saves/abc.fingerprint")).unwrap();
        assert_eq!(Print::parse(written.trim()), Some(Print::of(&first).unwrap()));
        // What an older version kept is read until it has written its own.
        file(&t.join("saves/gb~Demo.gb.srm"), b"old");
        file(&t.join("states/gb~Demo.gb.state"), b"old");
        let own = kept(&first);
        assert_eq!(own.save_ram_from, t.join("saves/gb~Demo.gb.srm"));
        assert_eq!(own.state_from, t.join("states/gb~Demo.gb.state"));
        file(&ram, b"first's save");
        file(&state, b"first's place");
        assert_eq!(kept(&first), Kept { save_ram_from: ram.clone(), state_from: state.clone(), ..own });
        // The same file from another folder is the same game.
        let copy = file(&t.join("copy/gb/Demo.gb"), b"one game");
        assert_eq!(kept(&copy).whose, Whose::Own);

        // Another file of the name: its own files, the first's left alone.
        let print = Print::of(&second).unwrap();
        let other = kept(&second);
        assert_eq!(other.whose, Whose::Apart(print.clone()));
        assert_eq!(other.save_ram, t.join(format!("saves/abc-{print}.srm")));
        assert_eq!(other.state, t.join(format!("states/abc-{print}.state")));
        assert_eq!(other.picture, t.join(format!("states/abc-{print}.png")));
        // It starts from the first's battery save, never from its place.
        assert_eq!(other.save_ram_from, ram);
        assert_eq!(other.state_from, other.state);
        file(&other.save_ram, b"second's save");
        assert_eq!(kept(&second).save_ram_from, other.save_ram);
        // The first is still the first.
        assert_eq!(kept(&first).whose, Whose::Own);
        assert_eq!(std::fs::read(&ram).unwrap(), b"first's save");

        // A game that can't be read, or a record that isn't one: as before.
        assert_eq!(kept(&t.join("gone.gb")).whose, Whose::Own);
        file(&t.join("saves/abc.fingerprint"), b"scribble");
        assert_eq!(kept(&second).whose, Whose::Own);
    }
}
