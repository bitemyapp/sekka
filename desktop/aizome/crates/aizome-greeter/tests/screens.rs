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
    User { name: name.into(), real_name: real.into(), uid, icon: None, tv: None }
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

// Game controllers.

use aizome_gamepad::{Button, Dir, Family, PadEvent, PadInfo};
use aizome_greeter::pads::Spot;
use aizome_greeter::sofa::Target;
use aizome_sys::tv::{Auth, TvAccount};

fn controller(id: u32, family: Family) -> PadInfo {
    PadInfo {
        id,
        slot: id as usize - 1,
        name: "Xbox Wireless Controller".into(),
        family,
        bluetooth: true,
        guid: String::new(),
        battery: Some((80, false)),
        source: aizome_gamepad::mapping::Source::Database,
        rumble: true,
        buttons: Button::all_bits(),
    }
}

fn press(h: &mut Harness<Greeter>, b: Button) {
    for pressed in [true, false] {
        h.send(Msg::Pad(PadEvent::Button { id: 1, slot: 0, button: b, pressed }));
    }
}

fn push(h: &mut Harness<Greeter>, dir: Dir) {
    h.send(Msg::Pad(PadEvent::Nav { id: 1, slot: 0, dir, repeat: false }));
}

fn tv_user(auth: Auth, pin_length: Option<u8>) -> User {
    User {
        name: "tv".into(),
        real_name: "TV".into(),
        uid: 1003,
        icon: None,
        tv: Some(TvAccount { user: "tv".into(), auth, pin_length }),
    }
}

#[test]
fn a_controller_changes_nothing_until_pressed_then_logs_in_by_its_keyboard() {
    let mut h = greeter(State::default(), people());
    let w = main_window(&h);
    let before = h.texts(w);
    // Switched on: nothing shows, nothing moves.
    h.send(Msg::Pad(PadEvent::Connected(controller(1, Family::Xbox))));
    assert!(!h.program().couch.active && h.program().sofa().is_none());
    assert_eq!(h.texts(w), before);
    // The first press shows what it does, and does nothing else: the
    // arrow's press, its direction, its release, as the thread sends them.
    h.send(Msg::Pad(PadEvent::Button { id: 1, slot: 0, button: Button::Right, pressed: true }));
    push(&mut h, Dir::Right);
    h.send(Msg::Pad(PadEvent::Button { id: 1, slot: 0, button: Button::Right, pressed: false }));
    assert!(h.program().couch.active);
    assert_eq!(h.program().selected, 0, "the first touch moved nothing");
    assert_eq!(h.program().couch.spot, Spot::People);
    h.save_png(w, shots().join("greeter-controller-people.png"), 2.0);
    // Then the arrows choose and yes picks: the keyboard comes up.
    push(&mut h, Dir::Right);
    assert_eq!(h.program().selected, 1);
    press(&mut h, Button::South);
    assert_eq!(h.program().stage, Stage::Password);
    asks_password(&mut h);
    assert!(h.program().couch.keys.is_some(), "the keyboard came up by itself");
    // q (where it starts), w, a slip taken back with X, then Start.
    press(&mut h, Button::South);
    push(&mut h, Dir::Right);
    press(&mut h, Button::South);
    press(&mut h, Button::South);
    press(&mut h, Button::West);
    assert_eq!(h.program().password, "qw");
    let scene = h.program().sofa().expect("the keyboard");
    let field = scene.keys.as_ref().and_then(|k| k.field.clone()).expect("its box");
    assert!(field.secret && field.text.is_empty() && field.length == 2, "dots, not the password: {field:?}");
    h.save_png(w, shots().join("greeter-controller-keyboard.png"), 2.0);
    press(&mut h, Button::Start);
    assert!(h.program().password.is_empty(), "sent");
    assert_eq!(h.program().login.step, Step::Waiting);
    // A key typed on a real keyboard: the controller's things go.
    asks_password(&mut h);
    h.type_text(w, "x");
    assert!(!h.program().couch.active && h.program().couch.keys.is_none());
    assert_eq!(h.program().password, "x");
}

