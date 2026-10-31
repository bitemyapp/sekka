//! Desktop entries, as the Desktop Entry Specification 1.5 defines them:
//! the `[Desktop Entry]` group's keys, `[Desktop Action id]` groups, and
//! localised values chosen for a [`Locale`].
//!
//! Parsing is lenient where the specification allows it and strict where
//! launching depends on it:
//!
//! - Malformed lines are skipped (see the key file syntax in `keyfile`).
//! - An entry needs `Type` and `Name`, except one with `Hidden=true`, which
//!   only says "deleted" and may be just that line.
//! - Localised keys (`Name[sr_RS@latin]`) are matched in the
//!   specification's order: `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`,
//!   `lang@MODIFIER`, `lang`, then the unlocalised key. The `.ENCODING`
//!   part of a locale is ignored on both sides.
//! - Values are kept with their escapes applied (`Exec` too; its own
//!   quoting rules are applied by [`Exec`](super::Exec)). Every key stays
//!   reachable through [`DesktopEntry::get`], including `X-` extensions.
//! - Actions are the ones `Actions=` lists that have a group with a `Name`;
//!   others are ignored, as the specification says.

use super::keyfile::{self, Line};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A locale for choosing localised values: `lang_COUNTRY.ENCODING@MODIFIER`
/// with every part but `lang` optional. The `C` and `POSIX` locales (and
/// the default value) choose no translation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Locale {
    lang: String,
    country: Option<String>,
    modifier: Option<String>,
}

impl Locale {
    /// Parse a locale name such as `sr_RS.UTF-8@latin`, `de_DE`, `pt` or
    /// `C.UTF-8`.
    pub fn parse(name: &str) -> Locale {
        let (lang, country, modifier) = split_locale(name.trim());
        if lang.is_empty() || lang == "C" || lang == "POSIX" {
            return Locale::default();
        }
        Locale { lang: lang.to_owned(), country: country.map(str::to_owned), modifier: modifier.map(str::to_owned) }
    }

    /// The messages locale of this process: the first non-empty of
    /// `LC_ALL`, `LC_MESSAGES` and `LANG`.
    pub fn from_env() -> Locale {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.trim().is_empty())
            .map(|value| Locale::parse(&value))
            .unwrap_or_default()
    }

    /// The locale that chooses no translations.
    pub fn untranslated() -> Locale {
        Locale::default()
    }

    /// Whether this is `C`/`POSIX` (no translations).
    pub fn is_untranslated(&self) -> bool {
        self.lang.is_empty()
    }

    /// The key suffixes this locale accepts, best first, e.g.
    /// `["sr_RS@latin", "sr_RS", "sr@latin", "sr"]`.
    pub fn candidates(&self) -> Vec<String> {
        if self.lang.is_empty() {
            return Vec::new();
        }
        let lang = &self.lang;
        let mut names = Vec::with_capacity(4);
        if let (Some(country), Some(modifier)) = (&self.country, &self.modifier) {
            names.push(format!("{lang}_{country}@{modifier}"));
        }
        if let Some(country) = &self.country {
            names.push(format!("{lang}_{country}"));
        }
        if let Some(modifier) = &self.modifier {
            names.push(format!("{lang}@{modifier}"));
        }
        names.push(lang.clone());
        names
    }

    /// How well a key's locale suffix matches: 0 best, 3 for the bare
    /// language, `None` if it doesn't apply to this locale at all.
    pub(crate) fn priority(&self, suffix: &str) -> Option<u8> {
        // Most localised lines are for other languages: reject them before
        // splitting anything.
        let rest = suffix.strip_prefix(self.lang.as_str())?;
        if self.lang.is_empty() || !matches!(rest.bytes().next(), None | Some(b'_' | b'.' | b'@')) {
            return None;
        }
        let (lang, country, modifier) = split_locale(suffix);
        if lang != self.lang {
            return None;
        }
        match (country, modifier) {
            (Some(c), Some(m)) if self.country.as_deref() == Some(c) && self.modifier.as_deref() == Some(m) => Some(0),
            (Some(c), None) if self.country.as_deref() == Some(c) => Some(1),
            (None, Some(m)) if self.modifier.as_deref() == Some(m) => Some(2),
            (None, None) => Some(3),
            _ => None,
        }
    }
}

