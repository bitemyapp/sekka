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
