//! What a game disc is for, from what's on it. Saturn, Dreamcast,
//! PlayStation and PlayStation 2 discs all come as cue sheets and CHDs
//! (and the PlayStations' as ISOs too), so a disc whose folder doesn't
//! name its system is known by its data track:
//!
//! - a Saturn's starts with `SEGA SEGASATURN `, a Dreamcast's with
//!   `SEGA SEGAKATANA ` (the system area), each with its product number;
//! - a PlayStation's `SYSTEM.CNF` boots with `BOOT`, a PlayStation 2's
//!   with `BOOT2`, naming the serial;
//! - a CHD's track list says a GD-ROM (a Dreamcast's) before anything is
//!   decompressed; a CD's or a DVD's first sectors are read through it
//!   ([`chd`]), as a CSO's are through its blocks.
//!
//! GDI and CDI images are Dreamcast discs and PBP the PlayStation's, by
//! their names. Other discs (a Mega-CD's, a PC Engine CD's, a PSP's,
//! music, a computer's) are none of these. What a disc is, is worked out
//! once and kept in the files index beside it ([`seen`]).

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::metadata::Facts;

/// The fact kept in the files index: the system, a newline, the serial
/// (either empty).
const FACT_DISC: &str = "chanoma.disc";

/// What a disc says of itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// Its system's id (`saturn`); none for a disc of no system here.
    pub system: Option<&'static str>,
    /// The serial it boots or its system area gives (`SCUS-94900`,
    /// `MK-81009`).
    pub serial: Option<String>,
}

impl Seen {
    fn of(system: &'static str, serial: Option<String>) -> Seen {
        Seen { system: Some(system), serial }
    }

    fn fact(&self) -> Vec<u8> {
        format!("{}\n{}", self.system.unwrap_or(""), self.serial.as_deref().unwrap_or("")).into_bytes()
    }

    fn from_fact(v: &[u8]) -> Seen {
        let v = String::from_utf8_lossy(v);
        let (system, serial) = v.split_once('\n').unwrap_or((&v, ""));
        let system = crate::library::system(system).map(|s| s.id);
        Seen { system, serial: (!serial.is_empty()).then(|| serial.to_owned()) }
    }
}

/// What `path`'s disc is, as the index keeps it; else, when `read`, looked
/// at and kept.
pub fn seen(facts: &mut Facts, path: &Path, read: bool) -> Option<Seen> {
    if let Some(v) = facts.get(path, FACT_DISC) {
        return Some(Seen::from_fact(&v));
    }
    if !read {
        return None;
    }
    let s = look(path);
    if let Err(e) = facts.index.set_fact(path, FACT_DISC, &s.fact()) {
        log::debug!("chanoma: keeping what {} is: {e}", path.display());
    }
    Some(s)
}

/// Look at `path`'s disc: a cue sheet's, a CloneCD image's, an ISO, a
/// CHD, a GDI's, or a playlist's first disc.
pub fn look(path: &Path) -> Seen {
    look_at(path, 0)
}

fn look_at(path: &Path, depth: u8) -> Seen {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "m3u" if depth < 2 => first_entry(path).map(|p| look_at(&p, depth + 1)).unwrap_or_default(),
        "cue" => cue(path),
        "gdi" => gdi(path),
        "ccd" => {
            Raw::open(&path.with_extension("img"), 0, Sector::Raw).map(|mut t| identify(&mut t)).unwrap_or_default()
        }
        "iso" => iso(path),
        "chd" => chd(path).unwrap_or_default(),
        "cdi" => Seen::of("dreamcast", None),
        "cso" => Cso::open(path).map(|mut t| identify(&mut t)).unwrap_or_default(),
        "pbp" => Seen::of("psx", None),
        _ => Seen::default(),
    }
}

/// A playlist's first disc, beside it.
fn first_entry(m3u: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(m3u).ok()?;
    let first = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    Some(m3u.parent()?.join(first))
}

/// A track read a sector's user data (2,048 bytes) at a time.
trait Track {
    fn user(&mut self, lba: u32) -> Option<Vec<u8>>;
}

