//! What the login screen's frames cost, against the 4 ms budget: from the
//! key or the tick to its damage rasterised at 2×, over the design's
//! wallpaper at 2880 × 1800.
//! `cargo test -p aizome-greeter --release --test budget -- --nocapture`;
//! the numbers are in docs/performance.md.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aizome_greeter::app::{Greeter, Msg};
use aizome_greeter::config::{Config, State};
use aizome_greeter::greetd::{AuthMessageType, ErrorType, Response};
use aizome_greeter::sessions::{Kind, Session};
use aizome_greeter::users::User;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

fn output() -> aizome_kit::Output {
    aizome_kit::Output {
        id: 1,
        name: "eDP-1".into(),
        connector: Some("eDP-1".into()),
        description: None,
        make: "SDC".into(),
        model: "ATNA40HQ10-0".into(),
        frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
        scale: 2.0,
        integer_scale: 2,
        physical_size: None,
        refresh_hz: 120.0,
        work_area: None,
    }
}

fn wallpaper() -> Option<PathBuf> {
    let p =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../aizome-design/reference/wallpapers/da-nang-at-night.jpg");
    p.exists().then_some(p)
}

fn greeter() -> (Harness<Greeter>, WindowKey) {
    let user = |n: &str, r: &str, uid| User { name: n.into(), real_name: r.into(), uid, icon: None };
    let session = |id: &str, name: &str| Session {
        id: id.into(),
        name: name.into(),
        comment: String::new(),
        exec: vec![id.into()],
        desktop_names: vec![name.into()],
        kind: Kind::Wayland,
        path: PathBuf::from(id),
    };
    let cfg = Config { wallpaper_night: wallpaper(), wallpaper_day: wallpaper(), ..Config::default() };
    let g = Greeter::with(
        cfg,
        State::default(),
        vec![user("callen", "Chris Allen", 1000), user("alice", "Alice Kimura", 1001), user("bob", "Bob Tanaka", 1002)],
        vec![session("aizome", "Aizome"), session("tatami", "Tatami")],
        "ganymede".into(),
        false,
        false,
    );
    let mut h = Harness::new(g);
    h.set_outputs(vec![output()]);
    for k in h.windows() {
        h.resize(k, Size::new(1440.0, 900.0));
    }
    if let Some(path) = wallpaper() {
        let rgba = aizome_face::wallpaper::prepare(&path, 2880, 1800);
        h.send(Msg::Wallpaper { output: 1, path, size: (2880, 1800), rgba });
    }
    let w = h.program().main_window().unwrap();
    let _ = h.render(w, 2.0);
    h.take_damage(w);
    (h, w)
}

/// The damage drawn at 2×, and how much of the window it was.
fn draw(h: &mut Harness<Greeter>, w: WindowKey) -> f64 {
    let mut bounds: Option<Rect> = None;
    for d in h.take_damage(w) {
        bounds = Some(bounds.map_or(d, |b| b.union(d)));
    }
    match bounds {
        Some(b) => {
            let _ = h.render_rect(w, b, 2.0);
            b.width() * b.height() / (1440.0 * 900.0)
        }
        None => 0.0,
    }
}

fn report(name: &str, mut v: Vec<Duration>, area: f64) -> f64 {
    v.sort();
    let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize].as_secs_f64() * 1e3;
    eprintln!(
        "greeter {name}: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms, {:.1}% of the window drawn (n={}, at 2×)",
        p(0.5),
        p(0.95),
        p(1.0),
        area * 100.0,
        v.len()
    );
    p(0.95)
}

fn measure(
    h: &mut Harness<Greeter>,
    w: WindowKey,
    n: usize,
    mut step: impl FnMut(&mut Harness<Greeter>, usize),
) -> (Vec<Duration>, f64) {
    let mut v = Vec::with_capacity(n);
    let mut area = 0.0;
    for i in 0..n {
        h.take_damage(w);
        let start = Instant::now();
        step(h, i);
        area += draw(h, w);
        v.push(start.elapsed());
    }
    (v, area / n as f64)
}

#[test]
fn the_login_screens_frames_fit_the_budget() {
    let (mut h, w) = greeter();
    // The row of people: → moves the selection.
    let (v, a) = measure(&mut h, w, 60, |h, _| h.named(w, NamedKey::ArrowRight, Modifiers::NONE));
    let row = report("moving through the people", v, a);
    // Picking someone, then typing their password.
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    h.send(Msg::Greetd(Response::AuthMessage {
        auth_message_type: AuthMessageType::Secret,
        auth_message: "Password: ".into(),
    }));
    let _ = h.render(w, 2.0);
    let (v, a) = measure(&mut h, w, 60, |h, i| {
        if i % 20 == 19 {
            h.named(w, NamedKey::Backspace, Modifiers::NONE);
        } else {
            h.type_text(w, "x");
        }
    });
    let typing = report("typing a password", v, a);
    // A wrong one: the shake's frames.
    let mut shakes = Vec::new();
    let mut area = 0.0;
    for _ in 0..10 {
        // A password sent, and greetd says no.
        h.type_text(w, "wrong");
        h.named(w, NamedKey::Enter, Modifiers::NONE);
        h.send(Msg::Greetd(Response::Error {
            error_type: ErrorType::AuthError,
            description: "pam_authenticate: AUTH_ERR".into(),
        }));
        let (v, a) = measure(&mut h, w, 8, |h, _| h.advance(Duration::from_millis(30)));
        shakes.extend(v);
        area += a;
        h.send(Msg::Greetd(Response::Success));
        h.send(Msg::Greetd(Response::AuthMessage {
            auth_message_type: AuthMessageType::Secret,
            auth_message: "Password: ".into(),
        }));
        h.advance(Duration::from_millis(400));
    }
    let shake = report("a wrong password's shake", shakes, area / 10.0);
    // The clock's minute.
    let (v, a) = measure(&mut h, w, 30, |h, _| h.send(Msg::Tick));
    let tick = report("the clock's tick", v, a);
    // Unoptimised builds and loaded machines get room; the release numbers
    // are what docs/performance.md holds to 4 ms.
    let limit = if cfg!(debug_assertions) { 60.0 } else { 4.0 };
    for (what, p95) in [("the row", row), ("typing", typing), ("the shake", shake), ("the tick", tick)] {
        assert!(p95 < limit, "{what}: p95 {p95:.2} ms over {limit} ms");
    }
}
