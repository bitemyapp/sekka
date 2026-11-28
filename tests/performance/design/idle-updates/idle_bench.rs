//! Supplemental measurements of the actual messages changed by PR #332.
mod common;
use common::*;
use aizome_shell::shell::Msg;
use aizome_shell::clock::Local;
use std::time::Instant;

fn cpu_ns() -> u64 {
    let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut t) }, 0);
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

#[test]
fn measure_changed_messages() {
    let kind = std::env::var("IDLE_BENCH_KIND").unwrap_or("meter".into());
    let n: usize = std::env::var("IDLE_BENCH_N").unwrap_or("1000".into()).parse().unwrap();
    let mut h = scene(false);
    with_mail_and_shelf(&mut h);
    h.program_mut().settings.appearance.theme = aizome_config::ThemeChoice::Night;
    h.program_mut().theme_day = false;
    let bar = window_titled(&h, "Menu bar");
    let db = window_titled(&h, "Deskbar");
    let size = aizome_shell::deskbar::size(&h.program().deskbar_model());
    h.resize(db, size);
    let surfaces = [bar, db];
    let paint = |h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        for w in surfaces {
            for r in h.take_damage(w) { let _ = h.render_rect(w, r, 2.0); }
        }
    };
    paint(&mut h);
    let mut wall = Vec::with_capacity(n);
    let mut cpu = Vec::with_capacity(n);
    let mut layouts = 0;
    let start = Instant::now();
    for i in 0..n+100 {
        // Put the previous minute on screen outside the timed region so
        // TimeChanged really changes a label even in a fast replay.
        if kind == "clock" {
            h.program_mut().now.minute = (Local::now().minute + 1) % 60;
            h.send(Msg::Nothing);
            paint(&mut h);
        }
        let msg = if kind == "clock" { Msg::TimeChanged } else {
            let mut m = meters();
            m.cores[i % 8] = (i * 13 % 100) as f32;
            Msg::Meters(m)
        };
        h.reset_stats();
        let c = cpu_ns();
        let t = Instant::now();
        h.send(msg);
        paint(&mut h);
        let elapsed = t.elapsed().as_nanos() as u64;
        let used = cpu_ns() - c;
        if i >= 100 {
            wall.push(elapsed);
            cpu.push(used);
            layouts += h.stats().view_layout.len();
        }
    }
    println!("BENCH_JSON {}", serde_json::json!({
        "kind":kind,"n":n,"wall_ns":wall,"cpu_ns":cpu,
        "view_layouts":layouts,"run_s":start.elapsed().as_secs_f64()
    }));
}
