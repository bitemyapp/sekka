//! Golden images of Yukimi's main views on the sample system, at 1×
//! (`tests/golden/*.png`; `AIZOME_UPDATE_GOLDENS=1` writes them anew).
//! They're drawn with Aizome's fonts, so they're compared only when
//! `AIZOME_FONT_DIR` says where those are.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aizome_kit::testing::{Harness, assert_golden};
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_yukimi::app::{Changes, Clock, Msg, Page, Runner, Source, Yukimi};

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"))
}

fn settle(h: &mut Harness<Yukimi>, done: impl Fn(&Yukimi) -> bool) {
    let start = Instant::now();
    while !done(h.program()) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
        h.send(Msg::NoteGone(0));
    }
}

fn sample(theme: ThemeChoice) -> Harness<Yukimi> {
    let model = Arc::new(aizome_yukimi::fixture::model());
    let mut app = Yukimi::with(
        Source::Sample(model.clone()),
        Runner::Mock(Arc::new(|_| Vec::new())),
        Clock::Fixed(aizome_yukimi::fixture::NOW, aizome_yukimi::fixture::HOUR),
    );
    app.check = Some(Arc::new(aizome_yukimi::fixture::check(&model)));
    app.index = Some(Arc::new(aizome_yukimi::fixture::index()));
    // Still snow, so the pictures are the same every time.
    Harness::with(app, Appearance { theme, reduced_motion: true, ..Appearance::default() }, false)
}

#[test]
fn goldens() {
    if std::env::var_os("AIZOME_FONT_DIR").is_none() {
        eprintln!("goldens: AIZOME_FONT_DIR isn't set, so the pictures aren't compared");
        return;
    }
    let mut h = sample(ThemeChoice::Night);
    let w = h.window();
    assert_golden(&mut h, w, golden("overview-night"), 1.0);
    h.send(Msg::Show(Page::Installed));
    assert_golden(&mut h, w, golden("installed-night"), 1.0);
    h.send(Msg::Show(Page::Discover));
    assert_golden(&mut h, w, golden("discover-night"), 1.0);
    h.send(Msg::Search("image".into()));
    settle(&mut h, |a| a.found.is_some());
    assert_golden(&mut h, w, golden("search-night"), 1.0);
    h.send(Msg::Open(aizome_yukimi::fixture::index().get("gimp2").cloned().unwrap()));
    assert_golden(&mut h, w, golden("details-night"), 1.0);
    let sheet = h.program().details.as_ref().unwrap().0;
    h.send(Msg::CloseSheet(sheet));
    h.send(Msg::Show(Page::Updates));
    assert_golden(&mut h, w, golden("updates-night"), 1.0);
    h.send(Msg::Show(Page::History));
    h.send(Msg::ToggleGeneration(28));
    settle(&mut h, |a| matches!(a.changes.get(&28), Some(Changes::Ready { .. })));
    assert_golden(&mut h, w, golden("history-night"), 1.0);
    h.send(Msg::Show(Page::Storage));
    assert_golden(&mut h, w, golden("storage-night"), 1.0);
    h.send(Msg::Show(Page::Desktops));
    assert_golden(&mut h, w, golden("desktops-night"), 1.0);
    h.send(Msg::Pick { setting: "calamares.desktops".into(), id: "gnome".into(), on: false });
    h.send(Msg::Pick { setting: "calamares.loginScreen".into(), id: "aizome".into(), on: true });
    assert_golden(&mut h, w, golden("desktops-changed-night"), 1.0);

    let mut h = sample(ThemeChoice::Day);
    let w = h.window();
    assert_golden(&mut h, w, golden("overview-day"), 1.0);
    h.send(Msg::Show(Page::Installed));
    assert_golden(&mut h, w, golden("installed-day"), 1.0);
    h.send(Msg::Show(Page::Updates));
    assert_golden(&mut h, w, golden("updates-day"), 1.0);
    h.send(Msg::Show(Page::Desktops));
    assert_golden(&mut h, w, golden("desktops-day"), 1.0);
}

#[test]
fn comparison_sheets() {
    if std::env::var_os("AIZOME_FONT_DIR").is_none() {
        return;
    }
    for (theme, name) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        let mut h = sample(theme);
        h.send(Msg::Show(Page::Updates));
        h.send(Msg::InspectChanges("sekka".into()));
        settle(&mut h, |a| a.comparison.as_ref().is_some_and(|c| c.details.is_some()));
        let details = aizome_yukimi::fixture::comparison();
        let comparison = h.program_mut().comparison.as_mut().unwrap();
        comparison.link.tooltip = "Saved generations 27 → 28: 1111111 → 65b011c".into();
        comparison.link.generations = Some((27, 28));
        comparison.link.before = details.root.before.clone();
        comparison.link.after = details.root.after.clone().unwrap();
        comparison.link.url = details.root.url().unwrap();
        let sheet = comparison.sheet;
        h.send(Msg::ComparisonLoaded(sheet, Arc::new(details)));
        assert_golden(&mut h, sheet, golden(&format!("comparison-{name}")), 1.0);
        h.send(Msg::ComparisonInto(0));
        assert_golden(&mut h, sheet, golden(&format!("comparison-input-{name}")), 1.0);
        h.send(Msg::ComparisonPackages(true));
        assert_golden(&mut h, sheet, golden(&format!("comparison-packages-{name}")), 1.0);
    }
}
