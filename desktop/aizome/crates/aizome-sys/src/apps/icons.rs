//! Icon lookup, as the Icon Theme Specification 0.13 defines it.
//!
//! - **Base directories**, in order: `$HOME/.icons`, `$XDG_DATA_HOME/icons`,
//!   each `$XDG_DATA_DIRS/icons`, then each `$XDG_DATA_DIRS/pixmaps` (where
//!   NixOS puts what other systems have in `/usr/share/pixmaps`) and
//!   `/usr/share/pixmaps`.
//! - **Themes**: a theme is a directory of that name in any base directory;
//!   its `index.theme` is the first one found. `Directories` and
//!   `ScaledDirectories` name the subdirectories, each with `Size`, `Scale`
//!   (1), `Type` (`Fixed`, `Scalable` or `Threshold`, the default),
//!   `MinSize`/`MaxSize` (`Size`) and `Threshold` (2); one without a valid
//!   `Size` is ignored. `Inherits` lists parents (comma-separated).
//! - **Lookup** follows the specification's `FindIcon`: the chosen theme,
//!   then its parents depth first (each theme once, so cycles are
//!   harmless), then `hicolor`, then unthemed icons directly in a base
//!   directory (`LookupFallbackIcon`). In each theme, `LookupIcon` takes
//!   the first subdirectory (in `index.theme` order, then base directory
//!   order, then png, svg, xpm) whose size matches exactly
//!   (`DirectoryMatchesSize`, same scale), else the closest one
//!   (`DirectorySizeDistance`, comparing sizes times scales). The
//!   specification's pseudocode for `Threshold` distances has typos; the
//!   distance used here is to the nearest edge of `Size ± Threshold`, as
//!   GTK does.
//! - **Several names**: [`IconLookup::lookup_any`] tries each name in turn
//!   through every theme (an app's own icon from hicolor beats the theme's
//!   `application-x-executable`), which is what launchers want;
//!   [`IconLookup::lookup_best`] is the specification's `FindBestIcon`,
//!   where each theme tries every name before its parents (a themed
//!   `text-x-generic` beats an unthemed `text-x-csrc`), for consistent
//!   looks among MIME type icons.
//! - **Names**: an absolute path is returned as-is if the file exists; a
//!   name with a `.png`, `.svg` or `.xpm` extension (a common mistake in
//!   `Icon=`) is looked up without it.
//!
//! [`IconLookup`] reads each theme's directory listings once, into a map
//! from icon name to where it exists, and remembers every answer, so a
//! repeated lookup is one hash lookup and no file system access. Call
//! [`IconLookup::invalidate`] after themes change (or icons are installed).

use super::keyfile::{self, KeyFile};
use super::xdg::XdgDirs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The theme every lookup falls back to.
const HICOLOR: &str = "hicolor";

/// An installed icon theme, for choosing one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconTheme {
    /// The directory name, which is what [`IconLookup::set_theme`] takes.
    pub id: String,
    /// The display name (`Name`, unlocalised).
    pub name: String,
    /// `Comment`.
    pub comment: Option<String>,
    /// `Inherits`.
    pub inherits: Vec<String>,
    /// `Hidden`: not meant to be offered.
    pub hidden: bool,
    /// The `index.theme` that describes it.
    pub index: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

#[derive(Clone, Debug)]
struct ThemeDir {
    path: String,
    size: u32,
    scale: u32,
    kind: Kind,
    min: u32,
    max: u32,
    threshold: u32,
}

impl ThemeDir {
    fn parse(path: &str, file: &KeyFile) -> Option<ThemeDir> {
        let number = |key: &str| file.get(path, key).and_then(|v| v.trim().parse::<u32>().ok());
        let size = number("Size").filter(|&size| size > 0)?;
        let kind = match file.get(path, "Type").map(str::trim) {
            Some("Fixed") => Kind::Fixed,
            Some("Scalable") => Kind::Scalable,
            _ => Kind::Threshold,
        };
        Some(ThemeDir {
            path: path.to_owned(),
            size,
            scale: number("Scale").filter(|&scale| scale > 0).unwrap_or(1),
            kind,
            min: number("MinSize").unwrap_or(size),
            max: number("MaxSize").unwrap_or(size),
            threshold: number("Threshold").unwrap_or(2),
        })
    }

