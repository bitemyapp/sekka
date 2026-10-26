//! The palette's rows and how they rank: commands of the focused app,
//! windows, files, apps, settings, actions, and the answer to arithmetic,
//! grouped in sections under All and alone in a scope.

use std::ops::Range;
use std::path::{Path, PathBuf};

use aizome_kit::Icon;
use aizome_search::{Candidate, Query, Scope, calc, rank};
use aizome_shell_proto as proto;

use crate::actions::Action;

/// A row the palette can show.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub scope: Scope,
    pub title: String,
    /// After the title in `text3`: "Edit › Find", "Mail · desk 1", a
    /// file's folder, an app's generic name.
    pub trail: String,
    pub shortcut: Option<proto::Shortcut>,
    pub glyph: Icon,
    /// Matched but not shown.
    pub keywords: Vec<String>,
    /// Its frecency key (`cmd:…`, `app:…`, `file:…`), if picks are counted.
    pub key: Option<String>,
    pub action: Action,
    /// Set in mono (files).
    pub mono: bool,
    /// Added to its match score (frecency, being recent).
    pub boost: i64,
}

impl Item {
    pub fn new(scope: Scope, title: impl Into<String>, glyph: Icon, action: Action) -> Item {
        Item {
            scope,
            title: title.into(),
            trail: String::new(),
            shortcut: None,
            glyph,
            keywords: Vec::new(),
            key: None,
            action,
            mono: false,
            boost: 0,
        }
    }

    pub fn trail(mut self, t: impl Into<String>) -> Item {
        self.trail = t.into();
        self
    }

    pub fn keywords(mut self, k: Vec<String>) -> Item {
        self.keywords = k;
        self
    }

    pub fn key(mut self, k: impl Into<String>) -> Item {
        self.key = Some(k.into());
        self
    }

    pub fn shortcut(mut self, s: Option<proto::Shortcut>) -> Item {
        self.shortcut = s;
        self
    }

    /// A file or folder from the index.
    pub fn file(path: &Path, folder: bool, home: &Path) -> Item {
        let name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
        let title = if folder { format!("{name}/") } else { name };
        let dir = path.parent().map(|p| shorten(p, home)).unwrap_or_default();
        let mut i = Item::new(
            Scope::File,
            title,
            if folder { Icon::Folder } else { file_glyph(path) },
            Action::Open { path: path.to_owned() },
        )
        .trail(dir)
        .key(format!("file:{}", path.display()));
        i.mono = true;
        i
    }
}

/// A path with the home folder as `~`, ending in `/`.
pub fn shorten(p: &Path, home: &Path) -> String {
    let s = match p.strip_prefix(home) {
        Ok(rel) if rel.as_os_str().is_empty() => "~".to_owned(),
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => p.display().to_string(),
    };
    format!("{s}/")
}

/// A file's glyph by its kind.
pub fn file_glyph(p: &Path) -> Icon {
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tiff" | "avif" => Icon::FileImage,
        "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "go" | "nix" | "toml" | "json" | "sh" | "hs" | "zig" => {
            Icon::FileCode
        }
        "md" | "txt" | "org" | "rst" | "pdf" | "doc" | "docx" | "odt" => Icon::FileText,
        "mp4" | "mkv" | "webm" | "mov" => Icon::Film,
        "mp3" | "flac" | "ogg" | "wav" | "opus" => Icon::EighthNotes,
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" => Icon::Archive,
        _ => Icon::File,
    }
}

/// A ranked row: its item and where its title matched.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub item: Item,
    pub ranges: Vec<Range<usize>>,
    pub score: i64,
}

/// Rows under a mono head ("COMMANDS  EDITOR MENUS").
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub scope: Scope,
    pub head: String,
    /// After the head, fainter: whose menus they are.
    pub sub: Option<String>,
    pub rows: Vec<Row>,
}

/// The section heads, in their order when nothing's typed.
pub const ORDER: [Scope; 7] =
    [Scope::Answer, Scope::Command, Scope::Window, Scope::File, Scope::App, Scope::Setting, Scope::Action];

/// What ranks rows: the query, the scope (None: All), and how many rows a
/// section shows under All.
pub struct Ranking<'a> {
    pub query: &'a str,
    pub scope: Option<Scope>,
    pub per_section: usize,
    /// The focused app's name, for the Commands head's "EDITOR MENUS".
    pub app_name: Option<&'a str>,
}

