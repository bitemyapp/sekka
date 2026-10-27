//! Displays against a real compositor: Aizome's headless backend with two
//! virtual outputs. Skipped unless `SETTEI_COMPOSITOR_SETTINGS` names
//! the settings file the compositor was started with (`--config`), and
//! `AIZOME_SOCKET` its socket:
//!
//! ```sh
//! aizome --backend headless --headless-output 1920x1080@1 \
//!   --headless-output 2880x1800@2 --no-shell --no-session --no-xwayland \
//!   --config /tmp/x/settings.toml --socket test-displays &
//! AIZOME_SOCKET=$XDG_RUNTIME_DIR/aizome.test-displays.sock \
//! SETTEI_COMPOSITOR_SETTINGS=/tmp/x/settings.toml \
//!   cargo test -p aizome-settei --test displays_live -- --ignored
//! ```

mod common;

use std::path::PathBuf;
use std::time::Duration;

use aizome_ipc::{Client, OutputInfo, Request, Response};
use aizome_kit::layout::{Point, Size};
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use settei::{Settei, Start};

fn outputs() -> Vec<OutputInfo> {
    let mut c = Client::connect().expect("the compositor");
    match c.call(&Request::Outputs).expect("its outputs") {
        Response::Outputs { outputs } => outputs,
        other => panic!("{other:?}"),
    }
}

