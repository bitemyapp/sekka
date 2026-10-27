//! What Koyomi's typical interactions cost in CPU time, from the input to
//! its pixels at 2× (the program's update, the view, layout and diff, and drawing
//! what changed): switching months and weeks, and dragging an event
//! across the week. Prints the percentiles (docs/performance.md) and
//! holds the 95th to the 4 ms budget in an optimised build.

mod common;

use std::time::Duration;

use aizome_kit::Vec2;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use common::*;
use koyomi::app::{Koyomi, Msg, View};

/// This thread's CPU time: the budget is CPU time, and a busy machine
/// (builds alongside) stretches wall-clock time without the frame costing
/// more.
fn cpu() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes only into `ts`.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

fn frame(h: &mut Harness<Koyomi>, act: impl FnOnce(&mut Harness<Koyomi>)) -> Duration {
    let w = h.window();
    h.take_damage(w);
    let start = cpu();
    act(h);
    // Each damaged rectangle drawn, as a window's frame draws them.
    for d in h.take_damage(w) {
        let _ = h.render_rect(w, d, 2.0);
    }
    cpu() - start
}

fn report(name: &str, mut v: Vec<Duration>) -> Duration {
    v.sort();
    let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
    eprintln!("koyomi {name}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)", p(0.5), p(0.95), v.last().unwrap(), v.len());
    p(0.95)
}

fn budget(name: &str, p95: Duration) {
    if !cfg!(debug_assertions) {
        assert!(p95 < Duration::from_millis(4), "{name}: p95 {p95:?}");
    }
}

#[test]
fn switching_months_and_weeks_fits_the_budget() {
    let mut h = harness("perf-switch", ThemeChoice::Night);
    let w = h.window();
    let _ = h.render(w, 2.0);
    let mut months = Vec::new();
    for round in 0..24 {
        let key = if round % 12 < 6 { NamedKey::ArrowRight } else { NamedKey::ArrowLeft };
        let d = frame(&mut h, |h| h.named(w, key, Modifiers::NONE));
        if round >= 2 {
            months.push(d);
        }
    }
    budget("month", report("next month", months));
    h.send(Msg::SetView(View::Week));
    let _ = h.render(w, 2.0);
    let mut weeks = Vec::new();
    for round in 0..24 {
        let key = if round % 12 < 6 { NamedKey::ArrowRight } else { NamedKey::ArrowLeft };
        let d = frame(&mut h, |h| h.named(w, key, Modifiers::NONE));
        if round >= 2 {
            weeks.push(d);
        }
    }
    budget("week", report("next week", weeks));
}

#[test]
fn dragging_across_the_week_fits_the_budget() {
    // Saves go to a thread, as in the app (not into the frame).
    let mut h = harness_with("perf-drag", ThemeChoice::Night, |o| o.background_saves = true);
    let w = h.window();
    h.send(Msg::SetView(View::Week));
    let _ = h.render(w, 2.0);
    let grid = h.find_id(w, "time-grid").unwrap();
    let col = (grid.width() - 56.0) / 7.0;
    let mut moves = Vec::new();
    let mut drops = Vec::new();
    for round in 0..6 {
        let review = h.find(w, "Design review: Koyomi").unwrap();
        let from = review.center();
        let dir = if round % 2 == 0 { 1.0 } else { -1.0 };
        let d = frame(&mut h, |h| h.press(w, from, PointerButton::Left, 1));
        let _ = d;
        // Across two days and down two hours, a step at a time.
        for i in 1..=40 {
            let t = i as f64 / 40.0;
            let p = from + Vec2::new(dir * col * 2.0 * t, dir * 96.0 * t);
            moves.push(frame(&mut h, |h| h.move_to(w, p)));
        }
        let to = from + Vec2::new(dir * col * 2.0, dir * 96.0);
        drops.push(frame(&mut h, |h| h.release(w, to, PointerButton::Left)));
    }
    budget("drag", report("drag step", moves));
    budget("drop", report("drop", drops));
}
