//! Responsiveness probes for tests/responsiveness/run.sh. Standard library
//! only, so `rustc -O probe.rs` builds it.
//!
//!   probe wake  --period-us N --stop FILE --out FILE
//!       Sleeps N µs at a time and records how late each wakeup is.
//!   probe frame --period-us N --work ITERATIONS --stop FILE --out FILE
//!       An application drawing frames: wakes at each frame deadline, does a
//!       fixed amount of work, and records when the frame finished.
//!   probe exec  --interval-ms N --stop FILE --out FILE -- PROGRAM [ARGS]
//!       Starts PROGRAM every N ms and records how long it took to finish.
//!   probe calibrate --work-us N
//!       Prints the iterations of frame work that take N µs on an idle CPU.
//!
//! A probe runs until FILE named by --stop exists, then writes JSON to --out.

use std::collections::HashMap;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

fn work(iterations: u64) -> u64 {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..iterations {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        x ^= x >> 33;
    }
    black_box(x)
}

/// Percentiles of samples in microseconds, as a JSON object body.
fn summary(name: &str, samples: &mut [u64]) -> String {
    samples.sort_unstable();
    let n = samples.len();
    let at = |p: f64| -> u64 {
        if n == 0 {
            0
        } else {
            samples[((p / 100.0) * (n - 1) as f64).round() as usize]
        }
    };
    let mean = if n == 0 { 0.0 } else { samples.iter().sum::<u64>() as f64 / n as f64 };
    format!(
        "\"{name}\":{{\"samples\":{n},\"mean_us\":{mean:.1},\"p50_us\":{},\"p90_us\":{},\"p99_us\":{},\"p999_us\":{},\"max_us\":{}}}",
        at(50.0),
        at(90.0),
        at(99.0),
        at(99.9),
        samples.last().copied().unwrap_or(0)
    )
}

struct Stop {
    file: String,
    checked: Instant,
}

impl Stop {
    fn requested(&mut self) -> bool {
        if self.checked.elapsed() < Duration::from_millis(100) {
            return false;
        }
        self.checked = Instant::now();
        Path::new(&self.file).exists()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_default();
    let mut options: HashMap<String, String> = HashMap::new();
    let mut command: Vec<String> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        if args[i] == "--" {
            command = args[i + 1..].to_vec();
            break;
        }
        options.insert(args[i].trim_start_matches("--").to_owned(), args.get(i + 1).cloned().unwrap_or_default());
        i += 2;
    }
    let number = |key: &str| -> u64 {
        options
            .get(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("--{key} is required"))
    };

    if mode == "calibrate" {
        let target = Duration::from_micros(number("work-us"));
        let mut iterations = 1_000u64;
        // Grow until one run takes at least 50 ms, then scale; take the best of five.
        while {
            let t = Instant::now();
            work(iterations);
            t.elapsed() < Duration::from_millis(50)
        } {
            iterations *= 2;
        }
        let best = (0..5)
            .map(|_| {
                let t = Instant::now();
                work(iterations);
                t.elapsed()
            })
            .min()
            .unwrap();
        println!("{}", (iterations as f64 * target.as_secs_f64() / best.as_secs_f64()) as u64);
        return;
    }

    let mut stop = Stop {
        file: options.get("stop").cloned().expect("--stop is required"),
        checked: Instant::now(),
    };
    let out = options.get("out").cloned().expect("--out is required");
    let started = Instant::now();
    let body = match mode.as_str() {
        "wake" => {
            let period = Duration::from_micros(number("period-us"));
            let mut late = Vec::new();
            while !stop.requested() {
                let t = Instant::now();
                sleep(period);
                late.push(t.elapsed().saturating_sub(period).as_micros() as u64);
            }
            summary("wake_late", &mut late)
        }
        "frame" => {
            let period = Duration::from_micros(number("period-us"));
            let iterations = number("work");
            let (mut late, mut frame) = (Vec::new(), Vec::new());
            let (mut missed, mut missed_twice) = (0u64, 0u64);
            let mut k: u32 = 1;
            while !stop.requested() {
                let deadline = started + period * k;
                let now = Instant::now();
                if deadline > now {
                    sleep(deadline - now);
                }
                late.push(Instant::now().saturating_duration_since(deadline).as_micros() as u64);
                work(iterations);
                let done = Instant::now();
                let took = done.saturating_duration_since(deadline);
                frame.push(took.as_micros() as u64);
                if took > period {
                    missed += 1;
                }
                if took > period * 2 {
                    missed_twice += 1;
                }
                // The next frame is the first deadline after this one finished.
                k = (done.duration_since(started).as_nanos() / period.as_nanos()) as u32 + 1;
            }
            let frames = frame.len() as u64;
            format!(
                "\"frames\":{frames},\"missed\":{missed},\"missed_twice\":{missed_twice},{},{}",
                summary("wake_late", &mut late),
                summary("frame_time", &mut frame)
            )
        }
        "exec" => {
            let interval = Duration::from_millis(number("interval-ms"));
            let (program, rest) = command.split_first().expect("a program after --");
            let mut took = Vec::new();
            let mut failures = 0u64;
            while !stop.requested() {
                let t = Instant::now();
                let ok = Command::new(program)
                    .args(rest)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                took.push(t.elapsed().as_micros() as u64);
                if !ok {
                    failures += 1;
                }
                sleep(interval.saturating_sub(t.elapsed()));
            }
            format!("\"failures\":{failures},{}", summary("exec", &mut took))
        }
        _ => panic!("unknown mode {mode:?}"),
    };
    let json = format!(
        "{{\"mode\":\"{mode}\",\"seconds\":{:.1},{body}}}\n",
        started.elapsed().as_secs_f64()
    );
    fs::write(&out, json).expect("write --out");
}
