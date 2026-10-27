//! How long Shashin's typical interactions take, from the event to the
//! frame rasterised at 2× (update, view, layout, diff, and painting what
//! changed through the offscreen renderer), and Shashin's own part alone:
//! `cargo test -p aizome-shashin --release --test budget -- --nocapture`
//! prints them. The numbers are in docs/performance.md.

use std::time::{Duration, Instant};

use aizome_kit::WindowKey;
use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;
use aizome_kit::theme::ThemeChoice;
use aizome_shashin::{Msg, Shashin};

mod common;
use common::*;

/// Rasterise what the last event damaged, at 2×: the share of the window.
fn draw(h: &mut Harness<Shashin>, w: WindowKey) -> f64 {
    // The damage's rectangles, as the window's renderer draws them (as a
    // rule one, the picture's).
    let damage = h.take_damage(w);
    let size = h.size(w);
    let _ = h.render_damage(w, &damage, 2.0);
    let area: f64 = damage.iter().map(|r: &Rect| r.width() * r.height()).sum();
    (area / (size.width * size.height)).min(1.0)
}

struct Times {
    frame: Vec<Duration>,
    own: Vec<Duration>,
    area: f64,
}

fn report(name: &str, t: &mut Times) {
    let p = |v: &mut Vec<Duration>, q: f64| {
        v.sort();
        v[((v.len() - 1) as f64 * q) as usize].as_secs_f64() * 1e3
    };
    let n = t.frame.len();
    eprintln!(
        "shashin {name}: frame p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms; Shashin's part p50 {:.2} ms, p95 {:.2} ms; {:.0}% of the window drawn (n={n}, at 2×)",
        p(&mut t.frame, 0.5),
        p(&mut t.frame, 0.95),
        p(&mut t.frame, 1.0),
        p(&mut t.own, 0.5),
        p(&mut t.own, 0.95),
        t.area / n as f64 * 100.0
    );
}

/// Time `act` `n` times (after two to warm up): its frame, and its own
/// part (the event, update, view and layout) alone.
fn measure(r: &mut Rig, n: usize, mut act: impl FnMut(&mut Rig, usize), mut between: impl FnMut(&mut Rig)) -> Times {
    let w = r.h.window();
    let _ = r.h.render(w, 2.0);
    let mut t = Times { frame: Vec::new(), own: Vec::new(), area: 0.0 };
    for i in 0..n + 2 {
        between(r);
        r.h.take_damage(w);
        let start = Instant::now();
        act(r, i);
        let own = start.elapsed();
        let area = draw(&mut r.h, w);
        if i >= 2 {
            t.frame.push(start.elapsed());
            t.own.push(own);
            t.area += area;
        }
    }
    t
}

#[test]
fn typical_interactions() {
    let d = scratch("budget");
    write_folder(&d);
    std::fs::write(d.join("DSCF0001.jpg"), camera_jpeg(3000, 2000)).unwrap();
    let mut r = rig_in(d, ThemeChoice::Night, "DSCF0001.jpg");
    let w = r.h.window();
    // A large window on the laptop's panel: 2880 × 1740 pixels of picture.
    r.h.resize(w, Size::new(1440.0, 900.0));
    settle(&mut r, 2.0);

    // Fitted, at rest: the drawing made for the fit, one to one.
    let mut t = measure(
        &mut r,
        30,
        |r, _| r.h.send(Msg::Fit),
        |r| {
            r.h.send(Msg::ActualSize);
            settle(r, 2.0);
        },
    );
    report("showing a fitted photo (its drawing made, then drawn)", &mut t);

    // Panning at 100% by dragging: each step a full redraw of the view.
    r.h.send(Msg::ActualSize);
    settle(&mut r, 2.0);
    r.h.press(w, Point::new(700.0, 450.0), PointerButton::Left, 1);
    let mut t = measure(
        &mut r,
        60,
        |r, i| {
            let x = 700.0 - (i as f64 * 7.0) % 300.0;
            r.h.move_to(w, Point::new(x, 450.0 - (i % 40) as f64 * 3.0));
        },
        |_| {},
    );
    r.h.release(w, Point::new(400.0, 450.0), PointerButton::Left);
    report("panning a photo at 100%", &mut t);

    // Pinching: drawn from what's there (nearest pixel) while the drawing
    // for the new zoom is made.
    settle(&mut r, 2.0);
    let mut t = measure(
        &mut r,
        40,
        |r, i| r.h.pinch(w, Point::new(720.0, 450.0), if i < 20 { 1.03 } else { 1.0 / 1.03 }),
        |_| {},
    );
    report("pinching", &mut t);

    // ❖+ and ❖−, each settling before the next.
    let mut t = measure(
        &mut r,
        20,
        |r, i| {
            if i % 2 == 0 {
                r.h.send(Msg::ZoomIn);
            } else {
                r.h.send(Msg::ZoomOut);
            }
        },
        |r| settle(r, 2.0),
    );
    report("a zoom step", &mut t);

    // → through the folder, the next picture read ahead.
    r.h.send(Msg::Fit);
    let mut t = measure(&mut r, 20, |r, _| r.h.named(w, NamedKey::ArrowRight, Modifiers::NONE), |r| settle(r, 2.0));
    report("the next picture", &mut t);
}
