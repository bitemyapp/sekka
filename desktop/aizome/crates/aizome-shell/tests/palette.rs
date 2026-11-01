//! The palette, headless: what it finds for a query, its keys, and its
//! pictures.

mod common;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_search::Scope;
use aizome_shell::actions::Action;
use aizome_shell::palette::PaletteEvent;
use aizome_shell::palette::view::PaletteMsg;
use aizome_shell::shell::Msg;
use common::*;

fn open(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, query: &str) -> aizome_kit::WindowKey {
    h.send(Msg::Run(Action::Palette { scope: None, query: String::new() }));
    let p = window_titled(h, "Palette");
    if !query.is_empty() {
        h.type_text(p, query);
    }
    p
}

#[test]
fn typing_finds_the_focused_apps_commands_first() {
    let mut h = scene(false);
    let p = open(&mut h, "bold");
    let s = h.program().palette.as_ref().expect("open");
    assert_eq!(s.query, "bold");
    assert_eq!(s.sections[0].scope, Scope::Command);
    assert_eq!(s.sections[0].rows[0].item.title, "Bold");
    assert_eq!(s.sections[0].rows[0].item.trail, "Format");
    assert!(h.texts(p).iter().any(|t| t == "Bold"));
}

#[test]
fn windows_and_arithmetic() {
    let mut h = scene(false);
    open(&mut h, "cargo");
    let s = h.program().palette.as_ref().unwrap();
    assert!(s.sections.iter().any(|s| s.scope == Scope::Window && s.rows[0].item.title == "cargo test"));
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Query("2^10/4".into()))));
    let s = h.program().palette.as_ref().unwrap();
    assert_eq!(s.sections[0].rows[0].item.title, "= 256");
}

