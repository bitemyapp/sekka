//! Linux terminal input protocols, ported against pinned Ghostty's MIT-licensed
//! key_encode, function_keys, kitty/key and mouse_encode modules. The frontend
//! supplies normalized events; encode on the PTY owner using current modes.
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers(pub u16);
impl Modifiers {
    pub const SHIFT: Self = Self(1);
    pub const ALT: Self = Self(2);
    pub const CONTROL: Self = Self(4);
    pub const SUPER: Self = Self(8);
    pub const HYPER: Self = Self(16);
    pub const META: Self = Self(32);
    pub const CAPS_LOCK: Self = Self(64);
    pub const NUM_LOCK: Self = Self(128);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
    fn binding(self) -> Self {
        Self(self.0 & 15)
    }
}
impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyAction {
    #[default]
    Press,
    Repeat,
    Release,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeypadKey {
    Digit(u8),
    Decimal,
    Divide,
    Multiply,
    Subtract,
    Add,
    Enter,
    Equal,
    Separator,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    Begin,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierKey {
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Key {
    #[default]
    Unidentified,
    /// Physical/base-layout scalar, not the shifted text. Layout-dependent
    /// unshifted text is supplied separately in `KeyEvent::unshifted`.
    Character(char),
    Escape,
    Enter,
    Tab,
    Backspace,
    Insert,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    PageUp,
    PageDown,
    Home,
    End,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Help,
    ContextMenu,
    F(u8),
    Keypad(KeypadKey),
    Modifier(ModifierKey),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub action: KeyAction,
    pub modifiers: Modifiers,
    pub consumed_modifiers: Modifiers,
    pub text: String,
    pub unshifted: Option<char>,
    pub composing: bool,
}
impl KeyEvent {
    pub fn new(key: Key) -> Self {
        Self {
            key,
            ..Self::default()
        }
    }
    fn effective_modifiers(&self) -> Modifiers {
        if self.text.is_empty() {
            self.modifiers
        } else {
            self.modifiers.without(self.consumed_modifiers)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KittyFlags(pub u8);
impl KittyFlags {
    pub const DISAMBIGUATE: Self = Self(1);
    pub const REPORT_EVENTS: Self = Self(2);
    pub const REPORT_ALTERNATES: Self = Self(4);
    pub const REPORT_ALL: Self = Self(8);
    pub const REPORT_ASSOCIATED: Self = Self(16);
    pub fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KittyKeyboard {
    flags: [KittyFlags; 8],
    index: usize,
}
impl KittyKeyboard {
    pub fn current(self) -> KittyFlags {
        self.flags[self.index]
    }
    pub fn push(&mut self, flags: KittyFlags) {
        self.index = (self.index + 1) % 8;
        self.flags[self.index] = KittyFlags(flags.0 & 31);
    }
    pub fn pop(&mut self, count: usize) {
        if count >= 8 {
            *self = Self::default();
            return;
        }
        for _ in 0..count {
            self.flags[self.index] = KittyFlags(0);
            self.index = (self.index + 7) % 8;
        }
    }
    pub fn set(&mut self, flags: KittyFlags, mode: u16) {
        let value = flags.0 & 31;
        match mode {
            1 => self.flags[self.index].0 = value,
            2 => self.flags[self.index].0 |= value,
            3 => self.flags[self.index].0 &= !value,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardOptions {
    pub application_cursor: bool,
    pub application_keypad: bool,
    pub backarrow: bool,
    pub ignore_keypad_with_numlock: bool,
    pub alt_esc_prefix: bool,
    pub modify_other_keys: bool,
    pub kitty_flags: KittyFlags,
}

/// An empty vector means this event does not produce terminal input.
pub fn encode_key(event: &KeyEvent, options: KeyboardOptions) -> Vec<u8> {
    if options.kitty_flags.0 & 31 != 0 {
        kitty_key(event, options).into_bytes()
    } else {
        legacy_key(event, options)
    }
}

fn functional_code(key: Key) -> Option<(u32, char, bool)> {
    use Key::*;
    Some(match key {
        Escape => (27, 'u', false),
        Enter => (13, 'u', false),
        Tab => (9, 'u', false),
        Backspace => (127, 'u', false),
        Insert => (2, '~', false),
        Delete => (3, '~', false),
        ArrowLeft => (1, 'D', false),
        ArrowRight => (1, 'C', false),
        ArrowUp => (1, 'A', false),
        ArrowDown => (1, 'B', false),
        PageUp => (5, '~', false),
        PageDown => (6, '~', false),
        Home => (1, 'H', false),
        End => (1, 'F', false),
        CapsLock => (57358, 'u', true),
        ScrollLock => (57359, 'u', false),
        NumLock => (57360, 'u', true),
        PrintScreen => (57361, 'u', false),
        Pause => (57362, 'u', false),
        F(1) => (1, 'P', false),
        F(2) => (1, 'Q', false),
        F(3) => (13, '~', false),
        F(4) => (1, 'S', false),
        F(n @ 5..=12) => (
            [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)],
            '~',
            false,
        ),
        F(n @ 13..=25) => (57376 + u32::from(n - 13), 'u', false),
        Keypad(k) => (
            match k {
                KeypadKey::Digit(n @ 0..=9) => 57399 + u32::from(n),
                KeypadKey::Decimal => 57409,
                KeypadKey::Divide => 57410,
                KeypadKey::Multiply => 57411,
                KeypadKey::Subtract => 57412,
                KeypadKey::Add => 57413,
                KeypadKey::Enter => 57414,
                KeypadKey::Equal => 57415,
                KeypadKey::Separator => 57416,
                KeypadKey::Left => 57417,
                KeypadKey::Right => 57418,
                KeypadKey::Up => 57419,
                KeypadKey::Down => 57420,
                KeypadKey::PageUp => 57421,
                KeypadKey::PageDown => 57422,
                KeypadKey::Home => 57423,
                KeypadKey::End => 57424,
                KeypadKey::Insert => 57425,
                KeypadKey::Delete => 57426,
                KeypadKey::Begin => 57427,
                _ => return None,
            },
            'u',
            false,
        ),
        Modifier(k) => (
            match k {
                ModifierKey::ShiftLeft => 57441,
                ModifierKey::ControlLeft => 57442,
                ModifierKey::AltLeft => 57443,
                ModifierKey::SuperLeft => 57444,
                ModifierKey::ShiftRight => 57447,
                ModifierKey::ControlRight => 57448,
                ModifierKey::AltRight => 57449,
                ModifierKey::SuperRight => 57450,
            },
            'u',
            true,
        ),
        _ => return None,
    })
}

fn control(ch: char) -> bool {
    ch < '\u{20}' || ch == '\u{7f}'
}
fn control_text(text: &str) -> bool {
    text.len() == 1 && control(text.as_bytes()[0] as char)
}
fn base_scalar(key: Key) -> Option<char> {
    Some(match key {
        Key::Character(c) => c,
        Key::Tab => '\t',
        Key::Keypad(KeypadKey::Digit(n @ 0..=9)) => char::from(b'0' + n),
        Key::Keypad(KeypadKey::Decimal) => '.',
        Key::Keypad(KeypadKey::Divide) => '/',
        Key::Keypad(KeypadKey::Multiply) => '*',
        Key::Keypad(KeypadKey::Subtract) => '-',
        Key::Keypad(KeypadKey::Add) => '+',
        Key::Keypad(KeypadKey::Equal) => '=',
        _ => return None,
    })
}

fn kitty_key(event: &KeyEvent, options: KeyboardOptions) -> String {
    let flags = options.kitty_flags;
    let report_all = flags.contains(KittyFlags::REPORT_ALL);
    let report_events = flags.contains(KittyFlags::REPORT_EVENTS);
    if event.action == KeyAction::Release
        && (!report_events
            || (!report_all && matches!(event.key, Key::Enter | Key::Tab | Key::Backspace)))
    {
        return String::new();
    }
    let entry = functional_code(event.key).or_else(|| {
        event
            .unshifted
            .filter(|c| *c != '\0')
            .map(|c| (c as u32, 'u', false))
    });
    if event.composing && !entry.is_some_and(|e| e.2) {
        return String::new();
    }
    if !event.composing {
        if !event.text.is_empty() && !control_text(&event.text) {
            match event.key {
                Key::Backspace => return String::new(),
                Key::Enter => return event.text.clone(),
                _ => {}
            }
        }
        if !report_all && event.effective_modifiers().binding().0 == 0 {
            match event.key {
                Key::Enter => return "\r".into(),
                Key::Tab => return "\t".into(),
                Key::Backspace => return "\x7f".into(),
                _ => {}
            }
            if event.action != KeyAction::Release
                && !event.text.is_empty()
                && !event.text.chars().any(control)
            {
                return event.text.clone();
            }
        }
    }
    let Some((code, final_byte, modifier_key)) = entry else {
        return if event.action == KeyAction::Release {
            String::new()
        } else {
            event.text.clone()
        };
    };
    if modifier_key && !report_all {
        return String::new();
    }
    // Ghostty's Linux backend exposes shift/alt/control/super and lock bits.
    let mods = 1 + (event.modifiers.0 & 0xcf);
    let action = if report_events {
        match event.action {
            KeyAction::Press => 1,
            KeyAction::Repeat => 2,
            KeyAction::Release => 3,
        }
    } else {
        0
    };
    if !matches!(final_byte, 'u' | '~') {
        return if action != 0 {
            format!("\x1b[1;{mods}:{action}{final_byte}")
        } else if mods > 1 {
            format!("\x1b[1;{mods}{final_byte}")
        } else {
            format!("\x1b[{final_byte}")
        };
    }
    let mut out = format!("\x1b[{code}");
    if flags.contains(KittyFlags::REPORT_ALTERNATES) && !char::from_u32(code).is_some_and(control) {
        let mut chars = event.text.chars();
        let first = chars.next();
        let multiple = chars.next().is_some();
        let shifted =
            first.filter(|c| *c as u32 != code && event.modifiers.contains(Modifiers::SHIFT));
        let base = base_scalar(event.key)
            .filter(|c| *c as u32 != code && first.is_none_or(|f| f != *c && !multiple));
        if let Some(c) = shifted {
            write!(out, ":{}", c as u32).unwrap();
        }
        if let Some(c) = base {
            write!(
                out,
                "{}:{}",
                if shifted.is_none() { ":" } else { "" },
                c as u32
            )
            .unwrap();
        }
    }
    let prior = action > 1 || mods > 1;
    if action > 1 {
        write!(out, ";{mods}:{action}").unwrap();
    } else if mods > 1 {
        write!(out, ";{mods}").unwrap();
    }
    if flags.contains(KittyFlags::REPORT_ASSOCIATED)
        && event.action != KeyAction::Release
        && event.modifiers.0 & 14 == 0
    {
        let mut chars = event.text.chars().filter(|c| !control(*c));
        if let Some(first) = chars.next() {
            if !prior {
                out.push(';');
            }
            write!(out, ";{}", first as u32).unwrap();
            for c in chars {
                write!(out, ":{}", c as u32).unwrap();
            }
        }
    }
    out.push(final_byte);
    out
}

fn pc_key(key: Key, mods: Modifiers, options: KeyboardOptions) -> Option<Vec<u8>> {
    let mask = mods.binding().0;
    let modifier = mask + 1;
    let key = match key {
        Key::Keypad(k) => {
            let numeric = match k {
                KeypadKey::Digit(n @ 0..=9) => Some((char::from(b'p' + n), char::from(b'0' + n))),
                KeypadKey::Decimal => Some(('n', '.')),
                KeypadKey::Divide => Some(('o', '/')),
                KeypadKey::Multiply => Some(('j', '*')),
                KeypadKey::Subtract => Some(('m', '-')),
                KeypadKey::Add => Some(('k', '+')),
                KeypadKey::Enter => Some(('M', '\r')),
                _ => None,
            };
            if let Some((application, normal)) = numeric {
                return Some(
                    if options.application_keypad && !options.ignore_keypad_with_numlock {
                        if mask == 0 {
                            format!("\x1bO{application}").into_bytes()
                        } else {
                            format!("\x1bO{modifier}{application}").into_bytes()
                        }
                    } else {
                        vec![normal as u8]
                    },
                );
            }
            match k {
                KeypadKey::Left => Key::ArrowLeft,
                KeypadKey::Right => Key::ArrowRight,
                KeypadKey::Up => Key::ArrowUp,
                KeypadKey::Down => Key::ArrowDown,
                KeypadKey::Home => Key::Home,
                KeypadKey::End => Key::End,
                KeypadKey::Insert => Key::Insert,
                KeypadKey::Delete => Key::Delete,
                KeypadKey::PageUp => Key::PageUp,
                KeypadKey::PageDown => Key::PageDown,
                KeypadKey::Begin => Key::Keypad(KeypadKey::Begin),
                _ => return None,
            }
        }
        _ => key,
    };
    let cursor = match key {
        Key::ArrowUp => Some('A'),
        Key::ArrowDown => Some('B'),
        Key::ArrowRight => Some('C'),
        Key::ArrowLeft => Some('D'),
        Key::Home => Some('H'),
        Key::End => Some('F'),
        Key::Keypad(KeypadKey::Begin) => Some('E'),
        _ => None,
    };
    if let Some(last) = cursor {
        return Some(
            if mask > 0 {
                format!("\x1b[1;{modifier}{last}")
            } else if options.application_cursor {
                format!("\x1bO{last}")
            } else {
                format!("\x1b[{last}")
            }
            .into_bytes(),
        );
    }
    let tilde = match key {
        Key::Insert => Some(2),
        Key::Delete => Some(3),
        Key::PageUp => Some(5),
        Key::PageDown => Some(6),
        Key::Help => Some(28),
        Key::ContextMenu => Some(29),
        Key::F(n @ 5..=25) => Some(
            [
                15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34, 42, 43, 44, 45, 46,
            ][usize::from(n - 5)],
        ),
        _ => None,
    };
    if let Some(code) = tilde {
        return Some(
            if mask > 0 {
                format!("\x1b[{code};{modifier}~")
            } else {
                format!("\x1b[{code}~")
            }
            .into_bytes(),
        );
    }
    if let Key::F(n @ 1..=4) = key {
        let last = char::from(b'P' + n - 1);
        return Some(
            if mask == 0 {
                format!("\x1bO{last}")
            } else if n == 3 {
                format!("\x1b[13;{modifier}~")
            } else {
                format!("\x1b[1;{modifier}{last}")
            }
            .into_bytes(),
        );
    }
    match key {
        Key::Backspace => {
            if options.modify_other_keys && mask != 0 && mask != 4 {
                return Some(format!("\x1b[27;{modifier};127~").into_bytes());
            }
            let byte = if mask == 0 || mask == 4 {
                if (mask == 4) ^ options.backarrow {
                    8
                } else {
                    127
                }
            } else if mods.contains(Modifiers::CONTROL) {
                8
            } else {
                127
            };
            Some(if mods.contains(Modifiers::ALT) {
                vec![27, byte]
            } else {
                vec![byte]
            })
        }
        Key::Tab | Key::Enter | Key::Escape => {
            let code = match key {
                Key::Tab => 9,
                Key::Enter => 13,
                _ => 27,
            };
            if mask == 0 {
                return Some(vec![code]);
            }
            if mask == 1 && key == Key::Tab && !options.modify_other_keys {
                return Some(b"\x1b[Z".to_vec());
            }
            if mask == 2 && !options.modify_other_keys {
                return Some(vec![27, code]);
            }
            Some(format!("\x1b[27;{modifier};{code}~").into_bytes())
        }
        _ => None,
    }
}

fn ctrl_byte(event: &KeyEvent) -> Option<u8> {
    if !event.modifiers.contains(Modifiers::CONTROL) {
        return None;
    }
    let mut mods = event.modifiers.binding().without(Modifiers::ALT);
    let mut ch = if event.text.len() == 1 {
        event.text.as_bytes()[0]
    } else {
        let c = base_scalar(event.key)?;
        if c as u32 > 255 || mods != Modifiers::CONTROL {
            return None;
        }
        c as u8
    };
    if mods.contains(Modifiers::SHIFT) && !ch.is_ascii_uppercase() && ch != b'@' {
        mods = mods.without(Modifiers::SHIFT);
    }
    if ch.is_ascii_uppercase()
        && let Some(c) = event.unshifted.filter(|c| *c != '\0' && *c as u32 <= 255)
    {
        ch = c as u8;
    }
    if mods != Modifiers::CONTROL {
        return None;
    }
    Some(match ch {
        b' ' | b'2' | b'@' => 0,
        b'/' | b'7' | b'_' => 31,
        b'0' | b'1' | b'9' => ch,
        b'3' => 27,
        b'4' | b'\\' => 28,
        b'5' | b']' => 29,
        b'6' | b'^' | b'~' => 30,
        b'8' | b'?' => 127,
        b'a'..=b'h' | b'j'..=b'l' | b'n'..=b'z' => ch - b'a' + 1,
        _ => return None,
    })
}

fn legacy_key(event: &KeyEvent, options: KeyboardOptions) -> Vec<u8> {
    if event.action == KeyAction::Release || event.composing {
        return Vec::new();
    }
    if let Some(bytes) = pc_key(event.key, event.modifiers, options) {
        if event.text.is_empty()
            || control_text(&event.text)
            || !matches!(event.key, Key::Backspace | Key::Enter | Key::Escape)
        {
            return bytes;
        }
        if event.key == Key::Backspace {
            return Vec::new();
        }
    }
    let mut chars = event.text.chars();
    let scalar = chars.next().filter(|_| chars.next().is_none());
    let binding = event.effective_modifiers().binding();
    if options.modify_other_keys
        && let Some(c) = scalar
    {
        let mods = event.modifiers.binding().0;
        if mods != 0 && (('@'..='\x7f').contains(&c) || mods & !1 != 0 || c == ' ') {
            return format!("\x1b[27;{};{}~", mods + 1, c as u32).into_bytes();
        }
    }
    if let Some(byte) = ctrl_byte(event) {
        return if binding.contains(Modifiers::ALT) {
            vec![27, byte]
        } else {
            vec![byte]
        };
    }
    if !event.text.is_empty()
        && event.modifiers.contains(Modifiers::CONTROL)
        && let Some(mut c) = scalar
    {
        let mut mods = event.modifiers.0 & 7;
        if c.is_ascii_uppercase() && mods & 1 != 0 {
            c = c.to_ascii_lowercase();
        }
        if event.unshifted != Some(c) {
            mods &= !1;
        }
        return format!("\x1b[{};{}u", c as u32, mods + 1).into_bytes();
    }
    if binding.contains(Modifiers::ALT) && options.alt_esc_prefix {
        let text = if event.text.is_empty() {
            event
                .unshifted
                .filter(|c| *c != '\0')
                .map(|c| c.to_string())
                .unwrap_or_default()
        } else {
            event.text.clone()
        };
        if !text.is_empty() {
            let mut out = vec![27];
            out.extend_from_slice(text.as_bytes());
            return out;
        }
    }
    event.text.as_bytes().to_vec()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseTracking {
    #[default]
    None,
    X10,
    Normal,
    Button,
    Any,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseFormat {
    #[default]
    X10,
    Utf8,
    Sgr,
    Urxvt,
    SgrPixels,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseAction {
    #[default]
    Press,
    Release,
    Motion,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    Eight,
    Nine,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub modifiers: Modifiers,
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseGeometry {
    pub width: u32,
    pub height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub padding_left: u32,
    pub padding_top: u32,
    pub padding_right: u32,
    pub padding_bottom: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseEncoder {
    last_cell: Option<(u32, u32)>,
    configuration: Option<(MouseTracking, MouseFormat, MouseGeometry)>,
}
impl MouseEncoder {
    pub fn reset(&mut self) {
        self.last_cell = None;
    }
    pub fn encode(
        &mut self,
        event: MouseEvent,
        tracking: MouseTracking,
        format: MouseFormat,
        size: MouseGeometry,
        any_button_pressed: bool,
    ) -> Vec<u8> {
        let configuration = Some((tracking, format, size));
        if self.configuration != configuration {
            self.last_cell = None;
            self.configuration = configuration;
        }
        if !event.x.is_finite()
            || !event.y.is_finite()
            || size.cell_width == 0
            || size.cell_height == 0
        {
            return Vec::new();
        }
        let should_report = match tracking {
            MouseTracking::None => false,
            MouseTracking::X10 => {
                event.action == MouseAction::Press
                    && matches!(
                        event.button,
                        Some(MouseButton::Left | MouseButton::Middle | MouseButton::Right)
                    )
            }
            MouseTracking::Normal => event.action != MouseAction::Motion,
            MouseTracking::Button => event.button.is_some(),
            MouseTracking::Any => true,
        };
        if !should_report {
            return Vec::new();
        }
        let outside = event.x < 0.0
            || event.y < 0.0
            || f64::from(event.x) > f64::from(size.width)
            || f64::from(event.y) > f64::from(size.height);
        if event.action != MouseAction::Release
            && outside
            && (!matches!(tracking, MouseTracking::Button | MouseTracking::Any)
                || !any_button_pressed)
        {
            return Vec::new();
        }
        let columns = size
            .width
            .saturating_sub(size.padding_left)
            .saturating_sub(size.padding_right)
            / size.cell_width;
        let rows = size
            .height
            .saturating_sub(size.padding_top)
            .saturating_sub(size.padding_bottom)
            / size.cell_height;
        if columns == 0 || rows == 0 {
            return Vec::new();
        }
        let x = f64::from(event.x) - f64::from(size.padding_left);
        let y = f64::from(event.y) - f64::from(size.padding_top);
        let cell = (
            (x / f64::from(size.cell_width))
                .floor()
                .max(0.0)
                .min(f64::from(columns - 1)) as u32,
            (y / f64::from(size.cell_height))
                .floor()
                .max(0.0)
                .min(f64::from(rows - 1)) as u32,
        );
        if event.action == MouseAction::Motion
            && format != MouseFormat::SgrPixels
            && self.last_cell == Some(cell)
        {
            return Vec::new();
        }
        self.last_cell = Some(cell);
        let mut code = if event.action == MouseAction::Release
            && !matches!(format, MouseFormat::Sgr | MouseFormat::SgrPixels)
        {
            3
        } else {
            match event.button {
                None => 3,
                Some(MouseButton::Left) => 0,
                Some(MouseButton::Middle) => 1,
                Some(MouseButton::Right) => 2,
                Some(MouseButton::WheelUp) => 64,
                Some(MouseButton::WheelDown) => 65,
                Some(MouseButton::WheelLeft) => 66,
                Some(MouseButton::WheelRight) => 67,
                Some(MouseButton::Eight) => 128,
                Some(MouseButton::Nine) => 129,
            }
        };
        if tracking != MouseTracking::X10 {
            if event.modifiers.contains(Modifiers::SHIFT) {
                code += 4;
            }
            if event.modifiers.contains(Modifiers::ALT) {
                code += 8;
            }
            if event.modifiers.contains(Modifiers::CONTROL) {
                code += 16;
            }
        }
        if event.action == MouseAction::Motion {
            code += 32;
        }
        let final_byte = if event.action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        match format {
            MouseFormat::X10 => {
                if cell.0 > 222 || cell.1 > 222 {
                    Vec::new()
                } else {
                    vec![
                        27,
                        b'[',
                        b'M',
                        32 + code,
                        33 + cell.0 as u8,
                        33 + cell.1 as u8,
                    ]
                }
            }
            MouseFormat::Utf8 => {
                let mut out = "\x1b[M".to_owned();
                for cp in [
                    32 + u32::from(code),
                    cell.0.saturating_add(33),
                    cell.1.saturating_add(33),
                ] {
                    let Some(c) = char::from_u32(cp) else {
                        return Vec::new();
                    };
                    out.push(c);
                }
                out.into_bytes()
            }
            MouseFormat::Sgr => {
                format!("\x1b[<{code};{};{}{final_byte}", cell.0 + 1, cell.1 + 1).into_bytes()
            }
            MouseFormat::Urxvt => format!(
                "\x1b[{};{};{}M",
                32 + u16::from(code),
                cell.0 + 1,
                cell.1 + 1
            )
            .into_bytes(),
            MouseFormat::SgrPixels => format!(
                "\x1b[<{code};{};{}{final_byte}",
                x.round() as i32,
                y.round() as i32
            )
            .into_bytes(),
        }
    }
}
