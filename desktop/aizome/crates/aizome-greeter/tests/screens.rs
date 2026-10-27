//! The login screen, headless: what each stage shows and does, and
//! pictures of it (`target/greeter-shots/`) to hold against the lock
//! screen's.

use std::path::{Path, PathBuf};

use aizome_greeter::app::{Greeter, Msg, Stage};
use aizome_greeter::config::{Config, State};
use aizome_greeter::greetd::{AuthMessageType, ErrorType, Response};
use aizome_greeter::login::Step;
use aizome_greeter::sessions::{Kind, Session};
use aizome_greeter::users::User;
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
        make: "SDC".into(),
        model: "ATNA40HQ10-0".into(),
        frame: Rect::new(0.0, 0.0, W, H),
        scale: 2.0,
        integer_scale: 2,
        physical_size: None,
        refresh_hz: 120.0,
        work_area: None,
    }
}

fn session(id: &str, name: &str, kind: Kind) -> Session {
    Session {
        id: id.into(),
        name: name.into(),
        comment: String::new(),
        exec: vec![format!("{id}-session")],
        desktop_names: vec![name.into()],
        kind,
        path: PathBuf::from(format!("/s/{id}.desktop")),
    }
}

fn sessions() -> Vec<Session> {
    vec![
        session("aizome", "Aizome", Kind::Wayland),
        session("gnome", "GNOME", Kind::Wayland),
        session("hyprland-uwsm", "Hyprland (uwsm)", Kind::Wayland),
        session("plasma", "Plasma (Wayland)", Kind::Wayland),
        session("tatami", "Tatami", Kind::Wayland),
        session("xfce", "Xfce Session", Kind::X11),
    ]
}

fn user(name: &str, real: &str, uid: u32) -> User {
    User { name: name.into(), real_name: real.into(), uid, icon: None }
}

fn people() -> Vec<User> {
    vec![user("callen", "Chris Allen", 1000), user("alice", "Alice Kimura", 1001), user("bob", "Bob Tanaka", 1002)]
}

/// The design's night wallpaper, where the design repository is beside.
fn wallpaper() -> Option<PathBuf> {
    let p =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../aizome-design/reference/wallpapers/da-nang-at-night.jpg");
    p.exists().then_some(p)
}

fn config() -> Config {
    Config {
        default_session: Some("aizome".into()),
        x11_prefix: vec!["startx".into(), "/usr/bin/env".into()],
        wallpaper_night: wallpaper(),
        wallpaper_day: wallpaper(),
        ..Config::default()
    }
}

fn greeter(state: State, users: Vec<User>) -> Harness<Greeter> {
    let g = Greeter::with(config(), state, users, sessions(), "ganymede".into(), false, false);
    let mut h = Harness::new(g);
    h.set_outputs(vec![output()]);
    for k in h.windows() {
        h.resize(k, Size::new(W, H));
    }
    // The wallpaper, as the thread would have it.
    if let Some(path) = wallpaper() {
        let rgba = aizome_face::wallpaper::prepare(&path, 2880, 1800);
        h.send(Msg::Wallpaper { output: 1, path, size: (2880, 1800), rgba });
    }
    h.send(Msg::Nothing);
    h
}

fn main_window(h: &Harness<Greeter>) -> WindowKey {
    h.program().main_window().expect("the login's surface")
}

fn shots() -> PathBuf {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("greeter-shots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn asks_password(h: &mut Harness<Greeter>) {
    h.send(Msg::Greetd(Response::AuthMessage {
        auth_message_type: AuthMessageType::Secret,
        auth_message: "Password: ".into(),
    }));
}

#[test]
fn several_people_choose_then_type_their_password() {
    let mut h = greeter(State::default(), people());
    let w = main_window(&h);
    assert_eq!(h.program().stage, Stage::People);
    let texts = h.texts(w);
    for want in ["Chris Allen", "Alice Kimura", "Bob Tanaka", "ganymede"] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
    h.save_png(w, shots().join("greeter-people.png"), 2.0);
    // → moves the selection; Return picks.
    h.named(w, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.program().selected, 1);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().stage, Stage::Password);
    assert_eq!(h.program().login.user.as_deref(), Some("alice"));
    asks_password(&mut h);
    assert_eq!(h.program().login.step, Step::Ask { secret: true, prompt: "Password".into() });
    assert_eq!(h.focused_name(w).as_deref(), Some("greeter-password"));
    assert!(h.texts(w).iter().any(|t| t == "Session: Aizome"), "{:?}", h.texts(w));
    h.save_png(w, shots().join("greeter-password.png"), 2.0);
    // Escape: back to the people, and greetd lets Alice's login go.
    h.named(w, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.program().stage, Stage::People);
    assert_eq!(h.program().login.step, Step::Nobody);
}