#[test]
fn app_results_activate_existing_windows_and_close_the_palette() {
    use aizome_sys::apps::{AppIndex, Locale, XdgDirs};
    let mut h = scene(false);
    let root = std::env::temp_dir().join(format!("aizome-palette-apps-{}", std::process::id()));
    let dirs = XdgDirs::rooted(&root);
    let apps = dirs.data_home.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(
        apps.join("org.aizome.Sumi.desktop"),
        "[Desktop Entry]\nType=Application\nName=Text Editor\nExec=aizome-sumi\n",
    )
    .unwrap();
    h.program_mut().apps = Some(std::sync::Arc::new(AppIndex::load_with(dirs, Locale::default())));
    h.send(Msg::Run(Action::Palette { scope: Some(Scope::App), query: "Text Editor".into() }));
    let p = window_titled(&h, "Palette");
    let item = &h.program().palette.as_ref().unwrap().sections[0].rows[0].item;
    assert_eq!(item.action, Action::FocusApp { app_id: "org.aizome.Sumi".into() });
    h.named(p, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().palette.is_none(), "activation closes the palette and releases the keyboard");
    assert!(h.program().frecency.score("app:org.aizome.Sumi") > 0.9);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn app_purposes_find_the_default_before_settings_and_other_terminals() {
    use aizome_sys::apps::{AppIndex, DefaultRole, Locale, XdgDirs};
    let mut h = scene(false);
    let root = std::env::temp_dir().join(format!("aizome-palette-discovery-{}", std::process::id()));
    let dirs = XdgDirs::rooted(&root);
    let apps = dirs.data_home.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    // Exercise shipped metadata, not a hand-written approximation of it.
    for (app, name) in [
        ("hotaru", "Hotaru"),
        ("hako", "Hako"),
        ("sumi", "Sumi"),
        ("settei", "Settei"),
        ("soroban", "Soroban"),
        ("nisshi", "Nisshi"),
    ] {
        let file = format!("org.aizome.{name}.desktop");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../apps/{app}/data/{file}")),
            apps.join(file),
        )
        .unwrap();
    }
    for (id, name, generic, categories, mime) in [
        ("foot", "Foot", "Terminal", "System;TerminalEmulator;", ""),
        ("footclient", "Foot Client", "Terminal", "System;TerminalEmulator;", ""),
        ("foot-server", "Foot Server", "Terminal", "System;TerminalEmulator;", ""),
        ("kitty", "kitty", "Terminal", "System;TerminalEmulator;", ""),
        ("firefox", "Firefox", "Web Browser", "Network;WebBrowser;", "x-scheme-handler/https;"),
    ] {
        std::fs::write(apps.join(format!("{id}.desktop")), format!(
            "[Desktop Entry]\nType=Application\nName={name}\nGenericName={generic}\nExec={id}\nCategories={categories}\nMimeType={mime}\n"
        )).unwrap();
    }
    let mut index = AppIndex::load_with(dirs, Locale::default());
    for (role, id) in [
        (DefaultRole::Terminal, "org.aizome.Hotaru.desktop"),
        (DefaultRole::FileManager, "org.aizome.Hako.desktop"),
        (DefaultRole::TextEditor, "org.aizome.Sumi.desktop"),
        (DefaultRole::Browser, "firefox.desktop"),
    ] {
        index.set_role_default(role, id).unwrap();
    }
    h.program_mut().apps = Some(std::sync::Arc::new(index));
    // A previously used alternative must not bury the chosen default for
    // a purpose query. An explicit app name must still find that app.
    for _ in 0..100 {
        h.program_mut().frecency.visit("app:foot");
    }
    let mut terminal = aizome_shell::palette::model::SettingEntry::default();
    terminal.id = "general.default-apps.terminal".into();
    terminal.title = "General › Default Apps › Terminal".into();
    terminal.keywords = vec!["terminal settings".into()];
    h.program_mut().settings_list = vec![terminal];
    for (query, wanted) in [
        ("term", "org.aizome.Hotaru"),
        ("terminal", "org.aizome.Hotaru"),
        ("TERM", "org.aizome.Hotaru"),
        ("shell", "org.aizome.Hotaru"),
        ("console", "org.aizome.Hotaru"),
        ("command line", "org.aizome.Hotaru"),
        ("hotaru terminal", "org.aizome.Hotaru"),
        ("foot", "foot"),
        ("kitty", "kitty"),
        ("files", "org.aizome.Hako"),
        ("folders", "org.aizome.Hako"),
        ("text editor", "org.aizome.Sumi"),
        ("sumi editor", "org.aizome.Sumi"),
        ("browser", "firefox"),
        ("calculator", "org.aizome.Soroban"),
        ("logs", "org.aizome.Nisshi"),
        ("settings", "org.aizome.Settei"),
    ] {
        let p = open(&mut h, query);
        let palette = h.program().palette.as_ref().unwrap();
        assert_eq!(palette.selected, 0);
        assert_eq!(palette.scroll, 0.0);
        let item = &palette.sections[0].rows[0].item;
        assert_eq!(item.action, Action::FocusApp { app_id: wanted.into() }, "query: {query}");
        assert!(h.texts(p).contains(&item.title), "the first app is drawn without scrolling: {query}");
        if wanted == "org.aizome.Hotaru" {
            assert_eq!(item.glyph, aizome_kit::Icon::SquareTerminal);
        }
        h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
    }
    h.send(Msg::Run(Action::Palette { scope: None, query: "terminal settings".into() }));
    assert_eq!(h.program().palette.as_ref().unwrap().sections[0].scope, Scope::Setting);
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
    let mut index = std::sync::Arc::try_unwrap(h.program_mut().apps.take().unwrap()).unwrap();
    index.set_role_default(DefaultRole::Terminal, "kitty.desktop").unwrap();
    h.program_mut().apps = Some(std::sync::Arc::new(index));
    h.send(Msg::Run(Action::Palette { scope: None, query: "terminal".into() }));
    assert_eq!(
        h.program().palette.as_ref().unwrap().sections[0].rows[0].item.action,
        Action::FocusApp { app_id: "kitty".into() }
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn keys_move_scope_run_and_close() {
    let mut h = scene(false);
    let p = open(&mut h, "");
    h.named(p, NamedKey::Tab, Modifiers::NONE);
    assert_eq!(h.program().palette.as_ref().unwrap().scope, Some(Scope::Command));
    h.named(p, NamedKey::Tab, Modifiers::SHIFT);
    assert_eq!(h.program().palette.as_ref().unwrap().scope, None);
    h.type_text(p, "lock");
    h.named(p, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(p, NamedKey::ArrowUp, Modifiers::NONE);
    assert_eq!(h.program().palette.as_ref().unwrap().selected, 0);
    h.named(p, NamedKey::Escape, Modifiers::NONE);
    assert!(h.program().palette.is_none());
    assert!(h.windows().iter().all(|k| h.title(*k) != "Palette"));
}

#[test]
fn running_a_command_asks_the_app() {
    let mut h = scene(false);
    let p = open(&mut h, "italic");
    h.named(p, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().palette.is_none(), "running closes it");
    // The pick is remembered for the coach and ranking.
    assert!(h.program().frecency.score("menu:org.aizome.Sumi:Format/Italic") > 0.9);
}

#[test]
fn pictures_of_the_palette() {
    for day in [false, true] {
        let mut h = scene(day);
        let p = open(&mut h, "f");
        {
            // The coach has something to say about Find… after picks.
            let s = h.program_mut();
            for _ in 0..14 {
                s.frecency.visit("menu:org.aizome.Sumi:Format/Bold");
            }
        }
        h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Query("bo".into()))));
        let name = if day { "palette-day.png" } else { "palette-night.png" };
        h.save_png(p, shots().join(name), 2.0);
        aizome_kit::testing::assert_golden(&mut h, p, golden(name), 2.0);
    }
}

