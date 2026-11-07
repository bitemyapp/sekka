//! First-boot setup on the login screen, headless: each page, what it says
//! and does with the keyboard, the helper's answers played to it, and
//! pictures of every page (`target/greeter-shots/setup-*`) over the night
//! and the day wallpaper. Set `AIZOME_GREETER_WALLPAPERS` to a directory
//! with `da-nang-at-night.jpg` and `tokyo-at-dusk.jpg` (Tatami's), and
//! `AIZOME_FONT_DIR` for Aizome's type.

use std::path::{Path, PathBuf};

use aizome_greeter::app::{Greeter, Msg, Stage};
use aizome_greeter::config::{Config, State};
use aizome_greeter::login::{Step, Tone};
use aizome_greeter::sessions::{Kind, Session};
use aizome_greeter::setup::{
    Checked, Choice, DesktopChoice, Done, Draft, Failure, Field, Offer, Page, Progress, Response, SetupMsg, Status,
    Zone,
};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;

const W: f64 = 1440.0;
const H: f64 = 900.0;

fn output() -> aizome_kit::Output {
    aizome_kit::Output {
        id: 1,
        name: "eDP-1".into(),
        connector: Some("eDP-1".into()),
        description: None,
        make: "Apple".into(),
        model: "J514S".into(),
        frame: Rect::new(0.0, 0.0, W, H),
        scale: 2.0,
        integer_scale: 2,
        physical_size: None,
        refresh_hz: 120.0,
        work_area: None,
    }
}

