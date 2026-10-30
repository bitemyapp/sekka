//! Hotaru settings follow Aizome's per-application TOML convention.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use aizome_kit::menu::{Shortcut, ShortcutKey};
use aizome_kit::sidestep_ui::KeyEvent;
use serde::{Deserialize, Serialize};

use crate::app::Msg;

const LIMIT: u64 = 64 * 1024;
/// Typographic points (14⅔ px at 1×).
pub const DEFAULT_FONT_SIZE: f64 = 11.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Desktop,
    Night,
    Day,
    HighContrast,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopyFormat {
    Plain,
    Vt,
    Html,
    #[default]
    Mixed,
}

impl CopyFormat {
    pub fn formatter(self) -> hotaru_core::formatter::CopyFormat {
        use hotaru_core::formatter::CopyFormat as Format;
        match self {
            Self::Plain => Format::Plain,
            Self::Vt => Format::Vt,
            Self::Html => Format::Html,
            Self::Mixed => Format::Mixed,
        }
    }
}

/// The cursor programs get until they ask for another (DECSCUSR).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

impl CursorStyle {
    pub fn shape(self) -> hotaru_core::CursorShape {
        match self {
            Self::Block => hotaru_core::CursorShape::Block,
            Self::Bar => hotaru_core::CursorShape::Bar,
            Self::Underline => hotaru_core::CursorShape::Underline,
        }
    }
}

/// What a program gets when it reads or writes the clipboard itself (OSC 52
/// or Kitty's OSC 5522), as Ghostty's `clipboard-read` and `clipboard-write`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    Allow,
    Ask,
    Deny,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Ignore,
    Settings,
    NewTab,
    ClosePane,
    CloseTab,
    SplitRight,
    SplitDown,
    ZoomPane,
    PreviousPane,
    NextPane,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    PreviousTab,
    NextTab,
    #[serde(rename = "select-tab-1")]
    SelectTab1,
    #[serde(rename = "select-tab-2")]
    SelectTab2,
    #[serde(rename = "select-tab-3")]
    SelectTab3,
    #[serde(rename = "select-tab-4")]
    SelectTab4,
    #[serde(rename = "select-tab-5")]
    SelectTab5,
    #[serde(rename = "select-tab-6")]
    SelectTab6,
    #[serde(rename = "select-tab-7")]
    SelectTab7,
    #[serde(rename = "select-tab-8")]
    SelectTab8,
    LastTab,
    Copy,
    Paste,
    CopyScreen,
    CopyAll,
    Find,
    FindNext,
    FindPrevious,
    FindSelection,
    ClearScreen,
    ResetTerminal,
    LargerText,
    SmallerText,
    ActualSize,
    ScrollToTop,
    ScrollToBottom,
    ScrollPageUp,
    ScrollPageDown,
    ReloadConfig,
}

impl Action {
    pub const SELECT_TAB: [Action; 8] = [
        Action::SelectTab1,
        Action::SelectTab2,
        Action::SelectTab3,
        Action::SelectTab4,
        Action::SelectTab5,
        Action::SelectTab6,
        Action::SelectTab7,
        Action::SelectTab8,
    ];

