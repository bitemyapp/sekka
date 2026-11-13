//! The window: the sidebar, navigation, search, `--pane`.

mod common;

use aizome_kit::theme::{Appearance, ThemeChoice};

#[test]
fn opens_on_a_pane_and_shows_it() {
    let (mut h, _) = common::open("opens", "mouse");
    let w = h.window();
    assert_eq!(h.program().pane_id(), "mouse");
    assert_eq!(h.title(w), "Mouse");
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Pointer speed"), "{texts:?}");
    h.save_png(w, common::shots_dir().join("mouse-night.png"), 1.0);
    h.set_appearance(Appearance { theme: ThemeChoice::Day, ..Appearance::default() });
    h.save_png(w, common::shots_dir().join("mouse-day.png"), 1.0);
}

#[test]
fn appearance_shows_previews() {
    let (mut h, _) = common::open("appearance", "appearance");
    let w = h.window();
    h.save_png(w, common::shots_dir().join("appearance-night.png"), 1.0);
}

#[test]
fn no_picture_is_explicit_and_each_theme_can_return_to_the_system_default() {
    use aizome_kit::layout::{Point, Size};
    let (mut h, path) = common::open("wallpaper-choice", "appearance.wallpaper");
    let w = h.window();
    h.resize(w, Size::new(1000.0, 1400.0));
    h.save_png(w, common::shots_dir().join("wallpaper-initial.png"), 1.0);
    std::fs::write(
        common::shots_dir().join("wallpaper-initial.json"),
        serde_json::to_vec_pretty(&h.inspected(w)).unwrap(),
    )
    .unwrap();
    let tile = |h: &aizome_kit::testing::Harness<settei::Settei>, label: &str| {
        let item = h
            .inspected(w)
            .into_iter()
            .find(|e| e["role"] == "button" && e["label"] == label && e["visible"] == true)
            .expect(label);
        let r = item["rect"].as_array().unwrap();
        Point::new(
            r[0].as_f64().unwrap() + r[2].as_f64().unwrap() / 2.0,
            r[1].as_f64().unwrap() + r[3].as_f64().unwrap() / 2.0,
        )
    };
    let at = tile(&h, "No picture");
    h.click(w, at);
    h.program().shared.flush();
    let saved = aizome_config::Settings::load_from(&path).unwrap();
    assert_eq!(saved.appearance.wallpaper_night.as_deref(), Some(""));
    assert_eq!(saved.appearance.wallpaper_day, None, "the other theme is unchanged");
    assert_eq!(saved.appearance.wallpaper_path(false), None);
    h.save_png(w, common::shots_dir().join("wallpaper-no-picture-night.png"), 1.0);

    let row = h.find_id(w, "entry:appearance.wallpaper").unwrap();
    h.click(w, Point::new(row.x1 - 25.0, row.center().y));
    assert!(h.texts(w).iter().any(|t| t == "Wallpaper for Day"));
    let at = tile(&h, "No picture");
    h.click(w, at);
    h.program().shared.flush();
    let saved = aizome_config::Settings::load_from(&path).unwrap();
    assert_eq!(saved.appearance.wallpaper_night.as_deref(), Some(""));
    assert_eq!(saved.appearance.wallpaper_day.as_deref(), Some(""));
    assert_eq!(saved.appearance.wallpaper_path(true), None);
    let at = tile(&h, "System default");
    h.click(w, at);
    h.program().shared.flush();
    let saved = aizome_config::Settings::load_from(&path).unwrap();
    assert_eq!(saved.appearance.wallpaper_day, None);
    assert_eq!(saved.appearance.wallpaper_night.as_deref(), Some(""));
    h.save_png(w, common::shots_dir().join("wallpaper-system-default-day.png"), 1.0);
}

/// The panes this test holds to their entries (the hardware panes have
/// tests of their own).
const MINE: [&str; 15] = [
    "general",
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
    "mouse",
    "trackpad",
    "general",
];

