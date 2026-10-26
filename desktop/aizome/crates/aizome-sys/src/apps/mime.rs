//! Default applications, as the MIME Applications Associations
//! Specification 1.0.1 defines them, and the default terminal, as the
//! xdg-terminal-exec convention (the proposed Default Terminal Execution
//! Specification) defines it.
//!
//! **`mimeapps.list` files**, most important first: for each directory in
//! `$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`, `$XDG_DATA_HOME/applications`
//! and each `$XDG_DATA_DIRS/applications`, the desktop-specific
//! `<desktop>-mimeapps.list` for each name in `XDG_CURRENT_DESKTOP`
//! (lowercased; `aizome` always counts), then `mimeapps.list`.
//! Desktop-specific files only set defaults; `[Added Associations]` and
//! `[Removed Associations]` are read from the plain files.
//!
//! **Associations** of a MIME type (the apps that can open it): walking
//! the directories in order, a plain file's added associations count if
//! the app is installed, its entry comes from that directory or a less
//! important one, and no more important file removed it; its removed
//! associations hide the app from the files and entries after it. Each
//! application directory then contributes its own entries whose `MimeType`
//! lists the type (and those its `mimeinfo.cache` names). The result:
//! added associations, then the rest (by directory, then ID).
//!
//! **The default** is the first ID in the first `[Default Applications]`
//! list that is installed and associated with the type; failing that, the
//! first association. [`AppIndex::set_default`] writes
//! `$XDG_CONFIG_HOME/mimeapps.list`, keeping every other line, putting the
//! app first in `[Added Associations]` (so it is associated, as the
//! specification asks) and dropping it from `[Removed Associations]`; the
//! file is replaced atomically. A link into `/nix/store` there (Home
//! Manager's `xdg.mimeApps`) is refused with [`Error::ManagedBySystem`]
//! rather than replaced.
//!
//! MIME types compare case-insensitively; URL schemes are
//! `x-scheme-handler/<scheme>`. Subclasses (`text/x-csrc` is a
//! `text/plain`) are not followed: that needs the shared MIME database.
//!
//! **Terminals**: `xdg-terminals.list` files (desktop-specific first) in
//! `$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`, then
//! `$XDG_DATA_HOME/xdg-terminal-exec` and each
//! `$XDG_DATA_DIRS/xdg-terminal-exec`; one desktop file ID per line
//! (`id:action` allowed, `#` comments, `-id` excludes, `+id` includes).
//! The default is the first listed installed entry with `TerminalEmulator`
//! in `Categories`, else `foot.desktop`, else the first terminal by name.
//! [`AppIndex::set_role_default`] moves the choice to the top of
//! `$XDG_CONFIG_HOME/xdg-terminals.list`.

use super::exec::{Exec, ExecContext};
use super::index::AppIndex;
use super::keyfile::{self, KeyFile, Line};
use super::xdg::XdgDirs;
use crate::{Error, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const DEFAULTS: &str = "Default Applications";
const ADDED: &str = "Added Associations";
const REMOVED: &str = "Removed Associations";
const TERMINALS: &str = "xdg-terminals.list";

/// A kind of default application, as a Settings panel shows them, with
/// the MIME types each covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefaultRole {
    Browser,
    Mail,
    Terminal,
    FileManager,
    TextEditor,
    ImageViewer,
    VideoPlayer,
    MusicPlayer,
    PdfViewer,
    Calendar,
    ArchiveManager,
}

impl DefaultRole {
    /// Every role, in the order a Settings panel would list them.
    pub const ALL: [DefaultRole; 11] = [
        DefaultRole::Browser,
        DefaultRole::Mail,
        DefaultRole::Terminal,
        DefaultRole::FileManager,
        DefaultRole::TextEditor,
        DefaultRole::ImageViewer,
        DefaultRole::VideoPlayer,
        DefaultRole::MusicPlayer,
        DefaultRole::PdfViewer,
        DefaultRole::Calendar,
        DefaultRole::ArchiveManager,
    ];

