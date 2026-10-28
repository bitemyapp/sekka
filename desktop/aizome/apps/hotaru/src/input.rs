//! Normalize native input; protocol encoding happens on the PTY owner.

use aizome_kit::sidestep_ui::{Key, KeyEvent, Modifiers, NamedKey, PointerButton};
use hotaru_core::input as vt;

pub fn modifiers(mods: Modifiers) -> vt::Modifiers {
    let mut value = vt::Modifiers::default();
    for (native, terminal) in [
        (Modifiers::SHIFT, vt::Modifiers::SHIFT),
        (Modifiers::ALT, vt::Modifiers::ALT),
        (Modifiers::CONTROL, vt::Modifiers::CONTROL),
        (Modifiers::SUPER, vt::Modifiers::SUPER),
        (Modifiers::CAPS_LOCK, vt::Modifiers::CAPS_LOCK),
        (Modifiers::NUM_LOCK, vt::Modifiers::NUM_LOCK),
    ] {
        if mods.contains(native) {
            value = value | terminal;
        }
    }
    value
}

pub fn key_event(event: &KeyEvent) -> vt::KeyEvent {
    let physical = physical_key(event.code);
    let named = match &event.key {
        Key::Named(name) => match name {
            NamedKey::Enter => vt::Key::Enter,
            NamedKey::Tab => vt::Key::Tab,
            NamedKey::Backspace => vt::Key::Backspace,
            NamedKey::Escape => vt::Key::Escape,
            NamedKey::Delete => vt::Key::Delete,
            NamedKey::Insert => vt::Key::Insert,
            NamedKey::ArrowUp => vt::Key::ArrowUp,
            NamedKey::ArrowDown => vt::Key::ArrowDown,
            NamedKey::ArrowLeft => vt::Key::ArrowLeft,
            NamedKey::ArrowRight => vt::Key::ArrowRight,
            NamedKey::Home => vt::Key::Home,
            NamedKey::End => vt::Key::End,
            NamedKey::PageUp => vt::Key::PageUp,
            NamedKey::PageDown => vt::Key::PageDown,
            NamedKey::F(n) => vt::Key::F(*n),
            NamedKey::PrintScreen => vt::Key::PrintScreen,
            NamedKey::ScrollLock => vt::Key::ScrollLock,
            NamedKey::Pause | NamedKey::Break => vt::Key::Pause,
            NamedKey::Menu => vt::Key::ContextMenu,
            NamedKey::Help => vt::Key::Help,
            _ => vt::Key::Unidentified,
        },
        _ => vt::Key::Unidentified,
    };
    use vt::{Key as K, KeypadKey as P, ModifierKey as M};
    let remapped_modifier = match event.native.keysym {
        0xffe1 => Some(K::Modifier(M::ShiftLeft)),
        0xffe2 => Some(K::Modifier(M::ShiftRight)),
        0xffe3 => Some(K::Modifier(M::ControlLeft)),
        0xffe4 => Some(K::Modifier(M::ControlRight)),
        0xffe9 => Some(K::Modifier(M::AltLeft)),
        0xffea => Some(K::Modifier(M::AltRight)),
        0xffeb => Some(K::Modifier(M::SuperLeft)),
        0xffec => Some(K::Modifier(M::SuperRight)),
        0xffe5 => Some(K::CapsLock),
        0xff7f => Some(K::NumLock),
        _ => None,
    };
    let key = if let Some(modifier) = remapped_modifier {
        modifier
    } else if matches!(physical, K::Keypad(_)) {
        match named {
            K::Home => K::Keypad(P::Home),
            K::End => K::Keypad(P::End),
            K::ArrowLeft => K::Keypad(P::Left),
            K::ArrowRight => K::Keypad(P::Right),
            K::ArrowUp => K::Keypad(P::Up),
            K::ArrowDown => K::Keypad(P::Down),
            K::PageUp => K::Keypad(P::PageUp),
            K::PageDown => K::Keypad(P::PageDown),
            K::Insert => K::Keypad(P::Insert),
            K::Delete => K::Keypad(P::Delete),
            _ if event.key == Key::Named(NamedKey::Begin) => K::Keypad(P::Begin),
            _ => physical,
        }
    } else if named != K::Unidentified {
        named
    } else if matches!(physical, K::Character(_)) {
        physical
    } else if let Some(character) = event.native.unshifted {
        K::Character(character)
    } else {
        physical
    };
    vt::KeyEvent {
        key,
        action: if !event.pressed {
            vt::KeyAction::Release
        } else if event.repeat {
            vt::KeyAction::Repeat
        } else {
            vt::KeyAction::Press
        },
        modifiers: modifiers(event.modifiers),
        consumed_modifiers: modifiers(event.native.consumed_modifiers),
        text: if event.native.text.is_empty() {
            event.text.clone().unwrap_or_default()
        } else {
            event.native.text.clone()
        },
        unshifted: event.native.unshifted,
        composing: event.composing.as_ref().is_some_and(|text| !text.is_empty()),
    }
}

