//! Golden images of Hako's main views, in Night and Day: columns, a list
//! and icons. `AIZOME_UPDATE_GOLDENS=1` writes them anew.
//!
//! The pictures can't change with the day or the machine: the clock is
//! fixed, times are shown in UTC, the sandbox shows no free space, and
//! thumbnails are waited for.

mod common;

use std::path::PathBuf;
use std::sync::Once;
use std::time::{Duration, Instant};

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::ThemeChoice;
use common::*;
use hako::ui::fileview::Layout;
use hako::ui::{Hako, Msg, Options};

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name)
}

fn hako(s: &Sandbox, theme: ThemeChoice) -> Harness<Hako> {
    utc();
    let mut opts = Options::sandbox(s.path());
    opts.home = s.home();
    opts.now = Some(NOW);
    let mut h = Harness::with(Hako::new(opts), appearance(theme), false);
    settle(&mut h);
    h
}

/// Settle, and wait for thumbnails too.
fn settle_all(h: &mut Harness<Hako>) {
    settle(h);
    let until = Instant::now() + Duration::from_secs(5);
    while h.program().thumbs_pending() && Instant::now() < until {
        h.send(Msg::Nop);
        std::thread::sleep(Duration::from_millis(10));
    }
    h.send(Msg::Nop);
}

fn shoot(theme: ThemeChoice, suffix: &str) {
    let s = Sandbox::new(&format!("golden-{suffix}"));
    populate(&s);
    let mut h = hako(&s, theme);
    let w = h.window();
    h.focus_window(w, true);
    // Columns: Home ▸ work ▸ sidestep (a folder selected: its contents).
    h.send(Msg::GoPath(w, s.home().join("work/sidestep")));
    settle_all(&mut h);
    assert_golden(&mut h, w, golden(&format!("columns-{suffix}.png")), 1.0);
    // A list: Downloads.
    h.send(Msg::SetLayout(w, Layout::List));
    h.send(Msg::GoPath(w, s.home().join("Downloads/release-notes.pdf")));
    settle_all(&mut h);
    assert_golden(&mut h, w, golden(&format!("list-{suffix}.png")), 1.0);
    // Icons: Pictures, with thumbnails.
    h.send(Msg::SetLayout(w, Layout::Icons));
    h.send(Msg::GoPath(w, s.home().join("Pictures/wallpaper-draft.png")));
    settle_all(&mut h);
    assert_golden(&mut h, w, golden(&format!("icons-{suffix}.png")), 1.0);
}

#[test]
fn night() {
    shoot(ThemeChoice::Night, "night");
}

#[test]
fn day() {
    shoot(ThemeChoice::Day, "day");
}
