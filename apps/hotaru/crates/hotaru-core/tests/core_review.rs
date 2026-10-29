use hotaru_core::{Terminal, TerminalEvent};

#[test]
fn restored_cursor_above_origin_margin_reports_row_one_without_underflow() {
    // Confirmed against Ghostty 6ba144158577d033ccb6ed4b574e4013957c9161:
    // the absolute cursor remains row 0, and origin-relative CPR is 1;1.
    let bytes = b"\x1b[?6h\x1b7\x1b[5;10r\x1b8\x1b[6n";
    for chunk in 1..=bytes.len() {
        let mut terminal = Terminal::new(80, 24, 0);
        for part in bytes.chunks(chunk) {
            terminal.feed(part);
        }
        let snapshot = terminal.snapshot();
        assert_eq!((snapshot.cursor.row, snapshot.cursor.col), (0, 0));
        assert_eq!(terminal.take_responses(), b"\x1b[1;1R", "chunk {chunk}");
    }
}

#[test]
fn oversized_dcs_header_discards_payload_through_string_terminator() {
    let mut bytes = b"\x1bP".to_vec();
    bytes.extend_from_slice(&[b';'; 32]);
    bytes.extend_from_slice(b"qPAYLOAD\x1b\\after");
    for chunk in 1..=bytes.len() {
        let mut terminal = Terminal::new(80, 24, 0);
        for part in bytes.chunks(chunk) {
            terminal.feed(part);
        }
        let snapshot = terminal.snapshot();
        assert_eq!(snapshot.lines[0].cells()[0].character(), 'a');
        assert_eq!(
            snapshot.cursor.col, 5,
            "chunk {chunk}: payload leaked to screen"
        );
        assert!(terminal.take_responses().is_empty());
    }
}

#[test]
fn invalid_dcs_private_marker_and_excess_intermediates_do_not_leak_payload() {
    for bytes in [
        &b"\x1bP1?qPAYLOAD\x1b\\x"[..],
        &b"\x1bP$%&qPAYLOAD\x1b\\x"[..],
    ] {
        let mut terminal = Terminal::new(80, 24, 0);
        terminal.feed(bytes);
        let snapshot = terminal.snapshot();
        assert_eq!(snapshot.lines[0].cells()[0].character(), 'x');
        assert_eq!(snapshot.cursor.col, 1);
    }
}

#[test]
fn oversized_csi_still_finishes_at_header_final_byte() {
    let mut bytes = b"\x1b[".to_vec();
    bytes.extend_from_slice(&[b';'; 32]);
    bytes.extend_from_slice(b"mvisible");
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.feed(&bytes);
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.lines[0].cells()[0].character(), 'v');
    assert_eq!(snapshot.cursor.col, 7);
}

#[test]
fn ordinary_osc_exact_body_limit_is_accepted_and_one_more_byte_discards_all() {
    for body_size in [2048, 2049] {
        let mut terminal = Terminal::new(80, 24, 0);
        let mut bytes = b"\x1b]2;".to_vec();
        bytes.extend(std::iter::repeat_n(b't', body_size));
        bytes.extend_from_slice(b"\x1b\\x");
        for chunk in bytes.chunks(17) {
            terminal.feed(chunk);
        }
        let events = terminal.take_events();
        if body_size == 2048 {
            assert!(matches!(&events[..], [TerminalEvent::Title(title)] if title.len() == 2048));
        } else {
            assert!(
                events.is_empty(),
                "an overflow must not dispatch its valid prefix"
            );
        }
        assert_eq!(terminal.snapshot().lines[0].cells()[0].character(), 'x');
    }
}

#[test]
fn osc_palette_burst_is_bounded_and_overflow_emits_no_prefix_replies() {
    let response = b"\x1b]4;0;rgb:0000/0000/0000\x07";
    assert_eq!(response.len(), 25);
    for extra in [0, 1] {
        let mut terminal = Terminal::new(80, 24, 0);
        let mut bytes = b"\x1b]4;".to_vec();
        bytes.extend_from_slice(&b"0;?;".repeat(512));
        bytes.extend(std::iter::repeat_n(b';', extra));
        bytes.push(7);
        // A complete OSC dispatch is atomic even with a one-byte threshold,
        // but the capture limit bounds its amplification to 12,800 bytes.
        assert_eq!(terminal.feed_bounded(&bytes, 1), bytes.len());
        let replies = terminal.take_responses();
        if extra == 0 {
            assert_eq!(replies, response.repeat(512));
            assert_eq!(replies.len(), 12_800);
        } else {
            assert!(replies.is_empty());
        }
    }
}

