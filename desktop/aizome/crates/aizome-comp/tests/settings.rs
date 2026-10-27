//! Settings keys Settei writes, honoured: what double-clicking a tab
//! does, scrolling distance, sticky keys and slow keys.

mod common;

use std::time::Duration;

use aizome_ipc::{Request, Synthetic};
use common::*;

const SHIFT: u32 = 1;
const KEY_A: u32 = 30;
const KEY_B: u32 = 48;

fn key(c: &Compositor, k: &str, pressed: bool) {
    c.ok(Request::Synthesize { events: vec![Synthetic::Key { key: k.into(), pressed }] });
}

fn tap(c: &Compositor, k: &str) {
    key(c, k, true);
    key(c, k, false);
}

fn double_click_tab(c: &Compositor, w: &aizome_ipc::WindowInfo) {
    // The tab sits on the frame's top edge, at its left.
    let (x, y) = ((w.geometry.x + 30) as f64, (w.geometry.y - 10) as f64);
    c.ok(Request::Synthesize {
        events: vec![
            Synthetic::PointerMotion { x, y },
            Synthetic::Click { button: "left".into() },
            Synthetic::Click { button: "left".into() },
        ],
    });
}

#[test]
fn double_clicking_a_tab_does_what_the_settings_say() {
    let c =
        Compositor::start(Options { settings: "[windows]\ntab_double_click = \"minimise\"\n", ..Default::default() });
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    double_click_tab(&c, &w);
    c.wait_windows("parked", |ws| ws[0].minimised);

    let c =
        Compositor::start(Options { settings: "[windows]\ntab_double_click = \"nothing\"\n", ..Default::default() });
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    double_click_tab(&c, &w);
    c.settle();
    let w2 = c.window("org.example.editor").unwrap();
    assert!(!w2.fill && !w2.minimised, "{w2:?}");

    // The default fills.
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    double_click_tab(&c, &w);
    c.wait_windows("filled", |ws| ws[0].fill);
}

#[test]
fn scrolling_goes_as_far_as_the_factor_says() {
    let c = Compositor::start(Options { settings: "[input.pointer]\nscroll_factor = 2.5\n", ..Default::default() });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    let (x, y) = ((w.geometry.x + w.geometry.w / 2) as f64, (w.geometry.y + w.geometry.h / 2) as f64);
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::PointerScroll { dx: 0.0, dy: 10.0 }],
    });
    let s = a.wait_seen("the scroll", |s| s.scrolled != 0.0);
    assert!((s.scrolled - 25.0).abs() < 0.01, "{}", s.scrolled);
}

#[test]
fn natural_scrolling_turns_the_scroll_round_once() {
    let c = Compositor::start(Options { settings: "[input.pointer]\nnatural_scroll = true\n", ..Default::default() });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let w = c.wait_window("org.example.editor");
    let (x, y) = ((w.geometry.x + w.geometry.w / 2) as f64, (w.geometry.y + w.geometry.h / 2) as f64);
    c.ok(Request::Synthesize {
        events: vec![Synthetic::PointerMotion { x, y }, Synthetic::PointerScroll { dx: 0.0, dy: 10.0 }],
    });
    let s = a.wait_seen("the scroll", |s| s.scrolled != 0.0);
    assert!((s.scrolled + 10.0).abs() < 0.01, "{}", s.scrolled);
}