/// Two seconds after start the shell warms the palette, and a second later
/// draws it ahead, hidden, as it would open: its first opening then shows
/// that drawing and draws only what changed since (nothing, here), not the
/// whole surface, and what shows is the palette drawn whole. Closed, it's kept hidden and drawn ahead again a second later;
/// a new theme draws it ahead again in that theme.
#[test]
fn the_first_opening_draws_only_what_changed_since_it_was_drawn_ahead() {
    use aizome_shell::palette::AHEAD_AFTER;
    let mut h = scene(false);
    h.send(Msg::WarmPalette);
    assert!(h.windows().iter().all(|k| h.title(*k) != "Palette"), "drawn ahead a second later");
    h.advance(AHEAD_AFTER);
    let p = window_titled(&h, "Palette");
    assert!(!h.is_shown(p), "drawn ahead hidden");
    let size = h.size(p);
    let whole = size.width * size.height;
    let ahead = h.check_frame(p);
    assert!(ahead.damaged >= whole, "drawn ahead whole: {} of {whole} points²", ahead.damaged);
    // Its bands, put together, are the palette drawn whole, pixel for pixel.
    let (w, hgt, drawn) = h.picture(p).expect("drawn ahead");
    let image = h.render(p, 2.0);
    assert_eq!(image.pixel_size(), (w, hgt));
    assert_eq!(aizome_kit::testing::difference(&drawn, &image.to_rgba8().expect("pixels"), 0), 0.0);
    for (round, day) in [(0, false), (1, false), (2, true)] {
        h.send(Msg::Run(Action::Palette { scope: None, query: String::new() }));
        assert_eq!(window_titled(&h, "Palette"), p, "the window drawn ahead shows");
        assert!(h.is_shown(p));
        let frame = h.check_frame(p);
        assert_eq!(frame.differs, 0.0, "round {round}: what shows is the palette drawn whole");
        assert!(
            frame.damaged < whole / 8.0,
            "round {round}: the first frame drew {} of {whole} points², not just what changed",
            frame.damaged
        );
        println!(
            "round {round}: the first opening drew {:.0} of {whole:.0} points² ({:.0} of {:.0} pixels at 2×)",
            frame.damaged,
            frame.damaged * 4.0,
            whole * 4.0
        );
        assert_eq!(h.focused_name(p).as_deref(), Some("palette-query"));
        h.named(p, NamedKey::Escape, Modifiers::NONE);
        assert!(h.program().palette.is_none());
        assert!(!h.is_shown(p), "round {round}: kept, hidden");
        if day {
            break;
        }
        h.advance(AHEAD_AFTER);
        assert!(h.check_frame(p).damaged >= whole, "round {round}: drawn ahead again");
        if round == 1 {
            // A new theme: drawn ahead again, in it.
            h.set_appearance(appearance(true));
            // (At idle: the loop's next turns.)
            h.advance(std::time::Duration::ZERO);
            assert!(h.check_frame(p).damaged >= whole, "drawn ahead again in the new theme");
        }
    }
}
