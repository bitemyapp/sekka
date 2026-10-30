use hotaru_core::{GraphemeWidthMethod, HistoryLimits, SemanticContent, Terminal};
use serde_json::{Value, json};
use std::io::{Cursor, Read};

#[test]
fn semantic_cells_rows_and_clear_behavior_match_pinned_library() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../upstream/semantic-fixtures.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let cols = case["cols"].as_u64().unwrap() as usize;
        let rows = case["rows"].as_u64().unwrap() as usize;
        let encoded = case["operations"].as_str().unwrap();
        let bytes: Vec<_> = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
            .collect();
        for chunk in [1, 7, 65536] {
            let mut terminal = Terminal::with_history_limits(
                cols,
                rows,
                HistoryLimits {
                    rows: 256,
                    bytes: None,
                },
            );
            terminal.set_default_grapheme_width(if case["mode"] == "unicode" {
                GraphemeWidthMethod::Unicode
            } else {
                GraphemeWidthMethod::Legacy
            });
            let mut input = Cursor::new(&bytes);
            let mut op = [0];
            while input.read_exact(&mut op).is_ok() {
                match op[0] {
                    b'F' => {
                        let mut len = [0; 4];
                        input.read_exact(&mut len).unwrap();
                        let mut bytes = vec![0; u32::from_le_bytes(len) as usize];
                        input.read_exact(&mut bytes).unwrap();
                        for part in bytes.chunks(chunk) {
                            terminal.feed(part);
                        }
                    }
                    b'R' => {
                        let mut dims = [0; 4];
                        input.read_exact(&mut dims).unwrap();
                        terminal.resize(
                            u16::from_le_bytes(dims[..2].try_into().unwrap()) as usize,
                            u16::from_le_bytes(dims[2..].try_into().unwrap()) as usize,
                        );
                    }
                    b'C' => while terminal.compress_history_step() {},
                    _ => panic!("invalid oracle fixture"),
                }
            }
            let snapshot = terminal.snapshot_all();
            let cells: Vec<_> = snapshot
                .lines
                .iter()
                .map(|row| {
                    row.cells()
                        .iter()
                        .map(|cell| {
                            [
                                cell.character() as u32,
                                cell.semantic_content() as u32,
                                cell.width() as u32,
                                u32::from(cell.is_protected()),
                            ]
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let prompts: Vec<_> = snapshot
                .lines
                .iter()
                .map(|row| row.semantic_prompt() as u8)
                .collect();
            let expected = &case["expected"];
            assert_eq!(
                json!(cells),
                expected["cells"],
                "{} {} chunk {chunk}",
                case["name"],
                case["mode"]
            );
            assert_eq!(
                json!(prompts),
                expected["row_prompts"],
                "{} {} chunk {chunk}",
                case["name"],
                case["mode"]
            );
            assert_eq!(
                json!([snapshot.cursor.col, snapshot.cursor.row]),
                expected["cursor"],
                "{} {} chunk {chunk}",
                case["name"],
                case["mode"]
            );
            assert_eq!(snapshot.cols, expected["cols"].as_u64().unwrap() as usize);
        }
    }
}

#[test]
fn retained_semantic_rows_survive_overwrite_reflow_compression_and_drop() {
    let mut terminal = Terminal::with_history_limits(
        20,
        4,
        HistoryLimits {
            rows: 256,
            bytes: None,
        },
    );
    for _ in 0..80 {
        terminal.feed(b"\x1b]133;P\x07prompt>\x1b]133;B\x07input\x1b]133;C\x07out\r\n");
    }
    while terminal.compress_history_step() {}
    let captured = terminal.content_snapshot();
    let collect = || {
        captured
            .rows()
            .map(|row| {
                (
                    row.semantic_prompt(),
                    row.cells()
                        .iter()
                        .map(|c| (c.character(), c.semantic_content()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = collect();
    assert!(expected.iter().any(|(_, cells)| {
        cells
            .iter()
            .any(|(_, semantic)| *semantic == SemanticContent::Prompt)
    }));
    assert!(expected.iter().any(|(_, cells)| {
        cells
            .iter()
            .any(|(_, semantic)| *semantic == SemanticContent::Input)
    }));
    terminal.feed(b"\x1b[2J\x1b]133;D\x07replacement");
    terminal.resize(7, 9);
    terminal.feed(&vec![b'x'; 4000]);
    while terminal.compress_history_step() {}
    drop(terminal);
    assert_eq!(collect(), expected);
}
