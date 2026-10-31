// SPDX-License-Identifier: GPL-3.0-or-later
//! Genkan's window, headless (`aizome_kit::testing::Harness`), on the
//! preview's sample data: no disks, network or helper are touched. Each page
//! draws and says what it should, and the checks between steps hold.
//!
//! `GENKAN_PICTURES=dir` also writes each page, in Night and Day, there.
use aizome_kit::testing::Harness;
use aizome_kit::theme::{Appearance, ThemeChoice};
use aizome_kit::{Point, WindowKey};
use calamares_nixos::applications;
use genkan::Step;
use genkan::app::{Field, Genkan, Msg, StepState};

fn open_in(page: &str, theme: ThemeChoice) -> (Harness<Genkan>, WindowKey) {
    let appearance = Appearance {
        theme,
        reduced_motion: true,
        ..Appearance::default()
    };
    let h = Harness::with(Genkan::new(Some(page.into())), appearance, false);
    let w = h.window();
    (h, w)
}

fn open(page: &str) -> (Harness<Genkan>, WindowKey) {
    open_in(page, ThemeChoice::Night)
}

fn says(h: &Harness<Genkan>, w: WindowKey, what: &str) -> bool {
    h.texts(w).iter().any(|t| t.contains(what))
}

const PAGES: [(&str, &str); 13] = [
    ("welcome", "This installer erases one whole disk"),
    ("disk", "Samsung SSD 990 PRO 1TB"),
    ("account", "Create Your Account"),
    (
        "github",
        "Found 2 public SSH keys on github.com/adalovelace.",
    ),
    ("desktop", "Choose Your Desktops"),
    ("applications", "application selected"),
    ("location", "Time Zone and Language"),
    (
        "review",
        "Everything on Samsung SSD 990 PRO 1TB will be erased",
    ),
    ("ready", "Ready to install"),
    ("prepfail", "Preparation failed"),
    ("install", "Copy the prepared system"),
    ("done", "NixOS Is Installed"),
    ("failed", "Installation Failed"),
];

#[test]
fn every_page_draws_and_says_what_it_should() {
    for (page, expected) in PAGES {
        let (mut h, w) = open(page);
        assert!(
            says(&h, w, expected),
            "{page}: {expected:?} not among {:?}",
            h.texts(w)
        );
        let _ = h.render(w, 1.0);
    }
}

#[test]
fn pictures() {
    let Some(dir) = std::env::var_os("GENKAN_PICTURES") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (theme, name) in [(ThemeChoice::Night, "night"), (ThemeChoice::Day, "day")] {
        for (page, _) in PAGES {
            let (mut h, w) = open_in(page, theme);
            h.save_png(w, dir.join(format!("{page}-{name}.png")), 1.0);
            if page == "applications" {
                h.send(Msg::AppSearch("Uta for YouTube Music".into()));
                h.save_png(
                    w,
                    dir.join(format!("applications-download-{name}.png")),
                    1.0,
                );
            }
        }
    }
}

#[test]
fn get_started_goes_to_the_disk() {
    let (mut h, w) = open("welcome");
    assert!(h.click_text(w, "Get Started"));
    assert_eq!(h.program().step, Step::Disk);
    assert!(says(&h, w, "Step 1 of 8"));
}

#[test]
fn encryption_needs_matching_passphrases() {
    let (mut h, w) = open("disk");
    h.send(Msg::Encrypt(true));
    h.send(Msg::Next);
    assert_eq!(h.program().step, Step::Disk);
    assert_eq!(h.program().flagged, Some(Field::Passphrase));
    assert_eq!(h.focused_name(w).as_deref(), Some("passphrase"));
    h.type_text(w, "a long disk passphrase");
    assert_eq!(h.program().flagged, None);
    h.send(Msg::PassphraseRepeat("another passphrase".into()));
    h.send(Msg::Next);
    assert_eq!(h.program().flagged, Some(Field::PassphraseRepeat));
    assert!(says(&h, w, "Disk passphrases do not match."));
    h.send(Msg::PassphraseRepeat("a long disk passphrase".into()));
    h.send(Msg::Next);
    assert_eq!(h.program().step, Step::Account);
}

#[test]
fn a_blocked_disk_cannot_be_chosen() {
    let (mut h, _) = open("disk");
    h.send(Msg::SelectDisk(2));
    assert_eq!(h.program().selected_disk, Some(0));
}

#[test]
fn the_username_follows_the_full_name_until_edited() {
    let (mut h, _) = open("account");
    h.send(Msg::FullName("Grace Hopper".into()));
    assert_eq!(h.program().username, "grace");
    h.send(Msg::Username("admiral".into()));
    h.send(Msg::FullName("Grace Brewster Hopper".into()));
    assert_eq!(h.program().username, "admiral");
}

#[test]
fn a_short_password_is_pointed_at() {
    let (mut h, w) = open("account");
    h.send(Msg::Password("short".into()));
    h.send(Msg::Next);
    assert_eq!(h.program().step, Step::Account);
    assert_eq!(h.focused_name(w).as_deref(), Some("password"));
    assert!(says(&h, w, "The password needs at least 12 characters"));
}

#[test]
fn keys_found_for_one_username_never_apply_to_another() {
    let (mut h, w) = open("github");
    assert_eq!(h.program().github_keys().len(), 2);
    h.send(Msg::GithubUser("someone-else".into()));
    assert!(h.program().github_keys().is_empty());
    assert!(h.program().github_request().1.is_empty());
    assert!(says(&h, w, "Press Return or Look Up"));
}

