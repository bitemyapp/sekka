//! What Hako's typical interactions cost, against the 4 ms budget: the
//! keyboard walking a big folder's column, the wheel scrolling it, and
//! the list view. Each frame is the event, the update, the view, layout
//! and diff, and rasterising the damage at 2× through the offscreen
//! renderer. Run with `--nocapture` to see the numbers (and `--release`
//! for a build like a session's).

mod common;

use std::time::{Duration, Instant};

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use aizome_kit::{Rect, Vec2, WindowKey};
use common::*;
use hako::ui::fileview::Layout;
use hako::ui::{Hako, Msg};

fn frame(h: &mut Harness<Hako>, w: WindowKey, act: impl FnOnce(&mut Harness<Hako>)) -> Duration {
    h.take_damage(w);
    let start = Instant::now();
    act(h);
    let mut bounds: Option<Rect> = None;
    for d in h.take_damage(w) {
        bounds = Some(bounds.map_or(d, |b| b.union(d)));
    }
    if let Some(b) = bounds {
        let _ = h.render_rect(w, b, 2.0);
    }
    start.elapsed()
}

fn report(name: &str, mut frames: Vec<Duration>) -> Duration {
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "hako {name}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
    p(0.5)
}

#[test]
fn walking_and_scrolling_a_big_folder_fit_the_budget() {
    let s = Sandbox::new("perf");
    populate(&s);
    let big = s.dir("home/big");
    for i in 0..10_000 {
        std::fs::write(big.join(format!("file-{i:05}.txt")), "").unwrap();
    }
    let mut h = harness(&s, ThemeChoice::Night);
    let w = h.window();
    h.focus_window(w, true);
    h.send(Msg::GoPath(w, big.join("file-00000.txt")));
    settle(&mut h);
    assert_eq!(h.program().browser(w).unwrap().columns.last().unwrap().rows.len(), 10_000);
    let _ = h.render(w, 1.0);

    // ↓ through the column: the selection, the path bar, the status and
    // the preview change.
    let mut frames = Vec::new();
    for i in 0..120 {
        let d = frame(&mut h, w, |h| h.named(w, NamedKey::ArrowDown, Modifiers::NONE));
        // The preview's text arrives in the background: let it.
        settle(&mut h);
        if i > 4 {
            frames.push(d);
        }
    }
    let walk = report("↓ in a column of 10,000", frames);
    eprintln!("{}", h.stats().summary());
    h.reset_stats();

    // The wheel over the column.
    let col = h.find_id(w, "col-1").unwrap().center();
    let mut frames = Vec::new();
    for i in 0..120 {
        let d = frame(&mut h, w, |h| h.scroll(w, col, Vec2::new(0.0, 40.0)));
        if i > 4 {
            frames.push(d);
        }
    }
    let wheel = report("scrolling a column of 10,000", frames);

    // The list view, ↓ and the wheel.
    h.send(Msg::SetLayout(w, Layout::List));
    settle(&mut h);
    let _ = h.render(w, 1.0);
    let mut frames = Vec::new();
    for i in 0..120 {
        let d = frame(&mut h, w, |h| h.named(w, NamedKey::ArrowDown, Modifiers::NONE));
        if i > 4 {
            frames.push(d);
        }
    }
    let list = report("↓ in a list of 10,000", frames);
    eprintln!("{}", h.stats().summary());

    // Optimised builds keep the typical frame (the median, which a busy
    // machine moves least) within the budget.
    if !cfg!(debug_assertions) {
        for (name, p50) in [("walk", walk), ("wheel", wheel), ("list", list)] {
            assert!(p50 < Duration::from_millis(4), "{name} p50 {p50:?}");
        }
    }
}

#[test]
fn listing_and_thumbnails_take_what_they_take() {
    let s = Sandbox::new("perf-bg");
    let big = s.dir("big");
    for i in 0..10_000 {
        std::fs::write(big.join(format!("file-{i:05}.txt")), "").unwrap();
    }
    let start = Instant::now();
    let mut es = hako::model::entry::read_folder(&big).unwrap();
    hako::model::entry::sort_entries(&mut es, Default::default());
    eprintln!("hako: listing and sorting 10,000 entries took {:?}", start.elapsed());
    let board = std::path::Path::new("/home/callen/work/aizome-design/boards/aizome-desktop-night@2x.png");
    if board.exists() {
        let pic = s.dir("pics").join("board.png");
        std::fs::copy(board, &pic).unwrap();
        let cache = hako::model::thumbs::Thumbnails { root: s.0.join("thumbs") };
        let start = Instant::now();
        assert!(hako::model::thumbs::make(&cache, &pic, "image/png").is_some());
        eprintln!("hako: thumbnails of a 2880 × 1800 PNG took {:?}", start.elapsed());
    }
}
