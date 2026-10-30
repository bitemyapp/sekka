//! Key bindings: the system's few chords (architecture.md, "Keys"), named
//! actions, and overrides from the settings' `keys.bindings`.
//!
//! ❖ is the command key: everything not listed here goes to the focused
//! app (❖C, ❖V, ❖Q… belong to apps). Terminal Close/Quit are contextual:
//! their fallback menus promise window actions, not terminal control bytes.

use std::collections::HashMap;

use smithay::input::keyboard::{Keysym, ModifiersState, keysyms};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl Mods {
    pub fn from_state(m: &ModifiersState) -> Mods {
        Mods { ctrl: m.ctrl, alt: m.alt, shift: m.shift, logo: m.logo }
    }
}

/// What a binding does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    GoToDesk(u32),
    TakeToDesk(u32),
    PrevDesk,
    NextDesk,
    TakeToPrevDesk,
    TakeToNextDesk,
    Overview,
    Switcher {
        reverse: bool,
    },
    NextWindowOfApp,
    HideApp,
    HideOthers,
    Minimise,
    Fill,
    SplitLeft,
    SplitRight,
    StackWithBehind,
    Unstack,
    PrevTab,
    NextTab,
    MoveTabLeft,
    MoveTabRight,
    /// The tab's marking menu, opened from the keyboard.
    WindowMenu,
    Fullscreen,
    Terminal,
    TerminalClose,
    TerminalQuit,
    ScreenshotScreen,
    ScreenshotArea,
    ScreenshotWindow,
    ChangeVt(i32),
    /// Reload what changed (the core does it after this key: ⌃❖R is the
    /// core's own, so it works with the logic set aside; this is for
    /// another chord in `[keys.bindings]`).
    Reload,
    Quit,
    Spawn(Vec<String>),
    /// Sent to the shell as an `Event::Binding`.
    Shell(String),
    /// Unbound (an override that removes a default).
    None,
}

impl Action {
    /// Parses an action name (as in settings overrides and IPC `action`).
    pub fn parse(s: &str) -> Option<Action> {
        let s = s.trim();
        if let Some(cmd) = s.strip_prefix("spawn ") {
            return Some(Action::Spawn(cmd.split_whitespace().map(String::from).collect()));
        }
        if let Some(n) = s.strip_prefix("go-to-desk-").and_then(|n| n.parse().ok()) {
            return Some(Action::GoToDesk(n));
        }
        if let Some(n) = s.strip_prefix("take-to-desk-").and_then(|n| n.parse().ok()) {
            return Some(Action::TakeToDesk(n));
        }
        if let Some(n) = s.strip_prefix("vt-").and_then(|n| n.parse().ok()) {
            return Some(Action::ChangeVt(n));
        }
        Some(match s {
            "previous-desk" => Action::PrevDesk,
            "next-desk" => Action::NextDesk,
            "take-to-previous-desk" => Action::TakeToPrevDesk,
            "take-to-next-desk" => Action::TakeToNextDesk,
            "overview" => Action::Overview,
            "switcher" => Action::Switcher { reverse: false },
            "switcher-reverse" => Action::Switcher { reverse: true },
            "next-window-of-app" => Action::NextWindowOfApp,
            "hide-app" => Action::HideApp,
            "hide-others" => Action::HideOthers,
            "minimise" | "minimize" | "park" => Action::Minimise,
            "fill" => Action::Fill,
            "split-left" => Action::SplitLeft,
            "split-right" => Action::SplitRight,
            "stack-with-behind" | "stack" => Action::StackWithBehind,
            "unstack" => Action::Unstack,
            "previous-tab" => Action::PrevTab,
            "next-tab" => Action::NextTab,
            "move-tab-left" => Action::MoveTabLeft,
            "move-tab-right" => Action::MoveTabRight,
            "window-menu" => Action::WindowMenu,
            "fullscreen" => Action::Fullscreen,
            "terminal" => Action::Terminal,
            "terminal-close" => Action::TerminalClose,
            "terminal-quit" => Action::TerminalQuit,
            "screenshot" | "screenshot-screen" => Action::ScreenshotScreen,
            "screenshot-area" => Action::ScreenshotArea,
            "screenshot-window" => Action::ScreenshotWindow,
            "quit" => Action::Quit,
            "reload" => Action::Reload,
            "none" => Action::None,
            other if SHELL_ACTIONS.contains(&other) => Action::Shell(other.to_string()),
            _ => return None,
        })
    }
}