    /// The MIME types the role covers, the primary one first; empty for
    /// [`Terminal`](DefaultRole::Terminal), which has none.
    pub fn mime_types(self) -> &'static [&'static str] {
        match self {
            DefaultRole::Browser => {
                &["x-scheme-handler/http", "x-scheme-handler/https", "text/html", "application/xhtml+xml"]
            }
            DefaultRole::Mail => &["x-scheme-handler/mailto"],
            DefaultRole::Terminal => &[],
            DefaultRole::FileManager => &["inode/directory"],
            DefaultRole::TextEditor => &["text/plain"],
            DefaultRole::ImageViewer => &[
                "image/png",
                "image/jpeg",
                "image/gif",
                "image/webp",
                "image/avif",
                "image/bmp",
                "image/tiff",
                "image/svg+xml",
                "image/heif",
                "image/jxl",
                "image/x-portable-pixmap",
            ],
            DefaultRole::VideoPlayer => &[
                "video/mp4",
                "video/x-matroska",
                "video/webm",
                "video/quicktime",
                "video/mpeg",
                "video/ogg",
                "video/x-msvideo",
                "video/x-flv",
                "video/3gpp",
                "video/mp2t",
            ],
            DefaultRole::MusicPlayer => &[
                "audio/mpeg",
                "audio/flac",
                "audio/x-flac",
                "audio/ogg",
                "audio/x-vorbis+ogg",
                "audio/x-opus+ogg",
                "audio/mp4",
                "audio/x-m4a",
                "audio/aac",
                "audio/wav",
                "audio/x-wav",
            ],
            DefaultRole::PdfViewer => &["application/pdf"],
            DefaultRole::Calendar => &["text/calendar"],
            DefaultRole::ArchiveManager => &[
                "application/zip",
                "application/x-tar",
                "application/x-compressed-tar",
                "application/x-bzip-compressed-tar",
                "application/x-xz-compressed-tar",
                "application/x-zstd-compressed-tar",
                "application/gzip",
                "application/x-7z-compressed",
                "application/vnd.rar",
                "application/x-rar",
            ],
        }
    }

    /// The type that decides the role's current default and candidates.
    pub fn primary_mime_type(self) -> Option<&'static str> {
        self.mime_types().first().copied()
    }

    /// An English label ("Web browser").
    pub fn label(self) -> &'static str {
        match self {
            DefaultRole::Browser => "Web browser",
            DefaultRole::Mail => "Mail",
            DefaultRole::Terminal => "Terminal",
            DefaultRole::FileManager => "Files",
            DefaultRole::TextEditor => "Text editor",
            DefaultRole::ImageViewer => "Images",
            DefaultRole::VideoPlayer => "Videos",
            DefaultRole::MusicPlayer => "Music",
            DefaultRole::PdfViewer => "PDF documents",
            DefaultRole::Calendar => "Calendar",
            DefaultRole::ArchiveManager => "Archives",
        }
    }
}

/// The three groups of one `mimeapps.list`, MIME types lowercased.
#[derive(Clone, Debug, Default)]
struct Groups {
    defaults: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

impl Groups {
    fn read(path: &Path) -> Option<Groups> {
        let text = std::fs::read(path).ok()?;
        Some(Groups::parse(&String::from_utf8_lossy(&text)))
    }

    fn parse(text: &str) -> Groups {
        let file = KeyFile::parse(text);
        let group = |name: &str| {
            let mut map = HashMap::new();
            for (key, value) in file.group(name).into_iter().flatten() {
                if !key.contains('[') {
                    map.insert(key.to_ascii_lowercase(), keyfile::split_list(value));
                }
            }
            map
        };
        Groups { defaults: group(DEFAULTS), added: group(ADDED), removed: group(REMOVED) }
    }
}

/// Every MIME and terminal list, read; see the module documentation.
#[derive(Clone, Debug, Default)]
pub(crate) struct MimeData {
    /// `[Default Applications]` of every list, most important first.
    defaults: Vec<HashMap<String, Vec<String>>>,
    /// The plain `mimeapps.list` files under the config directories.
    config: Vec<Groups>,
    /// Each application directory's plain `mimeapps.list` (by level).
    data: Vec<Groups>,
    /// Each application directory's `mimeinfo.cache` (by level).
    caches: Vec<HashMap<String, Vec<String>>>,
    terminals: Vec<String>,
    excluded_terminals: HashSet<String>,
}

/// The MIME and terminal list files under the config directories, for
/// change detection.
pub(crate) fn config_list_paths(dirs: &XdgDirs) -> Vec<PathBuf> {
    let prefixes = dirs.desktop_prefixes();
    let mut paths = Vec::new();
    for dir in dirs.config_search_dirs() {
        for base in ["mimeapps.list", TERMINALS] {
            for prefix in &prefixes {
                paths.push(dir.join(format!("{prefix}-{base}")));
            }
            paths.push(dir.join(base));
        }
    }
    paths
}

impl MimeData {
    pub(crate) fn load(dirs: &XdgDirs, app_dirs: &[PathBuf]) -> MimeData {
        let prefixes = dirs.desktop_prefixes();
        let mut data = MimeData::default();
        for dir in dirs.config_search_dirs() {
            data.read_dir_lists(&dir, &prefixes, None);
        }
        for (level, dir) in app_dirs.iter().enumerate() {
            data.read_dir_lists(dir, &prefixes, Some(level));
            let cache = std::fs::read(dir.join("mimeinfo.cache"))
                .map(|text| {
                    let file = KeyFile::parse(&String::from_utf8_lossy(&text));
                    let mut map: HashMap<String, Vec<String>> = HashMap::new();
                    for (key, value) in file.group("MIME Cache").into_iter().flatten() {
                        map.insert(key.to_ascii_lowercase(), keyfile::split_list(value));
                    }
                    map
                })
                .unwrap_or_default();
            data.caches.push(cache);
        }
        let mut list_dirs = dirs.config_search_dirs();
        list_dirs.extend(dirs.data_subdirs("xdg-terminal-exec"));
        for dir in list_dirs {
            for prefix in &prefixes {
                data.read_terminals(&dir.join(format!("{prefix}-{TERMINALS}")));
            }
            data.read_terminals(&dir.join(TERMINALS));
        }
        data
    }

