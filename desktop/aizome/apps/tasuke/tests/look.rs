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

/// The Compare page scrolled to the selected model's details: what can be
/// done with it (download, use, connect its account) and its timing.
#[test]
fn compare_details_in_night_and_day() {
    for name in ["compare", "compare-cloud"] {
        for (theme, theme_name) in THEMES {
            let app = Tasuke::fixture(name).expect("a fixture");
            let mut h = Harness::with(app, Appearance { theme, ..Appearance::default() }, false);
            let w = h.window();
            h.scroll(w, aizome_kit::layout::Point::new(1166.0, 400.0), aizome_kit::layout::Vec2::new(0.0, 4000.0));
            h.settle(Duration::from_millis(500), |_| false);
            assert_golden(&mut h, w, format!("tests/snapshots/{name}-details-{theme_name}.png"), 1.0);
        }
    }
}

/// Models with a Claude account connected and its model in use, scrolled to
/// the accounts.
#[test]
fn cloud_accounts_in_night_and_day() {
    for (theme, theme_name) in THEMES {
        let app = Tasuke::fixture("cloud-home").expect("a fixture");
        let mut h = Harness::with(app, Appearance { theme, ..Appearance::default() }, false);
        let w = h.window();
        h.send(tasuke_window::app::Msg::Show(tasuke_window::app::Page::Models));
        h.scroll(w, aizome_kit::layout::Point::new(1166.0, 400.0), aizome_kit::layout::Vec2::new(0.0, 8000.0));
        h.settle(Duration::from_millis(500), |_| false);
        assert_golden(&mut h, w, format!("tests/snapshots/models-accounts-{theme_name}.png"), 1.0);
    }
}
