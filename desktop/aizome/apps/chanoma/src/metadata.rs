//! What's known of a game beyond its name: the year it came out, its
//! genre, how many can play it and who made it, from libretro-database
//! (CC BY-SA 4.0), the databases RetroArch scans with, which the module
//! installs beside Chanoma (`$CHANOMA_METADATA`, `nix/chanoma-metadata.nix`).
//!
//! A game is found in them by its file's name, which for a set named as
//! No-Intro and Redump name them is the database's own. A file renamed
//! is found by what's in it, worked out once and kept in the files index
//! beside the file (a fact, forgotten when the file changes): a zip's
//! CRC from its directory, a cartridge's CRC (an NES one's without its
//! header too), or a disc's serial, from its `SYSTEM.CNF` or its system
//! area ([`crate::disc`]).
//!
//! The databases are RetroArch's `.rdb` files: `RARCHDB\0`, an offset,
//! then one MessagePack map a game until a nil.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use aizome_index::Index;

use crate::library::Game;

/// The fact kept in the files index for a renamed cartridge.
const FACT_CRC: &str = "chanoma.crc";
/// Cartridges larger than this aren't read for their CRC.
const READ_AT_MOST: u64 = 64 << 20;

/// What a database says of a game.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Info {
    pub year: Option<u16>,
    pub genre: Option<String>,
    /// How many can play at once.
    pub players: Option<u8>,
    pub developer: Option<String>,
}

impl Info {
    /// "1996 · Action · 2 players".
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.extend(self.year.map(|y| y.to_string()));
        parts.extend(self.genre.clone());
        parts.extend(self.players.map(|n| if n == 1 { "1 player".to_owned() } else { format!("{n} players") }));
        parts.join(" · ")
    }

    fn fill(&mut self, other: &Info) {
        self.year = self.year.or(other.year);
        self.genre = self.genre.take().or_else(|| other.genre.clone());
        self.players = self.players.or(other.players);
        self.developer = self.developer.take().or_else(|| other.developer.clone());
    }

    fn is_empty(&self) -> bool {
        *self == Info::default()
    }
}

/// The databases a system's games are in, by their names in
/// libretro-database's `rdb` folder.
pub fn databases(system: &str) -> &'static [&'static str] {
    match system {
        "nes" => &["Nintendo - Nintendo Entertainment System"],
        "snes" => &["Nintendo - Super Nintendo Entertainment System"],
        "n64" => &["Nintendo - Nintendo 64"],
        "gb" => &["Nintendo - Game Boy"],
        "gbc" => &["Nintendo - Game Boy Color"],
        "gba" => &["Nintendo - Game Boy Advance"],
        "genesis" => &["Sega - Mega Drive - Genesis"],
        "sms" => &["Sega - Master System - Mark III"],
        "gg" => &["Sega - Game Gear"],
        "pce" => &["NEC - PC Engine - TurboGrafx 16", "NEC - PC Engine SuperGrafx"],
        "psx" => &["Sony - PlayStation"],
        "ps2" => &["Sony - PlayStation 2"],
        "saturn" => &["Sega - Saturn"],
        "dreamcast" => &["Sega - Dreamcast"],
        "atari2600" => &["Atari - 2600"],
        "lynx" => &["Atari - Lynx"],
        "ngp" => &["SNK - Neo Geo Pocket", "SNK - Neo Geo Pocket Color"],
        "wswan" => &["Bandai - WonderSwan", "Bandai - WonderSwan Color"],
        _ => &[],
    }
}

