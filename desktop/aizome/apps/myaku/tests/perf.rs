//! What Myaku's typical frames cost, from the message to its pixels at
//! 2× (the update, the view, layout and diff, and rasterising each damaged
//! rectangle), with 600 processes: a new reading every second, a click on
//! a row, scrolling the table. Prints percentiles (for
//! docs/performance.md); holds the median to the 4 ms budget in an
//! optimised build. And memory: two hundred readings mustn't grow it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::testing::Harness;
use aizome_kit::{Point, Vec2};
use myaku::app::{Msg, Myaku, Request};
use myaku::fixture;

const PROCESSES: usize = 600;

fn frame(h: &mut Harness<Myaku>, act: impl FnOnce(&mut Harness<Myaku>)) -> Duration {
    let w = h.window();
    h.take_damage(w);
    let start = Instant::now();
    act(h);
    for d in h.take_damage(w) {
        let _ = h.render_rect(w, d, 2.0);
    }
    start.elapsed()
}

fn report(what: &str, mut v: Vec<Duration>) -> (Duration, Duration) {
    v.sort();
    let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
    eprintln!(
        "myaku {what}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x, {PROCESSES} processes)",
        p(0.5),
        p(0.95),
        v.last().unwrap(),
        v.len()
    );
    (p(0.5), p(0.95))
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| l.strip_prefix("VmRSS:").and_then(|v| v.split_whitespace().next()?.parse().ok()))
        })
        .unwrap_or(0)
}

#[test]
fn frames_fit_the_budget_and_memory_stays_put() {
    let (signals, _) = Myaku::recorder();
    let frames: Vec<Arc<_>> = (1..=260).map(|t| Arc::new(fixture::frame(PROCESSES, t))).collect();
    let mut h = Harness::new(Myaku::with_frame(frames[0].clone(), signals, Request::default()));
    let w = h.window();
    for f in &frames[1..30] {
        h.send(Msg::Frame(f.clone()));
    }
    let _ = h.render(w, 2.0);

    let mut readings = Vec::new();
    for f in &frames[30..90] {
        let f = f.clone();
        readings.push(frame(&mut h, |h| h.send(Msg::Frame(f))));
    }

    let table = h.find_id(w, "processes").expect("the table");
    let mut clicks = Vec::new();
    for k in 0..30 {
        let at = Point::new(table.x0 + 120.0, table.y0 + 40.0 + f64::from(k % 12) * 24.0);
        clicks.push(frame(&mut h, |h| h.click(w, at)));
    }
    let mut scrolls = Vec::new();
    for k in 0..30 {
        let dy = if k % 10 < 5 { 48.0 } else { -48.0 };
        scrolls.push(frame(&mut h, |h| h.scroll(w, table.center(), Vec2::new(0.0, dy))));
    }

    // Memory over two hundred more readings (after the history filled).
    let before = rss_kib();
    for f in &frames[60..260] {
        h.send(Msg::Frame(f.clone()));
        let _ = h.take_damage(w);
    }
    let after = rss_kib();
    eprintln!("myaku memory: {before} KiB before 200 readings, {after} KiB after; {}", h.stats().summary());

    let worst = [report("reading", readings), report("row click", clicks), report("scroll", scrolls)];
    if !cfg!(debug_assertions) {
        for (p50, _) in worst {
            assert!(p50 < Duration::from_millis(4), "p50 {p50:?}");
        }
    }
    assert!(after < before + 8 * 1024, "{before} KiB → {after} KiB: readings mustn't pile up");
}
