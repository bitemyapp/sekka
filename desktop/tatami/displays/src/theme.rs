// SPDX-License-Identifier: MIT OR Apache-2.0
//! The desktop's look for the window, when it has one: a stylesheet named
//! `tatami-displays/style-dark.css` or `style.css` in the user's
//! configuration directory or the system's ($XDG_CONFIG_DIRS, where the
//! Tatami session puts /etc/xdg/tatami first). Loaded over libadwaita's own
//! style, it changes colors, fonts and corners, not the window's layout. A
//! "-dark" one also makes the window dark, as its colors are, whatever the
//! light or dark preference. Without one, the window is plain libadwaita.
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub struct Theme {
    pub path: PathBuf,
    pub dark: bool,
}

/// Configuration directories, the user's first (XDG Base Directory).
pub fn config_dirs(
    config_home: Option<&str>,
    home: Option<&str>,
    config_dirs: Option<&str>,
) -> Vec<PathBuf> {
    let user = config_home
        .filter(|dir| dir.starts_with('/'))
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|home| home.starts_with('/'))
                .map(|home| Path::new(home).join(".config"))
        });
    let system = config_dirs
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or("/etc/xdg")
        .split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(PathBuf::from);
    user.into_iter().chain(system).collect()
}

/// The first stylesheet for the window in `dirs`; in each, a dark one first.
pub fn find(dirs: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Option<Theme> {
    dirs.iter().find_map(|dir| {
        [("style-dark.css", true), ("style.css", false)]
            .into_iter()
            .find_map(|(name, dark)| {
                let path = dir.join("tatami-displays").join(name);
                exists(&path).then_some(Theme { path, dark })
            })
    })
}

pub fn from_environment() -> Option<Theme> {
    let var = |name| std::env::var(name).ok();
    let dirs = config_dirs(
        var("XDG_CONFIG_HOME").as_deref(),
        var("HOME").as_deref(),
        var("XDG_CONFIG_DIRS").as_deref(),
    );
    find(&dirs, Path::is_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_users_stylesheet_comes_before_the_desktops() {
        let dirs = config_dirs(
            None,
            Some("/home/callen"),
            Some("/etc/xdg/tatami:/etc/xdg:relative"),
        );
        assert_eq!(
            dirs,
            ["/home/callen/.config", "/etc/xdg/tatami", "/etc/xdg"].map(PathBuf::from)
        );
        assert_eq!(
            config_dirs(Some("/c"), Some("/h"), None),
            ["/c", "/etc/xdg"].map(PathBuf::from)
        );
        // Tatami's dark stylesheet.
        let tatami =
            |path: &Path| path == Path::new("/etc/xdg/tatami/tatami-displays/style-dark.css");
        assert_eq!(
            find(&dirs, tatami),
            Some(Theme {
                path: "/etc/xdg/tatami/tatami-displays/style-dark.css".into(),
                dark: true
            })
        );
        // The user's own, which follows the light or dark preference.
        let both = |path: &Path| {
            tatami(path) || path == Path::new("/home/callen/.config/tatami-displays/style.css")
        };
        assert_eq!(find(&dirs, both).map(|theme| theme.dark), Some(false));
        // None: plain libadwaita.
        assert_eq!(find(&dirs, |_| false), None);
    }
}