/// Actions that belong to the shell: the compositor only reports them.
pub const SHELL_ACTIONS: &[&str] = &[
    "palette",
    "toolchest",
    "menu-bar",
    "power-key",
    "bell",
    "lock",
    "screenshot-tool",
    "record",
    "record-screen",
    "record-area",
    "stop-recording",
    "volume-up",
    "volume-down",
    "mute",
    "mic-mute",
    "brightness-up",
    "brightness-down",
    "play-pause",
    "next",
    "previous",
    "stop",
    "app-menu",
    "calculator",
    "mail",
    "web",
];

/// A chord: modifiers and a keysym.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub mods: Mods,
    pub sym: Keysym,
}

/// Parses `"Ctrl+Super+Up"`, `"Super+Shift+3"`, `"XF86AudioMute"`.
/// Modifier names: Ctrl/Control, Alt/Option, Shift, Super/Logo/Cmd/Mod4.
pub fn parse_chord(s: &str) -> Option<Chord> {
    let mut mods = Mods::default();
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last()?;
    for m in modifiers {
        match m.to_lowercase().as_str() {
            "ctrl" | "control" | "⌃" => mods.ctrl = true,
            "alt" | "option" | "opt" | "⌥" => mods.alt = true,
            "shift" | "⇧" => mods.shift = true,
            "super" | "logo" | "cmd" | "mod4" | "win" | "❖" => mods.logo = true,
            _ => return None,
        }
    }
    let sym = keysym_named(key)?;
    Some(Chord { mods, sym })
}

fn keysym_named(name: &str) -> Option<Keysym> {
    let k = match name {
        "Up" | "↑" => keysyms::KEY_Up,
        "Down" | "↓" => keysyms::KEY_Down,
        "Left" | "←" => keysyms::KEY_Left,
        "Right" | "→" => keysyms::KEY_Right,
        "Return" | "Enter" | "↩" => keysyms::KEY_Return,
        "Space" => keysyms::KEY_space,
        "Tab" | "⇥" => keysyms::KEY_Tab,
        "Grave" | "`" => keysyms::KEY_grave,
        "[" => keysyms::KEY_bracketleft,
        "]" => keysyms::KEY_bracketright,
        "Escape" | "Esc" => keysyms::KEY_Escape,
        "Print" => keysyms::KEY_Print,
        _ => {
            let k = smithay::input::keyboard::xkb::keysym_from_name(
                name,
                smithay::input::keyboard::xkb::KEYSYM_CASE_INSENSITIVE,
            );
            if k.raw() == keysyms::KEY_NoSymbol {
                return None;
            }
            return Some(lower(k));
        }
    };
    Some(Keysym::new(k))
}

/// Letters compare lowercased (Shift is a modifier of its own).
pub fn lower(k: Keysym) -> Keysym {
    let raw = k.raw();
    if (keysyms::KEY_A..=keysyms::KEY_Z).contains(&raw) {
        Keysym::new(raw + (keysyms::KEY_a - keysyms::KEY_A))
    } else {
        k
    }
}

