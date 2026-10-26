//! Pictures of Koe's pages in Night and Day: setting up, ready, listening,
//! and Vocabulary and Teaching. The service's status is made up and no
//! daemon runs. Golden images (`AIZOME_UPDATE_GOLDENS=1` rewrites them).
use aizome_kit::{
    testing::{Harness, assert_golden},
    theme::{Appearance, ThemeChoice},
};
use koe::{
    app::{Koe, Msg, Page, Paths},
    config::Config,
    models,
    protocol::{Phase, Status},
    service::Service,
};
use std::{path::Path, sync::Arc, time::Duration};

const THEMES: [(ThemeChoice, &str); 2] = [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")];

struct Nothing;
impl Service for Nothing {
    fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Koe on a folder of its own, after `prepare` has set it up.
fn harness(theme: ThemeChoice, page: Page, prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Harness<Koe>) {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path());
    let paths = Paths {
        config: dir.path().join("koe.toml"),
        socket: dir.path().join("runtime/control.sock"),
        models: dir.path().join("models"),
        vocabulary: dir.path().join("vocabulary"),
        settings: dir.path().join("settings.toml"),
        model_source: "http://127.0.0.1:9".into(),
        desktop: false,
    };
    let mut app = Koe::new(paths, Arc::new(Nothing));
    app.page = page;
    let mut h = Harness::with(app, Appearance { theme, ..Appearance::default() }, false);
    // The library read, and the absent service heard from.
    for _ in 0..10 {
        h.advance(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(20));
    }
    (dir, h)
}

/// Dictation on with the usual model (as big as the catalog's, but empty)
/// and shortcuts.
fn set_up(dir: &Path) {
    let base = models::find("base").unwrap();
    std::fs::create_dir_all(dir.join("models")).unwrap();
    let model = dir.join("models").join(base.file_name());
    std::fs::File::create(&model).unwrap().set_len(base.bytes).unwrap();
    let config =
        Config { enabled: true, engine: koe::config::Engine { model, ..Default::default() }, ..Default::default() };
    config.save(&dir.join("koe.toml")).unwrap();
    koe::service::ensure_shortcuts(&dir.join("settings.toml"), false).unwrap();
}

fn golden(h: &mut Harness<Koe>, name: &str, theme: &str) {
    let w = h.window();
    assert_golden(h, w, format!("tests/snapshots/{name}-{theme}.png"), 1.0);
}

#[test]
fn setting_up() {
    for (theme, name) in THEMES {
        let (_dir, mut h) = harness(theme, Page::Dictation, |_| {});
        golden(&mut h, "setting-up", name);
    }
}

#[test]
fn ready() {
    for (theme, name) in THEMES {
        let (_dir, mut h) = harness(theme, Page::Dictation, set_up);
        h.send(Msg::Status(Some(Status {
            phase: Phase::Idle,
            model_loaded: true,
            transcription_ms: Some(212),
            gpu: true,
            ..Status::default()
        })));
        golden(&mut h, "ready", name);
    }
}

#[test]
fn listening() {
    for (theme, name) in THEMES {
        let (_dir, mut h) = harness(theme, Page::Dictation, set_up);
        h.send(Msg::Status(Some(Status {
            phase: Phase::Recording,
            model_loaded: true,
            elapsed_ms: 3200,
            level: 0.09,
            ..Status::default()
        })));
        golden(&mut h, "listening", name);
    }
}

#[test]
fn vocabulary() {
    for (theme, name) in THEMES {
        let (_dir, mut h) = harness(theme, Page::Vocabulary, set_up);
        h.send(Msg::Status(Some(Status { model_loaded: true, ..Status::default() })));
        golden(&mut h, "vocabulary", name);
    }
}

/// With `KOE_TEST_SCREENSHOTS=DIR`, the whole Dictation page, ready, in
/// both themes, to look over (not compared).
#[test]
fn whole_page_screenshots() {
    let Some(dir) = std::env::var_os("KOE_TEST_SCREENSHOTS") else { return };
    for (theme, name) in THEMES {
        let (_dir, mut h) = harness(theme, Page::Dictation, set_up);
        h.send(Msg::Status(Some(Status { phase: Phase::Idle, model_loaded: true, ..Status::default() })));
        let w = h.window();
        h.resize(w, aizome_kit::Size::new(900.0, 1500.0));
        h.save_png(w, Path::new(&dir).join(format!("dictation-{name}.png")), 1.0);
    }
}
