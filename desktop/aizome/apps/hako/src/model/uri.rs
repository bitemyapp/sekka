//! `file://` URIs, and lists of them: on the clipboard (`text/uri-list`
//! and GNOME's `x-special/gnome-copied-files`, which file managers read
//! to paste files) and in drags.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// A path's `file://` URI, escaped as GLib escapes it (so thumbnails'
/// names, which hash the URI, agree with other programs').
pub fn file_uri(path: &Path) -> String {
    let mut s = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"!$&'()*+,-./:=@_~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The path a `file://` URI names (`file:///x`, `file://localhost/x`);
/// `None` for other schemes and hosts.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = match rest.strip_prefix("localhost") {
        Some(r) if r.starts_with('/') => r,
        _ => rest,
    };
    if !rest.starts_with('/') {
        return None;
    }
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    Some(PathBuf::from(OsString::from_vec(crate::fsops::trash::decode(rest))))
}

/// `text/uri-list`: one URI a line, CRLF-ended (RFC 2483).
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| file_uri(p) + "\r\n").collect()
}

/// `x-special/gnome-copied-files`: `copy` or `cut`, then a URI a line.
pub fn gnome_copied_files(cut: bool, paths: &[PathBuf]) -> String {
    let mut s = String::from(if cut { "cut" } else { "copy" });
    for p in paths {
        s.push('\n');
        s.push_str(&file_uri(p));
    }
    s
}

/// The files in a `text/uri-list` (or plain text of paths or file URIs,
/// a line each); other URIs and comments are left out.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| if l.starts_with('/') { Some(PathBuf::from(l)) } else { path_of(l) })
        .collect()
}

/// A `gnome-copied-files` text: whether it was cut, and its files.
pub fn parse_gnome_copied_files(text: &str) -> Option<(bool, Vec<PathBuf>)> {
    let mut lines = text.lines();
    let cut = match lines.next()?.trim() {
        "cut" => true,
        "copy" => false,
        _ => return None,
    };
    let files: Vec<PathBuf> = lines.filter_map(|l| path_of(l.trim())).collect();
    Some((cut, files))
}

/// Plain text of the paths, a line each, for programs that take text.
pub fn plain_paths(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_round_trip() {
        let p = Path::new("/home/u/My Files/naïve #1?.txt");
        let uri = file_uri(p);
        assert_eq!(uri, "file:///home/u/My%20Files/na%C3%AFve%20%231%3F.txt");
        assert_eq!(path_of(&uri).as_deref(), Some(p));
        assert_eq!(path_of("file://localhost/etc/hosts").as_deref(), Some(Path::new("/etc/hosts")));
        assert_eq!(path_of("https://example.com/x"), None);
        assert_eq!(path_of("file://otherhost/x"), None);
        // Bytes that aren't UTF-8 survive.
        let raw = PathBuf::from(OsString::from_vec(b"/tmp/\xff\xfe".to_vec()));
        assert_eq!(path_of(&file_uri(&raw)), Some(raw));
    }

    #[test]
    fn lists() {
        let ps = vec![PathBuf::from("/a b"), PathBuf::from("/c")];
        assert_eq!(uri_list(&ps), "file:///a%20b\r\nfile:///c\r\n");
        assert_eq!(parse_uri_list(&uri_list(&ps)), ps);
        assert_eq!(parse_uri_list("# comment\n/plain/path\nhttps://x\n"), vec![PathBuf::from("/plain/path")]);
        let g = gnome_copied_files(true, &ps);
        assert_eq!(g, "cut\nfile:///a%20b\nfile:///c");
        assert_eq!(parse_gnome_copied_files(&g), Some((true, ps.clone())));
        assert_eq!(parse_gnome_copied_files("copy\nfile:///c\n"), Some((false, vec![PathBuf::from("/c")])));
        assert_eq!(parse_gnome_copied_files("hello"), None);
    }
}
