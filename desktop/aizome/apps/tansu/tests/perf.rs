//! What Tansu's typical interactions cost, from the input to its pixels:
//! the update, the view, layout and diff, and rasterising what changed, at
//! 2×. Prints percentiles (for docs/performance.md) and holds them to the
//! 4 ms budget in an optimised build.

use std::rc::Rc;
use std::time::{Duration, Instant};

use aizome_kit::Point;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use tansu::app::{Msg, Tansu};
use tansu::mock::Mock;
use tansu::plan::Action;

fn frame(h: &mut Harness<Tansu>, act: impl FnOnce(&mut Harness<Tansu>)) -> Duration {
    let w = h.window();
    h.take_damage(w);
    let start = Instant::now();
    act(h);
    // Each damaged rectangle, as the window's renderer redraws them (a
    // selection changes strips at the top, middle and bottom: their union
    // would be the whole window).
    let damage = h.take_damage(w);
    let _ = h.render_damage(w, &damage, 2.0);
    start.elapsed()
}

fn report(what: &str, mut frames: Vec<Duration>) -> Duration {
    frames.sort();
    let p = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
    eprintln!(
        "tansu {what}: p50 {:?} p95 {:?} max {:?} (n={}, at 2x)",
        p(0.5),
        p(0.95),
        frames.last().unwrap(),
        frames.len()
    );
    p(0.5)
}

#[test]
fn typical_interactions_fit_the_frame_budget() {
    let mut h = Harness::new(Tansu::new(Rc::new(Mock::default())));
    let w = h.window();
    let _ = h.render(w, 2.0);

    // Clicking along the partition map, and arrows across it.
    let map = h.find_id(w, "partition-map").expect("the map");
    let xs = [0.004, 0.3, 0.8, 0.95, 0.99];
    let mut clicks = Vec::new();
    for round in 0..12 {
        for x in xs {
            let at = Point::new(map.x0 + map.width() * x, map.center().y);
            let d = frame(&mut h, |h| h.click(w, at));
            if round > 1 {
                clicks.push(d);
            }
        }
    }
    let mut arrows = Vec::new();
    for round in 0..20 {
        let key = if round % 2 == 0 { NamedKey::ArrowRight } else { NamedKey::ArrowLeft };
        arrows.push(frame(&mut h, |h| h.named(w, key, Modifiers::NONE)));
    }

    // Switching drives in the sidebar.
    let keys: Vec<String> = h.program().drives.iter().map(|d| d.key.clone()).collect();
    let mut drives = Vec::new();
    for round in 0..6 {
        for k in &keys {
            let k = k.clone();
            let d = frame(&mut h, |h| h.send(Msg::SelectDrive(k)));
            if round > 0 {
                drives.push(d);
            }
        }
    }

    // Queueing and undoing a deletion (the map, table, strip and status
    // all change).
    let stick = keys.iter().find(|k| k.contains("cruzer")).unwrap().clone();
    h.send(Msg::SelectDrive(stick));
    let i = h.program().layout.segs.iter().position(|s| s.short_name() == "sdc1").unwrap();
    h.send(Msg::SelectSeg(i));
    let mut queue = Vec::new();
    for _ in 0..20 {
        queue.push(frame(&mut h, |h| h.send(Msg::Do(Action::Delete))));
        assert_eq!(h.program().plan.len(), 1, "queued");
        assert!(
            h.program().selected().is_some_and(|i| h.program().layout.segs[i].is_free()),
            "the space it leaves is selected"
        );
        queue.push(frame(&mut h, |h| h.send(Msg::Undo)));
        assert!(h.program().plan.is_empty(), "undone");
        assert_eq!(
            h.program().selected().map(|i| h.program().layout.segs[i].short_name()).as_deref(),
            Some("sdc1"),
            "and it is selected again"
        );
    }

    eprintln!("{}", h.stats().summary());
    let worst = [
        report("map click", clicks),
        report("map arrow", arrows),
        report("drive switch", drives),
        report("queue/undo", queue),
    ];
    if !cfg!(debug_assertions) {
        // The median: the tail moves with whatever else the machine is doing
        // (docs/performance.md has both).
        for p50 in worst {
            assert!(p50 < Duration::from_millis(4), "p50 {p50:?}");
        }
    }
}
