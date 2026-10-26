//! The system's shortcuts: the compositor's default chords and action
//! names (as `aizome-comp`'s `bindings::defaults` lists them, and
//! `docs/ipc.md` documents them), the `[keys.bindings]` overrides on top,
//! and chords parsed, compared, printed with Aizome's glyphs and recorded
//! from key presses.

use std::collections::BTreeMap;

use aizome_kit::sidestep_ui::{Key, KeyEvent, Modifiers, NamedKey};

/// A chord: modifiers and a key (an XKB keysym name, lowercase for
/// letters), as the compositor parses them.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    pub key: String,
}

impl Chord {
    /// `"Ctrl+Super+Up"`, `"super+h"`, `"XF86AudioMute"`, `"⌃❖↑"`-free.
    pub fn parse(s: &str) -> Option<Chord> {
        let parts: Vec<&str> = s.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
        let (key, mods) = parts.split_last()?;
        let mut c = Chord { ctrl: false, alt: false, shift: false, logo: false, key: canonical_key(key) };
        for m in mods {
            match m.to_lowercase().as_str() {
                "ctrl" | "control" => c.ctrl = true,
                "alt" | "option" | "opt" => c.alt = true,
                "shift" => c.shift = true,
                "super" | "logo" | "cmd" | "mod4" | "win" => c.logo = true,
                _ => return None,
            }
        }
        (!c.key.is_empty()).then_some(c)
    }

    /// The form settings.toml keeps: `"Ctrl+Shift+Super+Left"`.
    pub fn to_config(&self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+"), (self.logo, "Super+")] {
            if on {
                s.push_str(name);
            }
        }
        let key = if self.key.chars().count() == 1 { self.key.to_uppercase() } else { self.key.clone() };
        s.push_str(&key);
        s
    }

    /// The modifiers' glyphs in Aizome's order (⌃ ⌥ ⇧ ❖) and the key's.
    pub fn glyphs(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (on, g) in [
            (self.ctrl, aizome_theme::glyphs::CONTROL),
            (self.alt, aizome_theme::glyphs::ALT),
            (self.shift, aizome_theme::glyphs::SHIFT),
            (self.logo, aizome_theme::glyphs::SUPER),
        ] {
            if on {
                v.push(g.to_owned());
            }
        }
        v.push(key_label(&self.key));
        v
    }

    /// Whether any of ⌃ ⌥ ⇧ ❖ is held.
    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.logo
    }

    /// A key that works without a modifier (function, media, Print).
    pub fn stands_alone(&self) -> bool {
        self.key.starts_with("XF86")
            || self.key == "Print"
            || (self.key.starts_with('F') && self.key[1..].parse::<u8>().is_ok())
    }
}

/// Keys as the compositor's parser and XKB name them.
fn canonical_key(k: &str) -> String {
    let lower = k.to_lowercase();
    match lower.as_str() {
        "up" | "↑" => "Up".into(),
        "down" | "↓" => "Down".into(),
        "left" | "←" => "Left".into(),
        "right" | "→" => "Right".into(),
        "return" | "enter" | "↩" => "Return".into(),
        "space" => "Space".into(),
        "tab" | "⇥" => "Tab".into(),
        "grave" | "`" => "Grave".into(),
        "escape" | "esc" => "Escape".into(),
        "print" => "Print".into(),
        "backspace" => "BackSpace".into(),
        "delete" => "Delete".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "page_up" | "prior" => "Page_Up".into(),
        "page_down" | "next" => "Page_Down".into(),
        _ if k.chars().count() == 1 => lower,
        _ if lower.starts_with('f') && lower[1..].parse::<u8>().is_ok() => lower.to_uppercase(),
        _ => k.to_owned(),
    }
}

