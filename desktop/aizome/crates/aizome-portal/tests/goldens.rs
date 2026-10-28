//! The chooser's pictures, Night and Day: Open with filters, Save with a
//! choice, and choosing a folder. Dates are reckoned in UTC from a fixed
//! present, and the sidebar's disks are fixed, so the pictures are the
//! same on any machine.

use std::path::{Path, PathBuf};
use std::sync::Once;

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_portal::chooser::{Chooser, Msg};
use aizome_portal::request::{Choice, Filter, Mode, Request, Rule};
use hako::model::places::{Place, PlaceKind};

mod common;
use common::*;

/// 17 October 2026, 23:33 UTC: the fixture's files are from 21:42.
const NOW: i64 = 1_792_280_000;

fn utc() {
    static ONCE: Once = Once::new();
    // SAFETY: set once, before any thread of this test binary reads the
    // environment's time zone (the first test to start sets it).
    ONCE.call_once(|| unsafe {
        std::env::set_var("TZ", "UTC");
        libc_tzset();
    });
}

unsafe extern "C" {
    #[link_name = "tzset"]
    fn libc_tzset();
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

fn pictured(home: &Home, request: Request, theme: ThemeChoice) -> Harness<Chooser> {
    utc();
    let mut c = Chooser::with_home(request, home.path(".state/goldens.json"), home.path(""));
    c.ask_desk = false;
    c.now = Some(NOW);
    c.places.disk = Some(Place { name: "nvme0".into(), path: "/".into(), kind: PlaceKind::Disk });
    c.places.volumes = vec![Place { name: "TRAVEL".into(), path: "/run/media/travel".into(), kind: PlaceKind::Volume }];
    c.places.desk = Some(("Desk: Taxes".into(), home.path("Documents/Taxes")));
    let mut h = Harness::with(c, Appearance { theme, ..Appearance::default() }, false);
    settle(&mut h);
    h
}

fn pictures(h: &mut Harness<Chooser>, name: &str) {
    let w = h.window();
    h.save_png(w, Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("chooser-{name}@2x.png")), 2.0);
    assert_golden(h, w, golden(name), 1.0);
}

#[test]
fn goldens() {
    let home = Home::new("goldens");
    for (theme, t) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut r = request(Mode::Open, &home.path("Pictures"));
        r.multiple = true;
        r.filters = vec![
            Filter { name: "Images".into(), rules: vec![Rule::Mime("image/*".into())] },
            Filter { name: "All Files".into(), rules: vec![Rule::Glob("*".into())] },
        ];
        let mut h = pictured(&home, r, theme);
        h.send(Msg::Select(vec![0]));
        pictures(&mut h, &format!("open-{t}"));

        let mut r = request(Mode::Save, &home.path("Documents"));
        r.title = "Export Summary".into();
        r.current_name = Some("summary.md".into());
        r.choices = vec![
            Choice {
                id: "format".into(),
                label: "Format".into(),
                options: vec![("md".into(), "Markdown".into()), ("txt".into(), "Plain text".into())],
                default: "md".into(),
            },
            Choice { id: "toc".into(), label: "Table of contents".into(), options: vec![], default: "true".into() },
        ];
        let mut h = pictured(&home, r, theme);
        pictures(&mut h, &format!("save-{t}"));

        let mut r = request(Mode::Open, &home.path("Documents"));
        r.directory = true;
        let mut h = pictured(&home, r, theme);
        pictures(&mut h, &format!("folder-{t}"));
    }
}
