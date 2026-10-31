//! Run the real loader in subprocesses: environment defaults must not race
//! other tests or require mutating the process environment after threads start.
use aizome_config::Settings;
use std::process::Command;

#[test]
fn system_keyboard_defaults_and_user_overrides() {
    for case in ["unset", "empty", "system", "user"] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "keyboard_defaults_child", "--ignored", "--nocapture"]);
        for name in ["LAYOUT", "VARIANT", "OPTIONS", "MODEL"] {
            child.env_remove(format!("XKB_DEFAULT_{name}"));
        }
        child.env("AIZOME_KEYBOARD_TEST_CASE", case);
        if case == "empty" {
            child.env("XKB_DEFAULT_LAYOUT", "");
        } else if case == "system" || case == "user" {
            child
                .env("XKB_DEFAULT_LAYOUT", "gb,de")
                .env("XKB_DEFAULT_VARIANT", ",nodeadkeys")
                .env("XKB_DEFAULT_OPTIONS", "grp:alt_shift_toggle,caps:escape")
                .env("XKB_DEFAULT_MODEL", "pc105");
        }
        assert!(child.status().unwrap().success(), "keyboard case {case}");
    }
}

#[test]
#[ignore = "invoked in isolated subprocesses by system_keyboard_defaults_and_user_overrides"]
fn keyboard_defaults_child() {
    let case = std::env::var("AIZOME_KEYBOARD_TEST_CASE").unwrap();
    if case == "unset" || case == "empty" {
        let keyboard = Settings::default().input.keyboard;
        assert_eq!(keyboard.layouts, ["us"]);
        assert_eq!(keyboard.variant, "");
        assert_eq!(keyboard.options, "");
        assert_eq!(keyboard.model, "");
        return;
    }

    let dir = std::env::temp_dir().join(format!("aizome-keyboard-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.toml");
    let fresh = Settings::load_from(&path).unwrap();
    assert_eq!(fresh.input.keyboard.layouts, ["gb", "de"]);
    assert_eq!(fresh.input.keyboard.variant, ",nodeadkeys");
    assert_eq!(fresh.input.keyboard.options, "grp:alt_shift_toggle,caps:escape");
    assert_eq!(fresh.input.keyboard.model, "pc105");
    assert!(!path.exists(), "loading defaults must not create a user override");

    let partial = Settings::from_toml("[input.keyboard]\nrepeat_rate = 31\n").unwrap();
    assert_eq!(partial.input.keyboard.layouts, fresh.input.keyboard.layouts);
    assert_eq!(partial.input.keyboard.repeat_rate, 31);
    let per_device = Settings::from_toml(
        "[[input.keyboards]]\nname = 'Test keyboard'\nid = '1234:5678'\noptions = 'compose:rwin'\n",
    )
    .unwrap();
    assert_eq!(per_device.input.keyboard.layouts, ["gb", "de"]);
    assert_eq!(per_device.input.options_for("Test keyboard", "1234:5678"), "grp:alt_shift_toggle,compose:rwin");
    assert_eq!(per_device.input.options_for("Other", "0000:0000"), fresh.input.keyboard.options);

    if case == "user" {
        std::fs::write(&path, "[input.keyboard]\nlayouts = ['us']\nvariant = ''\noptions = ''\nmodel = 'pc104'\n")
            .unwrap();
        let explicit = Settings::load_from(&path).unwrap();
        assert_eq!(explicit.input.keyboard.layouts, ["us"]);
        assert_eq!(explicit.input.keyboard.variant, "");
        assert_eq!(explicit.input.keyboard.options, "");
        assert_eq!(explicit.input.keyboard.model, "pc104");
        explicit.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), explicit);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
