use hotaru_core::formatter::{CopyFormat, FormatOptions, format_copy};
use hotaru_core::{Color, GraphemeWidthMethod, Terminal};
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
fn color(color: Color) -> Value {
    match color {
        Color::Default => json!({"default": true}),
        Color::Indexed(index) => json!({"index": index}),
        Color::Rgb(rgb) => json!({"rgb": [rgb.0, rgb.1, rgb.2]}),
    }
}

fn check(terminal: &mut Terminal, case: &Value, split: usize) {
    let expected = &case["snapshot"];
    let snapshot = terminal.snapshot();
    let context = format!("{} split{split}", case["name"]);
    assert_eq!(
        snapshot.cursor.row,
        expected["cursor"]["row"].as_u64().unwrap() as usize,
        "{context}: cursor row"
    );
    assert_eq!(
        snapshot.cursor.col,
        expected["cursor"]["col"].as_u64().unwrap() as usize,
        "{context}: cursor col"
    );
    assert_eq!(
        snapshot.cursor.visible,
        expected["cursor"]["visible"].as_bool().unwrap(),
        "{context}: cursor visible"
    );
    assert_eq!(
        snapshot.modes.grapheme_cluster,
        expected["_capture"]["grapheme_cluster"].as_bool().unwrap(),
        "{context}: mode"
    );
    assert_eq!(
        terminal.take_responses(),
        bytes(&expected["responses"]),
        "{context}: responses"
    );
    for (row, line) in snapshot.lines.iter().enumerate() {
        let expected_row = &expected["lines"][row];
        assert_eq!(
            line.wrapped(),
            expected_row["wrapped"].as_bool().unwrap(),
            "{context}: wrap row{row}"
        );
        for (col, cell) in line.cells().iter().copied().enumerate() {
            let expected_cell = &expected_row["cells"][col];
            let text = line
                .grapheme(col)
                .map(str::to_owned)
                .unwrap_or_else(|| cell.character().to_string());
            assert_eq!(
                text,
                expected_cell["text"].as_str().unwrap(),
                "{context}: text row{row} col{col}"
            );
            assert_eq!(
                cell.width(),
                expected_cell["width"].as_u64().unwrap() as usize,
                "{context}: width row{row} col{col}"
            );
            let style = line.style(cell);
            assert_eq!(
                color(style.foreground),
                expected_cell["foreground"],
                "{context}: fg row{row} col{col}"
            );
            assert_eq!(
                color(style.background),
                expected_cell["background"],
                "{context}: bg row{row} col{col}"
            );
            assert_eq!(
                color(style.underline_color),
                expected_cell["underline_color"],
                "{context}: underline row{row} col{col}"
            );
            assert_eq!(
                u64::from(style.attrs),
                expected_cell["attrs"].as_u64().unwrap(),
                "{context}: attrs row{row} col{col}"
            );
            let link = line.hyperlink(col).map_or(&[][..], |link| &link.uri);
            assert_eq!(
                link,
                bytes(&expected["_capture"]["links"][row][col]),
                "{context}: link row{row} col{col}"
            );
        }
    }
    for (format, name) in [(CopyFormat::Plain, "plain"), (CopyFormat::Vt, "vt")] {
        let output = format_copy(
            &terminal.content_snapshot(),
            &FormatOptions {
                format,
                ..FormatOptions::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let actual = output.plain.unwrap();
        // Public VT copies prepend captured default colors. The C content
        // formatter oracle deliberately records only selection body bytes.
        let actual = if format == CopyFormat::Vt {
            actual
                .strip_prefix(b"\x1b]10;rgb:e5/e5/e5\x1b\\\x1b]11;rgb:00/00/00\x1b\\")
                .expect("captured VT defaults")
        } else {
            &actual
        };
        let mut reference = bytes(&expected["_capture"]["formats"][name]);
        if format == CopyFormat::Vt && reference.windows(3).any(|s| s == b";5;") {
            // The application copy API resolves captured palette indices to
            // RGB; preserve every other byte of the raw reference body.
            let mut body = String::from_utf8(reference).unwrap();
            for command in [38, 48, 58] {
                for index in 0..256 {
                    let rgb = snapshot.palette[index];
                    body = body.replace(
                        &format!("\x1b[{command};5;{index}m"),
                        &format!("\x1b[{command};2;{};{};{}m", rgb.0, rgb.1, rgb.2),
                    );
                }
            }
            reference = body.into_bytes();
        }
        assert_eq!(actual, reference, "{context}: {name} copy");
    }
}

#[test]
fn unicode_terminal_matches_pinned_ghostty_at_every_byte_split() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../upstream/unicode-mode-fixtures.json")).unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let data = bytes(&case["input"]);
        for split in 0..=data.len() {
            let mut terminal = Terminal::new(
                case["cols"].as_u64().unwrap() as usize,
                case["rows"].as_u64().unwrap() as usize,
                0,
            );
            terminal.set_default_grapheme_width(if case["unicode"].as_bool().unwrap() {
                GraphemeWidthMethod::Unicode
            } else {
                GraphemeWidthMethod::Legacy
            });
            terminal.feed(&data[..split]);
            terminal.feed(&data[split..]);
            if let Some(sizes) = case["resizes"].as_array() {
                for size in sizes {
                    terminal.resize(
                        size[0].as_u64().unwrap() as usize,
                        size[1].as_u64().unwrap() as usize,
                    );
                }
            }
            check(&mut terminal, case, split);
        }
    }
}

