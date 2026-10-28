//! How long Shiori's typical interactions take, from the event to the
//! frame rasterised at 2× in a 1440 × 900 window (update, view, layout,
//! diff, and painting what changed through the offscreen renderer), with
//! the pages' drawings there: `cargo test -p aizome-shiori --release
//! --test budget -- --nocapture` prints them. The numbers are in
//! docs/performance.md.

mod common;

use std::time::{Duration, Instant};

use aizome_kit::Appearance;
use aizome_kit::WindowKey;
use aizome_kit::layout::{Point, Rect, Size, Vec2};
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use aizome_shiori::app::{Msg, Shiori};
use aizome_shiori::places::Places;

fn settle(h: &mut Harness<Shiori>, w: WindowKey) {
    for _ in 0..3000 {
        let _ = h.render(w, 2.0);
        h.send(Msg::Nothing);
        h.advance(Duration::from_millis(20));
        if h.program().idle() {
            return;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    panic!("Shiori didn't settle");
}

fn report(name: &str, (frames, own, area): &mut (Vec<Duration>, Vec<Duration>, f64)) {
    frames.sort();
    own.sort();
    let p = |v: &[Duration], q: f64| v[((v.len() - 1) as f64 * q) as usize].as_secs_f64() * 1e3;
    eprintln!(
        "shiori {name}: frame p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms; Shiori's part p50 {:.2} ms, p95 {:.2} ms; {:.0}% of the window drawn (n={}, at 2×)",
        p(frames, 0.5),
        p(frames, 0.95),
        p(frames, 1.0),
        p(own, 0.5),
        p(own, 0.95),
        *area / frames.len() as f64 * 100.0,
        frames.len()
    );
}

#[test]
fn typical_interactions() {
    let dir = common::scratch("budget");
    common::write(&dir, "long.pdf", &common::long(60));
    let mut app = Shiori::new();
    app.worker = common::worker();
    app.places = Places::in_memory();
    app.dry_run = true;
    app.initial = vec![dir.join("long.pdf")];
    let mut h = Harness::with(app, Appearance { theme: ThemeChoice::Night, ..Appearance::default() }, false);
    let w = h.window();
    h.resize(w, Size::new(1440.0, 900.0));
    settle(&mut h, w);
    let size = h.size(w);
    let measure = |h: &mut Harness<Shiori>, n: usize, act: &dyn Fn(&mut Harness<Shiori>, usize)| {
        let mut frames = Vec::new();
        let mut own = Vec::new();
        let mut area = 0.0;
        for i in 0..n + 2 {
            // The drawings come between frames, as they do from the worker.
            settle(h, w);
            h.take_damage(w);
            let start = Instant::now();
            act(h, i);
            let mine = start.elapsed();
            let damage = h.take_damage(w);
            let _ = h.render_damage(w, &damage, 2.0);
            if i >= 2 {
                frames.push(start.elapsed());
                own.push(mine);
                area += (damage.iter().map(|r: &Rect| r.width() * r.height()).sum::<f64>()
                    / (size.width * size.height))
                    .min(1.0);
            }
        }
        (frames, own, area)
    };
    // Scrolling a wheel's notch at a time.
    let mut t = measure(&mut h, 40, &|h, _| h.scroll(w, Point::new(700.0, 400.0), Vec2::new(0.0, 40.0)));
    report("scrolling a notch", &mut t);
    let worst = t.0[((t.0.len() - 1) as f64 * 0.95) as usize];
    // Moving the pointer over the text (the pointer's shape).
    let mut t = measure(&mut h, 40, &|h, i| {
        h.move_to(w, Point::new(300.0 + (i % 7) as f64 * 40.0, 200.0 + (i % 5) as f64 * 30.0))
    });
    report("moving the pointer", &mut t);
    // Paging down.
    let mut t = measure(&mut h, 20, &|h, _| h.send(Msg::Nav(w, aizome_shiori::app::Nav::PageDown)));
    report("paging down", &mut t);
    // Generous in debug builds; the numbers that count are release's.
    assert!(worst < Duration::from_millis(if cfg!(debug_assertions) { 200 } else { 12 }), "{worst:?}");
}
