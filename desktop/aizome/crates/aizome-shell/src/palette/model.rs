//! The palette's rows and how they rank: commands of the focused app,
//! windows, files, apps, settings, actions, and the answer to arithmetic,
//! grouped in sections under All and alone in a scope.

use std::ops::Range;
use std::path::{Path, PathBuf};

use aizome_kit::Icon;
use aizome_search::{Candidate, Query, Ranked, Scope, calc, rank};
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
    /// An app's purposes in Settei's Default Apps ("Terminal", "Text
    /// editor") for which it's the chosen app.
    pub purposes: Vec<&'static str>,
    /// Its frecency key (`cmd:…`, `app:…`, `file:…`), if picks are counted.
    pub key: Option<String>,
    pub action: Action,
    /// Set in mono (files).
    pub mono: bool,
    /// Added to its match score (frecency, being recent).
    pub boost: i64,
    /// Aizome's mark after the title: one of Aizome's own apps.
    pub mark: bool,
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
            purposes: Vec::new(),
            key: None,
            action,
            mono: false,
            boost: 0,
            mark: false,
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
    let purpose_query = q.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut prefer_apps = false;
    let mut ordinary_app_score = None;
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
            let preferred: Vec<bool> = pool.iter().map(|i| matches_purpose(i, &purpose_query)).collect();
            let has_preferred = preferred.iter().any(|p| *p);
            // Keep the ordinary winner before prioritising or truncating:
            // Commands versus Apps still follows ordinary matching.
            let mut ranked = rank(&mut query, &candidates, if has_preferred { pool.len() } else { limit });
            if scope == Scope::App {
                ordinary_app_score = ranked.first().map(|r| r.score);
                prefer_apps = has_preferred;
            }
            if has_preferred {
                // A role is itself a reason to find its app. Desktop
                // entries needn't repeat Settei's labels (mpv has neither
                // "Videos" nor "Music"). Don't add aliases to other queries.
                for (index, preferred) in preferred.iter().enumerate() {
                    if *preferred && !ranked.iter().any(|r| r.index == index) {
                        ranked.push(Ranked { index, score: 0, ranges: Vec::new() });
                    }
                }
                // Stable: all other apps keep their normal order. This is
                // priority, not a score that an exact title or habit can beat.
                ranked.sort_by_key(|r| !preferred[r.index]);
                ranked.truncate(limit);
            }
            ranked
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
    // A purpose takes Apps past other sections. Keep Commands ahead when
    // they would have beaten Apps without purpose matching, promoting them
    // too rather than leaving Settings or Files ahead of both.
    if !q.is_empty() && r.scope.is_none() {
        let answer = out.first().is_some_and(|s| s.scope == Scope::Answer) as usize;
        let best = |s: &Section| s.rows.iter().map(|r| r.score).max().unwrap_or(0);
        let commands = out.iter().find(|s| s.scope == Scope::Command).map(best);
        let commands_first = commands.is_some_and(|score| ordinary_app_score.is_none_or(|app| score >= app));
        let priority = |s: &Section| match s.scope {
            Scope::Command if prefer_apps && commands_first => 0,
            Scope::App if prefer_apps => 1,
            _ => 2,
        };
        out[answer..].sort_by(|a, b| {
            priority(a).cmp(&priority(b)).then(best(b).cmp(&best(a))).then(order_of(a.scope).cmp(&order_of(b.scope)))
        });
    }
    out
}

/// Asking for a purpose opens the app chosen for it: "term" and "terminal"
/// find the default terminal before another used more often, and before
/// the setting that chooses it. The query must start a word of the
/// purpose's name, from three letters: "te" and "fi" rank as before.
/// `q` has already been lowercased and its whitespace normalised.
fn matches_purpose(i: &Item, q: &str) -> bool {
    if i.scope != Scope::App || q.chars().count() < 3 {
        return false;
    }
    let starts_a_word = |purpose: &&str| {
        let purpose = purpose.to_lowercase();
        purpose.match_indices(q).any(|(at, _)| at == 0 || purpose[..at].ends_with(' '))
    };
    i.purposes.iter().any(starts_a_word)
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
    /// The option in that pane (`caps-lock`); none for a pane.
    #[serde(default)]
    pub entry: Option<String>,
}