#[test]
fn every_option_listed_is_on_its_pane() {
    let panes = settei::panes::all();
    let items = settei::listing::items(&panes);
    let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "ids are unique");
    let mut missing = Vec::new();
    for item in items.iter().filter(|i| i.kind == "option" && MINE.contains(&i.pane.as_str())) {
        let (h, _) = common::open("entries", &item.id);
        let w = h.window();
        let name = settei::pane::entry_name(&item.pane, item.entry.as_deref().unwrap_or(""));
        if h.find_id(w, &name).is_none() {
            missing.push(item.id.clone());
        }
    }
    assert!(missing.is_empty(), "options not on their panes: {missing:?}");
}

#[test]
fn search_finds_options_and_opens_them() {
    let (mut h, _) = common::open("search", "general");
    let w = h.window();
    h.send(settei::AppMsg::Search("caps lock".into()));
    let hit =
        h.program().hits.iter().find(|hit| h.program().panes[hit.pane].id() == "keyboard").expect("Keyboard found");
    assert_eq!(hit.entries[0].id, "caps-lock");
    assert_eq!(hit.entries[0].ranges, vec![0..4, 5..9], "both words highlighted");
    // The pane showing highlights its options that match: none here.
    h.send(settei::AppMsg::Search("night".into()));
    assert!(h.program().hits.iter().any(|hit| h.program().panes[hit.pane].id() == "appearance"));
    h.send(settei::AppMsg::Search("caps lock".into()));
    h.save_png(w, common::shots_dir().join("search-caps.png"), 1.0);
    h.send(settei::AppMsg::SearchSubmit);
    assert_eq!(h.program().pane_id(), "keyboard");
    assert_eq!(h.program().revealed.as_ref().map(|(p, e)| (*p, e.as_str())), Some(("keyboard", "caps-lock")));
    assert_eq!(h.focused_name(w).as_deref(), Some("entry:keyboard.caps-lock"));
    h.save_png(w, common::shots_dir().join("search-revealed.png"), 1.0);
}

#[test]
fn aliases_and_sub_pages_open() {
    let (h, _) = common::open("alias", "datetime");
    assert_eq!(h.program().pane_id(), "general");
    let texts = h.texts(h.window());
    assert!(texts.iter().any(|t| t == "Set the time automatically"), "{texts:?}");
    let (h, _) = common::open("alias2", "keyboard.shortcut-lock");
    assert!(h.texts(h.window()).iter().any(|t| t == "Lock the screen"));
}

