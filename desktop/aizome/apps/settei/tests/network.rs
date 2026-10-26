//! The Network pane over the sample: Wi-Fi, joining, details, VPNs.

mod common;

use aizome_kit::layout::Size;
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use settei::Settei;

fn open(name: &str) -> Harness<Settei> {
    let (mut h, _) = common::open(name, "network");
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    settle(&mut h, |h| has(h, "Kohaku"));
    h
}

#[test]
fn shows_wifi_networks_and_vpns() {
    let mut h = open("network-main");
    let w = h.window();
    assert_eq!(h.program().pane_id(), "network");
    let texts = h.texts(w);
    for want in [
        "Wi-Fi",
        "Kohaku",
        "Aizome Studio",
        "Hanada Café",
        "Other Network…",
        "Office",
        "Saved Networks",
        "Ethernet",
        "Cable unplugged",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want} missing from {texts:?}");
    }
    h.save_png(w, common::shots_dir().join("network-night.png"), 2.0);
    h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() });
    h.save_png(w, common::shots_dir().join("network-day.png"), 2.0);
}

#[test]
fn joining_asks_for_the_password_and_says_how_it_went() {
    let mut h = open("network-join");
    let w = h.window();
    // A secured network asks for its password in a sheet.
    assert!(h.click_text(w, "Tōkyō Guest"));
    settle(&mut h, |h| has(h, "Join “Tōkyō Guest”"));
    h.save_png(w, common::shots_dir().join("network-password.png"), 2.0);
    h.type_text(w, "kamenozoki");
    assert!(h.click_text(w, "Join"));
    // The Wi-Fi row says so, and so does the status strip once joining ends.
    settle(&mut h, |h| h.texts(h.window()).iter().filter(|t| *t == "Connected to Tōkyō Guest").count() == 2);
    let texts = h.texts(w);
    assert_eq!(texts.iter().filter(|t| *t == "Connected to Tōkyō Guest").count(), 2, "{texts:?}");
    // WEP isn't offered.
    assert!(h.click_text(w, "Old Router"));
    settle(&mut h, |h| h.texts(h.window()).iter().any(|t| t.contains("WEP")));
    assert!(h.texts(w).iter().any(|t| t.contains("isn't offered")), "{:?}", h.texts(w));
}

#[test]
fn a_wrong_password_is_said_plainly() {
    let mut h = open("network-wrong");
    let w = h.window();
    assert!(h.click_text(w, "Tōkyō Guest"));
    settle(&mut h, |h| has(h, "Join “Tōkyō Guest”"));
    h.type_text(w, "wrong");
    // Too short for WPA3? No: any length. Join with it.
    assert!(h.click_text(w, "Join"));
    // The sheet comes back for correction and explains why it didn't join.
    settle(&mut h, |h| h.texts(h.window()).iter().any(|t| t.contains("Check the password")));
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t.starts_with("Couldn't join: ")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Join “Tōkyō Guest”"), "{texts:?}");
    let field = h.inspected(w).into_iter().find(|e| e["name"] == "network-password").expect("the field");
    assert!(field["text"].is_null() && field["value"].is_null(), "the password stays private: {field}");
    // Put right, it joins.
    h.command(w, 'a');
    h.type_text(w, "kamenozoki\n");
    settle(&mut h, |h| h.texts(h.window()).iter().any(|t| t == "Connected to Tōkyō Guest"));
    assert!(h.texts(w).iter().any(|t| t == "Connected to Tōkyō Guest"), "{:?}", h.texts(w));
}

#[test]
fn details_show_addresses_and_settings() {
    let mut h = open("network-details");
    let w = h.window();
    assert!(h.click_text(w, "Details…"));
    settle(&mut h, |h| has(h, "Configure IPv4"));
    let texts = h.texts(w);
    for want in [
        "Join automatically",
        "Metered connection",
        "Private address",
        "DNS servers",
        "Configure IPv6",
        "Forget This Network…",
        "192.168.10.42/24\nfd00:10::42/64",
    ] {
        assert!(texts.iter().any(|t| t == want), "{want} missing from {texts:?}");
    }
    h.save_png(w, common::shots_dir().join("network-details.png"), 2.0);
    // Back to the network list.
    assert!(h.click_text(w, "Network"));
    settle(&mut h, |h| has(h, "Other Network…"));
}

#[test]
fn hidden_networks_join_by_name() {
    let mut h = open("network-hidden");
    let w = h.window();
    assert!(h.click_text(w, "Other Network…"));
    settle(&mut h, |h| has(h, "Join a Network by Name"));
    h.save_png(w, common::shots_dir().join("network-hidden.png"), 2.0);
}

/// Let background work (sample tasks, the sample compositor) come back:
/// flush the window until `until` holds, up to two seconds.
fn settle(h: &mut Harness<Settei>, until: impl Fn(&Harness<Settei>) -> bool) {
    for _ in 0..400 {
        if until(h) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.send(settei::AppMsg::Nothing);
    }
}

fn has(h: &Harness<Settei>, s: &str) -> bool {
    let w = h.window();
    h.texts(w).iter().any(|t| t == s)
}

#[test]
fn a_vpn_switches_on_and_a_network_is_forgotten() {
    let mut h = open("network-vpn");
    let w = h.window();
    // Office's switch: the first switch in the VPN section.
    let office = h.find(w, "Office").expect("the VPN");
    let switches: Vec<_> = h
        .semantics(w)
        .into_iter()
        .filter(|(s, r)| {
            s.role == aizome_kit::widget::SemanticRole::Switch && (r.center().y - office.center().y).abs() < 20.0
        })
        .map(|(_, r)| r)
        .collect();
    h.click(w, switches[0].center());
    settle(&mut h, |h| has(h, "WireGuard · Connected"));
    assert!(has(&h, "WireGuard · Connected"), "{:?}", h.texts(w));

    // Forget Aizome Studio from its page, confirming.
    assert!(h.click_text(w, "Saved Networks"));
    settle(&mut h, |h| has(h, "Hotel Sakura"));
    assert!(h.click_text(w, "Aizome Studio"));
    settle(&mut h, |h| has(h, "Forget This Network…"));
    assert!(h.click_text(w, "Forget This Network…"));
    settle(&mut h, |h| has(h, "Forget “Aizome Studio”?"));
    assert!(h.click_text(w, "Forget"));
    settle(&mut h, |h| h.texts(h.window()).iter().any(|t| t == "Forgot Aizome Studio"));
    // Back on the saved list, without it.
    assert!(has(&h, "Hotel Sakura"), "{:?}", h.texts(w));
    assert!(!has(&h, "Aizome Studio"), "{:?}", h.texts(w));
}