/// `lang`, `COUNTRY` and `MODIFIER` of `lang_COUNTRY.ENCODING@MODIFIER`.
fn split_locale(name: &str) -> (&str, Option<&str>, Option<&str>) {
    let (rest, modifier) = match name.split_once('@') {
        Some((rest, modifier)) => (rest, Some(modifier).filter(|m| !m.is_empty())),
        None => (name, None),
    };
    let rest = rest.split_once('.').map_or(rest, |(before, _encoding)| before);
    match rest.split_once('_') {
        Some((lang, country)) => (lang, Some(country).filter(|c| !c.is_empty()), modifier),
        None => (rest, None, modifier),
    }
}

/// What a desktop entry describes (`Type=`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// `Type=Application`: something to launch.
    Application,
    /// `Type=Link`: a URL to open ([`DesktopEntry::url`]).
    Link,
    /// `Type=Directory`: a menu directory.
    Directory,
    /// Another type, for forward compatibility (empty for a `Hidden=true`
    /// entry without one).
    Other(String),
}

/// An application action: a `[Desktop Action id]` group listed in
/// `Actions=`, such as Firefox's "New Private Window".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// The action's identifier (the group name's suffix).
    pub id: String,
    /// The localised `Name`.
    pub name: String,
    /// The localised `Icon`, if any.
    pub icon: Option<String>,
    /// `Exec`, escapes applied (field codes and quoting still in place).
    pub exec: Option<String>,
}

/// One key's raw values: unlocalised, and the best localised one for the
/// entry's locale with its priority.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct KeyValue {
    plain: Option<String>,
    localized: Option<(u8, String)>,
}

impl KeyValue {
    fn best(&self) -> Option<&str> {
        self.localized.as_ref().map(|(_, v)| v.as_str()).or(self.plain.as_deref())
    }

    fn offer(&mut self, priority: u8, raw: &str) {
        if self.localized.as_ref().is_none_or(|(best, _)| priority <= *best) {
            self.localized = Some((priority, raw.to_owned()));
        }
    }
}

/// A parsed desktop entry (`.desktop` file).
///
/// Fields are read-only views of the standard keys, already localised for
/// the locale it was parsed with and with escapes applied; every key
/// (standard or `X-` extension) is also available through
/// [`get`](Self::get) and its siblings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DesktopEntry {
    /// The desktop file ID, such as `org.gnome.Nautilus.desktop`.
    pub id: String,
    /// The file it was read from, if any.
    pub path: Option<PathBuf>,
    /// `Type`.
    pub kind: EntryKind,
    /// `Name`, localised.
    pub name: String,
    /// `GenericName`, localised ("Web Browser").
    pub generic_name: Option<String>,
    /// `Comment`, localised.
    pub comment: Option<String>,
    /// `Keywords`, localised.
    pub keywords: Vec<String>,
    /// `Categories`.
    pub categories: Vec<String>,
    /// `Icon`: an icon name, or an absolute path.
    pub icon: Option<String>,
    /// `Exec`, escapes applied but quoting and field codes still in place;
    /// see [`Exec`](super::Exec).
    pub exec: Option<String>,
    /// `TryExec`.
    pub try_exec: Option<String>,
    /// `Path`: the working directory to run the program in.
    pub working_dir: Option<PathBuf>,
    /// `Terminal`: run in a terminal emulator.
    pub terminal: bool,
    /// `NoDisplay`: exists (for MIME handling, say) but isn't shown in
    /// menus.
    pub no_display: bool,
    /// `Hidden`: deleted; hides entries with the same ID in less important
    /// directories.
    pub hidden: bool,
    /// `OnlyShowIn`.
    pub only_show_in: Vec<String>,
    /// `NotShowIn`.
    pub not_show_in: Vec<String>,
    /// `StartupWMClass`: the window class or app ID its windows have.
    pub startup_wm_class: Option<String>,
    /// `StartupNotify`, if given.
    pub startup_notify: Option<bool>,
    /// `MimeType`.
    pub mime_types: Vec<String>,
    /// `DBusActivatable`.
    pub dbus_activatable: bool,
    /// `URL`, for `Type=Link`.
    pub url: Option<String>,
    /// The actions from `Actions=`, in that order.
    pub actions: Vec<Action>,
    keys: HashMap<String, KeyValue>,
}