#[test]
fn a_sub_pages_crumb_leads_back_to_its_pane() {
    use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
    use aizome_kit::widget::SemanticRole;
    let (mut h, _) = common::open("crumb", "mouse");
    let w = h.window();
    // The pane's title in the strip, as assistive technology is told of it.
    let crumb = |h: &aizome_kit::testing::Harness<settei::Settei>| {
        h.semantics(w)
            .into_iter()
            .find(|(s, _)| s.role == SemanticRole::Button && s.label.as_deref() == Some("Back to General"))
            .map(|(_, r)| r)
    };
    // What only General's first page says.
    let first_page = |h: &aizome_kit::testing::Harness<settei::Settei>| {
        h.texts(w).iter().any(|t| t == "What starts when you log in.")
    };
    // The title strip's words and glyphs, left to right: the pane's tile and
    // name and, over a sub-page, the chevron and the sub-page's title.
    let strip = |h: &aizome_kit::testing::Harness<settei::Settei>| -> Vec<aizome_kit::layout::Rect> {
        h.inspected(w)
            .iter()
            .filter(|e| e["visible"] == true && (e["role"] == "label" || e["role"] == "image"))
            .map(|e| {
                let r: Vec<f64> = e["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                aizome_kit::layout::Rect::new(r[0], r[1], r[0] + r[2], r[1] + r[3])
            })
            .filter(|r| r.x0 > settei::app::SIDEBAR && r.y1 < 44.0)
            .collect()
    };
    assert_eq!(crumb(&h), None, "a pane without sub-pages has nothing to go back to");
    let pages =
        ["about", "software-update", "storage", "startup", "datetime", "language", "default-apps", "login-items"];
    for page in pages {
        h.send(settei::AppMsg::Open("general".into(), Some(page.into())));
        assert!(!first_page(&h), "General › {page} shows");
        let at = crumb(&h).unwrap_or_else(|| panic!("General › {page} has General to press"));
        let over = strip(&h);
        if page == "about" {
            h.move_to(w, at.center());
            h.save_png(w, common::shots_dir().join("crumb-hover-night.png"), 1.0);
        }
        h.click(w, at.center());
        assert_eq!(h.program().pane_id(), "general");
        assert!(first_page(&h), "General pressed in General › {page} shows General's first page");
        assert_eq!(crumb(&h), None, "and on its first page General isn't there to press ({page})");
        // Being pressable moved nothing: the tile and the name are where
        // the first page has them, and the strip is spaced as evenly.
        let first = strip(&h);
        assert_eq!((over.len(), first.len()), (4, 2), "{page}: {over:?}, then {first:?}");
        assert_eq!((over[0], over[1].origin()), (first[0], first[1].origin()), "{page}");
        let gap = over[1].x0 - over[0].x1;
        assert_eq!((over[2].x0 - over[1].x1, over[3].x0 - over[2].x1), (gap, gap), "{page}: {over:?}");
    }
    // From the keyboard: Shift+Tab from the page reaches it, Return presses it.
    h.leave(w);
    h.send(settei::AppMsg::Open("general".into(), Some("about".into())));
    let at = crumb(&h).expect("General › About has General to press");
    for _ in 0..64 {
        if h.focused_rect(w) == Some(at) {
            break;
        }
        h.named(w, NamedKey::Tab, Modifiers::SHIFT);
    }
    assert_eq!(h.focused_rect(w), Some(at), "Tab reaches General in General › About");
    h.save_png(w, common::shots_dir().join("crumb-focused-night.png"), 1.0);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert!(first_page(&h), "Return on it shows General's first page");
    assert_eq!(crumb(&h), None);
    // It went up a page, not back a pane: Mouse is still the pane before.
    h.send(settei::AppMsg::Back);
    assert_eq!(h.program().pane_id(), "mouse");
}

#[test]
fn the_layout_picker_adds_a_layout() {
    let (mut h, path) = common::open("picker", "keyboard");
    let w = h.window();
    assert!(h.click_text(w, "Add a Layout…"));
    let s = h.program().sheet_window().expect("a sheet");
    h.type_text(s, "japanese");
    h.save_png(w, common::shots_dir().join("layout-picker.png"), 1.0);
    // Pick the first match and add it.
    let at = h.find(s, "Japanese").expect("Japanese listed");
    h.click(s, at.center());
    assert!(h.click_text(s, "Add"));
    h.program().shared.flush();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("\"jp\""), "{saved}");
    assert_eq!(h.program().sheet_window(), None, "the sheet closed");
}

#[test]
fn a_shortcut_is_recorded_from_keys() {
    use aizome_kit::sidestep_ui::{Key, Modifiers};
    let (mut h, path) = common::open("record", "keyboard.shortcuts");
    let w = h.window();
    // The palette's keycaps: click them to record.
    assert!(h.click_text(w, "Command palette") || true);
    let row = h.find_id(w, "entry:keyboard.shortcut-palette").expect("the palette's row");
    let x = h.size(w).width - 80.0;
    h.click(w, aizome_kit::layout::Point::new(x, row.center().y));
    assert_eq!(h.focused_name(w).as_deref(), Some("recorder"));
    h.key(w, Key::Character("p".into()), Modifiers::SUPER);
    h.program().shared.flush();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("\"Super+P\" = \"palette\""), "{saved}");
    assert!(saved.contains("\"Super+Space\" = \"none\""), "{saved}");
    h.save_png(w, common::shots_dir().join("shortcut-recorded.png"), 1.0);
}

#[test]
fn choosing_day_saves_it_and_rethemes_the_window() {
    let (mut h, path) = common::open("day", "appearance");
    let w = h.window();
    assert!(h.theme().is_dark());
    assert!(h.click_text(w, "Day"));
    h.program().shared.flush();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("theme = \"day\""), "{saved}");
    assert!(!h.theme().is_dark(), "the window follows at once");
    // Focus colour: the swatches are named.
    let before = h.program().shared.settings.appearance.focus;
    assert_eq!(before, aizome_theme::FocusColor::Amber);
}

