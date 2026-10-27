//! Names: checking a new one, the next free one ("photo 2.jpg"), a
//! duplicate's ("photo copy.jpg"), temporary ones, and paths made plain.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

/// Double extensions kept whole when names are numbered.
const COMPOUND: &[&str] = &[".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tar.lz", ".tar.lzma", ".tar.Z"];

/// The longest name most filesystems take, in bytes.
pub const NAME_MAX: usize = 255;

/// A name split into its stem and extension: `report.pdf` →
/// (`report`, `.pdf`), `backup.tar.gz` → (`backup`, `.tar.gz`). Hidden
/// names (`.bashrc`) and folders (`is_dir`) have no extension.
pub fn split_ext(name: &OsStr, is_dir: bool) -> (OsString, OsString) {
    let b = name.as_bytes();
    if is_dir {
        return (name.to_os_string(), OsString::new());
    }
    for c in COMPOUND {
        if b.len() > c.len() && b.ends_with(c.as_bytes()) {
            let i = b.len() - c.len();
            return (OsString::from_vec(b[..i].to_vec()), OsString::from_vec(b[i..].to_vec()));
        }
    }
    match b.iter().rposition(|&c| c == b'.') {
        Some(i) if i > 0 && i + 1 < b.len() => {
            (OsString::from_vec(b[..i].to_vec()), OsString::from_vec(b[i..].to_vec()))
        }
        _ => (name.to_os_string(), OsString::new()),
    }
}

/// Why `name` can't be a file's name, if it can't.
pub fn check_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("A name can't be empty.");
    }
    if name.trim().is_empty() {
        return Some("A name needs more than spaces.");
    }
    if name == "." || name == ".." {
        return Some("“.” and “..” are taken by every folder.");
    }
    if name.contains('/') {
        return Some("A name can't contain “/”.");
    }
    if name.contains('\0') {
        return Some("A name can't contain a NUL character.");
    }
    if name.len() > NAME_MAX {
        return Some("That name is too long.");
    }
    None
}

/// Whether `name` would be hidden (it starts with a dot).
pub fn is_hidden_name(name: &OsStr) -> bool {
    name.as_bytes().first() == Some(&b'.')
}

/// `stem` with a trailing " N" taken off, and N (`photo 2` → (`photo`, 2)).
fn numbered(stem: &[u8]) -> (&[u8], u64) {
    if let Some(sp) = stem.iter().rposition(|&c| c == b' ')
        && sp > 0
        && sp + 1 < stem.len()
        && stem[sp + 1..].iter().all(u8::is_ascii_digit)
        && stem[sp + 1] != b'0'
        && let Ok(n) = std::str::from_utf8(&stem[sp + 1..]).unwrap_or("x").parse::<u64>()
    {
        return (&stem[..sp], n);
    }
    (stem, 1)
}

fn join(stem: &[u8], suffix: &str, ext: &[u8]) -> OsString {
    let mut v = stem.to_vec();
    v.extend_from_slice(suffix.as_bytes());
    v.extend_from_slice(ext);
    OsString::from_vec(v)
}

