//! Pictures of each fixture in Night and Day: golden images rendered by
//! the kit's offscreen renderer (`AIZOME_UPDATE_GOLDENS=1` rewrites them).
//! Made-up machines, conversations and notes; the real model catalog.

use std::time::Duration;

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use tasuke_window::app::{FIXTURES, Tasuke};
use tasuke_window::thumb::Thumb;

const THEMES: [(ThemeChoice, &str); 2] = [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")];

#[test]
fn every_fixture_in_night_and_day() {
    for name in FIXTURES {
        for (theme, theme_name) in THEMES {
            let app = Tasuke::fixture(name).expect("a fixture");
            let mut h = Harness::with(app, Appearance { theme, ..Appearance::default() }, false);
            // Screenshots' thumbnails are made on the kit's pool.
            h.settle(Duration::from_secs(10), |t| t.thumbs.values().all(|t| !matches!(t, Thumb::Loading)));
            let w = h.window();
            assert_golden(&mut h, w, format!("tests/snapshots/{name}-{theme_name}.png"), 1.0);
        }
    }
}
