use hotaru_core::{ContentSnapshot, RowRef, Snapshot, Terminal};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn same_row(actual: &RowRef, expected: &RowRef) {
    assert_eq!(actual.cells(), expected.cells());
    assert_eq!(actual.wrapped(), expected.wrapped());
    for (col, cell) in actual.cells().iter().enumerate() {
        assert_eq!(actual.style(*cell), expected.style(expected.cells()[col]));
        assert_eq!(actual.grapheme(col), expected.grapheme(col));
        assert_eq!(actual.hyperlink(col), expected.hyperlink(col));
    }
}
fn same(actual: &ContentSnapshot, expected: &Snapshot) {
    assert_eq!(
        (actual.cols, actual.rows, actual.cursor),
        (expected.cols, expected.rows, expected.cursor)
    );
    assert_eq!(actual.palette, expected.palette);
    assert_eq!(actual.default_foreground, expected.default_foreground);
    assert_eq!(actual.default_background, expected.default_background);
    let mut rows = actual.rows();
    for row in &expected.lines {
        same_row(&rows.next().unwrap(), row);
    }
    assert!(rows.next().is_none());
}
fn compress(terminal: &mut Terminal) {
    let mut ticks = 0;
    while terminal.compress_history_step() {
        ticks += 1;
        assert!(ticks < 10_000);
    }
}
fn write_lines(terminal: &mut Terminal, count: usize) {
    for index in 0..count {
        let line = format!(
            "\x1b]8;id=shared;https://example.test/雪\x1b\\\x1b[38;2;{};72;91m{index:04} 雪 e\u{301} \x1b]8;;\x1b\\unlinked\x1b[0m\r\n",
            index % 255
        );
        terminal.feed(line.as_bytes());
    }
}

#[test]
fn content_captures_survive_compression_eviction_reflow_and_owner_teardown() {
    let mut terminal = Terminal::new(40, 8, 500);
    write_lines(&mut terminal, 700);
    let resident = terminal.content_snapshot();
    let expected = terminal.snapshot_all();
    assert_eq!(resident.history_rows, 500);
    compress(&mut terminal);
    assert!(terminal.history_memory().compressed_rows > 350);
    let cold = Arc::new(terminal.content_snapshot());
    let reader = {
        let cold = cold.clone();
        let expected = expected.clone();
        std::thread::spawn(move || {
            for _ in 0..8 {
                same(&cold, &expected);
            }
        })
    };
    for (cols, rows) in [(13, 3), (80, 12), (1, 2), (40, 8)] {
        terminal.resize(cols, rows);
        write_lines(&mut terminal, 531);
        compress(&mut terminal);
        same(&terminal.content_snapshot(), &terminal.snapshot_all());
    }
    terminal.feed(b"\x1b[?1049hAlternate screen\x1b[?1049l\x1b[3J");
    assert_eq!(terminal.history_memory().rows, 0);
    drop(terminal);
    reader.join().unwrap();
    same(&resident, &expected);
    same(&cold, &expected);
}

#[test]
fn ranges_and_cancellation_cover_block_and_screen_boundaries() {
    let mut terminal = Terminal::new(40, 8, 500);
    write_lines(&mut terminal, 700);
    compress(&mut terminal);
    for _ in 0..13 {
        terminal.feed(b"tail\r\n");
    }
    let expected = terminal.snapshot_all();
    let content = terminal.content_snapshot();
    for start in [
        0,
        1,
        31,
        63,
        64,
        65,
        111,
        480,
        499,
        500,
        501,
        507,
        508,
        usize::MAX,
    ] {
        for count in [0, 1, 2, 7, 63, 128, usize::MAX] {
            let end = start.saturating_add(count);
            let from = start.min(expected.rows);
            let to = end.min(expected.rows);
            let rows: Vec<_> = content.rows_range(start..end).collect();
            assert_eq!(rows.len(), to - from, "range {start}..{end}");
            for (actual, expected) in rows.iter().zip(&expected.lines[from..to]) {
                same_row(actual, expected);
            }
        }
    }
    assert_eq!(
        content
            .rows_range(std::ops::Range { start: 99, end: 1 })
            .count(),
        0
    );
    let cancel = AtomicBool::new(false);
    let mut rows = content.rows();
    for expected in &expected.lines[..65] {
        same_row(&rows.next_cancellable(&cancel).unwrap().unwrap(), expected);
    }
    cancel.store(true, Ordering::Release);
    assert!(rows.next_cancellable(&cancel).is_err());
    cancel.store(false, Ordering::Release);
    same_row(
        &rows.next_cancellable(&cancel).unwrap().unwrap(),
        &expected.lines[65],
    );
    for offset in [0, 1, 100, 500] {
        let viewport = terminal.snapshot_scrolled(offset);
        let content = ContentSnapshot::from(viewport.clone());
        assert_eq!(content.history_rows, 0);
        same(&content, &viewport);
    }
}