#[test]
fn modifier_swaps_apply_live_to_both_sides_and_can_be_undone() {
    const CTRL: u32 = 1 << 2;
    const ALT: u32 = 1 << 3;
    const SUPER: u32 = 1 << 6;
    let c = Compositor::start(Options::default());
    // Kit apps receive Super directly, without compatibility translation to Ctrl.
    let a = c.client(Spec::new("org.aizome.modifier-test", 0x2222cc));
    c.wait_window("org.aizome.modifier-test");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let mut count = 0;
    for (options, expected) in [
        ("", [ALT, SUPER, CTRL, ALT, SUPER, CTRL]),
        ("altwin:swap_alt_win", [SUPER, ALT, CTRL, SUPER, ALT, CTRL]),
        ("altwin:swap_lalt_lwin", [SUPER, ALT, CTRL, ALT, SUPER, CTRL]),
        ("altwin:swap_ralt_rwin", [ALT, SUPER, CTRL, SUPER, ALT, CTRL]),
        ("ctrl:swap_lwin_lctl,ctrl:swap_rwin_rctl", [ALT, CTRL, SUPER, ALT, CTRL, SUPER]),
        ("ctrl:swap_lwin_lctl", [ALT, CTRL, SUPER, ALT, SUPER, CTRL]),
        ("ctrl:swap_rwin_rctl", [ALT, SUPER, CTRL, ALT, CTRL, SUPER]),
        ("ctrl:swap_lalt_lctl,ctrl:swap_ralt_rctl", [CTRL, SUPER, ALT, CTRL, SUPER, ALT]),
        ("ctrl:swap_lalt_lctl", [CTRL, SUPER, ALT, ALT, SUPER, CTRL]),
        ("ctrl:swap_ralt_rctl", [ALT, SUPER, CTRL, CTRL, SUPER, ALT]),
        ("", [ALT, SUPER, CTRL, ALT, SUPER, CTRL]),
    ] {
        std::fs::write(
            c.dir.join("config/aizome/settings.toml"),
            format!("[input.keyboard]\noptions = \"{options}\"\n"),
        )
        .unwrap();
        c.ok(Request::ReloadConfig);
        // Physical Alt, Super, Control on the left, then the right (evdev codes).
        for (physical, want) in ["56", "125", "29", "100", "126", "97"].into_iter().zip(expected) {
            key(&c, physical, true);
            tap(&c, "a");
            key(&c, physical, false);
            count += 1;
            let s = a.wait_seen("modified a", |s| s.keys.iter().filter(|(k, _)| *k == KEY_A).count() == count);
            let (_, mods) = s.keys.iter().rfind(|(k, _)| *k == KEY_A).unwrap();
            assert_eq!(mods & (ALT | SUPER | CTRL), want, "physical key {physical}, options {options}");
        }
    }
}

#[test]
fn sticky_keys_latch_a_modifier_for_the_next_key() {
    let c = Compositor::start(Options { settings: "[input.keyboard]\nsticky_keys = true\n", ..Default::default() });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    tap(&c, "shift");
    tap(&c, "a");
    tap(&c, "b");
    let s = a.wait_seen("a and b", |s| s.keys.iter().any(|(k, _)| *k == KEY_B));
    let mods = |k: u32| s.keys.iter().find(|(key, _)| *key == k).map(|(_, m)| *m).unwrap();
    assert_eq!(mods(KEY_A) & SHIFT, SHIFT, "Shift came with a");
    assert_eq!(mods(KEY_B) & SHIFT, 0, "and only with a");
    // Tapped twice, it lets go.
    tap(&c, "shift");
    tap(&c, "shift");
    tap(&c, "a");
    let s = a.wait_seen("the second a", |s| s.keys.iter().filter(|(k, _)| *k == KEY_A).count() == 2);
    let (_, m) = s.keys.iter().filter(|(k, _)| *k == KEY_A).nth(1).unwrap();
    assert_eq!(m & SHIFT, 0);
}

#[test]
fn slow_keys_count_only_keys_held_long_enough() {
    let c = Compositor::start(Options {
        settings: "[input.keyboard]\nslow_keys = true\nslow_keys_ms = 200\n",
        ..Default::default()
    });
    let a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    // A brush of the key: nothing.
    tap(&c, "a");
    // Held: it counts.
    key(&c, "b", true);
    std::thread::sleep(Duration::from_millis(400));
    key(&c, "b", false);
    let s = a.wait_seen("b", |s| s.keys.iter().any(|(k, _)| *k == KEY_B));
    assert!(!s.keys.iter().any(|(k, _)| *k == KEY_A), "a was only brushed: {:?}", s.keys);
}