#[test]
fn osc_palette_stops_at_invalid_pair_and_ignores_empty_fields() {
    let response = b"\x1b]4;0;rgb:0000/0000/0000\x07";
    for body in ["0;?;BAD;?;1;?", "0;?;1;INVALID;2;?"] {
        let mut terminal = Terminal::new(80, 24, 0);
        terminal.feed(format!("\x1b]4;{body}\x07").as_bytes());
        assert_eq!(terminal.take_responses(), response);
    }
    let mut terminal = Terminal::new(80, 24, 0);
    terminal.feed(b"\x1b]4;0;?;;0;?\x07");
    assert_eq!(terminal.take_responses(), response.repeat(2));
}

#[test]
fn clipboard_osc_cap_includes_selector_delimiter_and_reference_sentinel() {
    const LIMIT: usize = 8 * 1024 * 1024;
    for selector in [b"c;".as_slice(), b";"] {
        for extra in [0, 1] {
            let mut terminal = Terminal::new(80, 24, 0);
            let length = LIMIT - selector.len() - 1 + extra;
            let mut bytes = b"\x1b]52;".to_vec();
            bytes.extend_from_slice(selector);
            bytes.extend(std::iter::repeat_n(b'A', length));
            bytes.extend_from_slice(b"\x07x");
            for chunk in bytes.chunks(65536) {
                terminal.feed(chunk);
            }
            let events = terminal.take_events();
            if extra == 0 {
                assert!(
                    matches!(&events[..], [TerminalEvent::Clipboard { selection: b'c', data }]
                    if data.len() == length)
                );
            } else {
                assert!(events.is_empty());
            }
            assert_eq!(terminal.snapshot().lines[0].cells()[0].character(), 'x');
        }
    }
}

#[test]
fn malformed_byte_streams_and_control_mutations_preserve_screen_invariants() {
    let mut terminal = Terminal::new(19, 11, 7);
    let mut seed = 0x91ba_347d_u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for step in 0..10_000 {
        match next() % 6 {
            0 => {
                let bytes: Vec<_> = (0..32).map(|_| next() as u8).collect();
                terminal.feed(&bytes);
            }
            1 => {
                let private = if next() & 1 == 0 { "?" } else { "" };
                let finals = b"ABCDEFGHJKLMPRSTX@`adefghlmnrstu";
                let final_byte = finals[next() as usize % finals.len()] as char;
                terminal.feed(
                    format!(
                        "\x1b[{private}{};{}{final_byte}",
                        next() % 65536,
                        next() % 65536
                    )
                    .as_bytes(),
                );
            }
            2 => terminal.feed(b"\x1b[?6h\x1b7\x1b[2;5r\x1b8\x1b[6n\x1b[?6l"),
            3 => {
                terminal.feed(b"\x1bP1;2:3$%&q\xffmalformed\x1b\\");
                terminal.feed("e\u{301}猫\u{fe0f}".as_bytes());
            }
            4 => terminal.feed(b"\x1b]4;;0;?;BAD;?\x07\x1b[?1049h\x1b[?1049l"),
            _ => terminal.resize(next() as usize % 41, next() as usize % 17),
        }
        if step % 64 == 0 {
            // Leave unterminated random strings often enough to exercise the
            // control grammar instead of merely feeding one long OSC payload.
            terminal.feed(&[0x18]);
        }
        terminal.take_responses();
        terminal.take_events();
        let snapshot = terminal.snapshot();
        assert_eq!(snapshot.rows, snapshot.lines.len());
        assert!(snapshot.cursor.row < snapshot.rows && snapshot.cursor.col < snapshot.cols);
        for row in &snapshot.lines {
            assert_eq!(row.cells().len(), snapshot.cols);
            for (col, &cell) in row.cells().iter().enumerate() {
                let _ = row.style(cell);
                match cell.width() {
                    0 => assert!(col > 0 && row.cells()[col - 1].width() == 2),
                    1 => {}
                    2 => assert!(col + 1 < snapshot.cols && row.cells()[col + 1].width() == 0),
                    width => panic!("invalid cell width {width} after mutation {step}"),
                }
            }
        }
    }
}
