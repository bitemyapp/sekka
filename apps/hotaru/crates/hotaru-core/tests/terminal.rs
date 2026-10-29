use hotaru_core::{Color, Rgb, Snapshot, Terminal, TerminalEvent, attrs};

fn text(snapshot: &Snapshot, row: usize) -> String {
    let mut out = String::new();
    snapshot.lines[row].append_text(&mut out, 0, snapshot.cols, true);
    out
}
fn check_invariants(s: &Snapshot) {
    assert_eq!(s.lines.len(), s.rows);
    assert!(s.cursor.col < s.cols);
    assert!(s.cursor.row < s.rows);
    for row in &s.lines {
        assert_eq!(row.cells().len(), s.cols);
        for (col, &cell) in row.cells().iter().enumerate() {
            let _ = row.style(cell);
            match cell.width() {
                0 => assert!(col > 0 && row.cells()[col - 1].width() == 2),
                1 => {}
                2 => assert!(col + 1 < s.cols && row.cells()[col + 1].width() == 0),
                _ => panic!("invalid width"),
            }
        }
    }
}

#[test]
fn ascii_wrap_is_delayed_and_copy_joins_soft_lines() {
    let mut t = Terminal::new(5, 3, 10);
    t.feed(b"abcde");
    let s = t.snapshot();
    assert_eq!((s.cursor.row, s.cursor.col), (0, 4));
    assert!(!s.lines[0].wrapped());
    t.feed(b"f\r\nz");
    let s = t.snapshot();
    assert!(s.lines[0].wrapped());
    assert_eq!(s.plain_text(), "abcdef\nz");
    check_invariants(&s);
}

#[test]
fn copy_snapshot_owns_rows_styles_graphemes_and_palette() {
    let mut t = Terminal::new(8, 2, 2);
    t.feed("\x1b[31me\u{301}猫".as_bytes());
    let before = t.snapshot();
    let expected = before.plain_text();
    t.feed(b"\x1b]4;1;#123456\x07\x1b[0m\x1b[2Jdifferent\r\nmore\r\ntext");
    t.resize(1, 1);
    drop(t);
    assert_eq!(before.plain_text(), expected);
    assert_eq!(before.lines[0].grapheme(0), Some("e\u{301}"));
    assert_eq!(
        before.lines[0].style(before.lines[0].cells()[0]).foreground,
        Color::Indexed(1)
    );
    assert_eq!(before.palette[1], Rgb(205, 0, 0));
    check_invariants(&before);
}

#[test]
fn utf8_and_escape_sequences_ignore_read_boundaries() {
    let bytes =
        "one\r\n\x1b[1;38:2::12:34:56m蛍 e\u{301}\x1b[0m\x1b]2;Hotaru\x1b\\\x1b[6n".as_bytes();
    let mut whole = Terminal::new(12, 4, 8);
    whole.feed(bytes);
    let expected = whole.snapshot();
    let response = whole.take_responses();
    let events = whole.take_events();
    for size in 1..=bytes.len() {
        let mut split = Terminal::new(12, 4, 8);
        for part in bytes.chunks(size) {
            split.feed(part);
        }
        let actual = split.snapshot();
        assert_eq!(actual.plain_text(), expected.plain_text(), "chunk {size}");
        assert_eq!(actual.cursor, expected.cursor);
        assert_eq!(split.take_responses(), response);
        assert_eq!(split.take_events(), events);
        for (a, b) in actual.lines.iter().zip(&expected.lines) {
            for (&ca, &cb) in a.cells().iter().zip(b.cells()) {
                assert_eq!(a.style(ca), b.style(cb));
                assert_eq!(ca.width(), cb.width());
            }
        }
    }
}

#[test]
fn wide_glyph_boundaries_remain_valid_after_editing() {
    for edit in [
        "\x1b[2G\x1b[P",
        "\x1b[2G\x1b[@",
        "\x1b[2G\x1b[X",
        "\x1b[2Gx",
        "\x1b[1G\x1b[2P",
    ] {
        let mut t = Terminal::new(7, 3, 2);
        t.feed("猫犬ab".as_bytes());
        t.feed(edit.as_bytes());
        check_invariants(&t.snapshot());
        for width in [1, 2, 3, 4, 8] {
            t.resize(width, 2);
            check_invariants(&t.snapshot());
        }
    }
}

