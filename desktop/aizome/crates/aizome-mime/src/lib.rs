//! File types for the whole desktop, backed by Rust-native `xdg-mime`.
//! The XDG shared database supplies names, magic, aliases, subclasses,
//! icons and translated descriptions; applications have no private tables.
//!
//! Name-only guesses do not open files. Content queries belong on workers.
//! Loaded databases are checked for changes at most once every five seconds.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use xdg_mime::SharedMimeInfo;

pub type Mime = Arc<str>;
pub const DIRECTORY: &str = "inode/directory";
pub const SYMLINK: &str = "inode/symlink";
pub const UNKNOWN: &str = "application/octet-stream";
pub const TEXT: &str = "text/plain";
pub const EMPTY: &str = "application/x-zerosize";

#[derive(Clone, Debug)]
pub struct Guess {
    pub mime: Mime,
    pub uncertain: bool,
}

struct Backend {
    db: SharedMimeInfo,
    checked: Instant,
}

/// A reloadable XDG database and shared string pool.
pub struct MimeDb {
    backend: Mutex<Backend>,
    strings: Mutex<HashMap<String, Mime>>,
}

impl Default for MimeDb {
    fn default() -> Self {
        Self::with_backend(SharedMimeInfo::new())
    }
}

fn parse(value: &str) -> Option<mime::Mime> {
    value.trim().to_ascii_lowercase().parse().ok()
}

fn canonical(db: &SharedMimeInfo, mime: mime::Mime) -> mime::Mime {
    db.unalias_mime_type(&mime).unwrap_or(mime)
}

impl MimeDb {
    fn with_backend(db: SharedMimeInfo) -> Self {
        Self { backend: Mutex::new(Backend { db, checked: Instant::now() }), strings: Mutex::default() }
    }

    /// Use isolated XDG data directories (each containing `mime/`), in
    /// descending precedence, without changing the process environment.
    pub fn from_data_directories(dirs: &[impl AsRef<Path>]) -> Self {
        Self::with_backend(SharedMimeInfo::new_for_directories(dirs))
    }

