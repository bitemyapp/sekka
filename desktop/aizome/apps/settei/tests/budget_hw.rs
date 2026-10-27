//! The hardware and network panes' typical interactions against the 4 ms
//! frame budget, measured headlessly as `docs/performance.md` describes:
//! the update, the view, layout and diff, and rasterising the damage at
//! 2×. Asserted only in release builds; printed always.

mod common;

use std::time::{Duration, Instant};

use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::testing::Harness;
use settei::{AppMsg, Settei};

fn settle(h: &mut Harness<Settei>, ready: &str) {
    let w = h.window();
    for _ in 0..400 {
        if h.texts(w).iter().any(|t| t == ready) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
        h.send(AppMsg::Nothing);
    }
}

fn open(pane: &str, ready: &str) -> Harness<Settei> {
    let (mut h, _) = common::open(&format!("budget-{pane}"), pane);
    let w = h.window();
    h.resize(w, Size::new(940.0, 1000.0));
    settle(&mut h, ready);
    let _ = h.render(w, 2.0);
    h
}

/// One frame: `act`, then rasterising what it damaged at 2×.
fn frame(h: &mut Harness<Settei>, act: impl FnOnce(&mut Harness<Settei>)) -> (Duration, f64) {
    let w = h.window();
    h.take_damage(w);
    let start = Instant::now();
    act(h);
    let damage = h.take_damage(w);
    let mut bounds: Option<Rect> = None;
    for d in damage {
        bounds = Some(bounds.map_or(d, |b| b.union(d)));
    }
    let area = bounds.map_or(0.0, |b| b.area());
    if let Some(b) = bounds {
        let _ = h.render_rect(w, b, 2.0);
    }
    let size = h.size(w);
    (start.elapsed(), area / (size.width * size.height))
}

fn report(name: &str, mut frames: Vec<(Duration, f64)>) -> Duration {
    frames.sort_by_key(|f| f.0);
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize].0;
    let damage = frames.iter().map(|f| f.1).sum::<f64>() / frames.len() as f64;
    eprintln!(
        "settei {name}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x, {:.0}% of the window redrawn)",
        p(0.5),
        p(0.95),
        frames.last().unwrap().0,
        frames.len(),
        damage * 100.0
    );
    p(0.95)
}

#[test]
fn typical_interactions_fit_the_frame_budget() {
    let mut worst: Vec<(&str, Duration)> = Vec::new();

    // Network: the Wi-Fi switch, off and on (the networks go and come).
    let mut h = open("network", "Kohaku");
    let w = h.window();
    let mut frames = Vec::new();
    for i in 0..30 {
        let at = h.find(w, "Wi-Fi").expect("the Wi-Fi row");
        let x = h.size(w).width - 50.0;
        // The sample changes at once, as the view reads it.
        let f = frame(&mut h, |h| h.click(w, Point::new(x, at.center().y)));
        if i > 1 {
            frames.push(f);
        }
    }
    worst.push(("network: Wi-Fi switch", report("network, the Wi-Fi switch", frames)));

    // Displays: a display dragged across the arrangement, frame by frame
    // (the pane isn't viewed again while it moves).
    let mut h = open("displays", "Use as");
    let w = h.window();
    let area = h.find_id(w, "entry:displays.arrangement").expect("the arrangement");
    let from = Point::new(area.x0 + area.width() * 0.33, area.y0 + area.height() * 0.6);
    h.press(w, from, aizome_kit::sidestep_ui::PointerButton::Left, 1);
    let mut frames = Vec::new();
    for i in 0..60 {
        let to = Point::new(from.x + (i as f64 * 6.0) % 300.0, from.y + ((i * 7) % 40) as f64 - 20.0);
        let f = frame(&mut h, |h| h.move_to(w, to));
        if i > 2 {
            frames.push(f);
        }
    }
    h.release(w, from, aizome_kit::sidestep_ui::PointerButton::Left);
    worst.push(("displays: dragging", report("displays, dragging a display", frames)));

    // Sound: the input meter's tick (12.5 a second while the pane shows).
    let mut h = open("sound", "Built-in Speakers");
    let mut frames = Vec::new();
    for i in 0..60 {
        let f = frame(&mut h, |h| h.advance(Duration::from_millis(80)));
        if i > 2 {
            frames.push(f);
        }
    }
    worst.push(("sound: meter tick", report("sound, the input meter's tick", frames)));

    // Bluetooth: a device's options opened and closed.
    let mut h = open("bluetooth", "WH-1000XM5");
    let w = h.window();
    let mut frames = Vec::new();
    for i in 0..30 {
        let opts: Vec<Rect> = h
            .semantics(w)
            .into_iter()
            .filter(|(s, _)| s.label.as_deref().is_some_and(|l| l.starts_with("Options for")))
            .map(|(_, r)| r)
            .collect();
        let Some(r) = opts.first().copied() else { break };
        let f = frame(&mut h, |h| h.click(w, r.center()));
        if i > 1 {
            frames.push(f);
        }
    }
    if !frames.is_empty() {
        worst.push(("bluetooth: options", report("bluetooth, a device's options", frames)));
    }

    if !cfg!(debug_assertions) {
        for (name, p95) in worst {
            assert!(p95 < Duration::from_millis(4), "{name} p95 {p95:?}");
        }
    }
}