fn session(id: &str, name: &str) -> Session {
    Session {
        id: id.into(),
        name: name.into(),
        comment: String::new(),
        exec: vec![format!("{id}-session")],
        desktop_names: vec![name.into()],
        kind: Kind::Wayland,
        path: PathBuf::from(format!("/s/{id}.desktop")),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Light {
    Night,
    Day,
}

fn wallpaper(light: Light) -> Option<PathBuf> {
    let dir = std::env::var_os("AIZOME_GREETER_WALLPAPERS")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/run/current-system/sw/share/backgrounds/tatami".into());
    let p = dir.join(if light == Light::Night { "da-nang-at-night.jpg" } else { "tokyo-at-dusk.jpg" });
    p.exists().then_some(p)
}

/// Sekka's lockup, from the brand's files beside the desktop.
fn logo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../docs/brand/logos/sekka-lockup-night.svg")
}

fn config(light: Light) -> Config {
    Config {
        default_session: Some("aizome".into()),
        wallpaper_night: wallpaper(light),
        wallpaper_day: wallpaper(light),
        wallpaper: if light == Light::Night { "night".into() } else { "day".into() },
        setup_socket: Some("/run/sekka-setup.sock".into()),
        setup_logo: Some(logo()),
        ..Config::default()
    }
}

fn shots() -> PathBuf {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("greeter-shots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn choice(id: &str, label: &str) -> Choice {
    Choice { id: id.into(), label: label.into() }
}

fn zone(id: &str, city: &str, country: &str) -> Zone {
    Zone { id: id.into(), city: city.into(), country: country.into() }
}

/// What the helper says about a computer whose Mac was Chris's.
fn status() -> Status {
    Status {
        state: aizome_greeter::setup::State::Needed,
        device: "Mac".into(),
        offer: Offer {
            locales: vec![
                choice("en_US.UTF-8", "English (United States)"),
                choice("en_GB.UTF-8", "English (United Kingdom)"),
                choice("de_DE.UTF-8", "Deutsch (Deutschland)"),
                choice("fr_FR.UTF-8", "Français (France)"),
            ],
            keyboards: vec![
                choice("us", "English (US)"),
                choice("gb", "English (UK)"),
                choice("de", "German"),
                choice("fr", "French"),
            ],
            time_zones: vec![
                zone("America/Chicago", "Chicago", "United States"),
                zone("America/New_York", "New York", "United States"),
                zone("Europe/Berlin", "Berlin", "Germany"),
                zone("Europe/Busingen", "Busingen", "Germany"),
                zone("Europe/London", "London", "Britain (UK)"),
                zone("Asia/Tokyo", "Tokyo", "Japan"),
                zone("UTC", "UTC", "Coordinated Universal Time"),
            ],
            desktops: vec![
                DesktopChoice {
                    id: "aizome".into(),
                    label: "Aizome".into(),
                    description: "Sekka's own desktop: Haiku's window tabs, NeXT's menus and IRIX's Toolchest, in indigo, with a command palette for everything.".into(),
                },
                DesktopChoice {
                    id: "tatami".into(),
                    label: "Tatami".into(),
                    description: "Keyboard-driven Hyprland inspired by Omarchy: a Tokyo Night look, menus for everything and Omarchy's key bindings.".into(),
                },
            ],
        },
        suggested: Draft {
            full_name: "Chris Allen".into(),
            username: "chris".into(),
            hostname: "chriss-macbook-pro".into(),
            locale: "en_US.UTF-8".into(),
            keyboard: "us".into(),
            time_zone: "America/Chicago".into(),
            desktop: "aizome".into(),
        },
        interrupted: None,
    }
}

fn heard(h: &mut Harness<Greeter>, r: Response) {
    h.send(Msg::Setup(SetupMsg::Heard(r)));
}

fn checked(problems: &[(Field, &str)]) -> Response {
    Response::Checked(Checked {
        problems: problems.iter().map(|(f, m)| (*f, (*m).to_owned())).collect(),
        suggested_username: "chris".into(),
        suggested_hostname: "chriss-macbook-pro".into(),
    })
}

fn greeter(light: Light) -> (Harness<Greeter>, WindowKey) {
    let g = Greeter::with(
        config(light),
        State::default(),
        Vec::new(),
        vec![session("aizome", "Aizome"), session("tatami", "Tatami")],
        String::new(),
        false,
        false,
    );
    let mut h = Harness::new(g);
    h.set_outputs(vec![output()]);
    for k in h.windows() {
        h.resize(k, Size::new(W, H));
    }
    if let Some(path) = wallpaper(light) {
        let rgba = aizome_face::wallpaper::prepare(&path, 2880, 1800);
        h.send(Msg::Wallpaper { output: 1, path, size: (2880, 1800), rgba });
    }
    h.send(Msg::Logo(aizome_greeter::setup_view::logo(&logo(), 64.0, 2.0)));
    let w = h.program().main_window().expect("the login's surface");
    (h, w)
}

fn page(h: &Harness<Greeter>) -> Page {
    h.program().setup.as_ref().expect("setup").page
}

fn shows(h: &Harness<Greeter>, w: WindowKey, text: &str) -> bool {
    h.texts(w).iter().any(|t| t.contains(text))
}

fn shot(h: &mut Harness<Greeter>, w: WindowKey, name: &str, light: Light) {
    let suffix = if light == Light::Night { "night" } else { "day" };
    h.save_png(w, shots().join(format!("setup-{name}-{suffix}.png")), 2.0);
}

/// Every page, as a person goes through them with the keyboard.
fn the_whole_setup(light: Light) {
    let (mut h, w) = greeter(light);
    // Nobody has an account: setup, waiting for its helper.
    assert_eq!(h.program().stage, Stage::Setup);
    assert_eq!(page(&h), Page::Connecting);
    assert!(shows(&h, w, "Getting ready"));
    heard(&mut h, Response::Status(status()));
    assert_eq!(page(&h), Page::Welcome);
    assert!(shows(&h, w, "Set up your Mac"), "{:?}", h.texts(w));
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-start"));
    shot(&mut h, w, "1-welcome", light);

    // Region: the language, the keyboard (tried), the time zone searched.
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(page(&h), Page::Region);
    assert!(shows(&h, w, "Language: English (United States)"), "{:?}", h.texts(w));
    assert!(shows(&h, w, "America/Chicago now"), "{:?}", h.texts(w));
    shot(&mut h, w, "2-region", light);
    // The keyboard's menu: German.
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-keyboard"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.popups(w).len(), 1, "the keyboards");
    h.send(Msg::Setup(SetupMsg::Keyboard(2)));
    assert_eq!(h.program().setup.as_ref().unwrap().draft.keyboard, "de");
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-try"));
    h.type_text(w, "Grüße");
    // The zone: typed, ↓ to the second match, Return picks it.
    h.named(w, NamedKey::Tab, Modifiers::NONE);
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-zone"));
    h.type_text(w, "germ");
    assert!(shows(&h, w, "Berlin, Germany") && shows(&h, w, "Busingen, Germany"), "{:?}", h.texts(w));
    shot(&mut h, w, "3-region-search", light);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().setup.as_ref().unwrap().draft.time_zone, "Europe/Berlin");
    // Escape goes back, Return in the empty zone field goes on.
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(page(&h), Page::Welcome);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(page(&h), Page::Region);
    assert_eq!(h.program().setup.as_ref().unwrap().draft.keyboard, "de", "kept going back");
    h.send(Msg::Setup(SetupMsg::Continue));

    // The account: macOS's name suggested; the helper judges each field.
    assert_eq!(page(&h), Page::Account);
    heard(
        &mut h,
        checked(&[(
            Field::Password,
            "Password must have at least 12 characters, at most 1024 bytes, and no control characters.",
        )]),
    );
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-password"), "the first field to fill");
    assert!(shows(&h, w, "At least 12 characters"), "the rule, before it's broken: {:?}", h.texts(w));
    // A user name Genkan won't take, in its words, as it's typed.
    h.send(Msg::Setup(SetupMsg::Username("Chris".into())));
    heard(
        &mut h,
        checked(&[
            (
                Field::Username,
                "Username must start with a lowercase letter and contain only lowercase letters, digits, hyphens or underscores.",
            ),
            (
                Field::Password,
                "Password must have at least 12 characters, at most 1024 bytes, and no control characters.",
            ),
        ]),
    );
    h.type_text(w, "short");
    heard(
        &mut h,
        checked(&[
            (
                Field::Username,
                "Username must start with a lowercase letter and contain only lowercase letters, digits, hyphens or underscores.",
            ),
            (
                Field::Password,
                "Password must have at least 12 characters, at most 1024 bytes, and no control characters.",
            ),
        ]),
    );
    assert!(shows(&h, w, "Username must start with a lowercase letter"));
    assert!(shows(&h, w, "7 more"), "{:?}", h.texts(w));
    assert!(!h.texts(w).iter().any(|t| t.contains("short")), "the password doesn't show");
    // Continue with problems: every one said, the focus at the first.
    h.send(Msg::Setup(SetupMsg::Continue));
    assert_eq!(page(&h), Page::Account);
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-username"));
    assert!(shows(&h, w, "Password must have at least 12 characters"));
    shot(&mut h, w, "4-account-problems", light);
    // Fixed.
    h.send(Msg::Setup(SetupMsg::Username("chris".into())));
    h.send(Msg::Setup(SetupMsg::Password("correct horse battery".into())));
    h.send(Msg::Setup(SetupMsg::Repeat("correct horse batt".into())));
    heard(&mut h, checked(&[]));
    heard(&mut h, checked(&[]));
    assert!(!shows(&h, w, "don't match"), "not before the second is as long");
    h.send(Msg::Setup(SetupMsg::Repeat("correct horse battery".into())));
    assert!(h.program().setup.as_ref().unwrap().account_ready());
    h.send(Msg::Setup(SetupMsg::Continue));
    // Back to see it as it was left.
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(page(&h), Page::Account);
    heard(&mut h, checked(&[]));
    shot(&mut h, w, "5-account", light);
    h.send(Msg::Setup(SetupMsg::Continue));

    // The desktop: ← → choose, Return goes on.
    assert_eq!(page(&h), Page::Desktop);
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-desktop-aizome"));
    shot(&mut h, w, "6-desktop", light);
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.program().setup.as_ref().unwrap().draft.desktop, "tatami");
    h.named(w, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.program().setup.as_ref().unwrap().draft.desktop, "aizome");
    h.named(w, NamedKey::Enter, Modifiers::NONE);

    // Review, then the work.
    assert_eq!(page(&h), Page::Review);
    for want in [
        "Name: Chris Allen",
        "User name: chris",
        "Keyboard: German",
        "Time zone: Berlin, Germany (Europe/Berlin)",
        "Desktop: Aizome",
    ] {
        assert!(shows(&h, w, want), "{want} in {:?}", h.texts(w));
    }
    shot(&mut h, w, "7-review", light);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(page(&h), Page::Working);
    for step in ["check", "account", "settings", "configuration", "verify"] {
        heard(&mut h, Response::Progress(Progress { step: step.into(), index: 0, count: 6 }));
    }
    assert!(shows(&h, w, "Checking the configuration (now)"), "{:?}", h.texts(w));
    assert!(shows(&h, w, "Creating your account (done)"));
    shot(&mut h, w, "8-working", light);

    // It fails once, with the account made: try again, or log in.
    heard(
        &mut h,
        Response::Failed(Failure {
            message: "Setup couldn't check the result: the configuration in /etc/nixos didn't evaluate. Try again. Your account is ready, so you can also log in now and setup finishes the next time it can.".into(),
            account_exists: true,
            can_log_in: true,
            can_retry: true,
        }),
    );
    assert_eq!(page(&h), Page::Failed);
    assert_eq!(h.focused_name(w).as_deref(), Some("setup-retry"));
    assert!(shows(&h, w, "Log In") && shows(&h, w, "Try Again"));
    shot(&mut h, w, "9-failed", light);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(page(&h), Page::Working);

    // Done: the login screen, Chris picked, their desktop theirs.
    heard(
        &mut h,
        Response::Done(Done {
            username: "chris".into(),
            full_name: "Chris Allen".into(),
            uid: 1000,
            desktop: "aizome".into(),
        }),
    );
    assert_eq!(h.program().stage, Stage::Password);
    assert!(h.program().setup.is_none(), "setup, and its secrets, gone");
    assert_eq!(h.program().login.user.as_deref(), Some("chris"));
    assert_eq!(h.program().session().map(|s| s.id.as_str()), Some("aizome"));
    assert_eq!(h.program().state.last_user.as_deref(), Some("chris"));
    assert_eq!(
        h.program().login.message,
        Some(("Your account is ready. Log in with your new password.".into(), Tone::Info))
    );
    h.send(Msg::Greetd(aizome_greeter::greetd::Response::AuthMessage {
        auth_message_type: aizome_greeter::greetd::AuthMessageType::Secret,
        auth_message: "Password: ".into(),
    }));
    assert!(matches!(h.program().login.step, Step::Ask { .. }));
    assert!(shows(&h, w, "Chris Allen"));
    shot(&mut h, w, "10-login", light);
}