/// How a key shows on a keycap.
pub fn key_label(k: &str) -> String {
    match k {
        "Up" => "↑".into(),
        "Down" => "↓".into(),
        "Left" => "←".into(),
        "Right" => "→".into(),
        "Return" => "↩".into(),
        "Tab" => "⇥".into(),
        "Space" => "Space".into(),
        "Grave" => "`".into(),
        "Escape" => "⎋".into(),
        "BackSpace" => "⌫".into(),
        "Print" => "Print".into(),
        "XF86AudioRaiseVolume" => "Volume Up".into(),
        "XF86AudioLowerVolume" => "Volume Down".into(),
        "XF86AudioMute" => "Mute".into(),
        "XF86AudioMicMute" => "Mic Mute".into(),
        "XF86MonBrightnessUp" => "Brightness Up".into(),
        "XF86MonBrightnessDown" => "Brightness Down".into(),
        "XF86AudioPlay" => "Play".into(),
        "XF86AudioPause" => "Pause".into(),
        "XF86AudioNext" => "Next".into(),
        "XF86AudioPrev" => "Previous".into(),
        "XF86AudioStop" => "Stop".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_owned(),
    }
}

/// The compositor's default chords and their actions: `aizome-comp`'s
/// `bindings::defaults`, which must stay in step (a test of the
/// compositor's checks the names; here, `defaults_cover_the_documented_keys`).
pub fn defaults() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = [
        ("Super+Space", "palette"),
        ("Super+Alt+Space", "toolchest"),
        ("Super+Tab", "switcher"),
        ("Super+Shift+Tab", "switcher-reverse"),
        ("Super+Grave", "next-window-of-app"),
        ("Super+H", "hide-app"),
        ("Super+Alt+H", "hide-others"),
        ("Super+M", "minimise"),
        ("Ctrl+Super+Left", "previous-desk"),
        ("Ctrl+Super+Right", "next-desk"),
        ("Ctrl+Shift+Super+Left", "take-to-previous-desk"),
        ("Ctrl+Shift+Super+Right", "take-to-next-desk"),
        ("Ctrl+Super+Up", "overview"),
        ("Alt+Super+Up", "fill"),
        ("Alt+Super+Down", "minimise"),
        ("Alt+Super+Left", "split-left"),
        ("Alt+Super+Right", "split-right"),
        ("Alt+Super+S", "stack-with-behind"),
        ("Ctrl+Super+F", "fullscreen"),
        ("Super+Return", "terminal"),
        ("Ctrl+Super+Q", "lock"),
        ("Super+Shift+3", "screenshot-screen"),
        ("Super+Shift+4", "screenshot-area"),
        ("Super+Shift+5", "screenshot-tool"),
        ("Print", "screenshot-area"),
        ("Alt+Print", "record"),
        ("XF86AudioRaiseVolume", "volume-up"),
        ("XF86AudioLowerVolume", "volume-down"),
        ("XF86AudioMute", "mute"),
        ("XF86AudioMicMute", "mic-mute"),
        ("XF86MonBrightnessUp", "brightness-up"),
        ("XF86MonBrightnessDown", "brightness-down"),
        ("XF86AudioPlay", "play-pause"),
        ("XF86AudioPause", "play-pause"),
        ("XF86AudioNext", "next"),
        ("XF86AudioPrev", "previous"),
        ("XF86AudioStop", "stop"),
    ]
    .iter()
    .map(|(c, a)| ((*c).to_owned(), (*a).to_owned()))
    .collect();
    for n in 1..=9 {
        v.push((format!("Ctrl+Super+{n}"), format!("go-to-desk-{n}")));
        v.push((format!("Ctrl+Shift+Super+{n}"), format!("take-to-desk-{n}")));
    }
    v
}

/// A group of actions as the editor lists them.
pub struct Section {
    pub title: &'static str,
    pub actions: Vec<(String, String)>,
}

