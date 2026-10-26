//! How long a frame of Annai's scenes takes, from the clock's tick to the
//! damage rasterised at 2×, and stepping to the next page:
//! `cargo test -p aizome-annai --release --test budget -- --nocapture`.
//! The numbers are in docs/performance.md.

use std::time::{Duration, Instant};

use aizome_annai::app::{FRAME, PAGES};
use aizome_annai::{Annai, Msg};
use aizome_kit::Appearance;
use aizome_kit::layout::Rect;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

fn draw(h: &mut Harness<Annai>, w: aizome_kit::WindowKey) -> f64 {
    let mut bounds: Option<Rect> = None;
    for d in h.take_damage(w) {
        bounds = Some(bounds.map_or(d, |b| b.union(d)));
    }
    let size = h.size(w);
    match bounds {
        Some(b) => {
            let _ = h.render_rect(w, b, 2.0);
            b.width() * b.height() / (size.width * size.height)
        }
        None => 0.0,
    }
}

fn report(name: &str, mut v: Vec<Duration>, area: f64) {
    v.sort();
    let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize].as_secs_f64() * 1e3;
    eprintln!(
        "annai {name}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms, {:.0}% of the window drawn (n={}, at 2×)",
        p(0.5),
        p(0.95),
        p(1.0),
        area * 100.0,
        v.len()
    );
}

#[test]
fn a_frame_of_each_scene_and_a_page_turn() {
    let mut app = Annai::new(None);
    app.dry_run = true;
    let mut h = Harness::with(app, Appearance::default(), false);
    let w = h.window();
    let mut all = Vec::new();
    let mut area = 0.0;
    for page in 0..PAGES {
        h.send(Msg::Go(page));
        let _ = h.render(w, 2.0);
        h.take_damage(w);
        let mut v = Vec::new();
        let mut a = 0.0;
        for _ in 0..120 {
            let start = Instant::now();
            h.advance(FRAME);
            a += draw(&mut h, w);
            v.push(start.elapsed());
        }
        area += a / 120.0;
        all.extend(v.iter().copied());
        report(&format!("a frame of scene {}", page + 1), v, a / 120.0);
    }
    report("a frame of any scene", all, area / PAGES as f64);
    let mut v = Vec::new();
    let mut a = 0.0;
    h.send(Msg::Go(0));
    for _ in 0..30 {
        h.take_damage(w);
        let last = h.program().page + 1 == PAGES;
        let start = Instant::now();
        h.named(w, if last { NamedKey::Home } else { NamedKey::ArrowRight }, Modifiers::NONE);
        a += draw(&mut h, w);
        v.push(start.elapsed());
    }
    report("turning a page", v, a / 30.0);
}