    /// `DirectoryMatchesSize`.
    fn matches(&self, size: u32, scale: u32) -> bool {
        if self.scale != scale {
            return false;
        }
        match self.kind {
            Kind::Fixed => self.size == size,
            Kind::Scalable => self.min <= size && size <= self.max,
            Kind::Threshold => {
                self.size.saturating_sub(self.threshold) <= size && size <= self.size.saturating_add(self.threshold)
            }
        }
    }

    /// `DirectorySizeDistance`.
    fn distance(&self, size: u32, scale: u32) -> u64 {
        let wanted = u64::from(size) * u64::from(scale);
        let scale = u64::from(self.scale);
        let (low, high) = match self.kind {
            Kind::Fixed => (u64::from(self.size) * scale, u64::from(self.size) * scale),
            Kind::Scalable => (u64::from(self.min) * scale, u64::from(self.max) * scale),
            Kind::Threshold => (
                u64::from(self.size.saturating_sub(self.threshold)) * scale,
                u64::from(self.size.saturating_add(self.threshold)) * scale,
            ),
        };
        // Below the range, above it, or (0) inside it.
        low.saturating_sub(wanted) + wanted.saturating_sub(high)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ext {
    Png,
    Svg,
    Xpm,
}

impl Ext {
    const ALL: [Ext; 3] = [Ext::Png, Ext::Svg, Ext::Xpm];

    fn as_str(self) -> &'static str {
        match self {
            Ext::Png => "png",
            Ext::Svg => "svg",
            Ext::Xpm => "xpm",
        }
    }

    /// Split `name.ext` for a known extension.
    fn split(file: &str) -> Option<(&str, Ext)> {
        let (stem, ext) = file.rsplit_once('.')?;
        let ext = match ext {
            "png" => Ext::Png,
            "svg" => Ext::Svg,
            "xpm" => Ext::Xpm,
            _ => return None,
        };
        (!stem.is_empty()).then_some((stem, ext))
    }
}

/// Where one icon file is: subdirectory, root (base directory), extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Loc {
    dir: u32,
    root: u32,
    ext: Ext,
}

/// A theme read into memory.
#[derive(Debug)]
struct Theme {
    parents: Vec<String>,
    dirs: Vec<ThemeDir>,
    /// `<base>/<theme>` for each base directory that has the theme.
    roots: Vec<PathBuf>,
    /// Icon name → every place it is, in lookup order.
    icons: HashMap<Box<str>, Vec<Loc>>,
}

