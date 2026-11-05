use hotaru_core::{Cols, GridSize, Rows, Terminal};
use serde_json::Value;

fn bytes(value: &Value) -> Vec<u8> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_u64().unwrap() as u8)
        .collect()
}

#[test]
fn osc8_cells_match_pinned_ghostty_at_read_boundaries() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../upstream/hyperlink-fixtures.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        for chunk in [1, 7, 65536] {
            let cols = case["initial_cols"]
                .as_u64()
                .unwrap_or(case["cols"].as_u64().unwrap());
            let rows = case["initial_rows"]
                .as_u64()
                .unwrap_or(case["rows"].as_u64().unwrap());
            let mut terminal = Terminal::new(
                GridSize::new(
                    Cols::new(cols as usize).unwrap(),
                    Rows::new(rows as usize).unwrap(),
                ),
                1024,
            );
            for part in bytes(&case["input"]).chunks(chunk) {
                terminal.feed(part);
            }
            if let Some(sizes) = case["resizes"].as_array() {
                for size in sizes {
                    terminal.resize(GridSize::new(
                        Cols::new(size[0].as_u64().unwrap() as usize).unwrap(),
                        Rows::new(size[1].as_u64().unwrap() as usize).unwrap(),
                    ));
                }
            }
            let snapshot = terminal.snapshot();
            for (row, line) in snapshot.lines.iter().enumerate() {
                for col in 0..snapshot.cols {
                    let actual = line.hyperlink(col).map_or(&[][..], |link| &link.uri);
                    assert_eq!(
                        actual,
                        bytes(&case["links"][row][col]),
                        "{} chunk{chunk} row{row} col{col}",
                        case["name"]
                    );
                }
            }
        }
    }
}

#[test]
fn owned_links_survive_erase_resize_and_terminal_destruction() {
    let mut terminal = Terminal::new(
        GridSize::new(Cols::new(10).unwrap(), Rows::new(3).unwrap()),
        128,
    );
    terminal.feed(b"\x1b]8;id=owned;https://example.test\x1b\\hello\x1b]8;;\x1b\\");
    let snapshot = terminal.snapshot();
    terminal.feed(b"\x1b[2J");
    terminal.resize(GridSize::new(Cols::new(2).unwrap(), Rows::new(1).unwrap()));
    drop(terminal);
    for col in 0..5 {
        assert_eq!(
            &*snapshot.lines[0].hyperlink(col).unwrap().uri,
            b"https://example.test"
        );
    }
    assert!(snapshot.lines[0].hyperlink(5).is_none());
}
