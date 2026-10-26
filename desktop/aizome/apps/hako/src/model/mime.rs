//! What kind of file each is: MIME types from the shared MIME-info
//! database (freedesktop.org's `shared-mime-info`), as every desktop
//! reads it.
//!
//! - **By name**, for listing folders: `globs2` (weighted patterns: a
//!   literal name like `Makefile`, an extension like `*.png`, or a few
//!   wilder patterns), from each `mime/` folder in the XDG data folders.
//!   Reading the name costs nothing, so a folder of 100,000 files lists
//!   as fast as it can be read.
//! - **By content**, for a file the name says nothing about (Quick Look,
//!   Get Info, opening it): the first bytes, for the common signatures,
//!   and whether it's text.
//! - **What it's called**: the type's `comment` ("PNG image") from its
//!   `mime/<type>.xml`, read once when first asked for; `aliases` and
//!   `subclasses` to know a type's family (anything under `text/plain`
//!   shows as text).
//!
//! Without the database (a bare test machine) a small table of common
//! extensions stands in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// A MIME type, shared.
pub type Mime = Arc<str>;

pub const DIRECTORY: &str = "inode/directory";
pub const SYMLINK: &str = "inode/symlink";
pub const UNKNOWN: &str = "application/octet-stream";
pub const TEXT: &str = "text/plain";
pub const EMPTY: &str = "application/x-zerosize";

/// The database, read once.
pub struct MimeDb {
    /// Lower-cased extension (after the last dot, or a compound like
    /// `tar.gz`) → (weight, type), and the case-sensitive ones.
    ext: HashMap<String, (u32, Mime)>,
    ext_cs: HashMap<String, (u32, Mime)>,
    /// Whole names (`Makefile`, `README`).
    literal: HashMap<String, (u32, Mime)>,
    /// Other patterns, best first.
    globs: Vec<(u32, String, Mime, bool)>,
    aliases: HashMap<String, Mime>,
    parents: HashMap<String, Vec<Mime>>,
    generic_icons: HashMap<String, String>,
    dirs: Vec<PathBuf>,
    comments: Mutex<HashMap<String, Option<String>>>,
    names: Mutex<HashMap<String, Arc<str>>>,
    interned: Mutex<HashMap<String, Mime>>,
}

/// The `mime/` folders, most important first.
pub fn mime_dirs() -> Vec<PathBuf> {
    let mut out = vec![crate::xdg_dir("XDG_DATA_HOME", ".local/share").join("mime")];
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for d in dirs.split(':').filter(|d| !d.is_empty()) {
        out.push(Path::new(d).join("mime"));
    }
    for fallback in ["/run/current-system/sw/share/mime", "/usr/share/mime"] {
        out.push(PathBuf::from(fallback));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| d.join("globs2").is_file() && seen.insert(std::fs::canonicalize(d).unwrap_or_else(|_| d.clone())));
    out
}