    fn read_dir_lists(&mut self, dir: &Path, prefixes: &[String], level: Option<usize>) {
        for prefix in prefixes {
            if let Some(groups) = Groups::read(&dir.join(format!("{prefix}-mimeapps.list"))) {
                self.defaults.push(groups.defaults);
            }
        }
        let plain = Groups::read(&dir.join("mimeapps.list"));
        if let Some(groups) = &plain {
            self.defaults.push(groups.defaults.clone());
        }
        match level {
            None => self.config.extend(plain),
            Some(_) => self.data.push(plain.unwrap_or_default()),
        }
    }

    fn read_terminals(&mut self, path: &Path) {
        let Ok(text) = std::fs::read(path) else {
            return;
        };
        for line in String::from_utf8_lossy(&text).lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (exclude, item) = match line.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, line.strip_prefix('+').unwrap_or(line)),
            };
            let id = item.split(':').next().unwrap_or("").trim();
            if id.is_empty() || self.terminals.iter().any(|t| t == id) || self.excluded_terminals.contains(id) {
                continue;
            }
            if exclude {
                self.excluded_terminals.insert(id.to_owned());
            } else {
                self.terminals.push(id.to_owned());
            }
        }
    }

    /// The entries associated with `mime` (lowercase): added associations
    /// first, then the rest; see the module documentation.
    fn associations(&self, index: &AppIndex, mime: &str) -> Vec<usize> {
        let mut removed: HashSet<usize> = HashSet::new();
        let mut seen: HashSet<usize> = HashSet::new();
        let mut added = Vec::new();
        let mut others = Vec::new();
        let mut apply = |groups: &Groups, min_level: usize, removed: &mut HashSet<usize>| {
            for id in groups.added.get(mime).into_iter().flatten() {
                if let Some(i) = index.position(id)
                    && index.installed_at(i)
                    && index.level_of(i).is_some_and(|level| level >= min_level)
                    && !removed.contains(&i)
                    && seen.insert(i)
                {
                    added.push(i);
                }
            }
            for id in groups.removed.get(mime).into_iter().flatten() {
                if let Some(i) = index.position(id)
                    && index.level_of(i).is_some_and(|level| level >= min_level)
                {
                    removed.insert(i);
                }
            }
        };
        for groups in &self.config {
            apply(groups, 0, &mut removed);
        }
        let mut per_level: Vec<Vec<usize>> = Vec::new();
        for (level, groups) in self.data.iter().enumerate() {
            apply(groups, level, &mut removed);
            let cached = self.caches.get(level).and_then(|cache| cache.get(mime)).into_iter().flatten();
            let mut here: Vec<usize> = index
                .listing_mime(mime)
                .iter()
                .copied()
                .chain(cached.filter_map(|id| index.position(id)))
                .filter(|&i| index.level_of(i) == Some(level) && index.installed_at(i) && !removed.contains(&i))
                .collect();
            here.sort_by(|&a, &b| index.entry_at(a).map(|e| &e.id).cmp(&index.entry_at(b).map(|e| &e.id)));
            here.dedup();
            per_level.push(here);
        }
        for i in per_level.into_iter().flatten() {
            if seen.insert(i) {
                others.push(i);
            }
        }
        added.extend(others);
        added
    }

    /// The first installed, associated ID in a `[Default Applications]`
    /// list.
    fn explicit_default(&self, index: &AppIndex, mime: &str, associated: &[usize]) -> Option<usize> {
        self.defaults
            .iter()
            .filter_map(|defaults| defaults.get(mime))
            .flatten()
            .filter_map(|id| index.position(id))
            .find(|i| index.installed_at(*i) && associated.contains(i))
    }
}

impl AppIndex {
    fn id_at(&self, index: usize) -> Option<String> {
        self.entry_at(index).map(|entry| entry.id.clone())
    }

    /// The default application for a MIME type (`text/html`,
    /// `x-scheme-handler/https`), as a desktop file ID; only installed
    /// applications count. Falls back to the most preferred associated
    /// application when no default is set.
    pub fn default_for(&self, mime: &str) -> Option<String> {
        let mime = mime.trim().to_ascii_lowercase();
        let associated = self.mime.associations(self, &mime);
        self.mime
            .explicit_default(self, &mime, &associated)
            .or_else(|| associated.first().copied())
            .and_then(|i| self.id_at(i))
    }

    /// The applications that can open a MIME type, as desktop file IDs:
    /// the default, then added associations, then the others.
    pub fn apps_for(&self, mime: &str) -> Vec<String> {
        let mime = mime.trim().to_ascii_lowercase();
        let mut associated = self.mime.associations(self, &mime);
        if let Some(default) = self.mime.explicit_default(self, &mime, &associated) {
            associated.retain(|&i| i != default);
            associated.insert(0, default);
        }
        associated.into_iter().filter_map(|i| self.id_at(i)).collect()
    }

    /// Make `id` the default application for `mime`, writing
    /// `$XDG_CONFIG_HOME/mimeapps.list`. `id` must be installed.
    pub fn set_default(&mut self, mime: &str, id: &str) -> Result<()> {
        self.set_defaults(&[mime], id)
    }

