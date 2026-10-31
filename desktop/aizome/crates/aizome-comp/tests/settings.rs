//! Settings keys Settei writes, honoured: whether new windows of the same
//! app stack, what double-clicking a tab does, scrolling distance, each
//! keyboard's own modifier keys, sticky keys and slow keys.

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
fn new_windows_of_the_focused_app_stack_unless_the_settings_say_not() {
    // The default stacks a second window of the focused app into its group.
    let c = Compositor::start(Options::default());
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    let a = c.wait_window("org.example.editor");
    let _b = c.client(Spec::new("org.example.editor", 0x22cc22));
    let ws = c.wait_windows("stacked", |ws| ws.len() == 2 && ws.iter().all(|w| w.group.is_some()));
    let b = ws.iter().find(|w| w.id != a.id).unwrap();
    assert_eq!(ws[0].group, ws[1].group, "{ws:?}");
    assert!(b.group_active && b.focused, "the new window shows: {b:?}");
    // Another app's window opens on its own.
    let _v = c.client(Spec::new("org.example.viewer", 0xcc2222));
    let v = c.wait_window("org.example.viewer");
    assert!(v.group.is_none(), "{v:?}");

    let c = Compositor::start(Options {
        settings: "[windows]\nstack_new_windows_of_same_app = false\n",
        ..Default::default()
    });
    let _a = c.client(Spec::new("org.example.editor", 0x2222cc));
    c.wait_window("org.example.editor");
    let _b = c.client(Spec::new("org.example.editor", 0x22cc22));
    c.wait_windows("two windows", |ws| ws.len() == 2);
    c.settle();
    let ws = c.windows();
    assert!(ws.iter().all(|w| w.group.is_none()), "{ws:?}");
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
fn each_keyboard_types_with_its_own_modifier_keys() {
    const CAPS: u32 = 1 << 1;
    const ALT: u32 = 1 << 3;
    const SUPER: u32 = 1 << 6;
    const MAC: &str = "Apple Inc. Magic Keyboard";
    const PC: &str = "Logitech K120";
    let settings = |mac: &str| {
        format!(
            "[input.keyboard]\noptions = \"altwin:swap_alt_win,grp:alt_shift_toggle\"\n\
             [[input.keyboards]]\nname = \"{MAC}\"\nid = \"05ac:029c\"\noptions = \"{mac}\"\n"
        )
    };
    let c = Compositor::start(Options { settings: &settings(""), ..Default::default() });
    let mut events = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    events.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let a = c.client(Spec::new("org.aizome.modifier-test", 0x2222cc));
    c.wait_window("org.aizome.modifier-test");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);

    let plug = |name: &str, id: &str| {
        c.ok(Request::Synthesize {
            events: vec![Synthetic::PlugKeyboard {
                name: name.into(),
                id: id.into(),
                bus: aizome_ipc::KeyboardBus::Usb,
            }],
        })
    };
    plug(PC, "046d:c31c");
    plug(MAC, "05ac:029c");
    for name in [PC, MAC] {
        let got = events.by_ref().find_map(|e| match e {
            Ok(aizome_ipc::Event::KeyboardAdded { keyboard }) => Some(keyboard),
            _ => None,
        });
        assert_eq!(got.map(|k| k.name).as_deref(), Some(name));
    }
    let on = |keyboard: &str, key: &str, pressed: bool| {
        c.ok(Request::Synthesize {
            events: vec![Synthetic::KeyOn { keyboard: keyboard.into(), key: key.into(), pressed }],
        })
    };
    let mut count = 0;
    // The key beside the space bar (left Super on the Mac, left Alt on the
    // PC) held with a: what the app hears it as.
    let mut beside_space = |keyboard: &str, physical: &str| {
        on(keyboard, physical, true);
        on(keyboard, "a", true);
        on(keyboard, "a", false);
        on(keyboard, physical, false);
        count += 1;
        let s = a.wait_seen("modified a", |s| s.keys.iter().filter(|(k, _)| *k == KEY_A).count() == count);
        let (_, mods) = s.keys.iter().rfind(|(k, _)| *k == KEY_A).unwrap();
        mods & (ALT | SUPER)
    };
    assert_eq!(beside_space(PC, "56"), SUPER, "the PC's Alt is Super: swapped, as every keyboard's");
    assert_eq!(beside_space(MAC, "125"), SUPER, "the Mac's Command is Super: its own, unswapped");
    assert_eq!(beside_space(MAC, "56"), ALT, "and its Option is Alt");
    let keymaps = a.seen().keymaps;
    assert_eq!(beside_space(PC, "56"), SUPER, "back on the PC");
    assert_eq!(beside_space(PC, "125"), ALT);
    assert_eq!(a.seen().keymaps, keymaps + 1, "one keymap for the change of keyboard, none for its keys");

    let aizome_ipc::Response::Keyboards { keyboards } = c.request(Request::Keyboards) else { panic!() };
    let last: Vec<_> = keyboards.iter().map(|k| (k.name.as_str(), k.last)).collect();
    assert_eq!(last, [(MAC, false), (PC, true)]);

    // Caps Lock locked on one keyboard stays locked on the other.
    on(PC, "58", true);
    on(PC, "58", false);
    a.wait_seen("Caps Lock", |s| s.locked & CAPS != 0);
    on(MAC, "b", true);
    on(MAC, "b", false);
    let s = a.wait_seen("b", |s| s.keys.iter().any(|(k, _)| *k == KEY_B));
    // Locked as b came, or the app would type it in lower case.
    assert_ne!(s.locked_at_keys.last().copied().unwrap_or_default() & CAPS, 0, "Caps Lock with the Mac's keymap");
    on(MAC, "58", true);
    on(MAC, "58", false);
    a.wait_seen("Caps Lock off", |s| s.locked & CAPS == 0);

    // A change to the keyboard typing applies at once.
    std::fs::write(c.dir.join("config/aizome/settings.toml"), settings("altwin:swap_lalt_lwin")).unwrap();
    c.ok(Request::ReloadConfig);
    assert_eq!(beside_space(MAC, "125"), ALT, "the Mac swapped too, on its left");
    // Unplugged and plugged in again, it isn't news.
    c.ok(Request::Synthesize { events: vec![Synthetic::UnplugKeyboard { name: MAC.into() }] });
    plug(MAC, "05ac:029c");
    assert_eq!(beside_space(MAC, "125"), ALT, "back, with its own keys");
    plug("Another", "1234:5678");
    let got = events.by_ref().find_map(|e| match e {
        Ok(aizome_ipc::Event::KeyboardAdded { keyboard }) => Some(keyboard.name),
        _ => None,
    });
    assert_eq!(got.as_deref(), Some("Another"));

    // Composite keyboards can expose another typing interface with the
    // same vendor/product ID and a different name. Its keys use the saved
    // profile too, rather than falling back to every keyboard's mapping.
    let interface = "Apple Inc. Magic Keyboard Keyboard";
    plug(interface, "05AC:029C");
    assert_eq!(beside_space(interface, "125"), ALT, "the saved left-only swap applies to this interface");
    assert_eq!(beside_space(interface, "126"), SUPER, "its right Super stays unswapped");
    assert_eq!(beside_space(MAC, "126"), SUPER, "the original interface keeps the same profile");
}

