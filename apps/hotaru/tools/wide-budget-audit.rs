//! Public-API diagnostic for scalar/bulk overwrite allocation equivalence.
use hotaru_core::{HistoryLimits, Terminal};

fn main() {
    let seed = "雪雪\x1b[31;44mA\x1b[32mB\x1b[33mC\r\x1b[31m";
    let mut expected = None;
    for chunk in [1, 7, 65536] {
        let mut terminal = Terminal::with_history_limits(8, 3, HistoryLimits { rows: 1000, bytes: None });
        terminal.feed(b"old\r\n".repeat(80).as_slice());
        terminal.feed(seed.as_bytes());
        let initial = terminal.history_memory();
        for part in b"abcdefgh".chunks(chunk) {
            terminal.feed(part);
        }
        let after = terminal.history_memory();
        println!("chunk={chunk} initial_active={} final_active={} history={} text={:?}", initial.active_bytes, after.active_bytes, after.rows,terminal.snapshot().plain_text());
        if let Some(expected) = expected {
            assert_eq!(after.active_bytes,expected,"chunk-dependent ownership charge");
        } else {
            expected=Some(after.active_bytes);
        }
    }
}