impl DesktopEntry {
    /// Parse desktop entry text with the desktop file ID `id`, choosing
    /// localised values for `locale`.
    ///
    /// Fails with [`Error::InvalidArgument`] if there is no
    /// `[Desktop Entry]` group, or (unless `Hidden=true`) no `Type` or
    /// `Name`.
    pub fn parse(text: &str, id: &str, locale: &Locale) -> Result<DesktopEntry> {
        #[derive(Clone, Copy, PartialEq)]
        enum Group {
            None,
            Entry,
            Action(usize),
        }
        let mut group = Group::None;
        let mut seen_entry = false;
        let mut keys: HashMap<String, KeyValue> = HashMap::new();
        let mut action_groups: Vec<(String, HashMap<String, KeyValue>)> = Vec::new();
        for line in keyfile::lines(text) {
            match line {
                Line::Group(name) => {
                    group = if name == "Desktop Entry" {
                        seen_entry = true;
                        Group::Entry
                    } else if let Some(action) = name.strip_prefix("Desktop Action ") {
                        match action_groups.iter().position(|(id, _)| id == action) {
                            Some(index) => Group::Action(index),
                            None => {
                                action_groups.push((action.to_owned(), HashMap::new()));
                                Group::Action(action_groups.len() - 1)
                            }
                        }
                    } else {
                        Group::None
                    };
                }
                Line::Entry { key, locale: suffix, value } => {
                    let map = match group {
                        Group::Entry => &mut keys,
                        Group::Action(index) => match action_groups.get_mut(index) {
                            Some((_, map)) => map,
                            None => continue,
                        },
                        Group::None => continue,
                    };
                    match suffix {
                        None => map.entry(key.to_owned()).or_default().plain = Some(value.to_owned()),
                        Some(suffix) => {
                            if let Some(priority) = locale.priority(suffix) {
                                map.entry(key.to_owned()).or_default().offer(priority, value);
                            }
                        }
                    }
                }
                Line::Other => {}
            }
        }
        if !seen_entry {
            return Err(Error::InvalidArgument(format!("{id}: no [Desktop Entry] group")));
        }

        let string = |key: &str| keys.get(key).and_then(|v| v.plain.as_deref()).map(keyfile::unescape);
        let localized = |key: &str| keys.get(key).and_then(KeyValue::best).map(keyfile::unescape);
        let list =
            |key: &str| keys.get(key).and_then(|v| v.plain.as_deref()).map(keyfile::split_list).unwrap_or_default();
        let localized_list =
            |key: &str| keys.get(key).and_then(KeyValue::best).map(keyfile::split_list).unwrap_or_default();
        let boolean = |key: &str| keys.get(key).and_then(|v| v.plain.as_deref()).and_then(keyfile::parse_bool);
        let non_empty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());

        let hidden = boolean("Hidden").unwrap_or(false);
        let kind = match string("Type").as_deref() {
            Some("Application") => EntryKind::Application,
            Some("Link") => EntryKind::Link,
            Some("Directory") => EntryKind::Directory,
            Some(other) if !other.is_empty() => EntryKind::Other(other.to_owned()),
            _ if hidden => EntryKind::Other(String::new()),
            _ => return Err(Error::InvalidArgument(format!("{id}: no Type"))),
        };
        let name = match non_empty(localized("Name")) {
            Some(name) => name,
            None if hidden => String::new(),
            None => return Err(Error::InvalidArgument(format!("{id}: no Name"))),
        };

        let action_ids = list("Actions");
        let mut actions = Vec::new();
        for action_id in action_ids {
            if actions.iter().any(|a: &Action| a.id == action_id) {
                continue;
            }
            let Some((_, map)) = action_groups.iter().find(|(id, _)| *id == action_id) else {
                continue;
            };
            let value = |key: &str| map.get(key).and_then(KeyValue::best).map(keyfile::unescape);
            let Some(name) = non_empty(value("Name")) else {
                continue;
            };
            let exec = map.get("Exec").and_then(|v| v.plain.as_deref()).map(keyfile::unescape);
            actions.push(Action { id: action_id, name, icon: non_empty(value("Icon")), exec: non_empty(exec) });
        }

