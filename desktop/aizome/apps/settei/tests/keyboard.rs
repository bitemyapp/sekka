//! Modifier changes through the real dropdown, persisted and reopened.

mod common;

use aizome_config::Settings;
use aizome_kit::layout::Size;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use settei::{Settei, Start};

fn choose(h: &mut Harness<Settei>, current: &str, index: usize) {
    let w = h.window();
    assert!(h.click_text(w, current), "{:?}", h.texts(w));
    let popup = h.popups(w).last().map(|(p, _)| *p).expect("modifier menu");
    h.popup_keyboard(popup, Key::Named(NamedKey::Home), Modifiers::default());
    for _ in 0..index {
        h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    }
    h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::default());
    h.program().shared.flush();
}

#[test]
fn modifier_menu_saves_reopens_and_restores_defaults() {
    let dir = common::temp_dir("modifier-keys");
    let path = dir.join("settings.toml");
    std::fs::write(&path, "[input.keyboard]\noptions = \"ctrl:nocaps,compose:menu,grp:alt_shift_toggle\"\n").unwrap();
    let mut h = Harness::new(Settei::sample(path.clone(), Start::parse("keyboard.modifiers")));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    assert!(h.texts(w).iter().any(|t| t == "Modifier keys"));
    choose(&mut h, "Default", 1);
    assert_eq!(
        Settings::load_from(&path).unwrap().input.keyboard.options,
        "ctrl:nocaps,compose:menu,grp:alt_shift_toggle,altwin:swap_alt_win"
    );
    h.save_png(w, common::shots_dir().join("keyboard-modifiers.png"), 1.0);
    drop(h);

    let mut h = Harness::new(Settei::sample(path.clone(), Start::parse("keyboard.modifiers")));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    assert!(h.texts(w).iter().any(|t| t == "Swap Alt and Super"));
    choose(&mut h, "Swap Alt and Super", 2);
    assert_eq!(
        Settings::load_from(&path).unwrap().input.keyboard.options,
        "ctrl:nocaps,compose:menu,grp:alt_shift_toggle,altwin:swap_lalt_lwin"
    );
    choose(&mut h, "Swap left Alt and Super", 0);
    assert_eq!(
        Settings::load_from(&path).unwrap().input.keyboard.options,
        "ctrl:nocaps,compose:menu,grp:alt_shift_toggle"
    );
}

#[test]
fn custom_modifiers_are_shown_without_rewriting_them() {
    let dir = common::temp_dir("custom-modifier-keys");
    let path = dir.join("settings.toml");
    let original = "[input.keyboard]\noptions = \"altwin:meta_win,compose:menu\"\n";
    std::fs::write(&path, original).unwrap();
    let mut h = Harness::new(Settei::sample(path.clone(), Start::parse("keyboard.modifiers")));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    assert!(h.texts(w).iter().any(|t| t == "Custom"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    choose(&mut h, "Custom", 1);
    assert_eq!(Settings::load_from(&path).unwrap().input.keyboard.options, "compose:menu,altwin:swap_alt_win");
}

#[test]
fn modifier_keys_are_discoverable_by_macos_and_key_names() {
    let panes = settei::panes::all();
    let keyboard = panes.iter().find(|p| p.id() == "keyboard").unwrap();
    let entries = keyboard.entries();
    let infos = [settei::search::PaneInfo { title: "Keyboard", keywords: &[], entries: &entries }];
    for query in ["macOS", "swap super alt", "command", "modifier", "control"] {
        let hits = settei::search::find(query, &infos);
        assert!(hits.iter().any(|h| h.entries.iter().any(|e| e.id == "modifiers")), "{query}");
    }
}

/// Choose from the setup sheet's menu `id` (the pane beneath has menus
/// showing the same).
fn choose_in(h: &mut Harness<Settei>, w: aizome_kit::WindowKey, id: &str, index: usize) {
    let menu = h.find_id(w, id).unwrap_or_else(|| panic!("{id}"));
    h.click(w, menu.center());
    let popup = h.popups(w).last().map(|(p, _)| *p).expect("a menu");
    h.popup_keyboard(popup, Key::Named(NamedKey::Home), Modifiers::default());
    for _ in 0..index {
        h.popup_keyboard(popup, Key::Named(NamedKey::ArrowDown), Modifiers::default());
    }
    h.popup_keyboard(popup, Key::Named(NamedKey::Enter), Modifiers::default());
    h.program().shared.flush();
}

/// A key on the sample's keyboard (its Magic Keyboard), by evdev code.
fn press(h: &mut Harness<Settei>, code: u32) {
    let msg: settei::pane::PaneMsg = std::sync::Arc::new(settei::panes::keyboard::Msg::SampleKey(code));
    h.send(settei::AppMsg::Pane("keyboard", msg));
}

const MAC: &str = "Apple Inc. Magic Keyboard";

fn sample(name: &str, settings: &str, target: &str) -> (Harness<Settei>, std::path::PathBuf) {
    let dir = common::temp_dir(name);
    let path = dir.join("settings.toml");
    std::fs::write(&path, settings).unwrap();
    let mut h = Harness::new(Settei::sample(path.clone(), Start::parse(target)));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1180.0));
    (h, path)
}