/// Two layouts: the PC switches to the second (Alt+Shift), then the Mac,
/// with keys of its own (so a keymap of its own), types b twice, then a
/// capital Y and a y. The Shift is the first change of modifiers since the
/// change of keyboard, which tells the app the layout the seat is really
/// in. What the app saw.
fn layout_after_a_change_of_keyboard(numlock: bool, caps: bool) -> common::Seen {
    const PC: &str = "Logitech K120";
    const MAC: &str = "Apple Inc. Magic Keyboard";
    let settings = format!(
        "[input.keyboard]\nlayouts = [\"us\", \"de\"]\noptions = \"grp:alt_shift_toggle\"\nnumlock = {numlock}\n\
         [[input.keyboards]]\nname = \"{MAC}\"\nid = \"05ac:029c\"\noptions = \"altwin:swap_alt_win\"\n"
    );
    let c = Compositor::start(Options { settings: &settings, ..Default::default() });
    let a = c.client(Spec::new("org.aizome.layout-test", 0x2222cc));
    c.wait_window("org.aizome.layout-test");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    for (name, id) in [(PC, "046d:c31c"), (MAC, "05ac:029c")] {
        c.ok(Request::Synthesize {
            events: vec![Synthetic::PlugKeyboard {
                name: name.into(),
                id: id.into(),
                bus: aizome_ipc::KeyboardBus::Usb,
            }],
        });
    }
    let on = |keyboard: &str, key: &str, pressed: bool| {
        c.ok(Request::Synthesize {
            events: vec![Synthetic::KeyOn { keyboard: keyboard.into(), key: key.into(), pressed }],
        })
    };
    if caps {
        on(PC, "58", true);
        on(PC, "58", false);
        a.wait_seen("Caps Lock on", |s| s.locked & 2 != 0);
    }
    on(PC, "56", true);
    on(PC, "42", true);
    on(PC, "42", false);
    on(PC, "56", false);
    a.wait_seen("the second layout on the PC", |s| s.group == 1);
    for _ in 0..2 {
        on(MAC, "48", true);
        on(MAC, "48", false);
    }
    on(MAC, "42", true);
    on(MAC, "21", true);
    on(MAC, "21", false);
    on(MAC, "42", false);
    on(MAC, "21", true);
    on(MAC, "21", false);
    a.wait_seen("Shift+y and y", |s| s.keys.iter().filter(|(k, _)| *k == 21).count() == 2);
    c.settle();
    let s = a.seen();
    // b, b, Shift, Y and y: all in the second layout.
    let last: Vec<u32> = s.group_at_keys.iter().rev().take(5).copied().collect();
    assert_eq!(last, [1; 5], "the layout as each key on the Mac came: {:?}", s.group_at_keys);
    s
}