/// Evdev physical identity is layout-independent; never derive the unshifted
/// layout character from this US key position. Native metadata supplies that.
fn physical_key(code: u32) -> vt::Key {
    use vt::{Key as K, KeypadKey as P, ModifierKey as M};
    let character = match code {
        2..=11 => Some(b"1234567890"[(code - 2) as usize] as char),
        16..=25 => Some(b"qwertyuiop"[(code - 16) as usize] as char),
        30..=38 => Some(b"asdfghjkl"[(code - 30) as usize] as char),
        44..=50 => Some(b"zxcvbnm"[(code - 44) as usize] as char),
        12 => Some('-'),
        13 => Some('='),
        26 => Some('['),
        27 => Some(']'),
        39 => Some(';'),
        40 => Some('\''),
        41 => Some('`'),
        43 | 86 => Some('\\'),
        51 => Some(','),
        52 => Some('.'),
        53 => Some('/'),
        57 => Some(' '),
        _ => None,
    };
    if let Some(character) = character {
        return K::Character(character);
    }
    match code {
        1 => K::Escape,
        14 => K::Backspace,
        15 => K::Tab,
        28 => K::Enter,
        29 => K::Modifier(M::ControlLeft),
        42 => K::Modifier(M::ShiftLeft),
        54 => K::Modifier(M::ShiftRight),
        56 => K::Modifier(M::AltLeft),
        97 => K::Modifier(M::ControlRight),
        100 => K::Modifier(M::AltRight),
        125 => K::Modifier(M::SuperLeft),
        126 => K::Modifier(M::SuperRight),
        58 => K::CapsLock,
        69 => K::NumLock,
        70 => K::ScrollLock,
        59..=68 => K::F((code - 58) as u8),
        87 => K::F(11),
        88 => K::F(12),
        183..=194 => K::F((code - 170) as u8),
        55 => K::Keypad(P::Multiply),
        71 => K::Keypad(P::Digit(7)),
        72 => K::Keypad(P::Digit(8)),
        73 => K::Keypad(P::Digit(9)),
        74 => K::Keypad(P::Subtract),
        75 => K::Keypad(P::Digit(4)),
        76 => K::Keypad(P::Digit(5)),
        77 => K::Keypad(P::Digit(6)),
        78 => K::Keypad(P::Add),
        79 => K::Keypad(P::Digit(1)),
        80 => K::Keypad(P::Digit(2)),
        81 => K::Keypad(P::Digit(3)),
        82 => K::Keypad(P::Digit(0)),
        83 => K::Keypad(P::Decimal),
        96 => K::Keypad(P::Enter),
        98 => K::Keypad(P::Divide),
        117 => K::Keypad(P::Equal),
        121 => K::Keypad(P::Separator),
        102 => K::Home,
        103 => K::ArrowUp,
        104 => K::PageUp,
        105 => K::ArrowLeft,
        106 => K::ArrowRight,
        107 => K::End,
        108 => K::ArrowDown,
        109 => K::PageDown,
        110 => K::Insert,
        111 => K::Delete,
        99 => K::PrintScreen,
        119 => K::Pause,
        139 => K::ContextMenu,
        _ => K::Unidentified,
    }
}

