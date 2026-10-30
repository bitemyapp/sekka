use hotaru_core::{CursorShape, Terminal};
use serde_json::Value;

fn bytes(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect()
}

#[test]
fn cursor_state_and_queries_match_real_ghostty_library() {
    let corpus: Value =
        serde_json::from_str(include_str!("../../../upstream/cursor-fixtures.json")).unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let data = bytes(&case["input"]);

        for chunk in [1, 7, 65536] {
            let mut terminal = Terminal::new(10, 4, 100);
            let shape = |value: &Value| match value.as_i64().unwrap() {
                0 => CursorShape::Bar,
                1 => CursorShape::Block,
                2 => CursorShape::Underline,
                _ => panic!("invalid fixture shape"),
            };
            if let Some(blink) = case["default"].as_bool() {
                terminal.set_default_cursor(shape(&case["default_shape"]), blink);
            }
            for part in data.chunks(chunk) {
                terminal.feed(part);
            }
            if case["configure_after"].is_array() {
                terminal.set_default_cursor(
                    shape(&case["configure_after"][0]),
                    case["configure_after"][1].as_bool().unwrap(),
                );
            }
            let mut suffix = case.get("after").map(bytes).unwrap_or_default();
            suffix.extend_from_slice(b"\x1bP$q q\x1b\\\x1b[?12$p\x1b[?25$p");
            for part in suffix.chunks(chunk) {
                terminal.feed(part);
            }
            let c = terminal
                .snapshot_scrolled(case["scroll"].as_u64().unwrap_or(0) as usize)
                .cursor;
            let shape = match c.shape {
                CursorShape::Bar => 0,
                CursorShape::Block => 1,
                CursorShape::Underline => 2,
            };
            let context = format!(
                "{} default {:?} chunk {chunk}",
                case["name"], case["default"]
            );
            assert_eq!(
                shape,
                case["CURSOR_VISUAL_STYLE"].as_i64().unwrap(),
                "{context}"
            );
            assert_eq!(
                c.visible,
                case["CURSOR_VISIBLE"].as_bool().unwrap()
                    && case["CURSOR_VIEWPORT_HAS_VALUE"].as_bool().unwrap(),
                "{context}"
            );
            assert_eq!(
                c.blinking,
                case["CURSOR_BLINKING"].as_bool().unwrap(),
                "{context}"
            );
            assert_eq!(
                c.in_viewport,
                case["CURSOR_VIEWPORT_HAS_VALUE"].as_bool().unwrap(),
                "{context}"
            );
            if c.in_viewport {
                assert_eq!(
                    c.col as u64,
                    case["CURSOR_VIEWPORT_X"].as_u64().unwrap(),
                    "{context}"
                );
                assert_eq!(
                    c.row as u64,
                    case["CURSOR_VIEWPORT_Y"].as_u64().unwrap(),
                    "{context}"
                );
            }
            assert_eq!(
                terminal.take_responses(),
                bytes(&case["responses"]),
                "{context}"
            );
        }
    }
}

#[test]
fn cursor_defaults_snapshots_and_viewport_are_owned() {
    let mut term = Terminal::new(10, 4, 100);
    term.set_default_cursor(CursorShape::Bar, true);
    assert_eq!(term.snapshot().cursor.shape, CursorShape::Bar);
    term.feed(b"\x1b[4 q");
    term.set_default_cursor(CursorShape::Block, true);
    assert_eq!(term.snapshot().cursor.shape, CursorShape::Underline);
    assert!(!term.snapshot().cursor.blinking);
    let held = term.snapshot();
    term.feed(b"\x1b[ q");
    assert_eq!(term.snapshot().cursor.shape, CursorShape::Block);
    assert!(term.snapshot().cursor.blinking);
    assert_eq!(held.cursor.shape, CursorShape::Underline);
    for _ in 0..10 {
        term.feed(b"x\r\n");
    }
    assert!(!term.snapshot_scrolled(1).cursor.in_viewport);
    assert!(term.snapshot().cursor.in_viewport);
    term.feed(b"\x1b[H");
    assert_eq!(term.snapshot_scrolled(1).cursor.row, 1);
    assert!(term.snapshot_scrolled(1).cursor.in_viewport);
    assert!(term.snapshot_scrolled(1).cursor.visible);
    term.feed(b"\x1b[?25l");
    assert!(term.snapshot().cursor.in_viewport);
    assert!(!term.snapshot().cursor.visible);
    term.feed(b"\x1b[6 q\x1bc");
    assert_eq!(term.snapshot().cursor.shape, CursorShape::Block);
    assert!(term.snapshot().cursor.blinking);
}
