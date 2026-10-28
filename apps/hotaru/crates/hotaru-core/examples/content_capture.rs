//! Same-host capture measurement. Pass `eager` to exercise snapshot_all on the
//! checkpoint before ContentSnapshot; `owned` additionally measures iteration.
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
    let owned = args[3] == "owned";
    let mut terminal = Terminal::with_history_limits(
        80,
        24,
        hotaru_core::HistoryLimits {
            rows: history,
            bytes: None,
        },
    );
    for index in 0..history + 24 {
        terminal.feed(format!("{index:08}: retained history 雪 e\u{301}\r\n").as_bytes());
    }
    if cold {
        while terminal.compress_history_step() {}
    }
    assert_eq!(terminal.scrollback_len(), history);
    let iterations = if owned { 500 } else { 30 };
    let mut capture_ns = Vec::new();
    let mut release_ns = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        if owned {
            let snapshot = black_box(terminal.content_snapshot());
            capture_ns.push(start.elapsed().as_nanos() as u64);
            let start = Instant::now();
            drop(black_box(snapshot));
            release_ns.push(start.elapsed().as_nanos() as u64);
        } else {
            let snapshot = black_box(terminal.snapshot_all());
            capture_ns.push(start.elapsed().as_nanos() as u64);
            let start = Instant::now();
            drop(black_box(snapshot));
            release_ns.push(start.elapsed().as_nanos() as u64);
        }
    }
    let mut checksum = 0u64;
    let worker_restore_ns = if owned {
        let snapshot = terminal.content_snapshot();
        drop(terminal);
        std::thread::spawn(move || {
            let start = Instant::now();
            let mut checksum = 0u64;
            for row in snapshot.rows() {
                checksum = checksum.wrapping_add(row.cells()[0].character() as u64);
            }
            (start.elapsed().as_nanos() as u64, checksum)
        })
        .join()
        .map(|(elapsed, sum)| {
            checksum = sum;
            elapsed
        })
        .unwrap()
    } else {
        0
    };
    println!(
        "{}",
        json!({"history_rows":history,"storage":if cold{"cold"}else{"resident"},
        "method":if owned{"content_snapshot"}else{"snapshot_all"},"samples":iterations,
        "capture_ns_p50":percentile(&mut capture_ns,50),"capture_ns_p95":percentile(&mut capture_ns,95),
        "release_ns_p50":percentile(&mut release_ns,50),"worker_restore_ns":worker_restore_ns,"checksum":black_box(checksum)})
    );
}