/// How a track's sectors are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sector {
    /// 2,352 bytes as on the disc: the user data after the header, at 16
    /// (mode 1) or 24 (mode 2, after the subheader) by the mode it says.
    Raw,
    /// User data alone, 2,048 bytes.
    Cooked,
    /// Mode 2's 2,336: the subheader, then the user data.
    Mode2,
}

impl Sector {
    fn of_cue(mode: &str) -> Option<Sector> {
        match mode.to_ascii_uppercase().as_str() {
            "MODE1/2352" | "MODE2/2352" => Some(Sector::Raw),
            "MODE1/2048" | "MODE2/2048" => Some(Sector::Cooked),
            "MODE2/2336" => Some(Sector::Mode2),
            _ => None,
        }
    }

    fn size(self) -> u64 {
        match self {
            Sector::Raw => 2352,
            Sector::Cooked => 2048,
            Sector::Mode2 => 2336,
        }
    }

    /// The user data in a sector of this layout.
    fn data(self, sector: &[u8]) -> Option<Vec<u8>> {
        let at = match self {
            Sector::Raw => {
                if *sector.get(15)? == 2 {
                    24
                } else {
                    16
                }
            }
            Sector::Cooked => 0,
            Sector::Mode2 => 8,
        };
        sector.get(at..at + 2048).map(<[u8]>::to_vec)
    }
}

/// A track in a file of its own, or from an offset in one.
struct Raw {
    f: std::fs::File,
    start: u64,
    sector: Sector,
}

impl Raw {
    fn open(path: &Path, start: u64, sector: Sector) -> Option<Raw> {
        Some(Raw { f: std::fs::File::open(path).ok()?, start, sector })
    }
}

impl Track for Raw {
    fn user(&mut self, lba: u32) -> Option<Vec<u8>> {
        let size = self.sector.size();
        let mut buf = vec![0u8; size as usize];
        self.f.seek(SeekFrom::Start(self.start + u64::from(lba) * size)).ok()?;
        self.f.read_exact(&mut buf).ok()?;
        self.sector.data(&buf)
    }
}

/// A product number from a Sega system area: space-padded ASCII.
fn field(b: &[u8]) -> Option<String> {
    let s: String = String::from_utf8_lossy(b).trim().chars().filter(|c| !c.is_control()).collect();
    (!s.is_empty()).then_some(s)
}

/// What a data track says it is.
fn identify(t: &mut dyn Track) -> Seen {
    let Some(first) = t.user(0) else { return Seen::default() };
    if first.starts_with(b"SEGA SEGASATURN ") {
        return Seen::of("saturn", field(&first[0x20..0x2a]));
    }
    if first.starts_with(b"SEGA SEGAKATANA ") {
        return Seen::of("dreamcast", field(&first[0x40..0x4a]));
    }
    let Some(pvd) = t.user(16) else { return Seen::default() };
    if pvd.get(1..6) != Some(b"CD001") {
        return Seen::default();
    }
    if let Some(cnf) = iso_file(t, &pvd, b"SYSTEM.CNF") {
        let cnf = String::from_utf8_lossy(&cnf).into_owned();
        let boot = |key: &str| {
            cnf.lines().find(|l| {
                let l = l.trim_start().to_ascii_uppercase();
                l.strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('='))
            })
        };
        if let Some(line) = boot("BOOT2") {
            return Seen::of("ps2", boot_serial(line));
        }
        if let Some(line) = boot("BOOT") {
            return Seen::of("psx", boot_serial(line));
        }
    }
    // The first PlayStation discs boot PSX.EXE, with no SYSTEM.CNF; their
    // volume says whose they are.
    if pvd[8..40].starts_with(b"PLAYSTATION") {
        return Seen::of("psx", None);
    }
    Seen::default()
}

/// A file in an ISO 9660 disc's top folder, by name (its first sector at
/// most: `SYSTEM.CNF` is a few lines).
fn iso_file(t: &mut dyn Track, pvd: &[u8], want: &[u8]) -> Option<Vec<u8>> {
    let root = &pvd[156..190];
    let (at, len) = (u32_at(root, 2), u32_at(root, 10));
    let mut dir = Vec::new();
    for s in 0..len.div_ceil(2048).min(16) {
        dir.extend(t.user(at + s)?);
    }
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
        let name = name.split(|&b| b == b';').next().unwrap_or(name);
        if name.eq_ignore_ascii_case(want) {
            let (at, len) = (u32_at(rec, 2), u32_at(rec, 10).min(2048));
            let mut data = t.user(at)?;
            data.truncate(len as usize);
            return Some(data);
        }
        i += n;
    }
    None
}

