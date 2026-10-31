//! Where the apps module looks: the XDG Base Directory Specification's
//! directories, the desktop names from `XDG_CURRENT_DESKTOP`, and `PATH`.
//!
//! Everything that reads files takes an [`XdgDirs`], so tests (and tools)
//! can point it at a fixture tree instead of the person's home:
//! [`XdgDirs::from_env`] reads the environment once, [`XdgDirs::rooted`]
//! builds the conventional layout under any directory.
//!
//! Conventions, as the Base Directory Specification says: an empty or
//! relative value in an `XDG_*` variable is ignored and the default used
//! instead (`~/.config`, `~/.local/share`, `/etc/xdg`,
//! `/usr/local/share:/usr/share`); relative entries in the lists are
//! dropped.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The desktop name Aizome registers in `XDG_CURRENT_DESKTOP`; it always
/// counts as current for `OnlyShowIn`/`NotShowIn` and desktop-specific
/// `mimeapps.list` files.
pub const DESKTOP_NAME: &str = "Aizome";

/// The XDG base directories and desktop names the apps module reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdgDirs {
    /// `$HOME`, if known.
    pub home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, default `~/.config`.
    pub config_home: PathBuf,
    /// `$XDG_CONFIG_DIRS`, most important first, default `/etc/xdg`.
    pub config_dirs: Vec<PathBuf>,
    /// `$XDG_DATA_HOME`, default `~/.local/share`.
    pub data_home: PathBuf,
    /// `$XDG_DATA_DIRS`, most important first, default
    /// `/usr/local/share:/usr/share`.
    pub data_dirs: Vec<PathBuf>,
    /// The current desktop names, from `XDG_CURRENT_DESKTOP`
    /// (colon-separated, most important first), always including
    /// [`DESKTOP_NAME`].
    pub desktops: Vec<String>,
}

impl XdgDirs {
    /// The directories and desktop names from this process's environment.
    pub fn from_env() -> XdgDirs {
        let home =
            std::env::var_os("HOME").map(PathBuf::from).filter(|home| home.is_absolute()).or_else(home_from_passwd);
        let base = home.clone().unwrap_or_else(|| PathBuf::from("/"));
        let config_home = absolute_var("XDG_CONFIG_HOME").unwrap_or_else(|| base.join(".config"));
        let data_home = absolute_var("XDG_DATA_HOME").unwrap_or_else(|| base.join(".local").join("share"));
        let config_dirs = path_list(std::env::var_os("XDG_CONFIG_DIRS").as_deref(), &["/etc/xdg"]);
        let data_dirs = path_list(std::env::var_os("XDG_DATA_DIRS").as_deref(), &["/usr/local/share", "/usr/share"]);
        let desktops = parse_desktops(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref());
        XdgDirs { home, config_home, config_dirs, data_home, data_dirs, desktops }
    }

    /// The conventional layout under `root`, for tests and tools: home
    /// `root/home`, config `root/home/.config`, data
    /// `root/home/.local/share`, config dirs `[root/etc/xdg]`, data dirs
    /// `[root/usr/local/share, root/usr/share]`, desktop `Aizome`. Nothing
    /// is created.
    pub fn rooted(root: &Path) -> XdgDirs {
        let home = root.join("home");
        XdgDirs {
            config_home: home.join(".config"),
            data_home: home.join(".local").join("share"),
            home: Some(home),
            config_dirs: vec![root.join("etc").join("xdg")],
            data_dirs: vec![root.join("usr/local/share"), root.join("usr/share")],
            desktops: vec![DESKTOP_NAME.to_owned()],
        }
    }

    /// The `applications` directories, most important first:
    /// `$XDG_DATA_HOME/applications`, then each `$XDG_DATA_DIRS` entry's.
    /// Duplicates (the same path listed twice) are dropped.
    pub fn application_dirs(&self) -> Vec<PathBuf> {
        self.data_subdirs("applications")
    }

    /// `sub` under `$XDG_DATA_HOME` and then under each `$XDG_DATA_DIRS`
    /// entry, without duplicates.
    pub(crate) fn data_subdirs(&self, sub: &str) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for base in std::iter::once(&self.data_home).chain(&self.data_dirs) {
            let dir = base.join(sub);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs
    }

    /// `$XDG_CONFIG_HOME` followed by each `$XDG_CONFIG_DIRS` entry.
    pub(crate) fn config_search_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.config_home.clone()];
        for dir in &self.config_dirs {
            if !dirs.contains(dir) {
                dirs.push(dir.clone());
            }
        }
        dirs
    }

    /// The desktop names lowercased, for desktop-specific file names such
    /// as `aizome-mimeapps.list`.
    pub(crate) fn desktop_prefixes(&self) -> Vec<String> {
        let mut prefixes: Vec<String> = Vec::new();
        for desktop in &self.desktops {
            let lower = desktop.to_lowercase();
            if !lower.is_empty() && !prefixes.contains(&lower) {
                prefixes.push(lower);
            }
        }
        prefixes
    }
}

