//! Games kept zipped, as cartridge sets come: what's in a zip (from its
//! central directory, which says each file's CRC without unpacking it),
//! and its game unpacked for a core, which takes a file or bytes, never a
//! zip (RetroArch unpacks them itself, before the core sees them).
//!
//! Stored and deflated files only, in zips under 4 GiB: what ROM sets use.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A file in a zip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub crc: u32,
    method: u16,
    packed: u64,
    pub size: u64,
    /// Where its local header is.
    offset: u64,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn bad(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_owned())
}

/// The files in the zip at `path`, read from its central directory.
pub fn entries(path: &Path) -> std::io::Result<Vec<Entry>> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    // The end record is the last 22 bytes, unless a comment follows it.
    let tail = len.min(22 + 65_535);
    f.seek(SeekFrom::Start(len - tail))?;
    let mut t = vec![0; tail as usize];
    f.read_exact(&mut t)?;
    let end = (0..t.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(&t, i) == 0x0605_4b50)
        .ok_or_else(|| bad("not a zip"))?;
    let count = u16_at(&t, end + 10) as usize;
    let size = u32_at(&t, end + 12) as usize;
    let at = u32_at(&t, end + 16) as u64;
    if at + size as u64 > len {
        return Err(bad("a zip cut short"));
    }
    f.seek(SeekFrom::Start(at))?;
    let mut dir = vec![0; size];
    f.read_exact(&mut dir)?;
    let mut out = Vec::with_capacity(count);
    let mut i = 0;
    while out.len() < count && i + 46 <= dir.len() && u32_at(&dir, i) == 0x0201_4b50 {
        let name_len = u16_at(&dir, i + 28) as usize;
        let extra = u16_at(&dir, i + 30) as usize;
        let comment = u16_at(&dir, i + 32) as usize;
        let Some(name) = dir.get(i + 46..i + 46 + name_len) else { break };
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            crc: u32_at(&dir, i + 16),
            method: u16_at(&dir, i + 10),
            packed: u32_at(&dir, i + 20) as u64,
            size: u32_at(&dir, i + 24) as u64,
            offset: u32_at(&dir, i + 42) as u64,
        });
        i += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// The game in a zip: the first file with one of `extensions`, else the
/// largest (a set's zip holds one ROM, sometimes with a readme).
pub fn game_in<'a>(entries: &'a [Entry], extensions: &[&str]) -> Option<&'a Entry> {
    let ext = |e: &Entry| Path::new(&e.name).extension().map(|x| x.to_string_lossy().to_ascii_lowercase());
    entries
        .iter()
        .find(|e| ext(e).is_some_and(|x| extensions.contains(&x.as_str())))
        .or_else(|| entries.iter().filter(|e| !e.name.ends_with('/')).max_by_key(|e| e.size))
}

/// `entry`'s bytes, checked against its CRC.
pub fn read(path: &Path, entry: &Entry) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(entry.offset))?;
    let mut head = [0; 30];
    f.read_exact(&mut head)?;
    if u32_at(&head, 0) != 0x0403_4b50 {
        return Err(bad("a zip's file without its header"));
    }
    let skip = u16_at(&head, 26) as i64 + u16_at(&head, 28) as i64;
    f.seek(SeekFrom::Current(skip))?;
    let mut packed = vec![0; entry.packed as usize];
    f.read_exact(&mut packed)?;
    let bytes = match entry.method {
        0 => packed,
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, entry.size as usize)
            .map_err(|e| bad(&format!("a zip's file won't unpack: {e:?}")))?,
        m => return Err(bad(&format!("a zip packed in a way not read here ({m})"))),
    };
    if crc32fast::hash(&bytes) != entry.crc {
        return Err(bad("a zip's file doesn't match its checksum"));
    }
    Ok(bytes)
}

