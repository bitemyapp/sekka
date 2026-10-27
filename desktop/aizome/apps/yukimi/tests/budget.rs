//! How long Yukimi's typical interactions take, from the event to the
//! frame rasterised at 2× (update, view, layout, diff, and painting what
//! changed through the offscreen renderer): `cargo test -p aizome-yukimi
//! --release --test budget -- --nocapture` prints them. The numbers are in
//! docs/performance.md.
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::WindowKey;
use aizome_kit::layout::{Point, Rect, Vec2};
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::theme::Appearance;
use aizome_yukimi::app::{Clock, Msg, Page, Runner, Source, Yukimi};

fn sample(motion: bool) -> (Harness<Yukimi>, WindowKey) {
    let model = Arc::new(aizome_yukimi::fixture::model());
    let mut app = Yukimi::with(
        Source::Sample(model.clone()),
        Runner::Mock(Arc::new(|_| Vec::new())),
        Clock::Fixed(aizome_yukimi::fixture::NOW, 21),
    );
    app.check = Some(Arc::new(aizome_yukimi::fixture::check(&model)));
    app.index = Some(Arc::new(aizome_yukimi::fixture::index()));
    let h = Harness::with(app, Appearance { reduced_motion: !motion, ..Appearance::default() }, false);
    let w = h.window();
    (h, w)
}

/// Rasterise what the last event damaged, at 2×: its rectangles, as the
/// window's renderer draws them. The share of the window they cover.
fn draw(h: &mut Harness<Yukimi>, w: WindowKey) -> f64 {
    let damage = h.take_damage(w);
    let size = h.size(w);
    let _ = h.render_damage(w, &damage, 2.0);
    covered(&damage, size.width * size.height)
}

/// How much of `area` the rectangles cover (overlaps counted once).
fn covered(rects: &[Rect], area: f64) -> f64 {
    // Across each band between the rectangles' tops and bottoms, the union
    // of what spans it.
    let mut ys: Vec<f64> = rects.iter().flat_map(|r| [r.y0, r.y1]).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut sum = 0.0;
    for band in ys.windows(2) {
        let mut xs: Vec<(f64, f64)> =
            rects.iter().filter(|r| r.y0 <= band[0] && r.y1 >= band[1]).map(|r| (r.x0, r.x1)).collect();
        xs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut width, mut end) = (0.0, f64::NEG_INFINITY);
        for (x0, x1) in xs {
            let from = x0.max(end);
            if x1 > from {
                width += x1 - from;
            }
            end = end.max(x1);
        }
        sum += width * (band[1] - band[0]);
    }
    (sum / area).min(1.0)
}

/// Time `act` `n` times (after two to warm up), with its frame.
fn measure(
    name: &str,
    n: usize,
    h: &mut Harness<Yukimi>,
    w: WindowKey,
    mut act: impl FnMut(&mut Harness<Yukimi>, usize),
) -> Duration {
    let _ = h.render(w, 2.0);
    let mut frames = Vec::new();
    let mut damaged = 0.0;
    for i in 0..n + 2 {
        h.take_damage(w);
        let start = Instant::now();
        act(h, i);
        let area = draw(h, w);
        if i >= 2 {
            frames.push(start.elapsed());
            damaged += area;
        }
    }
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "yukimi {name}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms, {:.0}% of the window drawn (n={n}, at 2×)",
        p(0.5).as_secs_f64() * 1e3,
        p(0.95).as_secs_f64() * 1e3,
        frames.last().unwrap().as_secs_f64() * 1e3,
        damaged / n as f64 * 100.0
    );
    p(0.95)
}

#[test]
fn typical_interactions() {
    let (mut h, w) = sample(false);
    // Choosing a place in the sidebar: a new page, drawn whole.
    let places =
        [Page::Installed, Page::Discover, Page::Updates, Page::History, Page::Storage, Page::Desktops, Page::Overview];
    measure("switching pages", 36, &mut h, w, |h, i| h.send(Msg::Show(places[i % places.len()])));

    // Scrolling Installed with the wheel.
    h.send(Msg::Show(Page::Installed));
    measure("scrolling", 40, &mut h, w, |h, i| {
        let dy = if (i / 8) % 2 == 0 { 40.0 } else { -40.0 };
        h.scroll(w, Point::new(640.0, 420.0), Vec2::new(0.0, dy));
    });

    // Typing in Discover's search (the answer comes later, from its thread).
    h.send(Msg::Show(Page::Discover));
    measure("typing a search", 30, &mut h, w, |h, i| {
        if i % 6 == 5 {
            h.named(w, NamedKey::Backspace, Modifiers::default());
        } else {
            h.type_text(w, &"image".chars().nth(i % 6).unwrap_or('e').to_string());
        }
    });

    // Opening and closing a generation in History.
    h.send(Msg::Show(Page::History));
    measure("opening a generation", 20, &mut h, w, |h, i| h.send(Msg::ToggleGeneration(24 + (i % 5) as u32)));

    // Picking desktops on the Desktops page.
    h.send(Msg::Show(Page::Desktops));
    measure("picking a desktop", 20, &mut h, w, |h, i| {
        h.send(Msg::Pick { setting: "calamares.desktops".into(), id: "plasma".into(), on: i % 2 == 0 })
    });

    // Hovering over rows (no view: the kit redraws the row).
    h.send(Msg::Show(Page::Discover));
    h.send(Msg::Search(String::new()));
    measure("hovering cards", 40, &mut h, w, |h, i| h.move_to(w, Point::new(330.0 + (i % 4) as f64 * 218.0, 300.0)));
}

#[test]
fn falling_snow() {
    let (mut h, w) = sample(true);
    h.focus_window(w, true);
    let _ = h.render(w, 2.0);
    measure("a frame of snow", 60, &mut h, w, |h, _| h.advance(Duration::from_millis(34)));
}

/// Each page on its own, its update and view apart from its drawing.
#[test]
fn each_page() {
    if std::env::var_os("YUKIMI_BUDGET_DETAIL").is_none() {
        return;
    }
    let (mut h, w) = sample(false);
    for page in Page::ALL {
        let mut total = Vec::new();
        let mut viewing = Vec::new();
        for _ in 0..12 {
            h.send(Msg::Show(Page::Overview));
            if page == Page::Overview {
                h.send(Msg::Show(Page::Storage));
            }
            let _ = h.render(w, 2.0);
            h.take_damage(w);
            let start = Instant::now();
            h.send(Msg::Show(page));
            let viewed = start.elapsed();
            draw(&mut h, w);
            total.push(start.elapsed());
            viewing.push(viewed);
        }
        total.sort();
        viewing.sort();
        eprintln!(
            "yukimi {:?}: p50 {:.2} ms, of which update, view and layout {:.2} ms",
            page,
            total[6].as_secs_f64() * 1e3,
            viewing[6].as_secs_f64() * 1e3
        );
    }
}