/// Rank `items` (every scope's but Files') and `files` (already searched
/// for this query by the index) into sections.
pub fn sections(r: &Ranking, items: &[Item], files: &[Item]) -> Vec<Section> {
    let mut out = Vec::new();
    let q = r.query.trim();
    // The answer to arithmetic comes first.
    if r.scope.is_none()
        && calc::looks_like_math(q)
        && let Ok(v) = calc::eval(q)
    {
        let result = calc::format(v);
        let item =
            Item::new(Scope::Answer, format!("= {result}"), Icon::Calculator, Action::Copy { text: result.clone() })
                .trail("Copy the result");
        out.push(Section {
            scope: Scope::Answer,
            head: "ANSWER".into(),
            sub: None,
            rows: vec![Row { item, ranges: vec![], score: i64::MAX }],
        });
    }
    let limit = if r.scope.is_some() { 60 } else { r.per_section.max(1) };
    let mut query = Query::new(q);
    for scope in ORDER.iter().copied().filter(|s| *s != Scope::Answer) {
        if r.scope.is_some_and(|only| only != scope) {
            continue;
        }
        // Actions show under All only when something's typed.
        if scope == Scope::Action && r.scope.is_none() && q.is_empty() {
            continue;
        }
        let rows: Vec<Row> = if scope == Scope::File {
            files
                .iter()
                .take(limit)
                .map(|i| Row { item: i.clone(), ranges: name_ranges(i, q), score: i.boost })
                .collect()
        } else {
            let pool: Vec<&Item> = items.iter().filter(|i| i.scope == scope).collect();
            let candidates: Vec<Candidate> = pool
                .iter()
                .map(|i| Candidate {
                    title: i.title.clone(),
                    keywords: keywords_of(i),
                    boost: i.boost + exactness(&i.title, q),
                })
                .collect();
            rank(&mut query, &candidates, limit)
                .into_iter()
                .map(|rk| Row { item: pool[rk.index].clone(), ranges: rk.ranges, score: rk.score })
                .collect()
        };
        if rows.is_empty() {
            continue;
        }
        let head = scope.label().to_uppercase();
        let sub =
            (scope == Scope::Command).then(|| r.app_name.map(|a| format!("{} MENUS", a.to_uppercase()))).flatten();
        out.push(Section { scope, head, sub, rows });
    }
    // With something typed, the section holding the best match leads.
    if !q.is_empty() && r.scope.is_none() {
        let answer = out.first().is_some_and(|s| s.scope == Scope::Answer) as usize;
        out[answer..].sort_by(|a, b| {
            let best = |s: &Section| s.rows.iter().map(|r| r.score).max().unwrap_or(0);
            best(b).cmp(&best(a)).then(order_of(a.scope).cmp(&order_of(b.scope)))
        });
    }
    out
}

fn order_of(s: Scope) -> usize {
    ORDER.iter().position(|x| *x == s).unwrap_or(ORDER.len())
}

fn keywords_of(i: &Item) -> Vec<String> {
    let mut k = i.keywords.clone();
    if !i.trail.is_empty() && i.scope == Scope::Command {
        // "Edit › Find" helps "edit find" find its items.
        k.push(format!("{} {}", i.trail, i.title));
    }
    k
}

/// Where a file's name matched (the index matched it; highlight again).
fn name_ranges(i: &Item, q: &str) -> Vec<Range<usize>> {
    if q.is_empty() {
        return Vec::new();
    }
    let mut query = Query::for_paths(q);
    query.matches(&i.title).map(|m| m.ranges).unwrap_or_default()
}

/// The rows of every section in order: what ↑↓ walk.
pub fn flat(sections: &[Section]) -> Vec<&Row> {
    sections.iter().flat_map(|s| s.rows.iter()).collect()
}

/// The scope after `s` (Tab) or before it (⇧Tab) among `scopes`, All
/// included.
pub fn cycle(scopes: &[Scope], s: Option<Scope>, forward: bool) -> Option<Scope> {
    let all: Vec<Option<Scope>> = std::iter::once(None).chain(scopes.iter().copied().map(Some)).collect();
    let i = all.iter().position(|x| *x == s).unwrap_or(0);
    let n = all.len();
    all[if forward { (i + 1) % n } else { (i + n - 1) % n }]
}

/// A scope by the name `aizomectl` and the Toolchest's Find ▸ use.
pub fn scope_named(name: &str) -> Option<Scope> {
    match name.to_ascii_lowercase().as_str() {
        "commands" | "command" => Some(Scope::Command),
        "windows" | "window" => Some(Scope::Window),
        "files" | "file" => Some(Scope::File),
        "apps" | "app" => Some(Scope::App),
        "settings" | "setting" => Some(Scope::Setting),
        "actions" | "action" => Some(Scope::Action),
        _ => None,
    }
}

/// How often a command was run from menus or the palette this week: its
/// `menu:` frecency, about a count of picks in the last seven days.
pub fn coach_count(score: f64) -> u32 {
    score.round().max(0.0) as u32
}