/// `BOOT = cdrom:\SCUS_949.00;1` → `SCUS-94900`.
pub fn boot_serial(line: &str) -> Option<String> {
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

/// A cue sheet's tracks: each data track's first sectors, in order, until
/// one says what it is. Redump's Dreamcast sheets say their high-density
/// area.
fn cue(sheet: &Path) -> Seen {
    let Ok(text) = std::fs::read_to_string(sheet) else { return Seen::default() };
    let dir = sheet.parent().unwrap_or(Path::new("."));
    let gd = text.lines().any(|l| l.trim().eq_ignore_ascii_case("REM HIGH-DENSITY AREA"));
    let mut file: Option<PathBuf> = None;
    let mut sector: Option<Sector> = None;
    let mut tracks: Vec<(PathBuf, u64, Sector)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("FILE ") {
            let name = match rest.strip_prefix('"') {
                Some(r) => r.split('"').next().unwrap_or(r),
                None => rest.rsplit_once(' ').map_or(rest, |(n, _)| n),
            };
            file = Some(dir.join(name));
        } else if let Some(rest) = line.strip_prefix("TRACK ") {
            sector = rest.split_whitespace().nth(1).and_then(Sector::of_cue);
        } else if let Some(rest) = line.strip_prefix("INDEX 01 ")
            && let (Some(f), Some(s)) = (&file, sector.take())
        {
            tracks.push((f.clone(), msf(rest.trim()).unwrap_or(0) * s.size(), s));
        }
    }
    for (f, start, s) in tracks {
        let seen = Raw::open(&f, start, s).map(|mut t| identify(&mut t)).unwrap_or_default();
        if seen.system.is_some() {
            return seen;
        }
    }
    if gd { Seen::of("dreamcast", None) } else { Seen::default() }
}

/// `MM:SS:FF` as frames (75 a second).
fn msf(s: &str) -> Option<u64> {
    let mut p = s.split(':').map(|n| n.parse::<u64>().ok());
    let (m, s, f) = (p.next()??, p.next()??, p.next()??);
    Some((m * 60 + s) * 75 + f)
}

/// A GD-ROM's sheet: `3 45000 4 2352 track03.bin 0`, the high-density
/// area's first data track holding the system area.
fn gdi(path: &Path) -> Seen {
    let dir = path.parent().unwrap_or(Path::new("."));
    let serial = std::fs::read_to_string(path).ok().and_then(|text| {
        text.lines().skip(1).find_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            // Number, start, type (4 for data), sector size, file, offset.
            let (start, kind, size) = (parts.get(1)?, parts.get(2)?, parts.get(3)?);
            if *kind != "4" || start.parse::<u64>().ok()? < 45000 {
                return None;
            }
            let sector = if *size == "2048" { Sector::Cooked } else { Sector::Raw };
            let name = l.split_once('"').and_then(|(_, r)| r.split('"').next()).unwrap_or(parts.get(4)?);
            let mut t = Raw::open(&dir.join(name), 0, sector)?;
            identify(&mut t).serial
        })
    });
    Seen::of("dreamcast", serial)
}

/// An ISO: user data alone, or (a BIN renamed) whole sectors.
fn iso(path: &Path) -> Seen {
    let Ok(mut f) = std::fs::File::open(path) else { return Seen::default() };
    let mut sync = [0u8; 12];
    let raw = f.read_exact(&mut sync).is_ok() && sync == SYNC;
    let sector = if raw { Sector::Raw } else { Sector::Cooked };
    Raw::open(path, 0, sector).map(|mut t| identify(&mut t)).unwrap_or_default()
}

const SYNC: [u8; 12] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];

