//! Golden images of Settei's main views, Night and Day, through the
//! offscreen renderer (`AIZOME_UPDATE_GOLDENS=1` writes them anew). They
//! were looked at when written.

mod common;

use std::path::PathBuf;

use aizome_kit::testing::{Harness, assert_golden};
use settei::{AppMsg, Settei, Start};

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens").join(format!("{name}.png"))
}

/// Compare the window with the golden `name`: skipped without Aizome's
/// fonts (`AIZOME_FONT_DIR`), which the goldens are drawn in.
fn compare(h: &mut Harness<Settei>, name: &str) {
    if !h.theme().has_ui_font() {
        eprintln!("skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR=~/work/.aizome-fonts)");
        return;
    }
    let w = h.window();
    assert_golden(h, w, golden(name), 1.0);
}

fn open(target: &str, day: bool) -> Harness<Settei> {
    let dir = common::temp_dir(target);
    let path = dir.join("settings.toml");
    if day {
        std::fs::write(&path, "[appearance]\ntheme = \"day\"\n").unwrap();
    }
    let start = if target.is_empty() { Start::default() } else { Start::parse(target) };
    Harness::new(Settei::sample(path, start))
}

fn check(target: &str, name: &str) {
    for day in [false, true] {
        let mut h = open(target, day);
        let suffix = if day { "day" } else { "night" };
        compare(&mut h, &format!("{name}-{suffix}"));
    }
}

#[test]
fn general() {
    check("general", "general");
}

#[test]
fn about() {
    check("general.about", "about");
}

#[test]
fn appearance() {
    check("appearance", "appearance");
}

#[test]
fn keyboard_shortcuts() {
    check("keyboard.shortcuts", "keyboard-shortcuts");
}

#[test]
fn users() {
    check("users", "users");
}

#[test]
fn battery() {
    check("battery", "battery");
}

#[test]
fn search_results() {
    for day in [false, true] {
        let mut h = open("keyboard", day);
        h.send(AppMsg::Search("lock".into()));
        let suffix = if day { "day" } else { "night" };
        compare(&mut h, &format!("search-{suffix}"));
    }
}
