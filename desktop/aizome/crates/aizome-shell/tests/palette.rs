//! The palette, headless: what it finds for a query, its keys, and its
//! pictures.

mod common;

use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_search::Scope;
use aizome_shell::actions::Action;
use aizome_shell::palette::PaletteEvent;
use aizome_shell::palette::model::SettingEntry;
use aizome_shell::palette::view::PaletteMsg;
use aizome_shell::shell::Msg;
use aizome_sys::apps::{AppIndex, DefaultRole, Locale, XdgDirs};
use common::*;

/// Private desktop entries and defaults; never the session's configuration.
struct DefaultApps {
    root: std::path::PathBuf,
    dirs: XdgDirs,
}

impl DefaultApps {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("aizome-palette-{name}-{}", std::process::id()));
        let dirs = XdgDirs::rooted(&root);
        std::fs::create_dir_all(dirs.data_home.join("applications")).unwrap();
        Self { root, dirs }
    }

    fn write(&self, id: &str, contents: &str) {
        std::fs::write(self.dirs.data_home.join("applications").join(id), contents).unwrap();
    }

    fn shipped(&self, app: &str, id: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../apps/{app}/data/{id}"));
        self.write(id, &std::fs::read_to_string(path).unwrap());
    }

    fn load(&self) -> AppIndex {
        AppIndex::load_with(self.dirs.clone(), Locale::untranslated())
    }
}

impl Drop for DefaultApps {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn default_settings(h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>) {
    h.program_mut().settings_list = DefaultRole::ALL
        .into_iter()
        .map(|role| SettingEntry {
            id: format!("general.{role:?}"),
            title: role.label().into(),
            path: format!("General › Default Apps › {}", role.label()),
            pane: Some("general".into()),
            ..Default::default()
        })
        .collect();
}

fn first_app(
    h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>,
    query: &str,
    id: &str,
) -> aizome_kit::WindowKey {
    let p = open(h, query);
    let state = h.program().palette.as_ref().unwrap();
    assert_eq!(state.selected, 0);
    let item = &state.sections[0].rows[0].item;
    assert_eq!(item.action, Action::FocusApp { app_id: id.into() }, "query: {query}");
    assert!(h.texts(p).contains(&item.title), "the selected app is visible: {query}");
    p
}

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
fn a_purpose_finds_the_terminal_chosen_in_settei() {
    use aizome_sys::apps::{AppIndex, DefaultRole, Locale, XdgDirs};
    let mut h = scene(false);
    let root = std::env::temp_dir().join(format!("aizome-palette-purpose-{}", std::process::id()));
    let dirs = XdgDirs::rooted(&root);
    let apps = dirs.data_home.join("applications");
    std::fs::create_dir_all(&apps).unwrap();
    // Hotaru's shipped entry, not an approximation of it.
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/hotaru/data/org.aizome.Hotaru.desktop"),
        apps.join("org.aizome.Hotaru.desktop"),
    )
    .unwrap();
    for (id, name) in [
        ("foot", "Foot"),
        ("footclient", "Foot Client"),
        ("foot-server", "Foot Server"),
        ("kitty", "kitty"),
        ("other-terminal", "Terminal"),
    ] {
        std::fs::write(
            apps.join(format!("{id}.desktop")),
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nGenericName=Terminal\nExec={id}\nCategories=System;TerminalEmulator;\n"
            ),
        )
        .unwrap();
    }
    let mut index = AppIndex::load_with(dirs, Locale::default());
    index.set_role_default(DefaultRole::Terminal, "org.aizome.Hotaru.desktop").unwrap();
    h.program_mut().apps = Some(std::sync::Arc::new(index));
    default_settings(&mut h);
    // A terminal used before Hotaru was the default.
    for _ in 0..100 {
        h.program_mut().frecency.visit("app:foot");
        h.program_mut().frecency.visit("app:other-terminal");
    }
    let found = |h: &mut aizome_kit::testing::Harness<aizome_shell::shell::Shell>, query: &str| {
        let p = open(h, query);
        let item = h.program().palette.as_ref().unwrap().sections[0].rows[0].item.clone();
        assert!(h.texts(p).contains(&item.title), "the first row is drawn: {query}");
        h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
        item
    };
    for query in ["ter", "term", "terminal", "TERMINAL", " Terminal "] {
        let item = found(&mut h, query);
        assert_eq!(item.action, Action::FocusApp { app_id: "org.aizome.Hotaru".into() }, "query: {query}");
        assert_eq!(item.glyph, aizome_kit::Icon::SquareTerminal);
    }
    assert_eq!(found(&mut h, "foot").action, Action::FocusApp { app_id: "foot".into() });
    // The priority follows the choice, not Hotaru.
    let mut index = std::sync::Arc::try_unwrap(h.program_mut().apps.take().unwrap()).unwrap();
    index.set_role_default(DefaultRole::Terminal, "kitty.desktop").unwrap();
    h.program_mut().apps = Some(std::sync::Arc::new(index));
    assert_eq!(found(&mut h, "terminal").action, Action::FocusApp { app_id: "kitty".into() });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn all_settei_roles_find_apps_whose_names_and_keywords_do_not_describe_the_role() {
    let fixture = DefaultApps::new("all-roles");
    for (n, role) in DefaultRole::ALL.into_iter().enumerate() {
        fixture.write(&format!("chosen-{n}.desktop"), &format!(
            "[Desktop Entry]\nType=Application\nName=選んだアプリ{n}\nExec=chosen-{n}\nCategories=TerminalEmulator;\nMimeType={};\n",
            role.primary_mime_type().unwrap_or("")
        ));
    }
    let mut index = fixture.load();
    for (n, role) in DefaultRole::ALL.into_iter().enumerate() {
        index.set_role_default(role, &format!("chosen-{n}.desktop")).unwrap();
    }
    let mut h = scene(true);
    default_settings(&mut h);
    h.send(Msg::Apps(aizome_shell::shell::AppsLoaded(std::sync::Arc::new(fixture.load()))));
    for (n, role) in DefaultRole::ALL.into_iter().enumerate() {
        for query in [role.label().to_lowercase(), role.label().to_uppercase()] {
            let id = format!("chosen-{n}");
            let p = open(&mut h, &query);
            // This scene has a "Move to Desk > Mail" command, which keeps
            // its precedence under All. Explicitly choosing Apps always
            // selects the app chosen for the purpose.
            h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Scope(Some(Scope::App)))));
            let state = h.program().palette.as_ref().unwrap();
            assert_eq!(state.selected, 0);
            let item = &state.sections[0].rows[0].item;
            assert_eq!(item.action, Action::FocusApp { app_id: id.clone() }, "query: {query}");
            assert!(h.texts(p).contains(&item.title), "chosen app drawn: {query}");
            let before = h.program().frecency.score(&format!("app:{id}"));
            h.named(p, NamedKey::Enter, Modifiers::NONE);
            assert!(h.program().palette.is_none(), "Enter closes the palette: {role:?}");
            assert!(
                h.program().frecency.score(&format!("app:{id}")) > before + 0.9,
                "Enter activates the chosen app: {role:?}"
            );
        }
    }
}