#[test]
fn the_last_person_starts_at_their_password_with_their_session() {
    let mut state = State::default();
    state.logged_in("callen", "tatami");
    let mut h = greeter(state, people());
    let w = main_window(&h);
    assert_eq!(h.program().stage, Stage::Password);
    assert_eq!(h.program().login.user.as_deref(), Some("callen"));
    assert_eq!(h.program().session().map(|s| s.id.as_str()), Some("tatami"));
    asks_password(&mut h);
    h.type_text(w, "hunter2");
    assert_eq!(h.program().password, "hunter2");
    assert!(!h.texts(w).iter().any(|t| t.contains("hunter2")), "the password doesn't show");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    // Sent, and gone from the greeter.
    assert!(h.program().password.is_empty());
    assert_eq!(h.program().login.step, Step::Waiting);
    // Wrong: it says so, shakes, and asks again.
    h.send(Msg::Greetd(Response::Error {
        error_type: ErrorType::AuthError,
        description: "pam_authenticate: AUTH_ERR".into(),
    }));
    asks_password(&mut h);
    assert!(h.texts(w).iter().any(|t| t == "That password didn't work. Try again."));
    assert!(h.program().shake > 0);
    h.advance(std::time::Duration::from_millis(400));
    assert_eq!(h.program().shake, 0);
    h.save_png(w, shots().join("greeter-wrong.png"), 2.0);
}

#[test]
fn the_session_menu_lists_every_session_and_the_pick_is_kept() {
    let mut h = greeter(State::default(), vec![user("callen", "Chris Allen", 1000)]);
    let w = main_window(&h);
    // One person: straight to the password.
    assert_eq!(h.program().stage, Stage::Password);
    asks_password(&mut h);
    h.send(Msg::SessionMenu);
    let popups = h.popups(w);
    assert_eq!(popups.len(), 1, "the session menu");
    let menu = h.render_popup(popups[0].0, 2.0);
    let _ = menu;
    // Pick Tatami (sorted: Aizome, GNOME, Hyprland, Plasma, Tatami, Xfce).
    h.send(Msg::PickSession(4));
    assert_eq!(h.program().session().map(|s| s.id.as_str()), Some("tatami"));
    assert!(h.texts(w).iter().any(|t| t == "Session: Tatami"));
    // Logging in starts Tatami, and it's remembered (in memory: a test).
    h.type_text(w, "pw");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    h.send(Msg::Greetd(Response::Success));
    assert_eq!(h.program().login.step, Step::Starting);
    h.send(Msg::Greetd(Response::Success));
    assert_eq!(h.program().login.step, Step::Done);
    assert_eq!(h.program().state.sessions.get("callen").map(String::as_str), Some("tatami"));
    assert_eq!(h.program().state.last_user.as_deref(), Some("callen"));
}

#[test]
fn without_a_list_the_user_name_is_typed() {
    let mut h = greeter(State::default(), Vec::new());
    let w = main_window(&h);
    assert_eq!(h.program().stage, Stage::Name);
    h.save_png(w, shots().join("greeter-name.png"), 2.0);
    h.type_text(w, "alice");
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().login.user.as_deref(), Some("alice"));
    asks_password(&mut h);
    assert!(h.texts(w).iter().any(|t| t == "alice"), "{:?}", h.texts(w));
}

#[test]
fn every_display_shows_the_time_and_only_one_the_login() {
    let mut h = greeter(State::default(), people());
    let mut second = output();
    second.id = 2;
    second.name = "DP-1".into();
    second.frame = Rect::new(1440.0, 0.0, 1440.0 + 1920.0, 1080.0);
    second.scale = 1.0;
    h.set_outputs(vec![output(), second]);
    let windows = h.windows();
    assert_eq!(windows.len(), 2);
    let other = *windows.iter().find(|k| Some(**k) != h.program().main_window()).unwrap();
    h.resize(other, Size::new(1920.0, 1080.0));
    let texts = h.texts(other);
    assert!(!texts.iter().any(|t| t == "Chris Allen"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains(':')), "the time: {texts:?}");
}