/// The desktop names in an `XDG_CURRENT_DESKTOP` value: colon-separated,
/// empty items dropped, [`DESKTOP_NAME`] appended when missing (and alone
/// when the value is unset or empty).
pub fn parse_desktops(value: Option<&str>) -> Vec<String> {
    let mut desktops: Vec<String> = Vec::new();
    for name in value.unwrap_or("").split(':') {
        let name = name.trim();
        if !name.is_empty() && !desktops.iter().any(|known| known == name) {
            desktops.push(name.to_owned());
        }
    }
    if !desktops.iter().any(|name| name.eq_ignore_ascii_case(DESKTOP_NAME)) {
        desktops.push(DESKTOP_NAME.to_owned());
    }
    desktops
}

fn absolute_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute())
}

/// A colon-separated list of absolute paths; relative entries are invalid
/// and dropped; an unset or empty (or all-invalid) list means `default`.
fn path_list(value: Option<&OsStr>, default: &[&str]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(value) = value {
        for path in std::env::split_paths(value) {
            if path.is_absolute() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if paths.is_empty() {
        paths = default.iter().map(PathBuf::from).collect();
    }
    paths
}

/// The home directory from the password database, for when `HOME` is
/// unset.
fn home_from_passwd() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = vec![0u8; 4096];
    // SAFETY: an all-zero `passwd` is a valid value for getpwuid_r to fill.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer is valid for the call and the buffer's length
    // is passed with it; getpwuid_r writes only into them.
    let status =
        unsafe { libc::getpwuid_r(libc::getuid(), &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut result) };
    if status != 0 || result.is_null() || entry.pw_dir.is_null() {
        return None;
    }
    // SAFETY: on success pw_dir points to a NUL-terminated string inside
    // `buffer`, which is still alive.
    let dir = unsafe { std::ffi::CStr::from_ptr(entry.pw_dir) };
    let path = PathBuf::from(OsStr::from_bytes(dir.to_bytes()));
    path.is_absolute().then_some(path)
}

/// The executable `program` names: itself when it contains a `/` (an
/// absolute path, or relative to the current directory), otherwise the
/// first executable file of that name in `PATH`.
pub fn find_program(program: &str) -> Option<PathBuf> {
    find_program_in(program, std::env::var_os("PATH").as_deref())
}

pub(crate) fn find_program_in(program: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    if program.contains('/') {
        let candidate = PathBuf::from(program);
        return is_executable(&candidate).then_some(candidate);
    }
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

/// Whether `path` is a regular file (after symlinks) with an execute bit.
pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_names_always_include_aizome() {
        assert_eq!(parse_desktops(None), ["Aizome"]);
        assert_eq!(parse_desktops(Some("")), ["Aizome"]);
        assert_eq!(parse_desktops(Some("Hyprland:Tatami")), ["Hyprland", "Tatami", "Aizome"]);
        assert_eq!(parse_desktops(Some("Aizome:GNOME")), ["Aizome", "GNOME"]);
        assert_eq!(parse_desktops(Some("::KDE::KDE")), ["KDE", "Aizome"]);
    }

    #[test]
    fn path_lists_drop_relative_entries() {
        let list = path_list(Some(OsStr::new("/a:relative::/b:/a")), &["/default"]);
        assert_eq!(list, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(path_list(Some(OsStr::new("x:y")), &["/d"]), [PathBuf::from("/d")]);
        assert_eq!(path_list(None, &["/d"]), [PathBuf::from("/d")]);
    }

    #[test]
    fn rooted_layout_and_derived_dirs() {
        let dirs = XdgDirs::rooted(Path::new("/r"));
        assert_eq!(dirs.config_home, Path::new("/r/home/.config"));
        assert_eq!(
            dirs.application_dirs(),
            [
                PathBuf::from("/r/home/.local/share/applications"),
                PathBuf::from("/r/usr/local/share/applications"),
                PathBuf::from("/r/usr/share/applications"),
            ]
        );
        let mut twice = dirs.clone();
        twice.data_dirs.push(PathBuf::from("/r/usr/share"));
        twice.desktops = vec!["KDE".into(), "Aizome".into(), "kde".into()];
        assert_eq!(twice.application_dirs().len(), 3);
        assert_eq!(twice.desktop_prefixes(), ["kde", "aizome"]);
    }

    #[test]
    fn programs_are_found_in_path_or_by_path() {
        let path = OsStr::new("relative:/run/current-system/sw/bin:/usr/bin:/bin");
        let sh = find_program_in("sh", Some(path)).expect("sh in PATH");
        assert!(sh.is_absolute());
        assert_eq!(find_program_in(&sh.to_string_lossy(), None), Some(sh));
        assert_eq!(find_program_in("surely-not-a-program-name", Some(path)), None);
        assert_eq!(find_program_in("", Some(path)), None);
        assert_eq!(find_program_in("/", Some(path)), None);
    }
}
