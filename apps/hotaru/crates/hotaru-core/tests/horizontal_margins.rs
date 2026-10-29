use hotaru_core::{Color, GraphemeWidthMethod, Rgb, SemanticContent, Snapshot, Terminal};
use serde_json::{Value, json};

fn color(value: Color) -> Value {
    match value {
        Color::Default => json!({"default":true}),
        Color::Indexed(index) => json!({"index":index}),
        Color::Rgb(Rgb(r, g, b)) => json!({"rgb":[r,g,b]}),
    }
}

#[test]
fn one_column_reflow_and_margin_edits_never_create_orphan_wide_tails() {
    fn check(snapshot: &Snapshot, context: &str) {
        for (r, row) in snapshot.lines.iter().enumerate() {
            for (col, cell) in row.cells().iter().enumerate() {
                if cell.width() == 0 {
                    assert!(col > 0);
                    assert_eq!(row.cells()[col - 1].width(), 2);
                }
                if cell.width() == 2 {
                    assert!(col + 1 < row.cells().len());
                    assert_eq!(
                        row.cells()[col + 1].width(),
                        0,
                        "{context} row={r} col={col} cells={:?}",
                        row.cells()
                            .iter()
                            .map(|c| (c.character(), c.width()))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    for method in [GraphemeWidthMethod::Legacy, GraphemeWidthMethod::Unicode] {
        for chunk in [1, 7, 65536] {
            let mut terminal = Terminal::new(8, 3, 100);
            terminal.set_default_grapheme_width(method);
            let initial = "雪😀\u{fe0e}☀\u{fe0f}A\u{301}\r\n雪雪雪雪\r\n";
            for _ in 0..70 {
                for bytes in initial.as_bytes().chunks(chunk) {
                    terminal.feed(bytes);
                }
            }
            let held = terminal.snapshot();
            for cols in [1, 2, 8, 1, 3, 8] {
                terminal.resize(cols, 3);
                check(
                    &terminal.snapshot(),
                    &format!("resize method={method:?} chunk={chunk} cols={cols}"),
                );
                // Fullscreen left edge, an interior right boundary, edits at
                // a wide head/tail, insert mode, alternate screen and reset.
                for sequence in [
                    "\x1b[?69h\x1b[1;3s\x1b[H雪雪\r\x1b[@\x1b[P",
                    "\x1b[H雪A\x1b[2G\x1b[@\x1b[P\x1b[4h雪\x1b[4l",
                    "\x1b[?1049h\x1b[H雪\r\x1b[99@\x1b[99P\x1b[?1049l",
                    "\x1b[?69l\r\x1b[@\x1b[P\x1b[!p",
                ] {
                    for bytes in sequence.as_bytes().chunks(chunk) {
                        terminal.feed(bytes);
                        check(
                            &terminal.snapshot(),
                            &format!(
                                "edit method={method:?} chunk={chunk} cols={cols} seq={sequence:?} bytes={bytes:?}"
                            ),
                        );
                    }
                }
            }
            check(&held, "held");
        }
    }
}

#[test]
fn margin_edit_cow_keeps_old_rows_and_outside_cells_exact() {
    for edit in [b"\x1b[@\x1b[P".as_slice(), b"\x1b[P\x1b[@"] {
        let mut terminal = Terminal::new(8, 3, 0);
        terminal.feed(b"ABCDEFGH\r\nIJKLMNOP\r\nQRSTUVWX");
        terminal.feed("\x1b[?69h\x1b[1;3s\x1b[H雪雪\r".as_bytes());
        let before = terminal.snapshot();
        let before_value = compact(&before, Vec::new());
        terminal.feed(edit);
        let after = terminal.snapshot();
        assert_eq!(compact(&before, Vec::new()), before_value);
        assert!(!hotaru_core::RowRef::ptr_eq(
            &before.lines[1],
            &after.lines[1]
        ));
        assert!(hotaru_core::RowRef::ptr_eq(
            &before.lines[2],
            &after.lines[2]
        ));
        for row in 0..3 {
            for col in 3..8 {
                let a = before.lines[row].cells()[col];
                let b = after.lines[row].cells()[col];
                assert_eq!(a, b);
                assert_eq!(before.lines[row].style(a), after.lines[row].style(b));
            }
        }
        for row in &after.lines {
            for (col, cell) in row.cells().iter().enumerate() {
                if cell.width() == 2 {
                    assert_eq!(row.cells()[col + 1].width(), 0);
                }
            }
        }
    }
}

fn compact(snapshot: &Snapshot, responses: Vec<u8>) -> Value {
    let lines: Vec<_> = snapshot
        .lines
        .iter()
        .map(|row| {
            let cells: Vec<_> = row
                .cells()
                .iter()
                .copied()
                .enumerate()
                .map(|(col, cell)| {
                    let style = row.style(cell);
                    let mut fields = vec![
                        json!(
                            row.grapheme(col)
                                .map(str::to_owned)
                                .unwrap_or_else(|| cell.character().to_string())
                        ),
                        json!(cell.width()),
                    ];
                    if style != hotaru_core::Style::default() {
                        fields.extend([
                            color(style.foreground),
                            color(style.background),
                            color(style.underline_color),
                            json!(style.attrs),
                        ]);
                    }
                    Value::Array(fields)
                })
                .collect();
            json!([row.wrapped(), cells])
        })
        .collect();
    json!({"cursor":[snapshot.cursor.col,snapshot.cursor.row,snapshot.cursor.visible],"lines":lines,"responses":responses,"alternate_screen":snapshot.modes.alternate_screen})
}

#[test]
fn margins_match_actual_library_across_chunks() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../upstream/horizontal-margins-fixtures.json"
    ))
    .unwrap();
    assert_eq!(
        fixtures["library_sha256"],
        "70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639"
    );
    for case in fixtures["cases"].as_array().unwrap() {
        let encoded = case["data_hex"].as_str().unwrap();
        let data: Vec<_> = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
            .collect();
        for chunk in [1, 7, 65536] {
            let mut terminal = Terminal::new(
                case["cols"].as_u64().unwrap() as usize,
                case["rows"].as_u64().unwrap() as usize,
                0,
            );
            terminal.set_default_grapheme_width(if case["mode"] == "unicode" {
                GraphemeWidthMethod::Unicode
            } else {
                GraphemeWidthMethod::Legacy
            });
            for part in data.chunks(chunk) {
                terminal.feed(part);
            }
            let snapshot = terminal.snapshot();
            assert_eq!(
                compact(&snapshot, terminal.take_responses()),
                case["expected"],
                "{} mode={} chunk={chunk}",
                case["name"],
                case["mode"]
            );
        }
    }
}

#[test]
fn pending_wrap_recomputes_margin_before_wide_print_independent_of_chunks() {
    // Pinned Ghostty's scalar path reuses the pre-wrap right boundary, unlike
    // its bulk path. xterm-411 charproc.c recomputes the boundary after WrapLine.
    // This expectation is explicit; the raw divergent reference states remain
    // separate from the ordinary exact-library fixtures.
    for method in [GraphemeWidthMethod::Legacy, GraphemeWidthMethod::Unicode] {
        for (left, right) in [(2, 3), (3, 4), (6, 7)] {
            for text in ["雪X", "😀\u{fe0e}X"] {
                let data = format!("\x1b[?69h\x1b[{left};{right}s\x1b[3;8HX{text}");
                let mut expected = None;
                for split in 0..=data.len() {
                    let mut terminal = Terminal::new(8, 5, 0);
                    terminal.set_default_grapheme_width(method);
                    terminal.feed(&data.as_bytes()[..split]);
                    terminal.feed(&data.as_bytes()[split..]);
                    let snapshot = terminal.snapshot();
                    assert_eq!(snapshot.lines[2].cells()[7].character(), 'X');
                    assert_eq!(
                        snapshot.lines[3].cells()[left - 1].character(),
                        text.chars().next().unwrap()
                    );
                    assert_eq!(snapshot.lines[4].cells()[left - 1].character(), 'X');
                    assert_eq!((snapshot.cursor.row, snapshot.cursor.col), (4, left));
                    let actual = compact(&snapshot, Vec::new());
                    if let Some(expected) = &expected {
                        assert_eq!(&actual, expected, "split {split}");
                    } else {
                        expected = Some(actual);
                    }
                }
            }
        }
    }
}

#[test]
fn rectangular_scroll_preserves_outside_cells_metadata_and_held_rows() {
    let mut terminal = Terminal::new(8, 5, 100);
    for (r, text) in ["abcdefgh", "ijklmnop", "qrstuvwx", "yz012345", "6789ABCD"]
        .into_iter()
        .enumerate()
    {
        terminal.feed(format!("\x1b[{};1H{text}", r + 1).as_bytes());
    }
    terminal.feed("\x1b[2;3H\x1b[31;44m\x1b]133;P\x07\x1b[1\"q\x1b]8;;https://example.org/retained\x07雪\u{301}\x1b]8;;\x07\x1b[0\"q\x1b]133;C\x07\x1b[0m".as_bytes());
    let held = terminal.snapshot();
    let old = compact(&held, Vec::new());
    terminal.feed(b"\x1b[?69h\x1b[3;6s\x1b[2;4r\x1b[2;3H\x1b[L");
    let current = terminal.snapshot();
    for (before, after) in held.lines.iter().zip(&current.lines) {
        for col in [0, 1, 6, 7] {
            assert_eq!(before.cells()[col], after.cells()[col]);
        }
    }
    let moved = &current.lines[2];
    let cell = moved.cells()[2];
    assert_eq!(moved.grapheme(2), Some("雪\u{301}"));
    assert_eq!(moved.style(cell).foreground, Color::Indexed(1));
    assert!(cell.is_protected());
    assert_eq!(cell.semantic_content(), SemanticContent::Prompt);
    assert_eq!(
        moved.hyperlink(2).unwrap().uri.as_ref(),
        b"https://example.org/retained"
    );
    assert_eq!(
        current.lines[1].semantic_prompt(),
        held.lines[1].semantic_prompt()
    );
    assert_eq!(
        current.lines[2].semantic_prompt(),
        held.lines[2].semantic_prompt()
    );
    assert!(current.lines[1].cells()[2..6].iter().all(|c| !c.has_text()));
    terminal.feed(b"\x1b[1;5r\x1b[5;3H");
    for _ in 0..200 {
        terminal.feed(b"\n");
    }
    assert_eq!(terminal.scrollback_len(), 0);
    terminal.resize(3, 3);
    drop(terminal);
    assert_eq!(compact(&held, Vec::new()), old);
}

#[test]
fn only_full_width_regions_touching_the_top_append_history() {
    let text = |row: &hotaru_core::Row| {
        let mut output = String::new();
        row.append_text(&mut output, 0, row.cells().len(), true);
        output
    };
    for (margins, expected) in [(b"1;8".as_slice(), 2), (b"3;6".as_slice(), 0)] {
        let mut terminal = Terminal::new(8, 5, 128);
        terminal.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\x1b[?69h\x1b[");
        terminal.feed(margins);
        terminal.feed(b"s\x1b[1;3r\x1b[3;3H\x1b[2S");
        assert_eq!(terminal.scrollback_len(), expected);
        let snapshot = terminal.snapshot();
        assert_eq!((snapshot.cursor.row, snapshot.cursor.col), (2, 2));
        assert_eq!(text(&snapshot.lines[3]), "four");
        assert_eq!(text(&snapshot.lines[4]), "five");
        if expected != 0 {
            let all = terminal.snapshot_all();
            assert_eq!(text(&all.lines[0]), "one");
            assert_eq!(text(&all.lines[1]), "two");
            assert_eq!(text(&snapshot.lines[0]), "three");
        }
    }
}

#[test]
fn resize_resets_bounds_but_retains_mode_until_soft_reset() {
    let mut terminal = Terminal::new(8, 5, 0);
    terminal.feed(b"\x1b[?69h\x1b[3;6s\x1b[?6h\x1bP$qs\x1b\\");
    assert_eq!(terminal.take_responses(), b"\x1bP1$r3;6s\x1b\\");
    terminal.resize(10, 6);
    terminal.feed(b"\x1bP$qs\x1b\\\x1b[?69$p\x1b[H\x1b[6n");
    assert_eq!(
        terminal.take_responses(),
        b"\x1bP1$r1;10s\x1b\\\x1b[?69;1$y\x1b[1;1R"
    );
    terminal.feed(b"\x1b[4;7s\x1b[?47h\x1bP$qs\x1b\\\x1b[?47l\x1bP$qs\x1b\\");
    assert_eq!(
        terminal.take_responses(),
        b"\x1bP1$r4;7s\x1b\\\x1bP1$r4;7s\x1b\\"
    );
    terminal.feed(b"\x1b[!p\x1bP$qs\x1b\\\x1b[?69$p");
    assert_eq!(terminal.take_responses(), b"\x1bP0$r\x1b\\\x1b[?69;2$y");
}
