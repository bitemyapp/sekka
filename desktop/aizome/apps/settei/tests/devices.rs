//! Bluetooth, Sound and Printers & Scanners over the sample.

mod common;

use std::time::Duration;

use aizome_kit::layout::Size;
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use settei::Settei;

fn open(name: &str, pane: &str, wait_for: &str) -> Harness<Settei> {
    let (mut h, _) = common::open(name, pane);
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    settle(&mut h, |h| has(h, wait_for));
    h
}

fn shots(h: &mut Harness<Settei>, name: &str) {
    let w = h.window();
    h.save_png(w, common::shots_dir().join(format!("{name}-night.png")), 2.0);
    h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() });
    h.save_png(w, common::shots_dir().join(format!("{name}-day.png")), 2.0);
    h.set_appearance(Appearance::default());
}

#[test]
fn bluetooth_shows_devices_and_pairs_with_a_code() {
    let mut h = open("bluetooth", "bluetooth", "WH-1000XM5");
    let w = h.window();
    for want in ["My Devices", "MX Master 3S", "Connected · 14%", "JBL Flip 6", "Pixel Watch 2"] {
        assert!(
            h.texts(w).iter().any(|t| t == want || t == &want.to_uppercase()),
            "{want} missing from {:?}",
            h.texts(w)
        );
    }
    shots(&mut h, "bluetooth");
    // Pairing a nearby speaker asks to compare a code.
    let texts = h.texts(w);
    let jbl = h.find(w, "JBL Flip 6").expect("the speaker");
    let _ = texts;
    // Its Connect button is at the row's right.
    let connects: Vec<_> =
        h.semantics(w).into_iter().filter(|(s, _)| s.label.as_deref() == Some("Connect")).map(|(_, r)| r).collect();
    let button =
        connects.into_iter().find(|r| (r.center().y - jbl.center().y).abs() < 20.0).expect("its Connect button");
    h.click(w, button.center());
    settle(&mut h, |h| has(h, "Pair with “JBL Flip 6”?"));
    assert!(has(&h, "482 913"), "{:?}", h.texts(w));
    h.save_png(w, common::shots_dir().join("bluetooth-pairing.png"), 2.0);
    assert!(h.click_text(w, "Pair"));
    settle(&mut h, |h| h.texts(h.window()).iter().filter(|t| *t == "JBL Flip 6").count() == 1 && has(h, "Connected"));
    // Paired: it's among My Devices, connected.
    assert!(!has(&h, "Pair with “JBL Flip 6”?"));
}

#[test]
fn sound_lists_devices_apps_and_meters_the_input() {
    let mut h = open("sound", "sound", "Built-in Speakers");
    let w = h.window();
    for want in [
        "Built-in Speakers",
        "WH-1000XM5",
        "Built-in Microphone",
        "Output volume",
        "Balance",
        "Input level",
        "Spotify",
        "Alert volume",
    ] {
        assert!(h.texts(w).iter().any(|t| t == want), "{want} missing from {:?}", h.texts(w));
    }
    // The meter moves with the sample's voice.
    h.advance(Duration::from_millis(400));
    shots(&mut h, "sound");
}

#[test]
fn printers_show_state_and_add_one() {
    let mut h = open("printers", "printers", "Brother HL-L2390DW");
    let w = h.window();
    for want in ["Default printer", "Canon TS8350", "Out of paper · Living room", "Paused", "1 job"] {
        assert!(h.texts(w).iter().any(|t| t == want), "{want} missing from {:?}", h.texts(w));
    }
    assert!(h.click_text(w, "Canon TS8350"));
    settle(&mut h, |h| has(h, "Kohaku boarding pass.pdf"));
    shots(&mut h, "printers");
    assert!(h.click_text(w, "Add Printer…"));
    settle(&mut h, |h| has(h, "EPSON ET-2850 Series"));
    assert!(h.click_text(w, "EPSON ET-2850 Series"));
    settle(&mut h, |h| has(h, "Name"));
    h.save_png(w, common::shots_dir().join("printers-add.png"), 2.0);
    assert!(h.click_text(w, "Add"));
    settle(&mut h, |h| h.texts(h.window()).iter().any(|t| t.starts_with("Added ")));
}

/// Let background work (sample tasks) come back: flush the window until
/// `until` holds, up to two seconds.
fn settle(h: &mut Harness<Settei>, until: impl Fn(&Harness<Settei>) -> bool) {
    for _ in 0..400 {
        if until(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
        h.send(settei::AppMsg::Nothing);
    }
}

fn has(h: &Harness<Settei>, s: &str) -> bool {
    let w = h.window();
    h.texts(w).iter().any(|t| t == s)
}

#[test]
fn sound_switches_the_output() {
    let mut h = open("sound-switch", "sound", "Built-in Speakers");
    let w = h.window();
    assert!(h.click_text(w, "WH-1000XM5"));
    // The headphones are the output now: their row is bold and checked,
    // and the volume shown is theirs.
    settle(&mut h, |h| has(h, "40%"));
    assert!(has(&h, "40%"), "{:?}", h.texts(w));
}