/// The actions the editor lists, by group, with what each is called.
pub fn sections(desks: usize) -> Vec<Section> {
    let s = |title, list: &[(&str, &str)]| Section {
        title,
        actions: list.iter().map(|(a, l)| ((*a).to_owned(), (*l).to_owned())).collect(),
    };
    let mut desk_actions: Vec<(String, String)> = vec![
        ("overview".into(), "Desks overview".into()),
        ("previous-desk".into(), "Previous Desk".into()),
        ("next-desk".into(), "Next Desk".into()),
        ("take-to-previous-desk".into(), "Take the window to the previous Desk".into()),
        ("take-to-next-desk".into(), "Take the window to the next Desk".into()),
    ];
    for n in 1..=desks.clamp(1, 9) {
        desk_actions.push((format!("go-to-desk-{n}"), format!("Go to Desk {n}")));
    }
    for n in 1..=desks.clamp(1, 9) {
        desk_actions.push((format!("take-to-desk-{n}"), format!("Take the window to Desk {n}")));
    }
    vec![
        s("Finding", &[("palette", "Command palette"), ("toolchest", "Toolchest")]),
        s(
            "Windows",
            &[
                ("switcher", "Switch apps"),
                ("switcher-reverse", "Switch apps backwards"),
                ("next-window-of-app", "Next window of the app"),
                ("hide-app", "Hide the app"),
                ("hide-others", "Hide the others"),
                ("minimise", "Minimise"),
                ("fill", "Fill the screen"),
                ("split-left", "Split left"),
                ("split-right", "Split right"),
                ("stack-with-behind", "Stack with the window behind"),
                ("fullscreen", "Full screen"),
            ],
        ),
        Section { title: "Desks", actions: desk_actions },
        s(
            "Screenshots",
            &[
                ("screenshot-screen", "The whole screen"),
                ("screenshot-area", "An area"),
                ("screenshot-tool", "Screenshot and recording tools"),
            ],
        ),
        s("Screen Recording", &[("record", "Record an area or a display, or stop")]),
        s("Session", &[("terminal", "Open a terminal"), ("lock", "Lock the screen")]),
        s(
            "Media Keys",
            &[
                ("volume-up", "Volume up"),
                ("volume-down", "Volume down"),
                ("mute", "Mute"),
                ("mic-mute", "Mute the microphone"),
                ("brightness-up", "Brighter"),
                ("brightness-down", "Dimmer"),
                ("play-pause", "Play or pause"),
                ("next", "Next track"),
                ("previous", "Previous track"),
            ],
        ),
    ]
}

/// What each chord does now: the defaults with `overrides` on top
/// (`"none"` unbinds).
pub fn effective(overrides: &BTreeMap<String, String>) -> BTreeMap<Chord, String> {
    let mut map: BTreeMap<Chord, String> = BTreeMap::new();
    for (c, a) in defaults() {
        if let Some(c) = Chord::parse(&c) {
            map.insert(c, a);
        }
    }
    for (c, a) in overrides {
        let Some(c) = Chord::parse(c) else { continue };
        if a.trim() == "none" {
            map.remove(&c);
        } else {
            map.insert(c, a.trim().to_owned());
        }
    }
    map
}

/// The overrides that turn the defaults into `wanted`.
pub fn overrides_for(wanted: &BTreeMap<Chord, String>) -> BTreeMap<String, String> {
    let defaults = effective(&BTreeMap::new());
    let mut out = BTreeMap::new();
    for (c, a) in wanted {
        if defaults.get(c) != Some(a) {
            out.insert(c.to_config(), a.clone());
        }
    }
    for c in defaults.keys() {
        if !wanted.contains_key(c) {
            out.insert(c.to_config(), "none".to_owned());
        }
    }
    out
}

/// The chords doing `action`.
pub fn chords_for(map: &BTreeMap<Chord, String>, action: &str) -> Vec<Chord> {
    map.iter().filter(|(_, a)| a.as_str() == action).map(|(c, _)| c.clone()).collect()
}

/// `action` on `chord` alone: its other chords dropped, and whatever
/// `chord` did before replaced. Returns the action it replaced, if any.
pub fn rebind(map: &mut BTreeMap<Chord, String>, action: &str, chord: Chord) -> Option<String> {
    map.retain(|_, a| a != action);
    map.insert(chord, action.to_owned()).filter(|a| a != action)
}

