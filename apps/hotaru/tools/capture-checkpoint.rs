//! Eager capture driver for checkpoint 13423440ef68b3633b41d267aea196d87b1007da.
//! Keep workload and measurement boundaries identical to content_capture.rs.
use hotaru_core::Terminal;
use serde_json::json;
use std::{hint::black_box, time::Instant};
fn percentile(samples: &mut [u64], fraction: usize) -> u64 {
    samples.sort_unstable();
    samples[(samples.len() - 1) * fraction / 100]
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let history: usize = args[1].parse().unwrap();
    let cold = args[2] == "cold";
    let mut terminal = Terminal::new(80, 24, history);
    for index in 0..history + 24 {
        terminal.feed(format!("{index:08}: retained history 雪 e\u{301}\r\n").as_bytes());
    }
    if cold {
        while terminal.compress_history_step() {}
    }
    assert_eq!(terminal.scrollback_len(), history);
    let mut capture_ns = Vec::new();
    let mut release_ns = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let snapshot = black_box(terminal.snapshot_all());
        capture_ns.push(start.elapsed().as_nanos() as u64);
        let start = Instant::now();
        drop(black_box(snapshot));
        release_ns.push(start.elapsed().as_nanos() as u64);
    }
    println!(
        "{}",
        json!({"history_rows":history,"storage":if cold{"cold"}else{"resident"},"method":"snapshot_all","samples":30,
        "capture_ns_p50":percentile(&mut capture_ns,50),"capture_ns_p95":percentile(&mut capture_ns,95),
        "release_ns_p50":percentile(&mut release_ns,50)})
    );
}