/// The default chords, as (chord, action name).
pub fn defaults() -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = vec![
        ("Super+Space", "palette".into()),
        ("Super+Alt+Space", "toolchest".into()),
        // F10 only over Aizome's apps and the desktop (decide_key): other
        // apps keep it.
        ("F10", "menu-bar".into()),
        ("Super+Alt+M", "menu-bar".into()),
        ("XF86PowerOff", "power-key".into()),
        ("Super+Tab", "switcher".into()),
        ("Super+Shift+Tab", "switcher-reverse".into()),
        ("Super+Grave", "next-window-of-app".into()),
        ("Super+H", "hide-app".into()),
        ("Super+Alt+H", "hide-others".into()),
        ("Super+M", "minimise".into()),
        ("Ctrl+Super+Left", "previous-desk".into()),
        ("Ctrl+Super+Right", "next-desk".into()),
        ("Ctrl+Shift+Super+Left", "take-to-previous-desk".into()),
        ("Ctrl+Shift+Super+Right", "take-to-next-desk".into()),
        ("Ctrl+Super+Up", "overview".into()),
        ("Alt+Super+Up", "fill".into()),
        ("Alt+Super+Down", "minimise".into()),
        ("Alt+Super+Left", "split-left".into()),
        ("Alt+Super+Right", "split-right".into()),
        // ⌃❖ is the window manager's, and ⌃⇧❖ carries: a window to a
        // Desk, a tab along its group or out of it.
        ("Ctrl+Super+S", "stack-with-behind".into()),
        ("Ctrl+Shift+Super+S", "unstack".into()),
        ("Ctrl+Super+Space", "window-menu".into()),
        // ⌘⇧[ ⌘⇧] as on a Mac; not ❖[ ❖] (Aizome's apps' Back and
        // Forward). Shift matches as a modifier: these are ❖{ ❖} typed.
        ("Super+Shift+bracketleft", "previous-tab".into()),
        ("Super+Shift+bracketright", "next-tab".into()),
        ("Ctrl+Shift+Super+bracketleft", "move-tab-left".into()),
        ("Ctrl+Shift+Super+bracketright", "move-tab-right".into()),
        ("Ctrl+Super+F", "fullscreen".into()),
        ("Super+Return", "terminal".into()),
        ("Super+W", "terminal-close".into()),
        ("Super+Q", "terminal-quit".into()),
        ("Ctrl+Super+Q", "lock".into()),
        ("Super+Shift+3", "screenshot-screen".into()),
        ("Super+Shift+4", "screenshot-area".into()),
        ("Super+Shift+5", "screenshot-tool".into()),
        ("Print", "screenshot-area".into()),
        // Tatami's: start a recording (pick an area, or click for a display), or
        // stop the one going.
        ("Alt+Print", "record".into()),
        ("XF86AudioRaiseVolume", "volume-up".into()),
        ("XF86AudioLowerVolume", "volume-down".into()),
        ("XF86AudioMute", "mute".into()),
        ("XF86AudioMicMute", "mic-mute".into()),
        ("XF86MonBrightnessUp", "brightness-up".into()),
        ("XF86MonBrightnessDown", "brightness-down".into()),
        ("XF86AudioPlay", "play-pause".into()),
        ("XF86AudioPause", "play-pause".into()),
        ("XF86AudioNext", "next".into()),
        ("XF86AudioPrev", "previous".into()),
        ("XF86AudioStop", "stop".into()),
    ];
    const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const GO: [&str; 9] = [
        "Ctrl+Super+1",
        "Ctrl+Super+2",
        "Ctrl+Super+3",
        "Ctrl+Super+4",
        "Ctrl+Super+5",
        "Ctrl+Super+6",
        "Ctrl+Super+7",
        "Ctrl+Super+8",
        "Ctrl+Super+9",
    ];
    const TAKE: [&str; 9] = [
        "Ctrl+Shift+Super+1",
        "Ctrl+Shift+Super+2",
        "Ctrl+Shift+Super+3",
        "Ctrl+Shift+Super+4",
        "Ctrl+Shift+Super+5",
        "Ctrl+Shift+Super+6",
        "Ctrl+Shift+Super+7",
        "Ctrl+Shift+Super+8",
        "Ctrl+Shift+Super+9",
    ];
    for i in 0..9 {
        v.push((GO[i], format!("go-to-desk-{}", DIGITS[i])));
        v.push((TAKE[i], format!("take-to-desk-{}", DIGITS[i])));
    }
    for n in 1..=12 {
        v.push((VT_CHORDS[n - 1], format!("vt-{n}")));
    }
    v
}