/// A CHD's header and track list, read as they are (they're never
/// compressed), then its first sectors through [`chd`].
fn chd(path: &Path) -> Option<Seen> {
    let mut f = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let meta = chd_metadata(&mut f)?;
    let has = |t: &[u8; 4]| meta.iter().any(|(m, _)| m == t);
    if has(b"CHGD") || has(b"CHGT") {
        return Some(Seen::of("dreamcast", None));
    }
    f.seek(SeekFrom::Start(0)).ok()?;
    let chd = ::chd::Chd::open(f, None).ok()?;
    let hunk_bytes = chd.header().hunk_size();
    if has(b"DVD ") {
        // A DVD's sectors are its user data, one after the other.
        let mut t =
            ChdTrack { chd, hunk_bytes, unit: 2048, first: 0, sector: Sector::Cooked, hunk: None, tmp: Vec::new() };
        return Some(identify(&mut t));
    }
    // A CD: its first data track, where it starts. Each track's frames
    // are padded to four (chdman's layout); a pregap kept in the file
    // ("V…") comes before the track's first sector.
    let mut frame = 0u64;
    for (_, v) in meta.iter().filter(|(m, _)| m == b"CHT2" || m == b"CHTR") {
        let field = |k: &str| v.split_whitespace().find_map(|p| p.strip_prefix(k).map(str::to_owned));
        let frames: u64 = field("FRAMES:")?.parse().ok()?;
        let kind = field("TYPE:")?;
        let pregap: u64 = field("PREGAP:").and_then(|p| p.parse().ok()).unwrap_or(0);
        let kept = field("PGTYPE:").is_some_and(|p| p.starts_with('V'));
        let sector = match kind.as_str() {
            "MODE1_RAW" | "MODE2_RAW" => Some(Sector::Raw),
            "MODE1" | "MODE2_FORM1" => Some(Sector::Cooked),
            "MODE2" | "MODE2_FORM_MIX" => Some(Sector::Mode2),
            _ => None,
        };
        if let Some(sector) = sector {
            let first = frame + if kept { pregap } else { 0 };
            let mut t = ChdTrack { chd, hunk_bytes, unit: CD_FRAME, first, sector, hunk: None, tmp: Vec::new() };
            return Some(identify(&mut t));
        }
        frame += frames.div_ceil(4) * 4;
    }
    Some(Seen::default())
}

/// A CD frame in a CHD: the sector, then its subcode.
const CD_FRAME: u64 = 2352 + 96;

/// The metadata of a CHD (versions 3 to 5): each entry's tag and text.
fn chd_metadata(f: &mut (impl Read + Seek)) -> Option<Vec<([u8; 4], String)>> {
    let mut head = [0u8; 56];
    f.read_exact(&mut head).ok()?;
    if &head[..8] != b"MComprHD" {
        return None;
    }
    let version = u32::from_be_bytes(head[12..16].try_into().ok()?);
    let at = match version {
        3 | 4 => 36,
        5 => 48,
        _ => return None,
    };
    let mut next = u64::from_be_bytes(head[at..at + 8].try_into().ok()?);
    let mut out = Vec::new();
    // A chain that loops, or runs on, isn't followed far.
    while next != 0 && out.len() < 128 {
        f.seek(SeekFrom::Start(next)).ok()?;
        let mut entry = [0u8; 16];
        f.read_exact(&mut entry).ok()?;
        let tag: [u8; 4] = entry[..4].try_into().ok()?;
        let len = (u32::from_be_bytes(entry[4..8].try_into().ok()?) & 0x00ff_ffff) as usize;
        let mut value = vec![0u8; len.min(4096)];
        f.read_exact(&mut value).ok()?;
        let text = String::from_utf8_lossy(value.split(|&b| b == 0).next().unwrap_or(&value)).into_owned();
        out.push((tag, text));
        let after = u64::from_be_bytes(entry[8..16].try_into().ok()?);
        if after <= next && after != 0 {
            break;
        }
        next = after;
    }
    Some(out)
}

/// A track of a CHD, read a hunk at a time.
struct ChdTrack {
    chd: ::chd::Chd<std::io::BufReader<std::fs::File>>,
    hunk_bytes: u32,
    /// Bytes a sector takes in the CHD's data.
    unit: u64,
    /// The track's first sector, in sectors from the start.
    first: u64,
    sector: Sector,
    hunk: Option<(u32, Vec<u8>)>,
    tmp: Vec<u8>,
}