#[test]
fn the_pointer_gives_the_screen_back_and_the_corner_button_explains() {
    let mut h = greeter(State::default(), people());
    let w = main_window(&h);
    h.send(Msg::Pad(PadEvent::Connected(controller(1, Family::PlayStation))));
    press(&mut h, Button::South);
    assert!(h.program().couch.active);
    h.send(Msg::Pointer);
    assert!(!h.program().couch.active && h.program().sofa().is_none());
    // The controller button at the top right, clicked: what controllers
    // do here, in the buttons of the one connected.
    assert!(h.texts(w).iter().any(|t| t == "Game controllers"), "{:?}", h.texts(w));
    h.send(Msg::Help);
    let help = h.program().sofa().and_then(|s| s.help).expect("the panel");
    assert_eq!(help.family, Family::PlayStation);
    assert_eq!(help.pads.len(), 1);
    h.save_png(w, shots().join("greeter-controller-help.png"), 2.0);
    h.send(Msg::Sofa(Target::Close));
    assert!(h.program().sofa().is_none());
    // From the controller: up to the top's buttons, yes on the first.
    press(&mut h, Button::South);
    push(&mut h, Dir::Up);
    assert_eq!(h.program().couch.spot, Spot::Controllers);
    press(&mut h, Button::South);
    assert!(h.program().couch.help);
    press(&mut h, Button::East);
    assert!(!h.program().couch.help);
    // Home opens it anywhere; and the power menu is a controller's too.
    press(&mut h, Button::Guide);
    assert!(h.program().couch.help);
    press(&mut h, Button::Guide);
    push(&mut h, Dir::Right);
    assert_eq!(h.program().couch.spot, Spot::Power);
    press(&mut h, Button::South);
    assert_eq!(h.program().couch.power, Some(0));
    push(&mut h, Dir::Down);
    assert_eq!(h.program().couch.power, Some(1));
    h.save_png(w, shots().join("greeter-controller-power.png"), 2.0);
    press(&mut h, Button::East);
    assert_eq!(h.program().couch.power, None);
}

#[test]
fn the_tv_account_with_nothing_to_sign_in_with_starts_when_asked() {
    let mut users = people();
    users.push(tv_user(Auth::None, None));
    let mut state = State::default();
    state.logged_in("tv", "chanoma");
    let mut h = greeter(state, users);
    let w = main_window(&h);
    // Back from the TV: it waits for Start, rather than starting again.
    assert_eq!(h.program().stage, Stage::Password);
    assert!(h.program().tv_start());
    assert_eq!(h.program().login.user, None);
    let texts = h.texts(w);
    assert!(texts.iter().any(|t| t == "Start") && texts.iter().any(|t| t == "Opens Chanoma"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.starts_with("Session:")), "no session to choose");
    assert_eq!(h.program().session().map(|s| s.exec.clone()), Some(Config::default().tv_session));
    h.save_png(w, shots().join("greeter-tv.png"), 2.0);
    h.send(Msg::Pad(PadEvent::Connected(controller(1, Family::Nintendo))));
    press(&mut h, Button::East);
    assert_eq!(h.program().login.user, None, "the first press only wakes it");
    // A, the right-hand button on Nintendo's: Start.
    press(&mut h, Button::East);
    assert_eq!(h.program().login.user.as_deref(), Some("tv"));
    // No question (PAM's nullok): Chanoma's session starts.
    h.send(Msg::Greetd(Response::Success));
    assert_eq!(h.program().login.step, Step::Starting);
    h.send(Msg::Greetd(Response::Success));
    assert_eq!(h.program().login.step, Step::Done);
}

#[test]
fn the_tv_accounts_pin_is_pressed_on_the_pad_and_goes_when_its_long_enough() {
    let mut users = people();
    users.push(tv_user(Auth::Pin, Some(4)));
    let mut h = greeter(State::default(), users);
    let w = main_window(&h);
    h.send(Msg::Pad(PadEvent::Connected(controller(1, Family::Xbox))));
    press(&mut h, Button::South);
    for _ in 0..3 {
        push(&mut h, Dir::Right);
    }
    assert_eq!(h.program().selected, 3, "the TV, last in the row");
    h.save_png(w, shots().join("greeter-controller-tv-tile.png"), 2.0);
    press(&mut h, Button::South);
    asks_password(&mut h);
    assert!(h.program().couch.pin && h.program().couch.keys.is_none(), "the PIN pad, not the keyboard");
    // 2 (Down), 4 (Right), 9 (LT), then 0 (RT): sent at the fourth.
    for b in [Button::Down, Button::Right, Button::L2] {
        press(&mut h, b);
    }
    assert_eq!(h.program().password, "249");
    let pad = h.program().sofa().and_then(|s| s.pin).expect("the pad");
    assert_eq!((pad.typed, pad.length), (3, Some(4)));
    h.save_png(w, shots().join("greeter-controller-tv-pin.png"), 2.0);
    press(&mut h, Button::R2);
    assert!(h.program().password.is_empty(), "sent");
    assert_eq!(h.program().login.step, Step::Waiting);
    // Wrong: it says so on the pad, in a PIN's words.
    h.send(Msg::Greetd(Response::Error {
        error_type: ErrorType::AuthError,
        description: "pam_authenticate: AUTH_ERR".into(),
    }));
    asks_password(&mut h);
    let pad = h.program().sofa().and_then(|s| s.pin).expect("the pad");
    assert_eq!(pad.error.as_deref(), Some("That PIN didn't work. Try again."));
}
