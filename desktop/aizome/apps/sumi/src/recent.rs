//! Recent files, in the freedesktop list every desktop shares
//! (`$XDG_DATA_HOME/recently-used.xbel`): Sumi adds the files it opens and
//! saves there, so the palette and other apps see them, and reads back the
//! ones it added for File ▸ Open Recent.
//!
//! The file is machine-written XML; it's edited as text, leaving every
//! other app's bookmarks as they were, and written atomically.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The default list's path.
pub fn default_path() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("recently-used.xbel")
}

const HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n      xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n      xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n>\n";

/// `file:///…` for a path, percent-encoded as GLib writes it.
pub fn file_uri(path: &Path) -> String {
    let mut s = String::from("file://");
    for b in path.as_os_str().as_encoded_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'!'
            | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'@' => s.push(char::from(*b)),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    escape(&s)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// `2026-10-17T16:31:05.123456Z`.
pub fn iso8601(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_micros()
    )
}

fn set_attr(tag: &str, name: &str, value: &str) -> String {
    let key = format!(" {name}=\"");
    match tag.find(&key) {
        Some(s) => {
            let v = s + key.len();
            let e = tag[v..].find('"').map_or(tag.len(), |e| v + e);
            format!("{}{}{}", &tag[..v], value, &tag[e..])
        }
        None => {
            let end = tag.rfind("/>").or_else(|| tag.rfind('>')).unwrap_or(tag.len());
            format!("{} {name}=\"{value}\"{}", tag[..end].trim_end(), &tag[end..])
        }
    }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let s = tag.find(&key)? + key.len();
    let e = tag[s..].find('"')? + s;
    Some(&tag[s..e])
}

/// `text` (a recently-used.xbel, or nothing) with `path` added or brought
/// up to date as used by `app` now.
pub fn add_to(text: &str, path: &Path, mime: &str, app: &str, exec: &str, now: SystemTime) -> String {
    let t = iso8601(now);
    let uri = file_uri(path);
    let mut text =
        if text.contains("<xbel") && text.contains("</xbel>") { text.to_owned() } else { format!("{HEADER}</xbel>\n") };
    let href = format!("href=\"{uri}\"");
    let app_tag = format!(
        "<bookmark:application name=\"{}\" exec=\"{}\" modified=\"{t}\" count=\"1\"/>",
        escape(app),
        escape(exec)
    );
    if let Some(at) = text.find(&href) {
        let start = text[..at].rfind("<bookmark ").unwrap_or(at);
        let end = text[start..].find("</bookmark>").map(|e| start + e + "</bookmark>".len());
        let head_end = text[start..].find('>').map_or(text.len(), |e| start + e + 1);
        let self_closing = text[..head_end].ends_with("/>");
        let head = set_attr(&set_attr(&text[start..head_end], "modified", &t), "visited", &t);
        match (end, self_closing) {
            (Some(end), false) => {
                let mut body = text[head_end..end].to_owned();
                let app_key = format!("name=\"{}\"", escape(app));
                if let Some(a) = body.find(&app_key) {
                    let s = body[..a].rfind('<').unwrap_or(a);
                    let e = body[s..].find('>').map_or(body.len(), |e| s + e + 1);
                    let old = &body[s..e];
                    let count = attr(old, "count").and_then(|c| c.parse::<u64>().ok()).unwrap_or(0) + 1;
                    let new = set_attr(&set_attr(old, "modified", &t), "count", &count.to_string());
                    body.replace_range(s..e, &new);
                } else if let Some(a) = body.find("</bookmark:applications>") {
                    body.insert_str(a, &format!("  {app_tag}\n          "));
                } else if let Some(a) = body.find("</metadata>") {
                    body.insert_str(a, &format!("  <bookmark:applications>\n            {app_tag}\n          </bookmark:applications>\n        "));
                }
                text.replace_range(start..end, &format!("{head}{body}"));
            }
            _ => {
                // A bare bookmark: give it what we know.
                let open = head.trim_end_matches("/>").trim_end_matches('>').to_owned() + ">";
                let entry = format!("{open}\n{}  </bookmark>", info(mime, &app_tag));
                text.replace_range(start..head_end, &entry);
            }
        }
    } else {
        let entry = format!(
            "  <bookmark {href} added=\"{t}\" modified=\"{t}\" visited=\"{t}\">\n{}  </bookmark>\n",
            info(mime, &app_tag)
        );
        let at = text.rfind("</xbel>").unwrap_or(text.len());
        text.insert_str(at, &entry);
    }
    text
}

fn info(mime: &str, app_tag: &str) -> String {
    format!(
        "    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <mime:mime-type type=\"{}\"/>\n        <bookmark:applications>\n          {app_tag}\n        </bookmark:applications>\n      </metadata>\n    </info>\n",
        escape(mime)
    )
}

/// Add `path` to the list at `list` (atomically; quietly on failure).
pub fn add(list: &Path, path: &Path, mime: &str, app: &str, exec: &str) {
    let old = std::fs::read_to_string(list).unwrap_or_default();
    let new = add_to(&old, path, mime, app, exec, SystemTime::now());
    if let Some(dir) = list.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = crate::files::save_atomic(list, new.as_bytes()) {
        log::warn!("{}: {e}", list.display());
    }
}