#[test]
fn pdf_documents_and_media_roles_find_realistic_desktop_entries() {
    let fixture = DefaultApps::new("documents-and-media");
    fixture.shipped("shiori", "org.aizome.Shiori.desktop");
    // The unlocalised searchable fields of mpv's desktop entry. Its
    // purpose words are absent, and "pdf"/"documents" are separate in Shiori.
    fixture.write("mpv.desktop", "[Desktop Entry]\nType=Application\nName=mpv Media Player\nGenericName=Multimedia player\nKeywords=mpv;media;player;video;audio;tv;\nExec=mpv %U\nMimeType=video/mp4;audio/mpeg;\n");
    let mut index = fixture.load();
    for (role, id) in [
        (DefaultRole::PdfViewer, "org.aizome.Shiori.desktop"),
        (DefaultRole::VideoPlayer, "mpv.desktop"),
        (DefaultRole::MusicPlayer, "mpv.desktop"),
    ] {
        index.set_role_default(role, id).unwrap();
    }
    let mut h = scene(false);
    default_settings(&mut h);
    h.send(Msg::Apps(aizome_shell::shell::AppsLoaded(std::sync::Arc::new(fixture.load()))));
    for (q, id) in [
        ("pdf documents", "org.aizome.Shiori"),
        ("PDF  documents", "org.aizome.Shiori"),
        ("documents", "org.aizome.Shiori"),
        ("videos", "mpv"),
        ("music", "mpv"),
    ] {
        let p = first_app(&mut h, q, id);
        if q == "music" {
            h.save_png(p, shots().join("palette-purpose-music.png"), 2.0);
        }
        let apps = &h.program().palette.as_ref().unwrap().sections[0].rows;
        assert_eq!(apps.iter().filter(|r| r.item.action == Action::FocusApp { app_id: id.into() }).count(), 1);
        h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
    }
}