/// The shortcut coach's words, when a command with keys has been picked
/// by hand three or more times this week.
pub fn coach(count: u32, has_shortcut: bool) -> Option<String> {
    (has_shortcut && count >= 3).then(|| {
        format!("You've run this from menus {count} times this week. The keys above do it without leaving the text.")
    })
}

/// A setting from `aizome-settei --list-settings --json`: one pane or
/// option.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SettingEntry {
    pub id: String,
    pub title: String,
    /// "Keyboard › Remap Caps Lock" or the pane's name.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The pane it opens.
    #[serde(default)]
    pub pane: Option<String>,
}

/// The panes when Settei can't list its own.
pub fn builtin_settings() -> Vec<SettingEntry> {
    let p = |id: &str, title: &str, kw: &[&str]| SettingEntry {
        id: id.into(),
        title: title.into(),
        path: title.into(),
        keywords: kw.iter().map(|s| s.to_string()).collect(),
        pane: Some(id.into()),
    };
    vec![
        p("network", "Network", &["wifi", "wi-fi", "ethernet", "vpn", "internet"]),
        p("bluetooth", "Bluetooth", &["devices", "pair", "headphones"]),
        p("sound", "Sound", &["volume", "audio", "speakers", "microphone"]),
        p("displays", "Displays", &["monitor", "screen", "resolution", "scale", "night light"]),
        p("appearance", "Appearance", &["theme", "dark", "light", "night", "day", "wallpaper", "focus colour"]),
        p("desktop", "Desktop & Deskbar", &["menu bar", "deskbar", "desks", "clock"]),
        p("notifications", "Notifications", &["do not disturb", "alerts"]),
        p("keyboard", "Keyboard", &["layout", "shortcuts", "repeat", "caps lock"]),
        p("pointer", "Mouse & Touchpad", &["trackpad", "scroll", "tap", "cursor"]),
        p("power", "Battery & Power", &["sleep", "power mode", "energy"]),
        p("users", "Users", &["account", "password", "login"]),
        p("datetime", "Date & Time", &["clock", "timezone", "24-hour"]),
        p("printers", "Printers", &["print", "scanner"]),
        p("about", "About", &["system", "version", "hardware"]),
    ]
}

/// Settings rows from Settei's list (JSON: an array of entries, or an
/// object with `settings`).
pub fn parse_settings(json: &str) -> Option<Vec<SettingEntry>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = v.as_array().cloned().or_else(|| v.get("settings").and_then(|s| s.as_array()).cloned())?;
    Some(arr.into_iter().filter_map(|e| serde_json::from_value::<SettingEntry>(e).ok()).collect())
}

pub fn setting_item(s: &SettingEntry) -> Item {
    let pane = s.pane.clone().unwrap_or_else(|| s.id.split(['.', '/', ':']).next().unwrap_or(&s.id).to_owned());
    let anchor =
        (s.pane.is_some() && s.id != pane || s.pane.is_none() && s.id.contains(['.', '/', ':'])).then(|| s.id.clone());
    let title = if s.path.is_empty() { s.title.clone() } else { s.path.clone() };
    Item::new(Scope::Setting, title, Icon::Settings, Action::Settings { pane, anchor })
        .trail("Settings")
        .keywords(s.keywords.iter().cloned().chain(std::iter::once(s.title.clone())).collect())
        .key(format!("set:{}", s.id))
}

/// Where the settings list is cached.
pub fn settings_cache_path() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| aizome_config::home_dir().join(".cache"))
        .join("aizome/settings-list.json")
}

