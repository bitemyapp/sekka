//! Files as bytes: telling their encoding (UTF-8, UTF-16 by its byte order
//! mark or its zeros, else Latin-1) and line endings (LF, CRLF, CR), turning
//! them into text with `\n` line breaks and back, and saving atomically
//! (a new file beside the old one, synced, then renamed over it).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// How text is written as bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Encoding {
    #[default]
    Utf8,
    /// UTF-8 starting with a byte order mark.
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// ISO 8859-1: one byte a character, U+0000–U+00FF.
    Latin1,
}

impl Encoding {
    pub const ALL: [Encoding; 5] =
        [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Latin1];

    /// As the menus name it.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Latin1 => "Latin-1 (ISO 8859-1)",
        }
    }

    /// As the status shows it.
    pub fn short(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Latin1 => "Latin-1",
        }
    }
}

/// How lines end in the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
    Cr,
}

impl LineEnding {
    pub const ALL: [LineEnding; 3] = [LineEnding::Lf, LineEnding::Crlf, LineEnding::Cr];

    pub fn short(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF (Unix, macOS)",
            LineEnding::Crlf => "CRLF (Windows)",
            LineEnding::Cr => "CR (Classic Mac OS)",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }
}

/// A file's bytes as text.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    /// With `\n` line breaks only.
    pub text: String,
    pub encoding: Encoding,
    /// The most common of its line endings (LF if it has none).
    pub line_ending: LineEnding,
    /// It had more than one kind (saving makes them all `line_ending`).
    pub mixed: bool,
    /// It has NULs or other control bytes text doesn't (shown anyway).
    pub binary: bool,
}

/// The encoding of `bytes`: a byte order mark, else valid UTF-8, else
/// UTF-16 if every other byte is mostly zero, else Latin-1.
pub fn detect(bytes: &[u8]) -> Encoding {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Encoding::Utf8Bom;
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        return Encoding::Utf16Le;
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return Encoding::Utf16Be;
    }
    // Text in UTF-16 without a mark: ASCII's high bytes are zero.
    let sample = &bytes[..bytes.len().min(8192) & !1];
    if sample.len() >= 4 {
        let pairs = sample.len() / 2;
        let even = sample.iter().step_by(2).filter(|b| **b == 0).count();
        let odd = sample.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
        let utf16 = |zeros: usize, other: usize| zeros * 10 >= pairs * 3 && other * 20 < pairs;
        if bytes.len().is_multiple_of(2) {
            if utf16(odd, even) && decode_utf16(bytes, false).is_some() {
                return Encoding::Utf16Le;
            }
            if utf16(even, odd) && decode_utf16(bytes, true).is_some() {
                return Encoding::Utf16Be;
            }
        }
    }
    if std::str::from_utf8(bytes).is_ok() { Encoding::Utf8 } else { Encoding::Latin1 }
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> Option<String> {
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| if big_endian { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) });
    char::decode_utf16(units).collect::<Result<String, _>>().ok()
}

/// Line endings as `\n`: which kind was most common, and whether there
/// were others.
pub fn normalize_line_endings(text: String) -> (String, LineEnding, bool) {
    let b = text.as_bytes();
    if !b.contains(&b'\r') {
        return (text, LineEnding::Lf, false);
    }
    let (mut crlf, mut lf, mut cr) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    let kinds = [crlf, lf, cr].iter().filter(|n| **n > 0).count();
    let ending = if crlf > lf && crlf >= cr {
        LineEnding::Crlf
    } else if cr > lf && cr > crlf {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(p) = rest.find('\r') {
        out.push_str(&rest[..p]);
        out.push('\n');
        rest = &rest[p + 1..];
        if rest.starts_with('\n') {
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    (out, ending, kinds > 1)
}

/// `bytes` as text, in the encoding given or the one detected.
pub fn decode(bytes: &[u8], encoding: Option<Encoding>) -> Decoded {
    let encoding = encoding.unwrap_or_else(|| detect(bytes));
    let text = match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(bytes);
            match std::str::from_utf8(body) {
                Ok(s) => s.to_owned(),
                Err(_) => String::from_utf8_lossy(body).into_owned(),
            }
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let be = encoding == Encoding::Utf16Be;
            let body =
                if bytes.starts_with(if be { &[0xfe, 0xff] } else { &[0xff, 0xfe] }) { &bytes[2..] } else { bytes };
            let units = body
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| if be { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) });
            char::decode_utf16(units).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
        }
        Encoding::Latin1 => bytes.iter().map(|b| char::from(*b)).collect(),
    };
    let binary =
        text.bytes().take(65536).any(|c| c == 0 || (c < 0x20 && !matches!(c, b'\n' | b'\r' | b'\t' | 0x0c | 0x1b)));
    let (text, line_ending, mixed) = normalize_line_endings(text);
    Decoded { text, encoding, line_ending, mixed, binary }
}

/// A character `encoding` can't hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unencodable {
    pub ch: char,
    /// Its place in the text, in characters.
    pub at: usize,
}

