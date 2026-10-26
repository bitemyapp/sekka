//! What the shell's typical frames cost, headlessly: the message's update,
//! the views, layout and diff, and rasterising the damage at 2× through the
//! offscreen renderer — the clock's tick, a meter moving, a key in the
//! palette, a window opening (the Deskbar and the bar), a panel's slider.
//! Run with `--release -- --nocapture` for the numbers in
//! docs/performance.md; a debug build only checks that frames stay small.

mod common;

use std::time::{Duration, Instant};

use aizome_kit::prelude::*;
use aizome_kit::testing::Harness;
use aizome_shell::shell::{Msg, Shell};
use common::*;

/// Times `n` rounds of `step` (which sends something), each followed by
/// rasterising the damage of `windows`: p50, p95, max in µs.
fn measure(
    h: &mut Harness<Shell>,
    windows: &[WindowKey],
    n: usize,
    mut step: impl FnMut(&mut Harness<Shell>, usize),
) -> (u128, u128, u128) {
    let mut times = Vec::new();
    for i in 0..n {
        for w in windows {
            h.take_damage(*w);
        }
        let t = Instant::now();
        step(h, i);
        for w in windows {
            for r in h.take_damage(*w) {
                let _ = h.render_rect(*w, r, 2.0);
            }
        }
        times.push(t.elapsed().as_micros());
    }
    times.sort_unstable();
    (times[n / 2], times[n * 95 / 100], *times.last().unwrap())
}

fn report(name: &str, (p50, p95, max): (u128, u128, u128)) {
    println!(
        "{name}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        p50 as f64 / 1000.0,
        p95 as f64 / 1000.0,
        max as f64 / 1000.0
    );
    // The budget, with room for a loaded machine and unoptimised code.
    let limit = if cfg!(debug_assertions) { 40_000 } else { 4_000 };
    assert!(p95 < limit, "{name}: p95 {p95} µs");
}

#[test]
fn typical_frames_fit_the_budget() {
    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    let bar = window_titled(&h, "Menu bar");
    let db = window_titled(&h, "Deskbar");
    let size = aizome_shell::deskbar::size(&h.program().deskbar_model());
    h.resize(db, size);
    let surfaces = [bar, db];

    report(
        "clock tick",
        measure(&mut h, &surfaces, 60, |h, i| {
            // (Msg::Tick reads the real clock; set the minute instead.)
            h.program_mut().now.minute = (i % 60) as u32;
            h.send(Msg::Nothing);
            let _ = Duration::ZERO;
        }),
    );

    report(
        "meter",
        measure(&mut h, &surfaces, 60, |h, i| {
            let mut m = meters();
            m.cores[i % 8] = (i * 13 % 100) as f32;
            h.send(Msg::Meters(m));
        }),
    );

    report(
        "window opens and closes",
        measure(&mut h, &surfaces, 40, |h, i| {
            use aizome_ipc::{Event, WindowInfo};
            use aizome_shell::ipc::IpcEvent;
            let id = 100 + i as u64;
            if i % 2 == 0 {
                let w = WindowInfo {
                    id,
                    app_id: "foot".into(),
                    title: format!("shell {i}"),
                    desk: Some(3),
                    ..Default::default()
                };
                h.send(Msg::Ipc(IpcEvent::Event(Event::WindowOpened { window: w })));
            } else {
                h.send(Msg::Ipc(IpcEvent::Event(Event::WindowClosed { id: id - 1 })));
            }
        }),
    );

    h.send(Msg::Run(aizome_shell::actions::Action::Palette { scope: None, query: String::new() }));
    let p = window_titled(&h, "Palette");
    let letters = ["f", "i", "n", "d", " ", "r", "e", "p"];
    report(
        "palette key",
        measure(&mut h, &[p], 40, |h, i| {
            if i % 9 == 8 {
                h.named(p, aizome_kit::sidestep_ui::NamedKey::Backspace, aizome_kit::sidestep_ui::Modifiers::SUPER);
            } else {
                h.type_text(p, letters[i % 8]);
            }
        }),
    );
    report(
        "palette arrow",
        measure(&mut h, &[p], 40, |h, i| {
            let k = if i % 3 == 2 {
                aizome_kit::sidestep_ui::NamedKey::ArrowUp
            } else {
                aizome_kit::sidestep_ui::NamedKey::ArrowDown
            };
            h.named(p, k, aizome_kit::sidestep_ui::Modifiers::NONE);
        }),
    );
}

#[test]
fn the_palettes_first_opening_after_warming() {
    // A fresh shell, its palette's text laid out (and its glyphs rasterized,
    // on a thread of their own) ahead at idle as the live shell does two
    // seconds after starting, and a second later the palette drawn ahead,
    // hidden, as it would open: its update, view and layout, and its whole
    // surface rasterised offscreen at 2×, in bands, at idle. Then the first
    // opening: update, view, layout and diff, and its first frame at 2× as
    // the window draws it (the picture drawn ahead copied in, and what
    // changed since drawn over it); then closed, drawn ahead again at idle,
    // and opened again.
    let mut h = scene(false);
    let theme = h.theme().clone();
    let t = Instant::now();
    let glyphs = h.program_mut().warm_palette(&theme);
    let warming = t.elapsed();
    if let Some(glyphs) = glyphs {
        let _ = glyphs.join();
    }
    h.program_mut().palette_ahead.on = true;
    let t = Instant::now();
    h.send(Msg::PreparePalette(h.program().palette_ahead.asked));
    h.advance(Duration::ZERO);
    let drawing_ahead = t.elapsed();
    let p = window_titled(&h, "Palette");
    assert!(h.picture(p).is_some(), "drawn ahead");
    let open = |h: &mut Harness<Shell>| {
        let t = Instant::now();
        h.send(Msg::Run(aizome_shell::actions::Action::Palette { scope: None, query: String::new() }));
        let _ = h.render_frame(p, 2.0);
        t.elapsed()
    };
    let first = open(&mut h);
    h.send(Msg::Palette(aizome_shell::palette::PaletteEvent::Ui(aizome_shell::palette::view::PaletteMsg::Close)));
    h.advance(aizome_shell::palette::AHEAD_AFTER);
    let again = open(&mut h);
    println!(
        "palette: warming {:.2} ms and drawing ahead {:.2} ms (once, at idle), first opening {:.2} ms, again {:.2} ms",
        warming.as_secs_f64() * 1e3,
        drawing_ahead.as_secs_f64() * 1e3,
        first.as_secs_f64() * 1e3,
        again.as_secs_f64() * 1e3
    );
    let limit = if cfg!(debug_assertions) { Duration::from_millis(200) } else { Duration::from_millis(20) };
    assert!(first < limit, "{first:?}");
}
