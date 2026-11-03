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
//! header too), or the serial a PlayStation disc boots, from its
//! `SYSTEM.CNF`.
//!
//! The databases are RetroArch's `.rdb` files: `RARCHDB\0`, an offset,
//! then one MessagePack map a game until a nil.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use aizome_index::Index;

use crate::library::Game;

/// The facts kept in the files index for a renamed file.
const FACT_CRC: &str = "chanoma.crc";
const FACT_SERIAL: &str = "chanoma.serial";
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
        "gb" => &["Nintendo - Game Boy"],
        "gbc" => &["Nintendo - Game Boy Color"],
        "gba" => &["Nintendo - Game Boy Advance"],
        "genesis" => &["Sega - Mega Drive - Genesis"],
        "sms" => &["Sega - Master System - Mark III"],
        "gg" => &["Sega - Game Gear"],
        "pce" => &["NEC - PC Engine - TurboGrafx 16", "NEC - PC Engine SuperGrafx"],
        "psx" => &["Sony - PlayStation"],
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
    names: HashMap<String, Info>,
    crcs: HashMap<u32, String>,
    serials: HashMap<String, String>,
}

/// A serial as compared: letters and digits, upper case (`SCUS-94900`,
/// `SCUS_949.00` and `scus94900` are one).
fn serial_key(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect()
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

    /// The name of the game with this CRC, or this disc serial.
    fn name_by_crc(&self, crc: u32) -> Option<&str> {
        self.crcs.get(&crc).map(String::as_str)
    }

    fn name_by_serial(&self, serial: &str) -> Option<&str> {
        self.serials.get(&serial_key(serial)).map(String::as_str)
    }
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

/// `path`'s ids (a disc's when `disc`): kept in the index, else worked out
/// (when `read`) and kept.
fn ids(index: &Index, path: &Path, disc: bool, read: bool) -> Option<Ids> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    // A disc by its sheet; a CHD or PBP isn't read.
    if disc && !matches!(ext.as_str(), "cue" | "m3u") {
        return None;
    }
    let key = if disc { FACT_SERIAL } else { FACT_CRC };
    if let Ok(Some(v)) = index.fact(path, key) {
        return Some(if disc {
            Ids { serial: (!v.is_empty()).then(|| String::from_utf8_lossy(&v).into_owned()), ..Ids::default() }
        } else {
            Ids { crcs: v.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect(), serial: None }
        });
    }
    if !read {
        return None;
    }
    let (ids, value) = if disc {
        let serial = disc_serial(path);
        let value = serial.clone().unwrap_or_default().into_bytes();
        (Ids { serial, ..Ids::default() }, value)
    } else {
        let crcs = file_crcs(path, &ext);
        let value = crcs.iter().flat_map(|c| c.to_le_bytes()).collect();
        (Ids { crcs, serial: None }, value)
    };
    if let Err(e) = index.set_fact(path, key, &value) {
        log::debug!("chanoma: keeping what {} is: {e}", path.display());
    }
    Some(ids)
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

/// The serial a PlayStation disc boots (`SCUS-94900`), from its first
/// track's `SYSTEM.CNF`: a cue sheet's (or a playlist's first disc's).
pub fn disc_serial(sheet: &Path) -> Option<String> {
    let text = std::fs::read_to_string(sheet).ok()?;
    let dir = sheet.parent()?;
    if sheet.extension().is_some_and(|e| e.eq_ignore_ascii_case("m3u")) {
        let first = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
        return disc_serial(&dir.join(first));
    }
    // The first file and the first track's mode.
    let mut file = None;
    let mut mode = None;
    for line in text.lines().map(str::trim) {
        if file.is_none()
            && let Some(rest) = line.strip_prefix("FILE ")
        {
            let name = match rest.strip_prefix('"') {
                Some(r) => r.split('"').next()?,
                None => rest.rsplit_once(' ').map_or(rest, |(n, _)| n),
            };
            file = Some(dir.join(name));
        } else if let Some(rest) = line.strip_prefix("TRACK ") {
            mode = rest.split_whitespace().nth(1).map(str::to_owned);
            break;
        }
    }
    let (size, skip) = match mode.as_deref()? {
        "MODE2/2352" => (2352, 24),
        "MODE1/2352" => (2352, 16),
        "MODE2/2336" => (2336, 8),
        "MODE1/2048" | "MODE2/2048" => (2048, 0),
        _ => return None,
    };
    let mut disc = Disc { f: std::fs::File::open(file?).ok()?, size, skip };
    let pvd = disc.sectors(16, 1)?;
    if &pvd[1..6] != b"CD001" {
        return None;
    }
    let root = &pvd[156..190];
    let (at, len) = (u32_at(root, 2), u32_at(root, 10));
    let dir = disc.sectors(at, (len as usize).div_ceil(2048).min(16))?;
    let mut i = 0;
    while i < dir.len() {
        let n = usize::from(dir[i]);
        if n == 0 {
            // Records don't cross sectors: on to the next.
            i = (i / 2048 + 1) * 2048;
            continue;
        }
        let rec = dir.get(i..i + n)?;
        let name = rec.get(33..33 + usize::from(*rec.get(32)?))?;
        if name.eq_ignore_ascii_case(b"SYSTEM.CNF;1") || name.eq_ignore_ascii_case(b"SYSTEM.CNF") {
            let (at, len) = (u32_at(rec, 2), u32_at(rec, 10).min(2048));
            let cnf = disc.sectors(at, 1)?;
            let cnf = String::from_utf8_lossy(&cnf[..len as usize]).into_owned();
            return boot_serial(&cnf);
        }
        i += n;
    }
    None
}

/// `BOOT = cdrom:\SCUS_949.00;1` → `SCUS-94900`.
fn boot_serial(cnf: &str) -> Option<String> {
    let line = cnf.lines().find(|l| l.trim_start().to_ascii_uppercase().starts_with("BOOT"))?;
    let exe = line.split_once('=')?.1.trim();
    let exe = exe.rsplit(['\\', ':', '/']).next()?;
    let exe = exe.split(';').next()?.trim();
    let (prefix, number) = exe.split_once('_')?;
    let number: String = number.chars().filter(char::is_ascii_digit).collect();
    (!prefix.is_empty() && !number.is_empty()).then(|| format!("{}-{number}", prefix.to_ascii_uppercase()))
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// A disc image's track, read a sector's user data at a time.
struct Disc {
    f: std::fs::File,
    size: u64,
    skip: u64,
}

impl Disc {
    fn sectors(&mut self, at: u32, n: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(n * 2048);
        let mut buf = [0; 2048];
        for s in 0..n as u64 {
            self.f.seek(SeekFrom::Start((u64::from(at) + s) * self.size + self.skip)).ok()?;
            self.f.read_exact(&mut buf).ok()?;
            out.extend_from_slice(&buf);
        }
        Some(out)
    }
}

/// Find each game in its system's databases, by name, else (when `read`,
/// or already worked out) by what's in its file. Says how many were
/// found by name and how many by what's in them.
pub fn describe(games: &mut [Game], index: &Index, dir: &Path, read: bool) -> (usize, usize) {
    let mut catalogs: HashMap<&'static str, Catalog> = HashMap::new();
    let (mut by_name, mut by_content) = (0, 0);
    for g in games.iter_mut() {
        let c = catalogs.entry(g.system.id).or_insert_with(|| Catalog::load(dir, g.system.id));
        if c.is_empty() {
            continue;
        }
        let stem = g.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if c.knows(&stem) {
            if let Some(i) = c.by_name(&stem) {
                g.info = Some(i.clone());
                by_name += 1;
            }
            g.known_as = Some(stem);
            continue;
        }
        let Some(ids) = ids(index, &g.path, g.system.id == "psx", read) else { continue };
        let name =
            ids.crcs.iter().find_map(|crc| c.name_by_crc(*crc)).or_else(|| c.name_by_serial(ids.serial.as_deref()?));
        if let Some(n) = name {
            if let Some(i) = c.by_name(n) {
                g.info = Some(i.clone());
                by_content += 1;
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

    /// A PlayStation disc's first track (2,352-byte sectors, as a BIN is)
    /// booting `boot`.
    pub fn disc(boot: &str) -> Vec<u8> {
        let mut img = vec![0u8; 24 * 2352];
        let mut put = |lba: usize, data: &[u8]| {
            let at = lba * 2352 + 24;
            img[at..at + data.len()].copy_from_slice(data);
        };
        let mut pvd = vec![0u8; 2048];
        pvd[0] = 1;
        pvd[1..6].copy_from_slice(b"CD001");
        // The root folder: a record at sector 18, one sector long.
        pvd[156] = 34;
        pvd[158..162].copy_from_slice(&18u32.to_le_bytes());
        pvd[166..170].copy_from_slice(&2048u32.to_le_bytes());
        put(16, &pvd);
        let cnf = format!("BOOT = cdrom:\\{boot};1\r\nTCB = 4\r\n");
        let mut root = vec![0u8; 2048];
        let mut i = 0;
        for (name, at, len) in
            [(&b"\0"[..], 18u32, 2048u32), (b"README.TXT;1", 19, 5), (b"SYSTEM.CNF;1", 20, cnf.len() as u32)]
        {
            let n = 33 + name.len() + (name.len() + 1) % 2;
            root[i] = n as u8;
            root[i + 2..i + 6].copy_from_slice(&at.to_le_bytes());
            root[i + 10..i + 14].copy_from_slice(&len.to_le_bytes());
            root[i + 32] = name.len() as u8;
            root[i + 33..i + 33 + name.len()].copy_from_slice(name);
            i += n;
        }
        put(18, &root);
        put(20, cnf.as_bytes());
        img
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
        ]))
        .unwrap();
        let crash = c.by_name("Crash Bandicoot (USA)").unwrap();
        assert_eq!(crash.summary(), "1996 · Action · 1 player");
        assert_eq!(crash.developer.as_deref(), Some("Naughty Dog"));
        assert_eq!(c.by_name(c.name_by_crc(0xab9d_b689).unwrap()).unwrap().summary(), "2 players");
        assert_eq!(c.name_by_serial("scus_949.00").and_then(|n| c.by_name(n)).unwrap(), crash);
        assert_eq!(c.by_name("Nothing Known (USA)"), None, "named, but nothing said");
        assert!(Catalog::default().add(b"not a database").is_err());
    }

    #[test]
    fn a_disc_says_its_serial() {
        let d = std::env::temp_dir().join(format!("chanoma-disc-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Game (Track 1).bin"), disc("SCUS_949.00")).unwrap();
        std::fs::write(
            d.join("Renamed.cue"),
            "FILE \"Game (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        std::fs::write(d.join("Game.m3u"), "# a playlist\nRenamed.cue\n").unwrap();
        assert_eq!(disc_serial(&d.join("Renamed.cue")).as_deref(), Some("SCUS-94900"));
        assert_eq!(disc_serial(&d.join("Game.m3u")).as_deref(), Some("SCUS-94900"));
        assert_eq!(boot_serial("BOOT=cdrom:SLPS_012.34;1").as_deref(), Some("SLPS-01234"));
        assert_eq!(boot_serial("VMODE = NTSC"), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