    fn set_defaults(&mut self, mimes: &[&str], id: &str) -> Result<()> {
        let id = self.installed_id(id)?;
        let mimes = mimes.iter().map(|mime| valid_mime(mime)).collect::<Result<Vec<_>>>()?;
        let path = self.dirs.config_home.join("mimeapps.list");
        edit_file(&path, |editor| {
            for mime in &mimes {
                editor.set(DEFAULTS, mime, Some(&format!("{id};")));
                let mut added = editor.get_list(ADDED, mime);
                added.retain(|other| *other != id);
                added.insert(0, id.clone());
                editor.set(ADDED, mime, Some(&join_list(&added)));
                let mut removed = editor.get_list(REMOVED, mime);
                if removed.contains(&id) {
                    removed.retain(|other| *other != id);
                    let value = (!removed.is_empty()).then(|| join_list(&removed));
                    editor.set(REMOVED, mime, value.as_deref());
                }
            }
        })?;
        self.reload_lists();
        for mime in &mimes {
            if self.default_for(mime).as_deref() != Some(id.as_str()) {
                log::warn!(
                    "apps: {id} is now the default for {mime} in mimeapps.list, but a desktop-specific list overrides it"
                );
            }
        }
        Ok(())
    }

    fn installed_id(&self, id: &str) -> Result<String> {
        self.position(id)
            .filter(|&i| self.installed_at(i))
            .and_then(|i| self.id_at(i))
            .ok_or_else(|| Error::NotFound(format!("no installed application {id}")))
    }

    /// The current default for a role: for MIME roles, the default for
    /// its primary type; for the terminal, see the module documentation.
    pub fn role_default(&self, role: DefaultRole) -> Option<String> {
        match role.primary_mime_type() {
            Some(mime) => self.default_for(mime),
            None => self.default_terminal().and_then(|i| self.id_at(i)),
        }
    }

    /// Make `id` the default for a role: for MIME roles, for its primary
    /// type and every other type of the role the app can open (in one
    /// write); for the terminal, by listing it first in
    /// `$XDG_CONFIG_HOME/xdg-terminals.list`.
    pub fn set_role_default(&mut self, role: DefaultRole, id: &str) -> Result<()> {
        let canonical = self.installed_id(id)?;
        if role == DefaultRole::Terminal {
            if !self.get(&canonical).is_some_and(|entry| entry.has_category("TerminalEmulator")) {
                return Err(Error::InvalidArgument(format!("{canonical} is not a terminal emulator")));
            }
            let path = self.dirs.config_home.join(TERMINALS);
            edit_terminal_list(&path, &canonical)?;
            self.reload_lists();
            return Ok(());
        }
        let mimes: Vec<&str> = role
            .mime_types()
            .iter()
            .enumerate()
            .filter(|(n, mime)| *n == 0 || self.apps_for(mime).contains(&canonical))
            .map(|(_, mime)| *mime)
            .collect();
        self.set_defaults(&mimes, &canonical)
    }

    /// The applications to offer for a role, as desktop file IDs: those
    /// that can open its primary type (or, for the terminal, those with
    /// `TerminalEmulator` in `Categories`) and are shown in menus; the
    /// current default first, even if hidden. Terminals are sorted by
    /// name.
    pub fn role_candidates(&self, role: DefaultRole) -> Vec<String> {
        let default = self.role_default(role);
        let mut ids: Vec<String> = match role.primary_mime_type() {
            Some(mime) => self.apps_for(mime).into_iter().filter(|id| self.is_shown(id)).collect(),
            None => {
                let mut terminals: Vec<usize> = (0..self.all().len())
                    .filter(|&i| self.is_terminal(i) && self.shown_at(i))
                    .filter(|&i| self.entry_at(i).is_some_and(|e| !self.mime.excluded_terminals.contains(&e.id)))
                    .collect();
                terminals.sort_by_cached_key(|&i| self.entry_at(i).map(|e| (e.name.to_lowercase(), e.id.clone())));
                terminals.into_iter().filter_map(|i| self.id_at(i)).collect()
            }
        };
        if let Some(default) = default {
            ids.retain(|id| *id != default);
            ids.insert(0, default);
        }
        ids
    }

    fn is_terminal(&self, index: usize) -> bool {
        self.installed_at(index) && self.entry_at(index).is_some_and(|e| e.has_category("TerminalEmulator"))
    }

    fn default_terminal(&self) -> Option<usize> {
        let listed = self.mime.terminals.iter().filter_map(|id| self.position(id)).find(|&i| self.is_terminal(i));
        listed.or_else(|| self.position("foot.desktop").filter(|&i| self.is_terminal(i))).or_else(|| {
            (0..self.all().len())
                .filter(|&i| self.is_terminal(i) && self.shown_at(i))
                .filter(|&i| self.entry_at(i).is_some_and(|e| !self.mime.excluded_terminals.contains(&e.id)))
                .min_by_key(|&i| self.entry_at(i).map(|e| (e.name.to_lowercase(), e.id.clone())))
        })
    }

    /// The command prefix that runs a program in the default terminal:
    /// its `Exec` (without field codes) followed by its `X-TerminalArgExec`
    /// (default `-e`), ready for [`LaunchOptions::terminal`]. `None` when
    /// no terminal is installed.
    ///
    /// [`LaunchOptions::terminal`]: super::LaunchOptions::terminal
    pub fn terminal_command(&self) -> Option<Vec<String>> {
        let entry = self.entry_at(self.default_terminal()?)?;
        let exec = Exec::parse(entry.exec.as_deref()?).ok()?;
        let context = ExecContext { name: &entry.name, icon: entry.icon.as_deref(), location: entry.path.as_deref() };
        let mut argv = exec.expand(&[], &context);
        let exec_arg = entry.get("X-TerminalArgExec").unwrap_or_else(|| "-e".to_owned());
        if !exec_arg.is_empty() {
            argv.push(exec_arg);
        }
        Some(argv)
    }
}