impl Theme {
    fn load(name: &str, base_dirs: &[PathBuf]) -> Option<Theme> {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return None;
        }
        let roots: Vec<PathBuf> = base_dirs.iter().map(|base| base.join(name)).filter(|root| root.is_dir()).collect();
        let index = roots.iter().find_map(|root| std::fs::read(root.join("index.theme")).ok())?;
        let file = KeyFile::parse(&String::from_utf8_lossy(&index));
        let list = |key: &str| -> Vec<String> {
            file.get("Icon Theme", key)
                .map(|value| {
                    keyfile::unescape(value)
                        .split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut seen = HashSet::new();
        let dirs: Vec<ThemeDir> = list("Directories")
            .into_iter()
            .chain(list("ScaledDirectories"))
            .filter(|dir| seen.insert(dir.clone()))
            .filter_map(|dir| ThemeDir::parse(&dir, &file))
            .collect();
        let mut icons: HashMap<Box<str>, Vec<Loc>> = HashMap::new();
        for (d, dir) in dirs.iter().enumerate() {
            for (r, root) in roots.iter().enumerate() {
                let Ok(read) = std::fs::read_dir(root.join(&dir.path)) else {
                    continue;
                };
                for entry in read.flatten() {
                    let file_name = entry.file_name();
                    let Some((stem, ext)) = file_name.to_str().and_then(Ext::split) else {
                        continue;
                    };
                    let loc = Loc { dir: d as u32, root: r as u32, ext };
                    match icons.get_mut(stem) {
                        Some(locs) => locs.push(loc),
                        None => {
                            icons.insert(stem.into(), vec![loc]);
                        }
                    }
                }
            }
        }
        for locs in icons.values_mut() {
            locs.sort_unstable();
            locs.dedup();
        }
        Some(Theme { parents: list("Inherits"), dirs, roots, icons })
    }

    fn path(&self, name: &str, loc: Loc) -> Option<PathBuf> {
        let root = self.roots.get(loc.root as usize)?;
        let dir = self.dirs.get(loc.dir as usize)?;
        Some(root.join(&dir.path).join(format!("{name}.{}", loc.ext.as_str())))
    }

    /// `LookupIcon`.
    fn lookup(&self, name: &str, size: u32, scale: u32) -> Option<PathBuf> {
        let locs = self.icons.get(name)?;
        let dir = |loc: &Loc| self.dirs.get(loc.dir as usize);
        if let Some(loc) = locs.iter().find(|loc| dir(loc).is_some_and(|d| d.matches(size, scale))) {
            return self.path(name, *loc);
        }
        let closest = locs
            .iter()
            .filter_map(|loc| Some((dir(loc)?.distance(size, scale), *loc)))
            .min_by_key(|(distance, loc)| (*distance, *loc))?;
        self.path(name, closest.1)
    }
}

#[derive(Debug, Default)]
struct Inner {
    base_dirs: Vec<PathBuf>,
    theme: String,
    /// Loaded themes; `None` for names that aren't installed.
    themes: HashMap<String, Option<Arc<Theme>>>,
    /// The chosen theme, its ancestors and hicolor, in lookup order.
    chain: Option<Vec<Arc<Theme>>>,
    /// Each base directory's loose icon files: name → extensions.
    loose: Option<Vec<HashMap<String, Vec<Ext>>>>,
    results: HashMap<(String, u32, u32), Option<PathBuf>>,
}

impl Inner {
    fn theme(&mut self, name: &str) -> Option<Arc<Theme>> {
        if let Some(theme) = self.themes.get(name) {
            return theme.clone();
        }
        let theme = Theme::load(name, &self.base_dirs).map(Arc::new);
        self.themes.insert(name.to_owned(), theme.clone());
        theme
    }

    fn chain(&mut self) -> Vec<Arc<Theme>> {
        if let Some(chain) = &self.chain {
            return chain.clone();
        }
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let selected = self.theme.clone();
        self.walk(&selected, &mut visited, &mut chain);
        self.walk(HICOLOR, &mut visited, &mut chain);
        self.chain = Some(chain.clone());
        chain
    }

    fn walk(&mut self, name: &str, visited: &mut HashSet<String>, chain: &mut Vec<Arc<Theme>>) {
        if !visited.insert(name.to_owned()) {
            return;
        }
        let Some(theme) = self.theme(name) else {
            return;
        };
        chain.push(theme.clone());
        for parent in &theme.parents {
            self.walk(parent, visited, chain);
        }
    }

    fn loose(&mut self) -> &[HashMap<String, Vec<Ext>>] {
        let base_dirs = &self.base_dirs;
        self.loose.get_or_insert_with(|| {
            base_dirs
                .iter()
                .map(|dir| {
                    let mut files: HashMap<String, Vec<Ext>> = HashMap::new();
                    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                        if let Some((stem, ext)) = entry.file_name().to_str().and_then(Ext::split) {
                            files.entry(stem.to_owned()).or_default().push(ext);
                        }
                    }
                    files
                })
                .collect()
        })
    }

    /// `FindBestIcon` (with one name, `FindIcon`); absolute paths first.
    fn find_best(&mut self, names: &[&str], size: u32, scale: u32) -> Option<PathBuf> {
        for name in names.iter().filter(|name| name.starts_with('/')) {
            let path = Path::new(name);
            if path.is_file() {
                return Some(path.to_owned());
            }
        }
        let names: Vec<&str> = names
            .iter()
            .filter(|name| !name.is_empty() && !name.starts_with('/'))
            .map(|name| Ext::split(name).map_or(*name, |(stem, _)| stem))
            .collect();
        for theme in self.chain() {
            for name in &names {
                if let Some(path) = theme.lookup(name, size, scale) {
                    return Some(path);
                }
            }
        }
        let base_dirs = self.base_dirs.clone();
        let loose = self.loose();
        for name in &names {
            for (dir, files) in base_dirs.iter().zip(loose) {
                if let Some(exts) = files.get(*name)
                    && let Some(ext) = Ext::ALL.iter().find(|ext| exts.contains(ext))
                {
                    return Some(dir.join(format!("{name}.{}", ext.as_str())));
                }
            }
        }
        None
    }
}

/// Finds icon files by name, size and scale; see the module documentation.
/// Cheap to share (`Send + Sync`); lookups lock briefly.
#[derive(Debug)]
pub struct IconLookup {
    inner: Mutex<Inner>,
}

impl IconLookup {
    /// A lookup in `dirs`' icon directories, preferring `theme`.
    pub fn new(dirs: &XdgDirs, theme: &str) -> IconLookup {
        IconLookup::with_base_dirs(IconLookup::search_path(dirs), theme)
    }