impl Track for ChdTrack {
    fn user(&mut self, lba: u32) -> Option<Vec<u8>> {
        let at = (self.first + u64::from(lba)) * self.unit;
        let (n, within) = ((at / u64::from(self.hunk_bytes)) as u32, (at % u64::from(self.hunk_bytes)) as usize);
        if self.hunk.as_ref().is_none_or(|(h, _)| *h != n) {
            let mut out = self.chd.get_hunksized_buffer();
            self.chd.hunk(n).ok()?.read_hunk_in(&mut self.tmp, &mut out).ok()?;
            self.hunk = Some((n, out));
        }
        let (_, data) = self.hunk.as_ref()?;
        let size = if self.unit == CD_FRAME { 2352 } else { 2048 };
        self.sector.data(data.get(within..within + size)?)
    }
}

/// A CSO (CISO): an ISO in blocks, each deflated unless its place in the
/// index is marked as kept plain.
struct Cso {
    f: std::fs::File,
    block: u64,
    align: u32,
    blocks: u64,
}

impl Cso {
    fn open(path: &Path) -> Option<Cso> {
        let mut f = std::fs::File::open(path).ok()?;
        let mut head = [0u8; 24];
        f.read_exact(&mut head).ok()?;
        if &head[..4] != b"CISO" {
            return None;
        }
        let total = u64::from_le_bytes(head[8..16].try_into().ok()?);
        let block = u64::from(u32::from_le_bytes(head[16..20].try_into().ok()?));
        (block >= 2048).then(|| Cso { f, block, align: u32::from(head[21]), blocks: total.div_ceil(block) })
    }

    fn block(&mut self, n: u64) -> Option<Vec<u8>> {
        if n >= self.blocks {
            return None;
        }
        let mut index = [0u8; 8];
        self.f.seek(SeekFrom::Start(24 + n * 4)).ok()?;
        self.f.read_exact(&mut index).ok()?;
        let (here, next) =
            (u32::from_le_bytes(index[..4].try_into().ok()?), u32::from_le_bytes(index[4..].try_into().ok()?));
        let at = |v: u32| u64::from(v & 0x7fff_ffff) << self.align;
        let len = at(next).checked_sub(at(here))?.min(self.block * 2);
        let mut data = vec![0u8; len as usize];
        self.f.seek(SeekFrom::Start(at(here))).ok()?;
        self.f.read_exact(&mut data).ok()?;
        if here & 0x8000_0000 != 0 {
            data.truncate(self.block as usize);
            Some(data)
        } else {
            miniz_oxide::inflate::decompress_to_vec(&data).ok()
        }
    }
}

