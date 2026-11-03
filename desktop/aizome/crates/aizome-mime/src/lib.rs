//! File types for the whole desktop. GIO reads shared-mime-info's names,
//! magic, aliases, subclasses, icons and translated descriptions. There
//! are no private extension/signature tables or database parsers here.
//!
//! Name-only guesses do not open files. Content queries belong on workers;
//! GIO handles regular files, directories and unreadable files without
//! treating a failed read as an empty document. No GTK application, main
//! loop or session bus is required.

use gio::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

pub type Mime = Arc<str>;
pub const DIRECTORY: &str = "inode/directory";
pub const SYMLINK: &str = "inode/symlink";
pub const UNKNOWN: &str = "application/octet-stream";
pub const TEXT: &str = "text/plain";
pub const EMPTY: &str = "application/x-zerosize";

/// A name or content guess. Ambiguous names need a content query when
/// opening the file; listings can use the inexpensive tentative type.
#[derive(Clone, Debug)]
pub struct Guess {
    pub mime: Mime,
    pub uncertain: bool,
}

/// A shared string pool around GIO's process-wide, reloadable database.
#[derive(Default)]
pub struct MimeDb {
    strings: Mutex<HashMap<String, Mime>>,
}

impl MimeDb {
    pub fn get() -> &'static Self {
        static DB: OnceLock<MimeDb> = OnceLock::new();
        DB.get_or_init(Self::default)
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
        // Paths and types can arrive through a portal or desktop entry.
        // Reject embedded NULs before gtk-rs converts them to C strings.
        let name = name.filter(|p| !p.as_os_str().as_encoded_bytes().contains(&0));
        if name.is_none() && data.is_none() {
            return Guess { mime: self.intern(UNKNOWN), uncertain: true };
        }
        let (mime, uncertain) = gio::content_type_guess(name, data);
        Guess { mime: self.canonical(&mime), uncertain }
    }

    /// A tentative type from the name alone; unknown names have no type.
    pub fn by_name(&self, name: &str) -> Option<Mime> {
        let guess = self.guess(Some(Path::new(name)), None);
        (guess.mime.as_ref() != UNKNOWN).then_some(guess.mime)
    }

    pub fn by_content(&self, head: &[u8]) -> Mime {
        self.guess(None, Some(head)).mime
    }

    /// With `read`, let GIO use file metadata and content to disambiguate
    /// the name. Otherwise do no file I/O. Failed queries retain a name
    /// guess, or UNKNOWN, never EMPTY.
    pub fn for_file(&self, path: &Path, read: bool) -> Mime {
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return self.intern(UNKNOWN);
        }
        if read
            && let Ok(info) = gio::File::for_path(path).query_info(
                "standard::content-type",
                gio::FileQueryInfoFlags::NONE,
                gio::Cancellable::NONE,
            )
            && let Some(mime) = info.content_type()
        {
            return self.canonical(&mime);
        }
        self.guess(path.file_name().map(Path::new), None).mime
    }

    pub fn canonical(&self, mime: &str) -> Mime {
        if mime.contains('\0') {
            return self.intern(UNKNOWN);
        }
        let lower = mime.trim().to_ascii_lowercase();
        let canonical = gio::content_type_from_mime_type(&lower);
        self.intern(canonical.as_deref().unwrap_or(&lower))
    }

    pub fn equal(&self, a: &str, b: &str) -> bool {
        if a.contains('\0') || b.contains('\0') {
            return false;
        }
        gio::content_type_equals(&self.canonical(a), &self.canonical(b))
    }

    pub fn is_a(&self, mime: &str, ancestor: &str) -> bool {
        if mime.contains('\0') || ancestor.contains('\0') {
            return false;
        }
        let mime = self.canonical(mime);
        let ancestor = self.canonical(ancestor);
        // URI handlers are association keys, not binary file formats.
        // GIO's generic octet-stream ancestry must not select a file
        // viewer as a fallback browser or mail client.
        if mime.starts_with("x-scheme-handler/") || ancestor.starts_with("x-scheme-handler/") {
            return mime == ancestor;
        }
        gio::content_type_is_a(&mime, &ancestor)
    }

    pub fn is_text(&self, mime: &str) -> bool {
        self.is_a(mime, TEXT)
    }

    /// Match a chooser's exact type (including subclasses), family, or *.
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
            _ => gio::content_type_get_description(&self.canonical(mime)).into(),
        }
    }

    pub fn kind_name(&self, mime: &str) -> Arc<str> {
        self.intern(&self.describe(mime))
    }

    pub fn generic_icon(&self, mime: &str) -> Option<String> {
        gio::content_type_get_generic_icon_name(&self.canonical(mime)).map(Into::into)
    }
}

/// File chooser patterns, case-insensitive and Unicode-aware. Invalid
/// patterns are treated literally, so an unfinished bracket still names
/// a file. MIME database glob semantics themselves belong to GIO.
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
