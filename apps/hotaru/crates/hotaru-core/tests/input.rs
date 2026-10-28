use hotaru_core::Terminal;
use hotaru_core::input::*;
use serde_json::Value;

fn key(name: &str) -> Key {
    use Key::*;
    if name.len() == 1 {
        return Character(name.to_ascii_lowercase().chars().next().unwrap());
    }
    if let Some(n) = name.strip_prefix("DIGIT_") {
        return Character(n.chars().next().unwrap());
    }
    if let Some(n) = name.strip_prefix('F').and_then(|v| v.parse::<u8>().ok()) {
        return F(n);
    }
    if let Some(n) = name.strip_prefix("NUMPAD_") {
        return Keypad(match n {
            n if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() => {
                KeypadKey::Digit(n.parse().unwrap())
            }
            "DECIMAL" => KeypadKey::Decimal,
            "DIVIDE" => KeypadKey::Divide,
            "MULTIPLY" => KeypadKey::Multiply,
            "SUBTRACT" => KeypadKey::Subtract,
            "ADD" => KeypadKey::Add,
            "ENTER" => KeypadKey::Enter,
            "EQUAL" => KeypadKey::Equal,
            "SEPARATOR" => KeypadKey::Separator,
            "LEFT" => KeypadKey::Left,
            "RIGHT" => KeypadKey::Right,
            "UP" => KeypadKey::Up,
            "DOWN" => KeypadKey::Down,
            "PAGE_UP" => KeypadKey::PageUp,
            "PAGE_DOWN" => KeypadKey::PageDown,
            "HOME" => KeypadKey::Home,
            "END" => KeypadKey::End,
            "INSERT" => KeypadKey::Insert,
            "DELETE" => KeypadKey::Delete,
            "BEGIN" => KeypadKey::Begin,
            _ => panic!("unmapped fixture keypad {name}"),
        });
    }
    match name {
        "ESCAPE" => Escape,
        "ENTER" => Enter,
        "TAB" => Tab,
        "BACKSPACE" => Backspace,
        "INSERT" => Insert,
        "DELETE" => Delete,
        "ARROW_LEFT" => ArrowLeft,
        "ARROW_RIGHT" => ArrowRight,
        "ARROW_UP" => ArrowUp,
        "ARROW_DOWN" => ArrowDown,
        "PAGE_UP" => PageUp,
        "PAGE_DOWN" => PageDown,
        "HOME" => Home,
        "END" => End,
        "HELP" => Help,
        "CONTEXT_MENU" => ContextMenu,
        "CAPS_LOCK" => CapsLock,
        "SCROLL_LOCK" => ScrollLock,
        "NUM_LOCK" => NumLock,
        "PRINT_SCREEN" => PrintScreen,
        "PAUSE" => Pause,
        "SHIFT_LEFT" => Modifier(ModifierKey::ShiftLeft),
        "SHIFT_RIGHT" => Modifier(ModifierKey::ShiftRight),
        "CONTROL_LEFT" => Modifier(ModifierKey::ControlLeft),
        "CONTROL_RIGHT" => Modifier(ModifierKey::ControlRight),
        "ALT_LEFT" => Modifier(ModifierKey::AltLeft),
        "ALT_RIGHT" => Modifier(ModifierKey::AltRight),
        "META_LEFT" => Modifier(ModifierKey::SuperLeft),
        "META_RIGHT" => Modifier(ModifierKey::SuperRight),
        "SPACE" => Character(' '),
        "SLASH" => Character('/'),
        "BRACKET_LEFT" => Character('['),
        "BRACKET_RIGHT" => Character(']'),
        "MINUS" => Character('-'),
        "EQUAL" => Character('='),
        "BACKSLASH" => Character('\\'),
        "SEMICOLON" => Character(';'),
        "QUOTE" => Character('\''),
        "BACKQUOTE" => Character('`'),
        "COMMA" => Character(','),
        "PERIOD" => Character('.'),
        "UNIDENTIFIED" => Unidentified,
        _ => panic!("unmapped fixture key {name}"),
    }
}
fn number(value: &Value) -> u64 {
    value.as_u64().unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|x| number(x) as u8)
        .collect()
}
fn fixtures() -> Value {
    serde_json::from_str(include_str!("../../../upstream/input-fixtures.json")).unwrap()
}

