use hotaru_core::{Snapshot, Terminal};

fn same(left: &Snapshot, right: &Snapshot) {
    assert_eq!(
        (left.cols, left.rows, left.cursor),
        (right.cols, right.rows, right.cursor)
    );
    for (a, b) in left.lines.iter().zip(&right.lines) {
        assert_eq!(a.wrapped(), b.wrapped());
        for (col, (&x, &y)) in a.cells().iter().zip(b.cells()).enumerate() {
            assert_eq!(
                (
                    x.character(),
                    x.width(),
                    x.is_protected(),
                    a.style(x),
                    a.grapheme(col)
                ),
                (
                    y.character(),
                    y.width(),
                    y.is_protected(),
                    b.style(y),
                    b.grapheme(col)
                )
            );
        }
    }
}
fn compress(terminal: &mut Terminal) {
    let mut steps = 0;
    while terminal.compress_history_step() {
        steps += 1;
        assert!(steps < 10_000);
    }
}
#[test]
fn compressed_history_matches_resident_terminal_through_scroll_resize_and_clear() {
    let mut resident = Terminal::new(40, 12, 500);
    let mut cold = Terminal::new(40, 12, 500);
    for index in 0..1200 {
        let line = format!(
            "\x1b[38;2;{};17;81m{index:05} 雪 e\u{301} \x1b[4:3mcurly\x1b[0m\r\n",
            index % 255
        );
        resident.feed(line.as_bytes());
        cold.feed(line.as_bytes());
    }
    let held = cold.snapshot_all();
    compress(&mut cold);
    assert!(cold.history_memory().compressed_rows > 300);
    same(&resident.snapshot_all(), &cold.snapshot_all());
    same(&held, &cold.snapshot_all());
    for offset in [0, 1, 11, 63, 64, 97, 499, 500, usize::MAX] {
        same(
            &resident.snapshot_scrolled(offset),
            &cold.snapshot_scrolled(offset),
        );
    }
    for (cols, rows) in [(13, 7), (80, 24), (1, 1), (3, 9), (40, 12)] {
        resident.resize(cols, rows);
        cold.resize(cols, rows);
        same(&resident.snapshot_all(), &cold.snapshot_all());
        compress(&mut cold);
        for _ in 0..81 {
            resident.feed(b"new\r\n");
            cold.feed(b"new\r\n");
        }
        same(&resident.snapshot_all(), &cold.snapshot_all());
    }
    resident.feed(b"\x1b[3J");
    cold.feed(b"\x1b[3J");
    assert_eq!(cold.history_memory().rows, 0);
    same(&resident.snapshot_all(), &cold.snapshot_all());
    drop(cold);
    assert!(!held.plain_text().is_empty());
}

#[test]
fn compressed_eviction_preserves_recycled_row_resize_semantics() {
    let mut resident = Terminal::new(8, 3, 128);
    let mut cold = Terminal::new(8, 3, 128);
    for _ in 0..132 {
        resident.feed(b"abcdefghij\r\n");
        cold.feed(b"abcdefghij\r\n");
    }
    for terminal in [&mut resident, &mut cold] {
        terminal.feed(b"\x1b[?7l");
        terminal.resize(4, 3);
    }
    compress(&mut cold);
    assert!(cold.history_memory().compressed_rows > 0);
    for terminal in [&mut resident, &mut cold] {
        terminal.feed(b"\x1b[?7h");
        for _ in 0..26 {
            terminal.feed(b"abcdef\r\n");
        }
        terminal.feed(b"\x1b[?7l");
        terminal.resize(8, 3);
    }
    same(&resident.snapshot_all(), &cold.snapshot_all());
}