#[test]
fn layout_survives_a_change_of_keyboard_without_locks() {
    let s = layout_after_a_change_of_keyboard(false, false);
    assert_eq!(s.group, 1, "the second layout after typing on the Mac");
}

#[test]
fn layout_survives_a_change_of_keyboard_with_num_lock() {
    let s = layout_after_a_change_of_keyboard(true, false);
    assert_eq!(s.group, 1, "the second layout after typing on the Mac with Num Lock on");
    assert_ne!(s.locked & 0x10, 0, "and Num Lock still on: {:#x}", s.locked);
}

#[test]
fn layout_survives_a_change_of_keyboard_with_caps_lock() {
    let s = layout_after_a_change_of_keyboard(false, true);
    assert_eq!(s.group, 1, "the second layout after typing on the Mac with Caps Lock on");
    assert_ne!(s.locked & 2, 0, "and Caps Lock still on");
}

#[test]
fn a_caught_key_says_which_keyboard_and_reaches_no_app() {
    const SUPER: u32 = 1 << 6;
    let c = Compositor::start(Options::default());
    // Listening, and never asking for a key.
    let mut others = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
    others.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let a = c.client(Spec::new("org.aizome.catch-test", 0x2222cc));
    c.wait_window("org.aizome.catch-test");
    a.wait_seen("keyboard focus", |s| s.keyboard_focus);
    let synth = |e: Synthetic| c.ok(Request::Synthesize { events: vec![e] });
    let plug = |name: &str| {
        synth(Synthetic::PlugKeyboard {
            name: name.into(),
            id: "05ac:029c".into(),
            bus: aizome_ipc::KeyboardBus::Bluetooth,
        })
    };
    plug("Apple Inc. Magic Keyboard");
    let on = |key: &str, pressed: bool| {
        synth(Synthetic::KeyOn { keyboard: "Apple Inc. Magic Keyboard".into(), key: key.into(), pressed })
    };
    let tap = |key: &str| {
        on(key, true);
        on(key, false);
    };

    // Asked on a connection listening for events, which alone hears of it.
    let catch = |tag: &str| {
        let mut asker = aizome_ipc::Client::connect_to(&c.ipc).unwrap().events().unwrap();
        asker.stream().set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let r = asker.request(&Request::CatchKey { tag: Some(tag.into()) }).unwrap();
        assert_eq!(r, aizome_ipc::Response::Ok);
        asker
    };
    let caught = |events: &mut aizome_ipc::Events| {
        events.by_ref().find_map(|e| match e {
            Ok(aizome_ipc::Event::KeyCaught { tag, code, keyboard }) => Some((tag, code, keyboard)),
            _ => None,
        })
    };
    let mut asker = catch("setup");
    // Command, beside the space bar, alone: no app ever hears that.
    on("125", true);
    tap("a");
    on("125", false);
    let (tag, code, keyboard) = caught(&mut asker).expect("caught");
    assert_eq!((tag.as_str(), code), ("setup", 125));
    assert_eq!(keyboard.map(|k| (k.name, k.id)), Some(("Apple Inc. Magic Keyboard".into(), "05ac:029c".into())));
    let s = a.wait_seen("a", |s| s.keys.iter().any(|(k, _)| *k == KEY_A));
    assert!(!s.keys.iter().any(|(k, _)| *k == 125), "Command was swallowed: {:?}", s.keys);
    assert_eq!(s.keys.iter().find(|(k, _)| *k == KEY_A).unwrap().1 & SUPER, 0, "and held nothing");
    // Another connection heard nothing of it: the next it hears is a
    // keyboard plugged in after.
    plug("After");
    let mut heard = Vec::new();
    loop {
        let e = others.next().expect("events").expect("the keyboard plugged in after");
        if matches!(&e, aizome_ipc::Event::KeyboardAdded { keyboard } if keyboard.name == "After") {
            break;
        }
        heard.push(e);
    }
    assert!(!heard.iter().any(|e| matches!(e, aizome_ipc::Event::KeyCaught { .. })), "{heard:?}");
    drop(asker);

    // Asked on a connection that doesn't listen, nobody would hear it.
    let mut plain = aizome_ipc::Client::connect_to(&c.ipc).unwrap();
    let r = plain.request(&Request::CatchKey { tag: Some("deaf".into()) }).unwrap();
    assert!(matches!(r, aizome_ipc::Response::Error { .. }), "{r:?}");
    tap("d");
    a.wait_seen("d", |s| s.keys.iter().any(|(k, _)| *k == 32));

    // Another connection's word without a tag changes nothing; the
    // asker's own gives up.
    let mut asker = catch("again");
    assert_eq!(plain.request(&Request::CatchKey { tag: None }).unwrap(), aizome_ipc::Response::Ok);
    tap("c");
    assert_eq!(caught(&mut asker).map(|(t, code, _)| (t, code)), Some(("again".into(), 46)));
    let mut asker = catch("again");
    assert_eq!(asker.request(&Request::CatchKey { tag: None }).unwrap(), aizome_ipc::Response::Ok);
    tap("b");
    a.wait_seen("b", |s| s.keys.iter().any(|(k, _)| *k == KEY_B));
    assert!(!a.seen().keys.iter().any(|(k, _)| *k == 46), "c was caught");

    // The asker gone (Settei quit), nothing is caught.
    drop(catch("setup"));
    // The compositor sees the connection close before the key comes.
    c.settle();
    tap("e");
    a.wait_seen("e", |s| s.keys.iter().any(|(k, _)| *k == 18));
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
