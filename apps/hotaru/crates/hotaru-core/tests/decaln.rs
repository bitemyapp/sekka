use hotaru_core::{Color, HistoryLimits, Rgb, RowPrompt, SemanticContent, Style, Terminal};

#[test]
fn alignment_replaces_owned_content_but_preserves_pen_colors_and_cursor_attributes() {
    let mut terminal = Terminal::with_history_limits(
        8,
        3,
        HistoryLimits {
            rows: 64,
            bytes: None,
        },
    );
    for _ in 0..12 {
        terminal.feed(b"old\r\n");
    }
    terminal.feed("\x1b[?2027h\x1b]133;P\x07\x1b[1;3;4:3;38;2;1;2;3;48;2;4;5;6;58;2;7;8;9m\x1b[1\"q\x1b]8;;https://example.org/retained\x1b\\👨\u{200d}👩\u{200d}👧".as_bytes());
    let held = terminal.snapshot_all();
    let history = held.position.history_rows;
    let family = held
        .lines
        .iter()
        .find(|row| row.grapheme(0).is_some())
        .unwrap()
        .clone();
    let old_cluster = family.grapheme(0).unwrap().to_owned();
    terminal.feed(b"\x1b#8");
    let aligned = terminal.snapshot_all();
    assert_eq!(aligned.lines.len(), held.lines.len());
    for (old, new) in held.lines[..history].iter().zip(&aligned.lines[..history]) {
        assert_eq!(old.cells(), new.cells());
        assert_eq!(old.semantic_prompt(), new.semantic_prompt());
    }
    let expected = Style {
        foreground: Color::Rgb(Rgb(1, 2, 3)),
        background: Color::Rgb(Rgb(4, 5, 6)),
        ..Style::default()
    };
    for row in &aligned.lines[history..] {
        assert_eq!(row.semantic_prompt(), RowPrompt::None);
        assert!(!row.wrapped());
        for (col, cell) in row.cells().iter().copied().enumerate() {
            assert_eq!(cell.character(), 'E');
            assert_eq!(cell.width(), 1);
            assert!(!cell.is_protected());
            assert_eq!(cell.semantic_content(), SemanticContent::Output);
            assert_eq!(row.style(cell), expected);
            assert!(row.grapheme(col).is_none());
            assert!(row.hyperlink(col).is_none());
        }
    }
    assert_eq!(
        (
            terminal.snapshot().cursor.row,
            terminal.snapshot().cursor.col
        ),
        (0, 0)
    );
    assert_eq!(family.grapheme(0), Some(old_cluster.as_str()));
    assert!(family.hyperlink(0).is_some());
    // The pattern clears cell attributes, not the current protected/link/
    // semantic cursor state. The SGR pen keeps only foreground/background.
    terminal.feed(b"X");
    let after = terminal.snapshot();
    let row = &after.lines[0];
    let cell = row.cells()[0];
    assert_eq!(cell.character(), 'X');
    assert!(cell.is_protected());
    assert_eq!(cell.semantic_content(), SemanticContent::Prompt);
    assert_eq!(row.style(cell), expected);
    assert!(row.hyperlink(0).is_some());
}

#[test]
fn alignment_resets_origin_margins_and_pending_wrap_but_not_rep() {
    let mut terminal = Terminal::new(3, 3, 0);
    terminal.feed(b"\x1b[44m\x1b[2;3r\x1b[?6h\x1b[?25lAAA\x1b#8\x1b[?6$p\x1b[3b");
    assert_eq!(terminal.take_responses(), b"\x1b[?6;2$y");
    let state = terminal.snapshot();
    assert_eq!(state.plain_text(), "AAA\nEEE\nEEE");
    assert!(!state.cursor.visible);
    terminal.feed(b"\x1b[T");
    let state = terminal.snapshot();
    assert_eq!(state.plain_text(), "\nAAA\nEEE");
    assert!(state.lines.iter().all(|row| {
        row.cells()
            .iter()
            .all(|cell| row.style(*cell).background == Color::Indexed(4))
    }));
}

#[test]
fn alignment_is_local_to_active_screen_and_exact_escape_sequence() {
    let mut terminal = Terminal::new(5, 3, 20);
    terminal.feed(b"main\x1b[?1049halt\x1b#8");
    assert_eq!(terminal.snapshot().plain_text(), "EEEEE\nEEEEE\nEEEEE");
    terminal.feed(b"\x1b[?1049l");
    assert_eq!(terminal.snapshot().plain_text(), "main\n\n");
    terminal.feed(b"\x1b##8");
    assert_eq!(terminal.snapshot().plain_text(), "main\n\n");
}