/// A MIME type fit to write: `type/subtype`, lowercased, no spaces or key
/// file syntax.
fn valid_mime(mime: &str) -> Result<String> {
    let mime = mime.trim().to_ascii_lowercase();
    let valid = mime.split_once('/').is_some_and(|(kind, sub)| !kind.is_empty() && !sub.is_empty())
        && !mime.contains(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '[' | ']' | '=' | ';' | '#'));
    if valid { Ok(mime) } else { Err(Error::InvalidArgument(format!("not a MIME type: {mime:?}"))) }
}

fn join_list(ids: &[String]) -> String {
    let mut value = ids.join(";");
    value.push(';');
    value
}

/// A key file kept line by line, so editing one key leaves every other
/// line (comments, unknown groups, odd formatting) as it was.
#[derive(Clone, Debug, Default)]
pub(crate) struct LineEditor {
    lines: Vec<String>,
}

impl LineEditor {
    pub(crate) fn parse(text: &str) -> LineEditor {
        let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        LineEditor { lines }
    }

    pub(crate) fn text(&self) -> String {
        let mut text = self.lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }

    /// Line numbers of `key` (compared ignoring ASCII case) in every group
    /// named `group`.
    fn find(&self, group: &str, key: &str) -> Vec<usize> {
        let mut current: Option<&str> = None;
        let mut found = Vec::new();
        for (n, line) in self.lines.iter().enumerate() {
            match keyfile::parse_line(line) {
                Line::Group(name) => current = Some(name),
                Line::Entry { key: k, locale: None, .. } if current == Some(group) && k.eq_ignore_ascii_case(key) => {
                    found.push(n)
                }
                _ => {}
            }
        }
        found
    }

    /// The list value of `key` in `group` (the last one, as readers see).
    pub(crate) fn get_list(&self, group: &str, key: &str) -> Vec<String> {
        let Some(&n) = self.find(group, key).last() else {
            return Vec::new();
        };
        match self.lines.get(n).map(|line| keyfile::parse_line(line)) {
            Some(Line::Entry { value, .. }) => keyfile::split_list(value),
            _ => Vec::new(),
        }
    }

    /// Set `key` in `group` to `value` (raw), or remove it with `None`.
    /// An existing line is replaced in place (duplicates removed); a new
    /// key goes at the end of the group's first occurrence, and a new group
    /// at the end of the file.
    pub(crate) fn set(&mut self, group: &str, key: &str, value: Option<&str>) {
        let found = self.find(group, key);
        let Some(value) = value else {
            for &n in found.iter().rev() {
                self.lines.remove(n);
            }
            return;
        };
        let line = format!("{key}={value}");
        if let Some((&first, rest)) = found.split_first() {
            for &n in rest.iter().rev() {
                self.lines.remove(n);
            }
            if let Some(slot) = self.lines.get_mut(first) {
                *slot = line;
            }
            return;
        }
        let header = self.lines.iter().position(|l| keyfile::parse_line(l) == Line::Group(group));
        match header {
            Some(header) => {
                let end = self.lines[header + 1..]
                    .iter()
                    .position(|l| matches!(keyfile::parse_line(l), Line::Group(_)))
                    .map_or(self.lines.len(), |offset| header + 1 + offset);
                let mut at = end;
                while at > header + 1 && self.lines[at - 1].trim().is_empty() {
                    at -= 1;
                }
                self.lines.insert(at, line);
            }
            None => {
                if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    self.lines.push(String::new());
                }
                self.lines.push(format!("[{group}]"));
                self.lines.push(line);
            }
        }
    }
}

/// Read `path` (missing is empty), let `edit` change it, and write it back
/// atomically.
fn edit_file(path: &Path, edit: impl FnOnce(&mut LineEditor)) -> Result<()> {
    let target = write_target(path)?;
    let text = match std::fs::read(&target) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut editor = LineEditor::parse(&text);
    edit(&mut editor);
    write_atomic(&target, editor.text().as_bytes())
}

/// Put `id` first in a terminal list, keeping comments and other entries
/// (dropping other lines for the same ID).
fn edit_terminal_list(path: &Path, id: &str) -> Result<()> {
    let target = write_target(path)?;
    let text = match std::fs::read(&target) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let names_id = |line: &str| {
        let line = line.trim();
        let item = line.strip_prefix(['-', '+']).unwrap_or(line);
        !line.starts_with('#') && item.split(':').next().map(str::trim) == Some(id)
    };
    let mut lines: Vec<&str> = text.lines().filter(|line| !names_id(line)).collect();
    let first_entry = lines.iter().position(|line| !line.trim().is_empty() && !line.trim().starts_with('#'));
    lines.insert(first_entry.unwrap_or(lines.len()), id);
    let mut out = lines.join("\n");
    out.push('\n');
    write_atomic(&target, out.as_bytes())
}