impl Track for Cso {
    fn user(&mut self, lba: u32) -> Option<Vec<u8>> {
        let at = u64::from(lba) * 2048;
        let block = self.block(at / self.block)?;
        let within = (at % self.block) as usize;
        block.get(within..within + 2048).map(<[u8]>::to_vec)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An ISO 9660 track's sectors (user data alone), the volume named
    /// for `volume`, its top folder holding `SYSTEM.CNF` with `cnf`, after
    /// `first` at sector 0.
    pub fn iso_sectors(first: &[u8], volume: &[u8], cnf: &str) -> Vec<[u8; 2048]> {
        let mut s = vec![[0u8; 2048]; 24];
        s[0][..first.len()].copy_from_slice(first);
        let pvd = &mut s[16];
        pvd[0] = 1;
        pvd[1..6].copy_from_slice(b"CD001");
        pvd[8..8 + volume.len()].copy_from_slice(volume);
        // The root folder: a record at sector 18, one sector long.
        pvd[156] = 34;
        pvd[158..162].copy_from_slice(&18u32.to_le_bytes());
        pvd[166..170].copy_from_slice(&2048u32.to_le_bytes());
        let mut root = [0u8; 2048];
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
        s[18] = root;
        if !cnf.is_empty() {
            s[20][..cnf.len()].copy_from_slice(cnf.as_bytes());
        }
        s
    }

    /// Sectors as a BIN holds them: 2,352 bytes each, mode 1 or 2.
    pub fn raw(sectors: &[[u8; 2048]], mode: u8) -> Vec<u8> {
        let mut out = Vec::new();
        for s in sectors {
            let mut r = vec![0u8; 2352];
            r[..12].copy_from_slice(&SYNC);
            r[15] = mode;
            let at = if mode == 2 { 24 } else { 16 };
            r[at..at + 2048].copy_from_slice(s);
            out.extend(r);
        }
        out
    }

    /// A Sega system area: `hardware` (`SEGA SEGASATURN `), its product
    /// number at `at`.
    pub fn sega(hardware: &[u8], at: usize, product: &[u8]) -> Vec<u8> {
        let mut ip = vec![b' '; 0x100];
        ip[..16].copy_from_slice(hardware);
        ip[at..at + product.len()].copy_from_slice(product);
        ip
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chanoma-disc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn cue_for(bin: &str, mode: &str) -> String {
        format!("FILE \"{bin}\" BINARY\n  TRACK 01 {mode}\n    INDEX 01 00:00:00\n")
    }

    #[test]
    fn discs_say_their_system_and_serial() {
        let d = temp("systems");
        let ps1 = raw(&iso_sectors(b"", b"PLAYSTATION", "BOOT = cdrom:\\SCUS_949.00;1\r\nTCB = 4\r\n"), 2);
        std::fs::write(d.join("Crash (Track 1).bin"), &ps1).unwrap();
        std::fs::write(d.join("Crash.cue"), cue_for("Crash (Track 1).bin", "MODE2/2352")).unwrap();
        assert_eq!(look(&d.join("Crash.cue")), Seen::of("psx", Some("SCUS-94900".into())));

        let ps2 = iso_sectors(b"", b"PLAYSTATION", "BOOT2 = cdrom0:\\SLUS_200.62;1\r\nVER = 1.00\r\n");
        std::fs::write(d.join("Gran Turismo 3.iso"), ps2.concat()).unwrap();
        assert_eq!(look(&d.join("Gran Turismo 3.iso")), Seen::of("ps2", Some("SLUS-20062".into())));

        let saturn = raw(&iso_sectors(&sega(b"SEGA SEGASATURN ", 0x20, b"MK-81009  "), b"SEGA SEGASATURN", ""), 1);
        std::fs::write(d.join("Nights.bin"), &saturn).unwrap();
        // Two tracks in one file: the data track first.
        std::fs::write(
            d.join("Nights.cue"),
            "FILE \"Nights.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 00:00:10\n",
        )
        .unwrap();
        assert_eq!(look(&d.join("Nights.cue")), Seen::of("saturn", Some("MK-81009".into())));
        std::fs::write(d.join("Nights.img"), &saturn).unwrap();
        std::fs::write(d.join("Nights.ccd"), "[CloneCD]\nVersion=3\n").unwrap();
        assert_eq!(look(&d.join("Nights.ccd")).system, Some("saturn"));

        // A Dreamcast's in Redump's form: the high-density area's data
        // track has the system area.
        let katana = raw(&iso_sectors(&sega(b"SEGA SEGAKATANA ", 0x40, b"MK-51000  "), b"", ""), 1);
        let low = raw(&iso_sectors(b"", b"", ""), 1);
        std::fs::write(d.join("Sonic (Track 1).bin"), &low).unwrap();
        std::fs::write(d.join("Sonic (Track 3).bin"), &katana).unwrap();
        std::fs::write(
            d.join("Sonic.cue"),
            "REM SINGLE-DENSITY AREA\nFILE \"Sonic (Track 1).bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n\
             REM HIGH-DENSITY AREA\nFILE \"Sonic (Track 3).bin\" BINARY\n  TRACK 03 MODE1/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        assert_eq!(look(&d.join("Sonic.cue")), Seen::of("dreamcast", Some("MK-51000".into())));
        std::fs::write(d.join("Sonic.gdi"), "3\n1 0 4 2352 \"Sonic (Track 1).bin\" 0\n2 756 0 2352 track02.raw 0\n3 45000 4 2352 \"Sonic (Track 3).bin\" 0\n").unwrap();
        assert_eq!(look(&d.join("Sonic.gdi")), Seen::of("dreamcast", Some("MK-51000".into())));

        // A playlist is its first disc's.
        std::fs::write(d.join("Crash.m3u"), "# discs\nCrash.cue\n").unwrap();
        assert_eq!(look(&d.join("Crash.m3u")).system, Some("psx"));
        // A Mega-CD's is none of these, nor is music.
        let mega = raw(&iso_sectors(b"SEGADISCSYSTEM  ", b"", ""), 1);
        std::fs::write(d.join("Sonic CD.bin"), &mega[..16 * 2352]).unwrap();
        std::fs::write(d.join("Sonic CD.cue"), cue_for("Sonic CD.bin", "MODE1/2352")).unwrap();
        assert_eq!(look(&d.join("Sonic CD.cue")), Seen::default());
        std::fs::write(d.join("Album.cue"), cue_for("Album.flac", "AUDIO")).unwrap();
        assert_eq!(look(&d.join("Album.cue")), Seen::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// An uncompressed CHD (version 5) of `data`, `unit` bytes a sector,
    /// with `meta` entries.
    pub fn chd_file(data: &[u8], unit: u32, hunk: u32, meta: &[(&[u8; 4], String)]) -> Vec<u8> {
        let mut out = vec![0u8; 124];
        out[..8].copy_from_slice(b"MComprHD");
        out[8..12].copy_from_slice(&124u32.to_be_bytes());
        out[12..16].copy_from_slice(&5u32.to_be_bytes());
        out[32..40].copy_from_slice(&(data.len() as u64).to_be_bytes());
        out[56..60].copy_from_slice(&hunk.to_be_bytes());
        out[60..64].copy_from_slice(&unit.to_be_bytes());
        // Metadata after the header, each entry linked to the next.
        let meta_at = out.len() as u64;
        out[48..56].copy_from_slice(&meta_at.to_be_bytes());
        for (i, (tag, text)) in meta.iter().enumerate() {
            let mut value = text.clone().into_bytes();
            value.push(0);
            let next = if i + 1 < meta.len() { (out.len() + 16 + value.len()) as u64 } else { 0 };
            out.extend(*tag);
            out.extend(((value.len() as u32) | 0x0100_0000).to_be_bytes());
            out.extend(next.to_be_bytes());
            out.extend(value);
        }
        // The map: each hunk's place in hunks, the hunks after it.
        let hunks = data.len().div_ceil(hunk as usize);
        let map_at = out.len() as u64;
        out[40..48].copy_from_slice(&map_at.to_be_bytes());
        let first = (out.len() + hunks * 4).div_ceil(hunk as usize);
        for h in 0..hunks {
            out.extend(((first + h) as u32).to_be_bytes());
        }
        out.resize(first * hunk as usize, 0);
        out.extend(data);
        out.resize((first + hunks) * hunk as usize, 0);
        out
    }

    #[test]
    fn a_chd_is_known_by_its_tracks_then_its_sectors() {
        let d = temp("chd");
        let frames = |raw: &[u8]| {
            let mut out = Vec::new();
            for s in raw.chunks(2352) {
                out.extend(s);
                out.extend([0u8; 96]);
            }
            out
        };
        let track = |kind: &str, n: usize| {
            format!("TRACK:1 TYPE:{kind} SUBTYPE:NONE FRAMES:{n} PREGAP:0 PGTYPE:MODE1 PGSUB:RW POSTGAP:0")
        };
        let saturn = raw(&iso_sectors(&sega(b"SEGA SEGASATURN ", 0x20, b"T-12705H  "), b"SEGA SEGASATURN", ""), 1);
        let file = chd_file(&frames(&saturn), 2448, 2448 * 8, &[(b"CHT2", track("MODE1_RAW", 24))]);
        std::fs::write(d.join("Saturn.chd"), file).unwrap();
        assert_eq!(look(&d.join("Saturn.chd")), Seen::of("saturn", Some("T-12705H".into())));

        let ps1 = raw(&iso_sectors(b"", b"PLAYSTATION", "BOOT = cdrom:\\SLUS_007.08;1\r\n"), 2);
        let file = chd_file(&frames(&ps1), 2448, 2448 * 8, &[(b"CHT2", track("MODE2_RAW", 24))]);
        std::fs::write(d.join("PlayStation.chd"), file).unwrap();
        assert_eq!(look(&d.join("PlayStation.chd")), Seen::of("psx", Some("SLUS-00708".into())));

        let ps2 = iso_sectors(b"", b"PLAYSTATION", "BOOT2 = cdrom0:\\SLES_500.03;1\r\n").concat();
        let file = chd_file(&ps2, 2048, 4096, &[(b"DVD ", String::new())]);
        std::fs::write(d.join("PlayStation 2.chd"), file).unwrap();
        assert_eq!(look(&d.join("PlayStation 2.chd")), Seen::of("ps2", Some("SLES-50003".into())));

        // A DVD that boots no PlayStation 2 game (a PSP's UMD) isn't one.
        let psp = iso_sectors(b"", b"PSP GAME", "").concat();
        let file = chd_file(&psp, 2048, 4096, &[(b"DVD ", String::new())]);
        std::fs::write(d.join("PSP.chd"), file).unwrap();
        assert_eq!(look(&d.join("PSP.chd")), Seen::default());

        // A GD-ROM is a Dreamcast's without a look inside.
        let file = chd_file(&[0u8; 2448 * 8], 2448, 2448 * 8, &[(b"CHGD", track("MODE1_RAW", 8))]);
        std::fs::write(d.join("Dreamcast.chd"), file).unwrap();
        assert_eq!(look(&d.join("Dreamcast.chd")).system, Some("dreamcast"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A CSO of `sectors`, a block each, every other one kept plain.
    fn cso_file(sectors: &[[u8; 2048]]) -> Vec<u8> {
        let mut out = b"CISO".to_vec();
        out.extend(24u32.to_le_bytes());
        out.extend(((sectors.len() * 2048) as u64).to_le_bytes());
        out.extend(2048u32.to_le_bytes());
        out.extend([1, 0, 0, 0]);
        let mut blocks = Vec::new();
        let mut index = Vec::new();
        let start = 24 + (sectors.len() + 1) * 4;
        for (i, s) in sectors.iter().enumerate() {
            let plain = i % 2 == 1;
            index.push((start + blocks.len()) as u32 | if plain { 0x8000_0000 } else { 0 });
            if plain {
                blocks.extend(s);
            } else {
                blocks.extend(miniz_oxide::deflate::compress_to_vec(s, 6));
            }
        }
        index.push((start + blocks.len()) as u32);
        for v in index {
            out.extend(v.to_le_bytes());
        }
        out.extend(blocks);
        out
    }

    #[test]
    fn a_cso_is_read_through_its_blocks() {
        let d = temp("cso");
        let ps2 = iso_sectors(b"", b"PLAYSTATION", "BOOT2 = cdrom0:\\SLUS_212.74;1\r\n");
        std::fs::write(d.join("Katamari Damacy (USA).cso"), cso_file(&ps2)).unwrap();
        assert_eq!(look(&d.join("Katamari Damacy (USA).cso")), Seen::of("ps2", Some("SLUS-21274".into())));
        let psp = iso_sectors(b"", b"PSP GAME", "");
        std::fs::write(d.join("Lumines (USA).cso"), cso_file(&psp)).unwrap();
        assert_eq!(look(&d.join("Lumines (USA).cso")), Seen::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn what_a_disc_is_survives_the_index() {
        for s in [Seen::of("saturn", Some("MK-81009".into())), Seen::of("psx", None), Seen::default()] {
            assert_eq!(Seen::from_fact(&s.fact()), s);
        }
        assert_eq!(boot_serial("BOOT=cdrom:SLPS_012.34;1").as_deref(), Some("SLPS-01234"));
        assert_eq!(boot_serial("BOOT2 = cdrom0:\\SCUS_971.13;1").as_deref(), Some("SCUS-97113"));
        assert_eq!(boot_serial("VMODE = NTSC"), None);
    }
}