#[test]
fn setup_by_night() {
    the_whole_setup(Light::Night);
}

#[test]
fn setup_by_day() {
    the_whole_setup(Light::Day);
}

#[test]
fn an_interrupted_setup_is_finished_and_a_set_up_computer_logs_in() {
    let (mut h, w) = greeter(Light::Night);
    let mut s = status();
    s.state = aizome_greeter::setup::State::Interrupted;
    heard(&mut h, Response::Status(s));
    assert_eq!(page(&h), Page::Working, "finishing what was begun");
    // Complete already (the marker): the login screen as usual.
    let (mut h, _) = greeter(Light::Night);
    let mut s = status();
    s.state = aizome_greeter::setup::State::Complete;
    heard(&mut h, Response::Status(s));
    assert_ne!(h.program().stage, Stage::Setup);
    assert!(h.program().setup.is_none());
    let _ = w;
}

#[test]
fn without_its_helper_setup_says_so_and_keeps_the_power_menu() {
    let (mut h, w) = greeter(Light::Night);
    h.send(Msg::Setup(SetupMsg::Lost("/run/sekka-setup.sock: No such file or directory".into())));
    assert_eq!(page(&h), Page::Unavailable);
    assert!(shows(&h, w, "Setup isn't available"));
    assert!(shows(&h, w, "Power"), "{:?}", h.texts(w));
    h.save_png(w, shots().join("setup-unavailable-night.png"), 2.0);
    h.named(w, NamedKey::F(3), Modifiers::NONE);
    assert_eq!(h.popups(w).len(), 1, "the power menu");
}