/// The files `app` used, newest first (by when it last did).
pub fn used_by(text: &str, app: &str) -> Vec<PathBuf> {
    let key = format!("name=\"{}\"", escape(app));
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for chunk in text.split("<bookmark ").skip(1) {
        let body = chunk.split("</bookmark>").next().unwrap_or(chunk);
        let Some(a) = body.find(&key) else { continue };
        let head = chunk.split('>').next().unwrap_or("");
        let Some(href) = attr(&format!(" {head}"), "href").map(unescape) else { continue };
        let Some(p) = href.strip_prefix("file://") else { continue };
        let path = PathBuf::from(std::ffi::OsString::from(String::from_utf8_lossy(&percent_decode(p)).into_owned()));
        let s = body[..a].rfind('<').unwrap_or(a);
        let tag = body[s..].split('>').next().unwrap_or("");
        let when = attr(tag, "modified").unwrap_or("").to_owned();
        out.push((when, path));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    let mut seen = std::collections::HashSet::new();
    out.into_iter().map(|(_, p)| p).filter(|p| seen.insert(p.clone())).collect()
}

/// Forget `app`'s files: its application entries go, and bookmarks only it
/// used with them.
pub fn forget(text: &str, app: &str) -> String {
    let key = format!("name=\"{}\"", escape(app));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(s) = rest.find("<bookmark ") {
        out.push_str(&rest[..s]);
        let tail = &rest[s..];
        let end = tail
            .find("</bookmark>")
            .map_or_else(|| tail.find("/>").map_or(tail.len(), |e| e + 2), |e| e + "</bookmark>".len());
        let entry = &tail[..end];
        let apps = entry.matches("<bookmark:application ").count();
        if entry.contains(&key) && apps <= 1 {
            // Drop it, and the indentation and line break around it.
            let trimmed = out.trim_end_matches([' ', '\t']).len();
            out.truncate(trimmed);
            rest = tail[end..].strip_prefix('\n').unwrap_or(&tail[end..]);
            continue;
        }
        if entry.contains(&key) {
            // Shared with others: take out only our application line.
            let a = entry.find(&key).unwrap_or(0);
            let ls = entry[..a].rfind('<').unwrap_or(a);
            let le = entry[ls..].find("/>").map_or(entry.len(), |e| ls + e + 2);
            let line_start = entry[..ls].rfind('\n').unwrap_or(ls);
            out.push_str(&entry[..line_start]);
            out.push_str(&entry[le..]);
        } else {
            out.push_str(entry);
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn dates() {
        assert_eq!(iso8601(at(0)), "1970-01-01T00:00:00.000000Z");
        assert_eq!(iso8601(at(1_792_255_865)), "2026-10-17T16:51:05.000000Z");
    }

    #[test]
    fn adds_updates_and_keeps_the_rest() {
        let other = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\" xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\" xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\">\n  <bookmark href=\"file:///home/c/photo.png\" added=\"2026-01-01T00:00:00Z\" modified=\"2026-01-01T00:00:00Z\" visited=\"2026-01-01T00:00:00Z\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <mime:mime-type type=\"image/png\"/>\n        <bookmark:applications>\n          <bookmark:application name=\"Loupe\" exec=\"&apos;loupe %u&apos;\" modified=\"2026-01-01T00:00:00Z\" count=\"1\"/>\n        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n</xbel>\n";
        let p = Path::new("/home/c/my notes/ä.md");
        let t = add_to(other, p, "text/markdown", "Sumi", "'aizome-sumi %u'", at(1_792_255_865));
        assert!(t.contains("photo.png") && t.contains("Loupe"), "other apps' entries stay");
        assert!(t.contains("href=\"file:///home/c/my%20notes/%C3%A4.md\""));
        let t2 = add_to(&t, p, "text/markdown", "Sumi", "'aizome-sumi %u'", at(1_792_255_900));
        assert_eq!(t2.matches("my%20notes").count(), 1, "one bookmark per file");
        assert!(t2.contains("count=\"2\""));
        // The palette's reader sees it.
        let files = aizome_search::recent::parse(&t2);
        assert_eq!(files[0].path, p);
        assert_eq!(used_by(&t2, "Sumi"), vec![p.to_path_buf()]);
        // Another of Loupe's files used by Sumi too.
        let png = Path::new("/home/c/photo.png");
        let t3 = add_to(&t2, png, "image/png", "Sumi", "'aizome-sumi %u'", at(1_792_256_000));
        assert_eq!(used_by(&t3, "Sumi"), vec![png.to_path_buf(), p.to_path_buf()]);
        assert!(t3.contains("name=\"Loupe\""));
        let f = forget(&t3, "Sumi");
        assert!(used_by(&f, "Sumi").is_empty());
        assert!(f.contains("photo.png") && f.contains("Loupe"), "Loupe's bookmark stays: {f}");
        assert!(!f.contains("my%20notes"));
        assert!(f.trim_end().ends_with("</xbel>"));
        // From nothing.
        let fresh = add_to("", p, "text/markdown", "Sumi", "'aizome-sumi %u'", at(5));
        assert!(fresh.starts_with("<?xml") && fresh.trim_end().ends_with("</xbel>"));
        assert_eq!(aizome_search::recent::parse(&fresh).len(), 1);
    }
}