/// Shorten a stem so stem + suffix + ext fits in [`NAME_MAX`] (at a
/// character boundary where the bytes are UTF-8).
fn fit(stem: &[u8], extra: usize) -> &[u8] {
    if stem.len() + extra <= NAME_MAX {
        return stem;
    }
    let mut end = NAME_MAX.saturating_sub(extra).min(stem.len());
    while end > 0 && std::str::from_utf8(&stem[..end]).is_err() && (stem[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    &stem[..end]
}

/// The first of `name`, `name 2`, `name 3`… (before the extension) not
/// taken in `dir`: what "keep both" names the newcomer.
pub fn free_name(dir: &Path, name: &OsStr, is_dir: bool) -> OsString {
    free_name_with(name, is_dir, |n| std::fs::symlink_metadata(dir.join(n)).is_err())
}

/// [`free_name`] with `free` saying which names are free.
pub fn free_name_with(name: &OsStr, is_dir: bool, mut free: impl FnMut(&OsStr) -> bool) -> OsString {
    if free(name) {
        return name.to_os_string();
    }
    let (stem, ext) = split_ext(name, is_dir);
    let (base, n) = numbered(stem.as_bytes());
    for k in (n + 1).max(2).. {
        let suffix = format!(" {k}");
        let candidate = join(fit(base, suffix.len() + ext.len()), &suffix, ext.as_bytes());
        if free(&candidate) {
            return candidate;
        }
    }
    unreachable!("some number is free")
}

/// A duplicate's name in `dir`: `photo copy.jpg`, then `photo copy
/// 2.jpg`…
pub fn duplicate_name(dir: &Path, name: &OsStr, is_dir: bool) -> OsString {
    duplicate_name_with(name, is_dir, |n| std::fs::symlink_metadata(dir.join(n)).is_err())
}

pub fn duplicate_name_with(name: &OsStr, is_dir: bool, mut free: impl FnMut(&OsStr) -> bool) -> OsString {
    let (stem, ext) = split_ext(name, is_dir);
    let stem = stem.as_bytes();
    // A copy of a copy counts on: "photo copy" → "photo copy 2".
    let (base, start) = if stem.ends_with(b" copy") {
        (stem, 2)
    } else {
        match numbered(stem) {
            (b, n) if b.ends_with(b" copy") && n > 1 => (b, n + 1),
            _ => (stem, 0),
        }
    };
    let base: Vec<u8> = if base.ends_with(b" copy") {
        base.to_vec()
    } else {
        let mut b = fit(base, 5 + ext.len() + 4).to_vec();
        b.extend_from_slice(b" copy");
        b
    };
    if start == 0 {
        let c = join(&base, "", ext.as_bytes());
        if free(&c) {
            return c;
        }
    }
    for k in start.max(2).. {
        let suffix = format!(" {k}");
        let c = join(fit(&base, suffix.len() + ext.len()), &suffix, ext.as_bytes());
        if free(&c) {
            return c;
        }
    }
    unreachable!("some number is free")
}

/// The hidden name an item is copied under before it's whole:
/// `.photo.jpg.hako-part-1a2b3c`.
pub fn partial_name(name: &OsStr) -> OsString {
    let word = super::sys::random_word();
    let keep = fit(name.as_bytes(), 1 + 11 + word.len());
    let mut v = b".".to_vec();
    v.extend_from_slice(keep);
    v.extend_from_slice(format!(".hako-part-{word}").as_bytes());
    OsString::from_vec(v)
}

/// The hidden name a replaced item is set aside under until its
/// replacement is in place.
pub fn aside_name(name: &OsStr) -> OsString {
    let word = super::sys::random_word();
    let keep = fit(name.as_bytes(), 1 + 12 + word.len());
    let mut v = b".".to_vec();
    v.extend_from_slice(keep);
    v.extend_from_slice(format!(".hako-aside-{word}").as_bytes());
    OsString::from_vec(v)
}

/// Whether a name is one of the temporary names above (left behind by a
/// crash; lists hide them).
pub fn is_temporary(name: &OsStr) -> bool {
    let b = name.as_bytes();
    b.first() == Some(&b'.') && (contains(b, b".hako-part-") || contains(b, b".hako-aside-"))
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

/// A path made absolute (from the current folder) with `.` and `..`
/// resolved by name, not following links.
pub fn normalize(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
    let mut out = PathBuf::from("/");
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    out
}

/// `~` and `~/…` made into the home folder.
pub fn expand_tilde(text: &str, home: &Path) -> PathBuf {
    if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(text)
    }
}

/// A file name for showing: its bytes as UTF-8, or with replacement
/// characters where they aren't.
pub fn display(name: &OsStr) -> String {
    name.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn os(s: &str) -> &OsStr {
        OsStr::new(s)
    }

    #[test]
    fn splits_extensions() {
        assert_eq!(split_ext(os("report.pdf"), false), (OsString::from("report"), OsString::from(".pdf")));
        assert_eq!(split_ext(os("backup.tar.gz"), false), (OsString::from("backup"), OsString::from(".tar.gz")));
        assert_eq!(split_ext(os(".bashrc"), false), (OsString::from(".bashrc"), OsString::new()));
        assert_eq!(split_ext(os("notes."), false), (OsString::from("notes."), OsString::new()));
        assert_eq!(split_ext(os("v1.2"), true), (OsString::from("v1.2"), OsString::new()));
        assert_eq!(split_ext(os(".tar.gz"), false), (OsString::from(".tar"), OsString::from(".gz")));
    }

    #[test]
    fn numbers_free_names() {
        let taken: HashSet<&str> = ["photo.jpg", "photo 2.jpg", "dir", "a.tar.gz", "x 9"].into_iter().collect();
        let free = |n: &OsStr| !taken.contains(n.to_str().unwrap());
        assert_eq!(free_name_with(os("new.jpg"), false, free), "new.jpg");
        assert_eq!(free_name_with(os("photo.jpg"), false, free), "photo 3.jpg");
        assert_eq!(free_name_with(os("photo 2.jpg"), false, free), "photo 3.jpg");
        assert_eq!(free_name_with(os("dir"), true, free), "dir 2");
        assert_eq!(free_name_with(os("a.tar.gz"), false, free), "a 2.tar.gz");
        assert_eq!(free_name_with(os("x 9"), true, free), "x 10");
    }

    #[test]
    fn names_duplicates() {
        let taken: HashSet<&str> =
            ["photo.jpg", "photo copy.jpg", "notes", "notes copy", "notes copy 2"].into_iter().collect();
        let free = |n: &OsStr| !taken.contains(n.to_str().unwrap());
        assert_eq!(duplicate_name_with(os("photo.jpg"), false, free), "photo copy 2.jpg");
        assert_eq!(duplicate_name_with(os("notes"), true, free), "notes copy 3");
        assert_eq!(duplicate_name_with(os("solo.txt"), false, free), "solo copy.txt");
        assert_eq!(duplicate_name_with(os("photo copy.jpg"), false, free), "photo copy 2.jpg");
    }

    #[test]
    fn long_names_stay_within_the_limit() {
        let long = "é".repeat(127) + ".txt"; // 254 + 4 bytes > 255
        let n = free_name_with(os(&long), false, |n| n != OsStr::new(&long));
        assert!(n.len() <= NAME_MAX, "{}", n.len());
        assert!(n.to_str().is_some(), "cut at a character boundary");
        assert!(n.to_str().unwrap().ends_with(" 2.txt"));
        let d = duplicate_name_with(os(&long), false, |_| true);
        assert!(d.len() <= NAME_MAX && d.to_str().unwrap().ends_with(" copy.txt"));
        assert!(partial_name(os(&long)).len() <= NAME_MAX);
    }

    #[test]
    fn checks_names() {
        assert!(check_name("").is_some());
        assert!(check_name("  ").is_some());
        assert!(check_name("..").is_some());
        assert!(check_name("a/b").is_some());
        assert!(check_name(&"x".repeat(256)).is_some());
        assert!(check_name("fine name.txt").is_none());
        assert!(check_name(".hidden").is_none());
    }

    #[test]
    fn temporary_names_are_recognised() {
        assert!(is_temporary(&partial_name(os("a.txt"))));
        assert!(is_temporary(&aside_name(os("a"))));
        assert!(!is_temporary(os("a.hako-part-x")));
        assert!(!is_temporary(os(".hidden")));
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(normalize(Path::new("/../..")), PathBuf::from("/"));
        assert_eq!(expand_tilde("~/x", Path::new("/home/u")), PathBuf::from("/home/u/x"));
        assert_eq!(expand_tilde("/etc", Path::new("/home/u")), PathBuf::from("/etc"));
    }
}