#[test]
fn a_keyboard_of_its_own_changes_only_its_keys() {
    let (mut h, path) = sample(
        "keyboard-of-its-own",
        "[input.keyboard]\noptions = \"altwin:swap_alt_win,grp:alt_shift_toggle\"\n\
         [[input.keyboards]]\nname = \"Apple Inc. Magic Keyboard\"\nid = \"05ac:029c\"\noptions = \"\"\n",
        "keyboard.keyboards",
    );
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == "Swap Alt and Super"), "other keyboards' at first");
    choose(&mut h, "Other keyboards", 1);
    assert!(h.texts(w).iter().any(|t| t == "Default"), "the Mac's own: {:?}", h.texts(w));
    choose(&mut h, "Default", 2);
    choose(&mut h, "Caps Lock", 1);
    let s = Settings::load_from(&path).unwrap();
    assert_eq!(s.input.keyboard.options, "altwin:swap_alt_win,grp:alt_shift_toggle", "others' unchanged");
    assert_eq!(s.input.keyboards[0].options.as_deref(), Some("altwin:swap_lalt_lwin,ctrl:nocaps"));
    h.save_png(w, common::shots_dir().join("keyboard-of-its-own.png"), 1.0);

    // Forgotten, it types as the others do.
    let forget = h.find_id(w, "forget").expect("Forget");
    h.click(w, forget.center());
    h.program().shared.flush();
    assert!(Settings::load_from(&path).unwrap().input.keyboards.is_empty());
    assert!(h.texts(w).iter().any(|t| t == "All keyboards"), "{:?}", h.texts(w));
    assert!(h.texts(w).iter().any(|t| t == "Swap Alt and Super"));
}

#[test]
fn setting_up_a_mac_keyboard_leaves_super_beside_the_space_bar() {
    let (mut h, path) =
        sample("setup-mac", "[input.keyboard]\noptions = \"altwin:swap_alt_win,ctrl:nocaps\"\n", "keyboard.setup");
    let s = h.program().sheet_window().expect("the setup sheet");
    assert!(h.texts(s).iter().any(|t| t == "Waiting for the key…"), "{:?}", h.texts(s));
    // The space bar itself: the key to its left, please.
    press(&mut h, 57);
    assert!(h.texts(s).iter().any(|t| t.starts_with("That's the space bar")), "{:?}", h.texts(s));
    // Command.
    press(&mut h, 125);
    let texts = h.texts(s);
    assert!(texts.iter().any(|t| t == MAC), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("❖ Super (⌘ Command")), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Default"), "nothing to swap: {texts:?}");
    assert!(!texts.iter().any(|t| t == "Caps Lock and Compose"), "nothing else to start from");
    h.save_png(s, common::shots_dir().join("keyboard-setup-mac.png"), 1.0);
    assert!(h.click_text(s, "Save"));
    assert_eq!(h.program().sheet_window(), None);
    h.program().shared.flush();
    let saved = Settings::load_from(&path).unwrap();
    assert_eq!(saved.input.keyboard.options, "altwin:swap_alt_win,ctrl:nocaps", "the PC's stay swapped");
    let mac = saved.input.keyboard_profile(MAC, "05ac:029c").expect("the Mac's own");
    assert_eq!(mac.options.as_deref(), Some("ctrl:nocaps"), "unswapped, with the others' Caps Lock");
    let w = h.window();
    assert!(h.texts(w).iter().any(|t| t == MAC), "the Keys section shows it: {:?}", h.texts(w));
}