    /// A lookup in the given base directories (most important first).
    pub fn with_base_dirs(base_dirs: Vec<PathBuf>, theme: &str) -> IconLookup {
        IconLookup { inner: Mutex::new(Inner { base_dirs, theme: theme.to_owned(), ..Inner::default() }) }
    }

    /// A lookup for this process's environment, preferring the theme named
    /// by `AIZOME_ICON_THEME` (else `hicolor`).
    pub fn from_env() -> IconLookup {
        let theme = std::env::var("AIZOME_ICON_THEME")
            .ok()
            .filter(|theme| !theme.trim().is_empty())
            .unwrap_or_else(|| HICOLOR.to_owned());
        IconLookup::new(&XdgDirs::from_env(), theme.trim())
    }

    /// The base directories for `dirs`, in lookup order.
    pub fn search_path(dirs: &XdgDirs) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut add = |path: PathBuf| {
            if !paths.contains(&path) {
                paths.push(path);
            }
        };
        if let Some(home) = &dirs.home {
            add(home.join(".icons"));
        }
        add(dirs.data_home.join("icons"));
        for dir in &dirs.data_dirs {
            add(dir.join("icons"));
        }
        for dir in &dirs.data_dirs {
            add(dir.join("pixmaps"));
        }
        add(PathBuf::from("/usr/share/pixmaps"));
        paths
    }

    /// The theme looked in first.
    pub fn theme(&self) -> String {
        crate::service::lock(&self.inner).theme.clone()
    }

    /// Look in `theme` first from now on.
    pub fn set_theme(&self, theme: &str) {
        let mut inner = crate::service::lock(&self.inner);
        if inner.theme != theme {
            inner.theme = theme.to_owned();
            inner.chain = None;
            inner.results.clear();
        }
    }

    /// Forget everything read, so the next lookups see installed or
    /// removed icons and themes.
    pub fn invalidate(&self) {
        let mut inner = crate::service::lock(&self.inner);
        inner.themes.clear();
        inner.chain = None;
        inner.loose = None;
        inner.results.clear();
    }

    /// The file for icon `name` (or an absolute path) at `size` pixels
    /// and `scale` (1 for normal displays, 2 for HiDPI), if any.
    pub fn lookup(&self, name: &str, size: u32, scale: u32) -> Option<PathBuf> {
        self.lookup_best(&[name], size, scale)
    }

    /// The file for the first of `names` that any theme (or the unthemed
    /// icons) has: each name is looked up in full before the next, so an
    /// app's icon from hicolor beats the chosen theme's
    /// `application-x-executable` given as a fallback.
    pub fn lookup_any(&self, names: &[&str], size: u32, scale: u32) -> Option<PathBuf> {
        names.iter().find_map(|name| self.lookup_best(&[name], size, scale))
    }

