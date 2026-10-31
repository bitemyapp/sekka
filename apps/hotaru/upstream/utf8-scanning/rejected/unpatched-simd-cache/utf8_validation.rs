use hotaru_core::{GraphemeWidthMethod, Terminal};

#[test]
fn streamed_invalid_utf8_matches_bytewise_parser() {
    // Include valid 2/3/4-byte sequences beside malformed leads, overlong forms,
    // surrogate encodings, C1, truncated sequences, and VT control boundaries.
    let fragments: &[&[u8]] = &[
        b"text",
        "é雪👩".as_bytes(),
        b"\xc2",
        b"\xc2\x80",
        b"\xc0\xaf",
        b"\xe0\x80\xaf",
        b"\xed\xa0\x80",
        b"\xf4\x90\x80\x80",
        b"\xf0\x9f",
        b"\xff\xfe\x80",
        b"\x1b[31m",
        b"\r\n",
        b"\x7f",
        "e\u{301}👩🏽‍💻".as_bytes(),
        b"\x1b[2K",
        b"\x1b[3;5H",
        b"\t",
    ];
    for mode in [GraphemeWidthMethod::Legacy, GraphemeWidthMethod::Unicode] {
        let mut seed = 0x73d2_185du32;
        for case in 0..512 {
            let mut input = Vec::new();
            for _ in 0..32 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                input.extend_from_slice(fragments[seed as usize % fragments.len()]);
            }
            let mut scalar = Terminal::new(19, 4, 128);
            scalar.set_default_grapheme_width(mode);
            for byte in &input {
                scalar.feed(&[*byte]);
            }
            let expected = scalar.snapshot_all();
            for chunk in [3, 7, 16, 64, input.len()] {
                let mut bulk = Terminal::new(19, 4, 128);
                bulk.set_default_grapheme_width(mode);
                for bytes in input.chunks(chunk) {
                    bulk.feed(bytes);
                }
                let actual = bulk.snapshot_all();
                assert_eq!(actual.cursor, expected.cursor, "case={case} chunk={chunk}");
                assert_eq!(actual.modes, expected.modes, "case={case} chunk={chunk}");
                assert_eq!(actual.lines.len(), expected.lines.len());
                for (actual, expected) in actual.lines.iter().zip(&expected.lines) {
                    assert_eq!(actual.wrapped(), expected.wrapped());
                    for col in 0..19 {
                        assert_eq!(
                            actual.cells()[col].character(),
                            expected.cells()[col].character(),
                            "case={case} chunk={chunk} col={col}"
                        );
                        assert_eq!(actual.cells()[col].width(), expected.cells()[col].width());
                        assert_eq!(
                            actual.style(actual.cells()[col]),
                            expected.style(expected.cells()[col])
                        );
                        assert_eq!(actual.grapheme(col), expected.grapheme(col));
                    }
                }
            }
        }
    }
}

#[test]
fn compat_validation_preserves_standard_error_boundaries() {
    fn compare(input: &[u8]) {
        let standard = std::str::from_utf8(input).map_err(|e| (e.valid_up_to(), e.error_len()));
        let simd = simdutf8::compat::from_utf8(input).map_err(|e| (e.valid_up_to(), e.error_len()));
        assert_eq!(
            simd,
            standard,
            "input prefix: {:?}",
            &input[..input.len().min(32)]
        );
    }
    compare(&[]);
    for first in 0..=255u8 {
        compare(&[first]);
        for second in 0..=255u8 {
            compare(&[first, second]);
            for third in 0..=255u8 {
                compare(&[first, second, third]);
            }
        }
    }
    let valid = "ASCII é 雪 👩🏽‍💻 e\u{301}".as_bytes();
    let mut seed = 0x94c3_b16du32;
    for case in 0..2048 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let len = if case % 8 == 0 {
            65536
        } else {
            64 + seed as usize % 4096
        };
        let offset = case % 32;
        let mut bytes = vec![0; offset + len];
        for (at, value) in bytes[offset..].iter_mut().enumerate() {
            *value = valid[at % valid.len()];
        }
        // Every prefix cut exercises an incomplete sequence at some boundary;
        // sparse and dense mutations also check early failure in long runs.
        compare(&bytes[offset..]);
        for mutation in 0..16 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = offset + seed as usize % len;
            bytes[at] = if mutation % 2 == 0 {
                0xff
            } else {
                (seed >> 24) as u8
            };
            compare(&bytes[offset..]);
        }
    }
}

