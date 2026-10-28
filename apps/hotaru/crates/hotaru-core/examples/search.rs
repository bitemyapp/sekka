//! Development search timing driver; parsing and result serialization are
//! outside the timed region. Capture creation is included, release excluded.
use hotaru_core::{
    Terminal,
    search::{self, Options},
};
use serde_json::json;
use std::{hint::black_box, sync::atomic::AtomicBool, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: search INPUT NEEDLE ITERATIONS resident|cold".into());
    }
    let input = std::fs::read(&args[1])?;
    let iterations: usize = args[3].parse()?;
    if !(1..=1000).contains(&iterations) {
        return Err("iterations must be 1..1000".into());
    }
    let cold = match args[4].as_str() {
        "cold" => true,
        "resident" => false,
        _ => return Err("invalid storage".into()),
    };
    let mut terminal = Terminal::new(80, 24, 1_000_000);
    for chunk in input.chunks(65_536) {
        terminal.feed(chunk);
    }
    if cold {
        while terminal.compress_history_step() {}
    }
    let mut samples = Vec::new();
    let mut first = None;
    for _ in 0..iterations {
        let start = Instant::now();
        let result = black_box(search::find(
            terminal.content_snapshot(),
            args[2].as_bytes(),
            Options {
                max_matches: 1_000_000,
            },
            &AtomicBool::new(false),
        )?);
        samples.push(start.elapsed().as_nanos() as u64);
        assert!(!result.truncated());
        if let Some((total, _, _)) = &first {
            assert_eq!(*total, result.total_matches);
        } else {
            let points = result
                .matches
                .iter()
                .map(|m| [[m.start.row, m.start.col], [m.end.row, m.end.col]])
                .collect::<Vec<_>>();
            first = Some((result.total_matches, result.bytes_scanned, points));
        }
    }
    let (total, bytes, points) = first.unwrap();
    println!(
        "{}",
        json!({"samples_ns":samples,"total_matches":total,"bytes_scanned":bytes,"matches":points,
                         "history_rows":terminal.scrollback_len(),"storage":args[4],"input_bytes":input.len()})
    );
    Ok(())
}