const VT_CHORDS: [&str; 12] = [
    "XF86Switch_VT_1",
    "XF86Switch_VT_2",
    "XF86Switch_VT_3",
    "XF86Switch_VT_4",
    "XF86Switch_VT_5",
    "XF86Switch_VT_6",
    "XF86Switch_VT_7",
    "XF86Switch_VT_8",
    "XF86Switch_VT_9",
    "XF86Switch_VT_10",
    "XF86Switch_VT_11",
    "XF86Switch_VT_12",
];

/// The binding table: defaults with the settings' overrides on top.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    map: HashMap<Chord, Action>,
}

impl Bindings {
    pub fn new(overrides: &std::collections::BTreeMap<String, String>) -> Bindings {
        let mut map = HashMap::new();
        for (chord, action) in defaults() {
            if let (Some(c), Some(a)) = (parse_chord(chord), Action::parse(&action)) {
                map.insert(c, a);
            }
        }
        for (chord, action) in overrides {
            match (parse_chord(chord), Action::parse(action)) {
                (Some(c), Some(Action::None)) => {
                    map.remove(&c);
                }
                (Some(c), Some(a)) => {
                    map.insert(c, a);
                }
                _ => tracing::warn!("ignoring the binding {chord} = {action}"),
            }
        }
        Bindings { map }
    }

