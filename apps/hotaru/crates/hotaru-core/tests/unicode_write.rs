//! Bulk Unicode runs must match byte-at-a-time scalar printing.
use hotaru_core::formatter::{CopyFormat, FormatOptions, format_copy};
use hotaru_core::{Snapshot, Terminal};
use std::sync::atomic::AtomicBool;

fn compare(a: &Snapshot, b: &Snapshot, label: &str) {
    assert_eq!(
        (a.cols, a.rows, a.cursor, a.modes),
        (b.cols, b.rows, b.cursor, b.modes),
        "{label}"
    );
    for (row, (a, b)) in a.lines.iter().zip(&b.lines).enumerate() {
        assert_eq!(a.wrapped(), b.wrapped(), "{label} row{row} wrap");
        for col in 0..a.cells().len() {
            let x = a.cells()[col];
            let y = b.cells()[col];
            assert_eq!(
                (x.character(), x.width(), x.is_protected()),
                (y.character(), y.width(), y.is_protected()),
                "{label} row{row} col{col}"
            );
            assert_eq!(a.style(x), b.style(y), "{label} row{row} col{col} style");
            assert_eq!(
                a.grapheme(col),
                b.grapheme(col),
                "{label} row{row} col{col} grapheme"
            );
            assert_eq!(
                a.hyperlink(col),
                b.hyperlink(col),
                "{label} row{row} col{col} link"
            );
        }
    }
}

#[test]
fn unicode_runs_match_scalar_printing_through_edits_wraps_metadata_and_resize() {
    let mut seed = 0x61207e81u64;
    let mut retained = Vec::new();
    for (initial_cols, initial_rows) in
        [(1, 1), (2, 3), (7, 4), (63, 3), (64, 5), (65, 3), (257, 4)]
    {
        let mut bulk = Terminal::new(initial_cols, initial_rows, 32);
        let mut scalar = Terminal::new(initial_cols, initial_rows, 32);
        for step in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let data = match seed % 16 {
                0..=3 => format!(
                    "{}e\u{301}{}\u{301}{}",
                    "雪界éab".repeat(27),
                    "雪".repeat(67),
                    "👩🏽‍💻🇯🇵".repeat(3)
                )
                .into_bytes(),
                4 => {
                    format!("\x1b[{};{}H", 1 + (seed >> 8) % 5, 1 + (seed >> 16) % 260).into_bytes()
                }
                5 => format!(
                    "\x1b[{}{}",
                    1 + (seed >> 8) % 7,
                    ["@", "P", "X", "K"][(seed >> 24) as usize % 4]
                )
                .into_bytes(),
                6 => b"\x1b[1;4:3;53;38;2;10;20;30;48;2;40;50;60m".to_vec(),
                7 => b"\xff\xe0\x80\xed\xa0\x80\x1b[0m\xc3".to_vec(),
                8 => format!("\x1b]8;id={};https://example.test/雪\x1b\\", seed % 13).into_bytes(),
                9 => b"\x1b]8;;\x1b\\".to_vec(),
                10 => {
                    if seed & (1 << 17) == 0 {
                        b"\x1b[?7h".to_vec()
                    } else {
                        b"\x1b[?7l".to_vec()
                    }
                }
                11 => {
                    if seed & (1 << 17) == 0 {
                        b"\x1b[4h".to_vec()
                    } else {
                        b"\x1b[4l".to_vec()
                    }
                }
                12 => b"\x1b(0qx\x1b(B".to_vec(),
                13 => b"\r\n\r\n".to_vec(),
                14 => b"\x1b7\x1b[2;2H\x1b[1\"q\x1b8".to_vec(),
                _ => "\u{301}\u{200d}\u{fe0f}".repeat(19).into_bytes(),
            };
            bulk.feed(&data);
            for byte in data {
                scalar.feed(&[byte]);
            }
            if step % 47 == 0 {
                let cols = [1, 2, 7, 63, 64, 65, 257][(seed >> 24) as usize % 7];
                let rows = 1 + (seed >> 32) as usize % 5;
                bulk.resize(cols, rows);
                scalar.resize(cols, rows);
            }
            let a = bulk.snapshot_all();
            let b = scalar.snapshot_all();
            compare(&a, &b, &format!("initial{initial_cols} step{step}"));
            assert_eq!(bulk.take_responses(), scalar.take_responses());
            if step % 31 == 0 {
                for format in [CopyFormat::Mixed, CopyFormat::Vt] {
                    let options = FormatOptions {
                        format,
                        ..FormatOptions::default()
                    };
                    let cancel = AtomicBool::new(false);
                    let x = format_copy(&a.clone().into(), &options, &cancel).unwrap();
                    let y = format_copy(&b.clone().into(), &options, &cancel).unwrap();
                    assert_eq!((x.plain, x.html), (y.plain, y.html));
                }
            }
            if step % 83 == 0 {
                retained.push((a, b));
            }
        }
    }
    for (a, b) in retained {
        compare(&a, &b, "retained after terminal destruction");
    }
}