    /// The specification's `FindBestIcon`: each theme (the chosen one,
    /// its parents, hicolor) tries every name before the next theme, so
    /// a themed fallback beats an unthemed first choice; then unthemed
    /// icons. Absolute paths among the names are tried first.
    pub fn lookup_best(&self, names: &[&str], size: u32, scale: u32) -> Option<PathBuf> {
        let size = size.max(1);
        let scale = scale.max(1);
        let key = (names.join("\n"), size, scale);
        let mut inner = crate::service::lock(&self.inner);
        if let Some(found) = inner.results.get(&key) {
            return found.clone();
        }
        let found = inner.find_best(names, size, scale);
        inner.results.insert(key, found.clone());
        found
    }

    /// The icon themes installed (those with an `index.theme` listing
    /// directories, so not cursor-only themes), by display name.
    pub fn installed_themes(&self) -> Vec<IconTheme> {
        let base_dirs = crate::service::lock(&self.inner).base_dirs.clone();
        let mut themes: Vec<IconTheme> = Vec::new();
        for base in &base_dirs {
            for entry in std::fs::read_dir(base).into_iter().flatten().flatten() {
                let Ok(id) = entry.file_name().into_string() else {
                    continue;
                };
                if themes.iter().any(|theme| theme.id == id) {
                    continue;
                }
                let index = entry.path().join("index.theme");
                let Ok(text) = std::fs::read(&index) else {
                    continue;
                };
                let file = KeyFile::parse(&String::from_utf8_lossy(&text));
                let get = |key: &str| file.get("Icon Theme", key).map(keyfile::unescape);
                if get("Directories").is_none_or(|dirs| dirs.trim().is_empty()) {
                    continue;
                }
                themes.push(IconTheme {
                    name: get("Name").filter(|name| !name.trim().is_empty()).unwrap_or_else(|| id.clone()),
                    comment: get("Comment"),
                    inherits: get("Inherits")
                        .map(|list| {
                            list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
                        })
                        .unwrap_or_default(),
                    hidden: file.get("Icon Theme", "Hidden").and_then(keyfile::parse_bool).unwrap_or(false),
                    index,
                    id,
                });
            }
        }
        themes.sort_by_cached_key(|theme| (theme.name.to_lowercase(), theme.id.clone()));
        themes
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::TempDir;
    use super::*;

    /// A fake icon tree:
    ///
    /// - `base1/Child` inherits `Parent,Loop`; `base1/Loop` inherits
    ///   `Child` (a cycle).
    /// - `base1/Parent` and `base2/Parent` (the index only in base2).
    /// - `base2/hicolor` with fixed, scalable and scaled directories.
    /// - loose icons in `pixmaps`.
    fn tree() -> (TempDir, IconLookup) {
        let tmp = TempDir::new("icons");
        tmp.write(
            "base1/Child/index.theme",
            "[Icon Theme]\nName=Child\nInherits=Parent,Loop\nDirectories=32,scalable\n\
             [32]\nSize=32\nType=Threshold\n[scalable]\nSize=48\nType=Scalable\nMinSize=8\nMaxSize=512\n",
        );
        tmp.write("base1/Child/32/only-child.png", "");
        tmp.write("base1/Child/scalable/only-child.svg", "");
        tmp.write("base1/Child/scalable/vector.svg", "");
        tmp.write("base1/Loop/index.theme", "[Icon Theme]\nName=Loop\nInherits=Child\nDirectories=16\n[16]\nSize=16\n");
        tmp.write("base1/Loop/16/loop-icon.png", "");
        tmp.write("base1/Parent/24/from-base1.png", "");
        tmp.write(
            "base2/Parent/index.theme",
            "# index in the second base dir only\n[Icon Theme]\nName=Parent\nComment=The parent\n\
             Directories=24,48,broken\n[24]\nSize=24\nType=Fixed\n[48]\nSize=48\nType=Fixed\n[broken]\nType=Fixed\n",
        );
        tmp.write("base2/Parent/24/parent-icon.png", "");
        tmp.write("base2/Parent/48/parent-icon.png", "");
        tmp.write("base2/Parent/48/parent-icon.svg", "");
        tmp.write("base2/Parent/48/from-base1.png", "");
        tmp.write("base2/Parent/broken/parent-icon.png", "");
        tmp.write(
            "base2/hicolor/index.theme",
            "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=16x16/apps,48x48/apps,scalable/apps\n\
             ScaledDirectories=48x48@2/apps\n\
             [16x16/apps]\nSize=16\nType=Threshold\n[48x48/apps]\nSize=48\nType=Threshold\n\
             [48x48@2/apps]\nSize=48\nScale=2\nType=Threshold\n\
             [scalable/apps]\nSize=128\nMinSize=1\nMaxSize=256\nType=Scalable\n",
        );
        for dir in ["16x16/apps", "48x48/apps", "48x48@2/apps"] {
            tmp.write(&format!("base2/hicolor/{dir}/app.png"), "");
        }
        tmp.write("base2/hicolor/scalable/apps/app.svg", "");
        tmp.write("base2/hicolor/16x16/apps/tiny.png", "");
        tmp.write("base2/hicolor/48x48/apps/application-x-executable.png", "");
        tmp.write("base2/hicolor/48x48/apps/odd.xpm", "");
        tmp.write("base2/hicolor/48x48/apps/odd.svg", "");
        tmp.write("pixmaps/loose.xpm", "");
        tmp.write("pixmaps/loose.png", "");
        tmp.write("pixmaps/application-x-executable.png", "");
        let bases = ["base1", "base2", "pixmaps"].iter().map(|b| tmp.path().join(b)).collect();
        let lookup = IconLookup::with_base_dirs(bases, "Child");
        (tmp, lookup)
    }

    fn rel(tmp: &TempDir, path: Option<PathBuf>) -> Option<String> {
        path.map(|p| p.strip_prefix(tmp.path()).unwrap().to_string_lossy().into_owned())
    }

    #[test]
    fn inheritance_and_fallbacks() {
        let (tmp, icons) = tree();
        let find = |name: &str, size: u32| rel(&tmp, icons.lookup(name, size, 1));
        // Threshold 32±2 matches 32 exactly in Child.
        assert_eq!(find("only-child", 32).as_deref(), Some("base1/Child/32/only-child.png"));
        assert_eq!(find("only-child", 34).as_deref(), Some("base1/Child/32/only-child.png"));
        // 64 is outside the threshold; the scalable directory matches.
        assert_eq!(find("only-child", 64).as_deref(), Some("base1/Child/scalable/only-child.svg"));
        // Inherited from Parent, whose index is in base2 but which has
        // icons in both base directories; png before svg in a directory.
        assert_eq!(find("parent-icon", 48).as_deref(), Some("base2/Parent/48/parent-icon.png"));
        assert_eq!(find("from-base1", 24).as_deref(), Some("base1/Parent/24/from-base1.png"));
        // Closest size: 30 is nearer 24 than 48; 40 nearer 48.
        assert_eq!(find("parent-icon", 30).as_deref(), Some("base2/Parent/24/parent-icon.png"));
        assert_eq!(find("parent-icon", 40).as_deref(), Some("base2/Parent/48/parent-icon.png"));
        // The cycle Child → Loop → Child is harmless.
        assert_eq!(find("loop-icon", 16).as_deref(), Some("base1/Loop/16/loop-icon.png"));
        // hicolor last.
        assert_eq!(find("tiny", 16).as_deref(), Some("base2/hicolor/16x16/apps/tiny.png"));
        // svg before xpm in the same directory.
        assert_eq!(find("odd", 48).as_deref(), Some("base2/hicolor/48x48/apps/odd.svg"));
        // Unthemed icons: png before xpm.
        assert_eq!(find("loose", 48).as_deref(), Some("pixmaps/loose.png"));
        assert_eq!(find("nothing", 48), None);
        // An extension in the name is ignored.
        assert_eq!(find("loose.xpm", 48).as_deref(), Some("pixmaps/loose.png"));
        assert_eq!(find("tiny.png", 16).as_deref(), Some("base2/hicolor/16x16/apps/tiny.png"));
        // The directory without a Size is ignored.
        assert_eq!(find("parent-icon", 100).as_deref(), Some("base2/Parent/48/parent-icon.png"));
    }

    #[test]
    fn scale_and_scalable_directories() {
        let (tmp, icons) = tree();
        let find = |size: u32, scale: u32| rel(&tmp, icons.lookup("app", size, scale));
        assert_eq!(find(48, 1).as_deref(), Some("base2/hicolor/48x48/apps/app.png"));
        assert_eq!(find(48, 2).as_deref(), Some("base2/hicolor/48x48@2/apps/app.png"));
        assert_eq!(find(16, 1).as_deref(), Some("base2/hicolor/16x16/apps/app.png"));
        // 200 px: only the scalable directory matches.
        assert_eq!(find(200, 1).as_deref(), Some("base2/hicolor/scalable/apps/app.svg"));
        // 16@3 (48 px) has no scale-3 directory. By size times scale,
        // 48x48 (46–50 px) and scalable (1–256 px) are both at distance 0,
        // 16x16 at 30 and 48x48@2 (92–100 px) at 44; the first listed wins.
        assert_eq!(find(16, 3).as_deref(), Some("base2/hicolor/48x48/apps/app.png"));
    }

    #[test]
    fn fallback_names_and_absolute_paths() {
        let (tmp, icons) = tree();
        let find_any = |names: &[&str], size| rel(&tmp, icons.lookup_any(names, size, 1));
        let find_best = |names: &[&str], size| rel(&tmp, icons.lookup_best(names, size, 1));
        assert_eq!(
            find_any(&["missing-app", "application-x-executable"], 48).as_deref(),
            Some("base2/hicolor/48x48/apps/application-x-executable.png")
        );
        // Name order: the unthemed first choice beats the themed fallback.
        assert_eq!(find_any(&["loose", "application-x-executable"], 48).as_deref(), Some("pixmaps/loose.png"));
        // FindBestIcon: themes first, every name in each.
        assert_eq!(
            find_best(&["loose", "application-x-executable"], 48).as_deref(),
            Some("base2/hicolor/48x48/apps/application-x-executable.png")
        );
        assert_eq!(find_any(&["tiny", "parent-icon"], 16).as_deref(), Some("base2/hicolor/16x16/apps/tiny.png"));
        assert_eq!(find_best(&["tiny", "parent-icon"], 16).as_deref(), Some("base2/Parent/24/parent-icon.png"));
        assert_eq!(find_best(&["tiny"], 16), find_any(&["tiny"], 16));
        let absolute = tmp.path().join("pixmaps/loose.xpm");
        let absolute = absolute.to_str().unwrap();
        assert_eq!(icons.lookup(absolute, 1, 1).as_deref(), Some(Path::new(absolute)));
        let missing = tmp.path().join("pixmaps/missing.png");
        assert_eq!(icons.lookup(missing.to_str().unwrap(), 48, 1), None);
        assert_eq!(
            rel(&tmp, icons.lookup_any(&[missing.to_str().unwrap(), "tiny"], 16, 1)).as_deref(),
            Some("base2/hicolor/16x16/apps/tiny.png")
        );
        assert_eq!(icons.lookup_any(&[], 16, 1), None);
        assert_eq!(icons.lookup("", 16, 1), None);
        assert_eq!(icons.lookup("../escape", 16, 1), None);
    }

    #[test]
    fn answers_are_cached_until_invalidated() {
        let (tmp, icons) = tree();
        assert_eq!(icons.lookup("later", 48, 1), None);
        tmp.write("base2/hicolor/48x48/apps/later.png", "");
        // Remembered: no new reads.
        assert_eq!(icons.lookup("later", 48, 1), None);
        let found = icons.lookup("only-child", 32, 1);
        std::fs::remove_file(tmp.path().join("base1/Child/32/only-child.png")).unwrap();
        assert_eq!(icons.lookup("only-child", 32, 1), found);
        icons.invalidate();
        assert!(icons.lookup("later", 48, 1).is_some());
        assert_eq!(
            rel(&tmp, icons.lookup("only-child", 32, 1)).as_deref(),
            Some("base1/Child/scalable/only-child.svg")
        );
    }

    #[test]
    fn themes_can_be_switched_and_listed() {
        let (tmp, icons) = tree();
        assert_eq!(icons.theme(), "Child");
        assert!(icons.lookup("only-child", 32, 1).is_some());
        icons.set_theme("Parent");
        assert_eq!(icons.lookup("only-child", 32, 1), None);
        assert!(icons.lookup("parent-icon", 24, 1).is_some());
        icons.set_theme("Not Installed");
        assert_eq!(rel(&tmp, icons.lookup("tiny", 16, 1)).as_deref(), Some("base2/hicolor/16x16/apps/tiny.png"));
        let themes = icons.installed_themes();
        let ids: Vec<&str> = themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["Child", "hicolor", "Loop", "Parent"]);
        let parent = themes.iter().find(|t| t.id == "Parent").unwrap();
        assert_eq!(parent.comment.as_deref(), Some("The parent"));
        assert!(themes.iter().find(|t| t.id == "hicolor").unwrap().hidden);
        assert_eq!(themes[0].inherits, ["Parent", "Loop"]);
    }