#[test]
fn every_desktop_can_be_dropped_but_not_all() {
    let (mut h, w) = open("desktop");
    h.send(Msg::ToggleDesktop(calamares_nixos::Desktop::Aizome, false));
    assert_eq!(
        h.program().default_choice,
        Some(calamares_nixos::Desktop::Tatami)
    );
    h.send(Msg::ToggleDesktop(calamares_nixos::Desktop::Tatami, false));
    h.send(Msg::Next);
    assert_eq!(h.program().step, Step::Desktop);
    assert!(says(&h, w, "Select at least one desktop environment."));
}

#[test]
fn proprietary_applications_need_unfree_software() {
    let (mut h, _) = open("applications");
    let catalog = applications::catalog();
    let Some(index) = catalog.iter().position(|app| app.unfree) else {
        return;
    };
    h.send(Msg::ToggleApp(index, true));
    assert!(h.program().apps[index]);
    h.send(Msg::Unfree(false));
    assert!(!h.program().apps[index]);
    h.send(Msg::ToggleApp(index, true));
    assert!(!h.program().apps[index]);
}

#[test]
fn rustup_brings_the_build_tools() {
    let (mut h, _) = open("applications");
    let catalog = applications::catalog();
    let Some(index) = catalog.iter().position(|app| !app.requires.is_empty()) else {
        return;
    };
    h.send(Msg::ToggleApp(index, true));
    for id in &catalog[index].requires {
        let needed = catalog.iter().position(|app| &app.id == id).unwrap();
        assert!(h.program().apps[needed]);
        h.send(Msg::ToggleApp(needed, false));
        assert!(h.program().apps[needed], "{id} stays while it's needed");
    }
}

#[test]
fn searching_narrows_the_catalog() {
    let (mut h, w) = open("applications");
    h.send(Msg::AppSearch("no application is called this".into()));
    assert!(says(&h, w, "No Matching Applications"));
}

#[test]
fn download_only_applications_are_identified_before_selection() {
    let (mut h, w) = open("applications");
    let catalog = applications::catalog();
    for (id, needs_internet) in [("uta", true), ("firefox", false)] {
        let app = catalog
            .iter()
            .find(|app| app.id == id)
            .expect("catalog application");
        assert_eq!(!app.preload, needs_internet);
        h.send(Msg::AppSearch(app.name.clone()));
        assert!(
            h.find_id(w, &format!("app-{id}")).is_some(),
            "{id} is offered"
        );
        assert_eq!(
            h.texts(w).iter().any(|text| text == "Internet required"),
            needs_internet,
            "{id}"
        );
    }
    assert!(!says(
        &h,
        w,
        "Everything listed is already on this installation media"
    ));
}

#[test]
fn a_click_on_the_map_chooses_a_zone() {
    let (mut h, w) = open("location");
    if h.program().zones.is_none() {
        eprintln!("no tzdata here: set TZDIR");
        return;
    }
    let map = h.find_id(w, "zone-map").expect("the map");
    let before = h.program().zone;
    // Somewhere in western Europe at any window size: a fifth of the way
    // across, a third of the way down a map that's centred in its frame.
    let at = Point::new(map.x0 + map.width() * 0.49, map.y0 + map.height() * 0.3);
    h.click(w, at);
    assert_ne!(h.program().zone, before);
    assert!(says(&h, w, "Chosen on the map."));
}

#[test]
fn the_review_needs_the_exact_phrase_and_consent() {
    let (mut h, w) = open("ready");
    assert!(h.program().next_enabled());
    h.send(Msg::Erase("ERASE /dev/nvme1n1".into()));
    assert!(!h.program().next_enabled());
    h.send(Msg::Erase("ERASE /dev/nvme0n1".into()));
    h.send(Msg::Consent(false));
    assert!(!h.program().next_enabled());
    h.send(Msg::Consent(true));
    assert!(h.program().next_enabled());
    assert!(says(&h, w, "Erase Disk and Install"));
}

#[test]
fn a_failed_preparation_cannot_be_confirmed() {
    let (h, w) = open("prepfail");
    assert!(!h.program().next_enabled());
    assert!(says(&h, w, "sample preview failure"));
}

#[test]
fn the_window_stays_open_while_installing() {
    let (mut h, w) = open("install");
    assert_eq!(h.program().steps[3], StepState::Current);
    h.request_close(w);
    assert_eq!(h.windows().len(), 1);
    assert!(says(
        &h,
        w,
        "cannot be closed while it is writing to the disk"
    ));
}

#[test]
fn back_is_gone_once_installing() {
    let (h, w) = open("install");
    assert!(!h.texts(w).iter().any(|t| t == "Back"));
    assert!(!h.program().reachable(Step::Location));
}

#[test]
fn a_finished_installation_offers_shutting_down() {
    let (h, w) = open("done");
    assert!(says(&h, w, "NixOS Is Installed"));
    assert_eq!(h.program().installed_in(), Some(47.3));
    assert!(h.find_id(w, "shut-down").is_some());
    assert!((h.program().fraction() - 1.0).abs() < 1e-9);
}

#[test]
fn the_review_goes_back_to_the_first_step_that_fails() {
    let (mut h, w) = open("location");
    h.send(Msg::Password("short".into()));
    h.send(Msg::Encrypt(true));
    h.send(Msg::Next);
    assert_eq!(h.program().step, Step::Disk);
    assert_eq!(h.program().flagged, Some(Field::Passphrase));
    assert_eq!(h.focused_name(w).as_deref(), Some("passphrase"));
}

#[test]
fn super_a_selects_a_field_to_type_over() {
    let (mut h, w) = open("account");
    let field = h.find_id(w, "hostname").expect("the computer name field");
    h.click(w, Point::new(field.x1 - 4.0, field.center().y));
    h.command(w, 'a');
    h.type_text(w, "rust-test");
    assert_eq!(h.program().hostname, "rust-test");
}