/// The panes when Settei can't list its own.
pub fn builtin_settings() -> Vec<SettingEntry> {
    let p = |id: &str, title: &str, kw: &[&str]| SettingEntry {
        id: id.into(),
        title: title.into(),
        path: title.into(),
        keywords: kw.iter().map(|s| s.to_string()).collect(),
        pane: Some(id.into()),
        entry: None,
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
    let title = if s.path.is_empty() { s.title.clone() } else { s.path.clone() };
    Item::new(Scope::Setting, title, Icon::Settings, Action::Settings { pane, anchor: s.entry.clone() })
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

    /// An app as the shell lists it, chosen in Settei for `purposes`.
    fn app(name: &str, generic: &str, keywords: &[&str], purposes: &[&'static str]) -> Item {
        let keywords = keywords.iter().map(|k| k.to_string()).chain([generic.to_owned()]).collect();
        let mut i = Item::new(Scope::App, name, Icon::AppWindow, Action::None).trail(generic).keywords(keywords);
        i.purposes = purposes.to_vec();
        i
    }

    /// Apps whose purposes overlap, as their desktop entries describe
    /// them, and the setting that chooses the terminal.
    fn desktop() -> Vec<Item> {
        let mut foot = app("Foot", "Terminal", &["shell", "prompt", "command", "commandline"], &[]);
        // About a hundred picks.
        foot.boost = 110;
        vec![
            app("Hotaru", "Terminal", &["terminal", "shell", "console", "command line", "hotaru"], &["Terminal"]),
            foot,
            app("Foot Server", "Terminal", &["shell", "prompt", "command", "commandline"], &[]),
            app("kitty", "Terminal", &["shell", "prompt", "terminal"], &[]),
            app("Hako", "Files", &["files", "file manager", "folders", "browser", "documents"], &["Files"]),
            app("Sumi", "Text Editor", &["text editor", "editor", "text"], &["Text editor"]),
            app("Firefox", "Web Browser", &["internet", "www"], &["Web browser"]),
            setting_item(&SettingEntry {
                id: "general.default-terminal".into(),
                title: "Terminal".into(),
                path: "General › Default Apps › Terminal".into(),
                keywords: vec!["shell".into(), "foot".into(), "console".into()],
                pane: Some("general".into()),
                entry: Some("default-terminal".into()),
            }),
        ]
    }

    fn first(s: &[Section]) -> (Scope, &str) {
        (s[0].scope, s[0].rows[0].item.title.as_str())
    }

    #[test]
    fn a_purpose_finds_its_chosen_app_first() {
        for (q, wanted) in [
            ("ter", "Hotaru"),
            ("term", "Hotaru"),
            ("Term", "Hotaru"),
            ("terminal", "Hotaru"),
            ("files", "Hako"),
            ("text editor", "Sumi"),
            ("editor", "Sumi"),
            ("browser", "Firefox"),
        ] {
            let s = sections(&ranking(q, None), &desktop(), &[]);
            assert_eq!(first(&s), (Scope::App, wanted), "query: {q}");
        }
        // Ahead of the setting, while the other terminals keep their order.
        let s = sections(&ranking("terminal", None), &desktop(), &[]);
        assert_eq!(s[0].rows[1].item.title, "Foot");
        assert_eq!(s[1].scope, Scope::Setting);
    }

    #[test]
    fn other_words_rank_as_they_would_without_purpose_priority() {
        let chosen: Vec<Item> = items().into_iter().chain(desktop()).collect();
        let plain: Vec<Item> = chosen
            .iter()
            .cloned()
            .map(|mut i| {
                i.purposes.clear();
                i
            })
            .collect();
        let order = |s: Vec<Section>| -> Vec<(Scope, Vec<(String, i64)>)> {
            s.into_iter().map(|s| (s.scope, s.rows.into_iter().map(|r| (r.item.title, r.score)).collect())).collect()
        };
        // Too short, not the start of a word, or more than a purpose.
        for q in [
            "",
            " ",
            "t",
            "te",
            "fi",
            "rminal",
            "itor",
            "foot",
            "kitty",
            "hotaru",
            "terminal settings",
            "settings terminal",
            "terminal kitty",
            "find rep",
            "~/Documents",
            "文書",
            "🙂",
        ] {
            assert_eq!(
                order(sections(&ranking(q, None), &chosen, &[])),
                order(sections(&ranking(q, None), &plain, &[])),
                "query: {q}"
            );
        }
    }

    #[test]
    fn the_focused_apps_commands_keep_their_precedence() {
        let editor = [cmd("New", "File"), cmd("Open…", "File"), cmd("Copy", "Edit"), cmd("Find…", "Edit")];
        let all: Vec<Item> = editor.into_iter().chain(desktop()).collect();
        for (q, app) in [("file", "Hako"), ("edit", "Sumi")] {
            let s = sections(&ranking(q, None), &all, &[]);
            assert_eq!(s[0].scope, Scope::Command, "query: {q}");
            // Then the chosen app, ahead of the others.
            assert_eq!((s[1].scope, s[1].rows[0].item.title.as_str()), (Scope::App, app), "query: {q}");
        }
        // Commands that don't match leave Apps in the lead.
        assert_eq!(first(&sections(&ranking("terminal", None), &all, &[])), (Scope::App, "Hotaru"));
    }

    #[test]
    fn every_default_role_is_searchable_without_desktop_keywords() {
        for role in aizome_sys::apps::DefaultRole::ALL {
            let label = role.label();
            let chosen = app("選んだアプリ", "", &[], &[label]);
            let mut rival = app(label, label, &[], &[]);
            rival.boost = 10_000;
            let mut setting = Item::new(Scope::Setting, label, Icon::Settings, Action::None);
            setting.boost = 20_000;
            let window = Item::new(Scope::Window, label, Icon::AppWindow, Action::None);
            let all = [rival, chosen, setting, window];
            let mut folder = Item::file(&Path::new("/home/person").join(label), true, Path::new("/home/person"));
            folder.boost = 30_000;
            // Every useful prefix of each word and of the full purpose,
            // including capitals and pasted whitespace.
            for (start, _) in label.char_indices().filter(|(at, _)| *at == 0 || label[..*at].ends_with(' ')) {
                let suffix = &label[start..];
                for end in suffix.char_indices().skip(3).map(|(at, _)| at).chain([suffix.len()]) {
                    let prefix = &suffix[..end];
                    for q in
                        [prefix.to_lowercase(), prefix.to_uppercase(), format!("  {}  ", prefix.replace(' ', " \t "))]
                    {
                        for limit in [1, 4] {
                            for scope in [None, Some(Scope::App)] {
                                let r = Ranking { per_section: limit, ..ranking(&q, scope) };
                                let s = sections(&r, &all, std::slice::from_ref(&folder));
                                assert_eq!(
                                    first(&s),
                                    (Scope::App, "選んだアプリ"),
                                    "{role:?}, {q:?}, {limit}, {scope:?}"
                                );
                                assert_eq!(s[0].rows.iter().filter(|r| r.item.title == "選んだアプリ").count(), 1);
                                assert!(s[0].rows[0].ranges.is_empty(), "a purpose isn't a title highlight");
                            }
                        }
                    }
                }
            }
            // A purpose never leaks an app into another scope.
            for scope in [Scope::Setting, Scope::Window, Scope::File, Scope::Command] {
                let s = sections(&ranking(label, Some(scope)), &all, std::slice::from_ref(&folder));
                assert!(s.iter().all(|s| s.scope == scope), "{role:?}, {scope:?}");
            }
        }
    }

    #[test]
    fn chosen_terminal_beats_an_exact_title_and_heavy_usage_before_truncation() {
        let mut all = desktop();
        for n in 0..70 {
            let mut rival = app("Terminal", "Terminal", &[], &[]);
            rival.key = Some(format!("app:rival-{n}"));
            rival.boost = 10_000 + n;
            all.push(rival);
        }
        for scope in [None, Some(Scope::App)] {
            let r = Ranking { per_section: 1, ..ranking("terminal", scope) };
            let s = sections(&r, &all, &[]);
            assert_eq!(first(&s), (Scope::App, "Hotaru"));
            assert_eq!(s[0].rows.len(), if scope.is_some() { 60 } else { 1 });
            if scope.is_some() {
                assert_eq!(s[0].rows[1].item.key.as_deref(), Some("app:rival-69"));
            }
        }
    }

    #[test]
    fn command_precedence_does_not_leave_settings_or_folders_ahead() {
        let chosen = app("Hako", "Files", &["files"], &["Files"]);
        let command = cmd("New", "File");
        let mut setting = Item::new(Scope::Setting, "Files", Icon::Settings, Action::None);
        setting.boost = 10_000;
        let mut folder = Item::file(Path::new("/home/person/Files"), true, Path::new("/home/person"));
        folder.boost = 20_000;
        let all = [chosen, command, setting];
        for limit in [1, 4] {
            let r = Ranking { per_section: limit, ..ranking("file", None) };
            let s = sections(&r, &all, std::slice::from_ref(&folder));
            assert_eq!(
                s.iter().map(|s| s.scope).collect::<Vec<_>>(),
                [Scope::Command, Scope::App, Scope::File, Scope::Setting]
            );
            assert_eq!(s[1].rows[0].item.title, "Hako");
        }
    }

    #[test]
    fn command_order_uses_the_best_ordinary_app_even_when_it_will_be_hidden() {
        let mut rival = app("Files", "", &[], &[]);
        rival.boost = 500;
        let all = [app("Hako", "Files", &[], &["Files"]), rival, cmd("New", "File")];
        for limit in [1, 4] {
            let r = Ranking { per_section: limit, ..ranking("file", None) };
            let s = sections(&r, &all, &[]);
            assert_eq!(first(&s), (Scope::App, "Hako"));
            assert_eq!(s[1].scope, Scope::Command, "Apps already outranked Commands before purpose priority");
        }
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
        // As `aizome-settei --list-settings --json` lists them.
        let json = r#"[{"id":"keyboard","pane":"keyboard","kind":"pane","title":"Keyboard","path":"Keyboard",
                        "keywords":["layout"],"command":["aizome-settei","--pane","keyboard"]},
                       {"id":"keyboard.caps-lock","pane":"keyboard","entry":"caps-lock","kind":"option",
                        "title":"Caps Lock key","path":"Keyboard › Caps Lock key",
                        "command":["aizome-settei","--pane","keyboard.caps-lock"]},
                       {"id":"keyboard.shortcuts","pane":"keyboard","entry":"shortcuts","kind":"option",
                        "title":"Keyboard shortcuts","path":"Keyboard › Keyboard shortcuts",
                        "command":["aizome-settei","--pane","keyboard.shortcuts"]}]"#;
        let list = parse_settings(json).unwrap();
        assert_eq!(list.len(), 3);
        let caps = setting_item(&list[1]);
        assert_eq!(caps.title, "Keyboard › Caps Lock key");
        assert_eq!(caps.action, Action::Settings { pane: "keyboard".into(), anchor: Some("caps-lock".into()) });
        let pane = setting_item(&list[0]);
        assert_eq!(pane.action, Action::Settings { pane: "keyboard".into(), anchor: None });
        // Each opens with the command Settei lists for it: an option at its
        // place, not only its pane.
        let listed: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        for (s, l) in list.iter().zip(&listed) {
            let command: Vec<String> = serde_json::from_value(l["command"].clone()).unwrap();
            let Action::Settings { pane, anchor } = setting_item(s).action else { panic!("{} isn't a setting", s.id) };
            assert_eq!(
                crate::launch::settings(&pane, anchor.as_deref()),
                crate::launch::Launch::Argv(command),
                "{}",
                s.id
            );
        }
        assert!(parse_settings(r#"{"settings":[]}"#).unwrap().is_empty());
        assert!(builtin_settings().iter().any(|s| s.id == "network"));
    }
}