#[test]
fn choices_the_helper_refuses_send_the_person_back_to_change_them() {
    let (mut h, w) = greeter(Light::Night);
    heard(&mut h, Response::Status(status()));
    {
        let g = h.program_mut();
        let s = g.setup.as_mut().unwrap();
        s.page = Page::Review;
        s.password = "correct horse battery".into();
        s.repeat = "correct horse battery".into();
    }
    h.send(Msg::Setup(SetupMsg::SetUp));
    heard(
        &mut h,
        Response::Failed(Failure::default_choices(
            "This computer already has an account with that name. Choose another.",
        )),
    );
    assert_eq!(page(&h), Page::Failed);
    assert!(shows(&h, w, "Choose another"));
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(page(&h), Page::Account);
}

trait Choices {
    fn default_choices(m: &str) -> Failure;
}

impl Choices for Failure {
    fn default_choices(m: &str) -> Failure {
        Failure { message: m.into(), account_exists: false, can_log_in: false, can_retry: false }
    }
}

/// Made, without its password yet (it failed to be set): no one can log
/// in to it, so nothing but finishing is offered, and Escape doesn't go
/// back to choices that can't change.
#[test]
fn an_account_without_its_password_offers_only_to_finish() {
    let (mut h, w) = greeter(Light::Night);
    heard(&mut h, Response::Status(status()));
    {
        let g = h.program_mut();
        let s = g.setup.as_mut().unwrap();
        s.page = Page::Review;
        s.password = "correct horse battery".into();
        s.repeat = "correct horse battery".into();
    }
    h.send(Msg::Setup(SetupMsg::SetUp));
    heard(
        &mut h,
        Response::Failed(Failure {
            message: "Setup couldn't make your account: chpasswd took longer than 60 s. Try again: setup finishes the account it began.".into(),
            account_exists: true,
            can_log_in: false,
            can_retry: true,
        }),
    );
    assert_eq!(page(&h), Page::Failed);
    assert!(shows(&h, w, "Try Again"));
    assert!(!h.texts(w).iter().any(|t| t == "Log In" || t == "Back"), "{:?}", h.texts(w));
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(page(&h), Page::Failed);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(page(&h), Page::Working, "trying again");
}