    /// The action for modifiers and the key's keysyms (the unshifted latin
    /// one first).
    pub fn find(&self, mods: Mods, syms: &[Keysym]) -> Option<&Action> {
        syms.iter().find_map(|s| self.map.get(&Chord { mods, sym: lower(*s) }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chords() {
        let c = parse_chord("Ctrl+Super+Up").unwrap();
        assert!(c.mods.ctrl && c.mods.logo && !c.mods.shift);
        assert_eq!(c.sym.raw(), keysyms::KEY_Up);
        assert_eq!(parse_chord("Super+H").unwrap().sym.raw(), keysyms::KEY_h);
        assert_eq!(parse_chord("Super+Shift+3").unwrap().sym.raw(), keysyms::KEY_3);
        assert!(parse_chord("Hyper+Q").is_none());
        assert_eq!(parse_chord("XF86AudioMute").unwrap().sym.raw(), keysyms::KEY_XF86AudioMute);
        assert_eq!(parse_chord("Super+Shift+["), parse_chord("Super+Shift+bracketleft"));
        assert_eq!(parse_chord("Super+Shift+]").unwrap().sym.raw(), keysyms::KEY_bracketright);
    }

    #[test]
    fn defaults_and_overrides() {
        let mut o = std::collections::BTreeMap::new();
        o.insert("Super+Return".to_string(), "spawn kitty".to_string());
        o.insert("Super+M".to_string(), "none".to_string());
        let b = Bindings::new(&o);
        let logo = Mods { logo: true, ..Default::default() };
        assert_eq!(b.find(logo, &[Keysym::new(keysyms::KEY_Return)]), Some(&Action::Spawn(vec!["kitty".into()])));
        assert_eq!(b.find(logo, &[Keysym::new(keysyms::KEY_m)]), None);
        assert_eq!(b.find(logo, &[Keysym::new(keysyms::KEY_c)]), None, "❖C is the app's");
        let ctrl_logo = Mods { ctrl: true, logo: true, ..Default::default() };
        assert_eq!(b.find(ctrl_logo, &[Keysym::new(keysyms::KEY_3)]), Some(&Action::GoToDesk(3)));
        assert_eq!(b.find(logo, &[Keysym::new(keysyms::KEY_space)]), Some(&Action::Shell("palette".into())));
        assert!(Action::parse("go-to-desk-7") == Some(Action::GoToDesk(7)));
        // ❖⇧[ arrives as braceleft with bracketleft, the key's own symbol,
        // first (as the core passes them).
        let shift_logo = Mods { shift: true, logo: true, ..Default::default() };
        let (bl, br) = (Keysym::new(keysyms::KEY_bracketleft), Keysym::new(keysyms::KEY_bracketright));
        let (cl, cr) = (Keysym::new(keysyms::KEY_braceleft), Keysym::new(keysyms::KEY_braceright));
        assert_eq!(b.find(shift_logo, &[bl, cl]), Some(&Action::PrevTab));
        assert_eq!(b.find(shift_logo, &[br, cr]), Some(&Action::NextTab));
        assert_eq!(b.find(logo, &[bl]), None, "❖[ is the app's (Back)");
        let carry = Mods { ctrl: true, shift: true, logo: true, ..Default::default() };
        assert_eq!(b.find(carry, &[bl, cl]), Some(&Action::MoveTabLeft));
        assert_eq!(b.find(carry, &[br, cr]), Some(&Action::MoveTabRight));
        let s = [Keysym::new(keysyms::KEY_s)];
        assert_eq!(b.find(ctrl_logo, &s), Some(&Action::StackWithBehind));
        assert_eq!(b.find(carry, &[s[0], Keysym::new(keysyms::KEY_S)]), Some(&Action::Unstack));
        let alt_logo = Mods { alt: true, logo: true, ..Default::default() };
        assert_eq!(b.find(alt_logo, &s), None, "⌥❖S is the app's (Koyomi's Sidebar)");
        assert_eq!(b.find(ctrl_logo, &[Keysym::new(keysyms::KEY_space)]), Some(&Action::WindowMenu));
        // ⌥Print records (Alt turns Print into Sys_Req, so the key's own
        // symbol is what matches).
        let alt = Mods { alt: true, ..Default::default() };
        let print = [Keysym::new(keysyms::KEY_Print), Keysym::new(keysyms::KEY_Sys_Req)];
        assert_eq!(b.find(alt, &print), Some(&Action::Shell("record".into())));
        assert_eq!(b.find(Mods::default(), &print[..1]), Some(&Action::ScreenshotArea));
        assert_eq!(Action::parse("stop-recording"), Some(Action::Shell("stop-recording".into())));
    }

    #[test]
    fn terminal_close_quit_are_plain_command_defaults_and_can_be_overridden() {
        let logo = Mods { logo: true, ..Default::default() };
        let w = [Keysym::new(keysyms::KEY_w)];
        let q = [Keysym::new(keysyms::KEY_q)];
        let defaults = Bindings::new(&Default::default());
        assert_eq!(defaults.find(logo, &w), Some(&Action::TerminalClose));
        assert_eq!(defaults.find(logo, &q), Some(&Action::TerminalQuit));
        assert_eq!(defaults.find(Mods { shift: true, ..logo }, &w), None);
        assert_eq!(defaults.find(Mods { ctrl: true, shift: true, ..Default::default() }, &q), None);
        for key in [keysyms::KEY_c, keysyms::KEY_v, keysyms::KEY_f, keysyms::KEY_n] {
            assert_eq!(defaults.find(logo, &[Keysym::new(key)]), None, "terminal translation stays intact");
        }
        let overrides = [("Super+W".into(), "none".into()), ("Super+Q".into(), "spawn custom".into())].into();
        let configured = Bindings::new(&overrides);
        assert_eq!(configured.find(logo, &w), None);
        assert_eq!(configured.find(logo, &q), Some(&Action::Spawn(vec!["custom".into()])));
    }
}
