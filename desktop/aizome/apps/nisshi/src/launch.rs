//! Apps a desktop launched. Aizome's launcher runs an app in a scope named
//! for it (`app-<launcher>-<app ID>-<random>.scope`, the freedesktop
//! convention GNOME and KDE follow too) and gives its output to the journal
//! itself, under the app's ID or, when the shell watches the launch, an
//! identifier of that launch's own (`aizome-launch-<hex>`). journald then
//! credits the output to whoever opened the stream. Nisshi names such
//! output after the app, from its scope and desktop entry, so that every
//! launch of Koe is Koe's, and keeps each launch apart.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Whether a journal identifier is one launch's own.
pub fn is_launch_id(s: &str) -> bool {
    s.strip_prefix("aizome-launch-").is_some_and(|h| h.len() >= 8 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The app ID an app's scope or service is named for:
/// `app-aizome-org.foo\x2dbar-ff.scope` is `org.foo-bar`'s,
/// `app-gnome-org.gnome.Nautilus@3.service` Nautilus's.
pub fn app_of_unit(unit: &str) -> Option<String> {
    let rest = unit.strip_prefix("app-")?;
    let (body, kind) = rest.rsplit_once('.')?;
    let body = match (kind, body.split_once('@')) {
        ("service", Some((named, _))) => named,
        ("scope", _) => body.rsplit_once('-')?.0,
        _ => return None,
    };
    let (launcher, id) = body.split_once('-')?;
    // Scopes other programs make (a command runner's `app-sekka-work-1-2`)
    // look alike: only launchers known to name the app, or a random part
    // long enough to be one.
    let random = rest.rsplit_once('.')?.0.rsplit(['-', '@']).next().unwrap_or("");
    let known = matches!(launcher, "aizome" | "gnome" | "kde" | "flatpak");
    if !known && !(random.len() >= 8 && random.bytes().all(|b| b.is_ascii_hexdigit())) {
        return None;
    }
    let id = unescape(id);
    (!id.is_empty()).then_some(id)
}

/// systemd's escaping of a unit name's parts: `\x2d` for `-`.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\'
            && b.get(i + 1) == Some(&b'x')
            && let Some(n) = s.get(i + 2..i + 4).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(n);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where desktop entries are, as the XDG base directories say.
fn application_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")));
    let shared = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(shared.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .map(|d| d.join("applications"))
        .collect()
}

/// The `Name` an app's desktop entry gives it, in the first of `dirs` that
/// has it.
pub fn name_in(dirs: &[PathBuf], id: &str) -> Option<String> {
    dirs.iter().find_map(|d| {
        let text = std::fs::read_to_string(d.join(format!("{id}.desktop"))).ok()?;
        let mut in_entry = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
            } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
                return Some(name.trim().to_owned()).filter(|n| !n.is_empty());
            }
        }
        None
    })
}

thread_local! {
    static NAMES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// What to call an app: its desktop entry's name, else a reverse-DNS ID's
/// last part (`org.aizome.Koe` is Koe), else its ID. Looked up once per
/// thread (the reader's).
pub fn app_name(id: &str) -> String {
    NAMES.with(|names| {
        if let Some(n) = names.borrow().get(id) {
            return n.clone();
        }
        let n = name_in(&application_dirs(), id).unwrap_or_else(|| fallback_name(id));
        names.borrow_mut().insert(id.to_owned(), n.clone());
        n
    })
}

fn fallback_name(id: &str) -> String {
    if id.matches('.').count() >= 2 { id.rsplit('.').next().unwrap_or(id).to_owned() } else { id.to_owned() }
}

/// Programs that run others and say nothing of what they ran.
pub fn is_runner(comm: &str) -> bool {
    matches!(comm, "sh" | "bash" | "dash" | "zsh" | "fish" | "cat" | "env" | "systemd-run" | "setsid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_and_scopes() {
        assert!(is_launch_id("aizome-launch-f332d86ed3fac3deb4589d0998c3a08e"));
        assert!(!is_launch_id("aizome-launch-"));
        assert!(!is_launch_id("aizome-launcher"));
        assert!(!is_launch_id("aizome-launch-not-hex!"));
        assert_eq!(app_of_unit(r"app-aizome-org.foo\x2dbar-ff.scope").as_deref(), Some("org.foo-bar"));
        assert_eq!(
            app_of_unit(r"app-aizome-google\x2dchrome-72395e0ce4c25ac6.scope").as_deref(),
            Some("google-chrome")
        );
        assert_eq!(app_of_unit("app-gnome-org.gnome.Nautilus@3.service").as_deref(), Some("org.gnome.Nautilus"));
        assert_eq!(app_of_unit("dbus-:1.2-org.telegram.desktop@0.service"), None);
        assert_eq!(app_of_unit("session-2.scope"), None);
        assert_eq!(app_of_unit("app-sekka-work-127324-192331.scope"), None);
        assert_eq!(
            app_of_unit("app-tool-org.example.Viewer-72395e0ce4c25ac6.scope").as_deref(),
            Some("org.example.Viewer")
        );
        assert_eq!(fallback_name("org.aizome.Koe"), "Koe");
        assert_eq!(fallback_name("google-chrome"), "google-chrome");
        assert!(is_runner("sh") && !is_runner("aizome-koe"));
    }

    #[test]
    fn names_from_desktop_entries() {
        let dir = std::env::temp_dir().join(format!("nisshi-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("org.example.Tool.desktop"),
            "[Desktop Action new]\nName=New Window\n\n[Desktop Entry]\nType=Application\nName[fr]=Outil\nName=Example Tool\n",
        )
        .unwrap();
        assert_eq!(name_in(std::slice::from_ref(&dir), "org.example.Tool").as_deref(), Some("Example Tool"));
        assert_eq!(name_in(std::slice::from_ref(&dir), "org.example.Missing"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