#[test]
fn dense_malformed_utf8_keeps_control_and_chunk_boundaries() {
    for pattern in [
        b"\xff".as_slice(),
        b"\xc2X",
        b"\xe0\x80",
        b"\xf0\x9fX",
        b"\xf4\x90\x80\x80",
    ] {
        let mut data = pattern.repeat(65536 / pattern.len());
        data.extend_from_slice(b"\x1b[31mEND\r\n");
        data.extend_from_slice("雪花👩🏽‍💻".as_bytes());
        for mode in [GraphemeWidthMethod::Legacy, GraphemeWidthMethod::Unicode] {
            let mut whole = Terminal::new(120, 12, 0);
            let mut split = Terminal::new(120, 12, 0);
            whole.set_default_grapheme_width(mode);
            split.set_default_grapheme_width(mode);
            whole.feed(&data);
            for bytes in data.chunks(17) {
                split.feed(bytes);
            }
            let whole = whole.snapshot();
            let split = split.snapshot();
            assert_eq!(whole.cursor, split.cursor);
            assert_eq!(whole.plain_text(), split.plain_text());
            for (whole, split) in whole.lines.iter().zip(&split.lines) {
                for col in 0..120 {
                    assert_eq!(whole.cells()[col], split.cells()[col]);
                    assert_eq!(
                        whole.style(whole.cells()[col]),
                        split.style(split.cells()[col])
                    );
                    assert_eq!(whole.grapheme(col), split.grapheme(col));
                }
            }
        }
    }
}

#[test]
fn malformed_utf8_cache_survives_partial_parser_and_bounded_replies() {
    let fragment = b"\xf0\x9fX\xff\xc2text\x1b[6n\xe0\x80\xaf\x1b[31m\r\n";
    let mut input = fragment.repeat(64);
    input.extend_from_slice("👩🏽‍💻END".as_bytes());
    for mode in [GraphemeWidthMethod::Legacy, GraphemeWidthMethod::Unicode] {
        let mut expected = Terminal::new(29, 5, 0);
        expected.set_default_grapheme_width(mode);
        for byte in &input {
            expected.feed(&[*byte]);
        }
        let replies = expected.take_responses();
        let expected = expected.snapshot();
        for split in 0..fragment.len() {
            for budget in [1, 7, 64] {
                let mut terminal = Terminal::new(29, 5, 0);
                terminal.set_default_grapheme_width(mode);
                terminal.feed(&input[..split]);
                let mut actual_replies = terminal.take_responses();
                let mut at = split;
                while at < input.len() {
                    let consumed = terminal.feed_bounded(&input[at..], budget);
                    assert!(consumed > 0);
                    at += consumed;
                    actual_replies.extend(terminal.take_responses());
                }
                assert_eq!(actual_replies, replies, "split={split} budget={budget}");
                let actual = terminal.snapshot();
                assert_eq!(actual.cursor, expected.cursor);
                assert_eq!(actual.modes, expected.modes);
                assert_eq!(actual.plain_text(), expected.plain_text());
                for (actual, expected) in actual.lines.iter().zip(&expected.lines) {
                    for col in 0..29 {
                        assert_eq!(actual.cells()[col], expected.cells()[col]);
                        assert_eq!(
                            actual.style(actual.cells()[col]),
                            expected.style(expected.cells()[col])
                        );
                        assert_eq!(actual.grapheme(col), expected.grapheme(col));
                    }
                }
            }
        }
    }
}