        Ok(DesktopEntry {
            id: id.to_owned(),
            path: None,
            name,
            generic_name: non_empty(localized("GenericName")),
            comment: non_empty(localized("Comment")),
            keywords: localized_list("Keywords"),
            categories: list("Categories"),
            icon: non_empty(localized("Icon")),
            exec: non_empty(string("Exec")),
            try_exec: non_empty(string("TryExec")),
            working_dir: non_empty(string("Path")).map(PathBuf::from),
            terminal: boolean("Terminal").unwrap_or(false),
            no_display: boolean("NoDisplay").unwrap_or(false),
            hidden,
            only_show_in: list("OnlyShowIn"),
            not_show_in: list("NotShowIn"),
            startup_wm_class: non_empty(string("StartupWMClass")),
            startup_notify: boolean("StartupNotify"),
            mime_types: list("MimeType"),
            dbus_activatable: boolean("DBusActivatable").unwrap_or(false),
            url: non_empty(string("URL")),
            actions,
            kind,
            keys,
        })
    }

    /// Read and parse the file at `path` (invalid UTF-8 replaced), with the
    /// desktop file ID `id`.
    pub fn load(path: &Path, id: &str, locale: &Locale) -> Result<DesktopEntry> {
        let bytes = std::fs::read(path)?;
        let text = String::from_utf8_lossy(&bytes);
        let mut entry = DesktopEntry::parse(&text, id, locale)?;
        entry.path = Some(path.to_owned());
        Ok(entry)
    }

    /// The application ID: the desktop file ID without `.desktop`
    /// (`org.gnome.Nautilus`), as D-Bus activation and Wayland app IDs use
    /// it.
    pub fn app_id(&self) -> &str {
        self.id.strip_suffix(".desktop").unwrap_or(&self.id)
    }

    /// The unlocalised value of any key in `[Desktop Entry]`, escapes
    /// applied (`entry.get("X-GNOME-UsesNotifications")`).
    pub fn get(&self, key: &str) -> Option<String> {
        self.keys.get(key)?.plain.as_deref().map(keyfile::unescape)
    }

    /// The value of `key` localised for the entry's locale, falling back
    /// to the unlocalised value.
    pub fn get_localized(&self, key: &str) -> Option<String> {
        self.keys.get(key)?.best().map(keyfile::unescape)
    }

    /// The unlocalised value of `key` as a `;`-separated list.
    pub fn get_list(&self, key: &str) -> Option<Vec<String>> {
        self.keys.get(key)?.plain.as_deref().map(keyfile::split_list)
    }

    /// The unlocalised value of `key` as a boolean (`None` if missing or
    /// not a boolean).
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.keys.get(key)?.plain.as_deref().and_then(keyfile::parse_bool)
    }

    /// The raw value of `key`, escapes still in place.
    pub fn get_raw(&self, key: &str) -> Option<&str> {
        self.keys.get(key)?.plain.as_deref()
    }

    /// The names of every key the `[Desktop Entry]` group sets (without
    /// locale suffixes), in no particular order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.keys.keys().map(String::as_str)
    }

    /// The action with this ID.
    pub fn action(&self, id: &str) -> Option<&Action> {
        self.actions.iter().find(|action| action.id == id)
    }

    /// Whether `Categories` lists `category`.
    pub fn has_category(&self, category: &str) -> bool {
        self.categories.iter().any(|c| c == category)
    }

    /// Whether `MimeType` lists `mime` (compared case-insensitively, as
    /// MIME types are).
    pub fn supports_mime(&self, mime: &str) -> bool {
        self.mime_types.iter().any(|m| m.eq_ignore_ascii_case(mime))
    }

    /// Whether `OnlyShowIn`/`NotShowIn` allow showing the entry on these
    /// desktops (most important first): the first desktop either list
    /// names decides; if neither names any, it shows unless `OnlyShowIn`
    /// is set.
    pub fn shown_in(&self, desktops: &[String]) -> bool {
        for desktop in desktops {
            if self.only_show_in.iter().any(|d| d == desktop) {
                return true;
            }
            if self.not_show_in.iter().any(|d| d == desktop) {
                return false;
            }
        }
        self.only_show_in.is_empty()
    }

    /// Whether `TryExec` (if any) names an executable file: an absolute
    /// path, or a program in `PATH`.
    pub fn try_exec_found(&self) -> bool {
        match &self.try_exec {
            None => true,
            Some(program) if program.starts_with('/') => super::xdg::is_executable(Path::new(program)),
            Some(program) if program.contains('/') => false,
            Some(program) => super::xdg::find_program(program).is_some(),
        }
    }

    /// Whether this is an application that should appear in menus on
    /// these desktops: `Type=Application`, not `NoDisplay` or `Hidden`,
    /// allowed by `OnlyShowIn`/`NotShowIn`, and with something to run
    /// (`Exec`, or `DBusActivatable`). `TryExec` is checked separately
    /// ([`try_exec_found`](Self::try_exec_found)) since it looks at the
    /// file system.
    pub fn should_show(&self, desktops: &[String]) -> bool {
        self.kind == EntryKind::Application
            && !self.no_display
            && !self.hidden
            && (self.exec.is_some() || self.dbus_activatable)
            && self.shown_in(desktops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &str = r#"# A comment
[Desktop Entry]
Version=1.5
Type=Application
Name=Files
Name[de]=Dateien
Name[sr]=Датотеке
Name[sr@latin]=Datoteke
Name[sr_RS]=Датотеке (RS)
Name[sr_RS@latin]=Datoteke (RS)
Name[pt_BR.UTF-8]=Arquivos
GenericName=File Manager
Comment=Access and organize files
Comment[de]=Dateien öffnen\sund\sverwalten
Keywords=folder;manager;explore;disk;filesystem;
Keywords[de]=Ordner;Verwaltung;
Exec=nautilus --new-window %U
Icon=org.gnome.Nautilus
Terminal=false
StartupNotify=true
Categories=GNOME;GTK;Utility;Core;FileManager;
MimeType=inode/directory;application/x-7z-compressed;
DBusActivatable=true
X-GNOME-UsesNotifications=true
X-Escaped=one\;two;three\sfour
Actions=new-window;missing;nameless;

[Desktop Action new-window]
Name=New Window
Name[de]=Neues Fenster
Exec=nautilus --new-window

[Desktop Action nameless]
Exec=nautilus

[X-Other Group]
Name=Not this one
"#;

    fn parse(text: &str, locale: &str) -> DesktopEntry {
        DesktopEntry::parse(text, "org.gnome.Nautilus.desktop", &Locale::parse(locale)).expect("valid entry")
    }

    #[test]
    fn locales_parse() {
        let l = Locale::parse("sr_RS.UTF-8@latin");
        assert_eq!(l.candidates(), ["sr_RS@latin", "sr_RS", "sr@latin", "sr"]);
        assert_eq!(Locale::parse("de_DE.UTF-8").candidates(), ["de_DE", "de"]);
        assert_eq!(Locale::parse("sr@latin").candidates(), ["sr@latin", "sr"]);
        assert_eq!(Locale::parse("fr").candidates(), ["fr"]);
        assert!(Locale::parse("C.UTF-8").is_untranslated());
        assert!(Locale::parse("POSIX").is_untranslated());
        assert!(Locale::parse("").is_untranslated());
        assert_eq!(Locale::parse("C").candidates(), Vec::<String>::new());
    }

    #[test]
    fn localised_keys_follow_the_matching_order() {
        assert_eq!(parse(FILES, "sr_RS.UTF-8@latin").name, "Datoteke (RS)");
        assert_eq!(parse(FILES, "sr_RS").name, "Датотеке (RS)");
        assert_eq!(parse(FILES, "sr_ME@latin").name, "Datoteke");
        assert_eq!(parse(FILES, "sr_ME").name, "Датотеке");
        assert_eq!(parse(FILES, "de_AT.UTF-8").name, "Dateien");
        assert_eq!(parse(FILES, "pt_BR").name, "Arquivos");
        assert_eq!(parse(FILES, "pt_PT").name, "Files");
        assert_eq!(parse(FILES, "C").name, "Files");
        assert_eq!(parse(FILES, "en_US.UTF-8").name, "Files");
        let de = parse(FILES, "de_DE");
        assert_eq!(de.comment.as_deref(), Some("Dateien öffnen und verwalten"));
        assert_eq!(de.keywords, ["Ordner", "Verwaltung"]);
        assert_eq!(de.actions[0].name, "Neues Fenster");
        assert_eq!(de.get("Name").as_deref(), Some("Files"));
        assert_eq!(de.get_localized("Name").as_deref(), Some("Dateien"));
    }

    #[test]
    fn standard_keys() {
        let e = parse(FILES, "C");
        assert_eq!(e.kind, EntryKind::Application);
        assert_eq!(e.app_id(), "org.gnome.Nautilus");
        assert_eq!(e.generic_name.as_deref(), Some("File Manager"));
        assert_eq!(e.keywords, ["folder", "manager", "explore", "disk", "filesystem"]);
        assert_eq!(e.categories.len(), 5);
        assert!(e.has_category("FileManager"));
        assert_eq!(e.exec.as_deref(), Some("nautilus --new-window %U"));
        assert_eq!(e.icon.as_deref(), Some("org.gnome.Nautilus"));
        assert!(!e.terminal && !e.no_display && !e.hidden);
        assert_eq!(e.startup_notify, Some(true));
        assert!(e.dbus_activatable);
        assert!(e.supports_mime("inode/directory"));
        assert!(e.supports_mime("Inode/Directory"));
        assert!(!e.supports_mime("text/plain"));
        // Unknown keys stay reachable.
        assert_eq!(e.get_bool("X-GNOME-UsesNotifications"), Some(true));
        assert_eq!(e.get_list("X-Escaped").unwrap(), ["one;two", "three four"]);
        assert_eq!(e.get_raw("X-Escaped"), Some(r"one\;two;three\sfour"));
        assert_eq!(e.get("Nope"), None);
        assert!(e.keys().any(|k| k == "Version"));
        // Only listed actions with a group and a Name.
        assert_eq!(e.actions.len(), 1);
        let action = e.action("new-window").unwrap();
        assert_eq!(action.exec.as_deref(), Some("nautilus --new-window"));
        assert_eq!(action.icon, None);
        assert!(e.action("nameless").is_none());
        assert!(e.should_show(&["Aizome".into()]));
    }

    #[test]
    fn required_keys_and_hidden_entries() {
        let err = |text: &str| DesktopEntry::parse(text, "x.desktop", &Locale::default()).unwrap_err();
        assert!(matches!(err("Name=x\nType=Application\n"), Error::InvalidArgument(_)));
        assert!(matches!(err("[Desktop Entry]\nType=Application\n"), Error::InvalidArgument(_)));
        assert!(matches!(err("[Desktop Entry]\nName=x\n"), Error::InvalidArgument(_)));
        assert!(matches!(err("[Desktop Entry]\nName=\nType=Application"), Error::InvalidArgument(_)));
        assert!(matches!(err(""), Error::InvalidArgument(_)));
        // A deletion marker may be just that.
        let hidden = DesktopEntry::parse("[Desktop Entry]\nHidden=true\n", "x.desktop", &Locale::default()).unwrap();
        assert!(hidden.hidden);
        assert!(!hidden.should_show(&[]));
        let link = DesktopEntry::parse(
            "[Desktop Entry]\nType=Link\nName=Docs\nURL=https://example.org/a\\sb\n",
            "docs.desktop",
            &Locale::default(),
        )
        .unwrap();
        assert_eq!(link.kind, EntryKind::Link);
        assert_eq!(link.url.as_deref(), Some("https://example.org/a b"));
        assert!(!link.should_show(&[]));
    }

    #[test]
    fn odd_data_does_not_break_parsing() {
        let text = "garbage before\n[Desktop Entry\n[Desktop Entry]\nType=Application\n\
                    Name=Odd\nName[=broken\nTerminal=maybe\nNoDisplay=1\n=x\nExec=\n\
                    Actions=;;a;a;\n[Desktop Action a]\nName=A\nIcon=\n\u{0}\u{1}";
        let e = DesktopEntry::parse(text, "odd.desktop", &Locale::parse("de")).unwrap();
        assert_eq!(e.name, "Odd");
        assert!(!e.terminal);
        assert!(e.no_display);
        assert_eq!(e.exec, None);
        assert_eq!(e.actions.len(), 1);
        assert_eq!(e.actions[0].icon, None);
        assert!(!e.should_show(&[]));
    }

    #[test]
    fn show_in_rules() {
        let mut e = parse(FILES, "C");
        let desktops = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        e.only_show_in = vec!["GNOME".into()];
        assert!(!e.shown_in(&desktops(&["Aizome"])));
        assert!(e.shown_in(&desktops(&["GNOME", "Aizome"])));
        e.only_show_in.clear();
        e.not_show_in = vec!["Aizome".into()];
        assert!(!e.shown_in(&desktops(&["Hyprland", "Aizome"])));
        assert!(e.shown_in(&desktops(&["GNOME"])));
        // The first desktop either list names decides.
        e.only_show_in = vec!["KDE".into()];
        assert!(e.shown_in(&desktops(&["KDE", "Aizome"])));
        assert!(!e.shown_in(&desktops(&["Aizome", "KDE"])));
    }

    #[test]
    fn try_exec() {
        let mut e = parse(FILES, "C");
        assert!(e.try_exec_found());
        e.try_exec = Some("/definitely/not/here".into());
        assert!(!e.try_exec_found());
        e.try_exec = Some("relative/path".into());
        assert!(!e.try_exec_found());
        e.try_exec = Some("sh".into());
        assert!(e.try_exec_found());
        e.try_exec = Some("no-such-program-anywhere-xyz".into());
        assert!(!e.try_exec_found());
    }
}