#[test]
fn key_encodings_match_pinned_ghostty_c_api() {
    let data = fixtures();
    assert_eq!(
        data["upstream_revision"],
        "6ba144158577d033ccb6ed4b574e4013957c9161"
    );
    let mut failures = Vec::new();
    let mut count = 0;
    // [key,text,unshifted,mods,consumed,action,composing,options,kitty,bytes].
    for row in data["key_cases"].as_array().unwrap() {
        let bits = number(&row[7]);
        let event = KeyEvent {
            key: key(row[0].as_str().unwrap()),
            text: row[1].as_str().unwrap().into(),
            unshifted: char::from_u32(number(&row[2]) as u32).filter(|c| *c != '\0'),
            modifiers: Modifiers(number(&row[3]) as u16),
            consumed_modifiers: Modifiers(number(&row[4]) as u16),
            action: match number(&row[5]) {
                0 => KeyAction::Release,
                1 => KeyAction::Press,
                _ => KeyAction::Repeat,
            },
            composing: row[6].as_bool().unwrap(),
        };
        let options = KeyboardOptions {
            application_cursor: bits & 1 != 0,
            application_keypad: bits & 2 != 0,
            ignore_keypad_with_numlock: bits & 4 != 0,
            alt_esc_prefix: bits & 8 != 0,
            modify_other_keys: bits & 16 != 0,
            backarrow: bits & 32 != 0,
            kitty_flags: KittyFlags(number(&row[8]) as u8),
        };
        let actual = encode_key(&event, options);
        if actual != bytes(&row[9]) {
            count += 1;
            if failures.len() < 15 {
                failures.push(format!("{row}: actual={actual:?}"));
            }
        }
    }
    assert_eq!(
        count,
        0,
        "{count} oracle disagreements:\n{}",
        failures.join("\n")
    );
}

#[test]
fn mouse_encodings_match_pinned_ghostty_c_api() {
    let data = fixtures();
    let mut failures = Vec::new();
    let mut count = 0;
    // [tracking,format,geometry,button,mods,action,x,y,any_pressed,first,repeat].
    for row in data["mouse_cases"].as_array().unwrap() {
        let tracking = match number(&row[0]) {
            0 => MouseTracking::None,
            1 => MouseTracking::X10,
            2 => MouseTracking::Normal,
            3 => MouseTracking::Button,
            _ => MouseTracking::Any,
        };
        let format = match number(&row[1]) {
            0 => MouseFormat::X10,
            1 => MouseFormat::Utf8,
            2 => MouseFormat::Sgr,
            3 => MouseFormat::Urxvt,
            _ => MouseFormat::SgrPixels,
        };
        let g = &row[2];
        let size = MouseGeometry {
            width: number(&g[0]) as u32,
            height: number(&g[1]) as u32,
            cell_width: number(&g[2]) as u32,
            cell_height: number(&g[3]) as u32,
            padding_top: number(&g[4]) as u32,
            padding_bottom: number(&g[5]) as u32,
            padding_right: number(&g[6]) as u32,
            padding_left: number(&g[7]) as u32,
        };
        let event = MouseEvent {
            button: match number(&row[3]) {
                0 => None,
                1 => Some(MouseButton::Left),
                2 => Some(MouseButton::Right),
                3 => Some(MouseButton::Middle),
                4 => Some(MouseButton::WheelUp),
                5 => Some(MouseButton::WheelDown),
                6 => Some(MouseButton::WheelLeft),
                7 => Some(MouseButton::WheelRight),
                8 => Some(MouseButton::Eight),
                _ => Some(MouseButton::Nine),
            },
            modifiers: Modifiers(number(&row[4]) as u16),
            action: match number(&row[5]) {
                0 => MouseAction::Press,
                1 => MouseAction::Release,
                _ => MouseAction::Motion,
            },
            x: row[6].as_f64().unwrap() as f32,
            y: row[7].as_f64().unwrap() as f32,
        };
        let mut encoder = MouseEncoder::default();
        for column in [9, 10] {
            let actual = encoder.encode(event, tracking, format, size, row[8].as_bool().unwrap());
            if actual != bytes(&row[column]) {
                count += 1;
                if failures.len() < 15 {
                    failures.push(format!("{row}: call={column} actual={actual:?}"));
                }
            }
        }
    }
    assert_eq!(
        count,
        0,
        "{count} oracle disagreements:\n{}",
        failures.join("\n")
    );
}

