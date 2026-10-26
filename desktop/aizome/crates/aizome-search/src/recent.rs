//! Recently used files, from the freedesktop "recently used" list that GTK,
//! Qt (through KDE's integration) and Aizome's own apps write:
//! `$XDG_DATA_HOME/recently-used.xbel`.
//!
//! Only what the palette needs is read (each bookmark's URI and when it was
//! last visited or modified), with a scanner rather than an XML parser: the
//! file is machine-written and flat.

use std::path::{Path, PathBuf};

/// A recently used file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentFile {
    pub path: PathBuf,
    /// Seconds since the epoch of the last visit (or modification).
    pub at: u64,
}

/// The default list's path.
pub fn default_path() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("recently-used.xbel")
}

/// Reads the list, newest first, keeping only local files that still exist.
pub fn load(path: impl AsRef<Path>) -> Vec<RecentFile> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut files = parse(&text);
    files.retain(|f| f.path.exists());
    files
}

/// Parses the list's text, newest first.
pub fn parse(text: &str) -> Vec<RecentFile> {
    let mut out = Vec::new();
    for chunk in text.split("<bookmark ").skip(1) {
        let head = chunk.split('>').next().unwrap_or("");
        let Some(href) = attribute(head, "href") else { continue };
        let Some(path) = href.strip_prefix("file://").map(percent_decode) else { continue };
        let at = ["visited", "modified", "added"]
            .iter()
            .filter_map(|name| attribute(head, name).and_then(parse_iso8601))
            .max()
            .unwrap_or(0);
        out.push(RecentFile { path: PathBuf::from(unescape_xml(&path)), at });
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.at));
    out.dedup_by(|a, b| a.path == b.path);
    out
}

fn attribute<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let start = head.find(&key)? + key.len();
    let end = head[start..].find('"')? + start;
    Some(&head[start..end])
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape_xml(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// `2026-10-17T15:04:05.123456Z` (or without fraction) to epoch seconds.
fn parse_iso8601(s: &str) -> Option<u64> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let time = time.trim_end_matches('Z');
    let time = time.split('.').next()?;
    let mut t = time.split(':').map(|p| p.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    let days = days_from_civil(y, m, day);
    u64::try_from(days * 86400 + hh * 3600 + mm * 60 + ss).ok()
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bookmarks_newest_first() {
        let xbel = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0">
  <bookmark href="file:///home/c/old%20notes.md" added="2026-01-01T00:00:00Z" modified="2026-01-02T00:00:00Z" visited="2026-01-03T00:00:00.5Z">
    <info/>
  </bookmark>
  <bookmark href="file:///home/c/r%C3%A9sum%C3%A9.pdf" added="2026-10-01T10:00:00Z" modified="2026-10-01T10:00:00Z" visited="2026-10-16T08:30:00Z"/>
  <bookmark href="https://example.com/" added="2026-10-01T10:00:00Z"/>
</xbel>"#;
        let files = parse(xbel);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("/home/c/résumé.pdf"));
        assert_eq!(files[1].path, PathBuf::from("/home/c/old notes.md"));
        assert_eq!(files[1].at, parse_iso8601("2026-01-03T00:00:00Z").unwrap());
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(parse_iso8601("2026-10-17T00:00:00Z"), Some(days_from_civil(2026, 10, 17) as u64 * 86400));
    }
}