fn same_content(a: &hotaru_core::ContentSnapshot, b: &hotaru_core::ContentSnapshot) {
    assert_eq!((a.cols, a.rows, a.cursor), (b.cols, b.rows, b.cursor));
    for (a, b) in a.rows().zip(b.rows()) {
        assert_eq!(a.cells(), b.cells());
        assert_eq!(a.wrapped(), b.wrapped());
        for (col, cell) in a.cells().iter().copied().enumerate() {
            assert_eq!(a.style(cell), b.style(b.cells()[col]));
            assert_eq!(a.grapheme(col), b.grapheme(col));
            assert_eq!(a.hyperlink(col), b.hyperlink(col));
        }
    }
}

#[test]
fn maximum_clusters_survive_archive_reflow_copy_search_and_teardown() {
    use hotaru_core::{HistoryLimits, search};
    let cluster = format!("😀{}", "\u{1d185}".repeat(64));
    assert_eq!(cluster.chars().count(), 65);
    assert_eq!(cluster.len(), 260);
    let mut resident = Terminal::with_history_limits(
        12,
        4,
        HistoryLimits {
            rows: 1000,
            bytes: None,
        },
    );
    let mut cold = Terminal::with_history_limits(
        12,
        4,
        HistoryLimits {
            rows: 1000,
            bytes: None,
        },
    );
    for t in [&mut resident, &mut cold] {
        t.set_default_grapheme_width(GraphemeWidthMethod::Unicode);
        for _ in 0..700 {
            t.feed(
                format!("\x1b]8;id=bounded;https://example.test\x07{cluster}\x1b]8;;\x07\r\n")
                    .as_bytes(),
            );
        }
    }
    let held = resident.content_snapshot();
    while cold.compress_history_step() {}
    assert!(cold.history_memory().compressed_rows > 256);
    let archived = cold.content_snapshot();
    same_content(&held, &archived);
    let matches = search::find(
        archived.clone(),
        cluster.as_bytes(),
        search::Options::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(matches.total_matches, 700);
    assert!(
        matches
            .matches
            .iter()
            .all(|m| m.start.col == 0 && m.end.col == 2)
    );
    for (cols, rows) in [(3, 5), (1, 4), (30, 7), (12, 4)] {
        resident.resize(cols, rows);
        cold.resize(cols, rows);
        same_content(&resident.content_snapshot(), &cold.content_snapshot());
    }
    drop(resident);
    drop(cold);
    same_content(&held, &archived);
    let copy = format_copy(
        &archived,
        &FormatOptions {
            format: CopyFormat::Plain,
            ..FormatOptions::default()
        },
        &AtomicBool::new(false),
    )
    .unwrap()
    .plain
    .unwrap();
    assert_eq!(
        String::from_utf8(copy).unwrap().matches(&cluster).count(),
        700
    );
}

#[test]
fn unicode_metadata_mutations_preserve_chunk_invariant_byte_budgets() {
    use hotaru_core::HistoryLimits;
    for chunk in [1, 7, 65536] {
        let make = || {
            let mut terminal = Terminal::with_history_limits(
                12,
                4,
                HistoryLimits {
                    rows: 1000,
                    bytes: Some(80000),
                },
            );
            terminal.set_default_grapheme_width(GraphemeWidthMethod::Unicode);
            terminal
        };
        let mut whole = make();
        let mut pieces = make();
        let mut held = Vec::new();
        for index in 0..180 {
            let uri = "u".repeat((index * 29) % 1500);
            let marks = "\u{1d185}".repeat(index % 70);
            let stage = format!(
                "\x1b]8;id={index};{uri}\x07\x1b[38;5;{}m👩\u{200d}💻 🇯🇵 ❤\x1b[44m️\r\n😀{marks}\x1b]8;;\x07\x1b[0m\r\n",
                index % 256
            );
            whole.feed(stage.as_bytes());
            for part in stage.as_bytes().chunks(chunk) {
                pieces.feed(part);
            }
            if index % 19 == 0 {
                while pieces.compress_history_step() {}
            }
            if index % 31 == 0 {
                held.push((whole.content_snapshot(), pieces.content_snapshot()));
            }
            if index % 43 == 0 {
                whole.resize(7, 4);
                pieces.resize(7, 4);
            }
            if index % 43 == 3 {
                whole.resize(12, 4);
                pieces.resize(12, 4);
            }
            let a = whole.history_memory();
            let b = pieces.history_memory();
            assert_eq!(
                (a.rows, a.logical_bytes, a.active_bytes),
                (b.rows, b.logical_bytes, b.active_bytes),
                "chunk{chunk} stage{index}"
            );
            assert!(a.logical_bytes <= 80000usize.saturating_sub(a.active_bytes));
            same_content(&whole.content_snapshot(), &pieces.content_snapshot());
        }
        drop(whole);
        drop(pieces);
        for (a, b) in held {
            same_content(&a, &b);
        }
    }
}