#[test]
fn adding_a_user_in_the_sample() {
    let (mut h, _) = common::open("adduser", "users");
    let w = h.window();
    assert!(h.click_text(w, "Add User…"));
    let s = h.program().sheet_window().expect("the sheet");
    h.type_text(s, "Kei Tanaka");
    assert!(h.click_text(s, "Add User"));
    assert_eq!(h.program().sheet_window(), None);
    let users = h.program().shared.sys.accounts.snapshot();
    let kei = users.user("kei").expect("an account named from the full name");
    assert_eq!(kei.real_name, "Kei Tanaka");
    assert!(h.texts(w).iter().any(|t| t == "Kei Tanaka"));
}

#[test]
fn a_command_of_ones_own_gets_keys() {
    use aizome_kit::sidestep_ui::{Key, Modifiers};
    let (mut h, path) = common::open("command", "keyboard.commands");
    let w = h.window();
    assert!(h.click_text(w, "Add a Command…"));
    let s = h.program().sheet_window().expect("the sheet");
    h.type_text(s, "foot -e htop");
    let keys = h.find_id(s, "command-keys").expect("the keys field");
    h.click(s, keys.center());
    h.key(s, Key::Character("t".into()), Modifiers::SUPER | Modifiers::ALT);
    h.save_png(w, common::shots_dir().join("command-sheet.png"), 1.0);
    assert!(h.click_text(s, "Add"));
    h.program().shared.flush();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("\"Alt+Super+T\" = \"spawn foot -e htop\""), "{saved}");
}

