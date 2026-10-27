//! Typical interactions against the 4 ms frame budget, measured headlessly
//! as `docs/performance.md` describes: the update (a settings change saves
//! the file), the view, layout and diff, and rasterising the damage at 2×.
//! Asserted only in release builds; printed always.

mod common;

use std::time::{Duration, Instant};

use aizome_kit::layout::Rect;
use aizome_kit::testing::Harness;
use settei::{AppMsg, Settei};

fn frame(h: &mut Harness<Settei>, act: impl FnOnce(&mut Harness<Settei>)) -> Duration {
    let w = h.window();
    h.take_damage(w);
    let start = Instant::now();
    act(h);
    let damage = h.take_damage(w);
    let mut bounds: Option<Rect> = None;
    for d in damage {
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
        "settei {name}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
    p(0.95)
}

#[test]
fn typical_interactions_fit_the_frame_budget() {
    let (mut h, _) = common::open("budget", "mouse");
    let w = h.window();
    let _ = h.render(w, 2.0);

    // A switch flipped: the setting saved, the row redrawn.
    let mut toggles = Vec::new();
    for i in 0..40 {
        let at = h.find(w, "Natural scrolling").expect("the row");
        let x = h.size(w).width - 50.0;
        let t = frame(&mut h, |h| h.click(w, aizome_kit::layout::Point::new(x, at.center().y)));
        if i > 2 {
            toggles.push(t);
        }
    }
    let toggle_p95 = report("a switch", toggles);

    // Moving between panes from the sidebar.
    let mut moves = Vec::new();
    for (i, pane) in
        ["Keyboard", "Mouse", "Appearance", "General", "Trackpad", "Users & Groups"].iter().cycle().take(36).enumerate()
    {
        let at = h.find(w, pane).expect("in the sidebar");
        let t = frame(&mut h, |h| h.click(w, at.center()));
        if i > 5 {
            moves.push(t);
        }
    }
    let pane_p95 = report("another pane", moves);

    // Typing in the search field.
    let mut keys = Vec::new();
    let mut q = String::new();
    for (i, c) in "keyboard repeat ".chars().cycle().take(64).enumerate() {
        q.push(c);
        if q.len() > 16 {
            q.clear();
        }
        let query = q.clone();
        let t = frame(&mut h, |h| h.send(AppMsg::Search(query)));
        if i > 3 {
            keys.push(t);
        }
    }
    let search_p95 = report("a key in search", keys);

    if !cfg!(debug_assertions) {
        assert!(toggle_p95 < Duration::from_millis(4), "switch p95 {toggle_p95:?}");
        assert!(search_p95 < Duration::from_millis(4), "search p95 {search_p95:?}");
        assert!(pane_p95 < Duration::from_millis(4), "another pane p95 {pane_p95:?}");
    }
}
