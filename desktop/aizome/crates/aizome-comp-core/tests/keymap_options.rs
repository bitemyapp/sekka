//! An empty option list stays empty. Given no options, libxkbcommon reads
//! the session's `XKB_DEFAULT_OPTIONS`, which on a system Genkan set up is
//! the swap that puts ❖ beside the space bar: settings that clear the
//! options, or a keyboard set up to keep its keys, would keep it. Real
//! keymaps, compiled in a subprocess with the session's variable set, as
//! the compositor compiles them.
use aizome_comp_core::keyboards::KeymapSpec;
use smithay::input::keyboard::{Keycode, keysyms, xkb};
use std::process::Command;

/// Whether the key left of the space bar on a PC keyboard (Left Alt,
/// evdev 56) types Super with these options.
fn left_alt_is_super(options: Option<String>) -> bool {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(&context, "", "", "us", "", options, xkb::KEYMAP_COMPILE_NO_FLAGS)
        .expect("a US keymap");
    xkb::State::new(&keymap).key_get_one_sym(Keycode::new(56 + 8)).raw() == keysyms::KEY_Super_L
}

#[test]
fn an_empty_option_list_drops_the_sessions_options() {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "keymap_options_child", "--ignored", "--nocapture"])
        .env("XKB_DEFAULT_OPTIONS", "altwin:swap_alt_win")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn the_recommended_left_swap_preserves_german_altgr() {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",
        "",
        "de",
        "",
        Some(aizome_ipc::setup::SWAP_LEFT_ALT_SUPER.into()),
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .expect("a German keymap");
    let mut state = xkb::State::new(&keymap);
    assert_eq!(state.key_get_one_sym(Keycode::new(56 + 8)).raw(), keysyms::KEY_Super_L);
    assert_eq!(state.key_get_one_sym(Keycode::new(100 + 8)).raw(), keysyms::KEY_ISO_Level3_Shift);
    state.update_key(Keycode::new(100 + 8), xkb::KeyDirection::Down);
    assert_eq!(state.key_get_utf8(Keycode::new(16 + 8)), "@");
}

#[test]
#[ignore = "invoked in a subprocess by an_empty_option_list_drops_the_sessions_options"]
fn keymap_options_child() {
    let spec = |options: &str| KeymapSpec {
        layout: "us".into(),
        variant: String::new(),
        model: String::new(),
        options: options.into(),
    };
    // libxkbcommon itself, given no options, takes the session's swap.
    assert!(left_alt_is_super(None), "XKB_DEFAULT_OPTIONS isn't in effect");
    assert!(left_alt_is_super(spec("altwin:swap_alt_win").xkb().options));
    assert!(!left_alt_is_super(spec("").xkb().options), "cleared options kept the session's swap");
    assert!(!left_alt_is_super(spec("compose:ralt").xkb().options));
}