#[test]
fn margin_scrolling_preserves_outside_rows() {
    let mut t = Terminal::new(8, 4, 10);
    t.feed(b"one\r\ntwo\r\nthree\r\nfour\x1b[2;3r\x1b[3;1H\n");
    let s = t.snapshot();
    assert_eq!(text(&s, 0), "one");
    assert_eq!(text(&s, 1), "three");
    assert_eq!(text(&s, 2), "");
    assert_eq!(text(&s, 3), "four");
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn scrollback_is_bounded_and_viewport_snapshot_is_owned() {
    let mut t = Terminal::new(5, 2, 3);
    for n in 0..20 {
        t.feed(format!("{n}\r\n").as_bytes());
    }
    assert_eq!(t.scrollback_len(), 3);
    let s = t.snapshot_scrolled(usize::MAX);
    assert!(!s.cursor.visible);
    assert_eq!(text(&s, 0), "16");
    assert_eq!(text(&s, 1), "17");
    t.feed(b"\x1b[3J");
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(text(&s, 0), "16");
}

#[test]
fn alternate_screen_restores_primary_cursor_and_style() {
    let mut t = Terminal::new(8, 3, 3);
    t.feed(b"\x1b[32mhello\x1b[?1049hworld\x1b[?1049l!");
    let s = t.snapshot();
    assert_eq!(text(&s, 0), "hello!");
    assert!(!s.modes.alternate_screen);
    assert_eq!(
        s.lines[0].style(s.lines[0].cells()[5]).foreground,
        Color::Indexed(2)
    );
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn sgr_semicolon_and_colon_truecolor_and_reset() {
    let mut t = Terminal::new(8, 1, 0);
    t.feed(b"\x1b[1;38;2;12;34;56mA\x1b[38:2::90:80:70mB\x1b[4:0;22;39mC");
    let s = t.snapshot();
    let row = &s.lines[0];
    let a = row.style(row.cells()[0]);
    let b = row.style(row.cells()[1]);
    let c = row.style(row.cells()[2]);
    assert_eq!(a.foreground, Color::Rgb(Rgb(12, 34, 56)));
    assert_eq!(a.attrs, attrs::BOLD);
    assert_eq!(b.foreground, Color::Rgb(Rgb(90, 80, 70)));
    assert_eq!(c.foreground, Color::Default);
    assert_eq!(c.attrs, 0);
}

#[test]
fn style_churn_does_not_corrupt_live_cells() {
    let mut t = Terminal::new(5, 2, 0);
    t.feed(b"\x1b[31ma");
    for n in 0..70000 {
        t.feed(
            format!(
                "\x1b[2G\x1b[38;2;{};{};{}mb",
                n % 256,
                (n / 256) % 256,
                (n / 65536) % 256
            )
            .as_bytes(),
        );
    }
    let s = t.snapshot();
    assert_eq!(text(&s, 0), "ab");
    assert_eq!(
        s.lines[0].style(s.lines[0].cells()[0]).foreground,
        Color::Indexed(1)
    );
    check_invariants(&s);
}

#[test]
fn malformed_utf8_does_not_swallow_controls_or_ascii() {
    let mut t = Terminal::new(12, 2, 0);
    t.feed(b"\xe2A\xf0\x80\x80\x80\x1b[2;1Hx");
    assert_eq!(
        text(&t.snapshot(), 0),
        "\u{fffd}A\u{fffd}\u{fffd}\u{fffd}\u{fffd}"
    );
    assert_eq!(text(&t.snapshot(), 1), "x");
}

#[test]
fn cancelled_and_oversized_csi_do_not_dispatch() {
    let mut t = Terminal::new(8, 2, 0);
    t.feed(b"\x1b[31\x18m");
    let oversized = format!("\x1b[{}31mX", "1;".repeat(32));
    t.feed(oversized.as_bytes());
    let s = t.snapshot();
    assert_eq!(text(&s, 0), "mX");
    assert_eq!(
        s.lines[0].style(s.lines[0].cells()[1]).foreground,
        Color::Default
    );
}

#[test]
fn osc_clipboard_is_event_only_and_title_is_bounded() {
    let mut t = Terminal::new(5, 2, 0);
    t.feed(b"\x1b]52;c;aGVsbG8=\x07\x1b]2;Hotaru\x1b\\");
    assert_eq!(
        t.take_events(),
        vec![
            TerminalEvent::Clipboard {
                selection: b'c',
                data: b"aGVsbG8=".to_vec()
            },
            TerminalEvent::Title("Hotaru".into())
        ]
    );
    assert!(t.take_responses().is_empty());
}

#[test]
fn large_copy_after_terminal_teardown() {
    let mut t = Terminal::new(1024, 2048, 0);
    let line = vec![b'x'; 1024];
    for _ in 0..2048 {
        t.feed(&line);
    }
    let snapshot = t.snapshot();
    drop(t);
    let copy = std::thread::spawn(move || snapshot.plain_text())
        .join()
        .unwrap();
    assert_eq!(copy.len(), 2 * 1024 * 1024);
    assert!(copy.bytes().all(|b| b == b'x'));
}

#[test]
fn randomized_edit_stream_never_breaks_cell_invariants() {
    let mut t = Terminal::new(17, 9, 11);
    let mut seed = 0x12345678u32;
    let actions: [&[u8]; 15] = [
        b"abc",
        "猫".as_bytes(),
        "e\u{301}".as_bytes(),
        b"\r\n",
        b"\x1b[2J",
        b"\x1b[2P",
        b"\x1b[3@",
        b"\x1b[8;12H",
        b"\x1b[2K",
        b"\x1b[2S",
        b"\x1b[3T",
        b"\x1b[?1049h",
        b"\x1b[?1049l",
        b"\x1b[1;31m",
        b"\x1b[0m",
    ];
    for i in 0..10000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        t.feed(actions[seed as usize % actions.len()]);
        if i % 37 == 0 {
            t.resize((seed as usize % 20) + 1, ((seed >> 8) as usize % 10) + 1);
        }
        check_invariants(&t.snapshot());
    }
}

#[test]
fn replies_apply_backpressure_without_silent_loss() {
    let mut t = Terminal::new(80, 24, 0);
    let input = b"\x1b[5n".repeat(20000);
    let mut at = 0;
    let mut replies = Vec::new();
    let mut pauses = 0;
    while at < input.len() {
        let consumed = t.feed_bounded(&input[at..], 4096);
        assert!(consumed > 0);
        at += consumed;
        assert!(t.pending_response_bytes() <= 4100);
        replies.extend(t.take_responses());
        pauses += 1;
    }
    assert!(pauses > 1);
    assert_eq!(replies, b"\x1b[0n".repeat(20000));
}

#[test]
fn bell_flood_keeps_latest_semantic_notifications() {
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.feed(&[7; 1024]);
    terminal.feed(b"\x1b]2;title\x07\x1b]52;c;aGk=\x07");
    let events = terminal.take_events();
    assert_eq!(events.len(), 3);
    assert!(events.contains(&TerminalEvent::Title("title".into())));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TerminalEvent::Clipboard { .. }))
    );
}