    #[test]
    fn size_rules() {
        let dir = |kind, size, min, max, threshold, scale| ThemeDir {
            path: String::new(),
            size,
            scale,
            kind,
            min,
            max,
            threshold,
        };
        let fixed = dir(Kind::Fixed, 32, 32, 32, 2, 1);
        assert!(fixed.matches(32, 1) && !fixed.matches(33, 1) && !fixed.matches(32, 2));
        assert_eq!(fixed.distance(16, 1), 16);
        assert_eq!(fixed.distance(16, 2), 0);
        let scalable = dir(Kind::Scalable, 48, 16, 256, 2, 1);
        assert!(scalable.matches(16, 1) && scalable.matches(256, 1) && !scalable.matches(8, 1));
        assert_eq!(scalable.distance(8, 1), 8);
        assert_eq!(scalable.distance(300, 1), 44);
        let threshold = dir(Kind::Threshold, 24, 24, 24, 3, 2);
        assert!(
            threshold.matches(21, 2)
                && threshold.matches(27, 2)
                && !threshold.matches(28, 2)
                && !threshold.matches(24, 1)
        );
        assert_eq!(threshold.distance(20, 2), 2);
        assert_eq!(threshold.distance(60, 1), 6);
        let tiny = dir(Kind::Threshold, 1, 1, 1, 2, 1);
        assert!(tiny.matches(1, 1));
        assert_eq!(tiny.distance(10, 1), 7);
    }

