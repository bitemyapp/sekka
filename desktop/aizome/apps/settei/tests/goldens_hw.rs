//! Golden images of the hardware and network panes over the sample, Night
//! and Day, through the offscreen renderer (`AIZOME_UPDATE_GOLDENS=1`
//! writes them anew). They were looked at when written.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use aizome_kit::layout::Size;
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

/// The sample opened at `target`, in Night or Day, once what the pane
/// asked for in the background has come back (`ready` shows).
fn open(target: &str, day: bool, ready: &str) -> Harness<Settei> {
    let dir = common::temp_dir(target);
    let path = dir.join("settings.toml");
    if day {
        std::fs::write(&path, "[appearance]\ntheme = \"day\"\n").unwrap();
    }
    let mut h = Harness::new(Settei::sample(path, Start::parse(target)));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1000.0));
    for _ in 0..400 {
        if h.texts(w).iter().any(|t| t == ready) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
        h.send(AppMsg::Nothing);
    }
    assert!(h.texts(w).iter().any(|t| t == ready), "{ready} never showed: {:?}", h.texts(w));
    h
}

fn check(target: &str, name: &str, ready: &str) {
    check_after(target, name, ready, |_| {});
}

fn check_after(target: &str, name: &str, ready: &str, then: impl Fn(&mut Harness<Settei>)) {
    for day in [false, true] {
        let mut h = open(target, day, ready);
        then(&mut h);
        let suffix = if day { "day" } else { "night" };
        compare(&mut h, &format!("{name}-{suffix}"));
    }
}

#[test]
fn network() {
    check("network", "network", "Kohaku");
}

#[test]
fn network_details() {
    check("network.dns", "network-details", "Configure IPv4");
}

#[test]
fn bluetooth() {
    check("bluetooth", "bluetooth", "WH-1000XM5");
}

#[test]
fn sound() {
    // The sample's voice a moment in, so the input meter shows a level.
    check_after("sound", "sound", "Built-in Speakers", |h| h.advance(Duration::from_millis(480)));
}

#[test]
fn displays() {
    check("displays", "displays", "Use as");
}

#[test]
fn printers() {
    check("printers", "printers", "Brother HL-L2390DW");
}