/// Where the databases are: `$CHANOMA_METADATA`.
pub fn dir() -> Option<PathBuf> {
    std::env::var_os("CHANOMA_METADATA").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// A system's databases, read.
#[derive(Debug, Default)]
pub struct Catalog {
    /// In order, for the names that only add tags to a file's.
    names: BTreeMap<String, Info>,
    crcs: HashMap<u32, String>,
    serials: HashMap<String, String>,
}

/// A serial as compared: letters and digits, upper case (`SCUS-94900`,
/// `SCUS_949.00` and `scus94900` are one), without the `MK-` Sega put
/// before its own: a Dreamcast disc says `MK-51035` where RetroArch's
/// database has `51035`, though it keeps `MK-51136-50`.
fn serial_key(s: &str) -> String {
    let key: String = s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    match key.strip_prefix("MK") {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest.to_owned(),
        _ => key,
    }
}

impl Catalog {
    /// `system`'s databases in `dir`; those missing are left out.
    pub fn load(dir: &Path, system: &str) -> Catalog {
        let mut c = Catalog::default();
        for name in databases(system) {
            let path = dir.join(format!("{name}.rdb"));
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if let Err(e) = c.add(&bytes) {
                        log::warn!("chanoma: {}: {e}", path.display());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!("chanoma: {}: {e}", path.display()),
            }
        }
        c
    }

    /// The games in a database's bytes. A game named more than once (a
    /// release per disc or revision) takes what any of them says.
    pub fn add(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.len() < 16 || &bytes[..7] != b"RARCHDB" {
            return Err("not a RetroArch database");
        }
        let mut r = Reader { b: bytes, i: 16 };
        loop {
            match r.next().ok_or("cut short")? {
                Value::Nil => return Ok(()),
                Value::Map(n) => {
                    let Record { name, info, crc, serial } = record(&mut r, n).ok_or("cut short")?;
                    let Some(name) = name else { continue };
                    if let Some(crc) = crc {
                        self.crcs.entry(crc).or_insert_with(|| name.clone());
                    }
                    if let Some(s) = serial.map(|s| serial_key(&s)).filter(|s| !s.is_empty()) {
                        self.serials.entry(s).or_insert_with(|| name.clone());
                    }
                    self.names.entry(name).or_default().fill(&info);
                }
                _ => return Err("not a database of games"),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn by_name(&self, name: &str) -> Option<&Info> {
        self.names.get(name).filter(|i| !i.is_empty())
    }

    /// Whether a game of this name is in the databases, said of or not.
    fn knows(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    /// The name that is a file's `stem` (one with a tag, its region, as
    /// No-Intro's and Redump's are) with more tags after it, as a database
    /// adds a release's languages: `Crazy Taxi (USA) (En,Ja)` for the
    /// Dreamcast's `Crazy Taxi (USA)`. Of several, the one with fewest.
    fn name_adding_tags(&self, stem: &str) -> Option<&str> {
        if !stem.ends_with(')') {
            return None;
        }
        let start = format!("{stem} (");
        self.names
            .range::<str, _>((std::ops::Bound::Included(start.as_str()), std::ops::Bound::Unbounded))
            .map(|(n, _)| n.as_str())
            .take_while(|n| n.starts_with(&start))
            .filter(|n| only_tags(&n[stem.len()..]))
            .min_by_key(|n| (n.matches(" (").count(), n.len()))
    }

    /// The name of the game with this CRC, or this disc serial.
    fn name_by_crc(&self, crc: u32) -> Option<&str> {
        self.crcs.get(&crc).map(String::as_str)
    }

    fn name_by_serial(&self, serial: &str) -> Option<&str> {
        self.serials.get(&serial_key(serial)).map(String::as_str)
    }
}

/// Whether `s` is only tags, each ` (…)`.
fn only_tags(mut s: &str) -> bool {
    while let Some(rest) = s.strip_prefix(" (") {
        let Some(end) = rest.find(')') else { return false };
        s = &rest[end + 1..];
    }
    s.is_empty()
}

/// A database's game.
#[derive(Default)]
struct Record {
    name: Option<String>,
    info: Info,
    crc: Option<u32>,
    serial: Option<String>,
}

/// The game in a map of `n` pairs.
fn record(r: &mut Reader, n: usize) -> Option<Record> {
    let mut g = Record::default();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    for _ in 0..n {
        let Value::Str(key) = r.next()? else { return None };
        let v = r.next()?;
        match (key, v) {
            (b"name", Value::Str(s)) => g.name = Some(text(s)),
            (b"genre", Value::Str(s)) => g.info.genre = Some(text(s)),
            (b"developer", Value::Str(s)) => g.info.developer = Some(text(s)),
            (b"releaseyear", Value::Int(y)) => g.info.year = u16::try_from(y).ok().filter(|y| *y > 1950),
            (b"users", Value::Int(u)) => g.info.players = u8::try_from(u).ok().filter(|u| *u > 0),
            (b"crc", Value::Str(s)) if s.len() == 4 => g.crc = Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]])),
            (b"serial", Value::Str(s)) => g.serial = Some(text(s)),
            (_, Value::Map(m)) => r.skip(m * 2)?,
            (_, Value::Array(m)) => r.skip(m)?,
            _ => {}
        }
    }
    Some(g)
}

/// A MessagePack value, as the databases use them: strings and binaries
/// alike are bytes.
#[derive(Debug, PartialEq)]
enum Value<'a> {
    Nil,
    Int(i64),
    Str(&'a [u8]),
    Map(usize),
    Array(usize),
    Other,
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }

    fn uint(&mut self, n: usize) -> Option<u64> {
        Some(self.take(n)?.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b)))
    }

    fn next(&mut self) -> Option<Value<'a>> {
        let t = *self.take(1)?.first()?;
        Some(match t {
            0x00..=0x7f => Value::Int(i64::from(t)),
            0x80..=0x8f => Value::Map(usize::from(t & 0x0f)),
            0x90..=0x9f => Value::Array(usize::from(t & 0x0f)),
            0xa0..=0xbf => Value::Str(self.take(usize::from(t & 0x1f))?),
            0xc0 => Value::Nil,
            0xc2 | 0xc3 => Value::Other,
            0xc4 | 0xd9 => {
                let n = self.uint(1)? as usize;
                Value::Str(self.take(n)?)
            }
            0xc5 | 0xda => {
                let n = self.uint(2)? as usize;
                Value::Str(self.take(n)?)
            }
            0xc6 | 0xdb => {
                let n = self.uint(4)? as usize;
                Value::Str(self.take(n)?)
            }
            0xcc => Value::Int(self.uint(1)? as i64),
            0xcd => Value::Int(self.uint(2)? as i64),
            0xce => Value::Int(self.uint(4)? as i64),
            0xcf => Value::Int(self.uint(8)? as i64),
            0xd0 => Value::Int(i64::from(self.uint(1)? as u8 as i8)),
            0xd1 => Value::Int(i64::from(self.uint(2)? as u16 as i16)),
            0xd2 => Value::Int(i64::from(self.uint(4)? as u32 as i32)),
            0xd3 => Value::Int(self.uint(8)? as i64),
            0xca => {
                self.take(4)?;
                Value::Other
            }
            0xcb => {
                self.take(8)?;
                Value::Other
            }
            0xdc => Value::Array(self.uint(2)? as usize),
            0xdd => Value::Array(self.uint(4)? as usize),
            0xde => Value::Map(self.uint(2)? as usize),
            0xdf => Value::Map(self.uint(4)? as usize),
            0xe0..=0xff => Value::Int(i64::from(t as i8)),
            _ => return None,
        })
    }

    /// Past `n` values, and what's inside them.
    fn skip(&mut self, n: usize) -> Option<()> {
        for _ in 0..n {
            match self.next()? {
                Value::Map(m) => self.skip(m * 2)?,
                Value::Array(m) => self.skip(m)?,
                _ => {}
            }
        }
        Some(())
    }
}