pub fn mouse_button(button: PointerButton) -> Option<vt::MouseButton> {
    Some(match button {
        PointerButton::Left => vt::MouseButton::Left,
        PointerButton::Middle => vt::MouseButton::Middle,
        PointerButton::Right => vt::MouseButton::Right,
        PointerButton::Back => vt::MouseButton::Eight,
        PointerButton::Forward => vt::MouseButton::Nine,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct MouseInput {
    pub event: vt::MouseEvent,
    pub width: f64,
    pub height: f64,
    pub cell_width: f64,
    pub cell_height: f64,
    pub padding: f64,
    pub any_button: bool,
}

impl MouseInput {
    /// Cell protocols use normalized coordinates so fractional native font
    /// metrics do not round a click into the neighboring column. Pixel reports
    /// retain the native coordinate system and terminal padding.
    pub fn encode(
        self,
        encoder: &mut vt::MouseEncoder,
        tracking: vt::MouseTracking,
        format: vt::MouseFormat,
    ) -> Vec<u8> {
        if ![self.width, self.height, self.cell_width, self.cell_height, self.padding]
            .iter()
            .all(|value| value.is_finite())
            || self.cell_width <= 0.0
            || self.cell_height <= 0.0
        {
            return Vec::new();
        }
        let mut event = self.event;
        let geometry = if format == vt::MouseFormat::SgrPixels {
            vt::MouseGeometry {
                width: self.width as u32,
                height: self.height as u32,
                cell_width: 1,
                cell_height: 1,
                padding_left: self.padding as u32,
                padding_top: self.padding as u32,
                padding_right: self.padding as u32,
                padding_bottom: self.padding as u32,
            }
        } else {
            let cols = ((self.width - 2.0 * self.padding) / self.cell_width).floor().max(1.0);
            let rows = ((self.height - 2.0 * self.padding) / self.cell_height).floor().max(1.0);
            let coordinate = |value: f32, extent: f64, cell: f64, count: f64| {
                let raw = (f64::from(value) - self.padding) / cell;
                if value >= 0.0 && f64::from(value) <= extent {
                    raw.clamp(0.0, count - 0.0001) as f32
                } else {
                    raw as f32
                }
            };
            event.x = coordinate(event.x, self.width, self.cell_width, cols);
            event.y = coordinate(event.y, self.height, self.cell_height, rows);
            vt::MouseGeometry {
                width: cols as u32,
                height: rows as u32,
                cell_width: 1,
                cell_height: 1,
                ..Default::default()
            }
        };
        encoder.encode(event, tracking, format, geometry, self.any_button)
    }
}

pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
    }
    for &byte in text.as_bytes() {
        match byte {
            b'\n' if !bracketed => out.push(b'\r'),
            b'\n' | b'\r' | b'\t' | 0x20..=0x7e | 0x80..=0xff => out.push(byte),
            _ => {}
        }
    }
    if bracketed {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_layout_identity_is_kept_distinct_from_physical_identity() {
        let event = KeyEvent {
            key: Key::Character("Z".into()),
            text: Some("Z".into()),
            pressed: true,
            repeat: false,
            code: 21,
            modifiers: Modifiers::SHIFT,
            composing: None,
            native: aizome_kit::sidestep_ui::KeyMetadata {
                keysym: 0x5a,
                text: "Z".into(),
                unshifted: Some('z'),
                consumed_modifiers: Modifiers::SHIFT,
            },
        };
        let normalized = key_event(&event);
        assert_eq!(normalized.key, vt::Key::Character('y'));
        assert_eq!(normalized.unshifted, Some('z'));
        assert_eq!(normalized.text, "Z");
        assert_eq!(normalized.consumed_modifiers, vt::Modifiers::SHIFT);
    }
    #[test]
    fn fractional_cell_coordinates_do_not_drift() {
        let input = MouseInput {
            event: vt::MouseEvent {
                action: vt::MouseAction::Press,
                button: Some(vt::MouseButton::Left),
                x: (10.0 + 9.5 * 8.4) as f32,
                y: 20.0,
                ..Default::default()
            },
            width: 860.0,
            height: 400.0,
            cell_width: 8.4,
            cell_height: 21.0,
            padding: 10.0,
            any_button: true,
        };
        assert_eq!(
            input.encode(&mut vt::MouseEncoder::default(), vt::MouseTracking::Normal, vt::MouseFormat::Sgr),
            b"\x1b[<0;10;1M"
        );
    }
    #[test]
    fn paste_cannot_terminate_its_bracket() {
        assert_eq!(paste_bytes("a\x1b[201~\nb", true), b"\x1b[200~a[201~\nb\x1b[201~");
        assert_eq!(paste_bytes("a\nb", false), b"a\rb");
    }
}