/// A lift for titles that are what was typed, or start with it: "bold"
/// finds Bold before Undo Bold.
pub fn exactness(title: &str, q: &str) -> i64 {
    if q.is_empty() {
        return 0;
    }
    let t = title.trim_end_matches('…').to_lowercase();
    let q = q.to_lowercase();
    if t == q {
        120
    } else if t.starts_with(&q) {
        40
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(title: &str, trail: &str) -> Item {
        Item::new(Scope::Command, title, Icon::Search, Action::None).trail(trail)
    }

    fn items() -> Vec<Item> {
        vec![
            cmd("Find and Replace…", "Edit › Find"),
            cmd("Replace in Files…", "Edit › Find"),
            cmd("Find All References", "Go"),
            cmd("Paste", "Edit"),
            Item::new(Scope::Window, "Re: frame trace replay", Icon::Mail, Action::None).trail("Mail · desk 1"),
            Item::new(Scope::App, "Soroban", Icon::Calculator, Action::None).keywords(vec!["calculator".into()]),
            Item::new(Scope::Action, "Lock", Icon::Lock, Action::None),
        ]
    }

    fn ranking(q: &str, scope: Option<Scope>) -> Ranking<'_> {
        Ranking { query: q, scope, per_section: 4, app_name: Some("Editor") }
    }

    #[test]
    fn all_groups_by_scope_with_the_best_section_first() {
        let s = sections(&ranking("find rep", None), &items(), &[]);
        assert_eq!(s[0].scope, Scope::Command);
        assert_eq!(s[0].sub.as_deref(), Some("EDITOR MENUS"));
        assert_eq!(s[0].rows[0].item.title, "Find and Replace…");
        assert!(!s[0].rows[0].ranges.is_empty());
        let s = sections(&ranking("calc", None), &items(), &[]);
        assert_eq!(s[0].scope, Scope::App, "a keyword finds Soroban");
    }

    #[test]
    fn a_scope_shows_only_its_rows() {
        let s = sections(&ranking("re", Some(Scope::Window)), &items(), &[]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].scope, Scope::Window);
    }

    #[test]
    fn arithmetic_answers_first() {
        let s = sections(&ranking("2^10 / 4", None), &items(), &[]);
        assert_eq!(s[0].scope, Scope::Answer);
        assert_eq!(s[0].rows[0].item.title, "= 256");
        assert_eq!(s[0].rows[0].item.action, Action::Copy { text: "256".into() });
    }

    #[test]
    fn empty_queries_leave_actions_out_of_all() {
        let s = sections(&ranking("", None), &items(), &[]);
        assert!(s.iter().all(|s| s.scope != Scope::Action));
        assert_eq!(s[0].scope, Scope::Command);
        assert_eq!(s[0].rows.len(), 4, "under All a section shows a few rows");
        let s = sections(&ranking("", Some(Scope::Action)), &items(), &[]);
        assert_eq!(s[0].rows[0].item.title, "Lock");
    }

    #[test]
    fn frecency_lifts_a_habit() {
        let mut it = items();
        it[1].boost = 400;
        let s = sections(&ranking("rep", Some(Scope::Command)), &it, &[]);
        assert_eq!(s[0].rows[0].item.title, "Replace in Files…");
    }

    #[test]
    fn files_keep_the_index_order_and_highlight_names() {
        let home = Path::new("/home/u");
        let f = Item::file(Path::new("/home/u/work/sidestep-engine/src/trace/replay.rs"), false, home);
        assert_eq!(f.title, "replay.rs");
        assert_eq!(f.trail, "~/work/sidestep-engine/src/trace/");
        assert_eq!(f.glyph, Icon::FileCode);
        let s = sections(&ranking("rep", None), &items(), &[f]);
        let files = s.iter().find(|s| s.scope == Scope::File).unwrap();
        assert!(!files.rows[0].ranges.is_empty());
        let d = Item::file(Path::new("/home/u/release-notes"), true, home);
        assert_eq!((d.title.as_str(), d.trail.as_str()), ("release-notes/", "~/"));
    }

    #[test]
    fn tab_cycles_scopes_through_all() {
        assert_eq!(cycle(&Scope::CYCLE, None, true), Some(Scope::Command));
        assert_eq!(cycle(&Scope::CYCLE, Some(Scope::Setting), true), None);
        assert_eq!(cycle(&Scope::CYCLE, None, false), Some(Scope::Setting));
        // Only the scopes the settings leave on.
        assert_eq!(cycle(&[Scope::App], Some(Scope::App), true), None);
        assert_eq!(cycle(&[Scope::App], None, false), Some(Scope::App));
        assert_eq!(scope_named("Files"), Some(Scope::File));
    }

    #[test]
    fn the_coach_speaks_after_three_picks() {
        assert!(coach(2, true).is_none());
        assert!(coach(14, false).is_none());
        assert!(coach(coach_count(14.2), true).unwrap().contains("14 times"));
    }

    #[test]
    fn settings_lists_parse_and_open_their_place() {
        let json = r#"[{"id":"keyboard","title":"Keyboard","path":"Keyboard","keywords":["layout"]},
                       {"id":"keyboard.caps","title":"Remap Caps Lock","path":"Keyboard › Remap Caps Lock","pane":"keyboard"}]"#;
        let list = parse_settings(json).unwrap();
        assert_eq!(list.len(), 2);
        let caps = setting_item(&list[1]);
        assert_eq!(caps.title, "Keyboard › Remap Caps Lock");
        assert_eq!(caps.action, Action::Settings { pane: "keyboard".into(), anchor: Some("keyboard.caps".into()) });
        let pane = setting_item(&list[0]);
        assert_eq!(pane.action, Action::Settings { pane: "keyboard".into(), anchor: None });
        assert!(parse_settings(r#"{"settings":[]}"#).unwrap().is_empty());
        assert!(builtin_settings().iter().any(|s| s.id == "network"));
    }
}