/// Text (with `\n` line breaks) as bytes, with `ending` line breaks.
pub fn encode(text: &str, encoding: Encoding, ending: LineEnding) -> Result<Vec<u8>, Unencodable> {
    let text: std::borrow::Cow<str> = match ending {
        LineEnding::Lf => text.into(),
        e => text.replace('\n', e.as_str()).into(),
    };
    Ok(match encoding {
        Encoding::Utf8 => text.into_owned().into_bytes(),
        Encoding::Utf8Bom => {
            let mut v = Vec::with_capacity(text.len() + 3);
            v.extend_from_slice(&[0xef, 0xbb, 0xbf]);
            v.extend_from_slice(text.as_bytes());
            v
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let be = encoding == Encoding::Utf16Be;
            let mut v = Vec::with_capacity(text.len() * 2 + 2);
            v.extend_from_slice(if be { &[0xfe, 0xff] } else { &[0xff, 0xfe] });
            for u in text.encode_utf16() {
                v.extend_from_slice(&if be { u.to_be_bytes() } else { u.to_le_bytes() });
            }
            v
        }
        Encoding::Latin1 => {
            let mut v = Vec::with_capacity(text.len());
            for (at, ch) in text.chars().enumerate() {
                match u8::try_from(u32::from(ch)) {
                    Ok(b) => v.push(b),
                    Err(_) => return Err(Unencodable { ch, at }),
                }
            }
            v
        }
    })
}

/// What a file on disk was when it was read or written: to tell whether
/// something else has changed it since.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub secs: u64,
    pub nanos: u32,
    pub len: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    let d = m.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(Stamp { secs: d.as_secs(), nanos: d.subsec_nanos(), len: m.len() })
}

/// The file a path names, through symbolic links (so saving keeps a link a
/// link and writes where it points).
pub fn resolve(path: &Path) -> PathBuf {
    match fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) if !dir.as_os_str().is_empty() => {
                fs::canonicalize(dir).map(|d| d.join(name)).unwrap_or_else(|_| path.to_path_buf())
            }
            _ => path.to_path_buf(),
        },
    }
}

/// Write `bytes` to `path` so that it's either the old file or the new one,
/// whole, even if the power goes: a temporary file beside it, synced, with
/// the old file's permissions, renamed over it, and the folder synced.
/// Where the folder can't be written in but the file can, the file is
/// written in place.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = resolve(path);
    let dir = target.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "untitled".into());
    let perms = fs::metadata(&target).ok().map(|m| m.permissions());
    let tmp = dir.join(format!(".{name}.sumi-{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let written: io::Result<()> = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        if let Some(p) = &perms {
            f.set_permissions(p.clone())?;
        }
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &target)?;
        if let Ok(d) = File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    match written {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            if e.kind() == io::ErrorKind::PermissionDenied && target.exists() {
                let mut f = OpenOptions::new().write(true).truncate(true).open(&target)?;
                f.write_all(bytes)?;
                f.sync_all()?;
                return Ok(());
            }
            Err(e)
        }
    }
}

/// The MIME type to tell the recent files list.
pub fn mime_type(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" => "text/markdown",
        "rs" => "text/rust",
        "py" => "text/x-python",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" => "text/x-typescript",
        "c" | "h" => "text/x-csrc",
        "cpp" | "cc" | "cxx" | "hpp" => "text/x-c++src",
        "go" => "text/x-go",
        "nix" => "text/x-nix",
        "sh" | "bash" => "application/x-shellscript",
        "toml" => "application/toml",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        _ => "text/plain",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_round_trip() {
        let text = "Ünïcødé ✓\nline two\n";
        for enc in Encoding::ALL {
            for eol in LineEnding::ALL {
                if enc == Encoding::Latin1 {
                    continue;
                }
                let bytes = encode(text, enc, eol).unwrap();
                let d = decode(&bytes, None);
                assert_eq!(d.text, text, "{enc:?} {eol:?}");
                assert_eq!(d.encoding, enc, "{enc:?} detected");
                assert_eq!(d.line_ending, eol);
                assert!(!d.mixed);
            }
        }
        let latin = encode("café\n", Encoding::Latin1, LineEnding::Crlf).unwrap();
        assert_eq!(latin, b"caf\xe9\r\n");
        let d = decode(&latin, None);
        assert_eq!((d.text.as_str(), d.encoding, d.line_ending), ("café\n", Encoding::Latin1, LineEnding::Crlf));
        assert_eq!(encode("a✓", Encoding::Latin1, LineEnding::Lf), Err(Unencodable { ch: '✓', at: 1 }));
    }

    #[test]
    fn utf16_without_a_mark() {
        let le: Vec<u8> = "hello world\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(detect(&le), Encoding::Utf16Le);
        let be: Vec<u8> = "hello world\n".encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        assert_eq!(detect(&be), Encoding::Utf16Be);
        assert_eq!(decode(&be, None).text, "hello world\n");
    }

    #[test]
    fn mixed_line_endings_take_the_most_common() {
        let (t, e, mixed) = normalize_line_endings("a\r\nb\r\nc\nd\re".to_owned());
        assert_eq!((t.as_str(), e, mixed), ("a\nb\nc\nd\ne", LineEnding::Crlf, true));
        let (t, e, mixed) = normalize_line_endings("one\rtwo\r".to_owned());
        assert_eq!((t.as_str(), e, mixed), ("one\ntwo\n", LineEnding::Cr, false));
    }

    #[test]
    fn binary_is_noticed() {
        assert!(decode(b"\x00\x01\x02ELF", None).binary);
        assert!(!decode(b"plain\ttext\n", None).binary);
    }

    #[test]
    fn saves_are_atomic_and_keep_links_and_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sumi-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("script.sh");
        fs::write(&file, "old").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o750)).unwrap();
        let link = dir.join("link.sh");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        save_atomic(&link, b"new").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o750);
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        save_atomic(&dir.join("new.txt"), b"fresh").unwrap();
        assert_eq!(fs::read_to_string(dir.join("new.txt")).unwrap(), "fresh");
        let _ = fs::remove_dir_all(&dir);
    }
}