#[test]
fn setting_up_a_pc_keyboard_swaps_and_can_start_from_another() {
    let (mut h, path) = sample(
        "setup-pc",
        "[input.keyboard]\noptions = \"ctrl:nocaps\"\n\
         [[input.keyboards]]\nname = \"Logitech K120\"\nid = \"046d:c31c\"\noptions = \"compose:ralt\"\n",
        "keyboard",
    );
    let w = h.window();
    assert!(h.click_text(w, "Set Up…"));
    let s = h.program().sheet_window().expect("the setup sheet");
    // Alt beside the space bar.
    press(&mut h, 56);
    let texts = h.texts(s);
    assert!(texts.iter().any(|t| t == "Swap Alt and Super"), "{texts:?}");
    assert!(texts.iter().any(|t| t == "Caps Lock and Compose"), "another keyboard to start from");
    choose_in(&mut h, s, "setup-from", 1);
    assert!(h.click_text(s, "Save"));
    h.program().shared.flush();
    let saved = Settings::load_from(&path).unwrap();
    assert_eq!(
        saved.input.keyboard_profile(MAC, "05ac:029c").and_then(|k| k.options.as_deref()),
        Some("compose:ralt,altwin:swap_alt_win"),
        "the Logitech's Compose key, with Alt and Super swapped"
    );

    // Set up again, it starts from its own; Escape gives up.
    assert!(h.click_text(w, "Set Up…"));
    press(&mut h, 1);
    assert_eq!(h.program().sheet_window(), None);
    // As other keyboards: still known, nothing of its own.
    assert!(h.click_text(w, "Set Up…"));
    let s = h.program().sheet_window().expect("the setup sheet");
    press(&mut h, 125);
    assert!(h.click_text(s, "Same as Other Keyboards"));
    h.program().shared.flush();
    let saved = Settings::load_from(&path).unwrap();
    assert_eq!(saved.input.keyboard_profile(MAC, "05ac:029c").map(|k| k.options.clone()), Some(None));
}

#[test]
fn a_keyboard_with_no_space_bar_types_as_the_others_without_a_key() {
    let (mut h, path) =
        sample("setup-receiver", "[input.keyboard]\noptions = \"altwin:swap_alt_win\"\n", "keyboard.setup");
    let s = h.program().sheet_window().expect("the setup sheet");
    let texts = h.texts(s);
    assert!(texts.iter().any(|t| t == "Waiting for the key…"), "{texts:?}");
    assert!(texts.iter().any(|t| t == MAC), "the keyboards not set up, the first chosen: {texts:?}");
    // The mouse's receiver says it's a keyboard, with no space bar to
    // press beside: chosen, it types as the others do, and is known.
    choose_in(&mut h, s, "setup-keyboard", 1);
    assert!(h.texts(s).iter().any(|t| t == "Logitech USB Receiver"), "{:?}", h.texts(s));
    h.save_png(s, common::shots_dir().join("keyboard-setup-receiver.png"), 1.0);
    assert!(h.click_text(s, "Same as Other Keyboards"));
    assert_eq!(h.program().sheet_window(), None);
    h.program().shared.flush();
    let saved = Settings::load_from(&path).unwrap();
    assert_eq!(
        saved.input.keyboard_profile("Logitech USB Receiver", "046d:c548").map(|k| k.options.clone()),
        Some(None)
    );
    assert!(saved.input.keyboard_profile(MAC, "05ac:029c").is_none(), "the Mac isn't set up");
    assert_eq!(saved.input.keyboard.options, "altwin:swap_alt_win");

    // Set up again, the Mac is left to choose; a key caught names its
    // keyboard, and the menu goes.
    let w = h.window();
    assert!(h.click_text(w, "Set Up…"));
    let s = h.program().sheet_window().expect("the setup sheet");
    assert!(h.find_id(s, "setup-keyboard").is_some());
    press(&mut h, 125);
    assert!(h.find_id(s, "setup-keyboard").is_none());
    assert!(h.click_text(s, "Same as Other Keyboards"));
    h.program().shared.flush();
    let saved = Settings::load_from(&path).unwrap();
    assert_eq!(saved.input.keyboards.len(), 2, "{:?}", saved.input.keyboards);
    assert_eq!(saved.input.keyboard_profile(MAC, "05ac:029c").map(|k| k.options.clone()), Some(None));
}