#[test]
fn protocol_negotiation_matches_pinned_ghostty_c_api() {
    let data = fixtures();
    for (index, row) in data["negotiation_cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        for chunk in [1, 7, 65536] {
            let mut terminal = Terminal::new(80, 24, 0);
            for part in bytes(&row[0]).chunks(chunk) {
                terminal.feed(part);
            }
            assert_eq!(
                terminal.take_responses(),
                bytes(&row[1]),
                "negotiation{index} chunk{chunk}"
            );
            for key_row in row[2].as_array().unwrap() {
                let event = KeyEvent {
                    key: key(key_row[0].as_str().unwrap()),
                    text: key_row[1].as_str().unwrap().into(),
                    unshifted: char::from_u32(number(&key_row[2]) as u32).filter(|c| *c != '\0'),
                    modifiers: Modifiers(number(&key_row[3]) as u16),
                    ..KeyEvent::default()
                };
                assert_eq!(
                    encode_key(&event, terminal.keyboard_options()),
                    bytes(&key_row[4]),
                    "negotiation{index} key{key_row}"
                );
            }
            let (tracking, format) = terminal.mouse_modes();
            let mut mouse = MouseEncoder::default();
            let size = MouseGeometry {
                width: 1000,
                height: 600,
                cell_width: 10,
                cell_height: 20,
                padding_top: 3,
                padding_bottom: 7,
                padding_right: 9,
                padding_left: 5,
            };
            let event = MouseEvent {
                button: Some(MouseButton::Left),
                x: 35.0,
                y: 55.0,
                ..MouseEvent::default()
            };
            assert_eq!(
                mouse.encode(event, tracking, format, size, true),
                bytes(&row[3]),
                "negotiation{index} mouse"
            );
        }
    }
}

#[test]
fn kitty_stack_is_per_screen_bounded_and_zero_pop_is_distinct() {
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.feed(b"\x1b[>1u\x1b[>2u\x1b[<0u\x1b[?u\x1b[<u\x1b[?u");
    assert_eq!(terminal.take_responses(), b"\x1b[?2u\x1b[?1u");
    terminal.feed(b"\x1b[?1049h\x1b[?u\x1b[>31u\x1b[?1049l\x1b[?u");
    assert_eq!(terminal.take_responses(), b"\x1b[?0u\x1b[?1u");
    terminal.feed(b"\x1b[=3;1u\x1b[=4;2u\x1b[=1;3u\x1b[?u\x1b[<9999u\x1b[?u");
    assert_eq!(terminal.take_responses(), b"\x1b[?6u\x1b[?0u");
    terminal.feed(b"\x1b[>32u\x1b[?u");
    assert_eq!(terminal.take_responses(), b"\x1b[?0u");
}

#[test]
fn current_modes_drive_keyboard_and_mouse_encoding() {
    let mut terminal = Terminal::new(80, 24, 0);
    assert!(terminal.keyboard_options().ignore_keypad_with_numlock);
    assert!(terminal.keyboard_options().alt_esc_prefix);
    terminal.feed(b"\x1b[?1h\x1b=\x1b[?1035l\x1b[?1002;1006h");
    assert_eq!(
        encode_key(&KeyEvent::new(Key::ArrowUp), terminal.keyboard_options()),
        b"\x1bOA"
    );
    assert_eq!(
        encode_key(
            &KeyEvent::new(Key::Keypad(KeypadKey::Digit(3))),
            terminal.keyboard_options()
        ),
        b"\x1bOs"
    );
    assert_eq!(
        terminal.mouse_modes(),
        (MouseTracking::Button, MouseFormat::Sgr)
    );
    terminal.feed(b"\x1b[?9h\x1b[?1002l\x1b[?9$p\x1b[?1002$p\x1b[?1005h\x1b[?1006l");
    assert_eq!(terminal.take_responses(), b"\x1b[?9;1$y\x1b[?1002;2$y");
    assert_eq!(
        terminal.mouse_modes(),
        (MouseTracking::None, MouseFormat::X10)
    );
    terminal.feed(b"\x1bc");
    assert_eq!(terminal.keyboard_options().kitty_flags, KittyFlags(0));
    assert!(!terminal.keyboard_options().application_cursor);
}

#[test]
fn hostile_geometry_is_rejected_without_panicking() {
    let mut encoder = MouseEncoder::default();
    let event = MouseEvent {
        x: f32::NAN,
        ..MouseEvent::default()
    };
    assert!(
        encoder
            .encode(
                event,
                MouseTracking::Any,
                MouseFormat::SgrPixels,
                MouseGeometry::default(),
                false
            )
            .is_empty()
    );
    let size = MouseGeometry {
        width: 1,
        height: 1,
        cell_width: 1,
        cell_height: 1,
        padding_left: u32::MAX,
        ..MouseGeometry::default()
    };
    assert!(
        encoder
            .encode(
                MouseEvent::default(),
                MouseTracking::Any,
                MouseFormat::Utf8,
                size,
                false
            )
            .is_empty()
    );
}