impl MimeDb {
    /// The process's database.
    pub fn get() -> &'static MimeDb {
        static DB: OnceLock<MimeDb> = OnceLock::new();
        DB.get_or_init(|| MimeDb::load(&mime_dirs()))
    }

    /// The database in `dirs` (most important first).
    pub fn load(dirs: &[PathBuf]) -> MimeDb {
        let mut db = MimeDb {
            ext: HashMap::new(),
            ext_cs: HashMap::new(),
            literal: HashMap::new(),
            globs: Vec::new(),
            aliases: HashMap::new(),
            parents: HashMap::new(),
            generic_icons: HashMap::new(),
            dirs: dirs.to_vec(),
            comments: Mutex::new(HashMap::new()),
            names: Mutex::new(HashMap::new()),
            interned: Mutex::new(HashMap::new()),
        };
        // Least important first, so more important folders overwrite.
        for dir in dirs.iter().rev() {
            if let Ok(text) = std::fs::read_to_string(dir.join("globs2")) {
                db.read_globs2(&text);
            }
            if let Ok(text) = std::fs::read_to_string(dir.join("aliases")) {
                for (a, b) in pairs(&text) {
                    let m = db.intern(b);
                    db.aliases.insert(a.to_owned(), m);
                }
            }
            if let Ok(text) = std::fs::read_to_string(dir.join("subclasses")) {
                for (a, b) in pairs(&text) {
                    let m = db.intern(b);
                    db.parents.entry(a.to_owned()).or_default().push(m);
                }
            }
            if let Ok(text) = std::fs::read_to_string(dir.join("generic-icons")) {
                for line in text.lines() {
                    if let Some((m, icon)) = line.split_once(':') {
                        db.generic_icons.insert(m.to_owned(), icon.to_owned());
                    }
                }
            }
        }
        if db.ext.is_empty() && db.literal.is_empty() {
            db.read_globs2(FALLBACK);
        }
        db.globs.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.len().cmp(&a.1.len())));
        db
    }

    fn read_globs2(&mut self, text: &str) {
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split(':');
            let (Some(w), Some(mime), Some(glob)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let flags = parts.next().unwrap_or("");
            let cs = flags.split(',').any(|f| f == "cs");
            let weight: u32 = w.parse().unwrap_or(50);
            if glob == "__NOGLOBS__" {
                continue;
            }
            let mime = self.intern(mime);
            let better = |m: &HashMap<String, (u32, Mime)>, k: &str| m.get(k).is_none_or(|(w, _)| weight >= *w);
            if let Some(ext) = glob.strip_prefix("*.")
                && !ext.contains(['*', '?', '['])
            {
                if cs {
                    if better(&self.ext_cs, ext) {
                        self.ext_cs.insert(ext.to_owned(), (weight, mime));
                    }
                } else {
                    let k = ext.to_lowercase();
                    if better(&self.ext, &k) {
                        self.ext.insert(k, (weight, mime));
                    }
                }
            } else if !glob.contains(['*', '?', '[']) {
                let k = if cs { glob.to_owned() } else { glob.to_lowercase() };
                if better(&self.literal, &k) {
                    self.literal.insert(k, (weight, mime));
                }
            } else {
                self.globs.push((weight, if cs { glob.to_owned() } else { glob.to_lowercase() }, mime, cs));
            }
        }
    }

    /// One shared copy of each type's name.
    pub fn intern(&self, mime: &str) -> Mime {
        let mut map = self.interned.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(m) = map.get(mime) {
            return m.clone();
        }
        let m: Mime = Arc::from(mime);
        map.insert(mime.to_owned(), m.clone());
        m
    }

    /// The type a file's name says, if it says one.
    pub fn by_name(&self, name: &str) -> Option<Mime> {
        let lower = name.to_lowercase();
        if let Some((_, m)) = self.literal.get(&lower).or_else(|| self.literal.get(name)) {
            return Some(m.clone());
        }
        // The longest extension first: `tar.gz` before `gz`.
        for (i, _) in name.match_indices('.') {
            if i == 0 && !name[1..].contains('.') {
                // A hidden name's dot isn't an extension's (`.bashrc` is
                // a literal, if it's anything).
                continue;
            }
            let ext = &name[i + 1..];
            if ext.is_empty() {
                continue;
            }
            if let Some((_, m)) = self.ext_cs.get(ext).or_else(|| self.ext.get(&ext.to_lowercase())) {
                return Some(m.clone());
            }
        }
        for (_, glob, m, cs) in &self.globs {
            let subject = if *cs { name } else { &lower };
            if glob_match(glob.as_bytes(), subject.as_bytes()) {
                return Some(m.clone());
            }
        }
        None
    }

    /// The type of a file's first bytes, for the common kinds; text if
    /// they look like text.
    pub fn by_content(&self, head: &[u8]) -> Mime {
        self.intern(sniff(head))
    }

    /// A file's type: by its name, else (if `read`) by its first bytes.
    pub fn for_file(&self, path: &Path, read: bool) -> Mime {
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        if let Some(m) = self.by_name(&name) {
            return m;
        }
        if !read {
            return self.intern(UNKNOWN);
        }
        let mut head = [0u8; 512];
        let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        if n == 0 && std::fs::metadata(path).is_ok_and(|m| m.len() == 0) {
            return self.intern(EMPTY);
        }
        self.by_content(&head[..n])
    }

    /// The canonical name of a type (aliases resolved).
    pub fn canonical(&self, mime: &str) -> Mime {
        self.aliases.get(mime).cloned().unwrap_or_else(|| self.intern(mime))
    }

    /// Whether `mime` is `ancestor` or descends from it (`text/x-rust` is
    /// text; every `text/*` is `text/plain`).
    pub fn is_a(&self, mime: &str, ancestor: &str) -> bool {
        let mut stack = vec![self.canonical(mime)];
        let mut seen = 0;
        while let Some(m) = stack.pop() {
            if &*m == ancestor {
                return true;
            }
            seen += 1;
            if seen > 32 {
                return false;
            }
            if let Some(ps) = self.parents.get(&*m) {
                stack.extend(ps.iter().cloned());
            }
            if ancestor == TEXT && m.starts_with("text/") {
                return true;
            }
        }
        false
    }

    /// Whether files of `mime` read as text.
    pub fn is_text(&self, mime: &str) -> bool {
        self.is_a(mime, TEXT)
            || matches!(
                mime,
                "application/json"
                    | "application/xml"
                    | "application/javascript"
                    | "application/x-shellscript"
                    | "application/toml"
                    | "application/x-yaml"
                    | "application/x-desktop"
                    | "application/x-perl"
                    | "application/x-ruby"
                    | "application/sql"
            )
    }

    /// [`describe`](MimeDb::describe), shared: what listings keep for each
    /// item.
    pub fn kind_name(&self, mime: &str) -> Arc<str> {
        if let Some(n) = self.names.lock().unwrap_or_else(PoisonError::into_inner).get(mime) {
            return n.clone();
        }
        let n: Arc<str> = Arc::from(self.describe(mime));
        self.names.lock().unwrap_or_else(PoisonError::into_inner).insert(mime.to_owned(), n.clone());
        n
    }

    /// What people call it: the type's comment ("PNG image"), or a name
    /// made from the type.
    pub fn describe(&self, mime: &str) -> String {
        if mime == DIRECTORY {
            return "Folder".into();
        }
        if let Some(c) = self.comment(mime) {
            return c;
        }
        match mime {
            UNKNOWN => "Document".into(),
            EMPTY => "Empty document".into(),
            _ => {
                let (major, minor) = mime.split_once('/').unwrap_or((mime, ""));
                let minor = minor.trim_start_matches("x-").trim_start_matches("vnd.");
                let kind = match major {
                    "image" => "image",
                    "audio" => "audio",
                    "video" => "video",
                    "text" => "text",
                    "font" => "font",
                    _ => "document",
                };
                format!("{} {kind}", minor.to_uppercase())
            }
        }
    }

    fn comment(&self, mime: &str) -> Option<String> {
        let mime = self.canonical(mime);
        let mut cache = self.comments.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(c) = cache.get(&*mime) {
            return c.clone();
        }
        let found = self
            .dirs
            .iter()
            .find_map(|d| std::fs::read_to_string(d.join(format!("{mime}.xml"))).ok())
            .and_then(|x| xml_comment(&x));
        cache.insert(mime.to_string(), found.clone());
        found
    }

    /// The icon theme's generic icon for the type (`image-x-generic`).
    pub fn generic_icon(&self, mime: &str) -> Option<&str> {
        self.generic_icons.get(mime).map(String::as_str)
    }
}

