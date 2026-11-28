//! Feed throughput against retained history: one terminal is fed the whole
//! input in fixed slices, and each slice's feed time is reported with the
//! history it ended with. Usage:
//! `history_scaling FILE COLS ROWS HISTORY_ROWS none|default|BYTES [SLICE_BYTES]`
use hotaru_core::{Cols, DEFAULT_HISTORY_BYTES, GridSize, HistoryLimits, Rows, Terminal};
use serde_json::json;
use std::time::Instant;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!(
            "usage: history_scaling FILE COLS ROWS HISTORY_ROWS none|default|BYTES [SLICE_BYTES]"
        );
        std::process::exit(2);
    }
    let data = std::fs::read(&args[1]).unwrap();
    let cols = Cols::new(args[2].parse().unwrap()).expect("COLS must be from 1 to MAX_COLS");
    let rows = Rows::new(args[3].parse().unwrap()).expect("ROWS must be from 1 to MAX_ROWS");
    let history: usize = args[4].parse().unwrap();
    let bytes = match args[5].as_str() {
        "none" => None,
        "default" => Some(DEFAULT_HISTORY_BYTES),
        value => Some(value.parse().unwrap()),
    };
    let slice: usize = args.get(6).map_or(4 << 20, |value| value.parse().unwrap());
    let mut terminal = Terminal::with_history_limits(
        GridSize::new(cols, rows),
        HistoryLimits {
            rows: history,
            bytes,
        },
    );
    let mut slices = Vec::new();
    let total = Instant::now();
    for part in data.chunks(slice) {
        let start = Instant::now();
        for chunk in part.chunks(65536) {
            terminal.feed(chunk);
        }
        let ns = start.elapsed().as_nanos() as u64;
        slices.push(json!({"bytes":part.len(),"ns":ns,"history_rows":terminal.scrollback_len()}));
    }
    let feed_ns = total.elapsed().as_nanos() as u64;
    let start = Instant::now();
    drop(terminal);
    let teardown_ns = start.elapsed().as_nanos() as u64;
    println!(
        "{}",
        json!({"input_bytes":data.len(),"cols":cols.get(),"rows":rows.get(),"scrollback":history,
        "scrollback_bytes":bytes,"feed_ns":feed_ns,"teardown_ns":teardown_ns,"slices":slices})
    );
}