/// Where writing `path` should go: the file itself, or a symlink's
/// target; refused for links into the Nix store (Home Manager's files).
fn write_target(path: &Path) -> Result<PathBuf> {
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    if !is_link {
        return Ok(path.to_owned());
    }
    match std::fs::canonicalize(path) {
        Ok(target) if target.starts_with("/nix/store") => Err(Error::ManagedBySystem(format!(
            "{} links into the Nix store (Home Manager?); change it in the configuration",
            path.display()
        ))),
        Ok(target) => Ok(target),
        // A dangling link: replace it.
        Err(_) => Ok(path.to_owned()),
    }
}

/// Write `contents` to `path` through a temporary file in the same
/// directory and a rename, keeping the old file's permissions.
pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let temp = dir.join(format!(".{name}.{}.tmp", super::random_hex(6)));
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        if let Ok(meta) = std::fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use super::super::entry::Locale;
    use super::super::testutil::TempDir;
    use super::*;

    fn entry(name: &str, mimes: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=prog\nMimeType={mimes}\n{extra}")
    }

    /// A tree with apps in the user and system data dirs and lists at
    /// every level.
    fn fixture() -> TempDir {
        let tmp = TempDir::new("mime");
        let system = "usr/share/applications";
        let local = "usr/local/share/applications";
        let user = "home/.local/share/applications";
        tmp.write(
            &format!("{system}/firefox.desktop"),
            &entry("Firefox", "text/html;x-scheme-handler/http;x-scheme-handler/https;", ""),
        );
        tmp.write(
            &format!("{system}/chromium.desktop"),
            &entry("Chromium", "text/html;x-scheme-handler/http;x-scheme-handler/https;application/xhtml+xml;", ""),
        );
        tmp.write(&format!("{system}/gedit.desktop"), &entry("Gedit", "text/plain;", ""));
        tmp.write(&format!("{system}/kate.desktop"), &entry("Kate", "text/plain;text/x-csrc;", ""));
        tmp.write(&format!("{system}/vim.desktop"), &entry("Vim", "text/plain;", "NoDisplay=true\n"));
        tmp.write(&format!("{system}/gone.desktop"), &entry("Gone", "text/plain;", "TryExec=/no/such/thing\n"));
        tmp.write(&format!("{system}/thunderbird.desktop"), &entry("Thunderbird", "x-scheme-handler/mailto;", ""));
        tmp.write(&format!("{system}/foot.desktop"), &entry("Foot", "", "Categories=System;TerminalEmulator;\n"));
        tmp.write(
            &format!("{system}/kitty.desktop"),
            &entry("Kitty", "", "Categories=TerminalEmulator;\nX-TerminalArgExec=--\n"),
        );
        tmp.write(
            &format!("{system}/xterm.desktop"),
            &entry("XTerm", "", "Categories=TerminalEmulator;\nNoDisplay=true\n"),
        );
        tmp.write(&format!("{system}/mimeinfo.cache"), "[MIME Cache]\nimage/png=viewer.desktop;\n");
        tmp.write(&format!("{system}/viewer.desktop"), &entry("Viewer", "", ""));
        // Distribution defaults, with a desktop-specific override.
        tmp.write(
            &format!("{system}/mimeapps.list"),
            "[Default Applications]\ntext/plain=gedit.desktop\nx-scheme-handler/http=firefox.desktop\n",
        );
        tmp.write(
            &format!("{system}/aizome-mimeapps.list"),
            "[Default Applications]\ntext/plain=kate.desktop;gedit.desktop\n",
        );
        tmp.write(&format!("{local}/local-only.desktop"), &entry("Local Only", "application/x-foo;", ""));
        tmp.write(&format!("{user}/user-editor.desktop"), &entry("User Editor", "", ""));
        tmp
    }

    fn index(tmp: &TempDir) -> AppIndex {
        AppIndex::load_with(XdgDirs::rooted(tmp.path()), Locale::default())
    }

    #[test]
    fn defaults_follow_file_precedence() {
        let tmp = fixture();
        let index = index(&tmp);
        // The desktop-specific list wins over the plain one in the same dir.
        assert_eq!(index.default_for("text/plain").as_deref(), Some("kate.desktop"));
        assert_eq!(index.default_for("Text/Plain").as_deref(), Some("kate.desktop"));
        assert_eq!(index.default_for("x-scheme-handler/http").as_deref(), Some("firefox.desktop"));
        // No default set: the most preferred association.
        assert_eq!(index.default_for("x-scheme-handler/mailto").as_deref(), Some("thunderbird.desktop"));
        assert_eq!(index.default_for("application/x-unknown"), None);
        // From mimeinfo.cache.
        assert_eq!(index.default_for("image/png").as_deref(), Some("viewer.desktop"));
        assert_eq!(
            index.apps_for("text/plain"),
            ["kate.desktop", "gedit.desktop", "vim.desktop"],
            "installed only, the default first"
        );
    }

    #[test]
    fn user_lists_override_and_associate() {
        let tmp = fixture();
        tmp.write(
            "home/.config/mimeapps.list",
            "[Default Applications]\n\
             text/plain=missing.desktop;gone.desktop;user-editor.desktop;gedit.desktop\n\
             [Added Associations]\n\
             text/plain=user-editor.desktop;local-only.desktop\n\
             [Removed Associations]\n\
             text/plain=kate.desktop;\n",
        );
        let index = index(&tmp);
        // Missing and TryExec-failing IDs are skipped; user-editor is
        // associated through Added Associations.
        assert_eq!(index.default_for("text/plain").as_deref(), Some("user-editor.desktop"));
        let apps = index.apps_for("text/plain");
        assert_eq!(apps, ["user-editor.desktop", "local-only.desktop", "gedit.desktop", "vim.desktop"]);
        // Removed associations take kate out, and with it the
        // desktop-specific default.
        assert!(!apps.contains(&"kate.desktop".to_owned()));
    }

    #[test]
    fn data_dir_additions_only_reach_their_own_or_lower_dirs() {
        let tmp = fixture();
        // A list in /usr/local/share/applications can't associate an app
        // from the more important user directory.
        tmp.write(
            "usr/local/share/applications/mimeapps.list",
            "[Added Associations]\ntext/plain=user-editor.desktop;local-only.desktop;firefox.desktop;\n\
             [Removed Associations]\ntext/plain=user-editor.desktop;gedit.desktop;\n",
        );
        let index = index(&tmp);
        let apps = index.apps_for("text/plain");
        assert!(!apps.contains(&"user-editor.desktop".to_owned()));
        assert!(apps.contains(&"local-only.desktop".to_owned()));
        assert!(apps.contains(&"firefox.desktop".to_owned()));
        assert!(!apps.contains(&"gedit.desktop".to_owned()));
    }

    #[test]
    fn setting_a_default_keeps_everything_else() {
        let tmp = fixture();
        let original = "# my associations\n\
                        [Default Applications]\n\
                        x-scheme-handler/http=chromium.desktop\n\
                        Text/Plain = gedit.desktop\n\
                        \n\
                        [X-Custom]\n\
                        keep=me\n\
                        [Removed Associations]\n\
                        text/plain=vim.desktop;kate.desktop;\n";
        tmp.write("home/.config/mimeapps.list", original);
        let mut index = index(&tmp);
        index.set_default("text/plain", "kate").unwrap();
        let written = std::fs::read_to_string(tmp.path().join("home/.config/mimeapps.list")).unwrap();
        assert_eq!(
            written,
            "# my associations\n\
             [Default Applications]\n\
             x-scheme-handler/http=chromium.desktop\n\
             text/plain=kate.desktop;\n\
             \n\
             [X-Custom]\n\
             keep=me\n\
             [Removed Associations]\n\
             text/plain=vim.desktop;\n\
             \n\
             [Added Associations]\n\
             text/plain=kate.desktop;\n"
        );
        assert_eq!(index.default_for("text/plain").as_deref(), Some("kate.desktop"));
        // The write isn't reported as an outside change.
        assert!(!index.refresh_if_changed());
        // Setting it again changes nothing more.
        index.set_default("text/plain", "kate.desktop").unwrap();
        assert_eq!(std::fs::read_to_string(tmp.path().join("home/.config/mimeapps.list")).unwrap(), written);
        // No temporary files left behind.
        let names: Vec<_> =
            std::fs::read_dir(tmp.path().join("home/.config")).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn setting_a_default_creates_the_file_and_validates() {
        let tmp = fixture();
        let mut index = index(&tmp);
        index.set_default("x-scheme-handler/mailto", "thunderbird.desktop").unwrap();
        let written = std::fs::read_to_string(tmp.path().join("home/.config/mimeapps.list")).unwrap();
        assert_eq!(
            written,
            "[Default Applications]\nx-scheme-handler/mailto=thunderbird.desktop;\n\n\
             [Added Associations]\nx-scheme-handler/mailto=thunderbird.desktop;\n"
        );
        assert!(matches!(index.set_default("text/plain", "nope.desktop"), Err(Error::NotFound(_))));
        assert!(matches!(index.set_default("text/plain", "gone.desktop"), Err(Error::NotFound(_))));
        for bad in ["", "text", "text/", "/plain", "text/plain;x", "a b/c", "x=y/z", "[a]/b"] {
            assert!(matches!(index.set_default(bad, "gedit"), Err(Error::InvalidArgument(_))), "{bad}");
        }
    }

    #[test]
    fn a_store_link_is_managed_by_the_system() {
        let tmp = fixture();
        std::fs::create_dir_all(tmp.path().join("home/.config")).unwrap();
        // Any file under /nix/store will do as the link's target, if there
        // is a store; otherwise only the ordinary-link case is checked.
        let store_file = std::fs::read_dir("/nix/store")
            .ok()
            .and_then(|mut dir| dir.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.is_file())));
        let mut index = index(&tmp);
        let link = tmp.path().join("home/.config/mimeapps.list");
        if let Some(store_file) = store_file {
            std::os::unix::fs::symlink(&store_file, &link).unwrap();
            assert!(matches!(index.set_default("text/plain", "gedit"), Err(Error::ManagedBySystem(_))));
            std::fs::remove_file(&link).unwrap();
        }
        // An ordinary link (a dotfiles repository) is written through.
        let real = tmp.write("dotfiles/mimeapps.list", "[Default Applications]\n");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        index.set_default("text/plain", "gedit").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("text/plain=gedit.desktop;"));
    }

    #[test]
    fn roles() {
        let tmp = fixture();
        let mut index = index(&tmp);
        assert_eq!(index.role_default(DefaultRole::Browser).as_deref(), Some("firefox.desktop"));
        assert_eq!(index.role_candidates(DefaultRole::Browser), ["firefox.desktop", "chromium.desktop"]);
        // Hidden apps aren't offered, unless they are the default.
        assert!(!index.role_candidates(DefaultRole::TextEditor).contains(&"vim.desktop".to_owned()));
        index.set_role_default(DefaultRole::Browser, "chromium.desktop").unwrap();
        for mime in DefaultRole::Browser.mime_types() {
            assert_eq!(index.default_for(mime).as_deref(), Some("chromium.desktop"), "{mime}");
        }
        // Only the types the app handles (plus the primary one).
        index.set_role_default(DefaultRole::Browser, "firefox.desktop").unwrap();
        assert_eq!(index.default_for("application/xhtml+xml").as_deref(), Some("chromium.desktop"));
        assert_eq!(index.default_for("text/html").as_deref(), Some("firefox.desktop"));
        index.set_role_default(DefaultRole::TextEditor, "vim.desktop").unwrap();
        assert_eq!(index.role_candidates(DefaultRole::TextEditor)[0], "vim.desktop");
        assert!(DefaultRole::ALL.iter().all(|role| (*role == DefaultRole::Terminal) == role.mime_types().is_empty()));
        assert_eq!(DefaultRole::Mail.primary_mime_type(), Some("x-scheme-handler/mailto"));
        assert_eq!(DefaultRole::Browser.label(), "Web browser");
    }

    #[test]
    fn terminals() {
        let tmp = fixture();
        let mut index = index(&tmp);
        // No lists: foot.
        assert_eq!(index.role_default(DefaultRole::Terminal).as_deref(), Some("foot.desktop"));
        assert_eq!(index.terminal_command().unwrap(), ["prog", "-e"]);
        assert_eq!(index.role_candidates(DefaultRole::Terminal), ["foot.desktop", "kitty.desktop"]);
        // Lists, desktop-specific first; exclusions; actions.
        tmp.write("usr/share/xdg-terminal-exec/xdg-terminals.list", "foot.desktop\n");
        tmp.write(
            "etc/xdg/aizome-xdg-terminals.list",
            "# preferred\n-foot.desktop\nnot-installed.desktop\n+kitty.desktop:new-window\n",
        );
        assert!(index.refresh_if_changed());
        assert_eq!(index.role_default(DefaultRole::Terminal).as_deref(), Some("kitty.desktop"));
        assert_eq!(index.terminal_command().unwrap(), ["prog", "--"]);
        assert_eq!(index.role_candidates(DefaultRole::Terminal), ["kitty.desktop"]);
        // Choosing one writes the user's list.
        tmp.write("home/.config/xdg-terminals.list", "# mine\nkitty.desktop\nxterm.desktop:x\n");
        index.set_role_default(DefaultRole::Terminal, "xterm").unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("home/.config/xdg-terminals.list")).unwrap(),
            "# mine\nxterm.desktop\nkitty.desktop\n"
        );
        assert_eq!(index.role_default(DefaultRole::Terminal).as_deref(), Some("xterm.desktop"));
        assert!(matches!(index.set_role_default(DefaultRole::Terminal, "gedit"), Err(Error::InvalidArgument(_))));
    }

    #[test]
    fn line_editor_edge_cases() {
        let mut editor = LineEditor::parse("");
        editor.set("G", "k", None);
        assert_eq!(editor.text(), "");
        editor.set("G", "k", Some("v"));
        assert_eq!(editor.text(), "[G]\nk=v\n");
        let mut editor = LineEditor::parse("[G]\nk=1\n[H]\nk=2\n[G]\nk=3\nx=y");
        assert_eq!(editor.get_list("G", "k"), ["3"]);
        editor.set("G", "k", Some("4"));
        assert_eq!(editor.text(), "[G]\nk=4\n[H]\nk=2\n[G]\nx=y\n");
        editor.set("H", "new", Some("n"));
        assert_eq!(editor.text(), "[G]\nk=4\n[H]\nk=2\nnew=n\n[G]\nx=y\n");
        editor.set("G", "k", None);
        assert_eq!(editor.text(), "[H]\nk=2\nnew=n\n[G]\nx=y\n".replace("[H]", "[G]\n[H]"));
        let mut crlf = LineEditor::parse("[G]\r\nk=1\r\n");
        crlf.set("G", "k", Some("2"));
        assert_eq!(crlf.text(), "[G]\r\nk=2\n");
    }

    #[test]
    #[ignore = "reads this machine's MIME configuration"]
    fn live_defaults() {
        let index = AppIndex::load();
        for role in DefaultRole::ALL {
            println!("{:>14}: {:?} of {:?}", role.label(), index.role_default(role), index.role_candidates(role));
        }
        println!("terminal command: {:?}", index.terminal_command());
        println!("https: {:?}", index.apps_for("x-scheme-handler/https"));
        assert!(index.default_for("x-scheme-handler/https").is_some());
    }
}