/// What's in a renamed file that a database can know it by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Ids {
    crcs: Vec<u32>,
    serial: Option<String>,
}

/// The facts the index keeps of the games' files, read a folder at a time.
pub struct Facts<'a> {
    pub index: &'a Index,
    /// Whether a fact holds only while its file is as it was.
    check: bool,
    folders: HashMap<(PathBuf, &'static str), HashMap<std::ffi::OsString, Vec<u8>>>,
}

impl<'a> Facts<'a> {
    pub fn new(index: &'a Index, check: bool) -> Facts<'a> {
        Facts { index, check, folders: HashMap::new() }
    }

    pub fn get(&mut self, path: &Path, key: &'static str) -> Option<Vec<u8>> {
        let (dir, name) = (path.parent()?, path.file_name()?);
        let Facts { index, check, folders } = self;
        let here = folders
            .entry((dir.to_owned(), key))
            .or_insert_with(|| index.facts_in(dir, key, *check).unwrap_or_default());
        here.get(name).cloned()
    }
}

/// `path`'s ids (a disc's serial when `disc`): kept in the index, else
/// worked out (when `read`) and kept.
fn ids(facts: &mut Facts, path: &Path, disc: bool, read: bool) -> Option<Ids> {
    if disc {
        let serial = crate::disc::seen(facts, path, read)?.serial;
        return Some(Ids { serial, ..Ids::default() });
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if let Some(v) = facts.get(path, FACT_CRC) {
        return Some(Ids { crcs: v.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect(), serial: None });
    }
    if !read {
        return None;
    }
    let crcs = file_crcs(path, &ext);
    let value: Vec<u8> = crcs.iter().flat_map(|c| c.to_le_bytes()).collect();
    if let Err(e) = facts.index.set_fact(path, FACT_CRC, &value) {
        log::debug!("chanoma: keeping what {} is: {e}", path.display());
    }
    Some(Ids { crcs, serial: None })
}

/// A cartridge's CRCs: a zip's file's from its directory; a file's own,
/// and an NES game's without its 16-byte header, as sets list them both.
fn file_crcs(path: &Path, ext: &str) -> Vec<u32> {
    if ext == "zip" {
        return crate::zip::entries(path).map(|all| all.iter().map(|e| e.crc).collect()).unwrap_or_default();
    }
    if std::fs::metadata(path).map_or(true, |m| m.len() > READ_AT_MOST) {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let mut out = vec![crc32fast::hash(&bytes)];
    if bytes.starts_with(b"NES\x1a") && bytes.len() > 16 {
        out.push(crc32fast::hash(&bytes[16..]));
    }
    out
}

/// The systems' databases read so far. They're 27 MB in all (the
/// PlayStation's near 8, the NES's 6, the PlayStation 2's 5.5), so a
/// start reads each once for both its looks through the games, and the
/// systems' at once, a thread each.
#[derive(Debug, Default)]
pub struct Catalogs(HashMap<&'static str, Catalog>);

impl Catalogs {
    /// Reads those of `systems`' databases not read yet.
    pub fn read(&mut self, dir: &Path, systems: impl IntoIterator<Item = &'static str>) {
        let mut missing: Vec<&'static str> = systems.into_iter().filter(|s| !self.0.contains_key(s)).collect();
        missing.sort_unstable();
        missing.dedup();
        let read: Vec<(&'static str, Catalog)> = std::thread::scope(|s| {
            let each: Vec<_> = missing.iter().map(|sys| s.spawn(move || (*sys, Catalog::load(dir, sys)))).collect();
            each.into_iter().filter_map(|t| t.join().ok()).collect()
        });
        self.0.extend(read);
    }
}

/// Find each game in its system's databases, by name, else (when `read`,
/// or already worked out) by what's in its file. Says how many were
/// found by name and how many by what's in them.
pub fn describe(games: &mut [Game], index: &Index, dir: &Path, read: bool, catalogs: &mut Catalogs) -> (usize, usize) {
    let began = std::time::Instant::now();
    catalogs.read(dir, games.iter().map(|g| g.system.id));
    log::debug!("chanoma: the games' databases read in {:.0?}", began.elapsed());
    // A look at what the index knows takes what it kept as it is; the look
    // after a refresh holds it to the files as they are.
    let mut facts = Facts::new(index, read);
    let (mut by_name, mut by_content) = (0, 0);
    for g in games.iter_mut() {
        let Some(c) = catalogs.0.get(g.system.id).filter(|c| !c.is_empty()) else { continue };
        let stem = g.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if c.knows(&stem) {
            if let Some(i) = c.by_name(&stem) {
                g.info = Some(i.clone());
                by_name += 1;
            }
            g.known_as = Some(stem);
            continue;
        }
        let by_ids = ids(&mut facts, &g.path, g.system.is_disc(), read).and_then(|ids| {
            ids.crcs.iter().find_map(|crc| c.name_by_crc(*crc)).or_else(|| c.name_by_serial(ids.serial.as_deref()?))
        });
        // Else its name with the tags a database adds (a Dreamcast CHD's
        // serial isn't read).
        let (name, counted) = match by_ids {
            Some(n) => (Some(n), &mut by_content),
            None => (c.name_adding_tags(&stem), &mut by_name),
        };
        if let Some(n) = name {
            if let Some(i) = c.by_name(n) {
                g.info = Some(i.clone());
                *counted += 1;
            }
            g.known_as = Some(n.to_owned());
        }
    }
    (by_name, by_content)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A value to write into a test database.
    pub enum V<'a> {
        S(&'a str),
        B(&'a [u8]),
        I(u64),
    }

    /// A database of `games`, as RetroArch writes them.
    pub fn rdb(games: &[&[(&str, V)]]) -> Vec<u8> {
        let mut out = b"RARCHDB\0".to_vec();
        out.extend([0; 8]);
        let s = |out: &mut Vec<u8>, b: &[u8], bin: bool| {
            if bin {
                out.push(0xc4);
            } else {
                out.push(0xd9);
            }
            out.push(b.len() as u8);
            out.extend(b);
        };
        for g in games {
            out.push(0x80 | g.len() as u8);
            for (k, v) in *g {
                s(&mut out, k.as_bytes(), false);
                match v {
                    V::S(t) => s(&mut out, t.as_bytes(), false),
                    V::B(b) => s(&mut out, b, true),
                    V::I(n) if *n < 128 => out.push(*n as u8),
                    V::I(n) => {
                        out.push(0xcd);
                        out.extend((*n as u16).to_be_bytes());
                    }
                }
            }
        }
        out.push(0xc0);
        out
    }

    #[test]
    fn a_files_name_is_found_with_the_tags_its_database_adds() {
        let mut c = Catalog::default();
        c.add(&rdb(&[
            &[("name", V::S("Crazy Taxi (USA) (En,Ja)")), ("genre", V::S("Racing"))],
            &[("name", V::S("Crazy Taxi (USA) (Rev 1) (En,Ja)"))],
            &[("name", V::S("Crazy Taxi (Japan)"))],
            &[("name", V::S("Crazy Taxi 2 (USA)"))],
            &[("name", V::S("Crazy Taxi (USA) Demo"))],
        ]))
        .unwrap();
        // The one with fewest tags; never another region, another game, or
        // more than tags.
        assert_eq!(c.name_adding_tags("Crazy Taxi (USA)"), Some("Crazy Taxi (USA) (En,Ja)"));
        assert_eq!(c.by_name("Crazy Taxi (USA) (En,Ja)").unwrap().genre.as_deref(), Some("Racing"));
        assert_eq!(c.name_adding_tags("Crazy Taxi (Europe)"), None);
        assert_eq!(c.name_adding_tags("Crazy Taxi 2 (USA)"), None, "its own name, nothing added");
        // A file without a tag of its own isn't guessed at.
        assert_eq!(c.name_adding_tags("Crazy Taxi"), None);
    }

    #[test]
    fn a_database_is_read_and_games_found_by_name_crc_and_serial() {
        let mut c = Catalog::default();
        c.add(&rdb(&[
            &[("name", V::S("Crash Bandicoot (USA)")), ("serial", V::B(b"SCUS-94900")), ("crc", V::B(&[0, 0, 0, 1]))],
            &[
                ("name", V::S("Crash Bandicoot (USA)")),
                ("genre", V::S("Action")),
                ("users", V::I(1)),
                ("releaseyear", V::I(1996)),
                ("developer", V::S("Naughty Dog")),
            ],
            &[
                ("name", V::S("Contra III - The Alien Wars (USA)")),
                ("crc", V::B(&[0xab, 0x9d, 0xb6, 0x89])),
                ("users", V::I(2)),
            ],
            &[("name", V::S("Nothing Known (USA)")), ("crc", V::B(&[0, 0, 0, 2]))],
            &[("name", V::S("Crazy Taxi (USA) (En,Ja)")), ("serial", V::B(b"51035"))],
            &[("name", V::S("Soulcalibur (Europe)")), ("serial", V::B(b"MK-51136-50"))],
        ]))
        .unwrap();
        // Sega's serials with their `MK-` or without, either way round.
        assert_eq!(c.name_by_serial("MK-51035"), Some("Crazy Taxi (USA) (En,Ja)"));
        assert_eq!(c.name_by_serial("MK-51136-50"), Some("Soulcalibur (Europe)"));
        assert_eq!(c.name_by_serial("51136-50"), Some("Soulcalibur (Europe)"));
        assert_eq!(c.name_by_serial("MKX-51035"), None);
        assert!(!only_tags(" (USA) extra"));
        assert!(only_tags(" (En,Ja) (Rev 1)"));
        let crash = c.by_name("Crash Bandicoot (USA)").unwrap();
        assert_eq!(crash.summary(), "1996 · Action · 1 player");
        assert_eq!(crash.developer.as_deref(), Some("Naughty Dog"));
        assert_eq!(c.by_name(c.name_by_crc(0xab9d_b689).unwrap()).unwrap().summary(), "2 players");
        assert_eq!(c.name_by_serial("scus_949.00").and_then(|n| c.by_name(n)).unwrap(), crash);
        assert_eq!(c.by_name("Nothing Known (USA)"), None, "named, but nothing said");
        assert!(Catalog::default().add(b"not a database").is_err());
    }
}