#[test]
fn clear_screen_at_a_marked_prompt_erases_everything_and_asks_for_a_redraw() {
    let mut t = Terminal::new(20, 4, 100);
    t.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n\x1b]133;A\x07$ \x1b]133;B\x07ls");
    assert!(t.scrollback_len() > 0);
    assert!(t.clear_screen());
    let s = t.snapshot();
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(s.plain_text().trim(), "");
    check_invariants(&s);
}

#[test]
fn clear_screen_while_a_program_runs_keeps_the_cursor_row_at_the_top() {
    let mut t = Terminal::new(20, 4, 100);
    t.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
    assert!(!t.clear_screen());
    let s = t.snapshot();
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!((s.cursor.row, s.cursor.col), (0, 3));
    assert_eq!(text(&s, 0).trim_end(), "six");
    assert_eq!(s.plain_text().trim(), "six");
    t.feed(b"\r\nseven");
    assert_eq!(text(&t.snapshot(), 1).trim_end(), "seven");
    check_invariants(&t.snapshot());
}

#[test]
fn clear_screen_leaves_the_alternate_screen_alone() {
    let mut t = Terminal::new(20, 4, 100);
    t.feed(b"shell\r\n\x1b[?1049hfull screen app");
    assert!(!t.clear_screen());
    assert!(t.snapshot().plain_text().contains("full screen app"));
}

#[test]
fn reset_discards_a_partial_escape_sequence_and_keeps_generation_moving() {
    let mut t = Terminal::new(20, 4, 100);
    t.feed(b"\x1b[31mred\x1b[?1049h\x1b]0;half a ti");
    let before = t.generation();
    t.reset();
    assert!(t.generation() > before);
    assert!(!t.modes().alternate_screen);
    t.feed(b"plain");
    let s = t.snapshot();
    assert_eq!(text(&s, 0).trim_end(), "plain");
    assert_eq!(
        s.lines[0].style(s.lines[0].cells()[0]).foreground,
        Color::Default
    );
}