#[test]
fn a_default_change_and_removal_refresh_the_prepared_palette() {
    use aizome_shell::palette::AHEAD_AFTER;
    use aizome_shell::shell::AppsLoaded;
    use std::sync::Arc;
    let fixture = DefaultApps::new("default-refresh");
    fixture.shipped("hotaru", "org.aizome.Hotaru.desktop");
    fixture.write(
        "kitty.desktop",
        "[Desktop Entry]\nType=Application\nName=kitty\nExec=kitty\nCategories=TerminalEmulator;\n",
    );
    let mut h = scene(false);
    h.send(Msg::Apps(AppsLoaded(Arc::new(fixture.load()))));
    h.send(Msg::WarmPalette);
    h.advance(AHEAD_AFTER);
    first_app(&mut h, "terminal", "org.aizome.Hotaru");
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
    fixture.load().set_role_default(DefaultRole::Terminal, "kitty.desktop").unwrap();
    // As when the index watcher notices Settei's saved defaults.
    h.send(Msg::Apps(AppsLoaded(Arc::new(fixture.load()))));
    h.advance(AHEAD_AFTER);
    first_app(&mut h, "terminal", "kitty");
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
    std::fs::remove_file(fixture.dirs.data_home.join("applications/kitty.desktop")).unwrap();
    h.send(Msg::Apps(AppsLoaded(Arc::new(fixture.load()))));
    h.advance(AHEAD_AFTER);
    first_app(&mut h, "terminal", "org.aizome.Hotaru");
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Close)));
    std::fs::remove_file(fixture.dirs.data_home.join("applications/org.aizome.Hotaru.desktop")).unwrap();
    h.send(Msg::Apps(AppsLoaded(Arc::new(fixture.load()))));
    h.send(Msg::Run(Action::Palette { scope: Some(Scope::App), query: "terminal".into() }));
    assert!(h.program().palette.as_ref().unwrap().sections.is_empty(), "no stale or nonexistent default");
}

#[test]
fn file_commands_stay_first_and_the_chosen_file_manager_still_beats_settings() {
    let fixture = DefaultApps::new("file-commands");
    fixture.shipped("hako", "org.aizome.Hako.desktop");
    let mut h = scene(false);
    default_settings(&mut h);
    h.send(Msg::Apps(aizome_shell::shell::AppsLoaded(std::sync::Arc::new(fixture.load()))));
    let p = open(&mut h, "file");
    let state = h.program().palette.as_ref().unwrap();
    assert_eq!(state.sections[0].scope, Scope::Command);
    assert_eq!(state.sections[1].scope, Scope::App);
    assert_eq!(state.sections[1].rows[0].item.action, Action::FocusApp { app_id: "org.aizome.Hako".into() });
    assert!(state.sections.iter().skip(2).any(|s| s.scope == Scope::Setting));
    h.save_png(p, shots().join("palette-purpose-file-commands.png"), 2.0);
    // Apps-only lets someone explicitly choose the file manager instead.
    h.send(Msg::Palette(PaletteEvent::Ui(PaletteMsg::Scope(Some(Scope::App)))));
    assert_eq!(h.program().palette.as_ref().unwrap().sections[0].rows[0].item.title, "Hako");
    h.named(p, NamedKey::Enter, Modifiers::NONE);
    assert!(h.program().frecency.score("app:org.aizome.Hako") > 0.9);
}

#[test]
fn typing_and_backspacing_switch_between_purpose_priority_and_ordinary_search() {
    let fixture = DefaultApps::new("purpose-typing");
    fixture.shipped("hotaru", "org.aizome.Hotaru.desktop");
    fixture.write(
        "other.desktop",
        "[Desktop Entry]\nType=Application\nName=Terminal\nExec=other\nCategories=TerminalEmulator;\n",
    );
    let mut h = scene(false);
    h.send(Msg::Apps(aizome_shell::shell::AppsLoaded(std::sync::Arc::new(fixture.load()))));
    for _ in 0..100 {
        h.program_mut().frecency.visit("app:other");
    }
    h.send(Msg::Run(Action::Palette { scope: Some(Scope::App), query: String::new() }));
    let p = window_titled(&h, "Palette");
    let first_title = |h: &aizome_kit::testing::Harness<aizome_shell::shell::Shell>| {
        h.program().palette.as_ref().unwrap().sections[0].rows[0].item.title.clone()
    };
    h.type_text(p, "t");
    h.type_text(p, "e");
    assert_eq!(first_title(&h), "Terminal", "two letters use ordinary matching and history");
    h.named(p, NamedKey::ArrowDown, Modifiers::NONE);
    assert_eq!(h.program().palette.as_ref().unwrap().selected, 1);
    h.type_text(p, "r");
    assert_eq!(first_title(&h), "Hotaru", "three letters recognise the purpose");
    assert_eq!(h.program().palette.as_ref().unwrap().selected, 0, "editing selects the new best result");
    h.named(p, NamedKey::Backspace, Modifiers::NONE);
    assert_eq!(first_title(&h), "Terminal", "backspacing restores ordinary order");
    h.type_text(p, "rminal settings");
    assert_eq!(h.program().palette.as_ref().unwrap().query, "terminal settings");
    assert!(h.program().palette.as_ref().unwrap().sections.is_empty(), "a longer request doesn't force an app result");
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