/// The game in the zip at `path` unpacked into `cache` as itself (a
/// core may go by its extension), and the unpacked games beyond the last
/// `keep` cleared away: they're unpacked again when played.
pub fn unpack(path: &Path, extensions: &[&str], cache: &Path, keep: usize) -> std::io::Result<PathBuf> {
    let all = entries(path)?;
    let entry = game_in(&all, extensions).ok_or_else(|| bad("an empty zip"))?;
    let name = Path::new(&entry.name).file_name().ok_or_else(|| bad("a zip's file without a name"))?;
    // A folder per zip, named for its CRC, so two zips with a file of the
    // same name don't meet.
    let dir = cache.join(format!("{:08x}", entry.crc));
    let out = dir.join(name);
    if std::fs::metadata(&out).is_ok_and(|m| m.len() == entry.size) {
        // Touched, as the most recent.
        let _ = std::fs::File::open(&dir).and_then(|f| f.set_modified(std::time::SystemTime::now()));
    } else {
        let bytes = read(path, entry)?;
        std::fs::create_dir_all(&dir)?;
        let part = dir.join(".unpacking");
        std::fs::write(&part, &bytes)?;
        std::fs::rename(&part, &out)?;
    }
    let mut older: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(cache)?
        .flatten()
        .filter(|e| e.path() != dir)
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    older.sort_by_key(|o| std::cmp::Reverse(o.0));
    for (_, p) in older.into_iter().skip(keep.saturating_sub(1)) {
        let _ = std::fs::remove_dir_all(p);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A zip of `files`, deflated when `deflate`, as a ROM set's are.
    pub fn zip(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
        let (mut out, mut dir) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let packed = if deflate { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
            let method: u16 = if deflate { 8 } else { 0 };
            let crc = crc32fast::hash(data);
            let offset = out.len() as u32;
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend([20, 0, 0, 0]);
            out.extend(method.to_le_bytes());
            out.extend([0; 4]);
            out.extend(crc.to_le_bytes());
            out.extend((packed.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(&packed);
            dir.extend(0x0201_4b50u32.to_le_bytes());
            dir.extend([20, 0, 20, 0, 0, 0]);
            dir.extend(method.to_le_bytes());
            dir.extend([0; 4]);
            dir.extend(crc.to_le_bytes());
            dir.extend((packed.len() as u32).to_le_bytes());
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend((name.len() as u16).to_le_bytes());
            dir.extend([0; 12]);
            dir.extend(offset.to_le_bytes());
            dir.extend(name.as_bytes());
        }
        let at = out.len() as u32;
        let n = files.len() as u16;
        out.extend(&dir);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0; 4]);
        out.extend(n.to_le_bytes());
        out.extend(n.to_le_bytes());
        out.extend((dir.len() as u32).to_le_bytes());
        out.extend(at.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn a_zipped_game_is_found_checked_and_unpacked() {
        let d = std::env::temp_dir().join(format!("chanoma-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let rom: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        for deflate in [false, true] {
            let path = d.join(format!("Kirby (USA) {deflate}.zip"));
            std::fs::write(&path, zip(&[("readme.txt", b"hi"), ("Kirby (USA).sfc", &rom)], deflate)).unwrap();
            let all = entries(&path).unwrap();
            assert_eq!(all.len(), 2);
            let game = game_in(&all, &["sfc", "smc"]).unwrap();
            assert_eq!((game.name.as_str(), game.crc, game.size), ("Kirby (USA).sfc", crc32fast::hash(&rom), 70_000));
            assert_eq!(read(&path, game).unwrap(), rom);
            let out = unpack(&path, &["sfc"], &d.join("cache"), 2).unwrap();
            assert_eq!(out.file_name().unwrap(), "Kirby (USA).sfc");
            assert_eq!(std::fs::read(&out).unwrap(), rom);
        }
        // Without a file of the system's, the largest.
        let path = d.join("Odd.zip");
        std::fs::write(&path, zip(&[("a.txt", b"hi"), ("game.bin", &rom[..300])], true)).unwrap();
        assert_eq!(game_in(&entries(&path).unwrap(), &["sfc"]).unwrap().name, "game.bin");
        // Two kept: the oldest goes.
        let cache = d.join("cache");
        let a = unpack(&path, &["sfc"], &cache, 2).unwrap();
        let other = d.join("Other.zip");
        std::fs::write(&other, zip(&[("Other.sfc", &rom[..999])], false)).unwrap();
        std::fs::File::open(a.parent().unwrap())
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        let b = unpack(&other, &["sfc"], &cache, 2).unwrap();
        assert!(b.is_file());
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 2);
        assert!(!a.exists(), "the oldest of three cleared");
        // Damaged: refused, not played.
        let mut broken = zip(&[("Kirby (USA).sfc", &rom)], false);
        broken[100] ^= 0xff;
        std::fs::write(d.join("Broken.zip"), &broken).unwrap();
        let path = d.join("Broken.zip");
        assert!(read(&path, &entries(&path).unwrap()[0]).is_err());
        assert!(entries(&d.join("missing.zip")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
