//! Starting things: apps through `aizome-sys::apps` (in systemd user scopes)
//! with an xdg-activation token so their first window takes the focus,
//! files with their default app, folders in Hako, Settings at a pane.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aizome_sys::apps::{App, AppIndex, LaunchOptions};

/// Hako, Aizome's file manager.
pub const HAKO: &str = "org.aizome.Hako";
pub const SETTEI: &str = "org.aizome.Settei";
pub const TANSU: &str = "org.aizome.Tansu";

/// What to start once an activation token comes.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// An app by its desktop entry, opening `targets`.
    App { app_id: String, targets: Vec<String> },
    /// A command line, run directly.
    Argv(Vec<String>),
}

impl Launch {
    /// The app id the token is asked for.
    pub fn app_id(&self) -> Option<&str> {
        match self {
            Launch::App { app_id, .. } => Some(app_id),
            Launch::Argv(_) => None,
        }
    }
}

/// How to open `path`: folders in Hako (if it's installed), files with
/// their default app through `xdg-open`.
pub fn open_path(index: Option<&AppIndex>, path: &Path) -> Launch {
    if path.is_dir() && index.is_some_and(|i| i.is_installed(HAKO)) {
        return Launch::App { app_id: HAKO.into(), targets: vec![path.to_string_lossy().into_owned()] };
    }
    Launch::Argv(vec!["xdg-open".into(), path.to_string_lossy().into_owned()])
}

/// Show `path` selected in its folder, in Hako.
pub fn reveal_path(index: Option<&AppIndex>, path: &Path) -> Launch {
    if index.is_some_and(|i| i.is_installed(HAKO)) {
        return Launch::Argv(vec!["aizome-hako".into(), "--select".into(), path.to_string_lossy().into_owned()]);
    }
    let dir = path.parent().unwrap_or(path);
    Launch::Argv(vec!["xdg-open".into(), dir.to_string_lossy().into_owned()])
}

/// Settei at a pane (and a place in it).
pub fn settings(pane: &str, anchor: Option<&str>) -> Launch {
    let mut argv = vec!["aizome-settei".to_owned(), "--pane".to_owned(), pane.to_owned()];
    if let Some(a) = anchor {
        argv.push("--option".into());
        argv.push(a.to_owned());
    }
    Launch::Argv(argv)
}

/// Run `launch` now, with `token` (from xdg-activation) for its first
/// window. Doesn't wait for the program: errors are logged.
pub fn start(index: Option<&AppIndex>, launch: &Launch, token: Option<String>, cwd: Option<PathBuf>) {
    match launch {
        Launch::App { app_id, targets } => {
            let Some(entry) = index.and_then(|i| i.match_window_identity(app_id)) else {
                log::warn!("no app {app_id} to launch");
                return;
            };
            let entry: App = entry.clone();
            let options = LaunchOptions {
                targets: targets.clone(),
                activation_token: token,
                working_dir: cwd,
                ..LaunchOptions::default()
            };
            let id = app_id.clone();
            std::thread::spawn(move || {
                if let Err(e) = aizome_sys::apps::launch(&entry, &options) {
                    log::warn!("launching {id}: {e}");
                }
            });
        }
        Launch::Argv(argv) => spawn(argv, token, cwd),
    }
}

/// Run a command line directly, detached, with the activation token in its
/// environment and the shell socket for kit apps.
pub fn spawn(argv: &[String], token: Option<String>, cwd: Option<PathBuf>) {
    let Some((prog, args)) = argv.split_first() else { return };
    let mut c = Command::new(prog);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit());
    if let Some(t) = token {
        c.env("XDG_ACTIVATION_TOKEN", &t).env("DESKTOP_STARTUP_ID", &t);
    }
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    match c.spawn() {
        // Reap it on a thread so it doesn't linger as a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("{prog}: {e}"),
    }
}

/// The home folder's usual places: (name, path).
pub fn places() -> Vec<(String, PathBuf)> {
    let home = aizome_config::home_dir();
    let mut out = vec![("Home".to_owned(), home.clone())];
    for (key, name) in [
        ("DESKTOP", "Desktop"),
        ("DOCUMENTS", "Documents"),
        ("DOWNLOAD", "Downloads"),
        ("PICTURES", "Pictures"),
        ("MUSIC", "Music"),
        ("VIDEOS", "Videos"),
    ] {
        out.push((name.to_owned(), user_dir(key).unwrap_or_else(|| home.join(name))));
    }
    out
}

