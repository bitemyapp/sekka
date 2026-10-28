//! Resident-memory and history-read baseline, run with tools/history_benchmark.py.
use hotaru_core::Terminal;
use serde_json::json;
use std::{hint::black_box, time::Instant};
fn rss_kib() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .map(|v| v.split_whitespace().next().unwrap().parse().unwrap())
        })
        .unwrap()
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let cols: usize = args[2].parse().unwrap();
    let rows: usize = args[3].parse().unwrap();
    let history: usize = args[4].parse().unwrap();
    let compress = args.get(5).is_some_and(|arg| arg == "1");
    let before = rss_kib();
    let mut terminal = Terminal::with_history_limits(
        cols,
        rows,
        hotaru_core::HistoryLimits {
            rows: history,
            bytes: None,
        },
    );
    let start = Instant::now();
    for part in data.chunks(65536) {
        terminal.feed(part);
    }
    let feed_ns = start.elapsed().as_nanos() as u64;
    let populated = rss_kib();
    let start = Instant::now();
    let mut steps = 0;
    if compress {
        while terminal.compress_history_step() {
            steps += 1;
        }
    }
    let compression_ns = start.elapsed().as_nanos() as u64;
    let cold = rss_kib();
    let memory = terminal.history_memory();
    let start = Instant::now();
    let snapshot = terminal.snapshot_all();
    let mut checksum = 0u64;
    for row in &snapshot.lines {
        checksum = checksum.wrapping_add(row.cells()[0].character() as u64);
    }
    let read_ns = start.elapsed().as_nanos() as u64;
    black_box(checksum);
    println!(
        "{}",
        json!({"engine":"hotaru","phase":if compress {"compressed"} else {"uncompressed"},"input_bytes":data.len(),
        "cols":cols,"rows":rows,"history_rows":terminal.scrollback_len(),"feed_ns":feed_ns,
        "read_history_ns":read_ns,"rss_before_kib":before,"rss_populated_kib":populated,
        "rss_cold_kib":cold,"rss_read_kib":rss_kib(),"checksum":checksum,"compression_ns":compression_ns,
        "compression_steps":steps,"compressed_rows":memory.compressed_rows,"compressed_blocks":memory.compressed_blocks,
        "resident_size_bound":memory.resident_size_bound,"compressed_bytes":memory.compressed_bytes})
    );
}
