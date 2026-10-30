use hotaru_core::formatter::{FormatOptions, format_copy};
use hotaru_core::{Color, Terminal};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

fn bytes(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect()
}

fn color(value: Color) -> Value {
    match value {
        Color::Default => json!({"default": true}),
        Color::Indexed(index) => json!({"index": index}),
        Color::Rgb(rgb) => json!({"rgb": [rgb.0, rgb.1, rgb.2]}),
    }
}

#[test]
fn repeat_and_saved_modes_match_actual_pinned_library() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../upstream/vt-controls-fixtures.json")).unwrap();
    assert_matches_recorded(&fixtures);
}

#[test]
fn stream_parser_and_widths_match_actual_pinned_library() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../upstream/stream-parser-fixtures.json"
    ))
    .unwrap();
    assert_matches_recorded(&fixtures);
}

/// Feeds every recorded case at several chunk sizes and compares the cells,
/// cursor, selected modes and replies the actual pinned library produced.
fn assert_matches_recorded(fixtures: &Value) {
    for case in fixtures["cases"].as_array().unwrap() {
        let data = bytes(&case["input"]);
        for chunk in [1, 7, 65536] {
            let context = format!("{} chunk {chunk}", case["name"]);
            let mut terminal = Terminal::new(
                case["cols"].as_u64().unwrap() as usize,
                case["rows"].as_u64().unwrap() as usize,
                0,
            );
            for part in data.chunks(chunk) {
                terminal.feed(part);
            }
            let snapshot = terminal.snapshot();
            assert_eq!(
                json!({"row":snapshot.cursor.row,"col":snapshot.cursor.col,"visible":snapshot.cursor.visible}),
                case["cursor"],
                "{context}"
            );
            assert_eq!(
                terminal.take_responses(),
                bytes(&case["responses"]),
                "{context}"
            );
            assert_eq!(
                json!({"alternate_screen":snapshot.modes.alternate_screen,"application_cursor":snapshot.modes.application_cursor,"bracketed_paste":snapshot.modes.bracketed_paste,"focus_events":snapshot.modes.focus_events}),
                case["modes"],
                "{context}"
            );
            for (row, expected) in snapshot.lines.iter().zip(case["lines"].as_array().unwrap()) {
                assert_eq!(
                    row.wrapped(),
                    expected["wrapped"].as_bool().unwrap(),
                    "{context}"
                );
                for (col, cell) in row.cells().iter().copied().enumerate() {
                    let text = row
                        .grapheme(col)
                        .map(str::to_owned)
                        .unwrap_or_else(|| cell.character().to_string());
                    let style = row.style(cell);
                    assert_eq!(
                        json!([
                            text,
                            cell.width(),
                            color(style.foreground),
                            color(style.background),
                            color(style.underline_color),
                            style.attrs
                        ]),
                        expected["cells"][col],
                        "{context} col {col}"
                    );
                }
            }
        }
    }
}

#[test]
fn repetition_keeps_owned_link_and_history_captures() {
    let mut terminal = Terminal::new(12, 3, 200);
    terminal.feed("\x1b[?2027h\x1b]8;;https://example.org\x1b\\猫\x1b[1000b".as_bytes());
    let captured = terminal.content_snapshot();
    let format = || {
        format_copy(
            &captured,
            &FormatOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
        .plain
        .unwrap()
    };
    let text = format();
    terminal.feed(b"\x1b]8;;\x1b\\\x1b[?1049hA\x1b[1000b\x1b[?1049l\x1b[2J");
    drop(terminal);
    assert_eq!(format(), text);
    assert!(captured.rows().any(|row| row.hyperlink(0).is_some()));
}
