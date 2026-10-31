//! Isolated 50 MB history/RSS audit; install as a replay example on the test host.
use hotaru_core::{Color, ContentSnapshot, HistoryLimits, Terminal, attrs};
use serde_json::json;
use std::{hint::black_box, time::Instant};

fn rss() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .map(|v| v.split_whitespace().next().unwrap().parse().unwrap())
        })
        .unwrap()
}
fn number(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}
fn color(hash: &mut u64, value: Color) {
    match value {
        Color::Default => number(hash, 0),
        Color::Indexed(index) => {
            number(hash, 1);
            number(hash, index.into());
        }
        Color::Rgb(rgb) => {
            number(hash, 2);
            for value in [rgb.0, rgb.1, rgb.2] {
                number(hash, value.into());
            }
        }
    }
}
fn digest(snapshot: &ContentSnapshot) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    number(&mut hash, snapshot.cols as u64);
    number(&mut hash, snapshot.rows as u64);
    for row in snapshot.rows() {
        for (col, cell) in row.cells().iter().enumerate() {
            let width = match cell.width() {
                2 => 1,
                0 => 2,
                _ => 0,
            };
            number(&mut hash, width);
            number(&mut hash, u64::from(cell.is_protected()));
            if let Some(cluster) = row.grapheme(col) {
                number(&mut hash, cluster.chars().count() as u64);
                for ch in cluster.chars() {
                    number(&mut hash, ch as u64);
                }
            } else {
                number(&mut hash, 1);
                number(&mut hash, cell.character() as u64);
            }
            let style = row.style(*cell);
            for flag in [
                attrs::BOLD,
                attrs::ITALIC,
                attrs::DIM,
                attrs::BLINK,
                attrs::INVERSE,
                attrs::HIDDEN,
                attrs::STRIKE,
                attrs::OVERLINE,
            ] {
                number(&mut hash, u64::from(style.attrs & flag != 0));
            }
            number(&mut hash, style.underline() as u64);
            for value in [style.foreground, style.background, style.underline_color] {
                color(&mut hash, value);
            }
            let uri = row.hyperlink(col).map_or(&[][..], |link| &link.uri);
            number(&mut hash, uri.len() as u64);
            for byte in uri {
                number(&mut hash, u64::from(*byte));
            }
        }
    }
    hash
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 9);
    let cols = args[2].parse().unwrap();
    let rows = args[3].parse().unwrap();
    let row_limit = args[4].parse().unwrap();
    let byte_limit = args[5].parse().unwrap();
    let compress = args[6] == "1";
    let hold = args[7].as_str();
    let mutation = args[8] == "1";
    assert!(matches!(hold, "none" | "resident" | "cold"));
    let base_rss = rss();
    let data = std::fs::read(&args[1]).unwrap();
    let input_bytes = data.len();
    let mut terminal = Terminal::with_history_limits(
        cols,
        rows,
        HistoryLimits {
            rows: row_limit,
            bytes: Some(byte_limit),
        },
    );
    let start = Instant::now();
    for part in data.chunks(65536) {
        terminal.feed(part);
    }
    let feed_ns = start.elapsed().as_nanos() as u64;
    drop(data);
    let populated_rss = rss();
    let populated = terminal.history_memory();
    let validation = digest(&terminal.content_snapshot());
    let start = Instant::now();
    let mut captured = (hold == "resident").then(|| terminal.content_snapshot());
    let resident_capture_ns = start.elapsed().as_nanos() as u64;
    let start = Instant::now();
    let mut steps = 0;
    if compress {
        while terminal.compress_history_step() {
            steps += 1;
        }
    }
    let compression_ns = start.elapsed().as_nanos() as u64;
    let cold_rss = rss();
    let cold = terminal.history_memory();
    let start = Instant::now();
    let snapshot = captured
        .take()
        .unwrap_or_else(|| terminal.content_snapshot());
    let capture_ns = if hold == "resident" {
        resident_capture_ns
    } else {
        start.elapsed().as_nanos() as u64
    };
    let retention = snapshot.retention();
    let captured_rss = rss();
    let mut terminal = Some(terminal);
    if mutation {
        terminal
            .as_mut()
            .unwrap()
            .feed(b"\x1b[3J\x1b[2Jreplacement");
    }
    if hold != "none" {
        drop(terminal.take());
    }
    let owner_release_rss = rss();
    let start = Instant::now();
    let mut checksum = 0u64;
    for row in snapshot.rows() {
        checksum = checksum.wrapping_add(row.cells()[0].character() as u64);
    }
    let read_ns = start.elapsed().as_nanos() as u64;
    black_box(checksum);
    let read_rss = rss();
    assert_eq!(
        digest(&snapshot),
        validation,
        "captured complete content changed"
    );
    drop(snapshot);
    drop(terminal);
    let released_rss = rss();
    println!(
        "{}",
        json!({"engine":"hotaru","input_bytes":input_bytes,"cols":cols,"rows":rows,"row_limit":row_limit,"byte_limit":byte_limit,"hold":hold,"mutated_before_read":mutation,
      "compressed":compress,"history_rows":cold.rows,"content_fnv1a64":format!("{validation:016x}"),"checksum":checksum,
      "feed_ns":feed_ns,"compression_ns":compression_ns,"capture_ns":capture_ns,"read_history_ns":read_ns,"compression_steps":steps,
      "rss_base_kib":base_rss,"rss_populated_kib":populated_rss,"rss_cold_kib":cold_rss,"rss_captured_kib":captured_rss,"rss_owner_released_kib":owner_release_rss,"rss_read_kib":read_rss,"rss_released_kib":released_rss,
      "logical_history_bytes":cold.logical_bytes,"logical_active_bytes":cold.active_bytes,"storage_populated_estimate":populated.retained_bytes_estimate,"storage_cold_estimate":cold.retained_bytes_estimate,
      "compressed_rows":cold.compressed_rows,"compressed_blocks":cold.compressed_blocks,"compressed_bytes":cold.compressed_bytes,
      "capture_storage_estimate":retention.storage_bytes_estimate,"capture_logical_bytes":retention.logical_bytes})
    );
}