/// `a b` pairs, one a line.
fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|l| l.split_once(' '))
}

/// The first `<comment>` without a language.
fn xml_comment(xml: &str) -> Option<String> {
    let start = xml.find("<comment>")? + "<comment>".len();
    let end = xml[start..].find("</comment>")? + start;
    let mut s = xml[start..end].to_owned();
    for (from, to) in [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&apos;", "'"), ("&amp;", "&")] {
        s = s.replace(from, to);
    }
    let mut chars = s.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().collect::<String>() + chars.as_str())
}

/// `*`, `?` and `[…]` globbing.
fn glob_match(glob: &[u8], s: &[u8]) -> bool {
    match glob.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| glob_match(rest, &s[i..])),
        Some((b'?', rest)) => !s.is_empty() && glob_match(rest, &s[1..]),
        Some((b'[', rest)) => {
            let Some(close) = rest.iter().position(|&c| c == b']') else { return false };
            let Some((&c, tail)) = s.split_first() else { return false };
            let set = &rest[..close];
            let (negate, set) = match set.split_first() {
                Some((b'!', s)) => (true, s),
                _ => (false, set),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == b'-' {
                    hit |= (set[i]..=set[i + 2]).contains(&c);
                    i += 3;
                } else {
                    hit |= set[i] == c;
                    i += 1;
                }
            }
            hit != negate && glob_match(&rest[close + 1..], tail)
        }
        Some((&g, rest)) => s.first() == Some(&g) && glob_match(rest, &s[1..]),
    }
}