    /// Every action the Shortcuts pane lists, by group, as menus name them.
    pub fn groups() -> [(&'static str, &'static [(Action, &'static str)]); 5] {
        use Action::*;
        [
            (
                "Tabs and panes",
                &[
                    (NewTab, "New Tab"),
                    (SplitRight, "Split Right"),
                    (SplitDown, "Split Down"),
                    (ClosePane, "Close Pane"),
                    (CloseTab, "Close Tab"),
                    (ZoomPane, "Zoom Pane"),
                ],
            ),
            (
                "Moving around",
                &[
                    (PreviousTab, "Previous Tab"),
                    (NextTab, "Next Tab"),
                    (SelectTab1, "Tab 1"),
                    (SelectTab2, "Tab 2"),
                    (SelectTab3, "Tab 3"),
                    (SelectTab4, "Tab 4"),
                    (SelectTab5, "Tab 5"),
                    (SelectTab6, "Tab 6"),
                    (SelectTab7, "Tab 7"),
                    (SelectTab8, "Tab 8"),
                    (LastTab, "Last Tab"),
                    (PreviousPane, "Previous Pane"),
                    (NextPane, "Next Pane"),
                    (FocusLeft, "Pane on the Left"),
                    (FocusRight, "Pane on the Right"),
                    (FocusUp, "Pane Above"),
                    (FocusDown, "Pane Below"),
                ],
            ),
            (
                "Edit",
                &[
                    (Copy, "Copy"),
                    (Paste, "Paste"),
                    (CopyScreen, "Copy Visible Screen"),
                    (CopyAll, "Copy All Output"),
                    (Find, "Find…"),
                    (FindNext, "Find Next"),
                    (FindPrevious, "Find Previous"),
                    (FindSelection, "Use Selection for Find"),
                    (ClearScreen, "Clear Screen"),
                ],
            ),
            (
                "View",
                &[
                    (LargerText, "Larger Text"),
                    (SmallerText, "Smaller Text"),
                    (ActualSize, "Actual Size"),
                    (ScrollToTop, "Scroll to Top"),
                    (ScrollToBottom, "Scroll to Bottom"),
                    (ScrollPageUp, "Page Up"),
                    (ScrollPageDown, "Page Down"),
                    (ResetTerminal, "Reset Terminal"),
                ],
            ),
            ("Hotaru", &[(Settings, "Settings…"), (ReloadConfig, "Reload Configuration")]),
        ]
    }

    pub fn label(self) -> &'static str {
        Self::groups()
            .into_iter()
            .flat_map(|(_, actions)| actions.iter())
            .find(|(action, _)| *action == self)
            .map_or("Nothing", |(_, label)| label)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub font_size: f64,
    pub font_weight: u16,
    pub font_families: Vec<String>,
    pub font_fallback: Vec<String>,
    pub font_emoji: Vec<String>,
    pub font_features: Vec<String>,
    pub theme: Theme,
    pub cursor_style: CursorStyle,
    /// Programs may still turn blinking on or off (DECSCUSR, DEC mode 12).
    pub cursor_blink: bool,
    pub scrollback: usize,
    pub copy_format: CopyFormat,
    /// The shell new panes start, given no arguments. Unset, it's fish where
    /// it's installed (the shell Sekka's terminals open in), else `$SHELL`.
    /// A command named at launch (`-- COMMAND`) runs instead.
    pub shell: Option<String>,
    /// Applies only to future default-shell panes; explicit commands bypass it.
    pub shell_integration: bool,
    /// Programs reading the clipboard (OSC 52 and 5522); asked by default.
    pub clipboard_read: Access,
    /// Programs writing the clipboard (OSC 52 and 5522); allowed by default.
    pub clipboard_write: Access,
    /// A finished selection also becomes the primary selection, which a
    /// middle click pastes, as other Linux programs do.
    pub copy_on_select: bool,
    /// Programs' notifications (OSC 9 and OSC 777) reach the desktop while
    /// their pane isn't the one in front.
    pub desktop_notifications: bool,
    /// Keep a pane whose program has ended, until a key closes it. Otherwise
    /// it closes, its tab with its last pane, and the window with its last tab.
    pub wait_after_command: bool,
    pub keybindings: BTreeMap<String, Action>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            font_weight: 400,
            font_families: vec!["IBM Plex Mono".into()],
            font_fallback: vec!["Noto Sans Mono CJK JP".into(), "Noto Sans Symbols 2".into()],
            font_emoji: vec!["Noto Color Emoji".into()],
            font_features: Vec::new(),
            theme: Theme::Desktop,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 10_000,
            copy_format: CopyFormat::Mixed,
            shell: None,
            shell_integration: true,
            clipboard_read: Access::Ask,
            clipboard_write: Access::Allow,
            copy_on_select: true,
            desktop_notifications: true,
            wait_after_command: false,
            keybindings: BTreeMap::new(),
        }
    }
}

/// OpenType features that turn programming ligatures off.
const NO_LIGATURES: [&str; 2] = ["calt=0", "liga=0"];

#[derive(Clone, Debug)]
pub struct Bindings {
    bindings: Vec<(Shortcut, Action)>,
    custom: Vec<(Shortcut, Action)>,
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").filter(|path| !path.is_empty()).map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|directory| directory.join("aizome/hotaru.toml"))
}

/// The shell a pane starts: `configured`, else fish on `path`, else `shell`
/// (`$SHELL`), else `/bin/sh`. Fish is the shell Sekka's terminals open in.
/// Started directly it loads Hotaru's fish integration, which a bash handing
/// itself over to fish skips, and it can't be lost to a bash that keeps
/// itself because the `SHLVL` it inherited isn't 1.
fn resolve_shell(configured: Option<&str>, path: Option<&OsStr>, shell: Option<OsString>) -> OsString {
    if let Some(configured) = configured {
        return configured.into();
    }
    let fish = path.into_iter().flat_map(std::env::split_paths).map(|directory| directory.join("fish")).find(|fish| {
        use std::os::unix::fs::PermissionsExt;
        fish.is_absolute()
            && std::fs::metadata(fish).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    });
    fish.map(PathBuf::into_os_string)
        .or_else(|| shell.filter(|shell| !shell.is_empty()))
        .unwrap_or_else(|| "/bin/sh".into())
}

impl Settings {
    /// The program a new default-shell pane runs.
    pub fn shell_program(&self) -> OsString {
        resolve_shell(self.shell.as_deref(), std::env::var_os("PATH").as_deref(), std::env::var_os("SHELL"))
    }

