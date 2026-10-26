//! Pictures of Myaku's main views in Night and Day, from made-up readings
//! (no process on this computer is shown, or signalled): golden images
//! (`AIZOME_UPDATE_GOLDENS=1` rewrites them).

use std::sync::Arc;

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use myaku::app::{Msg, Myaku, Request, Sel, Show};
use myaku::fixture;
use myaku::model::Lens;

const THEMES: [(ThemeChoice, &str); 2] = [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")];

/// Myaku after two minutes of readings.
fn harness(theme: ThemeChoice) -> Harness<Myaku> {
    let (signals, _) = Myaku::recorder();
    let mut h = Harness::with(
        Myaku::with_frame(Arc::new(fixture::frame(420, 1)), signals, Request::default()),
        Appearance { theme, ..Appearance::default() },
        false,
    );
    for tick in 2..=120 {
        h.send(Msg::Frame(Arc::new(fixture::frame(420, tick))));
    }
    h
}

fn golden(h: &mut Harness<Myaku>, name: &str, theme: &str) {
    let w = h.window();
    assert_golden(h, w, format!("tests/snapshots/{name}-{theme}.png"), 1.0);
}

#[test]
fn applications_by_cpu() {
    for (theme, name) in THEMES {
        let mut h = harness(theme);
        golden(&mut h, "cpu", name);
    }
}

#[test]
fn an_application_opened_with_the_inspector() {
    for (theme, name) in THEMES {
        let mut h = harness(theme);
        let firefox = h.program().frame.groups.iter().find(|g| g.name == "Firefox").unwrap().key;
        h.send(Msg::Toggle(firefox));
        h.send(Msg::Select(Sel::Group(firefox)));
        h.send(Msg::Inspector(true));
        h.send(Msg::Lens(Lens::Memory));
        golden(&mut h, "memory-inspector", name);
    }
}

#[test]
fn all_processes_by_disk() {
    let mut h = harness(ThemeChoice::Night);
    h.send(Msg::Show(Show::All));
    h.send(Msg::Lens(Lens::Disk));
    golden(&mut h, "disk-all", "night");
}