fn settle(h: &mut Harness<Settei>, until: impl Fn(&Harness<Settei>) -> bool) -> bool {
    for _ in 0..600 {
        if until(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
        h.send(settei::AppMsg::Nothing);
    }
    false
}

fn has(h: &Harness<Settei>, s: &str) -> bool {
    h.texts(h.window()).iter().any(|t| t == s)
}

fn arrangement(h: &Harness<Settei>) -> String {
    h.semantics(h.window())
        .into_iter()
        .find(|(s, _)| s.label.as_deref() == Some("Display arrangement"))
        .and_then(|(s, _)| s.value)
        .unwrap_or_default()
}

#[test]
#[ignore = "needs a headless Aizome compositor, named by SETTEI_COMPOSITOR_SETTINGS"]
fn arranges_and_applies_on_a_headless_compositor() {
    let Some(path) = std::env::var_os("SETTEI_COMPOSITOR_SETTINGS").map(PathBuf::from) else {
        eprintln!("skipping: SETTEI_COMPOSITOR_SETTINGS isn't set");
        return;
    };
    let before = outputs();
    assert_eq!(before.len(), 2, "{before:?}");
    let first = before[0].name.clone();
    let second = before[1].name.clone();
    eprintln!("outputs: {:?}", before.iter().map(|o| (&o.name, o.logical, o.scale)).collect::<Vec<_>>());

    let mut h = Harness::new(Settei::new(settei::panes::all(), path.clone(), false, Start::parse("displays")));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    assert!(settle(&mut h, |h| has(h, "Use as")), "{:?}", h.texts(w));
    h.save_png(w, common::shots_dir().join("displays-live.png"), 2.0);

    // Drag the first output's tile far to the right of the second.
    let area = h.find_id(w, "entry:displays.arrangement").expect("the arrangement");
    let value = arrangement(&h);
    eprintln!("arrangement: {value}");
    let tiles: Vec<(String, i32)> = before.iter().map(|o| (o.name.clone(), o.logical.x)).collect();
    // The first output is at the left (x 0): grab it a third of the way in.
    assert!(tiles[0].1 < tiles[1].1, "{tiles:?}");
    let from = Point::new(area.x0 + area.width() * 0.3, area.y0 + area.height() * 0.5);
    let to = Point::new(area.x1 - 12.0, area.y0 + area.height() * 0.5);
    h.drag(w, from, to, 10);
    // Wherever it lands, it moved, it touches the other along an edge and
    // they don't overlap.
    let moved = |o: &[OutputInfo]| {
        let a = o.iter().find(|x| x.name == first).unwrap().logical;
        let b = o.iter().find(|x| x.name == second).unwrap().logical;
        let overlap = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
        let beside = (a.x == b.x + b.w || b.x == a.x + a.w) && a.y < b.y + b.h && b.y < a.y + a.h;
        let stacked = (a.y == b.y + b.h || b.y == a.y + a.h) && a.x < b.x + b.w && b.x < a.x + a.w;
        a != before[0].logical && !overlap && (beside || stacked) && a.w == 1920 && b.w == 1440
    };
    let mut ok = false;
    for _ in 0..200 {
        h.send(settei::AppMsg::Nothing);
        if moved(&outputs()) {
            ok = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let after = outputs();
    assert!(ok, "{:?}", after.iter().map(|o| (&o.name, o.logical)).collect::<Vec<_>>());
    // Kept in the settings file, by make, model and serial (or connector).
    assert!(settle(&mut h, |_| std::fs::read_to_string(&path).is_ok_and(|t| t.contains("[[outputs]]"))));
    let text = std::fs::read_to_string(&path).unwrap();
    eprintln!("settings.toml:\n{text}");
    h.save_png(w, common::shots_dir().join("displays-live-moved.png"), 2.0);

    // A rotation applies at once, asks to be kept, and goes back when
    // nobody answers.
    let arranged = outputs();
    assert!(h.click_text(w, "Standard"), "{:?}", h.texts(w));
    let popup = h.popups(w).last().map(|(p, _)| *p).expect("the rotation menu");
    // Nothing is highlighted at first: down to Standard, then to 90°.
    h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::default());
    assert!(settle(&mut h, |h| has(h, "Keep these display settings?")), "{:?}", h.texts(w));
    let turned = outputs();
    let one = turned.iter().find(|o| o.name == first).unwrap();
    eprintln!("turned: {:?} {:?}", one.transform, one.logical);
    assert_ne!(one.transform, aizome_ipc::Transform::Normal);
    h.save_png(w, common::shots_dir().join("displays-live-keep.png"), 2.0);
    h.advance(Duration::from_secs(16));
    assert!(settle(&mut h, |_| outputs()
        .iter()
        .find(|o| o.name == first)
        .is_some_and(|o| o.transform == aizome_ipc::Transform::Normal)));
    assert!(!has(&h, "Keep these display settings?"));
    let back = outputs();
    assert_eq!(
        back.iter().map(|o| o.logical).collect::<Vec<_>>(),
        arranged.iter().map(|o| o.logical).collect::<Vec<_>>()
    );
    // The file still says what was kept before the rotation.
    assert!(!std::fs::read_to_string(&path).unwrap().contains("transform = \"90\""));

    // Mirroring shows at once, before it's kept; Revert takes it back.
    let mirror_of = |name: &str| outputs().into_iter().find(|o| o.name == name).and_then(|o| o.mirror);
    // The "Use as" menu: Main display, Extended display, Mirror for …, Off.
    let choose = |h: &mut Harness<Settei>, now: &str, row: usize| {
        assert!(h.click_text(w, now), "no {now:?} in {:?}", h.texts(w));
        let popup = h.popups(w).last().map(|(p, _)| *p).expect("the menu");
        // Nothing is lit at first: the first press lights the first row.
        for _ in 0..=row {
            h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
        }
        h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::default());
    };
    let used = if has(&h, "Main display") { "Main display" } else { "Extended display" };
    choose(&mut h, used, 2);
    assert!(settle(&mut h, |h| has(h, "Keep these display settings?")), "{:?}", h.texts(w));
    assert_eq!(mirror_of(&first).as_deref(), Some(second.as_str()), "mirroring only once kept");
    assert!(h.click_text(w, "Revert"));
    assert!(settle(&mut h, |_| mirror_of(&first).is_none()), "still mirroring after Revert");
    assert!(!std::fs::read_to_string(&path).unwrap().contains("mirror ="));

    // `[[outputs]]` changed elsewhere (the other display made the main
    // one) and applied: the open pane follows.
    assert!(settle(&mut h, |h| has(h, "Main display")), "{:?}", h.texts(w));
    let text = std::fs::read_to_string(&path).unwrap().replace("primary = true\n", "");
    std::fs::write(&path, format!("{text}\n[[outputs]]\nname = \"{second}\"\nprimary = true\n")).unwrap();
    Client::connect().unwrap().call(&Request::ReloadConfig).unwrap();
    assert!(
        settle(&mut h, |h| has(h, "Extended display") && !has(h, "Main display")),
        "the pane kept the old main display: {:?}",
        h.texts(w)
    );
}