/// A type from the first bytes.
pub fn sniff(head: &[u8]) -> &'static str {
    const SIGS: &[(&[u8], &str)] = &[
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"GIF87a", "image/gif"),
        (b"GIF89a", "image/gif"),
        (b"%PDF-", "application/pdf"),
        (b"PK\x03\x04", "application/zip"),
        (b"\x1f\x8b", "application/gzip"),
        (b"BZh", "application/x-bzip2"),
        (b"\xfd7zXZ\0", "application/x-xz"),
        (b"(\xb5/\xfd", "application/zstd"),
        (b"7z\xbc\xaf\x27\x1c", "application/x-7z-compressed"),
        (b"\x7fELF", "application/x-executable"),
        (b"#!", "application/x-shellscript"),
        (b"OggS", "audio/ogg"),
        (b"fLaC", "audio/flac"),
        (b"ID3", "audio/mpeg"),
        (b"\x1aE\xdf\xa3", "video/webm"),
        (b"BM", "image/bmp"),
        (b"II*\0", "image/tiff"),
        (b"MM\0*", "image/tiff"),
        (b"SQLite format 3\0", "application/vnd.sqlite3"),
        (b"\0asm", "application/wasm"),
    ];
    for (sig, mime) in SIGS {
        if head.starts_with(sig) {
            return mime;
        }
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" {
        match &head[8..12] {
            b"WEBP" => return "image/webp",
            b"WAVE" => return "audio/x-wav",
            b"AVI " => return "video/x-msvideo",
            _ => {}
        }
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return match &head[8..12] {
            b"avif" | b"avis" => "image/avif",
            b"heic" | b"heix" | b"mif1" => "image/heif",
            b"M4A " => "audio/mp4",
            b"qt  " => "video/quicktime",
            _ => "video/mp4",
        };
    }
    if head.is_empty() {
        return EMPTY;
    }
    if looks_like_text(head) { TEXT } else { UNKNOWN }
}

/// No NULs, and valid UTF-8 (a character cut at the end allowed), with
/// few control characters.
pub fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    let valid = match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none() && head.len() - e.valid_up_to() < 4,
    };
    if !valid {
        return false;
    }
    let controls = head.iter().filter(|&&b| b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t' | 0x0c | 0x1b)).count();
    controls * 20 < head.len().max(1)
}

