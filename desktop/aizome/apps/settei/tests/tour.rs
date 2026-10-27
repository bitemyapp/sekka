//! Pictures of every pane, Night and Day, tall enough to see them whole,
//! for people to look at (`SETTEI_SHOTS=dir`). Not goldens: those are
//! in `goldens.rs`.

mod common;

use aizome_kit::layout::Size;

fn shoot(target: &str, name: &str, day: bool) {
    let (mut h, path) = common::open(name, "");
    if day {
        std::fs::write(&path, "[appearance]\ntheme = \"day\"\n").unwrap();
    }
    drop(h);
    let start = settei::Start::parse(target);
    h = aizome_kit::testing::Harness::new(settei::Settei::sample(path, start));
    let w = h.window();
    h.resize(w, Size::new(940.0, 1300.0));
    let suffix = if day { "day" } else { "night" };
    h.save_png(w, common::shots_dir().join(format!("{name}-{suffix}.png")), 1.0);
}

#[test]
fn every_pane() {
    if std::env::var_os("SETTEI_SHOTS").is_none() {
        return;
    }
    let only = std::env::var("SETTEI_TOUR").unwrap_or_default();
    let targets = [
        "general",
        "general.about",
        "general.software-update",
        "general.storage",
        "general.startup",
        "general.datetime",
        "general.language",
        "general.default-apps",
        "general.login-items",
        "appearance",
        "accessibility",
        "desktop",
        "menubar",
        "battery",
        "performance",
        "search",
        "notifications",
        "lock",
        "users",
        "keyboard",
        "keyboard.shortcuts",
        "mouse",
        "trackpad",
    ];
    for t in targets {
        if !only.is_empty() && !only.split(',').any(|o| t.starts_with(o)) {
            continue;
        }
        let name = t.replace('.', "-");
        shoot(t, &name, false);
        shoot(t, &name, true);
    }
}