/// A chord from a key press, if it makes one (a modifier alone doesn't).
pub fn from_key(ev: &KeyEvent) -> Option<Chord> {
    let m = ev.modifiers;
    let key = match &ev.key {
        Key::Named(n) => match n {
            NamedKey::ArrowUp => "Up".to_owned(),
            NamedKey::ArrowDown => "Down".to_owned(),
            NamedKey::ArrowLeft => "Left".to_owned(),
            NamedKey::ArrowRight => "Right".to_owned(),
            NamedKey::Enter => "Return".to_owned(),
            NamedKey::Tab => "Tab".to_owned(),
            NamedKey::Escape => "Escape".to_owned(),
            NamedKey::Backspace => "BackSpace".to_owned(),
            NamedKey::Delete => "Delete".to_owned(),
            NamedKey::Home => "Home".to_owned(),
            NamedKey::End => "End".to_owned(),
            NamedKey::PageUp => "Page_Up".to_owned(),
            NamedKey::PageDown => "Page_Down".to_owned(),
            NamedKey::PrintScreen => "Print".to_owned(),
            NamedKey::F(n) => format!("F{n}"),
            _ => return None,
        },
        Key::Character(s) => {
            // The unshifted key: digits by their place, the rest lowercase.
            let digit = match ev.code {
                2..=10 => Some(char::from(b'1' + (ev.code - 2) as u8)),
                11 => Some('0'),
                _ => None,
            };
            match (digit, s.as_str()) {
                (Some(d), _) => d.to_string(),
                (None, " ") => "Space".to_owned(),
                (None, "`") | (None, "~") => "Grave".to_owned(),
                (None, s) => match s {
                    "-" | "_" => "minus".into(),
                    "=" | "+" => "equal".into(),
                    "[" | "{" => "bracketleft".into(),
                    "]" | "}" => "bracketright".into(),
                    ";" | ":" => "semicolon".into(),
                    "'" | "\"" => "apostrophe".into(),
                    "," | "<" => "comma".into(),
                    "." | ">" => "period".into(),
                    "/" | "?" => "slash".into(),
                    "\\" | "|" => "backslash".into(),
                    s => s.to_lowercase(),
                },
            }
        }
        Key::Unidentified => return None,
    };
    Some(Chord {
        ctrl: m.contains(Modifiers::CONTROL),
        alt: m.contains(Modifiers::ALT),
        shift: m.contains(Modifiers::SHIFT),
        logo: m.contains(Modifiers::SUPER),
        key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_round_trip() {
        let c = Chord::parse("Ctrl+Super+Up").unwrap();
        assert!(c.ctrl && c.logo && !c.shift);
        assert_eq!(c.to_config(), "Ctrl+Super+Up");
        assert_eq!(Chord::parse("super+h").unwrap().to_config(), "Super+H");
        assert_eq!(Chord::parse("Super+Shift+3").unwrap().glyphs(), ["⇧", "❖", "3"]);
        assert_eq!(Chord::parse("Ctrl+Alt+Shift+Super+Left").unwrap().glyphs(), ["⌃", "⌥", "⇧", "❖", "←"]);
        assert_eq!(Chord::parse("Hyper+Q"), None);
        assert!(Chord::parse("XF86AudioMute").unwrap().stands_alone());
    }

    #[test]
    fn overrides_are_the_difference_from_the_defaults() {
        let mut map = effective(&BTreeMap::new());
        assert_eq!(map.get(&Chord::parse("Super+Space").unwrap()).map(String::as_str), Some("palette"));
        // ❖P for the palette: ❖Space goes, ❖P comes.
        let replaced = rebind(&mut map, "palette", Chord::parse("Super+P").unwrap());
        assert_eq!(replaced, None);
        let o = overrides_for(&map);
        assert_eq!(o.get("Super+Space").map(String::as_str), Some("none"));
        assert_eq!(o.get("Super+P").map(String::as_str), Some("palette"));
        assert_eq!(o.len(), 2);
        // Read back, the same table.
        assert_eq!(effective(&o), map);
        // Taking another action's chord says which.
        let replaced = rebind(&mut map, "lock", Chord::parse("Super+M").unwrap());
        assert_eq!(replaced.as_deref(), Some("minimise"));
        // Back to the defaults: no overrides.
        let defaults = effective(&BTreeMap::new());
        assert!(overrides_for(&defaults).is_empty());
    }

    #[test]
    fn spellings_of_overrides_are_understood() {
        let mut o = BTreeMap::new();
        o.insert("super+return".to_owned(), "spawn kitty".to_owned());
        o.insert("Super+M".to_owned(), "none".to_owned());
        let map = effective(&o);
        assert_eq!(map.get(&Chord::parse("Super+Return").unwrap()).map(String::as_str), Some("spawn kitty"));
        assert!(chords_for(&map, "minimise").iter().all(|c| c.to_config() != "Super+M"));
    }

    #[test]
    fn every_listed_action_has_a_default() {
        let map = effective(&BTreeMap::new());
        for s in sections(9) {
            for (a, _) in &s.actions {
                assert!(!chords_for(&map, a).is_empty(), "{a} has no default chord");
            }
        }
    }
}
