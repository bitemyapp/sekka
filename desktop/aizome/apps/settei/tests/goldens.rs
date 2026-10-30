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
    let w = h.window();
    compare_window(h, w, name);
}

fn compare_window(h: &mut Harness<Settei>, w: aizome_kit::WindowKey, name: &str) {
    if !h.theme().has_ui_font() {
        eprintln!("skipping the golden {name}: Archivo isn't installed (set AIZOME_FONT_DIR=~/work/.aizome-fonts)");
        return;
    }
    assert_golden(h, w, golden(name), 1.0);
}

fn open(target: &str, day: bool) -> Harness<Settei> {
    open_with(target, day, "")
}

/// Settei on `target`, with `settings` besides the theme.
fn open_with(target: &str, day: bool, settings: &str) -> Harness<Settei> {
    let dir = common::temp_dir(target);
    let path = dir.join("settings.toml");
    let theme = if day { "[appearance]\ntheme = \"day\"\n" } else { "" };
    std::fs::write(&path, format!("{theme}{settings}")).unwrap();
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

/// The setup sheet once the sample's Magic Keyboard's Command key is
/// pressed, with another keyboard of its own to start from.
#[test]
fn keyboard_setup() {
    let other = "[[input.keyboards]]\nname = \"Logitech K120\"\nid = \"046d:c31c\"\noptions = \"compose:ralt\"\n";
    for day in [false, true] {
        let mut h = open_with("keyboard.setup", day, other);
        let command: settei::pane::PaneMsg = std::sync::Arc::new(settei::panes::keyboard::Msg::SampleKey(125));
        h.send(AppMsg::Pane("keyboard", command));
        let s = h.program().sheet_window().expect("the setup sheet");
        let suffix = if day { "day" } else { "night" };
        compare_window(&mut h, s, &format!("keyboard-setup-{suffix}"));
    }
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