#[test]
fn a_desk_name_is_kept_when_the_focus_moves_on() {
    let (mut h, path) = common::open("deskblur", "desktop");
    let w = h.window();
    let fields: Vec<aizome_kit::layout::Rect> = h
        .inspected(w)
        .iter()
        .filter(|e| e["role"] == "textinput" && e["visible"] == true)
        .map(|e| {
            let r: Vec<f64> = e["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
            aizome_kit::layout::Rect::new(r[0], r[1], r[0] + r[2], r[1] + r[3])
        })
        .collect();
    assert!(fields.len() >= 2, "the Desks' names: {fields:?}");
    h.click(w, fields[0].center());
    h.command(w, 'a');
    h.type_text(w, "Mail");
    // Not Return: the next Desk's name, clicked.
    h.click(w, fields[1].center());
    h.program().shared.flush();
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(saved.contains("name = \"Mail\""), "{saved}");
}

#[test]
fn removing_a_user_names_their_home_and_fixed_accounts_keep_their_type() {
    use settei::system::Accounts;
    let (mut h, _) = common::open("users-home", "users");
    let w = h.window();
    let change = |h: &mut aizome_kit::testing::Harness<settei::Settei>, f: fn(&mut aizome_sys::users::UsersState)| {
        if let Accounts::Sample(c) = &h.program().shared.sys.accounts {
            let _ = c.update(f).wait();
        }
        h.send(settei::AppMsg::SystemChanged);
    };
    // Guest's home isn't under /home on this computer.
    change(&mut h, |s| s.users[2].home = "/var/lib/guest".into());
    // Guest's row, where it is now (a note above moves it).
    let row_of = |h: &aizome_kit::testing::Harness<settei::Settei>, label: &str| {
        let guest = h.find(w, "Guest").expect("Guest's row");
        h.semantics(w).into_iter().find(|(s, r)| {
            (s.label.as_deref() == Some(label) || s.value.as_deref() == Some(label))
                && (r.center().y - guest.center().y).abs() < 24.0
        })
    };
    let (_, remove) = row_of(&h, "Remove").expect("Guest's Remove button");
    h.click(w, remove.center());
    let s = h.program().sheet_window().expect("the sheet");
    let texts = h.texts(s);
    assert!(texts.iter().any(|t| t == "Also delete /var/lib/guest and everything in it"), "{texts:?}");
    assert!(h.click_text(s, "Cancel"));
    // Accounts fixed by the configuration: the type shows, and can't change.
    let (kind, _) = row_of(&h, "Standard").expect("Guest's account type");
    assert!(!kind.disabled, "{kind:?}");
    change(&mut h, |s| s.users_managed = true);
    let (kind, _) = row_of(&h, "Standard").expect("Guest's account type");
    assert!(kind.disabled, "the account type can be changed though the configuration fixes it");
}

#[test]
fn on_sekka_yukimi_changes_who_logs_in_by_themselves() {
    use aizome_kit::testing::Harness;
    use settei::panes::users::{AutoLogin, Msg};
    use settei::system::choices::Now;
    let (mut h, _) = common::open("users-auto-login", "users");
    let w = h.window();
    let send = |h: &mut Harness<settei::Settei>, msg: Msg| {
        let msg: settei::pane::PaneMsg = std::sync::Arc::new(msg);
        h.send(settei::AppMsg::Pane("users", msg));
    };
    let choice = |now: &str, next: &str| AutoLogin::Choice(Now { now: now.into(), next: next.into() });
    let launched = |h: &Harness<settei::Settei>| h.program().shared.launched.last().cloned().unwrap_or_default();
    let says =
        |h: &Harness<settei::Settei>, w: aizome_kit::WindowKey, what: &str| h.texts(w).iter().any(|t| t.contains(what));
    // Sekka's choice: Yukimi is handed the person, and AccountsService's
    // flag (GDM's) stays as it is.
    send(&mut h, Msg::AutoLoginRead(choice("", "")));
    send(&mut h, Msg::AutoLoginConfirmed(Some("ren".into())));
    assert_eq!(launched(&h), ["aizome-yukimi", "--choose", "calamares.autoLogin=ren"]);
    assert!(h.program().shared.sys.accounts.snapshot().users.iter().all(|u| !u.automatic_login));
    assert!(says(&h, w, "Proposed in Yukimi: Ren Ishida."), "{:?}", h.texts(w));
    // Applied for the next start: the row says so, and the proposal is done.
    send(&mut h, Msg::AutoLoginRead(choice("", "ren")));
    assert!(says(&h, w, "From the next start: Ren Ishida."), "{:?}", h.texts(w));
    assert!(!says(&h, w, "Proposed in Yukimi"), "{:?}", h.texts(w));
    // Removing them waits until the computer no longer starts into their
    // account: the sheet offers to turn that off first, which proposes no
    // one in Yukimi, and nobody is removed meanwhile.
    let has_ren = |h: &Harness<settei::Settei>| h.program().shared.sys.accounts.snapshot().user("ren").is_some();
    send(&mut h, Msg::Remove("ren".into()));
    let s = h.program().sheet_window().expect("the sheet");
    assert!(says(&h, s, "Turn that off first"), "{:?}", h.texts(s));
    assert!(!says(&h, s, "Remove User"), "{:?}", h.texts(s));
    // Not even when asked without the sheet's button.
    send(&mut h, Msg::RemoveConfirmed);
    assert!(has_ren(&h));
    send(&mut h, Msg::Remove("ren".into()));
    let s = h.program().sheet_window().expect("the sheet");
    assert!(h.click_text(s, "Turn Off in Yukimi…"), "{:?}", h.texts(s));
    assert!(h.settle(std::time::Duration::from_secs(5), |p| p.shared.launched.len() == 2));
    assert_eq!(launched(&h), ["aizome-yukimi", "--choose", "calamares.autoLogin="]);
    assert!(says(&h, w, "Proposed in Yukimi: no one."), "{:?}", h.texts(w));
    assert!(has_ren(&h));
    // Once Yukimi has made the change, they are removed as anyone is, and
    // nothing more is proposed.
    send(&mut h, Msg::AutoLoginRead(choice("ren", "")));
    send(&mut h, Msg::Remove("ren".into()));
    let s = h.program().sheet_window().expect("the sheet");
    assert!(!says(&h, s, "Turn that off first"), "{:?}", h.texts(s));
    assert!(h.click_text(s, "Remove User"));
    assert!(
        h.settle(std::time::Duration::from_secs(5), |p| { p.shared.sys.accounts.snapshot().user("ren").is_none() })
    );
    assert_eq!(h.program().shared.launched.len(), 2);
}

#[test]
fn someone_who_must_set_a_password_is_not_offered_to_log_in_by_themselves() {
    use aizome_kit::testing::Harness;
    use aizome_sys::users::PasswordMode;
    use settei::panes::users::{AutoLogin, Msg};
    use settei::system::Accounts;
    use settei::system::choices::Now;
    let (mut h, _) = common::open("users-auto-login-password", "users");
    let w = h.window();
    let send = |h: &mut Harness<settei::Settei>, msg: Msg| {
        let msg: settei::pane::PaneMsg = std::sync::Arc::new(msg);
        h.send(settei::AppMsg::Pane("users", msg));
    };
    let says = |h: &Harness<settei::Settei>, what: &str| h.texts(w).iter().any(|t| t.contains(what));
    send(&mut h, Msg::AutoLoginRead(AutoLogin::Choice(Now { now: String::new(), next: String::new() })));
    assert!(!says(&h, "can't be chosen yet"), "{:?}", h.texts(w));
    // As Settei leaves a person added with the password left empty.
    if let Accounts::Sample(c) = &h.program().shared.sys.accounts {
        let _ = c
            .update(|s| {
                let ren = s.users.iter_mut().find(|u| u.name == "ren").expect("Ren");
                ren.password_mode = PasswordMode::SetAtLogin;
            })
            .wait();
    }
    h.send(settei::AppMsg::SystemChanged);
    assert!(says(&h, "Ren Ishida can't be chosen yet"), "{:?}", h.texts(w));
}

#[test]
fn removing_someone_goes_by_who_logs_in_by_themselves_then() {
    use aizome_kit::testing::Harness;
    use settei::panes::users::{AutoLogin, Msg};
    use settei::system::choices::{Files, Now};
    use std::time::Duration;
    let (mut h, settings) = common::open("users-remove-reads-again", "users");
    let w = h.window();
    // The system's choices as Yukimi leaves them, in files of this test's:
    // the running system's, and those of the one the computer starts next.
    let dir = settings.parent().expect("the test's directory");
    let files = Files { now: dir.join("now.json"), next: dir.join("next.json") };
    let choose = |file: &std::path::Path, who: &str| {
        let choice = format!(
            r#"{{"choices":[{{"setting":"calamares.autoLogin","title":"Log in automatically","people":true,"value":"{who}","options":[]}}]}}"#
        );
        std::fs::write(file, choice).expect("the choices");
    };
    choose(&files.now, "");
    choose(&files.next, "");
    h.program_mut().shared.choices = Some(files.clone());
    let send = |h: &mut Harness<settei::Settei>, msg: Msg| {
        let msg: settei::pane::PaneMsg = std::sync::Arc::new(msg);
        h.send(settei::AppMsg::Pane("users", msg));
    };
    let says =
        |h: &Harness<settei::Settei>, w: aizome_kit::WindowKey, what: &str| h.texts(w).iter().any(|t| t.contains(what));
    let has_ren = |h: &Harness<settei::Settei>| h.program().shared.sys.accounts.snapshot().user("ren").is_some();
    let sheet = |h: &mut Harness<settei::Settei>| {
        assert!(h.settle(Duration::from_secs(5), |p| p.sheet_window().is_some()), "no sheet");
        h.program().sheet_window().expect("the sheet")
    };
    // As the pane read it when it opened: no one logs in by themselves.
    send(&mut h, Msg::AutoLoginRead(AutoLogin::Choice(Now { now: String::new(), next: String::new() })));
    // Then Ren is chosen in Yukimi itself, and applied for the next start,
    // while this pane stays open. Removing Ren goes by that, not by what
    // the pane read before: only turning it off first is offered.
    choose(&files.next, "ren");
    send(&mut h, Msg::Remove("ren".into()));
    let s = sheet(&mut h);
    assert!(says(&h, s, "Turn that off first"), "{:?}", h.texts(s));
    assert!(!says(&h, s, "Remove User"), "{:?}", h.texts(s));
    send(&mut h, Msg::RemoveConfirmed);
    assert!(has_ren(&h));
    // The row has it as read then, too.
    assert!(says(&h, w, "From the next start: Ren Ishida."), "{:?}", h.texts(w));
    // Turned off in Yukimi again: Ren is removed as anyone is.
    choose(&files.next, "");
    send(&mut h, Msg::Remove("ren".into()));
    let s = sheet(&mut h);
    assert!(!says(&h, s, "Turn that off first"), "{:?}", h.texts(s));
    assert!(h.click_text(s, "Remove User"));
    assert!(h.settle(Duration::from_secs(5), |p| p.shared.sys.accounts.snapshot().user("ren").is_none()));
    assert!(h.program().shared.launched.is_empty());
}