    /// The family the terminal asks for first.
    pub fn primary_family(&self) -> &str {
        self.font_families.first().map_or("IBM Plex Mono", String::as_str)
    }

    /// Put `family` first, keeping the rest of the list behind it.
    pub fn set_primary_family(&mut self, family: &str) {
        self.font_families.retain(|name| !name.eq_ignore_ascii_case(family));
        self.font_families.insert(0, family.to_owned());
    }

    pub fn ligatures(&self) -> bool {
        !NO_LIGATURES.iter().all(|off| self.font_features.iter().any(|feature| feature == off))
    }

    pub fn set_ligatures(&mut self, on: bool) {
        self.font_features.retain(|feature| !NO_LIGATURES.contains(&feature.as_str()));
        if !on {
            self.font_features.extend(NO_LIGATURES.iter().map(|feature| (*feature).to_owned()));
        }
    }

    pub fn fonts(&self, size: f64) -> aizome_kit::sidestep_ui::grid::FontConfig {
        aizome_kit::sidestep_ui::grid::FontConfig {
            families: self.font_families.clone(),
            fallback_families: self.font_fallback.clone(),
            emoji_families: self.font_emoji.clone(),
            features: self.font_features.clone(),
            size: size as f32,
            weight: self.font_weight,
        }
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let mut data = String::new();
        if !file.metadata().map_err(|error| error.to_string())?.is_file() {
            return Err("Configuration must be a regular file.".into());
        }
        file.take(LIMIT + 1).read_to_string(&mut data).map_err(|error| error.to_string())?;
        if data.len() as u64 > LIMIT {
            return Err("Configuration exceeds 64 KiB.".into());
        }
        Self::parse(&data)
    }