/// An XDG user directory (`DESKTOP`, `DOWNLOAD`, …) from `user-dirs.dirs`.
pub fn user_dir(key: &str) -> Option<PathBuf> {
    let home = aizome_config::home_dir();
    let text = std::fs::read_to_string(aizome_config::config_dir().join("user-dirs.dirs")).ok()?;
    let wanted = format!("XDG_{key}_DIR=");
    let line = text.lines().find(|l| l.trim_start().starts_with(&wanted))?;
    let value = line.trim_start()[wanted.len()..].trim().trim_matches('"');
    let path = value.replace("$HOME", &home.to_string_lossy());
    Some(PathBuf::from(path))
}

/// The desktop folder.
pub fn desktop_dir() -> PathBuf {
    user_dir("DESKTOP").unwrap_or_else(|| aizome_config::home_dir().join("Desktop"))
}

/// The trash's files folder (`$XDG_DATA_HOME/Trash/files`).
pub fn trash_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| aizome_config::home_dir().join(".local/share"))
        .join("Trash")
}

/// Whether the trash holds anything.
pub fn trash_full() -> bool {
    std::fs::read_dir(trash_dir().join("files")).is_ok_and(|mut d| d.next().is_some())
}

/// Move a file to the trash (the freedesktop layout: the file under
/// `files/`, a `.trashinfo` under `info/`).
pub fn move_to_trash(path: &Path) -> std::io::Result<PathBuf> {
    let trash = trash_dir();
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let mut target = name.clone();
    let mut n = 1;
    while files.join(&target).exists() || info.join(format!("{target}.trashinfo")).exists() {
        n += 1;
        let p = Path::new(&name);
        target = match (p.file_stem(), p.extension()) {
            (Some(s), Some(e)) => format!("{} {n}.{}", s.to_string_lossy(), e.to_string_lossy()),
            _ => format!("{name} {n}"),
        };
    }
    let now = crate::clock::Local::now();
    let stamp =
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", now.year, now.month, now.day, now.hour, now.minute, now.second);
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    std::fs::write(
        info.join(format!("{target}.trashinfo")),
        format!("[Trash Info]\nPath={}\nDeletionDate={stamp}\n", percent_encode(&abs.to_string_lossy())),
    )?;
    let dest = files.join(&target);
    if std::fs::rename(path, &dest).is_err() {
        // Another file system: leave it, and say so.
        let _ = std::fs::remove_file(info.join(format!("{target}.trashinfo")));
        return Err(std::io::Error::other("the trash is on another disk"));
    }
    Ok(dest)
}

/// Empty the trash: its files and their info, on a thread.
pub fn empty_trash() {
    let trash = trash_dir();
    std::thread::spawn(move || {
        for sub in ["files", "info", "expunged"] {
            let dir = trash.join(sub);
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let _ = if p.is_dir() && !p.is_symlink() {
                        std::fs::remove_dir_all(&p)
                    } else {
                        std::fs::remove_file(&p)
                    };
                }
            }
        }
    });
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_open_at_a_place() {
        assert_eq!(
            settings("keyboard", Some("keyboard.caps")),
            Launch::Argv(vec![
                "aizome-settei".into(),
                "--pane".into(),
                "keyboard".into(),
                "--option".into(),
                "keyboard.caps".into()
            ])
        );
    }

    #[test]
    fn files_open_with_xdg_open_without_an_index() {
        let l = open_path(None, Path::new("/etc/hostname"));
        assert_eq!(l, Launch::Argv(vec!["xdg-open".into(), "/etc/hostname".into()]));
    }

    #[test]
    fn trashing_keeps_a_record() {
        let root = std::env::temp_dir().join(format!("aizome-trash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY: tests in this module don't read XDG_DATA_HOME concurrently.
        unsafe { std::env::set_var("XDG_DATA_HOME", root.join("data")) };
        let f = root.join("a b.txt");
        std::fs::write(&f, "x").unwrap();
        let dest = move_to_trash(&f).unwrap();
        assert!(dest.exists() && !f.exists());
        let info = std::fs::read_to_string(trash_dir().join("info/a b.txt.trashinfo")).unwrap();
        assert!(info.contains("a%20b.txt"));
        assert!(trash_full());
        std::fs::write(&f, "y").unwrap();
        let again = move_to_trash(&f).unwrap();
        assert!(again.ends_with("a b 2.txt"));
        let _ = std::fs::remove_dir_all(&root);
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
    }
}