/// What stands in for the database where there's none.
const FALLBACK: &str = "50:image/png:*.png
50:image/jpeg:*.jpg
50:image/jpeg:*.jpeg
50:image/gif:*.gif
50:image/webp:*.webp
50:image/svg+xml:*.svg
50:image/bmp:*.bmp
50:image/tiff:*.tif
50:image/tiff:*.tiff
50:image/avif:*.avif
50:image/heif:*.heic
50:image/x-icon:*.ico
50:text/plain:*.txt
50:text/plain:*.log
50:text/markdown:*.md
50:text/x-rust:*.rs
50:text/x-python:*.py
50:text/x-csrc:*.c
50:text/x-chdr:*.h
50:text/x-c++src:*.cpp
50:text/javascript:*.js
50:application/x-typescript:*.ts
50:text/x-nix:*.nix
50:application/toml:*.toml
50:application/json:*.json
50:application/x-yaml:*.yaml
50:application/x-yaml:*.yml
50:application/x-shellscript:*.sh
50:text/html:*.html
50:text/css:*.css
50:text/csv:*.csv
50:application/xml:*.xml
50:application/pdf:*.pdf
50:application/zip:*.zip
50:application/x-tar:*.tar
50:application/x-compressed-tar:*.tar.gz
50:application/x-compressed-tar:*.tgz
50:application/x-xz-compressed-tar:*.tar.xz
50:application/x-zstd-compressed-tar:*.tar.zst
50:application/gzip:*.gz
50:application/x-7z-compressed:*.7z
50:application/x-cd-image:*.iso
50:audio/mpeg:*.mp3
50:audio/flac:*.flac
50:audio/ogg:*.ogg
50:audio/x-wav:*.wav
50:video/mp4:*.mp4
50:video/x-matroska:*.mkv
50:video/webm:*.webm
50:font/ttf:*.ttf
50:font/otf:*.otf
50:application/x-desktop:*.desktop
50:text/x-makefile:makefile
";

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback() -> MimeDb {
        MimeDb::load(&[])
    }

    #[test]
    fn names_say_types() {
        let db = fallback();
        assert_eq!(db.by_name("cat.PNG").as_deref(), Some("image/png"));
        assert_eq!(db.by_name("backup.tar.gz").as_deref(), Some("application/x-compressed-tar"));
        assert_eq!(db.by_name("notes.gz").as_deref(), Some("application/gzip"));
        assert_eq!(db.by_name("Makefile").as_deref(), Some("text/x-makefile"));
        assert_eq!(db.by_name("README"), None);
        assert_eq!(db.by_name(".hidden"), None);
        assert_eq!(db.by_name(".config.json").as_deref(), Some("application/json"));
        assert!(db.is_text("text/x-rust"));
        assert!(db.is_text("application/json"));
        assert!(!db.is_text("image/png"));
    }

    #[test]
    fn the_real_database_when_there_is_one() {
        let dirs = mime_dirs();
        if dirs.is_empty() {
            return;
        }
        let db = MimeDb::load(&dirs);
        assert_eq!(db.by_name("x.png").as_deref(), Some("image/png"));
        assert_eq!(db.by_name("x.rs").as_deref(), Some("text/rust"));
        assert_eq!(db.describe("image/png"), "PNG image");
        assert!(db.is_text("text/rust"));
        assert!(db.is_a("application/x-shellscript", "text/plain"));
    }

    #[test]
    fn contents_say_types() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(sniff(b"%PDF-1.7"), "application/pdf");
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(sniff(b"hello, world\n"), TEXT);
        assert_eq!(sniff("héllo wörld".as_bytes()), TEXT);
        assert_eq!(sniff(b"\x00\x01\x02binary"), UNKNOWN);
        assert_eq!(sniff(b""), EMPTY);
        // A multi-byte character cut by the read is still text.
        let s = "日本語".as_bytes();
        assert_eq!(sniff(&s[..s.len() - 1]), TEXT);
    }

    #[test]
    fn globs_and_comments() {
        assert!(glob_match(b"*.[ch]", b"x.c"));
        assert!(!glob_match(b"*.[ch]", b"x.o"));
        assert!(glob_match(b"readme*", b"readme.md"));
        assert!(glob_match(b"?ake", b"make"));
        assert!(glob_match(b"[!a]b", b"xb") && !glob_match(b"[!a]b", b"ab"));
        assert_eq!(
            xml_comment("<mime-type><comment>PNG image</comment><comment xml:lang=\"de\">PNG-Bild</comment>"),
            Some("PNG image".into())
        );
        let db = fallback();
        assert_eq!(db.describe(DIRECTORY), "Folder");
        assert_eq!(db.describe("image/x-foo"), "FOO image");
        assert_eq!(db.describe(UNKNOWN), "Document");
    }
}