    pub fn parse(data: &str) -> Result<Self, String> {
        let settings: Self = toml::from_str(data).map_err(|error| error.to_string())?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.fonts(self.font_size).validate().map_err(|error| error.to_string())?;
        if !self.font_size.is_finite() || !(8.0..=72.0).contains(&self.font_size) {
            return Err("font_size must be between 8 and 72 points.".into());
        }
        if !(100..=900).contains(&self.font_weight) {
            return Err("font_weight must be between 100 and 900.".into());
        }
        if [&self.font_families, &self.font_fallback, &self.font_emoji].iter().any(|list| {
            list.len() > 32
                || list.iter().any(|name| name.is_empty() || name.len() > 256 || name.chars().any(char::is_control))
        }) {
            return Err("Font family lists accept at most 32 names of 1–256 characters.".into());
        }
        if self.font_features.len() > 64 || self.font_features.iter().any(|feature| feature.len() > 64) {
            return Err("At most 64 font features of 64 bytes each are supported.".into());
        }
        if self.scrollback > 1_000_000 {
            return Err("scrollback cannot exceed 1,000,000 rows.".into());
        }
        if self.shell.as_deref().is_some_and(|shell| shell.trim().is_empty() || shell.chars().any(char::is_control)) {
            return Err("shell must name a program.".into());
        }
        if self.keybindings.len() > 128 {
            return Err("At most 128 custom keybindings are supported.".into());
        }
        for key in self.keybindings.keys() {
            parse_shortcut(key)?;
        }
        Ok(())
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        self.validate()?;
        let data = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        let directory = path.parent().ok_or("Configuration has no parent directory.")?;
        if std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err("Configuration is a symbolic link; edit its source instead.".into());
        }
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = directory.join(format!(".hotaru-{}-{next}.tmp", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let mut operation = || -> std::io::Result<()> {
            if let Ok(metadata) = std::fs::metadata(path) {
                file.set_permissions(std::fs::Permissions::from_mode(metadata.permissions().mode() & 0o777))?;
            }
            file.write_all(data.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)?;
            Ok(())
        };
        let result = operation().map_err(|error| error.to_string());
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

pub fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    if value.len() > 80 {
        return Err("Keybinding is too long.".into());
    }
    let mut parts = value.split('+').map(str::trim).peekable();
    let mut shortcut = Shortcut::key(ShortcutKey::Escape);
    while let Some(part) = parts.next() {
        let part = part.to_ascii_lowercase();
        if parts.peek().is_some() {
            match part.as_str() {
                "super" | "cmd" => shortcut.command = true,
                "ctrl" | "control" => shortcut.ctrl = true,
                "alt" => shortcut.alt = true,
                "shift" => shortcut.shift = true,
                _ => return Err(format!("Unknown key modifier in {value:?}.")),
            }
        } else {
            shortcut.key = match part.as_str() {
                "enter" | "return" => ShortcutKey::Enter,
                "tab" => ShortcutKey::Tab,
                "space" => ShortcutKey::Space,
                "escape" | "esc" => ShortcutKey::Escape,
                "backspace" => ShortcutKey::Backspace,
                "delete" => ShortcutKey::Delete,
                "left" => ShortcutKey::Left,
                "right" => ShortcutKey::Right,
                "up" => ShortcutKey::Up,
                "down" => ShortcutKey::Down,
                "home" => ShortcutKey::Home,
                "end" => ShortcutKey::End,
                "pageup" => ShortcutKey::PageUp,
                "pagedown" => ShortcutKey::PageDown,
                "plus" => ShortcutKey::Char('+'),
                _ if part.starts_with('f') && part[1..].parse::<u8>().is_ok_and(|n| (1..=35).contains(&n)) => {
                    ShortcutKey::F(part[1..].parse().unwrap())
                }
                _ if part.chars().count() == 1 => ShortcutKey::Char(part.chars().next().unwrap()),
                _ => return Err(format!("Unknown key in {value:?}.")),
            };
        }
    }
    Ok(shortcut)
}

/// The configuration's spelling of `shortcut`, which `parse_shortcut` reads.
pub fn format_shortcut(shortcut: Shortcut) -> String {
    let mut out = String::new();
    for (held, name) in
        [(shortcut.command, "super"), (shortcut.ctrl, "ctrl"), (shortcut.alt, "alt"), (shortcut.shift, "shift")]
    {
        if held {
            out.push_str(name);
            out.push('+');
        }
    }
    match shortcut.key {
        ShortcutKey::Char('+') => out.push_str("plus"),
        ShortcutKey::Char(c) => out.extend(c.to_lowercase()),
        ShortcutKey::Enter => out.push_str("enter"),
        ShortcutKey::Tab => out.push_str("tab"),
        ShortcutKey::Space => out.push_str("space"),
        ShortcutKey::Escape => out.push_str("escape"),
        ShortcutKey::Backspace => out.push_str("backspace"),
        ShortcutKey::Delete => out.push_str("delete"),
        ShortcutKey::Up => out.push_str("up"),
        ShortcutKey::Down => out.push_str("down"),
        ShortcutKey::Left => out.push_str("left"),
        ShortcutKey::Right => out.push_str("right"),
        ShortcutKey::Home => out.push_str("home"),
        ShortcutKey::End => out.push_str("end"),
        ShortcutKey::PageUp => out.push_str("pageup"),
        ShortcutKey::PageDown => out.push_str("pagedown"),
        ShortcutKey::F(n) => out.push_str(&format!("f{n}")),
    }
    out
}

/// The shortcut a key press makes, as a binding names it: punctuation typed
/// with Shift by its unshifted key (⇧❖[ rather than ❖{). None for a press
/// of a modifier alone, or a key no shortcut can name.
pub fn shortcut_for(event: &KeyEvent) -> Option<Shortcut> {
    use aizome_kit::sidestep_ui::{Key, NamedKey};
    let m = event.modifiers;
    let key = match &event.key {
        Key::Character(text) => {
            let mut chars = text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else { return None };
            match event.native.unshifted.filter(|_| m.shift() && !c.is_alphabetic()) {
                Some(' ') => ShortcutKey::Space,
                Some(unshifted) => ShortcutKey::Char(unshifted),
                None if c == ' ' => ShortcutKey::Space,
                None => ShortcutKey::Char(c.to_lowercase().next().unwrap_or(c)),
            }
        }
        Key::Named(named) => match named {
            NamedKey::Enter => ShortcutKey::Enter,
            NamedKey::Tab => ShortcutKey::Tab,
            NamedKey::Escape => ShortcutKey::Escape,
            NamedKey::Backspace => ShortcutKey::Backspace,
            NamedKey::Delete => ShortcutKey::Delete,
            NamedKey::ArrowUp => ShortcutKey::Up,
            NamedKey::ArrowDown => ShortcutKey::Down,
            NamedKey::ArrowLeft => ShortcutKey::Left,
            NamedKey::ArrowRight => ShortcutKey::Right,
            NamedKey::Home => ShortcutKey::Home,
            NamedKey::End => ShortcutKey::End,
            NamedKey::PageUp => ShortcutKey::PageUp,
            NamedKey::PageDown => ShortcutKey::PageDown,
            NamedKey::F(n) if (1..=35).contains(n) => ShortcutKey::F(*n),
            _ => return None,
        },
        _ => return None,
    };
    Some(Shortcut { key, command: m.super_key(), shift: m.shift(), alt: m.alt(), ctrl: m.control() })
}

/// Shortcuts Aizome's compositor keeps for the desktop: Hotaru never hears
/// them (`aizome-comp-logic`'s default bindings).
pub fn desktop_shortcut(shortcut: Shortcut) -> bool {
    const DESKTOP: [&str; 28] = [
        "super+space",
        "super+alt+space",
        "super+alt+m",
        "super+tab",
        "super+shift+tab",
        "super+`",
        "super+h",
        "super+alt+h",
        "super+m",
        "super+ctrl+left",
        "super+ctrl+right",
        "super+ctrl+shift+left",
        "super+ctrl+shift+right",
        "super+ctrl+up",
        "super+alt+up",
        "super+alt+down",
        "super+alt+left",
        "super+alt+right",
        "super+alt+s",
        "super+alt+[",
        "super+alt+]",
        "super+ctrl+f",
        "super+enter",
        "super+ctrl+q",
        "super+ctrl+r",
        "super+shift+3",
        "super+shift+4",
        "super+shift+5",
    ];
    // ⌃❖1–9 go to a Desk, and with ⇧ take the window there.
    let desk =
        shortcut.command && shortcut.ctrl && !shortcut.alt && matches!(shortcut.key, ShortcutKey::Char('1'..='9'));
    desk || DESKTOP.iter().any(|reserved| parse_shortcut(reserved) == Ok(shortcut))
}

fn defaults() -> Vec<(Shortcut, Action)> {
    use Action::*;
    let mut defaults = vec![
        ("super+,", Settings),
        ("super+shift+,", ReloadConfig),
        ("super+t", NewTab),
        ("super+w", ClosePane),
        ("super+alt+w", CloseTab),
        ("super+d", SplitRight),
        ("super+shift+d", SplitDown),
        ("super+shift+enter", ZoomPane),
        ("super+[", PreviousPane),
        ("super+]", NextPane),
        // ⌥❖ and the arrows fill and split windows on Aizome's desktop.
        ("super+shift+left", FocusLeft),
        ("super+shift+right", FocusRight),
        ("super+shift+up", FocusUp),
        ("super+shift+down", FocusDown),
        ("super+shift+[", PreviousTab),
        ("super+shift+]", NextTab),
        // As a browser's tabs answer the command key and Page Up/Down.
        ("super+pageup", PreviousTab),
        ("super+pagedown", NextTab),
        ("super+9", LastTab),
        ("super+c", Copy),
        ("super+v", Paste),
        ("super+shift+c", CopyAll),
        ("super+f", Find),
        ("super+g", FindNext),
        ("super+shift+g", FindPrevious),
        ("super+e", FindSelection),
        ("super+k", ClearScreen),
        ("super+alt+r", ResetTerminal),
        ("super+plus", LargerText),
        ("super+=", LargerText),
        ("super+-", SmallerText),
        ("super+0", ActualSize),
        ("super+home", ScrollToTop),
        ("super+end", ScrollToBottom),
        ("shift+pageup", ScrollPageUp),
        ("shift+pagedown", ScrollPageDown),
        // Familiar Linux terminal aliases coexist with Aizome's command key,
        // with foot's (Aizome's other terminal) for find and text size.
        ("ctrl+shift+c", Copy),
        ("ctrl+shift+v", Paste),
        ("ctrl+shift+f", Find),
        ("ctrl+shift+o", SplitRight),
        ("ctrl+shift+e", SplitDown),
        ("ctrl+shift+t", NewTab),
        ("ctrl+shift+w", ClosePane),
        ("ctrl+alt+left", FocusLeft),
        ("ctrl+alt+right", FocusRight),
        ("ctrl+alt+up", FocusUp),
        ("ctrl+alt+down", FocusDown),
        ("ctrl+tab", NextTab),
        ("ctrl+shift+tab", PreviousTab),
        ("ctrl+pagedown", NextTab),
        ("ctrl+pageup", PreviousTab),
        ("ctrl+plus", LargerText),
        ("ctrl+=", LargerText),
        ("ctrl+-", SmallerText),
        ("ctrl+0", ActualSize),
    ];
    const DIGITS: [&str; 8] = ["super+1", "super+2", "super+3", "super+4", "super+5", "super+6", "super+7", "super+8"];
    defaults.extend(DIGITS.into_iter().zip(Action::SELECT_TAB));
    defaults.into_iter().map(|(key, action)| (parse_shortcut(key).unwrap(), action)).collect()
}

impl Bindings {
    pub fn new(settings: &Settings) -> Self {
        let custom: Vec<_> = settings
            .keybindings
            .iter()
            .filter_map(|(key, action)| parse_shortcut(key).ok().map(|key| (key, *action)))
            .collect();
        let mut bindings = defaults();
        for &(key, action) in &custom {
            bindings.retain(|(existing, _)| *existing != key);
            bindings.push((key, action));
        }
        Self { bindings, custom }
    }

    pub fn action(&self, key: &KeyEvent) -> Option<Action> {
        let found = self.bindings.iter().rev().find(|(shortcut, _)| shortcut.matches(key)).map(|(_, action)| *action);
        // ⇧❖[ types `{`: a shifted binding names its key by the unshifted
        // punctuation, which the kit's matching doesn't know.
        found.or_else(|| {
            let pressed = shortcut_for(key).filter(|shortcut| shortcut.shift && key.pressed)?;
            self.bindings.iter().rev().find(|(shortcut, _)| *shortcut == pressed).map(|(_, action)| *action)
        })
    }

    pub fn shortcut(&self, action: Action) -> Option<Shortcut> {
        self.custom
            .iter()
            .rev()
            .chain(self.bindings.iter())
            .find(|(_, candidate)| *candidate == action)
            .map(|(key, _)| *key)
    }

    /// Every shortcut that runs `action`, the one menus show first.
    pub fn shortcuts(&self, action: Action) -> Vec<Shortcut> {
        let mut all: Vec<Shortcut> = self.shortcut(action).into_iter().collect();
        for (key, candidate) in &self.bindings {
            if *candidate == action && !all.contains(key) {
                all.push(*key);
            }
        }
        all
    }

    /// The action `shortcut` runs now, if any.
    pub fn bound(&self, shortcut: Shortcut) -> Option<Action> {
        self.bindings
            .iter()
            .rev()
            .find(|(key, _)| *key == shortcut)
            .map(|(_, action)| *action)
            .filter(|action| *action != Action::Ignore)
    }

    /// Whether `shortcut` runs `action` without the configuration's help.
    pub fn is_default(shortcut: Shortcut, action: Action) -> bool {
        defaults().contains(&(shortcut, action))
    }
}

enum Job {
    Load(u64),
    Save(u64, Settings),
}
#[derive(Default)]
struct Queue {
    job: Option<Job>,
    stop: bool,
}
pub struct Worker {
    shared: Arc<(Mutex<Queue>, Condvar)>,
}

impl Worker {
    pub fn new(path: PathBuf, sender: aizome_kit::MsgSender<Msg>) -> std::io::Result<Self> {
        let shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let worker = shared.clone();
        std::thread::Builder::new().name("hotaru-settings".into()).spawn(move || {
            loop {
                let (lock, changed) = &*worker;
                let mut queue = lock.lock().unwrap_or_else(|error| error.into_inner());
                while queue.job.is_none() && !queue.stop {
                    queue = changed.wait(queue).unwrap_or_else(|error| error.into_inner());
                }
                if queue.stop && queue.job.is_none() {
                    break;
                }
                let job = queue.job.take().unwrap();
                drop(queue);
                match job {
                    Job::Load(token) => {
                        sender.send(Msg::ConfigLoaded(token, Settings::load(&path)));
                    }
                    Job::Save(token, settings) => {
                        sender.send(Msg::ConfigSaved(token, settings.save(&path)));
                    }
                }
            }
        })?;
        Ok(Self { shared })
    }
    fn submit(&self, job: Job) {
        let (lock, changed) = &*self.shared;
        lock.lock().unwrap_or_else(|error| error.into_inner()).job = Some(job);
        changed.notify_one();
    }
    pub fn load(&self, token: u64) {
        self.submit(Job::Load(token));
    }
    pub fn save(&self, token: u64, settings: Settings) {
        self.submit(Job::Save(token, settings));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, changed) = &*self.shared;
        lock.lock().unwrap_or_else(|error| error.into_inner()).stop = true;
        changed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_settings_and_typographical_errors() {
        for text in [
            "font_size = nan",
            "font_size = 999",
            "font_sizee = 14",
            "font_weight = 9",
            "[keybindings]\n'ctrl+wrong+q' = 'copy'",
            "clipboard_read = 'sometimes'",
        ] {
            assert!(Settings::parse(text).is_err(), "{text}");
        }
    }
    #[test]
    fn programs_clipboard_access_and_pane_endings_read_from_the_file() {
        let defaults = Settings::default();
        assert_eq!((defaults.clipboard_read, defaults.clipboard_write), (Access::Ask, Access::Allow));
        assert!(defaults.copy_on_select && defaults.desktop_notifications && !defaults.wait_after_command);
        let text = "clipboard_read = 'deny'\nclipboard_write = 'ask'\ncopy_on_select = false\n\
                    desktop_notifications = false\nwait_after_command = true";
        let settings = Settings::parse(text).unwrap();
        assert_eq!((settings.clipboard_read, settings.clipboard_write), (Access::Deny, Access::Ask));
        assert!(!settings.copy_on_select && !settings.desktop_notifications && settings.wait_after_command);
        assert_eq!(Settings::parse(&toml::to_string(&settings).unwrap()).unwrap(), settings);
    }
    #[test]
    fn panes_start_fish_where_its_installed_unless_a_shell_is_set() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("hotaru-shell-{}", std::process::id()));
        let (bin, other) = (root.join("bin"), root.join("other"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        // Not executable: not a shell to start.
        std::fs::write(other.join("fish"), "").unwrap();
        let path = std::env::join_paths([&other, &bin]).unwrap();
        let bash = || Some(OsString::from("/run/current-system/sw/bin/bash"));
        assert_eq!(resolve_shell(None, Some(&path), bash()), "/run/current-system/sw/bin/bash");
        std::fs::write(bin.join("fish"), "").unwrap();
        std::fs::set_permissions(bin.join("fish"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve_shell(None, Some(&path), bash()), bin.join("fish").into_os_string());
        assert_eq!(resolve_shell(Some("zsh"), Some(&path), bash()), "zsh");
        assert_eq!(resolve_shell(None, None, Some(OsString::new())), "/bin/sh");
        assert_eq!(Settings::parse("shell = 'zsh'").unwrap().shell.as_deref(), Some("zsh"));
        assert!(Settings::parse("shell = ' '").is_err());
        assert_eq!(Settings::default().shell, None);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn configured_binding_replaces_a_default_without_stealing_control_c() {
        let settings = Settings::parse("[keybindings]\n'super+d' = 'close-pane'\n'ctrl+shift+o' = 'ignore'").unwrap();
        let bindings = Bindings::new(&settings);
        let key = |key: &str, modifiers| KeyEvent {
            key: aizome_kit::sidestep_ui::Key::Character(key.into()),
            text: None,
            pressed: true,
            repeat: false,
            code: 0,
            modifiers,
            composing: None,
            native: Default::default(),
        };
        use aizome_kit::sidestep_ui::Modifiers;
        assert_eq!(bindings.action(&key("d", Modifiers::SUPER)), Some(Action::ClosePane));
        assert_eq!(bindings.action(&key("o", Modifiers::CONTROL | Modifiers::SHIFT)), Some(Action::Ignore));
        assert_eq!(bindings.action(&key("c", Modifiers::CONTROL)), None);
        assert_eq!(Settings::parse(&toml::to_string(&settings).unwrap()).unwrap(), settings);
    }
    fn press(key: aizome_kit::sidestep_ui::Key, modifiers: aizome_kit::sidestep_ui::Modifiers) -> KeyEvent {
        KeyEvent {
            key,
            text: None,
            pressed: true,
            repeat: false,
            code: 0,
            modifiers,
            composing: None,
            native: Default::default(),
        }
    }

    #[test]
    fn defaults_avoid_the_desktops_shortcuts_and_round_trip_through_the_file() {
        for (shortcut, action) in defaults() {
            assert!(!desktop_shortcut(shortcut), "{action:?} on {}", format_shortcut(shortcut));
            assert_eq!(parse_shortcut(&format_shortcut(shortcut)), Ok(shortcut));
        }
        assert!(desktop_shortcut(parse_shortcut("super+alt+left").unwrap()));
        assert!(desktop_shortcut(parse_shortcut("ctrl+super+4").unwrap()));
        assert!(!desktop_shortcut(parse_shortcut("super+4").unwrap()));
        let bindings = Bindings::new(&Settings::default());
        for (_, actions) in Action::groups() {
            for (action, _) in actions.iter().filter(|(action, _)| *action != Action::CopyScreen) {
                assert!(!bindings.shortcuts(*action).is_empty(), "{action:?} has no shortcut");
            }
        }
    }

    #[test]
    fn shifted_punctuation_matches_by_its_unshifted_key() {
        use aizome_kit::sidestep_ui::{Key, Modifiers};
        let bindings = Bindings::new(&Settings::default());
        let mut brace = press(Key::Character("{".into()), Modifiers::SUPER | Modifiers::SHIFT);
        brace.native.unshifted = Some('[');
        assert_eq!(bindings.action(&brace), Some(Action::PreviousTab));
        assert_eq!(shortcut_for(&brace).map(format_shortcut).as_deref(), Some("super+shift+["));
        let mut less = press(Key::Character("<".into()), Modifiers::SUPER | Modifiers::SHIFT);
        less.native.unshifted = Some(',');
        assert_eq!(bindings.action(&less), Some(Action::ReloadConfig));
        assert_eq!(bindings.action(&press(Key::Character("[".into()), Modifiers::SUPER)), Some(Action::PreviousPane));
        assert_eq!(bindings.action(&press(Key::Character("=".into()), Modifiers::SUPER)), Some(Action::LargerText));
        assert_eq!(bindings.action(&press(Key::Character("3".into()), Modifiers::SUPER)), Some(Action::SelectTab3));
        let capital = press(Key::Character("T".into()), Modifiers::SUPER | Modifiers::SHIFT);
        assert_eq!(shortcut_for(&capital).map(format_shortcut).as_deref(), Some("super+shift+t"));
    }

    #[test]
    fn ligatures_and_primary_family_edit_only_their_part_of_the_lists() {
        let mut settings = Settings { font_features: vec!["cv01=2".into()], ..Settings::default() };
        assert!(settings.ligatures());
        settings.set_ligatures(false);
        assert!(!settings.ligatures());
        assert_eq!(settings.font_features, ["cv01=2", "calt=0", "liga=0"]);
        settings.set_ligatures(true);
        assert_eq!(settings.font_features, ["cv01=2"]);
        settings.font_families = vec!["IBM Plex Mono".into(), "DejaVu Sans Mono".into()];
        settings.set_primary_family("dejavu sans mono");
        assert_eq!(settings.font_families, ["dejavu sans mono", "IBM Plex Mono"]);
        assert!(settings.validate().is_ok());
        assert_eq!(Settings::default().font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn saving_is_atomic_and_preserves_managed_symlinks() {
        let directory = std::env::temp_dir().join(format!(
            "hotaru-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("hotaru.toml");
        let settings = Settings { font_size: 19.0, theme: Theme::Day, ..Settings::default() };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
        let managed = directory.join("managed.toml");
        std::os::unix::fs::symlink(&path, &managed).unwrap();
        assert!(Settings::default().save(&managed).is_err());
        assert!(std::fs::symlink_metadata(&managed).unwrap().file_type().is_symlink());
        assert_eq!(Settings::load(&path).unwrap(), settings);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 2);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