    #[test]
    #[ignore = "reads this machine's icon themes"]
    fn live_icons() {
        let icons = IconLookup::new(&XdgDirs::from_env(), "Adwaita");
        for theme in icons.installed_themes() {
            println!("theme {} ({}) inherits {:?}", theme.id, theme.name, theme.inherits);
        }
        for (name, size, scale) in [
            ("firefox", 48, 1),
            ("foot", 32, 2),
            ("org.gnome.Nautilus", 64, 1),
            ("folder", 16, 1),
            ("utilities-terminal", 256, 1),
            ("chatgpt", 48, 1),
        ] {
            let started = std::time::Instant::now();
            let found = icons.lookup_any(&[name, "application-x-executable"], size, scale);
            let first = started.elapsed();
            let started = std::time::Instant::now();
            let again = icons.lookup_any(&[name, "application-x-executable"], size, scale);
            let cached = started.elapsed();
            println!("{name}@{size}x{scale}: {found:?} (first {first:?}, cached {cached:?})");
            assert_eq!(found, again);
        }
        assert!(icons.lookup("folder", 48, 1).is_some());
        icons.set_theme("breeze");
        let started = std::time::Instant::now();
        let folder = icons.lookup("folder", 48, 1);
        println!("breeze folder: {folder:?} in {:?} (theme load included)", started.elapsed());
        assert!(folder.is_some());
    }
}