    pub fn get() -> &'static Self {
        static DB: OnceLock<MimeDb> = OnceLock::new();
        DB.get_or_init(Self::default)
    }

    fn with_db<T>(&self, query: impl FnOnce(&SharedMimeInfo) -> T) -> T {
        let mut backend = self.backend.lock().unwrap_or_else(PoisonError::into_inner);
        if backend.checked.elapsed() >= Duration::from_secs(5) {
            backend.db.reload();
            backend.checked = Instant::now();
        }
        query(&backend.db)
    }

    /// Immediately check for database changes, for example after installing
    /// MIME definitions. Ordinary queries also check periodically.
    pub fn reload(&self) -> bool {
        let mut backend = self.backend.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = backend.db.reload();
        backend.checked = Instant::now();
        changed
    }

    pub fn intern(&self, value: &str) -> Mime {
        let mut strings = self.strings.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(shared) = strings.get(value) {
            return shared.clone();
        }
        let shared: Mime = Arc::from(value);
        strings.insert(value.to_owned(), shared.clone());
        shared
    }

    pub fn guess(&self, name: Option<&Path>, data: Option<&[u8]>) -> Guess {
        let name = name.filter(|p| !p.as_os_str().as_encoded_bytes().contains(&0));
        let (mime, uncertain) = self.with_db(|db| {
            let mut builder = db.guess_mime_type();
            if let Some(name) = name.and_then(Path::file_name) {
                builder.file_name(&name.to_string_lossy());
            }
            if let Some(data) = data {
                builder.data(data);
            }
            let guess = builder.guess();
            (canonical(db, guess.mime_type().clone()).to_string(), guess.uncertain())
        });
        Guess { mime: self.intern(&mime), uncertain }
    }

    /// A tentative type from the name alone; unknown names have no type.
    pub fn by_name(&self, name: &str) -> Option<Mime> {
        let guess = self.guess(Some(Path::new(name)), None);
        (guess.mime.as_ref() != UNKNOWN).then_some(guess.mime)
    }

    pub fn by_content(&self, head: &[u8]) -> Mime {
        self.guess(None, Some(head)).mime
    }

    /// With `read`, inspect metadata and at most 64 KiB of a regular file.
    /// Failed reads retain the name guess; they are never empty content.
    pub fn for_file(&self, path: &Path, read: bool) -> Mime {
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return self.intern(UNKNOWN);
        }
        if read {
            if let Ok(metadata) = fs::metadata(path) {
                let kind = metadata.file_type();
                let special = if kind.is_dir() {
                    Some(DIRECTORY)
                } else if kind.is_fifo() {
                    Some("inode/fifo")
                } else if kind.is_socket() {
                    Some("inode/socket")
                } else if kind.is_block_device() {
                    Some("inode/blockdevice")
                } else if kind.is_char_device() {
                    Some("inode/chardevice")
                } else {
                    None
                };
                if let Some(mime) = special {
                    return self.intern(mime);
                }
                // O_NONBLOCK also prevents a race replacing the regular file
                // with a FIFO from hanging a detection worker.
                if kind.is_file()
                    && let Ok(file) = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path)
                    && let Ok(current) = file.metadata()
                    && current.is_file()
                {
                    if current.len() == 0 {
                        return self.intern(EMPTY);
                    }
                    let mut data = Vec::new();
                    if file.take(64 * 1024).read_to_end(&mut data).is_ok() && !data.is_empty() {
                        return self.guess(Some(path), Some(&data)).mime;
                    }
                }
            } else if fs::symlink_metadata(path).is_ok_and(|m| m.is_symlink()) {
                return self.intern(SYMLINK);
            }
        }
        self.guess(Some(path), None).mime
    }

    pub fn canonical(&self, value: &str) -> Mime {
        let Some(mime) = parse(value) else { return self.intern(UNKNOWN) };
        self.intern(&self.with_db(|db| canonical(db, mime).to_string()))
    }

    pub fn equal(&self, a: &str, b: &str) -> bool {
        match (parse(a), parse(b)) {
            (Some(a), Some(b)) => self.with_db(|db| db.mime_type_equal(&a, &b)),
            _ => false,
        }
    }

    pub fn is_a(&self, mime: &str, ancestor: &str) -> bool {
        let (Some(mime), Some(ancestor)) = (parse(mime), parse(ancestor)) else { return false };
        // URI handlers are association keys, not binary file formats.
        if mime.type_() == "x-scheme-handler" || ancestor.type_() == "x-scheme-handler" {
            return mime == ancestor;
        }
        self.with_db(|db| db.mime_type_subclass(&mime, &ancestor))
    }

    pub fn is_text(&self, mime: &str) -> bool {
        self.is_a(mime, TEXT)
    }

    pub fn matches(&self, rule: &str, mime: &str) -> bool {
        let rule = rule.trim().to_ascii_lowercase();
        if rule == "*" || rule == "*/*" {
            return true;
        }
        if let Some(family) = rule.strip_suffix("/*") {
            return self.canonical(mime).split('/').next() == Some(family);
        }
        self.is_a(mime, &rule)
    }

    pub fn describe(&self, mime: &str) -> String {
        match mime {
            DIRECTORY => "Folder".into(),
            UNKNOWN => "Document".into(),
            EMPTY => "Empty document".into(),
            _ => {
                let Some(mime) = parse(mime) else { return "Document".into() };
                let languages = description_languages();
                let preferred: Vec<_> = languages.iter().map(String::as_str).collect();
                self.with_db(|db| {
                    db.lookup_description(&mime, &preferred).unwrap_or_else(|| canonical(db, mime).to_string())
                })
            }
        }
    }

    pub fn kind_name(&self, mime: &str) -> Arc<str> {
        self.intern(&self.describe(mime))
    }

    pub fn generic_icon(&self, mime: &str) -> Option<String> {
        let mime = parse(mime)?;
        self.with_db(|db| db.lookup_generic_icon_name(&canonical(db, mime)))
    }
}

fn description_languages() -> Vec<String> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "C".into());
    if locale == "C" || locale == "POSIX" || locale.starts_with("C.") {
        return vec!["C".into()];
    }
    let mut languages: Vec<String> =
        std::env::var("LANGUAGE").unwrap_or_default().split(':').filter(|v| !v.is_empty()).map(str::to_owned).collect();
    languages.push(locale);
    languages
}

/// File chooser patterns, case-insensitive and Unicode-aware. Invalid
/// patterns are treated literally, so an unfinished bracket still names
/// a file. MIME database glob semantics themselves belong to xdg-mime.
pub fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    // The chooser also accepts the shell's [^…] spelling; `glob` uses
    // [!…]. Only rewrite at a class's opening, leaving literal ^ alone.
    let mut normalized = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        normalized.push(c);
        if c == '[' {
            if chars.peek() == Some(&'^') {
                chars.next();
                normalized.push('!');
            } else if chars.peek() == Some(&'!') {
                normalized.push(chars.next().unwrap());
            }
            // A first ] is a member of the class, not its end.
            if chars.peek() == Some(&']') {
                normalized.push(chars.next().unwrap());
            }
            for member in chars.by_ref() {
                normalized.push(member);
                if member == ']' {
                    break;
                }
            }
        }
    }
    glob::Pattern::new(&normalized).map_or_else(|_| pattern == name, |p| p.matches(&name))
}
