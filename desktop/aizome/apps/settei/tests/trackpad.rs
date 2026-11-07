//! The Trackpad pane over the sample: turning the trackpad off greys the
//! rest of the pane, keeping its settings.

mod common;

use aizome_kit::layout::Size;
use aizome_kit::widget::SemanticRole;

#[test]
fn turning_the_trackpad_off_greys_its_other_settings() {
    let (mut h, path) = common::open("trackpad-off", "trackpad");
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    // The pane's controls: (on the master switch's row, disabled).
    let controls = |h: &aizome_kit::testing::Harness<settei::Settei>| {
        let master = h.find(w, "Use the trackpad").expect("the switch's row");
        h.semantics(w)
            .into_iter()
            .filter(|(s, _)| matches!(s.role, SemanticRole::Switch | SemanticRole::Slider | SemanticRole::ComboBox))
            .map(|(s, r)| ((r.center().y - master.center().y).abs() < 20.0, s.disabled))
            .collect::<Vec<_>>()
    };
    let before = controls(&h);
    // The switch, two sliders, a dropdown and four more switches.
    assert!(before.len() >= 8, "{before:?}");
    assert!(before.iter().all(|&(_, disabled)| !disabled), "all usable while on: {before:?}");

    let master = h
        .semantics(w)
        .into_iter()
        .filter(|(s, _)| s.role == SemanticRole::Switch)
        .map(|(_, r)| r)
        .min_by(|a, b| a.y0.total_cmp(&b.y0))
        .expect("the trackpad switch, first on the pane");
    h.click(w, master.center());

    let after = controls(&h);
    assert_eq!(after.len(), before.len());
    for (on_master_row, disabled) in after {
        assert_eq!(disabled, !on_master_row, "only the switch itself stays usable");
    }
    assert!(h.texts(w).iter().any(|t| t.starts_with("The trackpad is off.")), "{:?}", h.texts(w));
    let saved = std::fs::read_to_string(&path).expect("settings written");
    assert!(saved.contains("enabled = false"), "{saved}");
}
